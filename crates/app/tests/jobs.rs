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

/// The Phase 1 "done" job at the shell's job layer: 50,000 rows, 120 trees of
/// depth 3, checkpoints every 10 trees, a quit after the fifth checkpoint and
/// a resume to the identical result. Minutes long; run on demand with
/// `cargo test -p model-lab-app --test jobs -- --ignored walkthrough` (debug profile, which honours the override).
/// Set MODEL_LAB_ENGINE_JOB to the release sidecar to exercise the shipped binary.
#[test]
#[ignore]
fn walkthrough_fifty_thousand_row_job_quits_and_resumes() {
    if std::env::var("MODEL_LAB_ENGINE_JOB").is_err() {
        ensure_engine_job();
    }
    let root = tmp("walkthrough");
    let csv = root.join("income-50k.csv");
    {
        // The income mechanism of examples/generate.py, 50,000 rows.
        let mut s: u64 = 20261109;
        let mut next = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut text = String::from("age,year,income\n");
        for _ in 0..50_000 {
            let age = 18.0 + (next() * 53.0).floor();
            let year = 2000.0 + (next() * 13.0).floor();
            let mean = 30.0 + 2.6 * (age - 18.0) - 0.038 * (age - 18.0).powi(2) + 1.1 * (year - 2000.0);
            let sd = 6.0 + 0.18 * (age - 18.0);
            let (u1, u2) = (next().max(1e-12), next());
            let z = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
            text.push_str(&format!("{age},{year},{:.6}\n", (mean + sd * z).max(5.0)));
        }
        std::fs::write(&csv, text).unwrap();
    }
    let mut exp = experiment();
    exp.target = "income".into();
    exp.features = vec!["age".into(), "year".into()];
    exp.config.tree.trees = 120;
    exp.config.tree.depth = 3;
    exp.checkpoint_every_trees = 10;
    let jobs_dir = root.join("jobs");
    let started = Instant::now();
    let fresh = {
        let (sink, _e, _rx) = sink();
        let jobs = Jobs::load(&jobs_dir, sink);
        let id = jobs.start("boosting-on-data", "walkthrough", &exp, &csv, &serde_json::json!({})).unwrap();
        wait_status(&jobs, &id, JobStatus::Completed, Duration::from_secs(900));
        std::fs::read(jobs.job_dir(&id).join("result.json")).unwrap()
    };
    let fresh_secs = started.elapsed().as_secs();
    let id = {
        let (sink, _e, rx) = sink();
        let jobs = Jobs::load(&jobs_dir, sink);
        let id = jobs.start("boosting-on-data", "walkthrough", &exp, &csv, &serde_json::json!({})).unwrap();
        let mut checkpoints = 0;
        let mut etas = 0;
        let deadline = Instant::now() + Duration::from_secs(900);
        while Instant::now() < deadline {
            if let Ok((_, e)) = rx.recv_timeout(Duration::from_millis(200)) {
                match e {
                    JobEvent::Checkpoint { .. } => {
                        checkpoints += 1;
                        if checkpoints == 5 {
                            break;
                        }
                    }
                    JobEvent::Progress { eta: Some(_), latest: Some(_), .. } => etas += 1,
                    _ => {}
                }
            }
        }
        assert_eq!(checkpoints, 5);
        assert!(etas > 0, "no progress event carried an ETA range and a resource sample");
        drop(jobs);
        id
    };
    assert_eq!(std::process::Command::new("pgrep").arg("-f").arg("model-lab-engine-job").output().map(|o| o.stdout.is_empty()).unwrap_or(true), true, "an engine job survived the quit");
    let resumed_at = Instant::now();
    let (sink, _e, _rx) = sink();
    let jobs = Jobs::load(&jobs_dir, sink);
    let m = jobs.manifest(&id).unwrap();
    assert_eq!(m.status, JobStatus::Interrupted);
    assert_eq!(m.last_checkpoint_stage, Some(50));
    jobs.resume(&id).unwrap();
    wait_status(&jobs, &id, JobStatus::Completed, Duration::from_secs(900));
    let resumed = std::fs::read(jobs.job_dir(&id).join("result.json")).unwrap();
    assert_eq!(resumed, fresh);
    let (run, _) = jobs.open_result(&id).unwrap();
    assert_eq!(run.trace.len(), 121);
    eprintln!("walkthrough: fresh {fresh_secs}s, resume from 50 {}s", resumed_at.elapsed().as_secs());
}
