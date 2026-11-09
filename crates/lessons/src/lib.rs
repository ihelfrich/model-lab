//! Lessons as data. Everything here is UI-free: the Tauri shell and the
//! packet writer are consumers. Lesson files are untrusted input and are
//! validated before anything runs.
pub mod examples;
pub mod lesson;
pub mod markdown;
pub mod plots;
pub mod template;
