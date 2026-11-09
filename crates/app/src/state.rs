use model_lab_lessons::lesson::{load_all, Lesson, LessonError};
use model_lab_record::Record;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, RwLock};

/// Where things live on this machine.
#[derive(Clone, Debug)]
pub struct Dirs {
    pub bundled_lessons: PathBuf,
    pub user_lessons: PathBuf,
    pub record: PathBuf,
    pub jobs: PathBuf,
    pub packets: PathBuf,
}

impl Dirs {
    /// Everything under one data directory (tests and the default profile).
    pub fn under(bundled_lessons: PathBuf, data: &std::path::Path) -> Dirs {
        Dirs {
            bundled_lessons,
            user_lessons: data.join("lessons"),
            record: data.join("record"),
            jobs: data.join("jobs"),
            packets: data.join("packets"),
        }
    }
}

pub struct AppState {
    pub dirs: Dirs,
    pub lessons: RwLock<Vec<Lesson>>,
    pub lesson_errors: RwLock<Vec<LessonError>>,
    pub record: Mutex<Option<Record>>,
    pub record_error: Mutex<Option<String>>,
    pub instructor: AtomicBool,
}

impl AppState {
    /// Load lessons from the bundled then the user directory and open the
    /// learner record. A record that cannot be opened is reported, not
    /// fatal: lessons still work, answers are not saved.
    pub fn new(dirs: Dirs) -> AppState {
        let (lessons, lesson_errors) = load_all(&[dirs.bundled_lessons.clone(), dirs.user_lessons.clone()]);
        let record_path = dirs.record.join("default.sqlite");
        let (record, record_error) = match Record::open(&record_path) {
            Ok(r) => (Some(r), None),
            Err(e) => (None, Some(format!("Your answers are not being saved: {}: {e}", record_path.display()))),
        };
        let instructor = record
            .as_ref()
            .and_then(|r| r.meta_get("instructor").ok().flatten())
            .map(|v| v == "on")
            .unwrap_or(false);
        AppState {
            dirs,
            lessons: RwLock::new(lessons),
            lesson_errors: RwLock::new(lesson_errors),
            record: Mutex::new(record),
            record_error: Mutex::new(record_error),
            instructor: AtomicBool::new(instructor),
        }
    }

    /// A state with no record file: for tests of the unsaved path.
    pub fn without_record(dirs: Dirs, reason: &str) -> AppState {
        let state = AppState::new(dirs);
        *state.record.lock().unwrap() = None;
        *state.record_error.lock().unwrap() = Some(reason.to_string());
        state
    }

    pub fn lesson(&self, id: &str) -> Result<Lesson, String> {
        self.lessons
            .read()
            .map_err(|_| "lesson list unavailable".to_string())?
            .iter()
            .find(|l| l.front.id == id)
            .cloned()
            .ok_or_else(|| format!("No lesson with id {id}"))
    }
}
