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
pub(crate) fn create_job(limits: &Limits) -> std::io::Result<(windows_sys::Win32::Foundation::HANDLE, windows_sys::Win32::Foundation::HANDLE)> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectAssociateCompletionPortInformation, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_ASSOCIATE_COMPLETION_PORT, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_JOB_TIME, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
        JOB_OBJECT_LIMIT_PROCESS_TIME,
    };
    use windows_sys::Win32::System::IO::CreateIoCompletionPort;
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
            // Both: the per-process limit ends the offending process promptly;
            // the job limit caps the tree as a whole.
            info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_TIME | JOB_OBJECT_LIMIT_PROCESS_TIME;
            info.BasicLimitInformation.PerJobUserTimeLimit = (cpu.as_nanos() / 100) as i64;
            info.BasicLimitInformation.PerProcessUserTimeLimit = (cpu.as_nanos() / 100) as i64;
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
        // Limit events (memory refused, time exhausted) arrive as messages on
        // this port; hard limits are not reported any other way.
        let port = CreateIoCompletionPort(INVALID_HANDLE_VALUE, std::ptr::null_mut(), 0, 1);
        if port.is_null() {
            let e = std::io::Error::last_os_error();
            windows_sys::Win32::Foundation::CloseHandle(job);
            return Err(e);
        }
        let associate = JOBOBJECT_ASSOCIATE_COMPLETION_PORT { CompletionKey: std::ptr::null_mut(), CompletionPort: port };
        if SetInformationJobObject(
            job,
            JobObjectAssociateCompletionPortInformation,
            &associate as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>() as u32,
        ) == 0
        {
            let e = std::io::Error::last_os_error();
            windows_sys::Win32::Foundation::CloseHandle(port);
            windows_sys::Win32::Foundation::CloseHandle(job);
            return Err(e);
        }
        Ok((job, port))
    }
}

/// Job Object message identifiers (documented values).
#[cfg(windows)]
mod job_msg {
    pub const END_OF_JOB_TIME: u32 = 1;
    pub const END_OF_PROCESS_TIME: u32 = 2;
    pub const PROCESS_MEMORY_LIMIT: u32 = 9;
    pub const JOB_MEMORY_LIMIT: u32 = 10;
}

/// Which configured limit, if any, the Job Object's accounting shows was
/// reached (Windows). Hard limits are not reported through the violation
/// query (that covers notification limits only), so peak process memory and
/// total user time are compared with the configured values.
#[cfg(windows)]
pub(crate) fn violation(
    job: windows_sys::Win32::Foundation::HANDLE,
    port: windows_sys::Win32::Foundation::HANDLE,
    limits: &Limits,
) -> Option<LimitHit> {
    use windows_sys::Win32::System::JobObjects::{
        JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation, QueryInformationJobObject,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    };
    use windows_sys::Win32::System::IO::{GetQueuedCompletionStatus, OVERLAPPED};
    unsafe {
        // Drain every queued message; the strongest attribution wins.
        let mut hit = None;
        loop {
            let mut message = 0u32;
            let mut key = 0usize;
            let mut overlapped: *mut OVERLAPPED = std::ptr::null_mut();
            let ok = GetQueuedCompletionStatus(port, &mut message, &mut key, &mut overlapped, 0);
            if ok == 0 && overlapped.is_null() {
                break;
            }
            match message {
                job_msg::PROCESS_MEMORY_LIMIT | job_msg::JOB_MEMORY_LIMIT => hit = Some(LimitHit::Memory),
                job_msg::END_OF_PROCESS_TIME | job_msg::END_OF_JOB_TIME if hit.is_none() => hit = Some(LimitHit::Cpu),
                _ => {}
            }
        }
        if hit.is_some() {
            return hit;
        }
        if let Some(limit) = limits.memory_bytes {
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            let ok = QueryInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &mut info as *mut _ as *mut _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                std::ptr::null_mut(),
            );
            // Commit charge stops at the limit when the failing allocation is refused.
            if ok != 0 && (info.PeakProcessMemoryUsed as u64) >= limit * 9 / 10 {
                return Some(LimitHit::Memory);
            }
        }
        if let Some(cpu) = limits.cpu {
            let mut acct: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
            let ok = QueryInformationJobObject(
                job,
                JobObjectBasicAccountingInformation,
                &mut acct as *mut _ as *mut _,
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            );
            if ok != 0 && (acct.TotalUserTime as u128) >= cpu.as_nanos() / 100 {
                return Some(LimitHit::Cpu);
            }
        }
    }
    None
}

/// Which limit, if any, a Unix exit status indicates. SIGXCPU is the CPU
/// rlimit; a process that died of SIGABRT or SIGSEGV while a memory limit
/// was set is taken to have failed an allocation under RLIMIT_AS (Linux).
#[cfg(unix)]
pub(crate) fn violation(status: std::process::ExitStatus, limits: &Limits) -> Option<LimitHit> {
    use std::os::unix::process::ExitStatusExt;
    match status.signal() {
        Some(libc::SIGXCPU) => Some(LimitHit::Cpu),
        Some(libc::SIGABRT) | Some(libc::SIGSEGV) if limits.memory_bytes.is_some() => Some(LimitHit::Memory),
        _ => None,
    }
}
