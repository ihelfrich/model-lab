//! Checkpoints a job can resume from: written atomically (temporary file,
//! fsync, rename) and read back only when the stored hash matches.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Checkpoint envelope version.
pub const VERSION: u64 = 3;
/// Largest checkpoint file this module will read, as today's study worker enforces.
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
pub enum CheckpointError {
    Io(std::io::Error, PathBuf),
    Corrupt(PathBuf),
    WrongVersion(PathBuf, u64),
}

impl std::fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e, p) => write!(f, "checkpoint {}: {e}", p.display()),
            Self::Corrupt(p) => write!(f, "checkpoint {} is damaged: its contents do not match its hash", p.display()),
            Self::WrongVersion(p, v) => write!(f, "checkpoint {} uses version {v}, not {VERSION}", p.display()),
        }
    }
}
impl std::error::Error for CheckpointError {}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Write `{"v":3,"sha256":…,"document":…}` to `path` through a temporary file,
/// fsync and rename, and return the document hash.
pub fn write_atomic(path: &Path, document: &Value) -> Result<String, CheckpointError> {
    let io = |e: std::io::Error| CheckpointError::Io(e, path.to_path_buf());
    let bytes = serde_json::to_vec(document).map_err(|e| io(std::io::Error::other(e)))?;
    let hash = sha256_hex(&bytes);
    let envelope = serde_json::json!({"v": VERSION, "sha256": hash, "document": document});
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".tmp-{}", std::process::id()));
    let temporary = path.with_file_name(name);
    let result = (|| {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(&serde_json::to_vec(&envelope).map_err(std::io::Error::other)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(io(e));
    }
    Ok(hash)
}

/// Read a checkpoint, accepting it only when the stored hash matches the
/// document bytes.
pub fn read_verified(path: &Path) -> Result<Value, CheckpointError> {
    let io = |e: std::io::Error| CheckpointError::Io(e, path.to_path_buf());
    let size = std::fs::metadata(path).map_err(io)?.len();
    if size > MAX_BYTES {
        return Err(CheckpointError::Corrupt(path.to_path_buf()));
    }
    let bytes = std::fs::read(path).map_err(io)?;
    let envelope: Value = serde_json::from_slice(&bytes).map_err(|_| CheckpointError::Corrupt(path.to_path_buf()))?;
    match envelope.get("v").and_then(Value::as_u64) {
        Some(VERSION) => {}
        Some(v) => return Err(CheckpointError::WrongVersion(path.to_path_buf(), v)),
        None => match envelope.get("schema_version").and_then(Value::as_u64) {
            Some(v) => return Err(CheckpointError::WrongVersion(path.to_path_buf(), v)),
            None => return Err(CheckpointError::Corrupt(path.to_path_buf())),
        },
    }
    let (Some(stored), Some(document)) = (envelope.get("sha256").and_then(Value::as_str), envelope.get("document")) else {
        return Err(CheckpointError::Corrupt(path.to_path_buf()));
    };
    let recomputed = sha256_hex(&serde_json::to_vec(document).map_err(|e| io(std::io::Error::other(e)))?);
    if recomputed != stored {
        return Err(CheckpointError::Corrupt(path.to_path_buf()));
    }
    Ok(document.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("mlsup-ckpt-{}-{}", std::process::id(), rand::random::<u64>()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn write_then_read_round_trips_and_returns_hash() {
        let path = dir().join("state.json");
        let doc = json!({"spec_sha256": "abc", "stage": 17, "values": [1.5, 2.25]});
        let hash = write_atomic(&path, &doc).unwrap();
        assert_eq!(hash, crate::checkpoint::sha256_hex(&serde_json::to_vec(&doc).unwrap()));
        assert_eq!(read_verified(&path).unwrap(), doc);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn truncated_file_is_reported_corrupt_with_path() {
        let path = dir().join("state.json");
        write_atomic(&path, &json!({"stage": 3, "payload": "x".repeat(200)})).unwrap();
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
        match read_verified(&path) {
            Err(CheckpointError::Corrupt(p)) => assert_eq!(p, path),
            other => panic!("expected Corrupt, got {other:?}"),
        }
    }

    #[test]
    fn stale_temporary_file_is_not_read() {
        let path = dir().join("state.json");
        fs::write(path.with_file_name("state.json.tmp-1"), b"junk").unwrap();
        match read_verified(&path) {
            Err(CheckpointError::Io(e, p)) => {
                assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
                assert_eq!(p, path);
            }
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn version_two_study_checkpoint_is_wrong_version_not_corrupt() {
        let path = dir().join("old.json");
        fs::write(&path, r#"{"schema_version":2,"document_wire":"{}","document_sha256":"00"}"#).unwrap();
        match read_verified(&path) {
            Err(CheckpointError::WrongVersion(p, v)) => {
                assert_eq!(p, path);
                assert_eq!(v, 2);
            }
            other => panic!("expected WrongVersion, got {other:?}"),
        }
    }
}
