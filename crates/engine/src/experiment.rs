//! Model registry, deterministic experiments, diagnostics and replay contracts.
//! Numerical engines never import UI types; views consume a common Run.
use crate::{
    data::Data,
    linear::{self, LinearConfig, LinearFit, LinearMethod},
    model,
};
use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Saved-experiment schema. 1 was Model Lab 0.8; 2 (engine 0.9) changed how
/// forests and stochastic boosting draw their samples (per-tree streams).
pub const TRACE_SCHEMA: u64 = 2;
/// Shown when a schema-1 experiment is opened.
pub const SCHEMA_ONE_NOTICE: &str = "This experiment was saved by Model Lab 0.8. Random forests and stochastic boosting draw their samples differently since 0.9 (see CHANGED-RESULTS.md); it opens read-only and is not refitted.";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    Regression,
    BinaryClassification,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelId {
    Linear(LinearMethod),
    Tree,
    Boosting,
    Stochastic,
    Forest,
}
pub struct ModelDescriptor {
    pub id: ModelId,
    pub name: &'static str,
    pub task: Task,
    pub summary: &'static str,
    pub stage_name: &'static str,
    pub coefficients: bool,
    pub penalty_path: bool,
}
pub const MODELS: [ModelDescriptor; 9] = [
    ModelDescriptor {
        id: ModelId::Linear(LinearMethod::Ols),
        name: "Ordinary least squares",
        task: Task::Regression,
        summary: "Project outcomes onto a linear predictor space.",
        stage_name: "projection",
        coefficients: true,
        penalty_path: false,
    },
    ModelDescriptor {
        id: ModelId::Linear(LinearMethod::Logistic),
        name: "Logistic regression",
        task: Task::BinaryClassification,
        summary: "Connect a linear score to a probability.",
        stage_name: "Newton update",
        coefficients: true,
        penalty_path: false,
    },
    ModelDescriptor {
        id: ModelId::Linear(LinearMethod::Ridge),
        name: "Ridge regression",
        task: Task::Regression,
        summary: "Stabilize a linear fit with an L2 penalty.",
        stage_name: "solver sweep",
        coefficients: true,
        penalty_path: true,
    },
    ModelDescriptor {
        id: ModelId::Linear(LinearMethod::Lasso),
        name: "Lasso",
        task: Task::Regression,
        summary: "Shrink coefficients and select predictors with L1.",
        stage_name: "solver sweep",
        coefficients: true,
        penalty_path: true,
    },
    ModelDescriptor {
        id: ModelId::Linear(LinearMethod::ElasticNet),
        name: "Elastic net",
        task: Task::Regression,
        summary: "Combine sparse selection and grouped shrinkage.",
        stage_name: "solver sweep",
        coefficients: true,
        penalty_path: true,
    },
    ModelDescriptor {
        id: ModelId::Tree,
        name: "Decision tree",
        task: Task::Regression,
        summary: "Partition predictor space into mean-response regions.",
        stage_name: "tree",
        coefficients: false,
        penalty_path: false,
    },
    ModelDescriptor {
        id: ModelId::Boosting,
        name: "Gradient boosting",
        task: Task::Regression,
        summary: "Add shallow corrections to current residuals.",
        stage_name: "tree",
        coefficients: false,
        penalty_path: false,
    },
    ModelDescriptor {
        id: ModelId::Stochastic,
        name: "Stochastic boosting",
        task: Task::Regression,
        summary: "Learn each correction from a fresh row sample.",
        stage_name: "tree",
        coefficients: false,
        penalty_path: false,
    },
    ModelDescriptor {
        id: ModelId::Forest,
        name: "Random forest",
        task: Task::Regression,
        summary: "Average randomized, bootstrap-fitted trees.",
        stage_name: "tree",
        coefficients: false,
        penalty_path: false,
    },
];
impl ModelId {
    pub fn descriptor(self) -> &'static ModelDescriptor {
        MODELS.iter().find(|d| d.id == self).unwrap()
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Linear(m) => m.key(),
            Self::Tree => "tree",
            Self::Boosting => "boosting",
            Self::Stochastic => "stochastic",
            Self::Forest => "forest",
        }
    }
    pub fn parse(s: &str) -> Result<Self, String> {
        MODELS
            .iter()
            .find(|d| d.id.key() == s)
            .map(|d| d.id)
            .ok_or_else(|| format!("Unknown model identifier: {s}"))
    }
    fn tree_method(self) -> Option<model::Method> {
        match self {
            Self::Tree => Some(model::Method::Tree),
            Self::Boosting => Some(model::Method::Boosting),
            Self::Stochastic => Some(model::Method::Stochastic),
            Self::Forest => Some(model::Method::Forest),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct RunConfig {
    pub model: ModelId,
    pub seed: u64,
    pub validation: f64,
    pub linear: LinearConfig,
    pub tree: model::Config,
}
impl Default for RunConfig {
    fn default() -> Self {
        Self {
            model: ModelId::Linear(LinearMethod::Ols),
            seed: 4710,
            validation: 0.25,
            linear: LinearConfig::default(),
            tree: model::Config::default(),
        }
    }
}
impl RunConfig {
    /// The `config` object of a saved experiment and of a job specification.
    pub fn to_json(&self) -> Value {
        let c = self;
        let t = &c.tree;
        json!({"model":c.model.key(),"seed":c.seed,"validation":c.validation,"alpha":c.linear.alpha,"mix":c.linear.mix,"standardize":c.linear.standardize,"tolerance":c.linear.tolerance,"max_iterations":c.linear.max_iterations,
            "trees":t.trees,"depth":t.depth,"min_leaf":t.min_leaf,"rate":t.rate,"fraction":t.fraction,"mtry":t.mtry,"patience":t.patience,"min_relative_improvement":t.min_relative_improvement})
    }
    pub fn from_json(c: &Value) -> Result<Self, String> {
        let number = |key: &str| {
            c[key]
                .as_f64()
                .ok_or_else(|| format!("Missing numeric setting {key}"))
        };
        let integer = |key: &str| {
            c[key]
                .as_u64()
                .map(|v| v as usize)
                .ok_or_else(|| format!("Missing integer setting {key}"))
        };
        let mut cfg = RunConfig::default();
        cfg.model = ModelId::parse(c["model"].as_str().ok_or("Missing model")?)?;
        cfg.seed = c["seed"].as_u64().ok_or("Missing seed")?;
        cfg.validation = number("validation")?;
        cfg.linear.alpha = number("alpha")?;
        cfg.linear.mix = number("mix")?;
        cfg.linear.standardize = c["standardize"]
            .as_bool()
            .ok_or("Missing preprocessing setting")?;
        cfg.linear.tolerance = number("tolerance")?;
        cfg.linear.max_iterations = integer("max_iterations")?;
        cfg.tree.trees = integer("trees")?;
        cfg.tree.depth = integer("depth")?;
        cfg.tree.min_leaf = integer("min_leaf")?;
        cfg.tree.rate = number("rate")?;
        cfg.tree.fraction = number("fraction")?;
        cfg.tree.mtry = integer("mtry")?;
        cfg.tree.patience = integer("patience")?;
        cfg.tree.min_relative_improvement = number("min_relative_improvement")?;
        Ok(cfg)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Partition {
    pub train: Vec<usize>,
    pub valid: Vec<usize>,
    pub stratified: bool,
}
pub fn partition(data: &Data, task: Task, seed: u64, fraction: f64) -> Result<Partition, String> {
    let n = data.y.len();
    if n < 4 || !fraction.is_finite() || !(0.1..=0.5).contains(&fraction) {
        return Err(
            "Use at least four observations and a validation fraction between0.1 and0.5.".into(),
        );
    }
    let mut rng = StdRng::seed_from_u64(seed);
    if task == Task::BinaryClassification {
        if data.y.iter().any(|&v| v != 0. && v != 1.) {
            return Err("Binary classification requires 0/1 outcomes.".into());
        }
        let mut train = Vec::new();
        let mut valid = Vec::new();
        for class in [0., 1.] {
            let mut rows = (0..n).filter(|&i| data.y[i] == class).collect::<Vec<_>>();
            if rows.len() < 2 {
                return Err(
                    "For stratified validation, each class needs at least two observations.".into(),
                );
            }
            rows.shuffle(&mut rng);
            let count = ((rows.len() as f64 * fraction).round() as usize).clamp(1, rows.len() - 1);
            valid.extend_from_slice(&rows[..count]);
            train.extend_from_slice(&rows[count..]);
        }
        train.sort_unstable();
        valid.sort_unstable();
        Ok(Partition {
            train,
            valid,
            stratified: true,
        })
    } else {
        let mut rows = (0..n).collect::<Vec<_>>();
        rows.shuffle(&mut rng);
        let count = ((n as f64 * fraction).round() as usize).clamp(1, n - 2);
        Ok(Partition {
            train: rows[count..].to_vec(),
            valid: rows[..count].to_vec(),
            stratified: false,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Metrics {
    pub loss: f64,
    pub rmse: Option<f64>,
    pub mae: Option<f64>,
    pub r2: Option<f64>,
    pub accuracy: Option<f64>,
    pub brier: Option<f64>,
    pub auc: Option<f64>,
    pub confusion: Option<[[usize; 2]; 2]>,
}
pub fn metrics(y: &[f64], pred: &[f64], rows: &[usize], task: Task, threshold: f64) -> Metrics {
    let n = rows.len() as f64;
    let average = rows.iter().map(|&i| y[i]).sum::<f64>() / n;
    let sse = rows.iter().map(|&i| (y[i] - pred[i]).powi(2)).sum::<f64>();
    if task == Task::Regression {
        let sst = rows.iter().map(|&i| (y[i] - average).powi(2)).sum::<f64>();
        Metrics {
            loss: sse / n,
            rmse: Some((sse / n).sqrt()),
            mae: Some(rows.iter().map(|&i| (y[i] - pred[i]).abs()).sum::<f64>() / n),
            r2: if sst > 0. { Some(1. - sse / sst) } else { None },
            accuracy: None,
            brier: None,
            auc: None,
            confusion: None,
        }
    } else {
        let loss = rows
            .iter()
            .map(|&i| {
                let p = pred[i].clamp(1e-15, 1. - 1e-15);
                if y[i] == 1. {
                    -p.ln()
                } else {
                    -(-p).ln_1p()
                }
            })
            .sum::<f64>()
            / n;
        let mut confusion = [[0; 2]; 2];
        for &i in rows {
            confusion[y[i] as usize][usize::from(pred[i] >= threshold)] += 1;
        }
        let mut ranked = rows.iter().map(|&i| (pred[i], y[i])).collect::<Vec<_>>();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
        let positives = ranked.iter().filter(|x| x.1 == 1.).count() as f64;
        let negatives = n - positives;
        let mut sum = 0.;
        let mut k = 0;
        while k < ranked.len() {
            let mut end = k + 1;
            while end < ranked.len() && ranked[end].0 == ranked[k].0 {
                end += 1;
            }
            let rank = (k + 1 + end) as f64 / 2.;
            sum += ranked[k..end].iter().filter(|v| v.1 == 1.).count() as f64 * rank;
            k = end;
        }
        Metrics {
            loss,
            rmse: None,
            mae: None,
            r2: None,
            accuracy: Some((confusion[0][0] + confusion[1][1]) as f64 / n),
            brier: Some(sse / n),
            auc: if positives > 0. && negatives > 0. {
                Some((sum - positives * (positives + 1.) / 2.) / (positives * negatives))
            } else {
                None
            },
            confusion: Some(confusion),
        }
    }
}

#[derive(Clone)]
pub enum FittedModel {
    Linear(LinearFit),
    Trees(model::Fit),
}
#[derive(Clone, Debug)]
pub struct TraceStep {
    pub stage: usize,
    pub iteration: usize,
    pub label: String,
    pub objective: f64,
    pub train_loss: f64,
    pub valid_loss: f64,
    pub stationarity: Option<f64>,
}
#[derive(Clone)]
pub struct Run {
    pub name: String,
    pub data: Data,
    pub config: RunConfig,
    pub partition: Partition,
    pub fitted: FittedModel,
    pub trace: Vec<TraceStep>,
    pub fingerprint: String,
}
impl Run {
    /// Interactive bound on rows for in-process fits.
    pub const INTERACTIVE_ROWS: usize = 5000;

    pub fn fit(
        name: String,
        data: Data,
        config: RunConfig,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        Self::fit_with_limit(name, data, config, cancel, Self::INTERACTIVE_ROWS)
    }

    /// Reject data outside the rectangular, finite, 1–64-predictor shape or
    /// above `max_rows`.
    pub fn validate_data(data: &Data, max_rows: usize) -> Result<(), String> {
        let p = data.names.len();
        if data.y.len() > max_rows
            || p == 0
            || p > 64
            || data.x.len() != data.y.len()
            || data
                .x
                .iter()
                .any(|r| r.len() != p || r.iter().any(|v| !v.is_finite() || v.abs() > 1e12))
            || data.y.iter().any(|v| !v.is_finite() || v.abs() > 1e12)
        {
            return Err(format!("Use a rectangular finite numeric dataset with 1–64 predictors and at most {max_rows} rows."));
        }
        Ok(())
    }

    /// Prepare `config` for its model family and draw the partition.
    pub fn prepare(data: &Data, config: &mut RunConfig) -> Result<Partition, String> {
        let task = config.model.descriptor().task;
        if let ModelId::Linear(method) = config.model {
            config.linear.method = method;
        } else {
            config.tree.method = config.model.tree_method().unwrap();
            config.tree.seed = config.seed;
            config.tree.validation = config.validation;
        }
        partition(data, task, config.seed, config.validation)
    }

    pub fn fit_with_limit(
        name: String,
        data: Data,
        mut config: RunConfig,
        cancel: Arc<AtomicBool>,
        max_rows: usize,
    ) -> Result<Self, String> {
        Self::validate_data(&data, max_rows)?;
        let split = Self::prepare(&data, &mut config)?;
        let fitted = if let ModelId::Linear(_) = config.model {
            FittedModel::Linear(linear::fit(
                &data,
                &split.train,
                config.linear.clone(),
                cancel.clone(),
            )?)
        } else {
            FittedModel::Trees(model::fit(data.clone(), config.tree.clone(), cancel.clone())?)
        };
        Self::from_fitted(name, data, config, split, fitted, cancel)
    }

    /// Assemble the experiment record (trace of every stage) around a model
    /// fitted elsewhere, for example stage by stage under the supervisor.
    pub fn from_fitted(
        name: String,
        data: Data,
        config: RunConfig,
        mut split: Partition,
        fitted: FittedModel,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let task = config.model.descriptor().task;
        if let FittedModel::Trees(f) = &fitted {
            if f.train != split.train || f.valid != split.valid {
                return Err("Tree adapter partition does not match experiment partition.".into());
            }
            split = Partition {
                train: f.train.clone(),
                valid: f.valid.clone(),
                stratified: false,
            };
        }
        let count = match &fitted {
            FittedModel::Linear(f) => f.steps.len(),
            FittedModel::Trees(f) => f.trees.len() + 1,
        };
        let mut run = Self {
            name,
            data,
            config,
            partition: split,
            fitted,
            trace: Vec::with_capacity(count),
            fingerprint: String::new(),
        };
        run.fingerprint = data_fingerprint(&run.data);
        for stage in 0..count {
            if cancel.load(Ordering::Relaxed) {
                return Err("Fit canceled.".into());
            }
            let pred = run.predictions(stage);
            let tr = metrics(&run.data.y, &pred, &run.partition.train, task, 0.5);
            let va = metrics(&run.data.y, &pred, &run.partition.valid, task, 0.5);
            let (iteration, label, objective, stationarity) = match &run.fitted {
                FittedModel::Linear(f) => {
                    let s = f.step(stage);
                    (
                        s.iteration,
                        s.label.clone(),
                        s.objective,
                        Some(s.stationarity),
                    )
                }
                FittedModel::Trees(_) => (
                    stage,
                    if stage == 0 {
                        "Mean-response baseline".into()
                    } else {
                        format!("Added tree {stage}")
                    },
                    tr.loss / 2.,
                    None,
                ),
            };
            run.trace.push(TraceStep {
                stage,
                iteration,
                label,
                objective,
                train_loss: tr.loss,
                valid_loss: va.loss,
                stationarity,
            });
        }
        Ok(run)
    }
    pub fn task(&self) -> Task {
        self.config.model.descriptor().task
    }
    pub fn last_stage(&self) -> usize {
        self.trace.len() - 1
    }
    pub fn predict(&self, x: &[f64], stage: usize) -> f64 {
        match &self.fitted {
            FittedModel::Linear(f) => f.predict(x, stage),
            FittedModel::Trees(f) => f.predict(x, stage),
        }
    }
    pub fn predictions(&self, stage: usize) -> Vec<f64> {
        match &self.fitted {
            FittedModel::Linear(_) => self.data.x.iter().map(|x| self.predict(x, stage)).collect(),
            FittedModel::Trees(f) => f.predictions(stage),
        }
    }
    pub fn scores(&self, stage: usize, threshold: f64) -> (Metrics, Metrics) {
        let p = self.predictions(stage);
        (
            metrics(
                &self.data.y,
                &p,
                &self.partition.train,
                self.task(),
                threshold,
            ),
            metrics(
                &self.data.y,
                &p,
                &self.partition.valid,
                self.task(),
                threshold,
            ),
        )
    }
    pub fn coefficients(&self, stage: usize) -> Option<(f64, Vec<f64>)> {
        match &self.fitted {
            FittedModel::Linear(f) => Some(f.coefficients(stage)),
            _ => None,
        }
    }
    pub fn warning(&self) -> Option<&str> {
        match &self.fitted {
            FittedModel::Linear(f) => f.warning.as_deref(),
            _ => None,
        }
    }
    pub fn converged(&self) -> bool {
        match &self.fitted {
            FittedModel::Linear(f) => f.converged,
            _ => true,
        }
    }
    pub fn reference_row(&self) -> Vec<f64> {
        (0..self.data.names.len())
            .map(|j| {
                let mut v = self
                    .partition
                    .train
                    .iter()
                    .map(|&i| self.data.x[i][j])
                    .collect::<Vec<_>>();
                v.sort_by(f64::total_cmp);
                let n = v.len();
                if n % 2 == 1 {
                    v[n / 2]
                } else {
                    (v[n / 2 - 1] + v[n / 2]) / 2.
                }
            })
            .collect()
    }
    pub fn slice(&self, feature: usize, stage: usize) -> Vec<(f64, f64)> {
        let mut row = self.reference_row();
        let (lo, hi) = self.domain(feature);
        (0..161)
            .map(|k| {
                let x = lo + (hi - lo) * k as f64 / 160.;
                row[feature] = x;
                (x, self.predict(&row, stage))
            })
            .collect()
    }
    pub fn domain(&self, j: usize) -> (f64, f64) {
        let lo = self
            .data
            .x
            .iter()
            .map(|r| r[j])
            .fold(f64::INFINITY, f64::min);
        let hi = self
            .data
            .x
            .iter()
            .map(|r| r[j])
            .fold(f64::NEG_INFINITY, f64::max);
        if lo == hi {
            (lo - 1., hi + 1.)
        } else {
            (lo, hi)
        }
    }
    pub fn matched(&self, other: &Self) -> bool {
        self.data == other.data && self.partition == other.partition && self.task() == other.task()
    }
    pub fn to_json(&self) -> Value {
        let c = &self.config;
        json!({"schema_version":TRACE_SCHEMA,"engine_version":env!("CARGO_PKG_VERSION"),"name":self.name,"fingerprint":self.fingerprint,
            "data":{"names":self.data.names,"target":self.data.target,"x":self.data.x,"y":self.data.y},
            "config":c.to_json(),
            "partition":{"train":self.partition.train,"valid":self.partition.valid},
            "trace":self.trace.iter().map(|s|json!({"stage":s.stage,"iteration":s.iteration,"label":s.label,"objective":s.objective,"train_loss":s.train_loss,"valid_loss":s.valid_loss,"stationarity":s.stationarity,"coefficients":self.coefficients(s.stage)})).collect::<Vec<_>>(),
            "final_predictions":self.predictions(self.last_stage()),"converged":self.converged(),"warning":self.warning(),
            "ensemble": match &self.fitted { FittedModel::Trees(f) => crate::ensemble::state_document(f), FittedModel::Linear(_) => Value::Null }})
    }
    /// Open a saved tree-model experiment read-only: the stored trees are
    /// replayed (predictions only, no split search) and every recorded
    /// number is checked against the replay. `max_rows` is the caller's
    /// bound (a supervised job's `max_rows`), not the interactive one.
    pub fn from_trace(value: &Value, max_rows: usize) -> Result<Self, String> {
        match value["schema_version"].as_u64() {
            Some(TRACE_SCHEMA) => {}
            Some(1) => return Err(SCHEMA_ONE_NOTICE.into()),
            _ => return Err("Unsupported saved-experiment schema.".into()),
        }
        let d = &value["data"];
        let names =
            serde_json::from_value::<Vec<String>>(d["names"].clone()).map_err(|e| e.to_string())?;
        let data = Data {
            names,
            target: d["target"].as_str().ok_or("Missing response name")?.into(),
            x: serde_json::from_value(d["x"].clone()).map_err(|e| e.to_string())?,
            y: serde_json::from_value(d["y"].clone()).map_err(|e| e.to_string())?,
        };
        Self::validate_data(&data, max_rows)?;
        let mut cfg = RunConfig::from_json(&value["config"])?;
        if let ModelId::Linear(_) = cfg.model {
            return Err("Linear experiments are reloaded by refitting.".into());
        }
        if value["ensemble"].is_null() {
            return Err("This saved experiment has no ensemble state; open it with the interactive loader.".into());
        }
        let split = Self::prepare(&data, &mut cfg)?;
        let fit = crate::ensemble::EnsembleState::from_json(&value["ensemble"], data.clone())?.into_fit();
        let run = Self::from_fitted(
            value["name"].as_str().unwrap_or("Saved experiment").into(),
            data,
            cfg,
            split,
            FittedModel::Trees(fit),
            Arc::new(AtomicBool::new(false)),
        )?;
        Self::check_replay(value, &run)?;
        Ok(run)
    }
    fn check_replay(value: &Value, run: &Run) -> Result<(), String> {
        let predictions = serde_json::from_value::<Vec<f64>>(value["final_predictions"].clone())
            .map_err(|e| e.to_string())?;
        let train = serde_json::from_value::<Vec<usize>>(value["partition"]["train"].clone())
            .map_err(|e| e.to_string())?;
        let valid = serde_json::from_value::<Vec<usize>>(value["partition"]["valid"].clone())
            .map_err(|e| e.to_string())?;
        if predictions.len() != run.data.y.len()
            || run.partition.train != train
            || run.partition.valid != valid
            || value["fingerprint"].as_str() != Some(&run.fingerprint)
            || predictions
                .iter()
                .zip(run.predictions(run.last_stage()))
                .any(|(a, b)| !a.is_finite() || (a - b).abs() > 1e-8 * (1. + a.abs()))
        {
            return Err("This saved experiment does not reproduce under the current engine. No replay was accepted.".into());
        }
        let actual = run.to_json();
        for key in ["trace", "converged", "warning"] {
            if !replay_matches(&value[key], &actual[key]) {
                return Err(format!(
                    "Saved {key} does not reproduce under the current engine."
                ));
            }
        }
        Ok(())
    }
    pub fn from_json(value: &Value, cancel: Arc<AtomicBool>) -> Result<Self, String> {
        match value["schema_version"].as_u64() {
            Some(TRACE_SCHEMA) => {}
            Some(1) => return Err(SCHEMA_ONE_NOTICE.into()),
            _ => return Err("Unsupported saved-experiment schema.".into()),
        }
        let d = &value["data"];
        let c = &value["config"];
        let names =
            serde_json::from_value::<Vec<String>>(d["names"].clone()).map_err(|e| e.to_string())?;
        let data = Data {
            names,
            target: d["target"].as_str().ok_or("Missing response name")?.into(),
            x: serde_json::from_value(d["x"].clone()).map_err(|e| e.to_string())?,
            y: serde_json::from_value(d["y"].clone()).map_err(|e| e.to_string())?,
        };
        if data.y.len() > 5000 || data.names.len() > 64 {
            return Err("Saved experiment exceeds teaching-workspace limits.".into());
        }
        let cfg = RunConfig::from_json(c)?;
        let run = Self::fit(
            value["name"].as_str().unwrap_or("Saved experiment").into(),
            data,
            cfg,
            cancel,
        )?;
        Self::check_replay(value, &run)?;
        Ok(run)
    }
}
fn replay_matches(saved: &Value, actual: &Value) -> bool {
    match (saved, actual) {
        (Value::Number(a), Value::Number(b)) => match (a.as_f64(), b.as_f64()) {
            (Some(a), Some(b)) => {
                a.is_finite() && b.is_finite() && (a - b).abs() <= 1e-8 * (1. + b.abs())
            }
            _ => false,
        },
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| replay_matches(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.get(k).is_some_and(|w| replay_matches(v, w)))
        }
        _ => saved == actual,
    }
}

pub fn data_fingerprint(data: &Data) -> String {
    // Stable FNV-1a identity for matching experiments, not a cryptographic hash.
    let mut h = 0xcbf29ce484222325u64;
    let bytes =
        serde_json::to_vec(&json!({"x":data.x,"y":data.y,"names":data.names,"target":data.target}))
            .unwrap();
    for b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{h:016x}")
}

pub struct PenaltyPoint {
    pub alpha: f64,
    pub standardized: Vec<f64>,
    pub original: Vec<f64>,
    pub train_loss: f64,
    pub valid_loss: f64,
    pub converged: bool,
}
pub fn penalty_path(run: &Run, cancel: Arc<AtomicBool>) -> Result<Vec<PenaltyPoint>, String> {
    let ModelId::Linear(method) = run.config.model else {
        return Err("This model has no coefficient penalty path.".into());
    };
    if !method.penalized() {
        return Err("Choose ridge, lasso or elastic net for a penalty path.".into());
    }
    let mut points = Vec::new();
    for k in 0..33 {
        if cancel.load(Ordering::Relaxed) {
            return Err("Path canceled.".into());
        }
        let alpha = 10_f64.powf(-4. + k as f64 * 6. / 32.);
        let mut cfg = run.config.linear.clone();
        cfg.alpha = alpha;
        let f = linear::fit(&run.data, &run.partition.train, cfg, cancel.clone())?;
        let stage = f.steps.len() - 1;
        let predictions = run
            .data
            .x
            .iter()
            .map(|x| f.predict(x, stage))
            .collect::<Vec<_>>();
        let tr = metrics(
            &run.data.y,
            &predictions,
            &run.partition.train,
            Task::Regression,
            0.5,
        );
        let va = metrics(
            &run.data.y,
            &predictions,
            &run.partition.valid,
            Task::Regression,
            0.5,
        );
        points.push(PenaltyPoint {
            alpha,
            standardized: f.step(stage).beta.clone(),
            original: f.coefficients(stage).1,
            train_loss: tr.loss,
            valid_loss: va.loss,
            converged: f.converged,
        });
    }
    Ok(points)
}

pub const SCENARIOS: [(&str, &str); 5] = [
    (
        "Linear signal",
        "A positive and a negative effect, plus an irrelevant predictor.",
    ),
    (
        "Correlated predictors",
        "Two near-substitute predictors expose coefficient instability.",
    ),
    (
        "Sparse signal",
        "Only two of six predictors enter the synthetic response.",
    ),
    (
        "Binary outcomes",
        "Overlapping classes generated from a known logistic probability.",
    ),
    (
        "Nonlinear signal",
        "A curved response illustrates the limits of a linear specification.",
    ),
];
pub fn scenario(index: usize, seed: u64, noise: f64) -> Data {
    let mut rng = StdRng::seed_from_u64(seed);
    let index = index.min(SCENARIOS.len() - 1);
    let p = if index == 2 { 6 } else { 3 };
    let mut x = Vec::new();
    let mut y = Vec::new();
    for _ in 0..220 {
        let a = rng.gen_range(-2.5..2.5);
        let b = if index == 1 {
            a + rng.gen_range(-0.12..0.12)
        } else {
            rng.gen_range(-2.5..2.5)
        };
        let mut row = vec![a, b, rng.gen_range(-2.5..2.5)];
        while row.len() < p {
            row.push(rng.gen_range(-2.5..2.5));
        }
        let u1 = rng.gen_range(1e-12_f64..1.);
        let u2 = rng.gen_range(0_f64..1.);
        let e = (-2. * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos() * noise;
        let response = if index == 3 {
            let prob = linear::sigmoid(-0.3 + 1.35 * a - 0.9 * b);
            if rng.gen::<f64>() < prob {
                1.
            } else {
                0.
            }
        } else if index == 4 {
            1. + 2. * (1.5 * a).sin() + 0.5 * b + e
        } else {
            1. + 2.4 * a - 1.3 * b + e
        };
        x.push(row);
        y.push(response);
    }
    Data {
        names: (0..p)
            .map(|j| match j {
                0 => "Signal A".into(),
                1 => "Signal B".into(),
                _ => format!("Noise {}", j - 1),
            })
            .collect(),
        target: if index == 3 {
            "Class (0/1)".into()
        } else {
            "Response".into()
        },
        x,
        y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cancel() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }
    #[test]
    fn registry_has_all_nine_unique_models() {
        let mut keys = std::collections::HashSet::new();
        for m in &MODELS {
            assert!(keys.insert(m.id.key()));
            assert_eq!(ModelId::parse(m.id.key()).unwrap(), m.id);
        }
    }
    #[test]
    fn every_model_fits_and_replays() {
        for d in &MODELS {
            let data = scenario(
                if d.task == Task::BinaryClassification {
                    3
                } else {
                    0
                },
                4,
                0.8,
            );
            let cfg = RunConfig {
                model: d.id,
                ..Default::default()
            };
            let run = Run::fit("fixture".into(), data, cfg, cancel()).unwrap();
            assert!(run
                .trace
                .iter()
                .all(|s| s.objective.is_finite() && s.valid_loss.is_finite()));
            let replay = Run::from_json(
                &serde_json::from_str::<Value>(
                    &serde_json::to_string_pretty(&run.to_json()).unwrap(),
                )
                .unwrap(),
                cancel(),
            )
            .unwrap();
            assert_eq!(run.partition, replay.partition);
            assert_eq!(
                run.predictions(run.last_stage()),
                replay.predictions(replay.last_stage())
            );
        }
    }
    #[test]
    fn tampered_trace_and_invalid_shapes_are_rejected() {
        let run = Run::fit(
            "check".into(),
            scenario(0, 2, 1.),
            RunConfig::default(),
            cancel(),
        )
        .unwrap();
        let mut saved = run.to_json();
        saved["trace"][0]["objective"] = json!(123456.);
        assert!(Run::from_json(&saved, cancel()).is_err());
        let mut data = run.data;
        data.x[0].clear();
        assert!(Run::fit(
            "invalid".into(),
            data,
            RunConfig {
                model: ModelId::Forest,
                ..Default::default()
            },
            cancel()
        )
        .is_err());
    }
    #[test]
    fn stratification_preserves_both_classes() {
        let data = scenario(3, 4, 1.);
        let p = partition(&data, Task::BinaryClassification, 11, 0.25).unwrap();
        for rows in [&p.train, &p.valid] {
            assert!(rows.iter().any(|&i| data.y[i] == 0.));
            assert!(rows.iter().any(|&i| data.y[i] == 1.));
        }
        assert!(p.train.iter().all(|i| !p.valid.contains(i)));
        assert_eq!(p.train.len() + p.valid.len(), data.y.len());
    }
    #[test]
    fn classification_metrics_match_hand_values() {
        let m = metrics(
            &[0., 0., 1., 1.],
            &[0.1, 0.8, 0.4, 0.9],
            &[0, 1, 2, 3],
            Task::BinaryClassification,
            0.5,
        );
        assert_eq!(m.confusion, Some([[1, 1], [1, 1]]));
        assert_eq!(m.accuracy, Some(0.5));
        assert_eq!(m.auc, Some(0.75));
        let tied = metrics(
            &[0., 1.],
            &[0.5, 0.5],
            &[0, 1],
            Task::BinaryClassification,
            0.5,
        );
        assert_eq!(tied.auc, Some(0.5));
    }
    #[test]
    fn threshold_changes_decisions_not_probability_fit() {
        let run = Run::fit(
            "binary".into(),
            scenario(3, 7, 1.),
            RunConfig {
                model: ModelId::Linear(LinearMethod::Logistic),
                ..Default::default()
            },
            cancel(),
        )
        .unwrap();
        let a = run.scores(run.last_stage(), 0.2);
        let b = run.scores(run.last_stage(), 0.8);
        assert_eq!(a.1.loss, b.1.loss);
        assert_eq!(a.1.auc, b.1.auc);
        assert_ne!(a.1.confusion, b.1.confusion);
    }
    #[test]
    fn tampered_replay_is_rejected() {
        let run = Run::fit(
            "test".into(),
            scenario(0, 4, 1.),
            RunConfig::default(),
            cancel(),
        )
        .unwrap();
        let mut json = run.to_json();
        json["final_predictions"][0] = json!(1e6);
        assert!(Run::from_json(&json, cancel()).is_err());
    }
}
