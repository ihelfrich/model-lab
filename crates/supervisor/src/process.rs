//! Spawn a backend in its own process group (Unix) or Job Object (Windows),
//! read its pipes on bounded channels, and kill the whole tree synchronously.

use crate::limits::Limits;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

/// Channel capacity for child output; a flooding child blocks on the pipe
/// instead of growing the supervisor's memory.
pub const LINE_BUFFER: usize = 1024;
/// Environment variables copied from the supervisor into every child.
pub const INHERITED_ENV: [&str; 6] = ["PATH", "SystemRoot", "WINDIR", "TEMP", "TMP", "TMPDIR"];

#[derive(Debug, Clone)]
pub struct Command {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChildLine {
    Stdout(String),
    Stderr(String),
}

pub struct Child {
    inner: std::process::Child,
    lines: Receiver<ChildLine>,
    status: Option<ExitStatus>,
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(windows)]
    port: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
unsafe impl Send for Child {}

/// Spawn `cmd` in a fresh process group (Unix) or a kill-on-close Job Object
/// (Windows) with a minimal environment carrying `MODEL_LAB_NONCE`.
pub fn spawn(cmd: Command, nonce: &str, limits: &Limits) -> std::io::Result<Child> {
    let mut command = std::process::Command::new(&cmd.program);
    command.args(&cmd.args).current_dir(&cmd.cwd).env_clear();
    for key in INHERITED_ENV {
        if let Some(v) = std::env::var_os(key) {
            command.env(key, v);
        }
    }
    command.env(crate::protocol::NONCE_ENV, nonce);
    for (k, v) in &cmd.env {
        command.env(k, v);
    }
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let limits = limits.clone();
        // SAFETY: only async-signal-safe calls (setpgid, setrlimit) run in the child.
        unsafe {
            command.pre_exec(move || {
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                crate::limits::apply_in_child(&limits)
            });
        }
    }
    #[cfg(windows)]
    let (job, port) = crate::limits::create_job(limits)?;
    let mut inner = command.spawn()?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        if unsafe { AssignProcessToJobObject(job, inner.as_raw_handle() as _) } == 0 {
            let e = std::io::Error::last_os_error();
            let _ = inner.kill();
            return Err(e);
        }
    }
    let (tx, rx) = sync_channel(LINE_BUFFER);
    let stdout = inner.stdout.take().expect("piped stdout");
    let stderr = inner.stderr.take().expect("piped stderr");
    read_lines(stdout, tx.clone(), ChildLine::Stdout);
    read_lines(stderr, tx, ChildLine::Stderr);
    Ok(Child {
        inner,
        lines: rx,
        status: None,
        #[cfg(windows)]
        job,
        #[cfg(windows)]
        port,
    })
}

/// A detached reader: it ends at pipe EOF (every group member is dead) or when
/// the receiver is dropped. It is never joined, so nothing can block on it.
fn read_lines<R: std::io::Read + Send + 'static>(
    pipe: R,
    tx: SyncSender<ChildLine>,
    wrap: fn(String) -> ChildLine,
) {
    std::thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            if tx.send(wrap(line)).is_err() {
                break;
            }
        }
    });
}

impl Child {
    pub fn pid(&self) -> u32 {
        self.inner.id()
    }

    pub fn lines(&self) -> &Receiver<ChildLine> {
        &self.lines
    }

    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        if let Some(s) = self.status {
            return Ok(Some(s));
        }
        let s = self.inner.try_wait()?;
        self.status = s;
        Ok(s)
    }

    /// Terminate the whole tree: a polite signal, `grace`, then a forced kill,
    /// and wait for the direct child. The group is signalled even when the
    /// direct child has already exited, because its descendants may not have.
    pub fn kill_tree(&mut self, grace: Duration) -> std::io::Result<ExitStatus> {
        #[cfg(unix)]
        {
            let pgid = self.pid() as i32;
            // ESRCH (no such group) is the normal outcome once everything is gone.
            unsafe { libc::killpg(pgid, libc::SIGTERM) };
            let deadline = Instant::now() + grace;
            while Instant::now() < deadline {
                if self.try_wait()?.is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            unsafe { libc::killpg(pgid, libc::SIGKILL) };
        }
        #[cfg(windows)]
        {
            let _ = grace;
            use windows_sys::Win32::System::JobObjects::TerminateJobObject;
            unsafe { TerminateJobObject(self.job, 1) };
        }
        if let Some(s) = self.status {
            return Ok(s);
        }
        let s = self.inner.wait()?;
        self.status = Some(s);
        Ok(s)
    }
}

impl Child {
    /// After exit: which configured limit, if any, ended the process.
    pub fn limit_violation(&self, limits: &Limits) -> Option<crate::limits::LimitHit> {
        #[cfg(windows)]
        {
            crate::limits::violation(self.job, self.port, limits)
        }
        #[cfg(unix)]
        {
            self.status.and_then(|s| crate::limits::violation(s, limits))
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        // Always: the direct child may be gone while its descendants live on.
        let _ = self.kill_tree(Duration::from_secs(2));
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.port);
            windows_sys::Win32::Foundation::CloseHandle(self.job);
        }
    }
}
