pub const PRESETS: [(&str, f64); 4] = [
    ("Slow", 2.0),
    ("Normal", 0.5),
    ("Fast", 0.1),
    ("Turbo", 0.02),
];
pub fn due_steps(elapsed: f64, seconds: f64, remaining: usize) -> usize {
    if !elapsed.is_finite() || elapsed < 0.0 || !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    ((elapsed / seconds).floor() as usize).min(remaining)
}
#[cfg(test)]
mod tests {
    #[test]
    fn playback_catches_up_without_extra_per_tree_delay() {
        assert_eq!(super::due_steps(1.0, 0.1, 300), 10);
        assert_eq!(super::due_steps(0.1, 0.02, 300), 5);
        assert_eq!(super::due_steps(100.0, 0.1, 7), 7);
        assert_eq!(super::due_steps(0.01, 2.0, 30), 0);
    }
}
