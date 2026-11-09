//! A saved trace of a tree model carries its ensemble state and opens
//! read-only with `Run::from_trace`, which replays the stored trees instead
//! of refitting and honours a caller-supplied row bound.
use model_lab_engine::data::Data;
use model_lab_engine::experiment::{ModelId, Run, RunConfig};
use model_lab_engine::linear::LinearMethod;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::sync::{atomic::AtomicBool, Arc};

fn data(rows: usize, seed: u64) -> Data {
    let mut rng = StdRng::seed_from_u64(seed);
    let x: Vec<Vec<f64>> = (0..rows)
        .map(|_| (0..3).map(|_| rng.gen_range(-2.0..2.0)).collect::<Vec<_>>())
        .collect();
    let y = x
        .iter()
        .map(|r| r[0] * 1.5 - r[1] * r[1] + 0.3 * r[2] + rng.gen_range(-0.2..0.2))
        .collect();
    Data {
        names: vec!["a".into(), "b".into(), "c".into()],
        target: "y".into(),
        x,
        y,
    }
}

fn failure(result: Result<Run, String>) -> String {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(e) => e,
    }
}

fn cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn big_boosting_run() -> Run {
    let mut cfg = RunConfig::default();
    cfg.model = ModelId::parse("boosting").unwrap();
    cfg.seed = 7;
    cfg.tree.trees = 20;
    cfg.tree.depth = 2;
    Run::fit_with_limit("big".into(), data(6000, 7), cfg, cancel(), 50_000).unwrap()
}

#[test]
fn to_json_carries_the_ensemble_state() {
    let json = big_boosting_run().to_json();
    assert_eq!(json["ensemble"]["v"].as_u64(), Some(2));
    assert_eq!(json["ensemble"]["trees"].as_array().unwrap().len(), 20);
}

#[test]
fn from_trace_rebuilds_without_refitting() {
    let original = big_boosting_run();
    let json = original.to_json();
    let restored = Run::from_trace(&json, 50_000).unwrap();
    assert_eq!(restored.name, original.name);
    assert_eq!(restored.config, original.config);
    assert_eq!(restored.predictions(20), original.predictions(20));
    assert_eq!(restored.trace.len(), original.trace.len());
    for (a, b) in restored.trace.iter().zip(&original.trace) {
        assert_eq!(a.train_loss, b.train_loss);
        assert_eq!(a.valid_loss, b.valid_loss);
        assert_eq!(a.label, b.label);
    }
}

#[test]
fn from_json_still_rejects_above_interactive_rows() {
    let json = big_boosting_run().to_json();
    let err = failure(Run::from_json(&json, cancel()));
    assert!(err.contains("teaching-workspace limits"), "{err}");
}

#[test]
fn from_trace_honours_the_row_bound() {
    let json = big_boosting_run().to_json();
    let err = failure(Run::from_trace(&json, 5000));
    assert!(err.contains("5000"), "{err}");
}

#[test]
fn from_trace_rejects_tampered_losses() {
    let mut json = big_boosting_run().to_json();
    let v = json["trace"][5]["train_loss"].as_f64().unwrap();
    json["trace"][5]["train_loss"] = serde_json::json!(v + 1.0);
    let err = failure(Run::from_trace(&json, 50_000));
    assert!(err.contains("does not reproduce"), "{err}");
}

#[test]
fn from_trace_rejects_linear_and_missing_state() {
    let mut cfg = RunConfig::default();
    cfg.model = ModelId::Linear(LinearMethod::Ols);
    let ols = Run::fit("ols".into(), data(200, 3), cfg, cancel()).unwrap();
    let err = failure(Run::from_trace(&ols.to_json(), 50_000));
    assert!(err.contains("refitting"), "{err}");

    let mut json = big_boosting_run().to_json();
    json.as_object_mut().unwrap().remove("ensemble");
    let err = failure(Run::from_trace(&json, 50_000));
    assert!(err.contains("no ensemble state"), "{err}");
}
