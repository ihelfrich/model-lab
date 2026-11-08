//! The supervised engine job binary, driven directly with a nonce.
use model_lab_engine::experiment::{self, Run};
use model_lab_engine::job_spec;
use model_lab_supervisor::protocol::{self, Kind, Line};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("mlengjob-{}-{}", std::process::id(), rand::random::<u64>()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_csv(path: &Path, rows: usize) {
    let data = experiment::scenario(0, 99, 0.5);
    let mut text = format!("{},{}\n", data.names.join(","), data.target);
    for i in 0..rows {
        let r = &data.x[i % data.x.len()];
        text.push_str(&format!("{},{}\n", r.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","), data.y[i % data.y.len()] + (i / data.x.len()) as f64 * 1e-3));
    }
    std::fs::write(path, text).unwrap();
}

fn forest_spec(dir: &Path, trees: usize, seed: u64) -> (PathBuf, PathBuf) {
    let csv = dir.join("data.csv");
    if !csv.exists() {
        write_csv(&csv, 400);
    }
    let data = experiment::scenario(0, 99, 0.5);
    let mut config = experiment::RunConfig::default();
    config.model = experiment::ModelId::Forest;
    config.seed = seed;
    config.tree.trees = trees;
    config.tree.depth = 2;
    let spec = json!({
        "v": 3, "job": "fit", "label": "forest", "data_csv": csv, "target": data.target,
        "features": data.names, "config": config.to_json(), "max_rows": 50000, "checkpoint_every_trees": 10
    });
    let spec_path = dir.join("spec.json");
    std::fs::write(&spec_path, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    (spec_path, dir.join("checkpoint.json"))
}

struct Outcome {
    code: Option<i32>,
    events: Vec<Kind>,
    output: Vec<String>,
}

fn run(spec: &Path, checkpoint: &Path, nonce: Option<&str>, kill_after_checkpoints: Option<usize>) -> Outcome {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_model-lab-engine-job"));
    cmd.arg(spec).stdout(Stdio::piped()).stderr(Stdio::piped()).env_clear();
    if let Some(n) = nonce {
        cmd.env(protocol::NONCE_ENV, n);
    }
    cmd.env("MODEL_LAB_CHECKPOINT", checkpoint);
    let mut child = cmd.spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut events = vec![];
    let mut output = vec![];
    let mut checkpoints = 0;
    use std::io::BufRead;
    for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
        match protocol::parse_line(&line, nonce.unwrap_or("")) {
            Line::Event(e) => {
                if matches!(e.kind, Kind::Checkpoint { .. }) {
                    checkpoints += 1;
                    if Some(checkpoints) == kill_after_checkpoints {
                        #[cfg(unix)]
                        unsafe {
                            libc::kill(child.id() as i32, libc::SIGTERM);
                        }
                        #[cfg(not(unix))]
                        child.kill().unwrap();
                    }
                }
                events.push(e.kind);
            }
            Line::Output(s) => output.push(s),
        }
    }
    let status = child.wait().unwrap();
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    output.extend(err.lines().map(String::from));
    Outcome { code: status.code(), events, output }
}

#[test]
fn missing_nonce_exits_two_with_message() {
    let d = dir();
    let (spec, ck) = forest_spec(&d, 5, 1);
    let out = run(&spec, &ck, None, None);
    assert_eq!(out.code, Some(2), "{:?}", out.output);
    assert!(out.output.iter().any(|l| l.contains("MODEL_LAB_NONCE")), "{:?}", out.output);
}

#[test]
fn fifty_thousand_and_one_rows_is_a_clear_error_not_a_panic() {
    let d = dir();
    write_csv(&d.join("data.csv"), 50_001);
    let (spec, ck) = forest_spec(&d, 5, 1);
    let out = run(&spec, &ck, Some("abc"), None);
    assert_eq!(out.code, Some(1), "{:?}", out.output);
    match out.events.last() {
        Some(Kind::Failed { error }) => assert!(error.contains("50000") || error.contains("50,000"), "{error}"),
        other => panic!("expected Failed, got {other:?} / {:?}", out.output),
    }
    assert!(!out.output.iter().any(|l| l.contains("panicked")), "{:?}", out.output);
}

#[test]
fn forest_job_emits_one_progress_per_tree_and_checkpoints_every_ten() {
    let d = dir();
    let (spec, ck) = forest_spec(&d, 30, 7);
    let out = run(&spec, &ck, Some("abc"), None);
    assert_eq!(out.code, Some(0), "{:?}", out.output);
    let progress = out.events.iter().filter(|e| matches!(e, Kind::Progress { unit, units_total: Some(30), .. } if unit == "tree")).count();
    assert_eq!(progress, 30);
    assert_eq!(out.events.iter().filter(|e| matches!(e, Kind::Checkpoint { .. })).count(), 3);
    let completed: Vec<_> = out.events.iter().filter_map(|e| match e { Kind::Completed { result } => Some(result), _ => None }).collect();
    assert_eq!(completed.len(), 1);
    let run = Run::from_json(completed[0], Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(run.last_stage(), 30);
}

#[test]
fn resumed_job_completes_identically() {
    let d = dir();
    let (spec, ck) = forest_spec(&d, 60, 3);
    let whole = run(&spec, &ck, Some("abc"), None);
    assert_eq!(whole.code, Some(0), "{:?}", whole.output);
    let reference = whole.events.iter().find_map(|e| match e { Kind::Completed { result } => Some(result.clone()), _ => None }).unwrap();
    std::fs::remove_file(&ck).unwrap();

    let killed = run(&spec, &ck, Some("abc"), Some(2));
    assert_ne!(killed.code, Some(0), "{:?}", killed.output);
    assert!(ck.exists(), "checkpoint survives the kill");
    let resumed = run(&spec, &ck, Some("abc"), None);
    assert_eq!(resumed.code, Some(0), "{:?}", resumed.output);
    let first_progress = resumed.events.iter().find_map(|e| match e { Kind::Progress { units_done, .. } => Some(*units_done), _ => None }).unwrap();
    assert!(first_progress >= 20, "resumed from the second checkpoint, not from zero: {first_progress}");
    let result = resumed.events.iter().find_map(|e| match e { Kind::Completed { result } => Some(result.clone()), _ => None }).unwrap();
    assert_eq!(result, reference);
}

#[test]
fn spec_or_data_mismatch_refuses_the_checkpoint() {
    let d = dir();
    let (spec, ck) = forest_spec(&d, 30, 5);
    let out = run(&spec, &ck, Some("abc"), None);
    assert_eq!(out.code, Some(0), "{:?}", out.output);
    let (spec2, _) = forest_spec(&d, 30, 6);
    let out2 = run(&spec2, &ck, Some("abc"), None);
    assert_eq!(out2.code, Some(1), "{:?}", out2.output);
    match out2.events.last() {
        Some(Kind::Failed { error }) => assert!(error.contains("checkpoint") && error.contains("spec"), "{error}"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn spec_file_is_validated() {
    let d = dir();
    let bad = d.join("bad.json");
    std::fs::write(&bad, serde_json::to_vec(&json!({"v": 2, "job": "fit"})).unwrap()).unwrap();
    assert!(job_spec::load(&bad).unwrap_err().contains("v"));
    std::fs::write(&bad, serde_json::to_vec(&json!({"v": 3, "job": "fit", "label": "x", "data_csv": "d.csv", "target": "y", "features": ["a"], "config": Value::Null, "max_rows": 10, "checkpoint_every_trees": 0})).unwrap()).unwrap();
    assert!(job_spec::load(&bad).unwrap_err().contains("checkpoint_every_trees"));
}
