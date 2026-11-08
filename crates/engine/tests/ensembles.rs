//! Stage-wise ensembles: equivalence with `model::fit`, exact resume, per-tree
//! randomness, and agreement with recorded fixtures.
use model_lab_engine::data::Data;
use model_lab_engine::experiment;
use model_lab_engine::model::{self, Config, Method};
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn scenario(i: usize) -> Data {
    experiment::scenario(i, 4710 + i as u64, 0.8)
}

fn config(method: Method, depth: usize) -> Config {
    Config { method, trees: 12, depth, min_leaf: 2, rate: 0.3, fraction: 0.6, mtry: 1, validation: 0.25, seed: 11, patience: 0, min_relative_improvement: 0.001 }
}

fn method_name(m: Method) -> &'static str {
    match m {
        Method::Tree => "tree",
        Method::Boosting => "boosting",
        Method::Stochastic => "stochastic",
        Method::Forest => "forest",
    }
}

fn case(fit: &model::Fit, scenario_index: usize) -> Value {
    let last = fit.trees.len();
    json!({
        "scenario": scenario_index,
        "method": method_name(fit.cfg.method),
        "depth": fit.cfg.depth,
        "trees_fitted": last,
        "final_train_mse": fit.train_mse[last],
        "final_valid_mse": fit.valid_mse[last],
        "importance": fit.importance,
        "predictions_head": fit.predictions(last)[..5].to_vec(),
        "first_sample_head": fit.samples[0][..8].to_vec(),
    })
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn write_fixture(name: &str, methods: &[Method]) {
    let mut cases = vec![];
    for i in 0..5 {
        for &m in methods {
            for depth in [2, 4] {
                let fit = model::fit(scenario(i), config(m, depth), Arc::new(AtomicBool::new(false))).unwrap();
                cases.push(case(&fit, i));
            }
        }
    }
    std::fs::write(fixture_path(name), serde_json::to_string_pretty(&json!({"cases": cases})).unwrap()).unwrap();
}

/// Regenerates the fixtures. `ensembles-v1-stable.json` was written once from
/// the 0.8 code (single trees and full-data boosting, which the per-tree RNG
/// change leaves untouched) and must not be regenerated; `ensembles-v2.json`
/// records forests and stochastic boosting under engine 0.9.
#[test]
#[ignore]
fn write_fixtures() {
    match std::env::var("FIXTURE").as_deref() {
        Ok("v1") => write_fixture("ensembles-v1-stable.json", &[Method::Tree, Method::Boosting]),
        Ok("v2") => write_fixture("ensembles-v2.json", &[Method::Forest, Method::Stochastic]),
        _ => panic!("set FIXTURE=v1 or FIXTURE=v2"),
    }
}

fn check_fixture(name: &str) {
    let text = std::fs::read_to_string(fixture_path(name)).unwrap();
    let fixture: Value = serde_json::from_str(&text).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 20, "{name} has 20 cases");
    for expected in cases {
        let method = match expected["method"].as_str().unwrap() {
            "tree" => Method::Tree,
            "boosting" => Method::Boosting,
            "stochastic" => Method::Stochastic,
            "forest" => Method::Forest,
            other => panic!("{other}"),
        };
        let i = expected["scenario"].as_u64().unwrap() as usize;
        let fit = model::fit(scenario(i), config(method, expected["depth"].as_u64().unwrap() as usize), Arc::new(AtomicBool::new(false))).unwrap();
        let actual = case(&fit, i);
        assert_eq!(actual["trees_fitted"], expected["trees_fitted"], "{name} {expected}");
        assert_eq!(actual["first_sample_head"], expected["first_sample_head"], "{name} {expected}");
        for key in ["final_train_mse", "final_valid_mse"] {
            let (a, e) = (actual[key].as_f64().unwrap(), expected[key].as_f64().unwrap());
            assert!((a - e).abs() <= 1e-12 * e.abs().max(1.), "{name} {key}: {a} vs {e} in {expected}");
        }
        for key in ["importance", "predictions_head"] {
            let a: Vec<f64> = serde_json::from_value(actual[key].clone()).unwrap();
            let e: Vec<f64> = serde_json::from_value(expected[key].clone()).unwrap();
            assert_eq!(a.len(), e.len());
            for (x, y) in a.iter().zip(&e) {
                assert!((x - y).abs() <= 1e-12 * y.abs().max(1.), "{name} {key}: {x} vs {y} in {expected}");
            }
        }
    }
}

#[test]
fn single_tree_and_full_boosting_match_0_8_fixtures() {
    check_fixture("ensembles-v1-stable.json");
}

use model_lab_engine::ensemble::EnsembleState;

fn step_to_end(mut state: EnsembleState) -> model::Fit {
    while !state.finished() {
        state.step().unwrap();
    }
    state.into_fit()
}

#[test]
fn stepping_to_the_end_equals_fit() {
    for method in Method::ALL {
        let cfg = config(method, 3);
        let direct = model::fit(scenario(1), cfg.clone(), Arc::new(AtomicBool::new(false))).unwrap();
        let stepped = step_to_end(EnsembleState::new(scenario(1), cfg).unwrap());
        assert_eq!(direct.trees.len(), stepped.trees.len(), "{method:?}");
        assert_eq!(direct.train_mse, stepped.train_mse, "{method:?}");
        assert_eq!(direct.valid_mse, stepped.valid_mse, "{method:?}");
        assert_eq!(direct.importance, stepped.importance, "{method:?}");
        assert_eq!(direct.stage_predictions, stepped.stage_predictions, "{method:?}");
        assert_eq!(direct.samples, stepped.samples, "{method:?}");
    }
}

#[test]
fn resume_from_json_mid_way_is_bit_identical() {
    let cfg = Config { trees: 40, ..config(Method::Forest, 3) };
    let uninterrupted = step_to_end(EnsembleState::new(scenario(2), cfg.clone()).unwrap());
    let mut partial = EnsembleState::new(scenario(2), cfg).unwrap();
    for _ in 0..17 {
        partial.step().unwrap();
    }
    assert_eq!(partial.stage(), 17);
    let wire = serde_json::to_string(&partial.to_json()).unwrap();
    let restored = EnsembleState::from_json(&serde_json::from_str(&wire).unwrap(), scenario(2)).unwrap();
    assert_eq!(restored.stage(), 17);
    let resumed = step_to_end(restored);
    assert_eq!(resumed.trees.len(), 40);
    assert_eq!(resumed.stage_predictions, uninterrupted.stage_predictions);
    assert_eq!(resumed.oob_mse, uninterrupted.oob_mse);
    assert_eq!(resumed.importance, uninterrupted.importance);
}

#[test]
fn tree_depth_change_does_not_change_bootstrap_samples() {
    let shallow = step_to_end(EnsembleState::new(scenario(3), config(Method::Forest, 2)).unwrap());
    let deep = step_to_end(EnsembleState::new(scenario(3), config(Method::Forest, 5)).unwrap());
    assert_eq!(shallow.samples.len(), 12);
    for t in 0..12 {
        assert_eq!(shallow.samples[t], deep.samples[t], "tree {t}");
    }
}

#[test]
fn forest_matches_regenerated_fixture() {
    check_fixture("ensembles-v2.json");
}
