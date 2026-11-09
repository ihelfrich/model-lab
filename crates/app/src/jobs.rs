//! Supervised jobs from the shell: a job directory per run, a spec the
//! engine job binary reads, the supervisor bridge, manifests that survive a
//! quit, and resume from the last checkpoint.
use crate::experiment::{apply_overrides, resolve_data};
use model_lab_engine::experiment::{data_fingerprint, Run};
use model_lab_engine::job_spec::MAX_ROWS;
use model_lab_lessons::lesson::{Experiment, Kind};
use model_lab_supervisor::job::{JobEvent, JobHandle, JobSpec, Supervisor};
use model_lab_supervisor::limits::Limits;
use model_lab_supervisor::process::Command;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const CPU_LIMIT: Duration = Duration::from_secs(3600);
pub const WALL_LIMIT: Duration = Duration::from_secs(2 * 3600);
pub const MEMORY_LIMIT: u64 = 4 * 1024 * 1024 * 1024;
pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Running,
    Interrupted,
    Completed,
    Failed,
    Canceled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub label: String,
    pub lesson_id: String,
    pub created_ms: u64,
    pub status: JobStatus,
    pub units_total: Option<u64>,
    pub last_checkpoint_stage: Option<u64>,
    pub units_done: u64,
    pub error: Option<String>,
}

pub type Sink = Arc<dyn Fn(&str, &JobEvent) + Send + Sync>;

struct Active {
    cancel: Arc<AtomicBool>,
}

pub struct Jobs {
    dir: PathBuf,
    supervisor: Supervisor,
    sink: Sink,
    alive: Arc<AtomicBool>,
    active: Arc<Mutex<HashMap<String, Active>>>,
    manifests: Arc<Mutex<HashMap<String, Manifest>>>,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "model-lab-engine-job.exe"
    } else {
        "model-lab-engine-job"
    }
}

/// Release rule: the engine job binary sits beside the application binary
/// (Tauri strips the target-triple suffix when bundling a sidecar).
pub fn engine_job_path_release_only(exe_dir: &Path) -> Result<PathBuf, String> {
    let p = exe_dir.join(binary_name());
    if p.is_file() {
        Ok(p)
    } else {
        Err(format!("The engine job binary is missing: expected {}", p.display()))
    }
}

/// Where the engine job binary is. Debug builds also honour
/// `MODEL_LAB_ENGINE_JOB` and the workspace's `target/debug`; release builds
/// look beside the executable only.
pub fn engine_job_path() -> Result<PathBuf, String> {
    if cfg!(debug_assertions) {
        if let Ok(p) = std::env::var("MODEL_LAB_ENGINE_JOB") {
            let p = PathBuf::from(p);
            if p.is_file() {
                return Ok(p);
            }
        }
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug").join(binary_name());
        if dev.is_file() {
            return Ok(dev);
        }
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("executable has no directory")?;
    engine_job_path_release_only(dir)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

impl Jobs {
    /// Open the jobs directory; every manifest still `running` becomes
    /// `interrupted` (no process can have survived the previous run).
    pub fn load(dir: &Path, sink: Sink) -> Jobs {
        let _ = std::fs::create_dir_all(dir);
        let mut manifests = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let path = e.path().join("manifest.json");
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                let Ok(mut m) = serde_json::from_str::<Manifest>(&text) else { continue };
                if m.status == JobStatus::Running {
                    m.status = JobStatus::Interrupted;
                    let _ = write_json(&path, &m);
                }
                manifests.insert(m.id.clone(), m);
            }
        }
        Jobs {
            dir: dir.to_path_buf(),
            supervisor: Supervisor::new(),
            sink,
            alive: Arc::new(AtomicBool::new(true)),
            active: Arc::new(Mutex::new(HashMap::new())),
            manifests: Arc::new(Mutex::new(manifests)),
        }
    }

    pub fn job_dir(&self, id: &str) -> PathBuf {
        self.dir.join(id)
    }

    /// Newest first.
    pub fn list(&self) -> Vec<Manifest> {
        let mut v: Vec<Manifest> = self.manifests.lock().map(|m| m.values().cloned().collect()).unwrap_or_default();
        v.sort_by(|a, b| b.created_ms.cmp(&a.created_ms));
        v
    }

    pub fn manifest(&self, id: &str) -> Option<Manifest> {
        self.manifests.lock().ok()?.get(id).cloned()
    }

    /// Create the job directory, copy the data, write the spec and launch.
    pub fn start(&self, lesson_id: &str, label: &str, exp: &Experiment, csv_path: &Path, overrides: &Value) -> Result<String, String> {
        if exp.kind != Kind::Run {
            return Err("Only a data lesson can run as a supervised job.".into());
        }
        let config = apply_overrides(&exp.config, &exp.learner_may_change, overrides)?;
        let id = format!("{:x}-{:04x}", now_ms(), COUNTER.fetch_add(1, Ordering::Relaxed) & 0xffff);
        let dir = self.job_dir(&id);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let data_csv = dir.join("data.csv");
        std::fs::copy(csv_path, &data_csv).map_err(|e| format!("{}: {e}", csv_path.display()))?;
        // Validate the columns before launching: a bad CSV fails here, not in the worker.
        let mut probe = exp.clone();
        probe.source = model_lab_lessons::lesson::Source::Csv(data_csv.to_string_lossy().into_owned());
        let data = resolve_data(&probe, MAX_ROWS)?;
        let _ = data_fingerprint(&data);
        let spec = json!({
            "v": 3, "job": "fit", "label": label, "data_csv": data_csv,
            "target": exp.target, "features": exp.features,
            "config": config.to_json(),
            "max_rows": MAX_ROWS, "checkpoint_every_trees": exp.checkpoint_every_trees,
        });
        write_json(&dir.join("spec.json"), &spec)?;
        let manifest = Manifest {
            id: id.clone(),
            label: label.to_string(),
            lesson_id: lesson_id.to_string(),
            created_ms: now_ms(),
            status: JobStatus::Running,
            units_total: Some(config.tree.trees as u64),
            last_checkpoint_stage: None,
            units_done: 0,
            error: None,
        };
        write_json(&dir.join("manifest.json"), &manifest)?;
        self.manifests.lock().map_err(|_| "manifests unavailable")?.insert(id.clone(), manifest);
        self.launch(&id)
    }

    /// Resume an interrupted, canceled or failed job from its checkpoint.
    pub fn resume(&self, id: &str) -> Result<String, String> {
        let m = self.manifest(id).ok_or_else(|| format!("No job {id}"))?;
        if m.status == JobStatus::Running {
            return Err("The job is already running.".into());
        }
        if m.status == JobStatus::Completed {
            return Err("The job already completed; open its result.".into());
        }
        self.update(id, |m| {
            m.status = JobStatus::Running;
            m.error = None;
        })?;
        self.launch(id)
    }

    fn launch(&self, id: &str) -> Result<String, String> {
        let dir = self.job_dir(id);
        let manifest = self.manifest(id).ok_or_else(|| format!("No job {id}"))?;
        let program = engine_job_path()?;
        let spec = JobSpec {
            label: manifest.label.clone(),
            command: Command {
                program,
                args: vec![dir.join("spec.json").to_string_lossy().into_owned()],
                cwd: dir.clone(),
                env: vec![],
            },
            limits: Limits { wall: Some(WALL_LIMIT), cpu: Some(CPU_LIMIT), memory_bytes: Some(MEMORY_LIMIT), deny_network: false },
            units_total: manifest.units_total,
            sample_interval: SAMPLE_INTERVAL,
            checkpoint: Some(dir.join("checkpoint.json")),
        };
        let handle: JobHandle = self.supervisor.start(spec).map_err(|e| e.to_string())?;
        // The drain thread owns the handle: it forwards events, updates the
        // manifest and files the result. While the shell is alive, that is;
        // after a quit the manifest stays `running` on disk and `load` marks
        // it interrupted.
        let cancel_requested = Arc::new(AtomicBool::new(false));
        self.active.lock().map_err(|_| "jobs unavailable")?.insert(id.to_string(), Active { cancel: cancel_requested.clone() });
        let (sink, alive, manifests, active) = (self.sink.clone(), self.alive.clone(), self.manifests.clone(), self.active.clone());
        let id = id.to_string();
        let returned = id.clone();
        let manifest_path = dir.join("manifest.json");
        let result_path = dir.join("result.json");
        std::thread::Builder::new()
            .name(format!("job {id}"))
            .spawn(move || {
                loop {
                    if cancel_requested.swap(false, Ordering::Relaxed) {
                        handle.cancel();
                    }
                    let event = match handle.events().recv_timeout(Duration::from_millis(200)) {
                        Ok(e) => e,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    if !alive.load(Ordering::Relaxed) {
                        break;
                    }
                    let mut changed = false;
                    if let Ok(mut all) = manifests.lock() {
                        if let Some(m) = all.get_mut(&id) {
                            match &event {
                                JobEvent::Progress { units_done, .. } => m.units_done = *units_done,
                                JobEvent::Checkpoint { .. } => {
                                    m.last_checkpoint_stage = Some(m.units_done);
                                    changed = true;
                                }
                                JobEvent::Completed { result, .. } => {
                                    let _ = write_json(&result_path, result);
                                    m.status = JobStatus::Completed;
                                    changed = true;
                                }
                                JobEvent::Failed { error, limit, .. } => {
                                    m.status = if error == "canceled" { JobStatus::Canceled } else { JobStatus::Failed };
                                    m.error = Some(match limit {
                                        Some(l) => format!("{error} (limit: {l:?})"),
                                        None => error.clone(),
                                    });
                                    changed = true;
                                }
                                _ => {}
                            }
                            if changed {
                                let _ = write_json(&manifest_path, &*m);
                            }
                        }
                    }
                    sink(&id, &event);
                    if matches!(event, JobEvent::Completed { .. } | JobEvent::Failed { .. }) {
                        break;
                    }
                }
                if let Ok(mut a) = active.lock() {
                    a.remove(&id);
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(returned)
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut Manifest)) -> Result<(), String> {
        let mut all = self.manifests.lock().map_err(|_| "manifests unavailable")?;
        let m = all.get_mut(id).ok_or_else(|| format!("No job {id}"))?;
        f(m);
        write_json(&self.job_dir(id).join("manifest.json"), &*m)
    }

    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let active = self.active.lock().map_err(|_| "jobs unavailable")?;
        match active.get(id) {
            Some(a) => {
                a.cancel.store(true, Ordering::Relaxed);
                Ok(())
            }
            None => Err("The job is not running.".into()),
        }
    }

    /// The completed result, opened read-only by replaying its trees.
    pub fn open_result(&self, id: &str) -> Result<(Run, Manifest), String> {
        let m = self.manifest(id).ok_or_else(|| format!("No job {id}"))?;
        if m.status != JobStatus::Completed {
            return Err("The job has not completed.".into());
        }
        let text = std::fs::read_to_string(self.job_dir(id).join("result.json")).map_err(|e| e.to_string())?;
        let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        Ok((Run::from_trace(&value, MAX_ROWS)?, m))
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.active.lock().map(|a| a.contains_key(id)).unwrap_or(false)
    }
}

impl Drop for Jobs {
    fn drop(&mut self) {
        // A quit is not a cancel: manifests stay `running` on disk so the
        // next launch offers Resume. The supervisor's own Drop kills the
        // process trees.
        self.alive.store(false, Ordering::Relaxed);
    }
}
