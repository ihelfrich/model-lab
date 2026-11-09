use crate::commands::experiment::register_view;
use crate::experiment::{experiment_hash, view_of_run};
use crate::jobs::Manifest;
use crate::state::AppState;
use serde_json::Value;
use std::path::Path;

pub fn start_job(state: &AppState, lesson_id: &str, csv_path: &str, overrides: Value) -> Result<String, String> {
    let lesson = state.lesson(lesson_id)?;
    let label = format!("{} on {}", lesson.front.title, Path::new(csv_path).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default());
    let jobs = state.jobs.lock().map_err(|_| "jobs unavailable")?;
    jobs.start(lesson_id, &label, &lesson.experiment, Path::new(csv_path), &overrides)
}

pub fn list_jobs(state: &AppState) -> Result<Vec<Manifest>, String> {
    Ok(state.jobs.lock().map_err(|_| "jobs unavailable")?.list())
}

pub fn resume_job(state: &AppState, job_id: &str) -> Result<String, String> {
    state.jobs.lock().map_err(|_| "jobs unavailable")?.resume(job_id)
}

pub fn cancel_job(state: &AppState, job_id: &str) -> Result<(), String> {
    state.jobs.lock().map_err(|_| "jobs unavailable")?.cancel(job_id)
}

/// Open a completed job's result as a run the Run panel can show.
pub fn open_job_result(state: &AppState, job_id: &str) -> Result<String, String> {
    let (run, manifest) = state.jobs.lock().map_err(|_| "jobs unavailable")?.open_result(job_id)?;
    let hash = experiment_hash(&manifest.lesson_id, &run.config, &run.fingerprint);
    register_view(state, &manifest.lesson_id, view_of_run(run), hash)
}

pub mod ipc {
    use super::*;
    use std::sync::Arc;
    use tauri::State;

    #[tauri::command]
    pub fn start_job(state: State<'_, Arc<AppState>>, lesson_id: String, csv_path: String, overrides: Value) -> Result<String, String> {
        super::start_job(&state, &lesson_id, &csv_path, overrides)
    }
    #[tauri::command]
    pub fn list_jobs(state: State<'_, Arc<AppState>>) -> Result<Vec<Manifest>, String> {
        super::list_jobs(&state)
    }
    #[tauri::command]
    pub fn resume_job(state: State<'_, Arc<AppState>>, job_id: String) -> Result<String, String> {
        super::resume_job(&state, &job_id)
    }
    #[tauri::command]
    pub fn cancel_job(state: State<'_, Arc<AppState>>, job_id: String) -> Result<(), String> {
        super::cancel_job(&state, &job_id)
    }
    #[tauri::command]
    pub fn open_job_result(state: State<'_, Arc<AppState>>, job_id: String) -> Result<String, String> {
        super::open_job_result(&state, &job_id)
    }
}
