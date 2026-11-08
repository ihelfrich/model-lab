//! The specification file a supervised engine job is started from.
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const VERSION: u32 = 3;
pub const MIN_ROWS: usize = 4;
pub const MAX_ROWS: usize = 50_000;
pub const MAX_CHECKPOINT_EVERY: usize = 100;

#[derive(Debug, Clone, Deserialize)]
pub struct JobSpecFile {
    pub v: u32,
    pub job: String,
    pub label: String,
    pub data_csv: PathBuf,
    pub target: String,
    pub features: Vec<String>,
    pub config: serde_json::Value,
    pub max_rows: usize,
    pub checkpoint_every_trees: usize,
}

/// Read and validate a specification: `v` must be 3, `job` must be `fit`,
/// `checkpoint_every_trees` in 1..=100 and `max_rows` in 4..=50,000.
pub fn load(path: &Path) -> Result<JobSpecFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read job spec {}: {e}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("job spec is not JSON: {e}"))?;
    match value.get("v").and_then(serde_json::Value::as_u64) {
        Some(v) if v == VERSION as u64 => {}
        other => return Err(format!("job spec field v must be {VERSION}, found {other:?}")),
    }
    if value.get("job").and_then(serde_json::Value::as_str) != Some("fit") {
        return Err("job spec field job must be \"fit\"".into());
    }
    let spec: JobSpecFile = serde_json::from_value(value).map_err(|e| format!("job spec is incomplete: {e}"))?;
    if !(1..=MAX_CHECKPOINT_EVERY).contains(&spec.checkpoint_every_trees) {
        return Err(format!("checkpoint_every_trees must be between 1 and {MAX_CHECKPOINT_EVERY}, found {}", spec.checkpoint_every_trees));
    }
    if !(MIN_ROWS..=MAX_ROWS).contains(&spec.max_rows) {
        return Err(format!("max_rows must be between {MIN_ROWS} and {MAX_ROWS}, found {}", spec.max_rows));
    }
    if spec.features.is_empty() {
        return Err("job spec needs at least one feature".into());
    }
    Ok(spec)
}
