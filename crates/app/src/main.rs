#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use model_lab_app::commands::experiment::ipc as experiment;
use model_lab_app::commands::jobs::ipc as jobs;
use model_lab_app::commands::lessons::ipc as lessons;
use model_lab_app::commands::packet::ipc as packet;
use model_lab_app::commands::record::ipc as record;
use std::sync::Arc;
use model_lab_app::state::{AppState, Dirs};
use std::path::PathBuf;
use tauri::{Emitter, Manager};

fn bundled_lessons(app: &tauri::App) -> PathBuf {
    let shipped = app.path().resource_dir().map(|r| r.join("lessons")).unwrap_or_default();
    if shipped.is_dir() {
        return shipped;
    }
    // Development only: the repository's lessons directory. Release builds
    // read the bundled resources alone.
    if cfg!(debug_assertions) {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lessons");
        if repo.is_dir() {
            return repo;
        }
    }
    shipped
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            let dirs = Dirs::under(bundled_lessons(app), &data);
            let handle = app.handle().clone();
            let sink: model_lab_app::jobs::Sink = Arc::new(move |id: &str, event: &model_lab_supervisor::job::JobEvent| {
                let _ = handle.emit(&format!("job://{id}"), event);
            });
            app.manage(Arc::new(AppState::with_sink(dirs, sink)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            lessons::list_lessons,
            lessons::lesson_errors,
            lessons::read_panel,
            lessons::set_instructor,
            lessons::instructor,
            lessons::lesson_controls,
            experiment::start_experiment,
            experiment::experiment_status,
            experiment::cancel_experiment,
            experiment::run_panel,
            experiment::read_panel_for_run,
            jobs::start_job,
            jobs::list_jobs,
            jobs::resume_job,
            jobs::cancel_job,
            jobs::open_job_result,
            record::check_panel,
            record::commit_prediction,
            record::answer_question,
            record::export_record,
            packet::export_packet,
            packet::save_figure,
        ])
        .build(tauri::generate_context!())
        .expect("Model Lab could not start its window")
        .run(|app, event| {
            // Tauri exits the process without running destructors: kill
            // the supervised job trees here, leaving their manifests
            // `running` so the next launch offers Resume.
            if let tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<Arc<AppState>>() {
                    if let Ok(mut jobs) = state.jobs.lock() {
                        jobs.shutdown();
                    }
                }
            }
        });
}
