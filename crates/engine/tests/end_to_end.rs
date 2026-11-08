//! A 50,000-row boosting job under the supervisor: progress, an ETA range,
//! resource readings, a kill after the fourth checkpoint, and a resume that
//! completes with the identical result.
use model_lab_supervisor::job::{JobEvent, JobSpec, Supervisor};
use model_lab_supervisor::limits::Limits;
use model_lab_supervisor::process::Command;
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const ROWS: usize = 50_000;
const TREES: usize = 120;

fn write_large_csv(path: &Path) {
    let mut rng = StdRng::seed_from_u64(20261108);
    let mut text = String::from("x1,x2,x3,x4,x5,x6,x7,x8,y\n");
    for _ in 0..ROWS {
        let x: Vec<f64> = (0..8).map(|_| rng.gen_range(-2.0..2.0)).collect();
        let y = 1.5 * x[0] - 0.8 * x[1] * x[2] + (x[3] * 1.3).sin() + 0.4 * x[4].powi(2) + rng.gen_range(-0.5..0.5);
        for v in &x {
            text.push_str(&format!("{v:.5},"));
        }
        text.push_str(&format!("{y:.5}\n"));
    }
    std::fs::write(path, text).unwrap();
}

fn job(dir: &Path, csv: &Path) -> JobSpec {
    let spec = json!({
        "v": 3, "job": "fit", "label": "large boosting", "data_csv": csv,
        "target": "y", "features": ["x1","x2","x3","x4","x5","x6","x7","x8"],
        "config": {"model": "boosting", "seed": 17, "validation": 0.25, "alpha": 1.0, "mix": 0.5, "standardize": true,
                    "tolerance": 1e-8, "max_iterations": 300, "trees": TREES, "depth": 3, "min_leaf": 5, "rate": 0.1,
                    "fraction": 0.5, "mtry": 1, "patience": 0, "min_relative_improvement": 0.001},
        "max_rows": ROWS, "checkpoint_every_trees": 10
    });
    let spec_path = dir.join("spec.json");
    std::fs::write(&spec_path, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    JobSpec {
        label: "large boosting".into(),
        command: Command {
            program: PathBuf::from(env!("CARGO_BIN_EXE_model-lab-engine-job")),
            args: vec![spec_path.to_string_lossy().into_owned()],
            cwd: dir.to_path_buf(),
            env: vec![],
        },
        limits: Limits { wall: Some(Duration::from_secs(600)), memory_bytes: Some(2 * 1024 * 1024 * 1024), ..Default::default() },
        units_total: Some(TREES as u64),
        sample_interval: Duration::from_millis(500),
        checkpoint: Some(dir.join("checkpoint.json")),
    }
}

fn drain(handle: &model_lab_supervisor::job::JobHandle, cancel_after_checkpoints: Option<usize>) -> Vec<JobEvent> {
    let mut events = vec![];
    let mut checkpoints = 0;
    let deadline = Instant::now() + Duration::from_secs(600);
    while Instant::now() < deadline {
        match handle.events().recv_timeout(Duration::from_millis(500)) {
            Ok(e) => {
                if matches!(e, JobEvent::Checkpoint { .. }) {
                    checkpoints += 1;
                    if Some(checkpoints) == cancel_after_checkpoints {
                        handle.cancel();
                    }
                }
                events.push(e);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(_) => {}
        }
    }
    events
}

fn completed(events: &[JobEvent]) -> Option<(Value, Duration)> {
    events.iter().find_map(|e| match e { JobEvent::Completed { result, wall } => Some((result.clone(), *wall)), _ => None })
}

#[test]
fn large_boosting_job_reports_progress_survives_kill_and_resumes() {
    let dir = std::env::temp_dir().join(format!("mlsup-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("large.csv");
    write_large_csv(&csv);
    let sup = Supervisor::new();

    // Uninterrupted reference run.
    let reference = drain(&sup.start(job(&dir, &csv)).unwrap(), None);
    let (reference_result, reference_wall) = completed(&reference).unwrap_or_else(|| panic!("reference did not complete: {:?}", reference.last()));
    let progress: Vec<_> = reference.iter().filter_map(|e| match e { JobEvent::Progress { units_done, units_total, unit, eta, latest, .. } => Some((*units_done, *units_total, unit.clone(), eta.clone(), latest.clone())), _ => None }).collect();
    assert!(matches!(reference.first(), Some(JobEvent::Started { .. })));
    assert_eq!(progress.len(), TREES);
    assert!(progress.iter().all(|p| p.1 == Some(TREES as u64) && p.2 == "tree"));
    assert!(progress.windows(2).all(|w| w[1].0 > w[0].0), "units never decrease");
    assert!(progress.iter().skip(3).all(|p| p.3.is_some()), "every progress after the third tree has an ETA");
    assert!(progress.iter().any(|p| p.4.as_ref().is_some_and(|s| s.rss_bytes > 0)), "some progress carries a resource sample");
    let eta4 = progress[3].3.as_ref().unwrap();
    assert!(eta4.units_observed >= 3 && eta4.low <= eta4.high, "{eta4:?}");
    std::fs::remove_file(dir.join("checkpoint.json")).unwrap();

    // Killed after the fourth checkpoint.
    let killed = drain(&sup.start(job(&dir, &csv)).unwrap(), Some(4));
    match killed.last() {
        Some(JobEvent::Failed { error, .. }) => assert_eq!(error, "canceled"),
        other => panic!("expected Failed(canceled), got {other:?}"),
    }
    assert!(dir.join("checkpoint.json").exists());

    // Resumed.
    let resumed_started = Instant::now();
    let resumed = drain(&sup.start(job(&dir, &csv)).unwrap(), None);
    let resumed_wall_observed = resumed_started.elapsed();
    let first = resumed.iter().find_map(|e| match e { JobEvent::Progress { units_done, .. } => Some(*units_done), _ => None }).unwrap();
    assert!(first >= 40, "resumed from at least the fourth checkpoint: first progress {first}");
    let (resumed_result, resumed_wall) = completed(&resumed).unwrap_or_else(|| panic!("resume did not complete: {:?}", resumed.last()));
    assert_eq!(resumed_result, reference_result, "resumed result equals the uninterrupted result");
    assert!(resumed_wall < reference_wall, "resume {resumed_wall:?} faster than fresh {reference_wall:?} (observed {resumed_wall_observed:?})");
    eprintln!("reference wall {reference_wall:?}; resumed wall {resumed_wall:?}");
    sup.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}
