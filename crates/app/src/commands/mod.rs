//! IPC commands. Each `#[tauri::command]` is a thin wrapper over a function
//! that takes `&AppState`, so the behaviour is tested without a webview.
pub mod experiment;
pub mod jobs;
pub mod lessons;
pub mod record;
