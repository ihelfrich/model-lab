use crate::experiment::{best_stage, build_view, context, observation, plots_for, stages, Observation, RunView, Status};
use crate::state::AppState;
use model_lab_engine::experiment::Run;
use model_lab_lessons::lesson::Learner;
use model_lab_lessons::plots::Plot;
use serde::Serialize;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct RunEntry {
    pub lesson_id: String,
    pub status: Status,
    pub view: Option<Arc<RunView>>,
    pub experiment_hash: Option<String>,
    pub cancel: Arc<AtomicBool>,
    pub check: crate::commands::record::CheckState,
}

#[derive(Clone, Debug, Serialize)]
pub struct RunPanel {
    pub run_id: String,
    pub lesson_id: String,
    pub stages: usize,
    pub stage: usize,
    pub best_stage: Option<usize>,
    pub features: Vec<String>,
    pub plots: Vec<Plot>,
    pub observation: Option<Observation>,
    pub controls: Vec<Learner>,
    pub config: Value,
    pub context: Value,
    pub kind: String,
}

fn new_id() -> String {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("run-{ms:x}-{n:x}")
}
static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Start a lesson's experiment with the learner's overrides on a worker
/// thread. Returns the run id at once; poll `experiment_status`.
pub fn start_experiment(state: &Arc<AppState>, lesson_id: &str, overrides: Value) -> Result<String, String> {
    let lesson = state.lesson(lesson_id)?;
    let run_id = new_id();
    let cancel = Arc::new(AtomicBool::new(false));
    state.runs.lock().map_err(|_| "runs unavailable")?.insert(
        run_id.clone(),
        RunEntry { lesson_id: lesson_id.to_string(), status: Status::Running, view: None, experiment_hash: None, cancel: cancel.clone(), check: Default::default() },
    );
    let state2 = state.clone();
    let id2 = run_id.clone();
    let lesson_id = lesson_id.to_string();
    std::thread::Builder::new()
        .name(format!("experiment {run_id}"))
        .spawn(move || {
            let result = build_view(&lesson_id, &lesson.experiment, &overrides, cancel.clone(), Run::INTERACTIVE_ROWS);
            if let Ok(mut runs) = state2.runs.lock() {
                if let Some(entry) = runs.get_mut(&id2) {
                    match result {
                        Ok((view, hash)) => {
                            entry.view = Some(Arc::new(view));
                            entry.experiment_hash = Some(hash);
                            entry.status = Status::Done;
                        }
                        Err(_) if cancel.load(Ordering::Relaxed) => entry.status = Status::Canceled,
                        Err(e) => entry.status = Status::Failed(e),
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(run_id)
}

/// Register an already-built view (a supervised result opened read-only).
pub fn register_view(state: &AppState, lesson_id: &str, view: RunView, hash: String) -> Result<String, String> {
    let run_id = new_id();
    state.runs.lock().map_err(|_| "runs unavailable")?.insert(
        run_id.clone(),
        RunEntry {
            lesson_id: lesson_id.to_string(),
            status: Status::Done,
            view: Some(Arc::new(view)),
            experiment_hash: Some(hash),
            cancel: Arc::new(AtomicBool::new(false)),
            check: Default::default(),
        },
    );
    Ok(run_id)
}

pub fn experiment_status(state: &AppState, run_id: &str) -> Result<Status, String> {
    let runs = state.runs.lock().map_err(|_| "runs unavailable")?;
    runs.get(run_id).map(|e| e.status.clone()).ok_or_else(|| format!("No run {run_id}"))
}

pub fn cancel_experiment(state: &AppState, run_id: &str) -> Result<(), String> {
    let runs = state.runs.lock().map_err(|_| "runs unavailable")?;
    let entry = runs.get(run_id).ok_or_else(|| format!("No run {run_id}"))?;
    entry.cancel.store(true, Ordering::Relaxed);
    Ok(())
}

/// The finished view and its lesson id and hash.
pub fn view(state: &AppState, run_id: &str) -> Result<(Arc<RunView>, String, String), String> {
    let runs = state.runs.lock().map_err(|_| "runs unavailable")?;
    let entry = runs.get(run_id).ok_or_else(|| format!("No run {run_id}"))?;
    match (&entry.status, &entry.view, &entry.experiment_hash) {
        (Status::Done, Some(v), Some(h)) => Ok((v.clone(), entry.lesson_id.clone(), h.clone())),
        (Status::Running, _, _) => Err("The experiment is still running.".into()),
        (Status::Canceled, _, _) => Err("The experiment was canceled.".into()),
        (Status::Failed(e), _, _) => Err(e.clone()),
        _ => Err("The experiment has no result.".into()),
    }
}

pub fn run_panel(state: &AppState, run_id: &str, stage: usize, feature: usize, selected: Option<usize>) -> Result<RunPanel, String> {
    let (v, lesson_id, _) = view(state, run_id)?;
    let lesson = state.lesson(&lesson_id)?;
    let n = stages(&v);
    let stage = stage.min(n.saturating_sub(1));
    let features = match &*v {
        RunView::Fitted { run, .. } => run.data.names.clone(),
        RunView::ByHand { data, .. } => data.names.clone(),
    };
    let ctx = context(&v, stage, selected);
    Ok(RunPanel {
        run_id: run_id.to_string(),
        lesson_id,
        stages: n,
        stage,
        best_stage: best_stage(&v),
        features,
        plots: plots_for(&v, stage, feature, selected),
        observation: observation(&v, stage, selected),
        controls: lesson.experiment.learner_may_change.clone(),
        config: match &*v {
            RunView::Fitted { run, .. } => run.config.to_json(),
            RunView::ByHand { .. } => lesson.experiment.config.to_json(),
        },
        context: ctx.0,
        kind: match &*v {
            RunView::Fitted { .. } => "run".into(),
            RunView::ByHand { .. } => "by_hand".into(),
        },
    })
}

/// The Read panel rendered against a finished run.
pub fn read_panel_for_run(state: &AppState, run_id: &str, stage: usize, selected: Option<usize>) -> Result<super::lessons::ReadPanel, String> {
    let (v, lesson_id, _) = view(state, run_id)?;
    let ctx = context(&v, stage, selected);
    super::lessons::read_panel_with(state, &lesson_id, &ctx)
}

pub mod ipc {
    use super::*;
    use tauri::State;

    #[tauri::command]
    pub fn start_experiment(state: State<'_, Arc<AppState>>, lesson_id: String, overrides: Value) -> Result<String, String> {
        super::start_experiment(&state, &lesson_id, overrides)
    }
    #[tauri::command]
    pub fn experiment_status(state: State<'_, Arc<AppState>>, run_id: String) -> Result<Status, String> {
        super::experiment_status(&state, &run_id)
    }
    #[tauri::command]
    pub fn cancel_experiment(state: State<'_, Arc<AppState>>, run_id: String) -> Result<(), String> {
        super::cancel_experiment(&state, &run_id)
    }
    #[tauri::command]
    pub fn run_panel(state: State<'_, Arc<AppState>>, run_id: String, stage: usize, feature: usize, selected: Option<usize>) -> Result<RunPanel, String> {
        super::run_panel(&state, &run_id, stage, feature, selected)
    }
    #[tauri::command]
    pub fn read_panel_for_run(state: State<'_, Arc<AppState>>, run_id: String, stage: usize, selected: Option<usize>) -> Result<super::super::lessons::ReadPanel, String> {
        super::read_panel_for_run(&state, &run_id, stage, selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::experiment::{apply_overrides, build_view};
    use crate::state::Dirs;
    use model_lab_engine::data::Data;
    use model_lab_engine::experiment::{ModelId, RunConfig};
    use model_lab_lessons::lesson::{Experiment, Kind, Source};
    use std::path::{Path, PathBuf};

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../lessons/tests/fixtures")
    }
    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("model-lab-exp-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }
    fn boosting_experiment() -> Experiment {
        let mut config = RunConfig::default();
        config.model = ModelId::parse("boosting").unwrap();
        config.tree.trees = 12;
        Experiment {
            kind: Kind::Run,
            source: Source::Example("Florida communities".into()),
            target: "violentcrimes.perpop".into(),
            features: vec!["pct.unemployed".into(), "med.income".into()],
            config,
            byhand: None,
            learner_may_change: vec![Learner { field: "rate".into(), min: 0.05, max: 1.0, step: 0.05 }],
            checkpoint_every_trees: 10,
        }
    }
    fn no_cancel() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    #[test]
    fn context_for_a_fitted_run_has_the_fixed_names() {
        let (view, _) = build_view("l", &boosting_experiment(), &serde_json::json!({}), no_cancel(), 5000).unwrap();
        let ctx = context(&view, 12, Some(0));
        for path in ["data.n", "rows.train", "rows.valid", "stage.best", "stage.current", "metric.valid_mse", "metric.train_rmse", "selected.after", "selected.x.med_income"] {
            assert!(ctx.get(path).is_some(), "{path} missing: {}", ctx.0);
        }
    }

    #[test]
    fn context_for_byhand_has_rounds_and_candidates() {
        let lesson_dir = fixtures().join("valid-lesson");
        let lesson = model_lab_lessons::lesson::load_dir(&lesson_dir).unwrap();
        let (view, _) = build_view("fixture-lesson", &lesson.experiment, &serde_json::json!({}), no_cancel(), 5000).unwrap();
        let ctx = context(&view, 2, None);
        assert_eq!(ctx.get("byhand.round.1.mse_after").and_then(|v| v.as_f64()), Some(1.5625));
        assert_eq!(ctx.get("byhand.candidates.1.reduction").and_then(|v| v.as_f64()), Some(36.0));
        assert_eq!(ctx.get("byhand.sse").and_then(|v| v.as_f64()), Some(40.0));
        assert_eq!(ctx.get("byhand.candidates.0.admissible").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn overrides_are_restricted_and_clamped() {
        let exp = boosting_experiment();
        let cfg = apply_overrides(&exp.config, &exp.learner_may_change, &serde_json::json!({"rate": 5, "depth": 9})).unwrap();
        assert_eq!(cfg.tree.rate, 1.0);
        assert_eq!(cfg.tree.depth, exp.config.tree.depth);
    }

    #[test]
    fn observation_arithmetic_is_exact() {
        let (view, _) = build_view("l", &boosting_experiment(), &serde_json::json!({}), no_cancel(), 5000).unwrap();
        for stage in 1..=12 {
            let o = observation(&view, stage, Some(3)).unwrap();
            let expected = o.before + o.rate.unwrap() * o.h.unwrap();
            assert!((o.after - expected).abs() < 1e-12, "stage {stage}: {} vs {expected}", o.after);
            assert!((o.residual_after - (o.y - o.after)).abs() < 1e-12);
        }
    }

    #[test]
    fn large_data_is_refused_in_process() {
        let mut exp = boosting_experiment();
        let dir = tmp("large");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.csv");
        let mut text = String::from("a,y\n");
        for i in 0..5001 {
            text.push_str(&format!("{i},{}\n", i % 7));
        }
        std::fs::write(&path, text).unwrap();
        exp.source = Source::Csv(path.to_string_lossy().into());
        exp.target = "y".into();
        exp.features = vec!["a".into()];
        let err = build_view("l", &exp, &serde_json::json!({}), no_cancel(), 5000).err().unwrap();
        assert!(err.contains("supervised job") || err.contains("5000"), "{err}");
    }

    #[test]
    fn cancel_stops_a_running_fit() {
        let dirs = Dirs::under(fixtures(), &tmp("cancel"));
        let state = Arc::new(AppState::new(dirs));
        // A heavy experiment: 5000 synthetic rows, 300 deep trees.
        let mut exp = boosting_experiment();
        exp.config.tree.trees = 300;
        exp.config.tree.depth = 6;
        let dir = tmp("cancel-data");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("d.csv");
        let mut text = String::from("a,b,c,y\n");
        let mut s: u64 = 7;
        for _ in 0..5000 {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let a = (s >> 33) as f64 / 2f64.powi(31);
            let b = ((s >> 11) & 0xffff) as f64 / 65536.0;
            let c = (s & 0xffff) as f64 / 65536.0;
            text.push_str(&format!("{a},{b},{c},{}\n", a * b + c.sin()));
        }
        std::fs::write(&path, text).unwrap();
        exp.source = Source::Csv(path.to_string_lossy().into());
        exp.target = "y".into();
        exp.features = vec!["a".into(), "b".into(), "c".into()];
        // Install the experiment on a fixture lesson so start_experiment finds it.
        {
            let mut lessons = state.lessons.write().unwrap();
            let l = lessons.iter_mut().find(|l| l.front.id == "fixture-lesson").unwrap();
            l.experiment = exp;
        }
        let id = start_experiment(&state, "fixture-lesson", serde_json::json!({})).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(experiment_status(&state, &id).unwrap(), Status::Running);
        cancel_experiment(&state, &id).unwrap();
        let start = std::time::Instant::now();
        loop {
            let s = experiment_status(&state, &id).unwrap();
            if s != Status::Running {
                assert_eq!(s, Status::Canceled);
                break;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(2), "cancel took too long");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = Data { names: vec![], target: String::new(), x: vec![], y: vec![] };
    }

    #[test]
    fn in_sample_marks_rows_outside_this_rounds_sample() {
        let mut exp = boosting_experiment();
        exp.config.model = ModelId::parse("stochastic").unwrap();
        exp.config.tree.fraction = 0.5;
        let (view, _) = build_view("l", &exp, &serde_json::json!({}), no_cancel(), 5000).unwrap();
        let flags: Vec<bool> = (0..90).filter_map(|row| observation(&view, 1, Some(row)).and_then(|o| o.in_sample)).collect();
        assert!(flags.iter().any(|&b| b) && flags.iter().any(|&b| !b), "{flags:?}");
        let full = build_view("l", &boosting_experiment(), &serde_json::json!({}), no_cancel(), 5000).unwrap().0;
        let training_row = match &full {
            RunView::Fitted { run, .. } => run.partition.train[0],
            _ => unreachable!(),
        };
        assert_eq!(observation(&full, 1, Some(training_row)).unwrap().in_sample, Some(true));
    }

    #[test]
    fn fraction_below_one_selects_stochastic() {
        let mut exp = boosting_experiment();
        exp.learner_may_change.push(Learner { field: "fraction".into(), min: 0.3, max: 1.0, step: 0.1 });
        let cfg = apply_overrides(&exp.config, &exp.learner_may_change, &serde_json::json!({"fraction": 0.5})).unwrap();
        assert_eq!(cfg.model, ModelId::parse("stochastic").unwrap());
        let back = apply_overrides(&exp.config, &exp.learner_may_change, &serde_json::json!({"fraction": 1.0})).unwrap();
        assert_eq!(back.model, ModelId::parse("boosting").unwrap());
    }

    #[test]
    fn last_minus_best_is_nonnegative() {
        let (view, _) = build_view("l", &boosting_experiment(), &serde_json::json!({}), no_cancel(), 5000).unwrap();
        let ctx = context(&view, 12, None);
        let delta = ctx.get("metric.valid_mse_at_last_minus_best").and_then(|v| v.as_f64()).unwrap();
        assert!(delta >= 0.0, "{delta}");
    }

    #[test]
    fn mtry_comparison_is_a_real_second_fit() {
        let mut exp = boosting_experiment();
        exp.config.model = ModelId::parse("forest").unwrap();
        exp.config.tree.trees = 20;
        exp.config.tree.mtry = 1;
        let (view, _) = build_view("l", &exp, &serde_json::json!({}), no_cancel(), 5000).unwrap();
        match &view {
            RunView::Fitted { run, mtry_full: Some(full), .. } => {
                assert_eq!(full.cfg.mtry, run.data.names.len());
                assert_ne!(full.predictions(20), run.predictions(20));
            }
            _ => panic!("no comparison fit"),
        }
        let ctx = context(&view, 20, None);
        assert!(ctx.get("oob.mse_delta_mtry_full").and_then(|v| v.as_f64()).is_some(), "{}", ctx.0);
        let (boost, _) = build_view("l", &boosting_experiment(), &serde_json::json!({}), no_cancel(), 5000).unwrap();
        assert!(matches!(boost, RunView::Fitted { mtry_full: None, .. }));
    }
}
