use model_lab_supervisor::job::{JobEvent, JobSpec, Supervisor};
use model_lab_supervisor::limits::{LimitHit, Limits};
use model_lab_supervisor::process::Command;
use std::time::{Duration, Instant};

fn worker(args: &[&str]) -> Command {
    Command {
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_fake-worker")),
        args: args.iter().map(|s| s.to_string()).collect(),
        cwd: std::env::temp_dir(),
        env: vec![],
    }
}

fn spec(args: &[&str], limits: Limits) -> JobSpec {
    JobSpec {
        label: "test".into(),
        command: worker(args),
        limits,
        units_total: None,
        sample_interval: Duration::from_millis(200),
        checkpoint: None,
    }
}

fn collect(handle: &model_lab_supervisor::job::JobHandle, within: Duration) -> Vec<JobEvent> {
    let deadline = Instant::now() + within;
    let mut events = vec![];
    while Instant::now() < deadline {
        match handle.events().recv_timeout(Duration::from_millis(200)) {
            Ok(e) => events.push(e),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(_) => {}
        }
    }
    events
}

fn process_exists(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let out = std::process::Command::new("ps").args(["-o", "stat=", "-p", &pid.to_string()]).output().unwrap();
        let stat = String::from_utf8_lossy(&out.stdout).trim().to_string();
        !stat.is_empty() && !stat.starts_with('Z')
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code) != 0;
            CloseHandle(h);
            ok && code == STILL_ACTIVE as u32
        }
    }
}

#[test]
fn progress_events_carry_eta_after_three_units() {
    let sup = Supervisor::new();
    let handle = sup.start(spec(&["--unit-delay-ms", "50", "--emit", "10"], Limits::default())).unwrap();
    let events = collect(&handle, Duration::from_secs(15));
    let progress: Vec<_> = events.iter().filter_map(|e| match e { JobEvent::Progress { units_done, eta, unit, units_total, .. } => Some((*units_done, eta.clone(), unit.clone(), *units_total)), _ => None }).collect();
    assert_eq!(progress.len(), 10, "{events:?}");
    assert_eq!(progress[0].0, 1);
    assert!(progress[0].1.is_none(), "first progress has no ETA: {:?}", progress[0]);
    assert!(progress[1].1.is_none());
    assert!(progress[3].1.is_some(), "fourth progress carries an ETA: {:?}", progress[3]);
    assert!(progress.iter().all(|p| p.2 == "unit" && p.3 == Some(10)));
    assert!(matches!(events.last(), Some(JobEvent::Completed { .. }) | Some(JobEvent::Failed { .. })), "{:?}", events.last());
}

#[test]
fn spoofed_event_is_output_not_progress() {
    let sup = Supervisor::new();
    let handle = sup.start(spec(&["--spoof"], Limits::default())).unwrap();
    let events = collect(&handle, Duration::from_secs(10));
    let outputs: Vec<_> = events.iter().filter_map(|e| match e { JobEvent::Output(s) => Some(s.clone()), _ => None }).collect();
    assert_eq!(outputs.len(), 1, "{events:?}");
    assert!(outputs[0].contains("spoof"));
    assert!(!events.iter().any(|e| matches!(e, JobEvent::Progress { .. })));
    // A worker that exits without a Completed event has failed.
    assert!(matches!(events.last(), Some(JobEvent::Failed { .. })), "{:?}", events.last());
}

#[test]
fn wall_limit_fails_the_job_with_limit_wall() {
    let sup = Supervisor::new();
    let started = Instant::now();
    let handle = sup.start(spec(&["--sleep", "30"], Limits { wall: Some(Duration::from_secs(1)), ..Default::default() })).unwrap();
    let events = collect(&handle, Duration::from_secs(6));
    assert!(started.elapsed() < Duration::from_secs(4), "{:?}", started.elapsed());
    match events.last() {
        Some(JobEvent::Failed { limit, .. }) => assert_eq!(*limit, Some(LimitHit::Wall)),
        other => panic!("expected Failed{{limit: Wall}}, got {other:?}"),
    }
}

#[test]
fn cancel_emits_failed_canceled_and_nothing_after() {
    let sup = Supervisor::new();
    let handle = sup.start(spec(&["--sleep", "30"], Limits::default())).unwrap();
    // Let it start.
    let first = handle.events().recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(first, JobEvent::Started { .. }), "{first:?}");
    handle.cancel();
    // Samples and output may still arrive; the first non-sample event is the end.
    let next = loop {
        match handle.events().recv_timeout(Duration::from_secs(5)).unwrap() {
            JobEvent::Sample(_) | JobEvent::Output(_) => continue,
            other => break other,
        }
    };
    match next {
        JobEvent::Failed { error, .. } => assert_eq!(error, "canceled"),
        other => panic!("expected Failed(canceled), got {other:?}"),
    }
    assert!(matches!(handle.events().recv_timeout(Duration::from_secs(2)), Err(std::sync::mpsc::RecvTimeoutError::Disconnected)));
    assert!(handle.is_finished());
}

#[test]
fn dropping_the_supervisor_kills_running_jobs() {
    let sup = Supervisor::new();
    let handle = sup.start(spec(&["--grandchild", "--sleep", "60"], Limits::default())).unwrap();
    let mut grandchild = None;
    let deadline = Instant::now() + Duration::from_secs(10);
    while grandchild.is_none() && Instant::now() < deadline {
        if let Ok(JobEvent::Output(s)) = handle.events().recv_timeout(Duration::from_millis(200)) {
            if let Some(p) = s.strip_prefix("grandchild_pid=") {
                grandchild = Some(p.trim().parse::<u32>().unwrap());
            }
        }
    }
    let grandchild = grandchild.expect("grandchild pid reported");
    assert!(process_exists(grandchild));
    drop(handle);
    drop(sup);
    let deadline = Instant::now() + Duration::from_secs(3);
    while process_exists(grandchild) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!process_exists(grandchild), "grandchild {grandchild} survived supervisor drop");
}

#[test]
fn stderr_tail_is_in_the_failure_message() {
    let sup = Supervisor::new();
    let handle = sup.start(spec(&["--stderr-lines", "30", "--exit", "2"], Limits::default())).unwrap();
    let events = collect(&handle, Duration::from_secs(10));
    match events.last() {
        Some(JobEvent::Failed { error, .. }) => {
            assert!(error.contains("stderr line 30"), "{error}");
            assert!(!error.contains("stderr line 1\n"), "{error}");
            assert!(error.contains("exit"), "{error}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn memory_limit_ends_the_job_via_the_supervisor() {
    // Covers macOS, where no kernel rlimit applies: the sampler path must end the job.
    let sup = Supervisor::new();
    let handle = sup.start(JobSpec { sample_interval: Duration::from_millis(100), ..spec(&["--alloc", "512"], Limits { memory_bytes: Some(128 * 1024 * 1024), ..Default::default() }) }).unwrap();
    let events = collect(&handle, Duration::from_secs(10));
    match events.last() {
        Some(JobEvent::Failed { limit, .. }) => assert_eq!(*limit, Some(LimitHit::Memory)),
        other => panic!("expected Failed{{limit: Memory}}, got {other:?}"),
    }
}

#[test]
fn progress_emitted_right_before_exit_is_not_lost() {
    // Two thousand units written back to back, then exit: every one must arrive even
    // though most are still in the pipe when the child's exit is noticed.
    let sup = Supervisor::new();
    let handle = sup.start(spec(&["--emit", "2000"], Limits::default())).unwrap();
    let events = collect(&handle, Duration::from_secs(20));
    let progress = events.iter().filter(|e| matches!(e, JobEvent::Progress { .. })).count();
    assert_eq!(progress, 2000, "last events: {:?}", &events[events.len().saturating_sub(3)..]);
}

#[test]
fn non_utf8_output_does_not_stop_the_reader() {
    let sup = Supervisor::new();
    let handle = sup.start(spec(&["--bytes", "--flood", "20000"], Limits { wall: Some(Duration::from_secs(20)), ..Default::default() })).unwrap();
    let events = collect(&handle, Duration::from_secs(15));
    let outputs = events.iter().filter(|e| matches!(e, JobEvent::Output(_))).count();
    assert!(outputs >= 20000, "{outputs} output lines; last {:?}", events.last());
    assert!(matches!(events.last(), Some(JobEvent::Failed { limit: None, .. })), "{:?}", events.last());
}

#[test]
fn cancel_is_honoured_while_the_consumer_is_not_reading() {
    // A flooding worker and a consumer that never drains: cancel must still end the job.
    let sup = Supervisor::new();
    let handle = sup.start(spec(&["--flood", "2000000", "--sleep", "30"], Limits::default())).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    handle.cancel();
    let deadline = Instant::now() + Duration::from_secs(6);
    while !handle.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(handle.is_finished(), "job did not finish after cancel while the consumer was idle");
}

#[test]
fn fast_sampling_during_the_kill_grace_does_not_hang_shutdown() {
    let sup = Supervisor::new();
    let handle = sup.start(JobSpec { sample_interval: Duration::from_millis(1), ..spec(&["--trap-term"], Limits::default()) }).unwrap();
    let _ = handle.events().recv_timeout(Duration::from_secs(5)).unwrap(); // Started
    std::thread::sleep(Duration::from_millis(300));
    handle.cancel();
    let events = collect(&handle, Duration::from_secs(8));
    assert!(matches!(events.last(), Some(JobEvent::Failed { error, .. }) if error == "canceled"), "{:?}", events.last());
    let started = Instant::now();
    drop(handle);
    sup.shutdown();
    assert!(started.elapsed() < Duration::from_secs(5), "shutdown took {:?}", started.elapsed());
}

#[cfg(unix)]
#[test]
fn closed_pipes_do_not_spin_the_job_thread() {
    fn user_cpu() -> Duration {
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
        Duration::new(usage.ru_utime.tv_sec as u64, (usage.ru_utime.tv_usec * 1000) as u32)
    }
    // The sampler's own refresh cost is the same in both runs; only a spinning
    // job loop would make the closed-pipe run markedly more expensive.
    let sup = Supervisor::new();
    let before = user_cpu();
    let baseline = collect(&sup.start(spec(&["--sleep", "2"], Limits::default())).unwrap(), Duration::from_secs(10));
    let open_pipes = user_cpu() - before;
    assert!(matches!(baseline.last(), Some(JobEvent::Failed { .. })), "{:?}", baseline.last());
    let before = user_cpu();
    let events = collect(&sup.start(spec(&["--close-stdout", "--sleep", "2"], Limits::default())).unwrap(), Duration::from_secs(10));
    let closed_pipes = user_cpu() - before;
    assert!(matches!(events.last(), Some(JobEvent::Failed { .. })), "{:?}", events.last());
    assert!(closed_pipes < open_pipes + Duration::from_millis(400), "closed pipes cost {closed_pipes:?} against {open_pipes:?} with open pipes");
}

#[test]
fn samples_are_events_of_their_own() {
    let sup = Supervisor::new();
    let handle = sup.start(JobSpec { sample_interval: Duration::from_millis(200), ..spec(&["--sleep", "2"], Limits::default()) }).unwrap();
    let events = collect(&handle, Duration::from_secs(10));
    let samples = events.iter().filter(|e| matches!(e, JobEvent::Sample(_))).count();
    assert!(samples >= 5, "{samples} samples in 2 s at 200 ms");
}

#[cfg(target_os = "macos")]
#[test]
fn a_crash_under_a_memory_limit_is_not_called_a_memory_hit_on_macos() {
    let sup = Supervisor::new();
    let handle = sup.start(spec(&["--segfault"], Limits { memory_bytes: Some(512 * 1024 * 1024), ..Default::default() })).unwrap();
    let events = collect(&handle, Duration::from_secs(10));
    match events.last() {
        Some(JobEvent::Failed { limit, .. }) => assert_eq!(*limit, None, "no memory rlimit applies on macOS"),
        other => panic!("expected Failed, got {other:?}"),
    }
}
