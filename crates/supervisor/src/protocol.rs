//! Protocol v3: one JSON object per stdout line, tagged with the job's nonce.
//! Lines without the nonce (or with the wrong version, or malformed) are
//! plain output, so a worker cannot forge progress or completion.

use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};

/// Protocol version carried in every event line.
pub const VERSION: u32 = 3;
/// Environment variable through which a worker receives its nonce.
pub const NONCE_ENV: &str = "MODEL_LAB_NONCE";

/// A fresh 128-bit nonce as 32 lowercase hex characters, from OS randomness.
pub fn nonce() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Kind {
    Progress { units_done: u64, units_total: Option<u64>, unit: String, stage: Option<String> },
    Checkpoint { path: String, sha256: String },
    Completed { result: serde_json::Value },
    Failed { error: String },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Envelope {
    pub v: u32,
    pub nonce: String,
    #[serde(flatten)]
    pub kind: Kind,
}

/// One stdout line, classified.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Event(Envelope),
    Output(String),
}

/// Classify a stdout line. It is an event only when it parses, carries
/// version 3 and the expected nonce; everything else is plain output.
pub fn parse_line(line: &str, nonce: &str) -> Line {
    match serde_json::from_str::<Envelope>(line.trim_end()) {
        Ok(e) if e.v == VERSION && e.nonce == nonce => Line::Event(e),
        _ => Line::Output(line.to_string()),
    }
}

/// Encode an event as one JSON object without a trailing newline.
pub fn encode(nonce: &str, kind: Kind) -> String {
    serde_json::to_string(&Envelope { v: VERSION, nonce: nonce.to_string(), kind })
        .expect("protocol events always serialize")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_progress() {
        let n = nonce();
        let k = Kind::Progress { units_done: 3, units_total: Some(10), unit: "tree".into(), stage: None };
        match parse_line(&encode(&n, k.clone()), &n) {
            Line::Event(e) => assert_eq!(e.kind, k),
            Line::Output(_) => panic!("event parsed as output"),
        }
    }
    #[test]
    fn wrong_nonce_is_plain_output() {
        let line = encode("deadbeef", Kind::Failed { error: "spoof".into() });
        assert!(matches!(parse_line(&line, &nonce()), Line::Output(_)));
    }
    #[test]
    fn right_nonce_malformed_json_is_plain_output() {
        let n = nonce();
        let line = format!("{{\"v\":3,\"nonce\":\"{n}\",\"kind\":\"progress\",\"units_done\":");
        assert!(matches!(parse_line(&line, &n), Line::Output(_)));
    }
    #[test]
    fn wrong_version_is_plain_output() {
        let n = nonce();
        let line = encode(&n, Kind::Failed { error: "x".into() }).replace("\"v\":3", "\"v\":2");
        assert!(line.contains("\"v\":2"), "encode must write v first so the test can rewrite it: {line}");
        assert!(matches!(parse_line(&line, &n), Line::Output(_)));
    }
    #[test]
    fn nonces_are_32_hex_and_distinct() {
        let a = nonce();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, nonce());
    }
}
