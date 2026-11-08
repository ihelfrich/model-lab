use crate::data::Data;
use rand::{rngs::StdRng, seq::SliceRandom};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Method {
    Tree,
    Boosting,
    Stochastic,
    Forest,
}
impl Method {
    pub const ALL: [Self; 4] = [Self::Tree, Self::Boosting, Self::Stochastic, Self::Forest];
    pub fn label(self) -> &'static str {
        match self {
            Self::Tree => "Decision tree",
            Self::Boosting => "Gradient boosting",
            Self::Stochastic => "Stochastic boosting",
            Self::Forest => "Random forest",
        }
    }
    pub fn boosting(self) -> bool {
        matches!(self, Self::Boosting | Self::Stochastic)
    }
}

#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub method: Method,
    pub trees: usize,
    pub depth: usize,
    pub min_leaf: usize,
    pub rate: f64,
    pub fraction: f64,
    pub mtry: usize,
    pub validation: f64,
    pub seed: u64,
    pub patience: usize,
    pub min_relative_improvement: f64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            method: Method::Boosting,
            trees: 80,
            depth: 2,
            min_leaf: 3,
            rate: 0.1,
            fraction: 0.5,
            mtry: 1,
            validation: 0.25,
            seed: 4710,
            patience: 0,
            min_relative_improvement: 0.001,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Node {
    pub value: f64,
    pub n: usize,
    pub sse: f64,
    pub split: Option<(usize, f64, Box<Node>, Box<Node>)>,
}
impl Node {
    pub fn predict(&self, x: &[f64]) -> f64 {
        match &self.split {
            None => self.value,
            Some((j, t, l, r)) => {
                if x[*j] <= *t {
                    l.predict(x)
                } else {
                    r.predict(x)
                }
            }
        }
    }
    pub fn gains(&self, out: &mut [f64]) {
        if let Some((j, _, l, r)) = &self.split {
            out[*j] += (self.sse - l.sse - r.sse).max(0.0);
            l.gains(out);
            r.gains(out);
        }
    }
    pub fn rules(&self, names: &[String], path: String, output: &mut Vec<String>) {
        if let Some((j, t, l, r)) = &self.split {
            let prefix = if path.is_empty() {
                String::new()
            } else {
                format!("{path} AND ")
            };
            l.rules(names, format!("{prefix}{} <= {t:.4}", names[*j]), output);
            r.rules(names, format!("{prefix}{} > {t:.4}", names[*j]), output);
        } else {
            output.push(format!(
                "{}  =>  {:.5}   ({} sampled rows)",
                if path.is_empty() { "All rows" } else { &path },
                self.value,
                self.n
            ));
        }
    }
}

pub fn build_tree(
    x: &[Vec<f64>],
    y: &[f64],
    rows: &[usize],
    depth: usize,
    min_leaf: usize,
    mtry: usize,
    rng: &mut StdRng,
) -> Node {
    let mean = rows.iter().map(|&i| y[i]).sum::<f64>() / rows.len() as f64;
    let sse = rows.iter().map(|&i| (y[i] - mean).powi(2)).sum::<f64>();
    let mut node = Node {
        value: mean,
        n: rows.len(),
        sse,
        split: None,
    };
    if depth == 0 || rows.len() < 2 * min_leaf || sse == 0.0 {
        return node;
    }
    let mut features: Vec<usize> = (0..x[0].len()).collect();
    if mtry < features.len() {
        features.shuffle(rng);
        features.truncate(mtry.max(1));
    }
    let mut best = (sse, 0, 0.0);
    let mut found = false;
    for j in features {
        let mut sorted = rows.to_vec();
        sorted.sort_by(|&a, &b| x[a][j].total_cmp(&x[b][j]));
        // Center outcomes within the node before prefix SSE calculations.
        let total = rows.iter().map(|&i| y[i] - mean).sum::<f64>();
        let total_sq = sse;
        let (mut left, mut left_sq) = (0.0, 0.0);
        for k in 1..sorted.len() {
            let v = y[sorted[k - 1]] - mean;
            left += v;
            left_sq += v * v;
            if k < min_leaf || sorted.len() - k < min_leaf || x[sorted[k - 1]][j] == x[sorted[k]][j]
            {
                continue;
            }
            let right = total - left;
            let cost = (left_sq - left * left / k as f64).max(0.0)
                + (total_sq - left_sq - right * right / (sorted.len() - k) as f64).max(0.0);
            if cost < best.0 - 1e-12 * sse.max(1e-12) {
                let a = x[sorted[k - 1]][j];
                let b = x[sorted[k]][j];
                let midpoint = a + (b - a) / 2.0;
                best = (cost, j, if midpoint >= b { a } else { midpoint });
                found = true;
            }
        }
    }
    if found {
        let (l, r): (Vec<_>, Vec<_>) = rows.iter().copied().partition(|&i| x[i][best.1] <= best.2);
        node.split = Some((
            best.1,
            best.2,
            Box::new(build_tree(x, y, &l, depth - 1, min_leaf, mtry, rng)),
            Box::new(build_tree(x, y, &r, depth - 1, min_leaf, mtry, rng)),
        ));
    }
    node
}

#[derive(Clone)]
pub struct Fit {
    pub data: Data,
    pub cfg: Config,
    pub train: Vec<usize>,
    pub valid: Vec<usize>,
    pub baseline: f64,
    pub trees: Vec<Node>,
    pub samples: Vec<Vec<usize>>,
    pub train_mse: Vec<f64>,
    pub valid_mse: Vec<f64>,
    pub importance: Vec<f64>,
    pub stage_predictions: Vec<Vec<f64>>,
    pub oob_mse: Vec<Option<f64>>,
    pub oob_coverage: Vec<f64>,
}
impl Fit {
    pub fn predict(&self, x: &[f64], stage: usize) -> f64 {
        let b = stage.min(self.trees.len());
        if b == 0 {
            return self.baseline;
        }
        match self.cfg.method {
            Method::Tree => self.trees[0].predict(x),
            Method::Forest => self.trees[..b].iter().map(|t| t.predict(x)).sum::<f64>() / b as f64,
            _ => {
                self.baseline
                    + self.cfg.rate * self.trees[..b].iter().map(|t| t.predict(x)).sum::<f64>()
            }
        }
    }
    pub fn predictions(&self, stage: usize) -> Vec<f64> {
        self.stage_predictions[stage.min(self.trees.len())].clone()
    }
    pub fn best_stage(&self) -> usize {
        (0..self.valid_mse.len())
            .min_by(|&a, &b| self.valid_mse[a].total_cmp(&self.valid_mse[b]))
            .unwrap_or(0)
    }
}
pub(crate) fn mse(y: &[f64], p: &[f64], rows: &[usize]) -> f64 {
    rows.iter().map(|&i| (y[i] - p[i]).powi(2)).sum::<f64>() / rows.len() as f64
}

/// Fit an ensemble to completion. Since engine 0.9 this is the stage-wise
/// stepper in `crate::ensemble` run to the end; see `docs/CHANGED-RESULTS.md`.
pub fn fit(data: Data, cfg: Config, cancel: Arc<AtomicBool>) -> Result<Fit, String> {
    let mut state = crate::ensemble::EnsembleState::new(data, cfg)?;
    while !state.finished() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Training canceled.".into());
        }
        state.step()?;
    }
    Ok(state.into_fit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, SeedableRng};
    fn fixture() -> Data {
        Data {
            names: vec!["x".into()],
            target: "y".into(),
            x: vec![vec![1.], vec![2.], vec![3.], vec![4.]],
            y: vec![2., 4., 8., 10.],
        }
    }
    fn go(data: Data, cfg: Config) -> Fit {
        fit(data, cfg, Arc::new(AtomicBool::new(false))).unwrap()
    }
    #[test]
    fn cart_matches_hand_calculated_split_and_leaf_means() {
        let d = fixture();
        let mut rng = StdRng::seed_from_u64(1);
        let t = build_tree(&d.x, &d.y, &[0, 1, 2, 3], 1, 1, 1, &mut rng);
        assert_eq!(t.split.as_ref().unwrap().1, 2.5);
        assert_eq!(t.predict(&[1.]), 3.);
        assert_eq!(t.predict(&[4.]), 9.);
        assert_eq!(t.sse, 40.);
    }
    #[test]
    fn adjacent_float_values_do_not_create_empty_children() {
        let a = 1e12_f64 - 1.0;
        let b = f64::from_bits(a.to_bits() + 1);
        let x = vec![vec![a], vec![a], vec![b], vec![b]];
        let mut rng = StdRng::seed_from_u64(1);
        let t = build_tree(&x, &[0., 0., 1., 1.], &[0, 1, 2, 3], 1, 1, 1, &mut rng);
        assert_eq!(t.predict(&[a]), 0.0);
        assert_eq!(t.predict(&[b]), 1.0);
        let (_, _, l, r) = t.split.unwrap();
        assert_eq!((l.n, r.n), (2, 2));
    }
    #[test]
    fn residual_sign_and_learning_rate_match_two_hand_updates() {
        let d = fixture();
        let mut rng = StdRng::seed_from_u64(1);
        let t = build_tree(&d.x, &[-4., -2., 2., 4.], &[0, 1, 2, 3], 1, 1, 1, &mut rng);
        let p: Vec<f64> = d.x.iter().map(|x| 6. + 0.5 * t.predict(x)).collect();
        assert_eq!(p, vec![4.5, 4.5, 7.5, 7.5]);
        let residual: Vec<_> = d.y.iter().zip(&p).map(|(y, p)| y - p).collect();
        let t2 = build_tree(&d.x, &residual, &[0, 1, 2, 3], 1, 1, 1, &mut rng);
        let p2: Vec<_> =
            d.x.iter()
                .enumerate()
                .map(|(i, x)| p[i] + 0.5 * t2.predict(x))
                .collect();
        assert_eq!(p2, vec![3.75, 3.75, 8.25, 8.25]);
    }
    #[test]
    fn all_algorithms_replay_and_sampling_contracts() {
        for method in Method::ALL {
            let cfg = Config {
                method,
                trees: 12,
                min_leaf: 1,
                ..Default::default()
            };
            let d = TableHelper::data();
            let a = go(d.clone(), cfg.clone());
            let b = go(d, cfg);
            assert_eq!(a.predictions(12), b.predictions(12));
            for (stage, &loss) in a.train_mse.iter().enumerate() {
                assert!((loss - mse(&a.data.y, &a.predictions(stage), &a.train)).abs() < 1e-9);
                for (i, x) in a.data.x.iter().enumerate() {
                    assert!((a.stage_predictions[stage][i] - a.predict(x, stage)).abs() < 1e-10);
                }
            }
            for s in &a.samples {
                assert!(s
                    .iter()
                    .all(|i| a.train.contains(i) && !a.valid.contains(i)));
                if method == Method::Forest {
                    assert_eq!(s.len(), a.train.len());
                }
                if method == Method::Stochastic {
                    let unique: std::collections::HashSet<_> = s.iter().collect();
                    assert_eq!(unique.len(), s.len());
                    assert_eq!(s.len(), (a.train.len() as f64 * 0.5).floor() as usize);
                }
            }
            if method == Method::Boosting {
                assert!(a.train_mse.windows(2).all(|v| v[1] <= v[0] + 1e-10));
            }
            if method == Method::Forest {
                assert!(a.samples.iter().any(|s| {
                    let u: std::collections::HashSet<_> = s.iter().collect();
                    u.len() < s.len()
                }));
            }
        }
    }
    #[test]
    fn validation_labels_never_affect_training() {
        for method in Method::ALL {
            let cfg = Config {
                method,
                trees: 8,
                ..Default::default()
            };
            let d = TableHelper::data();
            let a = go(d.clone(), cfg.clone());
            let mut altered = d;
            for &i in &a.valid {
                altered.y[i] += 10000.;
            }
            let b = go(altered, cfg);
            assert_eq!(a.predictions(8), b.predictions(8));
            assert_ne!(a.valid_mse, b.valid_mse);
        }
    }
    #[test]
    fn root_split_matches_independent_brute_force_sse_oracle() {
        let mut rng = StdRng::seed_from_u64(314);
        for _ in 0..25 {
            let x: Vec<Vec<f64>> = (0..20)
                .map(|i| vec![i as f64, rng.gen_range(-5.0..5.0)])
                .collect();
            let y: Vec<f64> = (0..20).map(|_| rng.gen_range(-10.0..10.0)).collect();
            let rows: Vec<_> = (0..20).collect();
            let t = build_tree(&x, &y, &rows, 1, 2, 2, &mut rng);
            let mut oracle = f64::INFINITY;
            for j in 0..2 {
                let mut values: Vec<_> = x.iter().map(|v| v[j]).collect();
                values.sort_by(f64::total_cmp);
                for pair in values.windows(2) {
                    let threshold = (pair[0] + pair[1]) / 2.0;
                    let (l, r): (Vec<_>, Vec<_>) =
                        rows.iter().copied().partition(|&i| x[i][j] <= threshold);
                    if l.len() < 2 || r.len() < 2 {
                        continue;
                    }
                    let cost = [l, r]
                        .iter()
                        .map(|part| {
                            let avg = part.iter().map(|&i| y[i]).sum::<f64>() / part.len() as f64;
                            part.iter().map(|&i| (y[i] - avg).powi(2)).sum::<f64>()
                        })
                        .sum::<f64>();
                    oracle = oracle.min(cost);
                }
            }
            let cost = rows
                .iter()
                .map(|&i| (y[i] - t.predict(&x[i])).powi(2))
                .sum::<f64>();
            assert!((cost - oracle).abs() < 1e-8);
        }
    }
    #[test]
    fn depth_and_leaf_size_constraints_hold() {
        let d = TableHelper::data();
        let mut rng = StdRng::seed_from_u64(1);
        let rows: Vec<_> = (0..d.y.len()).collect();
        let t = build_tree(&d.x, &d.y, &rows, 3, 7, 2, &mut rng);
        fn check(t: &Node, depth: usize) {
            assert!(depth <= 3);
            if let Some((_, _, l, r)) = &t.split {
                assert!(l.n >= 7 && r.n >= 7);
                check(l, depth + 1);
                check(r, depth + 1);
            }
        }
        check(&t, 0);
    }
    #[test]
    fn constant_response_and_cancel() {
        let mut d = TableHelper::data();
        d.y.fill(7.0);
        let a = go(d.clone(), Config::default());
        assert!(a.predictions(80).iter().all(|&v| v == 7.0));
        assert!(fit(d, Config::default(), Arc::new(AtomicBool::new(true))).is_err());
    }
    #[test]
    fn csv_to_each_fitted_model_end_to_end() {
        let table = crate::data::Table::parse("sample.csv", include_str!("../../../sample.csv")).unwrap();
        for method in Method::ALL {
            let data = table.dataset(2, &[0, 1]).unwrap();
            let f = go(
                data,
                Config {
                    method,
                    trees: 10,
                    min_leaf: 1,
                    ..Default::default()
                },
            );
            assert_eq!(f.predictions(10).len(), 12);
            assert!(f.predictions(10).iter().all(|v| v.is_finite()));
            assert!(f.valid_mse.iter().all(|v| v.is_finite()));
        }
    }
    #[test]
    fn forest_oob_matches_independent_omitted_tree_vote_oracle() {
        let f = go(
            TableHelper::data(),
            Config {
                method: Method::Forest,
                trees: 15,
                ..Default::default()
            },
        );
        for b in 1..=15 {
            let mut losses = vec![];
            for &i in &f.train {
                let votes: Vec<_> = (0..b)
                    .filter(|&k| !f.samples[k].contains(&i))
                    .map(|k| f.trees[k].predict(&f.data.x[i]))
                    .collect();
                if !votes.is_empty() {
                    losses.push(
                        (f.data.y[i] - votes.iter().sum::<f64>() / votes.len() as f64).powi(2),
                    );
                }
            }
            assert!(
                (f.oob_mse[b].unwrap() - losses.iter().sum::<f64>() / losses.len() as f64).abs()
                    < 1e-10
            );
            assert_eq!(
                f.oob_coverage[b],
                losses.len() as f64 / f.train.len() as f64
            );
        }
    }
    #[test]
    fn early_stopping_obeys_patience_and_cached_predictions_are_real() {
        let mut d = TableHelper::data();
        d.y.fill(7.0);
        let f = go(
            d,
            Config {
                trees: 100,
                patience: 4,
                ..Default::default()
            },
        );
        assert_eq!(f.trees.len(), 4);
        assert_eq!(f.best_stage(), 0);
        for b in 0..=4 {
            for (i, x) in f.data.x.iter().enumerate() {
                assert_eq!(f.stage_predictions[b][i], f.predict(x, b));
            }
        }
    }
    struct TableHelper;
    impl TableHelper {
        fn data() -> Data {
            crate::data::Table::demo(0).dataset(2, &[0, 1]).unwrap()
        }
    }
}
