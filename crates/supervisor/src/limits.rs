//! Per-job resource limits, chosen by the job rather than a global cap.
//! CPU and memory are enforced by the operating system where it can
//! (rlimits on Unix, Job Object limits on Windows); the wall clock is the
//! supervisor's; network denial is recorded but not enforced in this build.
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Limits {
    pub wall: Option<Duration>,
    pub cpu: Option<Duration>,
    pub memory_bytes: Option<u64>,
    pub deny_network: bool,
}

/// Which limit ended a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitHit {
    Wall,
    Cpu,
    Memory,
}

pub const NETWORK_NOT_RESTRICTED: &str = "network: not restricted";
pub const NETWORK_DENIAL_NOT_ENFORCED: &str =
    "network: not restricted (requested denial is not enforced in this build)";

impl Limits {
    /// Human-readable statement of what is enforced, honest about what is not.
    pub fn describe(&self) -> String {
        let mut parts = vec![];
        if let Some(w) = self.wall {
            parts.push(format!("wall {} s", w.as_secs()));
        }
        if let Some(c) = self.cpu {
            parts.push(format!("cpu {} s", c.as_secs()));
        }
        if let Some(m) = self.memory_bytes {
            parts.push(format!("memory {} MiB", m / (1024 * 1024)));
        }
        let network = if self.deny_network { NETWORK_DENIAL_NOT_ENFORCED } else { NETWORK_NOT_RESTRICTED };
        if parts.is_empty() {
            if self.deny_network {
                network.to_string()
            } else {
                format!("no limits; {network}")
            }
        } else {
            format!("{}; {network}", parts.join("; "))
        }
    }
}

/// Runs in the forked child before exec (Unix): CPU seconds through
/// RLIMIT_CPU; memory through RLIMIT_AS on Linux. macOS rejects small
/// address-space limits with EINVAL and does not enforce them, so there the
/// supervisor's resource sampler ends a job whose tree exceeds the limit.
#[cfg(unix)]
pub(crate) fn apply_in_child(limits: &Limits) -> std::io::Result<()> {
    unsafe fn set(resource: libc::c_int, value: u64) -> std::io::Result<()> {
        let limit = libc::rlimit { rlim_cur: value as libc::rlim_t, rlim_max: value as libc::rlim_t };
        if libc::setrlimit(resource, &limit) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    unsafe {
        if let Some(cpu) = limits.cpu {
            set(libc::RLIMIT_CPU, cpu.as_secs().max(1))?;
        }
        #[cfg(target_os = "linux")]
        if let Some(bytes) = limits.memory_bytes {
            set(libc::RLIMIT_AS, bytes)?;
        }
    }
    Ok(())
}

/// Create the Job Object every Windows child is assigned to: kill-on-close so
/// the tree dies with the supervisor, plus per-process memory and job CPU
/// time limits when requested.
#[cfg(windows)]
pub(crate) fn create_job(limits: &Limits) -> std::io::Result<windows_sys::Win32::Foundation::HANDLE> {
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_JOB_TIME,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    };
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Some(bytes) = limits.memory_bytes {
            info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
            info.ProcessMemoryLimit = bytes as usize;
        }
        if let Some(cpu) = limits.cpu {
            info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_TIME;
            info.BasicLimitInformation.PerJobUserTimeLimit = (cpu.as_nanos() / 100) as i64;
        }
        if SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            let e = std::io::Error::last_os_error();
            windows_sys::Win32::Foundation::CloseHandle(job);
            return Err(e);
        }
        Ok(job)
    }
}
