//! Model Lab job supervisor. Every backend process (native engine jobs,
//! Python workers, student code) runs under it: one process tree per job,
//! killed whole on cancel or exit; nonce-tagged progress that student code
//! cannot spoof; per-job limits; atomic checkpoints; an ETA that is a range.

pub mod protocol;
