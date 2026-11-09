//! One directory per lesson: `lesson.md` (YAML front matter + Markdown),
//! `experiment.json`, `questions.json`, optional `derivation.md` and
//! `instructor.md`. Validation names the file and the field.
use model_lab_engine::experiment::{ModelId, RunConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// Minimum length of a question explanation, in characters.
pub const MIN_EXPLANATION: usize = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Level {
    Beginner,
    Intermediate,
    Advanced,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrontMatter {
    pub id: String,
    pub title: String,
    pub level: Level,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    pub estimated_minutes: u32,
    #[serde(default)]
    pub concepts: Vec<String>,
    /// Any other scalar key, kept verbatim (for example `status`).
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Example(String),
    FourPoint,
    Csv(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Run,
    ByHand,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Learner {
    pub field: String,
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ByHandSettings {
    pub rate: f64,
    pub depth: usize,
    pub rounds: usize,
}

#[derive(Clone, Debug)]
pub struct Experiment {
    pub kind: Kind,
    pub source: Source,
    pub target: String,
    pub features: Vec<String>,
    pub config: RunConfig,
    pub byhand: Option<ByHandSettings>,
    pub learner_may_change: Vec<Learner>,
    pub checkpoint_every_trees: usize,
}

#[derive(Deserialize)]
struct ExperimentFile {
    kind: Kind,
    source: Source,
    target: String,
    features: Vec<String>,
    #[serde(default)]
    config: Value,
    byhand: Option<ByHandSettings>,
    #[serde(default)]
    learner_may_change: Vec<Learner>,
    #[serde(default = "default_checkpoint")]
    checkpoint_every_trees: usize,
}

fn default_checkpoint() -> usize {
    10
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredictionKind {
    Direction,
    Number { tolerance: f64 },
    Choice { options: Vec<String> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prediction {
    pub id: String,
    pub prompt: String,
    pub kind: PredictionKind,
    /// Template path of the quantity the reveal compares against.
    pub reveals: String,
    /// The observation the quantity is read for (when it is `selected.*`).
    #[serde(default)]
    pub row: Option<usize>,
    /// The stage the quantity is read at; the last stage when absent.
    #[serde(default)]
    pub stage: Option<usize>,
    /// Templated text shown with the reveal.
    #[serde(default)]
    pub explanation: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub concept: String,
    pub stem: String,
    pub options: Vec<String>,
    pub correct: Vec<usize>,
    pub explanation: String,
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub requires_prediction: Option<String>,
}

#[derive(Deserialize)]
struct QuestionsFile {
    #[serde(default)]
    predictions: Vec<Prediction>,
    questions: Vec<Question>,
}

#[derive(Clone, Debug)]
pub struct Lesson {
    pub dir: PathBuf,
    pub front: FrontMatter,
    pub body: String,
    pub derivation: Vec<String>,
    pub experiment: Experiment,
    pub predictions: Vec<Prediction>,
    pub questions: Vec<Question>,
    pub instructor_notes: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LessonError {
    pub file: PathBuf,
    pub field: String,
    pub message: String,
}

fn fail(file: &Path, field: &str, message: impl Into<String>) -> LessonError {
    LessonError { file: file.to_path_buf(), field: field.into(), message: message.into() }
}

fn read(file: &Path) -> Result<String, LessonError> {
    std::fs::read_to_string(file).map_err(|e| fail(file, "file", e.to_string()))
}

/// Split `lesson.md` into its YAML front matter and body.
fn split_front_matter(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---")?;
    let rest = rest.strip_prefix("\r\n").or_else(|| rest.strip_prefix('\n'))?;
    let end = rest.find("\n---")?;
    let yaml = &rest[..end];
    let after = &rest[end + 4..];
    let body = after.strip_prefix("\r\n").or_else(|| after.strip_prefix('\n')).unwrap_or(after);
    Some((yaml, body))
}

fn parse_front_matter(file: &Path, yaml: &str) -> Result<FrontMatter, LessonError> {
    let map: BTreeMap<String, serde_yaml_ng::Value> =
        serde_yaml_ng::from_str(yaml).map_err(|e| fail(file, "front_matter", e.to_string()))?;
    let known = ["id", "title", "level", "prerequisites", "estimated_minutes", "concepts"];
    let mut object = serde_json::Map::new();
    let mut extra = BTreeMap::new();
    for (k, v) in map {
        let json: Value = serde_json::to_value(&v).map_err(|e| fail(file, &k, e.to_string()))?;
        if known.contains(&k.as_str()) {
            object.insert(k, json);
        } else {
            let text = match &json {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            extra.insert(k, text);
        }
    }
    let mut front: FrontMatter = serde_json::from_value(Value::Object(object))
        .map_err(|e| fail(file, "front_matter", e.to_string()))?;
    if front.id.is_empty() || !front.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(fail(file, "id", "lesson ids are lowercase words joined by hyphens"));
    }
    if front.title.trim().is_empty() {
        return Err(fail(file, "title", "a lesson needs a title"));
    }
    front.extra = extra;
    Ok(front)
}

/// The settings a lesson may let the learner change: the keys of
/// `RunConfig::to_json`.
pub fn config_keys() -> Vec<String> {
    RunConfig::default()
        .to_json()
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

fn parse_experiment(file: &Path, text: &str) -> Result<Experiment, LessonError> {
    let raw: ExperimentFile =
        serde_json::from_str(text).map_err(|e| fail(file, "experiment", e.to_string()))?;
    if raw.features.is_empty() {
        return Err(fail(file, "features", "at least one feature"));
    }
    let mut seen = HashSet::new();
    for f in &raw.features {
        if !seen.insert(f) {
            return Err(fail(file, "features", format!("feature {f} is listed twice")));
        }
    }
    if raw.features.contains(&raw.target) {
        return Err(fail(file, "target", "the target cannot also be a feature"));
    }
    let mut merged = RunConfig::default().to_json();
    match &raw.config {
        Value::Null => {}
        Value::Object(overrides) => {
            let keys = config_keys();
            for (k, v) in overrides {
                if !keys.contains(k) {
                    return Err(fail(file, &format!("config.{k}"), "not a RunConfig setting"));
                }
                merged[k] = v.clone();
            }
        }
        _ => return Err(fail(file, "config", "config must be an object")),
    }
    let config = RunConfig::from_json(&merged).map_err(|e| fail(file, "config", e))?;
    let keys = config_keys();
    for (i, l) in raw.learner_may_change.iter().enumerate() {
        let field = format!("learner_may_change[{i}].field");
        if !keys.contains(&l.field) {
            return Err(fail(file, &field, format!("{} is not a RunConfig setting", l.field)));
        }
        if !(l.min < l.max) {
            return Err(fail(file, &format!("learner_may_change[{i}].min"), "min must be below max"));
        }
        if !(l.step > 0.0) {
            return Err(fail(file, &format!("learner_may_change[{i}].step"), "step must be positive"));
        }
    }
    if raw.kind == Kind::ByHand {
        if raw.source != Source::FourPoint {
            return Err(fail(file, "source", "a by_hand lesson uses the four_point source"));
        }
        if raw.byhand.is_none() {
            return Err(fail(file, "byhand", "a by_hand lesson needs byhand settings"));
        }
        if let ModelId::Linear(_) = config.model {
            return Err(fail(file, "config.model", "a by_hand lesson is a tree lesson"));
        }
    }
    if !(1..=100).contains(&raw.checkpoint_every_trees) {
        return Err(fail(file, "checkpoint_every_trees", "between 1 and 100"));
    }
    Ok(Experiment {
        kind: raw.kind,
        source: raw.source,
        target: raw.target,
        features: raw.features,
        config,
        byhand: raw.byhand,
        learner_may_change: raw.learner_may_change,
        checkpoint_every_trees: raw.checkpoint_every_trees,
    })
}

fn parse_questions(file: &Path, text: &str) -> Result<(Vec<Prediction>, Vec<Question>), LessonError> {
    let raw: QuestionsFile =
        serde_json::from_str(text).map_err(|e| fail(file, "questions", e.to_string()))?;
    let mut ids = HashSet::new();
    for (i, p) in raw.predictions.iter().enumerate() {
        if !ids.insert(p.id.clone()) {
            return Err(fail(file, &format!("predictions[{i}].id"), "duplicate id"));
        }
        if p.prompt.trim().is_empty() || p.reveals.trim().is_empty() {
            return Err(fail(file, &format!("predictions[{i}]"), "prompt and reveals are required"));
        }
        if let PredictionKind::Choice { options } = &p.kind {
            if options.len() < 2 {
                return Err(fail(file, &format!("predictions[{i}].kind.options"), "at least two options"));
            }
        }
    }
    for (i, q) in raw.questions.iter().enumerate() {
        if !ids.insert(q.id.clone()) {
            return Err(fail(file, &format!("questions[{i}].id"), "duplicate id"));
        }
        if !(2..=6).contains(&q.options.len()) {
            return Err(fail(file, &format!("questions[{i}].options"), "between 2 and 6 options"));
        }
        if q.correct.is_empty() || q.correct.iter().any(|&c| c >= q.options.len()) {
            return Err(fail(file, &format!("questions[{i}].correct"), "indices into options, at least one"));
        }
        if q.explanation.chars().count() < MIN_EXPLANATION {
            return Err(fail(
                file,
                &format!("questions[{i}].explanation"),
                format!("explanations are at least {MIN_EXPLANATION} characters"),
            ));
        }
        if let Some(p) = &q.requires_prediction {
            if !raw.predictions.iter().any(|x| &x.id == p) {
                return Err(fail(file, &format!("questions[{i}].requires_prediction"), format!("no prediction {p}")));
            }
        }
    }
    Ok((raw.predictions, raw.questions))
}

/// Load one lesson directory.
pub fn load_dir(dir: &Path) -> Result<Lesson, LessonError> {
    let lesson_md = dir.join("lesson.md");
    let text = read(&lesson_md)?;
    let (yaml, body) = split_front_matter(&text)
        .ok_or_else(|| fail(&lesson_md, "front_matter", "lesson.md must start with a --- YAML --- block"))?;
    let front = parse_front_matter(&lesson_md, yaml)?;
    let experiment_file = dir.join("experiment.json");
    let experiment = parse_experiment(&experiment_file, &read(&experiment_file)?)?;
    let questions_file = dir.join("questions.json");
    let (predictions, questions) = parse_questions(&questions_file, &read(&questions_file)?)?;
    for (i, q) in questions.iter().enumerate() {
        if !front.concepts.is_empty() && !front.concepts.contains(&q.concept) {
            return Err(fail(
                &questions_file,
                &format!("questions[{i}].concept"),
                format!("{} is not among the lesson's concepts", q.concept),
            ));
        }
    }
    let derivation_file = dir.join("derivation.md");
    let derivation = if derivation_file.exists() {
        read(&derivation_file)?
            .split("\n---\n")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        vec![]
    };
    let instructor_file = dir.join("instructor.md");
    let instructor_notes = if instructor_file.exists() { Some(read(&instructor_file)?) } else { None };
    Ok(Lesson {
        dir: dir.to_path_buf(),
        front,
        body: body.to_string(),
        derivation,
        experiment,
        predictions,
        questions,
        instructor_notes,
    })
}

/// Load every lesson directory under each root, in order; a later root's
/// lesson with the same id replaces the earlier one. Directories without a
/// `lesson.md` are not lessons and are skipped. Errors are collected, never
/// fatal.
pub fn load_all(roots: &[PathBuf]) -> (Vec<Lesson>, Vec<LessonError>) {
    let mut lessons: Vec<Lesson> = vec![];
    let mut errors = vec![];
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else { continue };
        let mut dirs: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir() && p.join("lesson.md").is_file())
            .collect();
        dirs.sort();
        for dir in dirs {
            match load_dir(&dir) {
                Ok(lesson) => {
                    lessons.retain(|l| l.front.id != lesson.front.id);
                    lessons.push(lesson);
                }
                Err(e) => errors.push(e),
            }
        }
    }
    lessons.sort_by(|a, b| a.dir.file_name().cmp(&b.dir.file_name()));
    (lessons, errors)
}
