use crate::model::Fit;
use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};
#[derive(Clone, Debug)]
pub struct Metrics {
    pub mse: f64,
    pub rmse: f64,
    pub mae: f64,
    pub r2: Option<f64>,
}
pub fn metrics(y: &[f64], pred: &[f64], rows: &[usize]) -> Metrics {
    let mean = rows.iter().map(|&i| y[i]).sum::<f64>() / rows.len() as f64;
    let sse = rows.iter().map(|&i| (y[i] - pred[i]).powi(2)).sum::<f64>();
    let sst = rows.iter().map(|&i| (y[i] - mean).powi(2)).sum::<f64>();
    let mse = sse / rows.len() as f64;
    Metrics {
        mse,
        rmse: mse.sqrt(),
        mae: rows.iter().map(|&i| (y[i] - pred[i]).abs()).sum::<f64>() / rows.len() as f64,
        r2: if sst > 0.0 {
            Some(1.0 - sse / sst)
        } else {
            None
        },
    }
}
pub struct Analysis {
    pub importance: Vec<(String, f64, f64)>,
    pub feature: usize,
    pub grid: Vec<f64>,
    pub pdp: Vec<f64>,
    pub ice: Vec<Vec<f64>>,
    pub reference_rows: usize,
    pub validation_rows: usize,
    pub stage: usize,
}
pub fn analyze(fit: &Fit, feature: usize, stage: usize) -> Analysis {
    let mut rng = StdRng::seed_from_u64(fit.cfg.seed.wrapping_add(901));
    let mut validation = fit.valid.clone();
    validation.shuffle(&mut rng);
    validation.truncate(200);
    let mut reference = fit.train.clone();
    reference.shuffle(&mut rng);
    reference.truncate(128);
    let baseline = validation
        .iter()
        .map(|&i| (fit.data.y[i] - fit.predict(&fit.data.x[i], stage)).powi(2))
        .sum::<f64>()
        / validation.len() as f64;
    let mut importance = vec![];
    for j in 0..fit.data.names.len() {
        let mut increases = vec![];
        for _ in 0..3 {
            let mut values: Vec<_> = validation.iter().map(|&i| fit.data.x[i][j]).collect();
            values.shuffle(&mut rng);
            let loss = validation
                .iter()
                .zip(values)
                .map(|(&i, v)| {
                    let mut x = fit.data.x[i].clone();
                    x[j] = v;
                    (fit.data.y[i] - fit.predict(&x, stage)).powi(2)
                })
                .sum::<f64>()
                / validation.len() as f64;
            increases.push(loss - baseline);
        }
        let mean = increases.iter().sum::<f64>() / 3.0;
        let sd = (increases.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / 3.0).sqrt();
        importance.push((fit.data.names[j].clone(), mean, sd));
    }
    let lo = reference
        .iter()
        .map(|&i| fit.data.x[i][feature])
        .fold(f64::INFINITY, f64::min);
    let hi = reference
        .iter()
        .map(|&i| fit.data.x[i][feature])
        .fold(f64::NEG_INFINITY, f64::max);
    let grid: Vec<_> = (0..32).map(|k| lo + (hi - lo) * k as f64 / 31.0).collect();
    let all_ice: Vec<Vec<f64>> = reference
        .iter()
        .map(|&i| {
            grid.iter()
                .map(|&v| {
                    let mut x = fit.data.x[i].clone();
                    x[feature] = v;
                    fit.predict(&x, stage)
                })
                .collect()
        })
        .collect();
    let pdp = (0..grid.len())
        .map(|k| all_ice.iter().map(|row| row[k]).sum::<f64>() / all_ice.len() as f64)
        .collect();
    let reference_rows = all_ice.len();
    let ice = all_ice.into_iter().take(12).collect();
    Analysis {
        importance,
        feature,
        grid,
        pdp,
        ice,
        reference_rows,
        validation_rows: validation.len(),
        stage,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metrics_match_hand_arithmetic_and_constant_r2_is_unknown() {
        let m = metrics(&[1., 2., 3.], &[1., 3., 2.], &[0, 1, 2]);
        assert_eq!(m.mse, 2.0 / 3.0);
        assert_eq!(m.mae, 2.0 / 3.0);
        assert_eq!(m.r2, Some(0.0));
        assert_eq!(metrics(&[2., 2.], &[2., 3.], &[0, 1]).r2, None);
    }
    #[test]
    fn pdp_is_reference_ice_average_and_baseline_permutation_is_zero() {
        let d = crate::data::Table::demo(1).dataset(2, &[0, 1]).unwrap();
        let f = crate::model::fit(
            d,
            crate::model::Config {
                trees: 4,
                ..Default::default()
            },
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .unwrap();
        let a = analyze(&f, 0, 0);
        assert!(a.importance.iter().all(|(_, m, _)| m.abs() < 1e-10));
        assert!(a.pdp.iter().all(|v| (*v - f.baseline).abs() < 1e-10));
        let a = analyze(&f, 0, 4);
        assert_eq!(a.grid.len(), 32);
        assert!(a.pdp.iter().all(|v| v.is_finite()));
        // Independently average all reference rows by resetting the same reference sampling.
        let mut rng = StdRng::seed_from_u64(f.cfg.seed.wrapping_add(901));
        let mut v = f.valid.clone();
        v.shuffle(&mut rng);
        v.truncate(200);
        let mut rows = f.train.clone();
        rows.shuffle(&mut rng);
        rows.truncate(128);
        for (k, &value) in a.grid.iter().enumerate() {
            let mean = rows
                .iter()
                .map(|&i| {
                    let mut x = f.data.x[i].clone();
                    x[0] = value;
                    f.predict(&x, 4)
                })
                .sum::<f64>()
                / rows.len() as f64;
            assert!((mean - a.pdp[k]).abs() < 1e-10);
        }
    }
    #[test]
    fn permutation_detects_predictive_feature_but_not_constant_feature() {
        let data = crate::data::Data {
            names: vec!["signal".into(), "constant".into()],
            target: "y".into(),
            x: (0..80).map(|i| vec![i as f64 / 10.0, 0.0]).collect(),
            y: (0..80).map(|i| i as f64 / 5.0).collect(),
        };
        let f = crate::model::fit(
            data,
            crate::model::Config {
                method: crate::model::Method::Tree,
                depth: 8,
                min_leaf: 1,
                ..Default::default()
            },
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .unwrap();
        let a = analyze(&f, 0, 1);
        assert!(a.importance[0].1 > 1.0);
        assert!(a.importance[1].1.abs() < 1e-12);
    }
}
