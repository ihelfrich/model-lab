//! Supervised jobs from the shell: start, complete, open the result; quit
//! mid-run and resume from the checkpoint to the identical result.
use model_lab_app::jobs::{engine_job_path_release_only, JobStatus, Jobs};
use model_lab_engine::experiment::{ModelId, RunConfig};
use model_lab_lessons::lesson::{Experiment, Kind, Source};
use model_lab_supervisor::job::JobEvent;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

static BUILD: Once = Once::new();

fn ensure_engine_job() {
    BUILD.call_once(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let status = std::process::Command::new("cargo")
            .args(["build", "-p", "model-lab-engine", "--bin", "model-lab-engine-job"])
            .current_dir(&root)
            .status()
            .expect("cargo build of the engine job");
        assert!(status.success());
        let bin = root.join("target/debug").join(if cfg!(windows) { "model-lab-engine-job.exe" } else { "model-lab-engine-job" });
        assert!(bin.is_file(), "{}", bin.display());
        std::env::set_var("MODEL_LAB_ENGINE_JOB", bin);
    });
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("model-lab-jobs-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_csv(path: &Path, rows: usize) {
    let mut text = String::from("a,b,c,y\n");
    let mut s: u64 = 11;
    for _ in 0..rows {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let a = (s >> 33) as f64 / 2f64.powi(31);
        let b = ((s >> 11) & 0xffff) as f64 / 65536.0;
        let c = (s & 0xffff) as f64 / 65536.0;
        text.push_str(&format!("{a:.6},{b:.6},{c:.6},{:.6}\n", a * b - c + (a * 6.0).sin()));
    }
    std::fs::write(path, text).unwrap();
}

fn experiment() -> Experiment {
    let mut config = RunConfig::default();
    config.model = ModelId::parse("boosting").unwrap();
    config.tree.trees = 40;
    config.tree.depth = 2;
    config.seed = 3;
    Experiment {
        kind: Kind::Run,
        source: Source::Csv(String::new()),
        target: "y".into(),
        features: vec!["a".into(), "b".into(), "c".into()],
        config,
        byhand: None,
        learner_may_change: vec![],
        checkpoint_every_trees: 5,
    }
}

type Events = Arc<Mutex<Vec<(String, JobEvent)>>>;

fn sink() -> (model_lab_app::jobs::Sink, Events, Receiver<(String, JobEvent)>) {
    let events: Events = Arc::new(Mutex::new(vec![]));
    let (tx, rx): (Sender<(String, JobEvent)>, Receiver<(String, JobEvent)>) = channel();
    let e2 = events.clone();
    let sink: model_lab_app::jobs::Sink = Arc::new(move |id: &str, ev: &JobEvent| {
        e2.lock().unwrap().push((id.to_string(), ev.clone()));
        let _ = tx.send((id.to_string(), ev.clone()));
    });
    (sink, events, rx)
}

fn wait_status(jobs: &Jobs, id: &str, wanted: JobStatus, within: Duration) {
    let start = Instant::now();
    while start.elapsed() < within {
        if jobs.manifest(id).map(|m| m.status) == Some(wanted) && !jobs.is_running(id) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("job {id} did not reach {wanted:?}: {:?}", jobs.manifest(id));
}

#[test]
fn start_job_writes_spec_manifest_and_completes() {
    ensure_engine_job();
    let root = tmp("complete");
    let csv = root.join("data.csv");
    write_csv(&csv, 12_000);
    let (sink, events, _rx) = sink();
    let jobs = Jobs::load(&root.join("jobs"), sink);
    let id = jobs.start("boosting-on-data", "test job", &experiment(), &csv, &serde_json::json!({})).unwrap();
    let dir = jobs.job_dir(&id);
    assert!(dir.join("spec.json").is_file());
    assert!(dir.join("manifest.json").is_file());
    wait_status(&jobs, &id, JobStatus::Completed, Duration::from_secs(120));
    assert!(dir.join("result.json").is_file());
    let (run, manifest) = jobs.open_result(&id).unwrap();
    assert_eq!(run.trace.len(), 41);
    assert_eq!(manifest.units_done, 40);
    let ev = events.lock().unwrap();
    assert!(ev.iter().any(|(_, e)| matches!(e, JobEvent::Started { .. })));
    assert!(ev.iter().any(|(_, e)| matches!(e, JobEvent::Completed { .. })));
}

#[test]
fn interrupted_job_resumes_from_its_checkpoint_after_relaunch() {
    ensure_engine_job();
    let root = tmp("resume");
    let csv = root.join("data.csv");
    write_csv(&csv, 12_000);
    let jobs_dir = root.join("jobs");

    // A fresh, uninterrupted run for comparison.
    let fresh_result = {
        let (sink, _e, _rx) = sink();
        let jobs = Jobs::load(&jobs_dir, sink);
        let id = jobs.start("boosting-on-data", "job", &experiment(), &csv, &serde_json::json!({})).unwrap();
        wait_status(&jobs, &id, JobStatus::Completed, Duration::from_secs(120));
        std::fs::read(jobs.job_dir(&id).join("result.json")).unwrap()
    };

    // The interrupted run: quit (drop everything) after the second checkpoint.
    let id = {
        let (sink, _e, rx) = sink();
        let jobs = Jobs::load(&jobs_dir, sink);
        let id = jobs.start("boosting-on-data", "job", &experiment(), &csv, &serde_json::json!({})).unwrap();
        let mut checkpoints = 0;
        let deadline = Instant::now() + Duration::from_secs(120);
        while Instant::now() < deadline {
            if let Ok((_, e)) = rx.recv_timeout(Duration::from_millis(200)) {
                if matches!(e, JobEvent::Checkpoint { .. }) {
                    checkpoints += 1;
                    if checkpoints == 2 {
                        break;
                    }
                }
            }
        }
        assert_eq!(checkpoints, 2, "no second checkpoint within the deadline");
        drop(jobs);
        id
    };

    // Relaunch: the manifest says interrupted at checkpoint 10; resume completes identically.
    let (sink, events, _rx) = sink();
    let jobs = Jobs::load(&jobs_dir, sink);
    let m = jobs.manifest(&id).unwrap();
    assert_eq!(m.status, JobStatus::Interrupted);
    assert_eq!(m.last_checkpoint_stage, Some(10));
    jobs.resume(&id).unwrap();
    wait_status(&jobs, &id, JobStatus::Completed, Duration::from_secs(120));
    let resumed_result = std::fs::read(jobs.job_dir(&id).join("result.json")).unwrap();
    assert_eq!(resumed_result, fresh_result, "resumed result differs from the fresh run");
    // The resumed run started past stage 10: its first Progress event is at least there.
    let first_progress = events.lock().unwrap().iter().find_map(|(_, e)| match e {
        JobEvent::Progress { units_done, .. } => Some(*units_done),
        _ => None,
    });
    assert!(first_progress.is_some_and(|u| u >= 10), "{first_progress:?}");
}

#[test]
fn release_lookup_never_uses_developer_paths() {
    let dir = tmp("release-lookup");
    let err = engine_job_path_release_only(&dir).unwrap_err();
    assert!(err.contains("model-lab-engine-job"), "{err}");
    assert!(err.contains(dir.to_str().unwrap()), "{err}");
    assert!(!err.contains("target/debug"), "{err}");
}

#[test]
fn eta_is_shown_only_as_a_range_with_its_count() {
    ensure_engine_job();
    let root = tmp("eta");
    let csv = root.join("data.csv");
    write_csv(&csv, 12_000);
    let (sink, events, _rx) = sink();
    let jobs = Jobs::load(&root.join("jobs"), sink);
    let id = jobs.start("boosting-on-data", "eta", &experiment(), &csv, &serde_json::json!({})).unwrap();
    wait_status(&jobs, &id, JobStatus::Completed, Duration::from_secs(120));
    let ev = events.lock().unwrap();
    let progress: Vec<_> = ev
        .iter()
        .filter_map(|(_, e)| match e {
            JobEvent::Progress { units_done, eta, .. } => Some((*units_done, eta.clone())),
            _ => None,
        })
        .collect();
    assert!(progress.len() >= 10, "{}", progress.len());
    for (units, eta) in &progress {
        match eta {
            // Three observed intervals need four progress reports.
            None => assert!(*units <= 3, "no ETA at {units} units"),
            Some(e) => {
                assert!(e.low <= e.high);
                assert!(e.units_observed >= 3);
            }
        }
    }
    assert!(progress.iter().any(|(_, e)| e.is_some()));
}
