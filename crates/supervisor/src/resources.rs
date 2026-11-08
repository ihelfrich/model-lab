//! CPU and memory of a job's whole process tree, sampled on a thread at the
//! interval the job declares. GPU memory is reported as unknown (`None`) in
//! this build rather than guessed.
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct Sample {
    /// Milliseconds since the sampler started.
    pub at_ms: u64,
    /// Sum of per-process CPU use over the tree, in percent of one core.
    pub cpu_percent: f32,
    /// Sum of resident set sizes over the tree.
    pub rss_bytes: u64,
    /// Live processes in the tree.
    pub processes: u32,
    /// Always `None` here: no GPU accounting is attempted in this build.
    pub gpu_memory_bytes: Option<u64>,
}

pub struct Sampler {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Sampler {
    /// Start sampling the tree rooted at `root_pid`. Membership is the Unix
    /// process group on Unix, and a sticky parent chain everywhere (a process
    /// whose parent was ever in the tree stays in it, so reparenting after a
    /// parent exits does not lose descendants).
    pub fn start(root_pid: u32, interval: Duration, sink: SyncSender<Sample>) -> Sampler {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            let started = Instant::now();
            let mut system = match System::new() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("resource sampler unavailable: {e}");
                    return;
                }
            };
            let refresh = ProcessRefreshKind::nothing().with_memory().with_cpu().with_exe(UpdateKind::Never);
            let mut known: HashSet<u32> = HashSet::from([root_pid]);
            let mut next = started;
            while !flag.load(Ordering::Relaxed) {
                next += interval;
                system.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);
                let sample = measure(&system, root_pid, &mut known, started.elapsed());
                // A full channel drops this sample rather than blocking the
                // sampler (and whoever joins it); a disconnected one ends it.
                match sink.try_send(sample) {
                    Ok(()) | Err(std::sync::mpsc::TrySendError::Full(_)) => {}
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => break,
                }
                while Instant::now() < next && !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(10).min(next.saturating_duration_since(Instant::now())));
                }
            }
        });
        Sampler { stop, thread: Some(thread) }
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn in_group(pid: u32, root_pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::getpgid(pid as libc::pid_t) == root_pid as libc::pid_t }
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, root_pid);
        false
    }
}

fn measure(system: &System, root_pid: u32, known: &mut HashSet<u32>, elapsed: Duration) -> Sample {
    // Forget members that have gone, so a reused pid is not mistaken for one
    // of ours; the root stays as the anchor of the parent chain.
    known.retain(|pid| *pid == root_pid || system.process(Pid::from_u32(*pid)).is_some());
    // Grow the sticky set until it stops changing: a child may be listed before its parent.
    loop {
        let before = known.len();
        for (pid, process) in system.processes() {
            let pid = pid.as_u32();
            if known.contains(&pid) {
                continue;
            }
            let parent_known = process.parent().is_some_and(|p| known.contains(&p.as_u32()));
            if parent_known || in_group(pid, root_pid) {
                known.insert(pid);
            }
        }
        if known.len() == before {
            break;
        }
    }
    let mut sample = Sample { at_ms: elapsed.as_millis() as u64, cpu_percent: 0., rss_bytes: 0, processes: 0, gpu_memory_bytes: None };
    for pid in known.iter() {
        if let Some(p) = system.process(Pid::from_u32(*pid)) {
            sample.processes += 1;
            sample.cpu_percent += p.cpu_usage();
            sample.rss_bytes += p.memory();
        }
    }
    sample
}
