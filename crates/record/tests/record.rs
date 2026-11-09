use model_lab_record::{Record, RecordError};

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("model-lab-record-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn schema_is_created_and_versioned() {
    let path = tmp("schema").join("r.sqlite");
    {
        let r = Record::open(&path).unwrap();
        assert_eq!(r.meta_get("schema_version").unwrap().as_deref(), Some("1"));
    }
    let r = Record::open(&path).unwrap();
    assert_eq!(r.meta_get("schema_version").unwrap().as_deref(), Some("1"));
    r.meta_set("instructor", "on").unwrap();
    assert_eq!(r.meta_get("instructor").unwrap().as_deref(), Some("on"));
}

#[test]
fn mastery_needs_two_distinct_lesson_or_session_pairs() {
    let r = Record::open_in_memory().unwrap();
    let s1 = r.start_session().unwrap();
    let a1 = r.attempt(s1, "boosting-by-hand", "h1").unwrap();
    r.answer(a1, "q1", "residual", &[1], true, true, "x").unwrap();
    r.answer(a1, "q2", "residual", &[0], true, true, "x").unwrap();
    let m = r.mastery().unwrap();
    let residual = m.iter().find(|m| m.concept == "residual").unwrap();
    assert!(!residual.mastered, "{residual:?}");
    let s2 = r.start_session().unwrap();
    let a2 = r.attempt(s2, "boosting-by-hand", "h1").unwrap();
    r.answer(a2, "q1", "residual", &[1], true, true, "x").unwrap();
    let m = r.mastery().unwrap();
    let residual = m.iter().find(|m| m.concept == "residual").unwrap();
    assert!(residual.mastered);
    assert_eq!(residual.evidence.len(), 2);
}

#[test]
fn answers_without_a_prediction_do_not_count() {
    let r = Record::open_in_memory().unwrap();
    for lesson in ["one-tree", "random-forests"] {
        let s = r.start_session().unwrap();
        let a = r.attempt(s, lesson, "h").unwrap();
        r.answer(a, "q", "sse", &[0], true, false, "x").unwrap();
    }
    let m = r.mastery().unwrap();
    let sse = m.iter().find(|m| m.concept == "sse").unwrap();
    assert!(!sse.mastered);
}

#[test]
fn history_keeps_the_rendered_explanation() {
    let r = Record::open_in_memory().unwrap();
    let s = r.start_session().unwrap();
    let a = r.attempt(s, "boosting-by-hand", "hash-rate-0.5").unwrap();
    r.answer(a, "q2", "stump", &[0], true, true, "After one round the MSE is 3.2500.").unwrap();
    r.prediction(a, "p1", "down", "-1.5", true).unwrap();
    let h = r.lesson_history("boosting-by-hand").unwrap();
    assert_eq!(h.attempts.len(), 1);
    assert_eq!(h.attempts[0].experiment_hash, "hash-rate-0.5");
    assert_eq!(h.attempts[0].answers[0].rendered_explanation, "After one round the MSE is 3.2500.");
    assert_eq!(h.attempts[0].predictions[0].predicted, "down");
}

#[test]
fn export_has_no_paths() {
    let dir = tmp("export");
    let path = dir.join("r.sqlite");
    let r = Record::open(&path).unwrap();
    let s = r.start_session().unwrap();
    let a = r.attempt(s, "one-tree", "h").unwrap();
    r.answer(a, "q", "sse", &[0], true, true, "x").unwrap();
    let text = r.export_json().unwrap().to_string();
    assert!(!text.contains(dir.to_str().unwrap()), "{text}");
    assert!(text.contains("one-tree"));
}

#[test]
#[cfg(unix)]
fn read_only_location_is_an_error_not_a_panic() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp("readonly");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let result = Record::open(&dir.join("r.sqlite"));
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    match result {
        Err(RecordError::Io(_)) | Err(RecordError::Sql(_)) => {}
        Ok(_) => panic!("opened a record inside a read-only directory"),
    }
}
