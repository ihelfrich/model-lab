//! The Check panel: a prediction committed before the reveal, questions
//! unlocked by it, and the learner's record of both. Every number shown
//! comes from the run the learner is looking at, and every stored answer
//! keeps the numbers it was answered against.
use crate::commands::experiment::view;
use crate::experiment::context;
use crate::state::AppState;
use model_lab_lessons::lesson::{Prediction, PredictionKind, Question};
use model_lab_lessons::markdown::to_html;
use model_lab_lessons::template::{render, Context};
use model_lab_record::Record;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize)]
pub struct Reveal {
    pub actual: String,
    pub correct: bool,
    pub explanation_html: String,
    pub saved: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Answered {
    pub chosen: Vec<usize>,
    pub correct: bool,
    pub explanation_html: String,
    pub from_earlier_setting: bool,
    pub saved: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PredictionView {
    pub id: String,
    pub prompt_html: String,
    pub kind: PredictionKind,
    pub committed: Option<String>,
    pub revealed: Option<Reveal>,
}

#[derive(Clone, Debug, Serialize)]
pub struct QuestionView {
    pub id: String,
    pub concept: String,
    pub stem_html: String,
    /// Options in display order; `order[i]` is the original index of the i-th.
    pub options_html: Vec<String>,
    pub order: Vec<usize>,
    pub multiple: bool,
    pub answered: Option<Answered>,
    pub locked_until_prediction: Option<String>,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct LessonHistorySummary {
    pub attempts: usize,
    pub predictions: usize,
    pub answers: usize,
    pub correct_answers: usize,
    pub last_attempt_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CheckPanel {
    pub predictions: Vec<PredictionView>,
    pub questions: Vec<QuestionView>,
    pub record_notice: Option<String>,
    pub history: LessonHistorySummary,
    pub mastered: Vec<String>,
}

/// Per-run check state, authoritative for the run on screen.
#[derive(Default, Clone)]
pub struct CheckState {
    pub attempt: Option<i64>,
    pub predictions: HashMap<String, (String, Reveal)>,
    pub answers: HashMap<String, Answered>,
}

fn with_record<T>(state: &AppState, f: impl FnOnce(&Record) -> Result<T, String>) -> Option<Result<T, String>> {
    let guard = state.record.lock().ok()?;
    guard.as_ref().map(f)
}

fn session_id(state: &AppState, record: &Record) -> Result<i64, String> {
    let mut s = state.session.lock().map_err(|_| "session unavailable")?;
    if let Some(id) = *s {
        return Ok(id);
    }
    let id = record.start_session().map_err(|e| e.to_string())?;
    *s = Some(id);
    Ok(id)
}

/// The record attempt for this run, created on first use.
fn attempt_for(state: &AppState, run_id: &str, lesson_id: &str, hash: &str) -> Option<i64> {
    {
        let runs = state.runs.lock().ok()?;
        if let Some(a) = runs.get(run_id).and_then(|e| e.check.attempt) {
            return Some(a);
        }
    }
    let created = with_record(state, |r| {
        let s = session_id(state, r)?;
        r.attempt(s, lesson_id, hash).map_err(|e| e.to_string())
    })?
    .ok()?;
    if let Ok(mut runs) = state.runs.lock() {
        if let Some(e) = runs.get_mut(run_id) {
            e.check.attempt = Some(created);
        }
    }
    Some(created)
}

fn check_state(state: &AppState, run_id: &str) -> CheckState {
    state.runs.lock().ok().and_then(|r| r.get(run_id).map(|e| e.check.clone())).unwrap_or_default()
}

fn update_check(state: &AppState, run_id: &str, f: impl FnOnce(&mut CheckState)) {
    if let Ok(mut runs) = state.runs.lock() {
        if let Some(e) = runs.get_mut(run_id) {
            f(&mut e.check);
        }
    }
}

fn number_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Number(n) => n.as_f64().map(|f| format!("{}", (f * 1e6).round() / 1e6)).unwrap_or_else(|| n.to_string()),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Score a committed prediction against the revealed quantity.
pub fn score(kind: &PredictionKind, predicted: &str, actual: &serde_json::Value) -> bool {
    match kind {
        PredictionKind::Direction => {
            let v = actual.as_f64().unwrap_or(0.0);
            let dir = if v > 1e-12 { "up" } else if v < -1e-12 { "down" } else { "same" };
            predicted.trim().eq_ignore_ascii_case(dir)
        }
        PredictionKind::Number { tolerance } => match (predicted.trim().parse::<f64>(), actual.as_f64()) {
            (Ok(p), Some(a)) => (p - a).abs() <= *tolerance,
            _ => false,
        },
        PredictionKind::Choice { .. } => match (predicted.trim().parse::<f64>(), actual.as_f64()) {
            (Ok(p), Some(a)) => (p - a).abs() <= 1e-9,
            _ => predicted.trim() == number_text(actual),
        },
    }
}

fn deterministic_order(n: usize, seed: &str) -> Vec<usize> {
    let mut s: u64 = seed.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    let mut order: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let j = (s >> 33) as usize % (i + 1);
        order.swap(i, j);
    }
    order
}

fn run_context(state: &AppState, run_id: &str, stage: Option<usize>, row: Option<usize>) -> Result<(Context, String, String), String> {
    let (v, lesson_id, hash) = view(state, run_id)?;
    let last = crate::experiment::stages(&v).saturating_sub(1);
    Ok((context(&v, stage.unwrap_or(last), row), lesson_id, hash))
}

pub fn check_panel(state: &AppState, lesson_id: &str, run_id: &str) -> Result<CheckPanel, String> {
    let lesson = state.lesson(lesson_id)?;
    let (ctx, run_lesson, hash) = run_context(state, run_id, None, None)?;
    if run_lesson != lesson_id {
        return Err("That run belongs to another lesson.".into());
    }
    let check = check_state(state, run_id);
    let history = with_record(state, |r| r.lesson_history(lesson_id).map_err(|e| e.to_string())).and_then(Result::ok);
    let predictions = lesson
        .predictions
        .iter()
        .map(|p| {
            let committed = check.predictions.get(&p.id);
            PredictionView {
                id: p.id.clone(),
                prompt_html: to_html(&render(&p.prompt, &ctx).text),
                kind: p.kind.clone(),
                committed: committed.map(|(c, _)| c.clone()),
                revealed: committed.map(|(_, r)| r.clone()),
            }
        })
        .collect();
    let questions = lesson
        .questions
        .iter()
        .map(|q| {
            let order = deterministic_order(q.options.len(), &format!("{run_id}:{}", q.id));
            let answered = check.answers.get(&q.id).cloned().or_else(|| {
                // The most recent recorded answer from an earlier attempt.
                history.as_ref().and_then(|h| {
                    h.attempts.iter().find_map(|a| {
                        a.answers.iter().rev().find(|x| x.question_id == q.id).map(|x| Answered {
                            chosen: x.chosen.clone(),
                            correct: x.correct,
                            explanation_html: x.rendered_explanation.clone(),
                            from_earlier_setting: a.experiment_hash != hash,
                            saved: true,
                        })
                    })
                })
            });
            let locked = q.requires_prediction.as_ref().filter(|p| !check.predictions.contains_key(*p)).cloned();
            QuestionView {
                id: q.id.clone(),
                concept: q.concept.clone(),
                stem_html: to_html(&render(&q.stem, &ctx).text),
                options_html: order.iter().map(|&i| to_html(&render(&q.options[i], &ctx).text)).collect(),
                order,
                multiple: q.correct.len() > 1,
                answered,
                locked_until_prediction: locked,
            }
        })
        .collect();
    let summary = history
        .as_ref()
        .map(|h| LessonHistorySummary {
            attempts: h.attempts.len(),
            predictions: h.attempts.iter().map(|a| a.predictions.len()).sum(),
            answers: h.attempts.iter().map(|a| a.answers.len()).sum(),
            correct_answers: h.attempts.iter().flat_map(|a| &a.answers).filter(|x| x.correct).count(),
            last_attempt_ms: h.attempts.first().map(|a| a.started_ms),
        })
        .unwrap_or_default();
    let mastered = with_record(state, |r| r.mastery().map_err(|e| e.to_string()))
        .and_then(Result::ok)
        .map(|m| m.into_iter().filter(|m| m.mastered).map(|m| m.concept).collect())
        .unwrap_or_default();
    Ok(CheckPanel {
        predictions,
        questions,
        record_notice: state.record_error.lock().ok().and_then(|e| e.clone()),
        history: summary,
        mastered,
    })
}

fn find_prediction<'a>(lesson: &'a model_lab_lessons::lesson::Lesson, id: &str) -> Result<&'a Prediction, String> {
    lesson.predictions.iter().find(|p| p.id == id).ok_or_else(|| format!("No prediction {id}"))
}

fn find_question<'a>(lesson: &'a model_lab_lessons::lesson::Lesson, id: &str) -> Result<&'a Question, String> {
    lesson.questions.iter().find(|q| q.id == id).ok_or_else(|| format!("No question {id}"))
}

pub fn commit_prediction(state: &AppState, lesson_id: &str, run_id: &str, prompt_id: &str, predicted: &str) -> Result<Reveal, String> {
    let lesson = state.lesson(lesson_id)?;
    let p = find_prediction(&lesson, prompt_id)?;
    if check_state(state, run_id).predictions.contains_key(prompt_id) {
        return Err("This prediction has already been committed for this run.".into());
    }
    let (ctx, _, hash) = run_context(state, run_id, p.stage, p.row)?;
    let actual = ctx.get(&p.reveals).cloned().ok_or_else(|| format!("The experiment did not produce {}", p.reveals))?;
    let correct = score(&p.kind, predicted, &actual);
    let explanation_html = to_html(&render(p.explanation.as_deref().unwrap_or(""), &ctx).text);
    let actual_text = number_text(&actual);
    let saved = attempt_for(state, run_id, lesson_id, &hash)
        .and_then(|a| with_record(state, |r| r.prediction(a, prompt_id, predicted, &actual_text, correct).map_err(|e| e.to_string())))
        .is_some_and(|r| r.is_ok());
    let reveal = Reveal { actual: actual_text, correct, explanation_html, saved };
    let stored = reveal.clone();
    update_check(state, run_id, |c| {
        c.predictions.insert(prompt_id.to_string(), (predicted.to_string(), stored));
    });
    Ok(reveal)
}

pub fn answer_question(state: &AppState, lesson_id: &str, run_id: &str, question_id: &str, chosen: Vec<usize>) -> Result<Answered, String> {
    let lesson = state.lesson(lesson_id)?;
    let q = find_question(&lesson, question_id)?;
    let check = check_state(state, run_id);
    if check.answers.contains_key(question_id) {
        return Err("This question has already been answered for this run.".into());
    }
    if let Some(p) = &q.requires_prediction {
        if !check.predictions.contains_key(p) {
            let prompt = find_prediction(&lesson, p).map(|p| p.prompt.clone()).unwrap_or_default();
            return Err(format!("Commit your prediction first: {prompt}"));
        }
    }
    let (ctx, _, hash) = run_context(state, run_id, None, None)?;
    let mut chosen = chosen;
    chosen.sort_unstable();
    chosen.dedup();
    if chosen.iter().any(|&c| c >= q.options.len()) {
        return Err("That option does not exist.".into());
    }
    let mut correct_set = q.correct.clone();
    correct_set.sort_unstable();
    let correct = chosen == correct_set;
    // The spec's mastery rule: an answer counts only when a prediction was
    // committed earlier in the same attempt.
    let after_prediction = !check.predictions.is_empty();
    let explanation_html = to_html(&render(&q.explanation, &ctx).text);
    let saved = attempt_for(state, run_id, lesson_id, &hash)
        .and_then(|a| {
            with_record(state, |r| {
                r.answer(a, question_id, &q.concept, &chosen, correct, after_prediction, &explanation_html).map_err(|e| e.to_string())
            })
        })
        .is_some_and(|r| r.is_ok());
    let answered = Answered { chosen, correct, explanation_html, from_earlier_setting: false, saved };
    let stored = answered.clone();
    update_check(state, run_id, |c| {
        c.answers.insert(question_id.to_string(), stored);
    });
    Ok(answered)
}

pub fn export_record(state: &AppState) -> Result<PathBuf, String> {
    let json = with_record(state, |r| r.export_json().map_err(|e| e.to_string())).ok_or("No record is open.")??;
    std::fs::create_dir_all(&state.dirs.packets).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = state.dirs.packets.join(format!("record-{stamp}.json"));
    std::fs::write(&path, serde_json::to_vec_pretty(&json).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(path)
}

pub mod ipc {
    use super::*;
    use std::sync::Arc;
    use tauri::State;

    #[tauri::command]
    pub fn check_panel(state: State<'_, Arc<AppState>>, lesson_id: String, run_id: String) -> Result<CheckPanel, String> {
        super::check_panel(&state, &lesson_id, &run_id)
    }
    #[tauri::command]
    pub fn commit_prediction(state: State<'_, Arc<AppState>>, lesson_id: String, run_id: String, prompt_id: String, predicted: String) -> Result<Reveal, String> {
        super::commit_prediction(&state, &lesson_id, &run_id, &prompt_id, &predicted)
    }
    #[tauri::command]
    pub fn answer_question(state: State<'_, Arc<AppState>>, lesson_id: String, run_id: String, question_id: String, chosen: Vec<usize>) -> Result<Answered, String> {
        super::answer_question(&state, &lesson_id, &run_id, &question_id, chosen)
    }
    #[tauri::command]
    pub fn export_record(state: State<'_, Arc<AppState>>) -> Result<PathBuf, String> {
        super::export_record(&state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::experiment::register_view;
    use crate::experiment::build_view;
    use crate::state::Dirs;
    use std::path::{Path, PathBuf};
    use std::sync::{atomic::AtomicBool, Arc};

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../lessons/tests/fixtures")
    }
    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("model-lab-check-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }
    fn byhand_run(state: &AppState, overrides: serde_json::Value) -> String {
        let lesson = state.lesson("fixture-lesson").unwrap();
        let (view, hash) = build_view("fixture-lesson", &lesson.experiment, &overrides, Arc::new(AtomicBool::new(false)), 5000).unwrap();
        register_view(state, "fixture-lesson", view, hash).unwrap()
    }

    #[test]
    fn direction_prediction_is_scored_from_the_live_quantity() {
        let state = AppState::new(Dirs::under(fixtures(), &tmp("direction")));
        let run = byhand_run(&state, serde_json::json!({}));
        // p1 reveals selected.h for row 1 (x = 2): the first stump's left leaf, −3.
        let reveal = commit_prediction(&state, "fixture-lesson", &run, "p1", "down").unwrap();
        assert!(reveal.correct, "{reveal:?}");
        assert_eq!(reveal.actual, "-3");
        assert!(reveal.saved);
        let panel = check_panel(&state, "fixture-lesson", &run).unwrap();
        assert_eq!(panel.predictions[0].committed.as_deref(), Some("down"));
    }

    #[test]
    fn question_locked_until_its_prediction_is_committed() {
        let state = AppState::new(Dirs::under(fixtures(), &tmp("locked")));
        let run = byhand_run(&state, serde_json::json!({}));
        let err = answer_question(&state, "fixture-lesson", &run, "q2", vec![0]).unwrap_err();
        assert!(err.contains("prediction"), "{err}");
        let panel = check_panel(&state, "fixture-lesson", &run).unwrap();
        assert_eq!(panel.questions[1].locked_until_prediction.as_deref(), Some("p1"));
        commit_prediction(&state, "fixture-lesson", &run, "p1", "up").unwrap();
        let a = answer_question(&state, "fixture-lesson", &run, "q2", vec![0]).unwrap();
        assert!(a.correct);
        let history = state.record.lock().unwrap().as_ref().unwrap().lesson_history("fixture-lesson").unwrap();
        assert!(history.attempts[0].answers[0].after_prediction);
        assert!(history.attempts[0].answers[0].rendered_explanation.contains("<p>"));
    }

    #[test]
    fn answer_keeps_its_rendered_numbers_and_flags_setting_changes() {
        let state = AppState::new(Dirs::under(fixtures(), &tmp("earlier")));
        let first = byhand_run(&state, serde_json::json!({"rate": 0.5}));
        commit_prediction(&state, "fixture-lesson", &first, "p1", "down").unwrap();
        let a = answer_question(&state, "fixture-lesson", &first, "q2", vec![0]).unwrap();
        assert!(a.explanation_html.contains("3.25"), "{}", a.explanation_html);
        let second = byhand_run(&state, serde_json::json!({"rate": 0.25}));
        let panel = check_panel(&state, "fixture-lesson", &second).unwrap();
        let q2 = &panel.questions[1];
        let earlier = q2.answered.as_ref().expect("earlier answer shown");
        assert!(earlier.from_earlier_setting);
        assert!(earlier.explanation_html.contains("3.25"));
        // The current run's own stem renders the new number.
        assert!(q2.stem_html.contains("6.0625"), "{}", q2.stem_html);
    }

    #[test]
    fn unsaved_record_is_reported_not_fatal() {
        let state = AppState::without_record(Dirs::under(fixtures(), &tmp("unsaved")), "Your answers are not being saved: /nowhere: read-only");
        let run = byhand_run(&state, serde_json::json!({}));
        let panel = check_panel(&state, "fixture-lesson", &run).unwrap();
        assert!(panel.record_notice.as_deref().unwrap_or("").contains("not being saved"));
        let reveal = commit_prediction(&state, "fixture-lesson", &run, "p1", "down").unwrap();
        assert!(!reveal.saved);
        let a = answer_question(&state, "fixture-lesson", &run, "q1", vec![1]).unwrap();
        assert!(a.correct);
        assert!(!a.saved);
    }

    #[test]
    fn answers_before_any_prediction_do_not_count_toward_mastery() {
        let state = AppState::new(Dirs::under(fixtures(), &tmp("mastery-prediction")));
        let run = byhand_run(&state, serde_json::json!({}));
        // q1 requires no prediction, but none has been committed in this attempt.
        answer_question(&state, "fixture-lesson", &run, "q1", vec![1]).unwrap();
        let history = state.record.lock().unwrap().as_ref().unwrap().lesson_history("fixture-lesson").unwrap();
        assert!(!history.attempts[0].answers[0].after_prediction);
        commit_prediction(&state, "fixture-lesson", &run, "p1", "down").unwrap();
        let second = byhand_run(&state, serde_json::json!({}));
        commit_prediction(&state, "fixture-lesson", &second, "p1", "down").unwrap();
        answer_question(&state, "fixture-lesson", &second, "q1", vec![1]).unwrap();
        let history = state.record.lock().unwrap().as_ref().unwrap().lesson_history("fixture-lesson").unwrap();
        assert!(history.attempts[0].answers[0].after_prediction);
    }

    #[test]
    fn a_question_is_answered_once_per_run() {
        let state = AppState::new(Dirs::under(fixtures(), &tmp("once")));
        let run = byhand_run(&state, serde_json::json!({}));
        answer_question(&state, "fixture-lesson", &run, "q1", vec![1]).unwrap();
        assert!(answer_question(&state, "fixture-lesson", &run, "q1", vec![0]).is_err());
        commit_prediction(&state, "fixture-lesson", &run, "p1", "down").unwrap();
        assert!(commit_prediction(&state, "fixture-lesson", &run, "p1", "up").is_err());
    }
}
