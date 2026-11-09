//! Model Lab shell: all application state lives here, behind commands the
//! webview invokes. The webview renders; Rust decides.
pub mod commands;
pub mod experiment;
pub mod state;
