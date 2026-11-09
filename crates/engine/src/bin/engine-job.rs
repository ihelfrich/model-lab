//! `model-lab-engine-job <spec.json>`: one supervised native fit. Speaks
//! protocol v3 on stdout with the nonce from `MODEL_LAB_NONCE`, checkpoints
//! tree ensembles to `MODEL_LAB_CHECKPOINT` and resumes from it when the
//! spec and data match.
use model_lab_engine::data::Table;
use model_lab_engine::ensemble::EnsembleState;
use model_lab_engine::experiment::{data_fingerprint, FittedModel, ModelId, Run, RunConfig};
use model_lab_engine::job_spec;
use model_lab_engine::provenance::sha256;
use model_lab_supervisor::checkpoint::{self, CheckpointError};
use model_lab_supervisor::job::CHECKPOINT_ENV;
use model_lab_supervisor::protocol::{encode, Kind, NONCE_ENV};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Exit code after a termination request (SIGTERM): the checkpoint, if one
/// was due, has been written.
const EXIT_TERMINATED: i32 = 143;

static TERMINATE: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_term(_: libc::c_int) {
    TERMINATE.store(true, Ordering::Relaxed);
}

#[cfg(unix)]
static PARENT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Whether the process that started us has gone away (Unix: our parent pid
/// changed, i.e. we were reparented to init/launchd). Windows relies on the
/// supervisor's Job Object, which kills the tree when its handle closes.
fn parent_gone() -> bool {
    #[cfg(unix)]
    {
        let now = unsafe { libc::getppid() };
        let first = PARENT.load(Ordering::Relaxed);
        if first == 0 {
            PARENT.store(now, Ordering::Relaxed);
            return false;
        }
        now != first
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn install_signal_handler() {
    #[cfg(unix)]
    unsafe {
        let handler: extern "C" fn(libc::c_int) = on_term;
        libc::signal(libc::SIGTERM, handler as libc::sighandler_t);
    }
}

struct Emitter {
    nonce: String,
    out: std::io::Stdout,
}

impl Emitter {
    fn emit(&mut self, kind: Kind) {
        let mut lock = self.out.lock();
        let _ = writeln!(lock, "{}", encode(&self.nonce, kind));
        let _ = lock.flush();
    }
    /// A plain line for the person watching; never an event.
    fn say(&mut self, text: &str) {
        let mut lock = self.out.lock();
        let _ = writeln!(lock, "{text}");
        let _ = lock.flush();
    }
}

fn main() {
    let (nonce, checkpoint_path) = match (std::env::var(NONCE_ENV), std::env::var(CHECKPOINT_ENV)) {
        (Ok(n), Ok(c)) if !n.is_empty() && !c.is_empty() => (n, PathBuf::from(c)),
        _ => {
            eprintln!("model-lab-engine-job must be started by the supervisor: {NONCE_ENV} and {CHECKPOINT_ENV} are required");
            std::process::exit(2);
        }
    };
    let Some(spec_path) = std::env::args().nth(1) else {
        eprintln!("usage: model-lab-engine-job <spec.json>");
        std::process::exit(2);
    };
    install_signal_handler();
    let _ = parent_gone();
    let mut emitter = Emitter { nonce, out: std::io::stdout() };
    match run(Path::new(&spec_path), &checkpoint_path, &mut emitter) {
        Ok(result) => {
            emitter.emit(Kind::Completed { result });
        }
        Err(Outcome::Failed(error)) => {
            emitter.emit(Kind::Failed { error });
            std::process::exit(1);
        }
        Err(Outcome::Terminated) => std::process::exit(EXIT_TERMINATED),
    }
}

enum Outcome {
    Failed(String),
    Terminated,
}

impl From<String> for Outcome {
    fn from(e: String) -> Self {
        Outcome::Failed(e)
    }
}

fn run(spec_path: &Path, checkpoint_path: &Path, emitter: &mut Emitter) -> Result<serde_json::Value, Outcome> {
    let spec_bytes = std::fs::read(spec_path).map_err(|e| format!("cannot read job spec: {e}"))?;
    let spec_sha256 = sha256(&spec_bytes);
    let spec = job_spec::load(spec_path)?;
    let text = std::fs::read_to_string(&spec.data_csv).map_err(|e| format!("cannot read {}: {e}", spec.data_csv.display()))?;
    let table = Table::parse_with_limit(&spec.label, &text, spec.max_rows)?;
    let column = |name: &str| table.headers.iter().position(|h| h == name).ok_or_else(|| format!("column {name} is not in the data"));
    let target = column(&spec.target)?;
    let features = spec.features.iter().map(|f| column(f)).collect::<Result<Vec<_>, _>>()?;
    let data = table.dataset(target, &features)?;
    Run::validate_data(&data, spec.max_rows)?;
    let mut config = RunConfig::from_json(&spec.config)?;
    let split = Run::prepare(&data, &mut config)?;
    let cancel = Arc::new(AtomicBool::new(false));

    let fitted = if let ModelId::Linear(_) = config.model {
        emitter.emit(Kind::Progress { units_done: 0, units_total: Some(1), unit: "fit".into(), stage: Some("solving".into()) });
        let fit = model_lab_engine::linear::fit(&data, &split.train, config.linear.clone(), cancel.clone())?;
        emitter.emit(Kind::Progress { units_done: 1, units_total: Some(1), unit: "fit".into(), stage: None });
        FittedModel::Linear(fit)
    } else {
        let data_sha256 = data_fingerprint(&data);
        let mut state = match checkpoint::read_verified(checkpoint_path) {
            Ok(document) => {
                if document["spec_sha256"].as_str() != Some(spec_sha256.as_str()) {
                    return Err(format!("checkpoint {} was written for a different job spec; delete it or change the checkpoint path", checkpoint_path.display()).into());
                }
                if document["data_sha256"].as_str() != Some(data_sha256.as_str()) {
                    return Err(format!("checkpoint {} was written for different data", checkpoint_path.display()).into());
                }
                EnsembleState::from_json(&document["state"], data.clone())?
            }
            Err(CheckpointError::Io(e, _)) if e.kind() == std::io::ErrorKind::NotFound => EnsembleState::new(data.clone(), config.tree.clone())?,
            Err(e) => {
                // Damaged or foreign: keep it aside for inspection and start fresh.
                let mut aside = checkpoint_path.as_os_str().to_owned();
                aside.push(".damaged");
                let moved = std::fs::rename(checkpoint_path, &aside).is_ok();
                emitter.say(&format!(
                    "{e}; starting fresh{}",
                    if moved { format!(" (the damaged file is kept at {})", PathBuf::from(&aside).display()) } else { String::new() }
                ));
                EnsembleState::new(data.clone(), config.tree.clone())?
            }
        };
        let total = if config.tree.method == model_lab_engine::model::Method::Tree { 1 } else { config.tree.trees } as u64;
        // A checkpoint that cannot be written is reported and the fit goes on;
        // it never costs a completed fit its result.
        let write = |state: &EnsembleState, emitter: &mut Emitter| {
            let document = serde_json::json!({"spec_sha256": spec_sha256, "data_sha256": data_sha256, "state": state.to_json()});
            match checkpoint::write_atomic(checkpoint_path, &document) {
                Ok(hash) => emitter.emit(Kind::Checkpoint { path: checkpoint_path.to_string_lossy().into_owned(), sha256: hash }),
                Err(e) => emitter.say(&format!("warning: could not write the checkpoint: {e}; continuing without it")),
            }
        };
        let mut since_checkpoint = 0usize;
        while !state.finished() {
            state.step()?;
            since_checkpoint += 1;
            emitter.emit(Kind::Progress { units_done: state.stage() as u64, units_total: Some(total), unit: "tree".into(), stage: None });
            if since_checkpoint >= spec.checkpoint_every_trees {
                write(&state, emitter);
                since_checkpoint = 0;
            }
            if parent_gone() {
                // The supervisor died without killing us (a crash, kill -9):
                // leave a checkpoint and go, never a process behind.
                write(&state, emitter);
                emitter.say("the supervising process is gone; stopping after a checkpoint");
                return Err(Outcome::Terminated);
            }
            if TERMINATE.load(Ordering::Relaxed) {
                if since_checkpoint > 0 {
                    write(&state, emitter);
                }
                return Err(Outcome::Terminated);
            }
        }
        if since_checkpoint > 0 {
            write(&state, emitter);
        }
        FittedModel::Trees(state.into_fit())
    };
    let run = Run::from_fitted(spec.label.clone(), data, config, split, fitted, cancel)?;
    Ok(run.to_json())
}
