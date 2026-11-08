//! Per-job resource limits. Enforcement is added in Task 8; this task only
//! carries the declaration and the platform hooks `spawn` calls.
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Limits {
    pub wall: Option<Duration>,
    pub cpu: Option<Duration>,
    pub memory_bytes: Option<u64>,
    pub deny_network: bool,
}

/// Runs in the forked child before exec (Unix). Enforcement lands in Task 8.
#[cfg(unix)]
pub(crate) fn apply_in_child(_limits: &Limits) -> std::io::Result<()> {
    Ok(())
}

/// Create the Job Object every Windows child is assigned to: kill-on-close so
/// the tree dies with the supervisor. Resource limits land in Task 8.
#[cfg(windows)]
pub(crate) fn create_job(_limits: &Limits) -> std::io::Result<windows_sys::Win32::Foundation::HANDLE> {
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
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
