//! Time-to-completion from the job's own completed units, reported as a
//! range with the number of units it rests on. Never a bare number, and
//! nothing at all before three units have finished.

use std::time::{Duration, Instant};

/// Fewest completed units before any estimate is offered.
pub const MIN_UNITS: u32 = 3;
/// Most per-unit durations retained (the newest).
pub const MAX_OBSERVATIONS: usize = 1024;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EtaRange {
    pub low: Duration,
    pub high: Duration,
    pub units_observed: u32,
    pub units_remaining: u64,
}

/// Accumulates per-unit durations from monotone `observe` calls.
#[derive(Debug, Clone, Default)]
pub struct Eta {
    last_units: Option<u64>,
    last_at: Option<Instant>,
    per_unit: Vec<Duration>,
}

impl Eta {
    pub fn new() -> Self {
        Self::default()
    }

    /// Per-unit durations currently retained.
    pub fn observations(&self) -> usize {
        self.per_unit.len()
    }

    /// Record that `units_done` units have completed as of `at`. Repeats and
    /// decreases are ignored; several units completing between calls share
    /// the interval evenly.
    pub fn observe(&mut self, units_done: u64, at: Instant) {
        match (self.last_units, self.last_at) {
            (Some(prev), Some(prev_at)) if units_done > prev => {
                let n = units_done - prev;
                let divisor = n.min(u32::MAX as u64) as u32;
                let each = at.saturating_duration_since(prev_at) / divisor;
                // Retain the newest MAX_OBSERVATIONS; a jump larger than that
                // contributes that many equal durations, not one per unit.
                let add = (n as usize).min(MAX_OBSERVATIONS);
                self.per_unit.extend(std::iter::repeat(each).take(add));
                if self.per_unit.len() > MAX_OBSERVATIONS {
                    let excess = self.per_unit.len() - MAX_OBSERVATIONS;
                    self.per_unit.drain(..excess);
                }
                self.last_units = Some(units_done);
                self.last_at = Some(at);
            }
            (Some(_), Some(_)) => {}
            _ => {
                self.last_units = Some(units_done);
                self.last_at = Some(at);
            }
        }
    }

    /// Range for the remaining units, or `None` before `MIN_UNITS` have finished.
    pub fn estimate(&self, units_total: u64, now: Instant) -> Option<EtaRange> {
        let observed = self.per_unit.len() as u32;
        if observed < MIN_UNITS {
            return None;
        }
        let done = self.last_units.unwrap_or(0);
        let remaining = units_total.saturating_sub(done);
        let mut sorted = self.per_unit.clone();
        sorted.sort();
        let (p10, p90) = if sorted.len() < 10 {
            (sorted[0], sorted[sorted.len() - 1])
        } else {
            let idx = |q: f64| ((sorted.len() - 1) as f64 * q).round() as usize;
            (sorted[idx(0.10)], sorted[idx(0.90)])
        };
        let in_progress = now.saturating_duration_since(self.last_at.unwrap_or(now));
        let bound = |per: Duration| (per * (remaining as u32)).saturating_sub(in_progress);
        Some(EtaRange { low: bound(p10), high: bound(p90), units_observed: observed, units_remaining: remaining })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn secs(s: f64) -> Duration {
        Duration::from_secs_f64(s)
    }

    #[test]
    fn no_estimate_before_three_units() {
        let t0 = Instant::now();
        let mut eta = Eta::new();
        eta.observe(0, t0);
        eta.observe(1, t0 + secs(1.));
        eta.observe(2, t0 + secs(2.));
        assert_eq!(eta.estimate(10, t0 + secs(2.)), None);
    }

    #[test]
    fn steady_units_give_tight_range() {
        let t0 = Instant::now();
        let mut eta = Eta::new();
        eta.observe(0, t0);
        for u in 1..=3 {
            eta.observe(u, t0 + secs(u as f64));
        }
        let r = eta.estimate(10, t0 + secs(3.)).unwrap();
        assert!((r.low.as_secs_f64() - 7.).abs() < 1e-3, "{r:?}");
        assert!((r.high.as_secs_f64() - 7.).abs() < 1e-3, "{r:?}");
        assert_eq!(r.units_observed, 3);
        assert_eq!(r.units_remaining, 7);
    }

    #[test]
    fn uneven_units_widen_but_never_go_negative() {
        let t0 = Instant::now();
        let mut eta = Eta::new();
        eta.observe(0, t0);
        let mut t = 0.;
        for u in 1..=8u64 {
            t += if u <= 3 { 10. } else { 1. };
            eta.observe(u, t0 + secs(t));
        }
        let r = eta.estimate(20, t0 + secs(t)).unwrap();
        assert!((r.low.as_secs_f64() - 12.).abs() < 1e-3, "{r:?}");
        assert!((r.high.as_secs_f64() - 120.).abs() < 1e-3, "{r:?}");
        assert_eq!(r.units_observed, 8);
        let later = eta.estimate(20, t0 + secs(t + 200.)).unwrap();
        assert_eq!(later.low, Duration::ZERO);
        assert_eq!(later.high, Duration::ZERO);
    }

    #[test]
    fn a_huge_unit_jump_is_bounded_and_does_not_divide_by_zero() {
        let t0 = Instant::now();
        let mut eta = Eta::new();
        eta.observe(0, t0);
        eta.observe(1u64 << 32, t0 + secs(1.));
        eta.observe((1u64 << 32) + 5_000_000, t0 + secs(2.));
        assert!(eta.observations() <= MAX_OBSERVATIONS);
        let r = eta.estimate(1u64 << 33, t0 + secs(2.)).unwrap();
        assert!(r.low <= r.high);
    }

    #[test]
    fn repeated_units_done_is_ignored() {
        let t0 = Instant::now();
        let mut eta = Eta::new();
        eta.observe(0, t0);
        eta.observe(3, t0 + secs(3.));
        eta.observe(3, t0 + secs(4.));
        let r = eta.estimate(6, t0 + secs(4.)).unwrap();
        assert_eq!(r.units_observed, 3);
        assert_eq!(r.units_remaining, 3);
    }
}
