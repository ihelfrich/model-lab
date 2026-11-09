//! The bundled lessons: every one loads, every template resolves against
//! its own experiment, the taught numbers appear, and the question bar is met.
use model_lab_app::experiment::{build_view, context, stages};
use model_lab_lessons::lesson::{load_all, Lesson};
use model_lab_lessons::template::render;
use std::path::{Path, PathBuf};
use std::sync::{atomic::AtomicBool, Arc};

const LESSONS: &[&str] = &["one-tree", "boosting-by-hand"];
const PLANNED: &[&str] = &["one-tree", "boosting-by-hand", "boosting-on-data", "random-forests"];

fn lessons_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lessons")
}

fn bundled() -> Vec<Lesson> {
    let (lessons, errors) = load_all(&[lessons_dir()]);
    assert!(errors.is_empty(), "{errors:#?}");
    lessons
}

fn lesson(id: &str) -> Lesson {
    bundled().into_iter().find(|l| l.front.id == id).unwrap_or_else(|| panic!("lesson {id} is not bundled"))
}

fn rendered_text(l: &Lesson) -> (String, Vec<String>) {
    let (view, _) = build_view(&l.front.id, &l.experiment, &serde_json::json!({}), Arc::new(AtomicBool::new(false)), 5000).unwrap();
    let last = stages(&view).saturating_sub(1);
    let mut missing = vec![];
    let mut text = String::new();
    let ctx = context(&view, last, None);
    let body = render(&l.body, &ctx);
    text.push_str(&body.text);
    missing.extend(body.missing);
    for step in &l.derivation {
        let r = render(step, &ctx);
        text.push_str(&r.text);
        missing.extend(r.missing);
    }
    for q in &l.questions {
        for part in std::iter::once(&q.stem).chain(q.options.iter()).chain(std::iter::once(&q.explanation)) {
            let r = render(part, &ctx);
            text.push_str(&r.text);
            missing.extend(r.missing);
        }
    }
    for p in &l.predictions {
        let ctx = context(&view, p.stage.unwrap_or(last), p.row);
        for part in std::iter::once(&p.prompt).chain(p.explanation.iter()) {
            let r = render(part, &ctx);
            text.push_str(&r.text);
            missing.extend(r.missing);
        }
        assert!(ctx.get(&p.reveals).is_some(), "{}: prediction {} reveals {} which the experiment does not produce", l.front.id, p.id, p.reveals);
    }
    if let Some(notes) = &l.instructor_notes {
        let r = render(notes, &ctx);
        missing.extend(r.missing);
    }
    missing.sort();
    missing.dedup();
    (text, missing)
}

#[test]
fn every_bundled_lesson_loads() {
    let ids: Vec<String> = bundled().into_iter().map(|l| l.front.id).collect();
    for id in LESSONS {
        assert!(ids.iter().any(|x| x == id), "{id} missing from {ids:?}");
    }
}

#[test]
fn every_template_resolves_against_its_experiment() {
    for id in LESSONS {
        let (_, missing) = rendered_text(&lesson(id));
        assert!(missing.is_empty(), "{id}: unresolved {missing:?}");
    }
}

#[test]
fn boosting_by_hand_numbers_are_the_taught_ones() {
    let (text, _) = rendered_text(&lesson("boosting-by-hand"));
    for n in ["10.0000", "3.2500", "1.5625", "2.5"] {
        assert!(text.contains(n), "{n} not in the rendered lesson");
    }
}

#[test]
fn one_tree_shows_all_three_candidates() {
    let (text, _) = rendered_text(&lesson("one-tree"));
    for n in ["36", "21.33", "2.5"] {
        assert!(text.contains(n), "{n} not in the rendered lesson");
    }
}

#[test]
fn questions_meet_the_bar() {
    for id in LESSONS {
        let l = lesson(id);
        assert!(l.questions.len() >= 8, "{id}: {} questions", l.questions.len());
        assert!(l.predictions.len() >= 1, "{id}: no prediction prompt");
        for q in &l.questions {
            assert!(l.front.concepts.contains(&q.concept), "{id}: {} tags {} which is not a lesson concept", q.id, q.concept);
            assert!(q.explanation.chars().count() >= 80, "{id}: {} explanation too short", q.id);
            assert_eq!(q.options.len(), 4, "{id}: {} has {} options", q.id, q.options.len());
        }
        assert!(l.questions.iter().filter(|q| q.requires_prediction.is_some()).count() >= 2, "{id}: fewer than 2 questions require a prediction");
        assert!(l.questions.iter().filter(|q| q.live).count() >= 3, "{id}: fewer than 3 live questions");
        assert_eq!(l.front.extra.get("status").map(String::as_str), Some("draft-for-review"), "{id}: prose status");
    }
}

#[test]
fn prerequisites_name_existing_lessons_or_planned_ids() {
    for id in LESSONS {
        for p in &lesson(id).front.prerequisites {
            assert!(PLANNED.contains(&p.as_str()), "{id} requires unknown lesson {p}");
        }
    }
}
