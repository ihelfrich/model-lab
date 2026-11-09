//! Experiments behind the Run panel: resolving a lesson's data source,
//! applying the learner's changes within the lesson's bounds, fitting on a
//! thread with a cancel flag, and turning a result into the live context,
//! the plots and the observation arithmetic.
use model_lab_engine::byhand::{boosting_rounds, candidate_splits, Candidate, Round};
use model_lab_engine::data::{Data, Table};
use model_lab_engine::experiment::{FittedModel, Run, RunConfig};
use model_lab_engine::model::Fit;
use model_lab_engine::science::{analyze, Analysis};
use model_lab_lessons::examples;
use model_lab_lessons::lesson::{Experiment, Kind, Learner, Source};
use model_lab_lessons::plots::{self, Plot};
use model_lab_lessons::template::Context;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// What a finished experiment is.
pub enum RunView {
    Fitted {
        run: Run,
        /// Permutation importance, PDP and ICE at the last stage (forests).
        analysis: Option<Analysis>,
        /// A forest fitted once more with every predictor offered at every
        /// split (`mtry = p`), same seed: a real second fit for the lesson's
        /// comparison, `None` unless the run is a forest with `mtry < p`.
        mtry_full: Option<Box<Fit>>,
    },
    ByHand {
        data: Data,
        min_leaf: usize,
        candidates: Vec<Candidate>,
        rounds: Vec<Round>,
    },
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub enum Status {
    Running,
    Done,
    Failed(String),
    Canceled,
}

/// Row data for the arithmetic panel: `old + rate × h = new` for boosting,
/// the new tree's vote for a forest, both read from the real recorded stages.
#[derive(Clone, Debug, Serialize)]
pub struct Observation {
    pub row: usize,
    pub x: Vec<(String, f64)>,
    pub y: f64,
    pub before: f64,
    pub h: Option<f64>,
    pub rate: Option<f64>,
    pub after: f64,
    pub residual_before: f64,
    pub residual_after: f64,
    pub rules: Vec<String>,
    pub in_training: bool,
    /// Whether this row was in the sample the stage's tree was fitted to
    /// (stochastic boosting and forests); `None` before the first tree.
    pub in_sample: Option<bool>,
}

pub fn four_points() -> Data {
    Data {
        names: vec!["x".into()],
        target: "y".into(),
        x: vec![vec![1.], vec![2.], vec![3.], vec![4.]],
        y: vec![2., 4., 8., 10.],
    }
}

/// Resolve the lesson's data source. `max_rows` bounds a CSV source.
pub fn resolve_data(exp: &Experiment, max_rows: usize) -> Result<Data, String> {
    let table = match &exp.source {
        Source::FourPoint => return Ok(four_points()),
        Source::Example(name) => {
            let csv = examples::csv(name).ok_or_else(|| format!("No bundled example named {name:?}"))?;
            Table::parse_with_limit(name, csv, max_rows)?
        }
        Source::Csv(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            Table::parse_with_limit(path, &text, max_rows)?
        }
    };
    let index = |name: &str| {
        table
            .headers
            .iter()
            .position(|h| h == name)
            .ok_or_else(|| format!("Column {name:?} is not in {}", table.name))
    };
    let target = index(&exp.target)?;
    let features = exp.features.iter().map(|f| index(f)).collect::<Result<Vec<_>, _>>()?;
    table.dataset(target, &features)
}

/// Apply the learner's overrides: only fields the lesson allows, each
/// clamped to its range. Anything else is ignored.
pub fn apply_overrides(config: &RunConfig, allowed: &[Learner], overrides: &Value) -> Result<RunConfig, String> {
    let mut merged = config.to_json();
    if let Some(map) = overrides.as_object() {
        for (key, value) in map {
            let Some(rule) = allowed.iter().find(|l| &l.field == key) else { continue };
            let Some(v) = value.as_f64() else { continue };
            let clamped = v.clamp(rule.min, rule.max);
            let integer = matches!(merged[key], Value::Number(ref n) if n.is_u64() || n.is_i64());
            merged[key] = if integer { json!(clamped.round() as u64) } else { json!(clamped) };
        }
    }
    // A sample fraction below one is stochastic boosting; back at one it is
    // ordinary boosting again. The model name follows the slider.
    if let Some(fraction) = merged["fraction"].as_f64() {
        match merged["model"].as_str() {
            Some("boosting") if fraction < 1.0 => merged["model"] = json!("stochastic"),
            Some("stochastic") if fraction >= 1.0 => merged["model"] = json!("boosting"),
            _ => {}
        }
    }
    RunConfig::from_json(&merged)
}

/// Identity of an attempt: the lesson, the exact settings and the data.
pub fn experiment_hash(lesson_id: &str, config: &RunConfig, data_fingerprint: &str) -> String {
    let mut h = Sha256::new();
    h.update(lesson_id.as_bytes());
    h.update(b"\n");
    h.update(config.to_json().to_string().as_bytes());
    h.update(b"\n");
    h.update(data_fingerprint.as_bytes());
    format!("{:x}", h.finalize())
}

/// A column name as a template key: `med.income` → `med_income`, because
/// template paths are dotted.
pub fn key(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

fn fit_of(run: &Run) -> Option<&Fit> {
    match &run.fitted {
        FittedModel::Trees(f) => Some(f),
        FittedModel::Linear(_) => None,
    }
}

/// Build the view for a lesson experiment with the learner's overrides.
/// In-process fits are bounded by `max_rows` (the interactive bound).
pub fn build_view(
    lesson_id: &str,
    exp: &Experiment,
    overrides: &Value,
    cancel: Arc<AtomicBool>,
    max_rows: usize,
) -> Result<(RunView, String), String> {
    let config = apply_overrides(&exp.config, &exp.learner_may_change, overrides)?;
    match exp.kind {
        Kind::ByHand => {
            let data = four_points();
            let settings = exp.byhand.as_ref().ok_or("by-hand lesson without settings")?;
            let allowed = |f: &str| exp.learner_may_change.iter().any(|l| l.field == f);
            let rate = if allowed("rate") { config.tree.rate } else { settings.rate };
            let depth = if allowed("depth") { config.tree.depth } else { settings.depth };
            let rounds_n = if allowed("trees") { config.tree.trees.min(50) } else { settings.rounds };
            let min_leaf = if allowed("min_leaf") { config.tree.min_leaf } else { 1 };
            let rows: Vec<usize> = (0..data.y.len()).collect();
            let candidates = candidate_splits(&data, &rows, 0);
            let rounds = boosting_rounds(&data, rate, depth, rounds_n)?;
            let hash = experiment_hash(lesson_id, &config, &model_lab_engine::experiment::data_fingerprint(&data));
            Ok((RunView::ByHand { data, min_leaf, candidates, rounds }, hash))
        }
        Kind::Run => {
            let data = resolve_data(exp, max_rows)?;
            if data.y.len() > max_rows {
                return Err(format!(
                    "This data has {} rows; use a supervised job for more than {max_rows} rows.",
                    data.y.len()
                ));
            }
            let run = Run::fit_with_limit(lesson_id.to_string(), data, config.clone(), cancel, max_rows)?;
            let hash = experiment_hash(lesson_id, &config, &run.fingerprint);
            let analysis = fit_of(&run)
                .filter(|f| f.cfg.method == model_lab_engine::model::Method::Forest)
                .map(|f| analyze(f, 0, f.trees.len()));
            let mtry_full = mtry_comparison(&run)?;
            Ok((RunView::Fitted { run, analysis, mtry_full }, hash))
        }
    }
}

/// A fitted `Run` opened from a saved trace (a supervised job's result).
pub fn view_of_run(run: Run) -> RunView {
    let analysis = fit_of(&run)
        .filter(|f| f.cfg.method == model_lab_engine::model::Method::Forest)
        .map(|f| analyze(f, 0, f.trees.len()));
    // A supervised result is opened read-only; no second fit is run for it.
    RunView::Fitted { run, analysis, mtry_full: None }
}

/// For a forest that hides predictors at each split, fit it again with all
/// of them offered (`mtry = p`), same seed and data, so the lesson can show
/// a measured rather than asserted difference.
fn mtry_comparison(run: &Run) -> Result<Option<Box<Fit>>, String> {
    let Some(f) = fit_of(run) else { return Ok(None) };
    let p = run.data.names.len();
    if f.cfg.method != model_lab_engine::model::Method::Forest || f.cfg.mtry >= p {
        return Ok(None);
    }
    let mut cfg = f.cfg.clone();
    cfg.mtry = p;
    let full = model_lab_engine::model::fit(run.data.clone(), cfg, Arc::new(AtomicBool::new(false)))?;
    Ok(Some(Box::new(full)))
}

pub fn stages(view: &RunView) -> usize {
    match view {
        RunView::Fitted { run, .. } => run.trace.len(),
        RunView::ByHand { rounds, .. } => rounds.len() + 1,
    }
}

pub fn best_stage(view: &RunView) -> Option<usize> {
    match view {
        RunView::Fitted { run, .. } => run
            .trace
            .iter()
            .min_by(|a, b| a.valid_loss.total_cmp(&b.valid_loss))
            .map(|s| s.stage),
        RunView::ByHand { .. } => None,
    }
}

fn split_of(round: &Round) -> (Option<f64>, Option<f64>, Option<f64>) {
    match &round.tree.split {
        Some((_, t, l, r)) => (Some(*t), Some(l.value), Some(r.value)),
        None => (None, None, None),
    }
}

/// The live quantities a lesson's text can refer to. Names are fixed by the
/// plan (Task 4); `stage` is an object so `stage.current` and `stage.best`
/// can coexist.
pub fn context(view: &RunView, stage: usize, selected: Option<usize>) -> Context {
    match view {
        RunView::ByHand { data, min_leaf, candidates, rounds } => {
            let mean = data.y.iter().sum::<f64>() / data.y.len() as f64;
            let sse: f64 = data.y.iter().map(|y| (y - mean).powi(2)).sum();
            let rounds_json: Vec<Value> = rounds
                .iter()
                .map(|r| {
                    let (threshold, left, right) = split_of(r);
                    json!({
                        "stage": r.stage, "mse_before": r.mse_before, "mse_after": r.mse_after,
                        "threshold": threshold, "left_mean": left, "right_mean": right,
                        "before": r.before, "after": r.after, "correction": r.correction,
                        "residual_before": r.residual_before, "residual_after": r.residual_after,
                    })
                })
                .collect();
            let cands: Vec<Value> = candidates
                .iter()
                .map(|c| {
                    json!({
                        "threshold": c.threshold, "reduction": c.reduction, "sse_after": c.sse_after,
                        "left_mean": c.left_mean, "right_mean": c.right_mean,
                        "left_n": c.left.len(), "right_n": c.right.len(),
                        "admissible": c.left.len() >= *min_leaf && c.right.len() >= *min_leaf,
                    })
                })
                .collect();
            let mut ctx = json!({
                "data": {"n": data.y.len(), "target": data.target},
                "stage": {"current": stage, "last": rounds.len()},
                "byhand": {"mean": mean, "sse": sse, "mse": sse / data.y.len() as f64, "round": rounds_json, "candidates": cands,
                           "rate": byhand_rate(rounds, data)},
            });
            if let Some(obs) = observation(view, stage, selected) {
                ctx["selected"] = observation_json(&obs);
            }
            Context(ctx)
        }
        RunView::Fitted { run, analysis, mtry_full } => {
            let stage = stage.min(run.last_stage());
            let (train, valid) = run.scores(stage, 0.5);
            let best = best_stage(view).unwrap_or(0);
            let best_valid = run.trace.get(best).map(|s| s.valid_loss);
            let last_valid = run.trace.last().map(|s| s.valid_loss);
            let last_minus_best = match (last_valid, best_valid) {
                (Some(l), Some(b)) => Some((l - b).max(0.0)),
                _ => None,
            };
            let mut ctx = json!({
                "data": {"n": run.data.y.len(), "target": run.data.target},
                "rows": {"train": run.partition.train.len(), "valid": run.partition.valid.len()},
                "config": run.config.to_json(),
                "stage": {"current": stage, "best": best, "last": run.last_stage()},
                "metric": {
                    "train_mse": train.loss, "valid_mse": valid.loss,
                    "train_rmse": train.rmse, "valid_rmse": valid.rmse,
                    "best_valid_mse": best_valid,
                    "valid_mse_at_last_minus_best": last_minus_best,
                },
            });
            if let Some((intercept, coefs)) = run.coefficients(stage) {
                let mut c = serde_json::Map::new();
                c.insert("intercept".into(), json!(intercept));
                for (name, b) in run.data.names.iter().zip(coefs) {
                    c.insert(key(name), json!(b));
                }
                ctx["coef"] = Value::Object(c);
            }
            if let Some(f) = fit_of(run) {
                if f.cfg.method == model_lab_engine::model::Method::Forest {
                    let s = stage.min(f.oob_mse.len() - 1);
                    let delta = mtry_full.as_ref().and_then(|full| match (full.oob_mse.last().copied().flatten(), f.oob_mse.last().copied().flatten()) {
                        (Some(a), Some(b)) => Some(a - b),
                        _ => None,
                    });
                    ctx["oob"] = json!({"mse": f.oob_mse[s], "coverage": f.oob_coverage[s], "rows_train": f.train.len(),
                                        "mse_delta_mtry_full": delta, "mtry_full": mtry_full.as_ref().map(|full| full.cfg.mtry)});
                }
            }
            if let Some(a) = analysis {
                let mut imp = serde_json::Map::new();
                for (name, mean, sd) in &a.importance {
                    imp.insert(key(name), json!({"mean": mean, "sd": sd}));
                }
                ctx["importance"] = Value::Object(imp);
            }
            if let Some(obs) = observation(view, stage, selected) {
                ctx["selected"] = observation_json(&obs);
            }
            Context(ctx)
        }
    }
}

/// The learning rate actually used: correction divided by the stump's value
/// on a row where that value is not zero.
fn byhand_rate(rounds: &[Round], data: &Data) -> Option<f64> {
    let r = rounds.first()?;
    data.x.iter().zip(&r.correction).find_map(|(x, c)| {
        let h = r.tree.predict(x);
        (h != 0.0).then(|| c / h)
    })
}

fn observation_json(o: &Observation) -> Value {
    let mut x = serde_json::Map::new();
    for (n, v) in &o.x {
        x.insert(key(n), json!(v));
    }
    json!({
        "row": o.row, "y": o.y, "before": o.before, "h": o.h, "rate": o.rate, "after": o.after,
        "residual_before": o.residual_before, "residual_after": o.residual_after,
        "in_training": o.in_training, "in_sample": o.in_sample, "x": Value::Object(x),
    })
}

/// The selected row's arithmetic at `stage`, from the recorded stages.
pub fn observation(view: &RunView, stage: usize, selected: Option<usize>) -> Option<Observation> {
    let row = selected?;
    match view {
        RunView::ByHand { data, rounds, .. } => {
            if row >= data.y.len() {
                return None;
            }
            let x: Vec<(String, f64)> = data.names.iter().cloned().zip(data.x[row].iter().copied()).collect();
            let y = data.y[row];
            if stage == 0 || rounds.is_empty() {
                let mean = rounds.first().map(|r| r.before[row]).unwrap_or(y);
                return Some(Observation {
                    row, x, y, before: mean, h: None, rate: None, after: mean,
                    residual_before: y - mean, residual_after: y - mean, rules: vec![], in_training: true, in_sample: None,
                });
            }
            let r = &rounds[(stage - 1).min(rounds.len() - 1)];
            let h = r.tree.predict(&data.x[row]);
            let rate = if h != 0.0 { r.correction[row] / h } else { byhand_rate(rounds, data).unwrap_or(0.0) };
            let mut rules = vec![];
            r.tree.rules(&data.names, String::new(), &mut rules);
            Some(Observation {
                row, x, y, before: r.before[row], h: Some(h), rate: Some(rate), after: r.after[row],
                residual_before: r.residual_before[row], residual_after: r.residual_after[row], rules, in_training: true, in_sample: Some(true),
            })
        }
        RunView::Fitted { run, .. } => {
            if row >= run.data.y.len() {
                return None;
            }
            let stage = stage.min(run.last_stage());
            let x: Vec<(String, f64)> = run.data.names.iter().cloned().zip(run.data.x[row].iter().copied()).collect();
            let y = run.data.y[row];
            let after = run.predictions(stage)[row];
            let before = if stage == 0 { after } else { run.predictions(stage - 1)[row] };
            let (h, rate, rules, in_sample) = match fit_of(run) {
                Some(f) if stage >= 1 && stage <= f.trees.len() => {
                    let tree = &f.trees[stage - 1];
                    let mut rules = vec![];
                    tree.rules(&run.data.names, String::new(), &mut rules);
                    let rate = if f.cfg.method.boosting() { Some(f.cfg.rate) } else { None };
                    let in_sample = f.samples.get(stage - 1).map(|s| s.contains(&row));
                    (Some(tree.predict(&run.data.x[row])), rate, rules, in_sample)
                }
                _ => (None, None, vec![], None),
            };
            Some(Observation {
                row, x, y, before, h, rate, after,
                residual_before: y - before, residual_after: y - after, rules,
                in_training: run.partition.train.contains(&row),
                in_sample,
            })
        }
    }
}

/// The plots for a view at `stage`, with `feature` on the x axis.
pub fn plots_for(view: &RunView, stage: usize, feature: usize, selected: Option<usize>) -> Vec<Plot> {
    match view {
        RunView::ByHand { rounds, .. } => vec![plots::byhand_rounds(rounds, stage.saturating_sub(1).min(rounds.len().saturating_sub(1)))],
        RunView::Fitted { run, analysis, .. } => {
            let stage = stage.min(run.last_stage());
            let feature = feature.min(run.data.names.len() - 1);
            let mut out = vec![plots::loss_curve(run), plots::scatter_with_slice(run, feature, stage, selected)];
            if let Some(f) = fit_of(run) {
                if f.cfg.method == model_lab_engine::model::Method::Forest {
                    out.push(plots::oob_curve(f));
                }
            }
            if let Some(a) = analysis {
                out.push(plots::importance(a));
                out.push(plots::pdp_ice(a, &run.slice(a.feature, run.last_stage())));
            }
            out
        }
    }
}

/// Whether the cancel flag was raised, for tests and the runner.
pub fn canceled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}
