use model_lab_lessons::lesson::{load_all, load_dir, Kind, Level, Source};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

#[test]
fn valid_lesson_loads_with_every_part() {
    let lesson = load_dir(&fixture("valid-lesson")).unwrap();
    assert_eq!(lesson.front.id, "fixture-lesson");
    assert_eq!(lesson.front.title, "A fixture lesson");
    assert_eq!(lesson.front.level, Level::Beginner);
    assert_eq!(lesson.front.concepts, vec!["residual", "stump"]);
    assert_eq!(lesson.front.extra.get("status").map(String::as_str), Some("draft-for-review"));
    assert!(lesson.body.starts_with("# Boosting"));
    assert_eq!(lesson.derivation.len(), 2);
    assert_eq!(lesson.experiment.kind, Kind::ByHand);
    assert_eq!(lesson.experiment.source, Source::FourPoint);
    assert_eq!(lesson.experiment.byhand.as_ref().unwrap().rounds, 3);
    assert_eq!(lesson.experiment.config.tree.trees, 3);
    assert_eq!(lesson.experiment.config.tree.rate, 0.5);
    assert_eq!(lesson.predictions.len(), 1);
    assert_eq!(lesson.questions.len(), 2);
    assert_eq!(lesson.questions[1].requires_prediction.as_deref(), Some("p1"));
    assert!(lesson.instructor_notes.is_none());
}

#[test]
fn experiment_with_unknown_learner_field_is_rejected() {
    let err = load_dir(&fixture("bad-experiment")).unwrap_err();
    assert!(err.file.ends_with("experiment.json"), "{:?}", err.file);
    assert_eq!(err.field, "learner_may_change[0].field");
}

#[test]
fn question_with_short_explanation_is_rejected() {
    let err = load_dir(&fixture("bad-question")).unwrap_err();
    assert!(err.file.ends_with("questions.json"));
    assert_eq!(err.field, "questions[0].explanation");
}

#[test]
fn lesson_without_front_matter_is_rejected() {
    let err = load_dir(&fixture("no-front-matter")).unwrap_err();
    assert!(err.file.ends_with("lesson.md"));
    assert_eq!(err.field, "front_matter");
}

#[test]
fn user_lessons_override_bundled_by_id() {
    let (lessons, errors) = load_all(&[fixture(""), fixture("user-override")]);
    assert!(errors.iter().all(|e| !e.file.to_string_lossy().contains("valid-lesson")), "{errors:?}");
    let found: Vec<_> = lessons.iter().filter(|l| l.front.id == "fixture-lesson").collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].front.title, "Overridden by the user");
}

#[test]
fn crlf_lesson_files_load_like_lf_ones() {
    // Windows checkouts and Windows-authored lessons use CRLF.
    let src = fixture("valid-lesson");
    let dir = std::env::temp_dir().join(format!("model-lab-crlf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["lesson.md", "derivation.md", "experiment.json", "questions.json"] {
        let text = std::fs::read_to_string(src.join(name)).unwrap().replace('\n', "\r\n");
        std::fs::write(dir.join(name), text).unwrap();
    }
    let lesson = load_dir(&dir).unwrap();
    assert_eq!(lesson.front.id, "fixture-lesson");
    assert_eq!(lesson.derivation.len(), 2);
    assert!(lesson.body.starts_with("# Boosting"));
    assert!(!lesson.body.contains('\r'));
}
