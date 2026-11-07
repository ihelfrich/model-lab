//! Presentation only: no animation changes the fitted model or its scores.
pub fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
pub fn interpolate(before: f64, after: f64, t: f64) -> f64 {
    before + (after - before) * ease(t)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn animation_preserves_endpoints_and_signed_direction() {
        assert_eq!(interpolate(8.0, 7.8, 0.0), 8.0);
        assert_eq!(interpolate(8.0, 7.8, 1.0), 7.8);
        for k in 0..100 {
            let t = k as f64 / 100.0;
            let p = interpolate(8.0, 7.8, t);
            assert!((7.8..=8.0).contains(&p));
        }
        assert_eq!(ease(-1.0), 0.0);
        assert_eq!(ease(2.0), 1.0);
    }
}
