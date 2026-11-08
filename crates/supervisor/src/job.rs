//! One supervised job: a process tree, its protocol events, limits, ETA and
//! resource samples, delivered as `JobEvent`s on a bounded channel.
use crate::eta::{Eta, EtaRange};
use crate::limits::{LimitHit, Limits};
use crate::process::{spawn, Child, ChildLine, Command};
use crate::protocol::{self, Kind, Line};
use crate::resources::{Sample, Sampler};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Environment variable through which a worker receives its checkpoint path.
pub const CHECKPOINT_ENV: &str = "MODEL_LAB_CHECKPOINT";
/// Event channel capacity.
pub const EVENT_BUFFER: usize = 256;
/// Grace between the polite and the forced kill on cancel or limit.
pub const KILL_GRACE: Duration = Duration::from_secs(2);
/// Consecutive over-limit samples before the sampler ends a job (platforms
/// without a kernel memory limit).
pub const MEMORY_STRIKES: u32 = 2;
/// Lines of stderr kept for the failure message.
pub const STDERR_TAIL: usize = 20;
/// Longest the job thread waits to deliver a terminal event to a consumer
/// that is not reading before giving up on it.
pub const TERMINAL_DELIVERY: Duration = Duration::from_secs(30);

pub struct JobSpec {
    pub label: String,
    pub command: Command,
    pub limits: Limits,
    pub units_total: Option<u64>,
    pub sample_interval: Duration,
    pub checkpoint: Option<PathBuf>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum JobEvent {
    Started { pid: u32, started_at_ms: u64, limits: String },
    Progress {
        units_done: u64,
        units_total: Option<u64>,
        unit: String,
        stage: Option<String>,
        eta: Option<EtaRange>,
        latest: Option<Sample>,
    },
    Output(String),
    /// A resource reading, emitted on every sample so a quiet job is still visible.
    Sample(Sample),
    Checkpoint { path: String, sha256: String },
    Completed { result: serde_json::Value, wall: Duration },
    Failed { error: String, limit: Option<LimitHit>, wall: Duration },
}

pub struct JobHandle {
    events: Receiver<JobEvent>,
    cancel: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    iterations: Arc<AtomicU64>,
}

impl JobHandle {
    pub fn events(&self) -> &Receiver<JobEvent> {
        &self.events
    }
    /// Ask the job to stop. The job thread kills the tree and emits
    /// `Failed { error: "canceled" }` as its last event.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }
    /// Control-loop passes so far: a diagnostic. A job that is waiting
    /// (on a line, a sample or a sleep) makes a few dozen passes a second;
    /// thousands mean the loop is spinning.
    pub fn loop_iterations(&self) -> u64 {
        self.iterations.load(Ordering::Relaxed)
    }
}

struct Running {
    cancel: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[derive(Default)]
pub struct Supervisor {
    jobs: Mutex<Vec<Running>>,
}

impl Supervisor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&self, spec: JobSpec) -> std::io::Result<JobHandle> {
        let (tx, rx) = sync_channel(EVENT_BUFFER);
        let cancel = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let iterations = Arc::new(AtomicU64::new(0));
        let nonce = protocol::nonce();
        let mut command = spec.command.clone();
        if let Some(path) = &spec.checkpoint {
            command.env.push((CHECKPOINT_ENV.into(), path.to_string_lossy().into_owned()));
        }
        let child = spawn(command, &nonce, &spec.limits)?;
        let worker = JobThread { spec, nonce, child, tx, cancel: cancel.clone(), finished: finished.clone(), wall_deadline: None, iterations: iterations.clone() };
        let thread = std::thread::spawn(move || worker.run());
        self.jobs.lock().unwrap().retain(|r| r.thread.as_ref().is_some_and(|t| !t.is_finished()));
        self.jobs.lock().unwrap().push(Running { cancel: cancel.clone(), thread: Some(thread) });
        Ok(JobHandle { events: rx, cancel, finished, iterations })
    }

    /// Cancel every running job and wait for each to end.
    pub fn shutdown(self) {
        drop(self);
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        let jobs = std::mem::take(&mut *self.jobs.lock().unwrap());
        for job in &jobs {
            job.cancel.store(true, Ordering::Relaxed);
        }
        for mut job in jobs {
            if let Some(t) = job.thread.take() {
                let _ = t.join();
            }
        }
    }
}

struct JobThread {
    spec: JobSpec,
    nonce: String,
    child: Child,
    tx: SyncSender<JobEvent>,
    cancel: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    /// When the wall-clock limit falls, if one is set.
    wall_deadline: Option<Instant>,
    iterations: Arc<AtomicU64>,
}

enum Outcome {
    Completed(serde_json::Value),
    Failed(String, Option<LimitHit>),
}

#[derive(Default)]
struct LoopState {
    eta: Eta,
    latest: Option<Sample>,
    stderr_tail: VecDeque<String>,
    result: Option<Outcome>,
}

impl JobThread {
    /// One line from the child, before or after its exit: the same handling
    /// in both places, so nothing written just before exit is lost.
    fn handle_line(&self, line: ChildLine, st: &mut LoopState) {
        match line {
            ChildLine::Stdout(text) => match protocol::parse_line(&text, &self.nonce) {
                Line::Event(envelope) => match envelope.kind {
                    Kind::Progress { units_done, units_total, unit, stage } => {
                        let now = Instant::now();
                        st.eta.observe(units_done, now);
                        let total = units_total.or(self.spec.units_total);
                        let estimate = total.and_then(|t| st.eta.estimate(t, now));
                        self.emit(JobEvent::Progress { units_done, units_total: total, unit, stage, eta: estimate, latest: st.latest.clone() });
                    }
                    Kind::Checkpoint { path, sha256 } => self.emit(JobEvent::Checkpoint { path, sha256 }),
                    Kind::Completed { result } => st.result = Some(Outcome::Completed(result)),
                    Kind::Failed { error } => st.result = Some(Outcome::Failed(error, None)),
                },
                Line::Output(text) => self.emit(JobEvent::Output(text)),
            },
            ChildLine::Stderr(text) => {
                if st.stderr_tail.len() == STDERR_TAIL {
                    st.stderr_tail.pop_front();
                }
                st.stderr_tail.push_back(text.clone());
                self.emit(JobEvent::Output(text));
            }
        }
    }
}

impl JobThread {
    /// Deliver an event. A full channel applies backpressure (the child
    /// blocks on its pipe meanwhile, so no output is lost), but the wait
    /// yields to cancel and to the wall clock, so a consumer that stops
    /// reading never takes control of the job away from the supervisor.
    fn emit(&self, event: JobEvent) {
        let mut event = event;
        loop {
            match self.tx.try_send(event) {
                Ok(()) | Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return,
                Err(std::sync::mpsc::TrySendError::Full(e)) => {
                    if self.cancel.load(Ordering::Relaxed)
                        || self.wall_deadline.is_some_and(|d| Instant::now() >= d)
                    {
                        return;
                    }
                    event = e;
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }

    /// Deliver the last event, waiting a bounded time for a slow consumer.
    fn emit_terminal(&self, event: JobEvent) {
        let mut event = event;
        let deadline = Instant::now() + TERMINAL_DELIVERY;
        loop {
            match self.tx.try_send(event) {
                Ok(()) => return,
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return,
                Err(std::sync::mpsc::TrySendError::Full(e)) => {
                    if Instant::now() >= deadline {
                        return;
                    }
                    event = e;
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    fn run(mut self) {
        let started = Instant::now();
        self.wall_deadline = self.spec.limits.wall.map(|w| started + w);
        let started_at_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        self.emit(JobEvent::Started { pid: self.child.pid(), started_at_ms, limits: self.spec.limits.describe() });
        let (sample_tx, sample_rx) = sync_channel::<Sample>(64);
        let sampler = Sampler::start(self.child.pid(), self.spec.sample_interval, sample_tx);
        let mut st = LoopState::default();
        let mut strikes = 0u32;
        let outcome = loop {
            self.iterations.fetch_add(1, Ordering::Relaxed);
            // Lines from the child. Closed pipes with a live child must not spin.
            match self.child.lines().recv_timeout(Duration::from_millis(50)) {
                Ok(line) => self.handle_line(line, &mut st),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => std::thread::sleep(Duration::from_millis(50)),
            }
            // Resource samples; the sampler enforces the memory limit only where
            // the kernel does not (macOS), so a mis-attributed process cannot
            // end a job the kernel is already policing.
            loop {
                match sample_rx.try_recv() {
                    Ok(s) => {
                        if let Some(limit) = self.spec.limits.memory_bytes {
                            strikes = if s.rss_bytes > limit { strikes + 1 } else { 0 };
                        }
                        self.emit(JobEvent::Sample(s.clone()));
                        st.latest = Some(s);
                    }
                    Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
                }
            }
            if cfg!(target_os = "macos") && strikes >= MEMORY_STRIKES {
                let _ = self.child.kill_tree(KILL_GRACE);
                break Outcome::Failed(format!("memory use exceeded {} MiB", self.spec.limits.memory_bytes.unwrap_or(0) / (1024 * 1024)), Some(LimitHit::Memory));
            }
            if self.cancel.load(Ordering::Relaxed) {
                let _ = self.child.kill_tree(KILL_GRACE);
                break Outcome::Failed("canceled".into(), None);
            }
            if let Some(wall) = self.spec.limits.wall {
                if started.elapsed() > wall {
                    let _ = self.child.kill_tree(KILL_GRACE);
                    break Outcome::Failed(format!("wall-clock limit of {} s exceeded", wall.as_secs()), Some(LimitHit::Wall));
                }
            }
            // Exit.
            if let Ok(Some(status)) = self.child.try_wait() {
                // Drain what the pipes still hold. A descendant may keep them
                // open, so the wait is bounded: one second of silence ends it.
                let mut idle_since = Instant::now();
                loop {
                    match self.child.lines().recv_timeout(Duration::from_millis(50)) {
                        Ok(line) => {
                            self.handle_line(line, &mut st);
                            idle_since = Instant::now();
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {
                            if idle_since.elapsed() >= Duration::from_secs(1) {
                                break;
                            }
                        }
                    }
                }
                break match st.result.take() {
                    Some(Outcome::Completed(v)) if status.success() => Outcome::Completed(v),
                    Some(Outcome::Failed(e, l)) => Outcome::Failed(e, l),
                    _ => {
                        let limit = self.child.limit_violation(&self.spec.limits);
                        let tail: Vec<String> = st.stderr_tail.iter().cloned().collect();
                        Outcome::Failed(format!("worker exited ({status}) without a result; last stderr:\n{}", tail.join("\n")), limit)
                    }
                };
            }
        };
        // Close our end first so the sampler can never be stuck sending.
        drop(sample_rx);
        sampler.stop();
        let wall = started.elapsed();
        // The job is over before its last event is delivered: a consumer that
        // is not reading must not keep `is_finished` false.
        self.finished.store(true, Ordering::Relaxed);
        match outcome {
            Outcome::Completed(result) => self.emit_terminal(JobEvent::Completed { result, wall }),
            Outcome::Failed(error, limit) => self.emit_terminal(JobEvent::Failed { error, limit, wall }),
        }
        // Dropping `child` kills anything left in the tree.
    }
}
