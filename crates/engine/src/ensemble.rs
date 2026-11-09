//! Stage-wise tree ensembles: one tree per `step`, with a per-tree random
//! stream seeded from (seed, tree) so a resumed run and an uninterrupted run
//! are bit-identical and the bootstrap sample of tree t never depends on
//! depth or on what earlier trees did. Checkpoints hold the trees and the
//! per-stage scalars; predictions, samples and the partition are replayed
//! from them with the same arithmetic in the same order.
use crate::data::Data;
use crate::experiment::data_fingerprint;
use crate::model::{build_tree, mse, Config, Fit, Method, Node};
use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};
use serde_json::{json, Value};

/// Version of the state document written by `to_json`.
pub const STATE_VERSION: u64 = 2;

/// SplitMix64 finaliser: spreads a seed so neighbouring tree indices get
/// unrelated streams.
pub fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The random stream for tree `t`.
pub fn tree_rng(seed: u64, t: usize) -> StdRng {
    StdRng::seed_from_u64(splitmix64(seed) ^ (t as u64 + 1))
}

fn document(data: &Data, cfg: &Config, trees: &[Node], train_mse: &[f64], valid_mse: &[f64], stopped: bool) -> Value {
    json!({
        "v": STATE_VERSION,
        "config": cfg,
        "rows": data.y.len(),
        "data_sha256": data_fingerprint(data),
        "trees": trees,
        "train_mse": train_mse,
        "valid_mse": valid_mse,
        "stopped": stopped,
    })
}

/// The same document `EnsembleState::to_json` writes, derived from a
/// finished `Fit`, so a saved experiment can carry its trees and be reopened
/// by replay (`Run::from_trace`) rather than by refitting.
pub fn state_document(fit: &Fit) -> Value {
    document(&fit.data, &fit.cfg, &fit.trees, &fit.train_mse, &fit.valid_mse, fit.trees.len() < fit.cfg.trees)
}

#[derive(Clone)]
pub struct EnsembleState {
    data: Data,
    cfg: Config,
    train: Vec<usize>,
    valid: Vec<usize>,
    baseline: f64,
    prediction: Vec<f64>,
    forest_sum: Vec<f64>,
    oob_sum: Vec<f64>,
    oob_counts: Vec<usize>,
    trees: Vec<Node>,
    samples: Vec<Vec<usize>>,
    train_mse: Vec<f64>,
    valid_mse: Vec<f64>,
    importance: Vec<f64>,
    stage_predictions: Vec<Vec<f64>>,
    oob_mse: Vec<Option<f64>>,
    oob_coverage: Vec<f64>,
    stopping_best: f64,
    stale: usize,
    stopped: bool,
}

impl EnsembleState {
    /// Validate data and settings exactly as `model::fit` did, draw the
    /// train/validation split (unchanged from 0.8) and record stage 0.
    pub fn new(data: Data, cfg: Config) -> Result<Self, String> {
        if data.x.len() < 4
            || data.x.len() != data.y.len()
            || data.names.is_empty()
            || data.x.iter().any(|x| {
                x.len() != data.names.len() || x.iter().any(|v| !v.is_finite() || v.abs() > 1e12)
            })
            || data.y.iter().any(|v| !v.is_finite() || v.abs() > 1e12)
            || cfg.depth == 0
            || cfg.depth > 10
            || cfg.min_leaf == 0
            || cfg.trees == 0
            || cfg.trees > 300
            || !(0.0..=1.0).contains(&cfg.rate)
            || cfg.rate == 0.0
            || !(0.05..=1.0).contains(&cfg.fraction)
            || !(0.1..=0.5).contains(&cfg.validation)
            || cfg.mtry == 0
            || cfg.mtry > data.names.len()
            || cfg.patience > 100
            || !(0.0..1.0).contains(&cfg.min_relative_improvement)
        {
            return Err("Invalid data or model parameters.".into());
        }
        let mut split_rng = StdRng::seed_from_u64(cfg.seed);
        let mut ids: Vec<_> = (0..data.y.len()).collect();
        ids.shuffle(&mut split_rng);
        let nv = ((ids.len() as f64 * cfg.validation).round() as usize).clamp(1, ids.len() - 2);
        let valid = ids[..nv].to_vec();
        let train = ids[nv..].to_vec();
        let baseline = train.iter().map(|&i| data.y[i]).sum::<f64>() / train.len() as f64;
        let n = data.y.len();
        let prediction = vec![baseline; n];
        let train_mse = vec![mse(&data.y, &prediction, &train)];
        let valid_mse = vec![mse(&data.y, &prediction, &valid)];
        Ok(Self {
            importance: vec![0.0; data.names.len()],
            stopping_best: valid_mse[0],
            data,
            cfg,
            train,
            valid,
            baseline,
            stage_predictions: vec![prediction.clone()],
            prediction,
            forest_sum: vec![0.0; n],
            oob_sum: vec![0.0; n],
            oob_counts: vec![0; n],
            trees: vec![],
            samples: vec![],
            train_mse,
            valid_mse,
            oob_mse: vec![None],
            oob_coverage: vec![0.0],
            stale: 0,
            stopped: false,
        })
    }

    /// Trees fitted so far.
    pub fn stage(&self) -> usize {
        self.trees.len()
    }

    fn planned(&self) -> usize {
        if self.cfg.method == Method::Tree {
            1
        } else {
            self.cfg.trees
        }
    }

    /// True once every planned tree is fitted or early stopping has fired.
    pub fn finished(&self) -> bool {
        self.stopped || self.trees.len() >= self.planned()
    }

    fn draw_sample(&self, rng: &mut StdRng) -> Vec<usize> {
        match self.cfg.method {
            Method::Forest => (0..self.train.len())
                .map(|_| self.train[rng.gen_range(0..self.train.len())])
                .collect(),
            Method::Stochastic => {
                let mut s = self.train.clone();
                s.shuffle(rng);
                let n = ((s.len() as f64 * self.cfg.fraction).floor() as usize).max(1);
                s.truncate(n);
                s
            }
            _ => self.train.clone(),
        }
    }

    fn target(&self) -> Vec<f64> {
        if self.cfg.method.boosting() {
            self.data.y.iter().zip(&self.prediction).map(|(y, p)| y - p).collect()
        } else {
            self.data.y.clone()
        }
    }

    /// Fit exactly one more tree.
    pub fn step(&mut self) -> Result<(), String> {
        if self.finished() {
            return Err("The ensemble is complete; nothing to step.".into());
        }
        let t = self.trees.len();
        let mut rng = tree_rng(self.cfg.seed, t);
        let sample = self.draw_sample(&mut rng);
        let target = self.target();
        let mtry = if self.cfg.method == Method::Forest { self.cfg.mtry } else { self.data.names.len() };
        let tree = build_tree(&self.data.x, &target, &sample, self.cfg.depth, self.cfg.min_leaf, mtry, &mut rng);
        self.apply(tree, sample);
        Ok(())
    }

    /// Account for one fitted tree. Shared by `step` and by checkpoint replay,
    /// so both perform the same arithmetic in the same order.
    fn apply(&mut self, tree: Node, sample: Vec<usize>) {
        let stage = self.trees.len();
        tree.gains(&mut self.importance);
        let mut in_bag = vec![false; self.data.y.len()];
        for &i in &sample {
            in_bag[i] = true;
        }
        for (i, x) in self.data.x.iter().enumerate() {
            let h = tree.predict(x);
            match self.cfg.method {
                Method::Tree => self.prediction[i] = h,
                Method::Forest => {
                    self.forest_sum[i] += h;
                    self.prediction[i] = self.forest_sum[i] / (stage + 1) as f64;
                    if !in_bag[i] {
                        self.oob_sum[i] += h;
                        self.oob_counts[i] += 1;
                    }
                }
                _ => self.prediction[i] += self.cfg.rate * h,
            }
        }
        self.train_mse.push(mse(&self.data.y, &self.prediction, &self.train));
        self.valid_mse.push(mse(&self.data.y, &self.prediction, &self.valid));
        self.stage_predictions.push(self.prediction.clone());
        let covered: Vec<_> = self.train.iter().copied().filter(|&i| self.oob_counts[i] > 0).collect();
        self.oob_coverage.push(covered.len() as f64 / self.train.len() as f64);
        self.oob_mse.push(if covered.is_empty() {
            None
        } else {
            Some(
                covered
                    .iter()
                    .map(|&i| (self.data.y[i] - self.oob_sum[i] / self.oob_counts[i] as f64).powi(2))
                    .sum::<f64>()
                    / covered.len() as f64,
            )
        });
        self.samples.push(sample);
        self.trees.push(tree);
        let loss = *self.valid_mse.last().unwrap();
        if loss < self.stopping_best - self.cfg.min_relative_improvement * self.stopping_best.abs().max(1e-12) {
            self.stopping_best = loss;
            self.stale = 0;
        } else {
            self.stale += 1;
        }
        if self.cfg.method.boosting() && self.cfg.patience > 0 && self.stale >= self.cfg.patience {
            self.stopped = true;
        }
    }

    /// Finish: normalise importance and hand over the `Fit` the rest of the
    /// engine understands.
    pub fn into_fit(self) -> Fit {
        let mut importance = self.importance;
        let sum: f64 = importance.iter().sum();
        if sum > 0.0 {
            for v in &mut importance {
                *v /= sum;
            }
        }
        Fit {
            data: self.data,
            cfg: self.cfg,
            train: self.train,
            valid: self.valid,
            baseline: self.baseline,
            trees: self.trees,
            samples: self.samples,
            train_mse: self.train_mse,
            valid_mse: self.valid_mse,
            importance,
            stage_predictions: self.stage_predictions,
            oob_mse: self.oob_mse,
            oob_coverage: self.oob_coverage,
        }
    }

    /// The resumable document: settings, data fingerprint, the trees and the
    /// per-stage scalars. Predictions, samples and the split are replayed.
    pub fn to_json(&self) -> Value {
        document(&self.data, &self.cfg, &self.trees, &self.train_mse, &self.valid_mse, self.stopped)
    }

    /// Rebuild a state from `to_json` output and the same data, replaying each
    /// stored tree through `apply`; the stored per-stage losses must be
    /// reproduced exactly or the document is rejected.
    pub fn from_json(value: &Value, data: Data) -> Result<Self, String> {
        if value["v"].as_u64() != Some(STATE_VERSION) {
            return Err("Unsupported ensemble state version.".into());
        }
        if value["data_sha256"].as_str() != Some(data_fingerprint(&data).as_str()) {
            return Err("The checkpoint was written for different data.".into());
        }
        let cfg: Config = serde_json::from_value(value["config"].clone()).map_err(|e| e.to_string())?;
        let trees: Vec<Node> = serde_json::from_value(value["trees"].clone()).map_err(|e| e.to_string())?;
        let train_mse: Vec<f64> = serde_json::from_value(value["train_mse"].clone()).map_err(|e| e.to_string())?;
        let valid_mse: Vec<f64> = serde_json::from_value(value["valid_mse"].clone()).map_err(|e| e.to_string())?;
        let mut state = Self::new(data, cfg)?;
        for (t, tree) in trees.into_iter().enumerate() {
            let mut rng = tree_rng(state.cfg.seed, t);
            let sample = state.draw_sample(&mut rng);
            state.apply(tree, sample);
        }
        if state.train_mse != train_mse || state.valid_mse != valid_mse {
            return Err("The checkpoint does not reproduce its recorded losses.".into());
        }
        if value["stopped"].as_bool() == Some(true) {
            state.stopped = true;
        }
        Ok(state)
    }
}
