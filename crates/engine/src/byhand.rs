//! The arithmetic of the first lessons, on every row and with nothing
//! hidden: each admissible split of one feature with its SSE reduction, and
//! boosting rounds whose before, correction and after are all exposed.
//! No validation split: four points are the whole dataset.
use crate::data::Data;
use crate::model::{build_tree, Node};
use rand::{rngs::StdRng, SeedableRng};

/// Rounds a lesson may ask for.
pub const MAX_ROUNDS: usize = 50;

#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub threshold: f64,
    pub left: Vec<usize>,
    pub right: Vec<usize>,
    pub left_mean: f64,
    pub right_mean: f64,
    pub sse_after: f64,
    pub reduction: f64,
}

fn mean(y: &[f64], rows: &[usize]) -> f64 {
    rows.iter().map(|&i| y[i]).sum::<f64>() / rows.len() as f64
}

fn sse(y: &[f64], rows: &[usize]) -> f64 {
    let m = mean(y, rows);
    rows.iter().map(|&i| (y[i] - m).powi(2)).sum()
}

/// Every admissible split of `feature` over `rows`: one candidate at the
/// midpoint between each pair of distinct consecutive sorted values, in
/// threshold order. `reduction` is the parent SSE minus the two children's.
pub fn candidate_splits(data: &Data, rows: &[usize], feature: usize) -> Vec<Candidate> {
    if rows.len() < 2 || feature >= data.names.len() {
        return vec![];
    }
    let mut sorted = rows.to_vec();
    sorted.sort_by(|&a, &b| data.x[a][feature].total_cmp(&data.x[b][feature]));
    let parent = sse(&data.y, rows);
    let mut out = vec![];
    for k in 1..sorted.len() {
        let a = data.x[sorted[k - 1]][feature];
        let b = data.x[sorted[k]][feature];
        if a == b {
            continue;
        }
        let threshold = a + (b - a) / 2.0;
        let (left, right): (Vec<_>, Vec<_>) =
            rows.iter().copied().partition(|&i| data.x[i][feature] <= threshold);
        let sse_after = sse(&data.y, &left) + sse(&data.y, &right);
        out.push(Candidate {
            threshold,
            left_mean: mean(&data.y, &left),
            right_mean: mean(&data.y, &right),
            left,
            right,
            sse_after,
            reduction: parent - sse_after,
        });
    }
    out
}

#[derive(Clone, Debug)]
pub struct Round {
    pub stage: usize,
    pub tree: Node,
    pub before: Vec<f64>,
    pub correction: Vec<f64>,
    pub after: Vec<f64>,
    pub residual_before: Vec<f64>,
    pub residual_after: Vec<f64>,
    pub mse_before: f64,
    pub mse_after: f64,
}

fn mse_of(y: &[f64], p: &[f64]) -> f64 {
    y.iter().zip(p).map(|(y, p)| (y - p).powi(2)).sum::<f64>() / y.len() as f64
}

/// Boosting with trees of `depth` on ALL rows (no validation split), minimum
/// leaf 1 and every feature considered at every split, so the random stream
/// is never consulted. Round 0 starts from the mean of `y`;
/// `after = before + rate × tree(x)`.
pub fn boosting_rounds(data: &Data, rate: f64, depth: usize, rounds: usize) -> Result<Vec<Round>, String> {
    let n = data.y.len();
    if n < 2
        || data.x.len() != n
        || data.names.is_empty()
        || data.x.iter().any(|r| r.len() != data.names.len() || r.iter().any(|v| !v.is_finite()))
        || data.y.iter().any(|v| !v.is_finite())
    {
        return Err("The by-hand lesson needs a rectangular finite dataset with at least two rows.".into());
    }
    if !(rate > 0.0 && rate <= 1.0) {
        return Err("The learning rate must be in (0, 1].".into());
    }
    if depth == 0 || depth > 10 {
        return Err("Tree depth must be between 1 and 10.".into());
    }
    if rounds == 0 || rounds > MAX_ROUNDS {
        return Err(format!("Rounds must be between 1 and {MAX_ROUNDS}."));
    }
    let rows: Vec<usize> = (0..n).collect();
    let p = data.names.len();
    let mut rng = StdRng::seed_from_u64(0);
    let start = data.y.iter().sum::<f64>() / n as f64;
    let mut before = vec![start; n];
    let mut out = Vec::with_capacity(rounds);
    for stage in 0..rounds {
        let residual_before: Vec<f64> = data.y.iter().zip(&before).map(|(y, p)| y - p).collect();
        let tree = build_tree(&data.x, &residual_before, &rows, depth, 1, p, &mut rng);
        let correction: Vec<f64> = data.x.iter().map(|x| rate * tree.predict(x)).collect();
        let after: Vec<f64> = before.iter().zip(&correction).map(|(b, c)| b + c).collect();
        let residual_after: Vec<f64> = data.y.iter().zip(&after).map(|(y, p)| y - p).collect();
        out.push(Round {
            stage,
            tree,
            mse_before: mse_of(&data.y, &before),
            mse_after: mse_of(&data.y, &after),
            before: before.clone(),
            correction,
            residual_before,
            residual_after,
            after: after.clone(),
        });
        before = after;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn four_points() -> Data {
        Data {
            names: vec!["x".into()],
            target: "y".into(),
            x: vec![vec![1.], vec![2.], vec![3.], vec![4.]],
            y: vec![2., 4., 8., 10.],
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn four_point_candidates_match_hand_arithmetic() {
        let c = candidate_splits(&four_points(), &[0, 1, 2, 3], 0);
        assert_eq!(c.iter().map(|c| c.threshold).collect::<Vec<_>>(), vec![1.5, 2.5, 3.5]);
        let reductions: Vec<f64> = c.iter().map(|c| c.reduction).collect();
        assert!(close(reductions[0], 64.0 / 3.0), "{reductions:?}");
        assert!(close(reductions[1], 36.0), "{reductions:?}");
        assert!(close(reductions[2], 64.0 / 3.0), "{reductions:?}");
        assert_eq!(c[1].left_mean, 3.0);
        assert_eq!(c[1].right_mean, 9.0);
        assert_eq!(c[1].sse_after, 4.0);
        assert_eq!(c[1].left, vec![0, 1]);
    }

    #[test]
    fn four_point_boosting_matches_the_taught_route() {
        let r = boosting_rounds(&four_points(), 0.5, 1, 2).unwrap();
        assert_eq!(r[0].before, vec![6.0; 4]);
        assert_eq!(r[0].residual_before, vec![-4., -2., 2., 4.]);
        assert_eq!(r[0].after, vec![4.5, 4.5, 7.5, 7.5]);
        assert_eq!(r[0].mse_before, 10.0);
        assert_eq!(r[0].mse_after, 3.25);
        assert_eq!(r[1].after, vec![3.75, 3.75, 8.25, 8.25]);
        assert_eq!(r[1].mse_after, 1.5625);
        assert_eq!(r[0].tree.split.as_ref().unwrap().1, 2.5);
    }

    #[test]
    fn boosting_rounds_rejects_bad_settings() {
        let d = four_points();
        assert!(boosting_rounds(&d, 0.0, 1, 2).is_err());
        assert!(boosting_rounds(&d, 1.5, 1, 2).is_err());
        assert!(boosting_rounds(&d, 0.5, 1, 51).is_err());
        assert!(boosting_rounds(&d, 0.5, 0, 2).is_err());
    }

    #[test]
    fn correction_is_rate_times_tree_prediction() {
        let d = four_points();
        for r in boosting_rounds(&d, 0.3, 2, 6).unwrap() {
            for i in 0..4 {
                assert!(((r.after[i] - r.before[i]) - r.correction[i]).abs() < 1e-12);
                assert_eq!(r.correction[i], 0.3 * r.tree.predict(&d.x[i]));
            }
        }
    }
}
