use crate::state::AppState;
use model_lab_lessons::lesson::{Level, LessonError};
use model_lab_lessons::markdown::to_html;
use model_lab_lessons::template::{render, Context};
use serde::Serialize;
use std::sync::atomic::Ordering;

#[derive(Clone, Debug, Serialize)]
pub struct LessonCard {
    pub id: String,
    pub title: String,
    pub level: Level,
    pub estimated_minutes: u32,
    pub prerequisites: Vec<String>,
    pub prerequisite_titles: Vec<String>,
    pub concepts: Vec<String>,
    pub status: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReadPanel {
    pub title: String,
    pub html: String,
    pub derivation_html: Vec<String>,
    pub instructor_html: Option<String>,
    pub missing: Vec<String>,
    pub status: Option<String>,
}

/// The curriculum: every loaded lesson, prerequisites named but never gating.
pub fn list_lessons(state: &AppState) -> Result<Vec<LessonCard>, String> {
    let lessons = state.lessons.read().map_err(|_| "lesson list unavailable")?;
    Ok(lessons
        .iter()
        .map(|l| LessonCard {
            id: l.front.id.clone(),
            title: l.front.title.clone(),
            level: l.front.level,
            estimated_minutes: l.front.estimated_minutes,
            prerequisites: l.front.prerequisites.clone(),
            prerequisite_titles: l
                .front
                .prerequisites
                .iter()
                .map(|p| {
                    lessons
                        .iter()
                        .find(|x| &x.front.id == p)
                        .map(|x| x.front.title.clone())
                        .unwrap_or_else(|| format!("{p} (not installed)"))
                })
                .collect(),
            concepts: l.front.concepts.clone(),
            status: l.front.extra.get("status").cloned(),
        })
        .collect())
}

pub fn lesson_errors(state: &AppState) -> Result<Vec<LessonError>, String> {
    Ok(state.lesson_errors.read().map_err(|_| "lesson list unavailable")?.clone())
}

/// Render the lesson text against `ctx` (empty until an experiment has run:
/// the `[missing: …]` markers then show what the experiment will fill in).
pub fn read_panel_with(state: &AppState, id: &str, ctx: &Context) -> Result<ReadPanel, String> {
    let lesson = state.lesson(id)?;
    let mut missing = vec![];
    let body = render(&lesson.body, ctx);
    missing.extend(body.missing);
    let derivation_html = lesson
        .derivation
        .iter()
        .map(|step| {
            let r = render(step, ctx);
            missing.extend(r.missing);
            to_html(&r.text)
        })
        .collect();
    let instructor_html = if state.instructor.load(Ordering::Relaxed) {
        lesson.instructor_notes.as_ref().map(|n| to_html(&render(n, ctx).text))
    } else {
        None
    };
    missing.sort();
    missing.dedup();
    Ok(ReadPanel {
        title: lesson.front.title.clone(),
        html: to_html(&body.text),
        derivation_html,
        instructor_html,
        missing,
        status: lesson.front.extra.get("status").cloned(),
    })
}

pub fn read_panel(state: &AppState, id: &str) -> Result<ReadPanel, String> {
    read_panel_with(state, id, &Context::empty())
}

pub fn set_instructor(state: &AppState, on: bool) -> Result<bool, String> {
    state.instructor.store(on, Ordering::Relaxed);
    if let Some(r) = state.record.lock().map_err(|_| "record unavailable")?.as_ref() {
        let _ = r.meta_set("instructor", if on { "on" } else { "off" });
    }
    Ok(on)
}

pub fn instructor(state: &AppState) -> bool {
    state.instructor.load(Ordering::Relaxed)
}

pub mod ipc {
    use super::*;
    use tauri::State;

    #[tauri::command]
    pub fn list_lessons(state: State<'_, AppState>) -> Result<Vec<LessonCard>, String> {
        super::list_lessons(&state)
    }
    #[tauri::command]
    pub fn lesson_errors(state: State<'_, AppState>) -> Result<Vec<LessonError>, String> {
        super::lesson_errors(&state)
    }
    #[tauri::command]
    pub fn read_panel(state: State<'_, AppState>, id: String) -> Result<ReadPanel, String> {
        super::read_panel(&state, &id)
    }
    #[tauri::command]
    pub fn set_instructor(state: State<'_, AppState>, on: bool) -> Result<bool, String> {
        super::set_instructor(&state, on)
    }
    #[tauri::command]
    pub fn instructor(state: State<'_, AppState>) -> bool {
        super::instructor(&state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Dirs;
    use std::path::{Path, PathBuf};

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../lessons/tests/fixtures")
    }
    fn app_fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }
    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("model-lab-app-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn read_panel_renders_html_with_math_and_missing_markers() {
        let state = AppState::new(Dirs::under(fixtures(), &tmp("read")));
        let panel = read_panel(&state, "fixture-lesson").unwrap();
        assert!(panel.html.contains("data-tex"), "{}", panel.html);
        assert!(panel.html.contains("[missing: byhand.round.0.mse_before]"), "{}", panel.html);
        assert_eq!(panel.derivation_html.len(), 2);
        assert_eq!(
            panel.missing,
            vec!["byhand.round.0.mse_after".to_string(), "byhand.round.0.mse_before".to_string()]
        );
        assert_eq!(panel.status.as_deref(), Some("draft-for-review"));
    }

    #[test]
    fn curriculum_lists_prerequisite_titles_without_gating() {
        let mut dirs = Dirs::under(fixtures(), &tmp("curriculum"));
        dirs.user_lessons = app_fixtures();
        let state = AppState::new(dirs);
        let cards = list_lessons(&state).unwrap();
        let b = cards.iter().find(|c| c.id == "fixture-dependent").unwrap();
        assert_eq!(b.prerequisite_titles, vec!["A fixture lesson"]);
        assert!(cards.iter().any(|c| c.id == "fixture-lesson"));
    }

    #[test]
    fn instructor_toggle_persists_in_the_record() {
        let data = tmp("instructor");
        let state = AppState::new(Dirs::under(fixtures(), &data));
        assert!(!instructor(&state));
        set_instructor(&state, true).unwrap();
        let again = AppState::new(Dirs::under(fixtures(), &data));
        assert!(instructor(&again));
    }
}
