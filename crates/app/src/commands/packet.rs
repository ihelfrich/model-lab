use crate::commands::experiment::view;
use crate::experiment::{context, plots_for, stages, RunView};
use crate::state::AppState;
use model_lab_lessons::packet::{self, PacketInput};
use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

#[derive(Clone, Debug, Serialize)]
pub struct PacketFiles {
    pub dir: PathBuf,
    pub files: Vec<PathBuf>,
    pub figure_ids: Vec<String>,
}

fn csv_of(data: &model_lab_engine::data::Data) -> String {
    let mut out = String::new();
    out.push_str(&data.names.join(","));
    out.push(',');
    out.push_str(&data.target);
    out.push('\n');
    for (x, y) in data.x.iter().zip(&data.y) {
        for v in x {
            out.push_str(&format!("{v},"));
        }
        out.push_str(&format!("{y}\n"));
    }
    out
}

/// Write the packet for a lesson's current run. The instructor packet
/// needs instructor mode; `student_only` forces the student packet.
pub fn export_packet(state: &AppState, lesson_id: &str, run_id: &str, student_only: bool) -> Result<PacketFiles, String> {
    let lesson = state.lesson(lesson_id)?;
    let (v, run_lesson, _) = view(state, run_id)?;
    if run_lesson != lesson_id {
        return Err("That run belongs to another lesson.".into());
    }
    let last = stages(&v).saturating_sub(1);
    let ctx = context(&v, last, None);
    let plots = plots_for(&v, last, 0, None);
    let (results, data_csv) = match &*v {
        RunView::Fitted { run, .. } => (run.to_json(), csv_of(&run.data)),
        RunView::ByHand { data, rounds, .. } => (
            json!({"rounds": rounds.iter().map(|r| json!({"stage": r.stage, "before": r.before, "after": r.after, "mse_before": r.mse_before, "mse_after": r.mse_after})).collect::<Vec<_>>()}),
            csv_of(data),
        ),
    };
    let input = PacketInput {
        context: ctx,
        results,
        data_csv,
        figure_ids: plots.iter().map(|p| p.id.clone()).collect(),
        figure_titles: plots.iter().map(|p| p.title.clone()).collect(),
    };
    let instructor = !student_only && state.instructor.load(Ordering::Relaxed);
    let files = packet::write(&lesson, &input, instructor, &state.dirs.packets)?;
    Ok(PacketFiles { dir: files.dir, files: files.files, figure_ids: input.figure_ids })
}

/// Save one rendered figure into a packet directory the shell just wrote.
pub fn save_figure(state: &AppState, packet_dir: &str, id: &str, svg: &str) -> Result<PathBuf, String> {
    let refuse = || Err("Figures are saved only inside a packet directory.".to_string());
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return refuse();
    }
    // Compare real paths: `starts_with` is component-wise and would accept
    // `<packets>/../elsewhere`.
    let (Ok(dir), Ok(root)) = (std::fs::canonicalize(packet_dir), std::fs::canonicalize(&state.dirs.packets)) else {
        return refuse();
    };
    if dir == root || !dir.starts_with(&root) {
        return refuse();
    }
    let path = dir.join("figures").join(format!("{id}.svg"));
    std::fs::write(&path, svg).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

pub mod ipc {
    use super::*;
    use std::sync::Arc;
    use tauri::State;

    #[tauri::command]
    pub fn export_packet(state: State<'_, Arc<AppState>>, lesson_id: String, run_id: String, student_only: bool) -> Result<PacketFiles, String> {
        super::export_packet(&state, &lesson_id, &run_id, student_only)
    }
    #[tauri::command]
    pub fn save_figure(state: State<'_, Arc<AppState>>, packet_dir: String, id: String, svg: String) -> Result<PathBuf, String> {
        super::save_figure(&state, &packet_dir, &id, &svg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Dirs;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("model-lab-figure-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn save_figure_refuses_paths_that_escape_the_packets_directory() {
        let data = tmp("escape");
        let state = AppState::new(Dirs::under(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../lessons/tests/fixtures"), &data));
        let outside = data.join("elsewhere");
        std::fs::create_dir_all(outside.join("figures")).unwrap();
        let sneaky = format!("{}/../elsewhere", state.dirs.packets.display());
        std::fs::create_dir_all(&state.dirs.packets).unwrap();
        assert!(save_figure(&state, &sneaky, "x", "<svg/>").is_err());
        assert!(!outside.join("figures/x.svg").exists());
        let inside = state.dirs.packets.join("p-1");
        std::fs::create_dir_all(inside.join("figures")).unwrap();
        assert!(save_figure(&state, inside.to_str().unwrap(), "x", "<svg/>").is_ok());
        assert!(save_figure(&state, inside.to_str().unwrap(), "../x", "<svg/>").is_err());
    }
}
