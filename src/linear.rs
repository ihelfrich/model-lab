//! Inspectable linear and binary-response solvers, independent of the GUI.
//! See docs/MODEL-CONTRACTS.md for objective and scaling conventions.
use crate::data::Data;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinearMethod {
    Ols,
    Logistic,
    Ridge,
    Lasso,
    ElasticNet,
}
impl LinearMethod {
    pub const ALL: [Self; 5] = [
        Self::Ols,
        Self::Logistic,
        Self::Ridge,
        Self::Lasso,
        Self::ElasticNet,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Ols => "Ordinary least squares",
            Self::Logistic => "Logistic regression",
            Self::Ridge => "Ridge regression",
            Self::Lasso => "Lasso",
            Self::ElasticNet => "Elastic net",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Ols => "ols",
            Self::Logistic => "logistic",
            Self::Ridge => "ridge",
            Self::Lasso => "lasso",
            Self::ElasticNet => "elastic_net",
        }
    }
    pub fn penalized(self) -> bool {
        matches!(self, Self::Ridge | Self::Lasso | Self::ElasticNet)
    }
    pub fn mixture(self, mix: f64) -> f64 {
        match self {
            Self::Ridge => 0.,
            Self::Lasso => 1.,
            _ => mix,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinearConfig {
    pub method: LinearMethod,
    pub alpha: f64,
    pub mix: f64,
    pub standardize: bool,
    pub tolerance: f64,
    pub max_iterations: usize,
}
impl Default for LinearConfig {
    fn default() -> Self {
        Self {
            method: LinearMethod::Ols,
            alpha: 0.1,
            mix: 0.5,
            standardize: true,
            tolerance: 1e-8,
            max_iterations: 3000,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Transform {
    pub means: Vec<f64>,
    pub scales: Vec<f64>,
}
impl Transform {
    pub fn fit(data: &Data, train: &[usize], standardize: bool) -> Self {
        let p = data.names.len();
        let n = train.len() as f64;
        let means = (0..p)
            .map(|j| train.iter().map(|&i| data.x[i][j]).sum::<f64>() / n)
            .collect::<Vec<_>>();
        let scales = (0..p)
            .map(|j| {
                let s = (train
                    .iter()
                    .map(|&i| (data.x[i][j] - means[j]).powi(2))
                    .sum::<f64>()
                    / n)
                    .sqrt();
                if standardize && s > 1e-14 * means[j].abs().max(1.) {
                    s
                } else {
                    1.
                }
            })
            .collect();
        Self { means, scales }
    }
    pub fn row(&self, x: &[f64]) -> Vec<f64> {
        x.iter()
            .enumerate()
            .map(|(j, v)| (v - self.means[j]) / self.scales[j])
            .collect()
    }
    pub fn original(&self, intercept: f64, beta: &[f64]) -> (f64, Vec<f64>) {
        let b = beta
            .iter()
            .zip(&self.scales)
            .map(|(v, s)| v / s)
            .collect::<Vec<_>>();
        (
            intercept - b.iter().zip(&self.means).map(|(v, m)| v * m).sum::<f64>(),
            b,
        )
    }
}

#[derive(Clone, Debug)]
pub struct LinearStep {
    pub iteration: usize,
    pub label: String,
    pub intercept: f64,
    pub beta: Vec<f64>,
    pub objective: f64,
    pub stationarity: f64,
}
#[derive(Clone, Debug)]
pub struct LinearFit {
    pub config: LinearConfig,
    pub transform: Transform,
    pub steps: Vec<LinearStep>,
    pub converged: bool,
    pub rank: usize,
    pub warning: Option<String>,
}
impl LinearFit {
    pub fn step(&self, stage: usize) -> &LinearStep {
        &self.steps[stage.min(self.steps.len() - 1)]
    }
    pub fn eta(&self, x: &[f64], stage: usize) -> f64 {
        let s = self.step(stage);
        s.intercept
            + self
                .transform
                .row(x)
                .iter()
                .zip(&s.beta)
                .map(|(x, b)| x * b)
                .sum::<f64>()
    }
    pub fn predict(&self, x: &[f64], stage: usize) -> f64 {
        let e = self.eta(x, stage);
        if self.config.method == LinearMethod::Logistic {
            sigmoid(e)
        } else {
            e
        }
    }
    pub fn coefficients(&self, stage: usize) -> (f64, Vec<f64>) {
        let s = self.step(stage);
        self.transform.original(s.intercept, &s.beta)
    }
}

pub fn sigmoid(z: f64) -> f64 {
    if z >= 0. {
        1. / (1. + (-z).exp())
    } else {
        let e = z.exp();
        e / (1. + e)
    }
}
pub fn softplus(z: f64) -> f64 {
    z.max(0.) + (-z.abs()).exp().ln_1p()
}
pub fn logistic_loss(eta: f64, y: f64) -> f64 {
    if y == 1. {
        softplus(-eta)
    } else {
        softplus(eta)
    }
}
pub fn soft_threshold(v: f64, threshold: f64) -> f64 {
    v.signum() * (v.abs() - threshold).max(0.)
}

pub fn objective(
    x: &[Vec<f64>],
    y: &[f64],
    intercept: f64,
    beta: &[f64],
    cfg: &LinearConfig,
) -> f64 {
    let loss = x
        .iter()
        .zip(y)
        .map(|(row, &y)| {
            let eta = intercept + row.iter().zip(beta).map(|(x, b)| x * b).sum::<f64>();
            if cfg.method == LinearMethod::Logistic {
                logistic_loss(eta, y)
            } else {
                0.5 * (y - eta).powi(2)
            }
        })
        .sum::<f64>()
        / y.len() as f64;
    if !cfg.method.penalized() {
        return loss;
    }
    let mix = cfg.method.mixture(cfg.mix);
    loss + cfg.alpha * mix * beta.iter().map(|b| b.abs()).sum::<f64>()
        + 0.5 * cfg.alpha * (1. - mix) * beta.iter().map(|b| b * b).sum::<f64>()
}

fn snapshot(
    iteration: usize,
    label: String,
    intercept: f64,
    beta: &[f64],
    x: &[Vec<f64>],
    y: &[f64],
    cfg: &LinearConfig,
    stationarity: f64,
) -> LinearStep {
    LinearStep {
        iteration,
        label,
        intercept,
        beta: beta.to_vec(),
        objective: objective(x, y, intercept, beta, cfg),
        stationarity,
    }
}

fn validate(data: &Data, train: &[usize], cfg: &LinearConfig) -> Result<(), String> {
    let n = data.y.len();
    let p = data.names.len();
    if n < 4
        || data.x.len() != n
        || p == 0
        || p > 64
        || train.len() < 2
        || train.len() > n
        || train.iter().any(|&i| i >= n)
        || train.iter().collect::<std::collections::HashSet<_>>().len() != train.len()
        || data
            .x
            .iter()
            .any(|r| r.len() != p || r.iter().any(|v| !v.is_finite() || v.abs() > 1e12))
        || data.y.iter().any(|v| !v.is_finite() || v.abs() > 1e12)
        || !cfg.alpha.is_finite()
        || cfg.alpha < 0.
        || cfg.alpha > 1e8
        || !cfg.mix.is_finite()
        || !(0.0..=1.0).contains(&cfg.mix)
        || !cfg.tolerance.is_finite()
        || cfg.tolerance <= 0.
        || cfg.max_iterations == 0
        || cfg.max_iterations > 20000
    {
        return Err("Invalid linear-model data, row partition, or solver settings.".into());
    }
    if cfg.method == LinearMethod::Logistic {
        if data.y.iter().any(|&y| y != 0. && y != 1.) {
            return Err("Logistic regression requires a response coded exactly 0 or 1. Choose a binary dataset; labels are never silently converted.".into());
        }
        if !train.iter().any(|&i| data.y[i] == 0.) || !train.iter().any(|&i| data.y[i] == 1.) {
            return Err("The training partition must contain both response classes.".into());
        }
    }
    Ok(())
}

/// Column-pivoted modified Gram-Schmidt with a second orthogonalization pass.
/// Intermediate solutions are genuine projections onto selected column spaces.
fn qr_states(x: &[Vec<f64>], y: &[f64]) -> (Vec<Vec<f64>>, usize) {
    let n = x.len();
    let p = x[0].len();
    let mut columns = (0..p)
        .map(|j| x.iter().map(|r| r[j]).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut permutation = (0..p).collect::<Vec<_>>();
    let mut r = vec![vec![0.; p]; p];
    let mut qy = vec![0.; p];
    let reference = columns.iter().map(|c| dot(c, c).sqrt()).fold(0., f64::max);
    let threshold = reference * 1e-10;
    let mut states = Vec::new();
    let mut rank = 0;
    for k in 0..p.min(n) {
        let pivot = (k..p)
            .max_by(|&a, &b| {
                dot(&columns[a], &columns[a]).total_cmp(&dot(&columns[b], &columns[b]))
            })
            .unwrap();
        columns.swap(k, pivot);
        permutation.swap(k, pivot);
        for row in r.iter_mut().take(k) {
            row.swap(k, pivot);
        }
        let norm = dot(&columns[k], &columns[k]).sqrt();
        if norm <= threshold.max(1e-14) {
            break;
        }
        r[k][k] = norm;
        let q = columns[k].iter().map(|v| v / norm).collect::<Vec<_>>();
        qy[k] = dot(&q, y);
        for j in k + 1..p {
            let mut projection = dot(&q, &columns[j]);
            for (v, a) in columns[j].iter_mut().zip(&q) {
                *v -= projection * a;
            }
            let correction = dot(&q, &columns[j]);
            projection += correction;
            for (v, a) in columns[j].iter_mut().zip(&q) {
                *v -= correction * a;
            }
            r[k][j] = projection;
        }
        rank = k + 1;
        let mut b = vec![0.; p];
        for i in (0..rank).rev() {
            b[i] = (qy[i] - (i + 1..rank).map(|j| r[i][j] * b[j]).sum::<f64>()) / r[i][i];
        }
        let mut original = vec![0.; p];
        for j in 0..rank {
            original[permutation[j]] = b[j];
        }
        states.push(original);
    }
    (states, rank)
}
fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

/// Positive-definite solve for a Newton direction. Jitter is a numerical search
/// aid only; objective/score evaluation remains the unpenalized likelihood.
fn positive_solve(a: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let n = b.len();
    let scale = (0..n).map(|i| a[i][i].abs()).fold(1., f64::max);
    for attempt in 0..8 {
        let jitter = if attempt == 0 {
            0.
        } else {
            scale * 10_f64.powi(attempt - 13)
        };
        let mut l = vec![vec![0.; n]; n];
        let mut good = true;
        for i in 0..n {
            for j in 0..=i {
                let v = a[i][j] + if i == j { jitter } else { 0. }
                    - (0..j).map(|k| l[i][k] * l[j][k]).sum::<f64>();
                if i == j {
                    if v <= 0. || !v.is_finite() {
                        good = false;
                        break;
                    }
                    l[i][j] = v.sqrt();
                } else {
                    l[i][j] = v / l[j][j];
                }
            }
            if !good {
                break;
            }
        }
        if !good {
            continue;
        }
        let mut u = vec![0.; n];
        for i in 0..n {
            u[i] = (b[i] - (0..i).map(|j| l[i][j] * u[j]).sum::<f64>()) / l[i][i];
        }
        let mut v = vec![0.; n];
        for i in (0..n).rev() {
            v[i] = (u[i] - (i + 1..n).map(|j| l[j][i] * v[j]).sum::<f64>()) / l[i][i];
        }
        if v.iter().all(|v| v.is_finite()) {
            return Some(v);
        }
    }
    None
}

pub fn score(x: &[Vec<f64>], y: &[f64], intercept: f64, beta: &[f64], logistic: bool) -> Vec<f64> {
    let mut g = vec![0.; beta.len() + 1];
    let n = y.len() as f64;
    for (row, &target) in x.iter().zip(y) {
        let eta = intercept + dot(row, beta);
        let e = if logistic {
            sigmoid(eta) - target
        } else {
            eta - target
        };
        g[0] += e / n;
        for j in 0..beta.len() {
            g[j + 1] += e * row[j] / n;
        }
    }
    g
}
pub fn kkt(x: &[Vec<f64>], y: &[f64], intercept: f64, beta: &[f64], cfg: &LinearConfig) -> f64 {
    let g = score(x, y, intercept, beta, cfg.method == LinearMethod::Logistic);
    let mix = if cfg.method.penalized() {
        cfg.method.mixture(cfg.mix)
    } else {
        0.
    };
    let alpha = if cfg.method.penalized() {
        cfg.alpha
    } else {
        0.
    };
    beta.iter()
        .enumerate()
        .map(|(j, &b)| {
            let v = g[j + 1] + alpha * (1. - mix) * b;
            if b == 0. {
                (v.abs() - alpha * mix).max(0.)
            } else {
                (v + alpha * mix * b.signum()).abs()
            }
        })
        .fold(g[0].abs(), f64::max)
}

pub fn fit(
    data: &Data,
    train: &[usize],
    cfg: LinearConfig,
    cancel: Arc<AtomicBool>,
) -> Result<LinearFit, String> {
    validate(data, train, &cfg)?;
    let transform = Transform::fit(data, train, cfg.standardize);
    let x = train
        .iter()
        .map(|&i| transform.row(&data.x[i]))
        .collect::<Vec<_>>();
    let y = train.iter().map(|&i| data.y[i]).collect::<Vec<_>>();
    let p = data.names.len();
    let n = y.len() as f64;
    let average = y.iter().sum::<f64>() / n;
    let intercept = if cfg.method == LinearMethod::Logistic {
        (average / (1. - average)).ln()
    } else {
        average
    };
    let mut beta = vec![0.; p];
    let residual0 = kkt(&x, &y, intercept, &beta, &cfg);
    let mut steps = vec![snapshot(
        0,
        "Intercept-only baseline".into(),
        intercept,
        &beta,
        &x,
        &y,
        &cfg,
        residual0,
    )];
    let centered = y.iter().map(|v| v - average).collect::<Vec<_>>();
    let (projection_states, rank) = qr_states(&x, &centered);
    let mut warning = if rank < p {
        Some(format!(
            "Rank-deficient predictor matrix: rank {rank} of {p}. Unpenalized coefficients may not be unique."
        ))
    } else {
        None
    };
    let mut converged = false;
    match cfg.method {
        LinearMethod::Ols => {
            for (i, b) in projection_states.into_iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    return Err("Fit canceled.".into());
                }
                let residual = kkt(&x, &y, intercept, &b, &cfg);
                steps.push(snapshot(
                    i + 1,
                    format!(
                        "QR projection onto {} independent direction{}",
                        i + 1,
                        if i == 0 { "" } else { "s" }
                    ),
                    intercept,
                    &b,
                    &x,
                    &y,
                    &cfg,
                    residual,
                ));
            }
            converged = steps.last().unwrap().stationarity <= cfg.tolerance * (1. + residual0);
            if !converged {
                warning=Some("Least-squares stationarity is above tolerance; inspect predictor conditioning and scaling.".into());
            }
        }
        LinearMethod::Ridge | LinearMethod::Lasso | LinearMethod::ElasticNet => {
            let mix = cfg.method.mixture(cfg.mix);
            let l1 = cfg.alpha * mix;
            let l2 = cfg.alpha * (1. - mix);
            let gram = (0..p)
                .map(|j| {
                    (0..p)
                        .map(|k| x.iter().map(|r| r[j] * r[k]).sum::<f64>() / n)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let correlation = (0..p)
                .map(|j| x.iter().zip(&centered).map(|(r, y)| r[j] * y).sum::<f64>() / n)
                .collect::<Vec<_>>();
            let record_every = (cfg.max_iterations / 300).max(1);
            for sweep in 1..=cfg.max_iterations {
                if cancel.load(Ordering::Relaxed) {
                    return Err("Fit canceled.".into());
                }
                for j in 0..p {
                    let rho = correlation[j]
                        - (0..p)
                            .filter(|&k| k != j)
                            .map(|k| gram[j][k] * beta[k])
                            .sum::<f64>();
                    let denominator = gram[j][j] + l2;
                    beta[j] = if denominator > 1e-24 {
                        soft_threshold(rho, l1) / denominator
                    } else {
                        0.
                    };
                }
                // The centered-design intercept score is zero. Reuse Gram
                // statistics instead of rescanning n observations each sweep.
                let mut residual = (0..p)
                    .map(|j| {
                        let g = (0..p).map(|k| gram[j][k] * beta[k]).sum::<f64>() - correlation[j]
                            + l2 * beta[j];
                        if beta[j] == 0. {
                            (g.abs() - l1).max(0.)
                        } else {
                            (g + l1 * beta[j].signum()).abs()
                        }
                    })
                    .fold(0., f64::max);
                // Gram statistics are efficient, but acceptance checks the
                // actual observation-scale score, including the intercept.
                if residual <= cfg.tolerance * (1. + residual0) || sweep == cfg.max_iterations {
                    residual = residual.max(kkt(&x, &y, intercept, &beta, &cfg));
                }
                converged = residual <= cfg.tolerance * (1. + residual0);
                if sweep <= 12
                    || sweep % record_every == 0
                    || converged
                    || sweep == cfg.max_iterations
                {
                    steps.push(snapshot(
                        sweep,
                        format!("Coordinate-descent sweep {sweep}"),
                        intercept,
                        &beta,
                        &x,
                        &y,
                        &cfg,
                        residual,
                    ));
                }
                if converged {
                    break;
                }
            }
            if !converged {
                warning=Some(format!("Iteration limit reached. KKT residual {:.3e}; estimates have not met the requested tolerance.",steps.last().unwrap().stationarity));
            }
        }
        LinearMethod::Logistic => {
            let mut b0 = intercept;
            for iteration in 1..=cfg.max_iterations.min(300) {
                if cancel.load(Ordering::Relaxed) {
                    return Err("Fit canceled.".into());
                }
                let g = score(&x, &y, b0, &beta, true);
                let residual = g.iter().map(|v| v.abs()).fold(0., f64::max);
                if residual <= cfg.tolerance {
                    converged = true;
                    break;
                }
                let mut h = vec![vec![0.; p + 1]; p + 1];
                for row in &x {
                    let prob = sigmoid(b0 + dot(row, &beta));
                    let weight = prob * (1. - prob) / n;
                    let aug = std::iter::once(1.)
                        .chain(row.iter().copied())
                        .collect::<Vec<_>>();
                    for j in 0..=p {
                        for k in 0..=p {
                            h[j][k] += weight * aug[j] * aug[k];
                        }
                    }
                }
                let Some(direction) = positive_solve(&h, &g) else {
                    warning = Some(
                        "Newton system is numerically singular; no converged likelihood estimate."
                            .into(),
                    );
                    break;
                };
                let old = objective(&x, &y, b0, &beta, &cfg);
                let descent = dot(&g, &direction);
                let mut step = 1.;
                let mut accepted = None;
                for _ in 0..35 {
                    let candidate0 = b0 - step * direction[0];
                    let candidate = beta
                        .iter()
                        .enumerate()
                        .map(|(j, b)| b - step * direction[j + 1])
                        .collect::<Vec<_>>();
                    let value = objective(&x, &y, candidate0, &candidate, &cfg);
                    if value.is_finite() && value <= old - 1e-4 * step * descent {
                        accepted = Some((candidate0, candidate));
                        break;
                    }
                    step *= 0.5;
                }
                let Some((candidate0, candidate)) = accepted else {
                    warning = Some(
                        "Line search could not establish a decreasing likelihood objective.".into(),
                    );
                    break;
                };
                b0 = candidate0;
                beta = candidate;
                let residual = kkt(&x, &y, b0, &beta, &cfg);
                steps.push(snapshot(
                    iteration,
                    format!("Newton update {iteration} · step {step:.3}"),
                    b0,
                    &beta,
                    &x,
                    &y,
                    &cfg,
                    residual,
                ));
                if x.iter()
                    .zip(&y)
                    .all(|(row, &target)| (2. * target - 1.) * (b0 + dot(row, &beta)) > 1e-7)
                {
                    warning=Some("Complete separation: a strict separating direction was found. No finite unpenalized maximum-likelihood estimate exists.".into());
                    break;
                }
                if residual <= cfg.tolerance {
                    converged = true;
                    break;
                }
            }
            if !converged && warning.is_none() {
                warning=Some("Logistic solver reached its iteration limit without satisfying the score tolerance.".into());
            }
        }
    }
    if steps.iter().any(|s| {
        !s.objective.is_finite()
            || !s.intercept.is_finite()
            || s.beta.iter().any(|v| !v.is_finite())
    }) {
        return Err(
            "Nonfinite solver state; rescale predictors or use a better-conditioned design.".into(),
        );
    }
    Ok(LinearFit {
        config: cfg,
        transform,
        steps,
        converged,
        rank,
        warning,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cancel() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }
    fn data() -> Data {
        Data {
            names: vec!["x".into()],
            target: "y".into(),
            x: vec![vec![-2.], vec![-1.], vec![1.], vec![2.]],
            y: vec![-3., -1., 3., 5.],
        }
    }
    fn solve(method: LinearMethod, alpha: f64, mix: f64) -> LinearFit {
        fit(
            &data(),
            &[0, 1, 2, 3],
            LinearConfig {
                method,
                alpha,
                mix,
                standardize: false,
                ..Default::default()
            },
            cancel(),
        )
        .unwrap()
    }
    #[test]
    fn ols_exact_line_and_projection_orthogonality() {
        let f = solve(LinearMethod::Ols, 0., 0.);
        let (b0, b) = f.coefficients(999);
        assert!((b0 - 1.).abs() < 1e-12);
        assert!((b[0] - 2.).abs() < 1e-12);
        assert!(f.converged);
        assert!(f.step(999).stationarity < 1e-12);
    }
    #[test]
    fn orthogonal_penalty_solutions() {
        let ridge = solve(LinearMethod::Ridge, 1., 0.);
        assert!((ridge.step(999).beta[0] - 5. / 3.5).abs() < 1e-12);
        let lasso = solve(LinearMethod::Lasso, 1., 1.);
        assert!((lasso.step(999).beta[0] - 1.6).abs() < 1e-12);
        let zero = solve(LinearMethod::Lasso, 6., 1.);
        assert_eq!(zero.step(999).beta[0], 0.);
    }
    #[test]
    fn elastic_net_endpoints_match() {
        for (mix, method) in [(0., LinearMethod::Ridge), (1., LinearMethod::Lasso)] {
            let a = solve(LinearMethod::ElasticNet, 0.7, mix);
            let b = solve(method, 0.7, mix);
            assert!((a.step(999).beta[0] - b.step(999).beta[0]).abs() < 1e-12);
        }
    }
    #[test]
    fn zero_penalty_matches_ols() {
        for m in [
            LinearMethod::Ridge,
            LinearMethod::Lasso,
            LinearMethod::ElasticNet,
        ] {
            assert!((solve(m, 0., 0.4).step(999).beta[0] - 2.).abs() < 1e-12);
        }
    }
    #[test]
    fn validation_changes_do_not_affect_fit() {
        let mut d = data();
        d.x.push(vec![10000.]);
        d.y.push(-1e9);
        let cfg = LinearConfig {
            method: LinearMethod::Ridge,
            ..Default::default()
        };
        let a = fit(&d, &[0, 1, 2, 3], cfg.clone(), cancel()).unwrap();
        d.x[4][0] = -1e10;
        d.y[4] = 1e10;
        let b = fit(&d, &[0, 1, 2, 3], cfg, cancel()).unwrap();
        assert_eq!(a.transform.means, b.transform.means);
        assert_eq!(a.transform.scales, b.transform.scales);
        assert_eq!(a.step(999).beta, b.step(999).beta);
    }
    #[test]
    fn constant_and_collinear_columns_are_explicit() {
        let mut d = data();
        for row in &mut d.x {
            row.push(row[0] * 2.);
            row.push(7.);
        }
        d.names.extend(["duplicate".into(), "constant".into()]);
        let f = fit(&d, &[0, 1, 2, 3], LinearConfig::default(), cancel()).unwrap();
        assert_eq!(f.rank, 1);
        assert!(f.warning.as_ref().unwrap().contains("Rank"));
        for i in 0..4 {
            assert!((f.predict(&d.x[i], 999) - d.y[i]).abs() < 1e-10);
        }
    }
    #[test]
    fn logistic_stability_and_gradient() {
        assert_eq!(sigmoid(1000.), 1.);
        assert_eq!(sigmoid(-1000.), 0.);
        assert_eq!(logistic_loss(1000., 0.), 1000.);
        let d = data();
        let y = vec![0., 1., 0., 1.];
        let cfg = LinearConfig {
            method: LinearMethod::Logistic,
            ..Default::default()
        };
        let b = vec![0.2];
        let g = score(&d.x, &y, -0.3, &b, true);
        let eps = 1e-5;
        let finite = (objective(&d.x, &y, -0.3, &[b[0] + eps], &cfg)
            - objective(&d.x, &y, -0.3, &[b[0] - eps], &cfg))
            / (2. * eps);
        assert!((g[1] - finite).abs() < 1e-8);
    }
    #[test]
    fn separation_is_not_reported_as_convergence() {
        let mut d = data();
        d.y = vec![0., 0., 1., 1.];
        let f = fit(
            &d,
            &[0, 1, 2, 3],
            LinearConfig {
                method: LinearMethod::Logistic,
                ..Default::default()
            },
            cancel(),
        )
        .unwrap();
        assert!(!f.converged);
        assert!(f.warning.unwrap().contains("separation"));
    }
    #[test]
    fn logistic_nonseparated_fit() {
        let mut d = data();
        d.y = vec![0., 1., 0., 1.];
        let f = fit(
            &d,
            &[0, 1, 2, 3],
            LinearConfig {
                method: LinearMethod::Logistic,
                ..Default::default()
            },
            cancel(),
        )
        .unwrap();
        assert!(f.converged);
        assert!(f.step(999).stationarity < 1e-8);
        assert!(f
            .steps
            .windows(2)
            .all(|s| s[1].objective <= s[0].objective + 1e-12));
    }
    #[test]
    fn invalid_classes_and_cancellation() {
        let d = data();
        assert!(fit(
            &d,
            &[0, 1, 2, 3],
            LinearConfig {
                method: LinearMethod::Logistic,
                ..Default::default()
            },
            cancel()
        )
        .is_err());
        let c = Arc::new(AtomicBool::new(true));
        assert!(fit(&d, &[0, 1, 2, 3], LinearConfig::default(), c).is_err());
    }
}
