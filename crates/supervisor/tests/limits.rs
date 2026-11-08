use model_lab_supervisor::limits::Limits;
use model_lab_supervisor::process::*;
use std::time::{Duration, Instant};

fn worker(args: &[&str]) -> Command {
    Command {
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_fake-worker")),
        args: args.iter().map(|s| s.to_string()).collect(),
        cwd: std::env::temp_dir(),
        env: vec![],
    }
}

fn wait_for_exit(child: &mut Child, within: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + within;
    // Drain so a chatty child cannot block on a full pipe.
    while Instant::now() < deadline {
        while child.lines().try_recv().is_ok() {}
        if let Some(s) = child.try_wait().unwrap() {
            return Some(s);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

// macOS has no process memory rlimit the kernel enforces; there the
// supervisor's sampler ends the job (tested through `Supervisor` in job.rs).
#[cfg(not(target_os = "macos"))]
#[test]
fn memory_limit_ends_the_job() {
    let limits = Limits { memory_bytes: Some(128 * 1024 * 1024), ..Default::default() };
    let mut child = spawn(worker(&["--alloc", "512"]), "n0", &limits).unwrap();
    let status = wait_for_exit(&mut child, Duration::from_secs(10)).expect("child ended within 10 s");
    assert!(!status.success(), "{status:?}");
}

#[test]
fn cpu_limit_ends_a_spinning_job() {
    let limits = Limits { cpu: Some(Duration::from_secs(1)), ..Default::default() };
    let mut child = spawn(worker(&["--spin", "30"]), "n0", &limits).unwrap();
    let status = wait_for_exit(&mut child, Duration::from_secs(5)).expect("child ended within 5 s");
    assert!(!status.success(), "{status:?}");
}

#[test]
fn deny_network_is_recorded_not_claimed() {
    let limits = Limits { deny_network: true, ..Default::default() };
    assert_eq!(
        limits.describe(),
        "network: not restricted (requested denial is not enforced in this build)"
    );
    let none = Limits::default();
    assert_eq!(none.describe(), "no limits; network: not restricted");
}
