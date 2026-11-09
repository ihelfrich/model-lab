//! The learner record: one SQLite file per profile holding sessions,
//! attempts, committed predictions, answers and the mastery derived from
//! them. Never uploaded; exportable as JSON without file paths.
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SCHEMA_VERSION: &str = "1";

#[derive(Debug)]
pub enum RecordError {
    Io(String),
    Sql(String),
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordError::Io(s) => write!(f, "{s}"),
            RecordError::Sql(s) => write!(f, "{s}"),
        }
    }
}

impl From<rusqlite::Error> for RecordError {
    fn from(e: rusqlite::Error) -> Self {
        RecordError::Sql(e.to_string())
    }
}

type R<T> = Result<T, RecordError>;

pub struct Record {
    conn: Connection,
}

#[derive(Clone, Debug, Serialize)]
pub struct PredictionRow {
    pub prompt_id: String,
    pub predicted: String,
    pub actual: String,
    pub correct: bool,
    pub at_ms: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnswerRow {
    pub question_id: String,
    pub concept: String,
    pub chosen: Vec<usize>,
    pub correct: bool,
    pub after_prediction: bool,
    pub rendered_explanation: String,
    pub at_ms: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AttemptRow {
    pub id: i64,
    pub session_id: i64,
    pub experiment_hash: String,
    pub started_ms: i64,
    pub predictions: Vec<PredictionRow>,
    pub answers: Vec<AnswerRow>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LessonHistory {
    pub lesson_id: String,
    pub attempts: Vec<AttemptRow>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Mastery {
    pub concept: String,
    pub mastered: bool,
    pub evidence: Vec<(String, i64)>,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS sessions (id INTEGER PRIMARY KEY, started_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS attempts (
  id INTEGER PRIMARY KEY, session_id INTEGER NOT NULL REFERENCES sessions(id),
  lesson_id TEXT NOT NULL, experiment_hash TEXT NOT NULL, started_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS predictions (
  attempt_id INTEGER NOT NULL REFERENCES attempts(id), prompt_id TEXT NOT NULL,
  predicted TEXT NOT NULL, actual TEXT NOT NULL, correct INTEGER NOT NULL, at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS answers (
  attempt_id INTEGER NOT NULL REFERENCES attempts(id), question_id TEXT NOT NULL,
  concept TEXT NOT NULL, chosen_json TEXT NOT NULL, correct INTEGER NOT NULL,
  after_prediction INTEGER NOT NULL, rendered_explanation TEXT NOT NULL, at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS attempts_lesson ON attempts(lesson_id, started_at);
";

impl Record {
    pub fn open(path: &Path) -> R<Record> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| RecordError::Io(format!("{}: {e}", parent.display())))?;
            }
        }
        let conn = Connection::open(path).map_err(|e| RecordError::Io(format!("{}: {e}", path.display())))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> R<Record> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> R<Record> {
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        conn.execute_batch(SCHEMA)?;
        conn.execute(
            "INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION],
        )?;
        Ok(Record { conn })
    }

    pub fn meta_get(&self, key: &str) -> R<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0))
            .optional()?)
    }

    pub fn meta_set(&self, key: &str, value: &str) -> R<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn start_session(&self) -> R<i64> {
        self.conn.execute("INSERT INTO sessions(started_at) VALUES (?1)", params![now_ms()])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn attempt(&self, session: i64, lesson: &str, experiment_hash: &str) -> R<i64> {
        self.conn.execute(
            "INSERT INTO attempts(session_id, lesson_id, experiment_hash, started_at) VALUES (?1, ?2, ?3, ?4)",
            params![session, lesson, experiment_hash, now_ms()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn prediction(&self, attempt: i64, prompt_id: &str, predicted: &str, actual: &str, correct: bool) -> R<()> {
        self.conn.execute(
            "INSERT INTO predictions(attempt_id, prompt_id, predicted, actual, correct, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![attempt, prompt_id, predicted, actual, correct as i64, now_ms()],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn answer(
        &self,
        attempt: i64,
        question: &str,
        concept: &str,
        chosen: &[usize],
        correct: bool,
        after_prediction: bool,
        rendered_explanation: &str,
    ) -> R<()> {
        self.conn.execute(
            "INSERT INTO answers(attempt_id, question_id, concept, chosen_json, correct, after_prediction, rendered_explanation, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                attempt,
                question,
                concept,
                serde_json::to_string(chosen).unwrap_or_else(|_| "[]".into()),
                correct as i64,
                after_prediction as i64,
                rendered_explanation,
                now_ms()
            ],
        )?;
        Ok(())
    }

    /// Whether `prompt_id` was committed in this attempt.
    pub fn predicted_in(&self, attempt: i64, prompt_id: &str) -> R<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM predictions WHERE attempt_id = ?1 AND prompt_id = ?2",
            params![attempt, prompt_id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    fn predictions_of(&self, attempt: i64) -> R<Vec<PredictionRow>> {
        let mut st = self.conn.prepare(
            "SELECT prompt_id, predicted, actual, correct, at FROM predictions WHERE attempt_id = ?1 ORDER BY at, rowid",
        )?;
        let rows = st.query_map(params![attempt], |r| {
            Ok(PredictionRow {
                prompt_id: r.get(0)?,
                predicted: r.get(1)?,
                actual: r.get(2)?,
                correct: r.get::<_, i64>(3)? != 0,
                at_ms: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    fn answers_of(&self, attempt: i64) -> R<Vec<AnswerRow>> {
        let mut st = self.conn.prepare(
            "SELECT question_id, concept, chosen_json, correct, after_prediction, rendered_explanation, at
             FROM answers WHERE attempt_id = ?1 ORDER BY at, rowid",
        )?;
        let rows = st.query_map(params![attempt], |r| {
            let chosen: String = r.get(2)?;
            Ok(AnswerRow {
                question_id: r.get(0)?,
                concept: r.get(1)?,
                chosen: serde_json::from_str(&chosen).unwrap_or_default(),
                correct: r.get::<_, i64>(3)? != 0,
                after_prediction: r.get::<_, i64>(4)? != 0,
                rendered_explanation: r.get(5)?,
                at_ms: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Attempts of one lesson, newest first, each with its predictions and
    /// answers.
    pub fn lesson_history(&self, lesson: &str) -> R<LessonHistory> {
        let mut st = self.conn.prepare(
            "SELECT id, session_id, experiment_hash, started_at FROM attempts WHERE lesson_id = ?1 ORDER BY started_at DESC, id DESC",
        )?;
        let heads: Vec<(i64, i64, String, i64)> = st
            .query_map(params![lesson], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<_, _>>()?;
        let mut attempts = vec![];
        for (id, session_id, experiment_hash, started_ms) in heads {
            attempts.push(AttemptRow {
                id,
                session_id,
                experiment_hash,
                started_ms,
                predictions: self.predictions_of(id)?,
                answers: self.answers_of(id)?,
            });
        }
        Ok(LessonHistory { lesson_id: lesson.into(), attempts })
    }

    /// A concept is mastered when it has correct answers given after a
    /// committed prediction in at least two distinct (lesson, session)
    /// pairs.
    pub fn mastery(&self) -> R<Vec<Mastery>> {
        let mut st = self.conn.prepare(
            "SELECT DISTINCT a.concept, t.lesson_id, t.session_id
             FROM answers a JOIN attempts t ON t.id = a.attempt_id
             WHERE a.correct = 1 AND a.after_prediction = 1
             ORDER BY a.concept, t.lesson_id, t.session_id",
        )?;
        let rows: Vec<(String, String, i64)> =
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        let concepts: Vec<String> = self
            .conn
            .prepare("SELECT DISTINCT concept FROM answers ORDER BY concept")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;
        Ok(concepts
            .into_iter()
            .map(|concept| {
                let evidence: Vec<(String, i64)> =
                    rows.iter().filter(|r| r.0 == concept).map(|r| (r.1.clone(), r.2)).collect();
                Mastery { mastered: evidence.len() >= 2, concept, evidence }
            })
            .collect())
    }

    /// Everything in the record, without any file path.
    pub fn export_json(&self) -> R<Value> {
        let lessons: Vec<String> = self
            .conn
            .prepare("SELECT DISTINCT lesson_id FROM attempts ORDER BY lesson_id")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;
        let mut histories = vec![];
        for l in lessons {
            histories.push(serde_json::to_value(self.lesson_history(&l)?).unwrap_or(Value::Null));
        }
        let sessions: Vec<Value> = self
            .conn
            .prepare("SELECT id, started_at FROM sessions ORDER BY id")?
            .query_map([], |r| Ok(json!({"id": r.get::<_, i64>(0)?, "started_ms": r.get::<_, i64>(1)?})))?
            .collect::<Result<_, _>>()?;
        Ok(json!({
            "schema_version": SCHEMA_VERSION,
            "exported_ms": now_ms(),
            "sessions": sessions,
            "lessons": histories,
            "mastery": serde_json::to_value(self.mastery()?).unwrap_or(Value::Null),
        }))
    }
}
