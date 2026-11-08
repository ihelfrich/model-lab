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

fn drain(child: &Child) -> Vec<String> {
    let mut out = vec![];
    while let Ok(line) = child.lines().recv_timeout(Duration::from_secs(20)) {
        if let ChildLine::Stdout(s) = line {
            out.push(s);
        }
    }
    out
}

fn process_exists(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // A killed process may linger as a zombie until its new parent reaps
        // it (CI runners often do not), so ask ps for the state and treat
        // zombies as gone.
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
fn grandchild_dies_with_the_group() {
    let mut child = spawn(worker(&["--grandchild"]), "n0", &Limits::default()).unwrap();
    let mut grandchild = None;
    while let Ok(line) = child.lines().recv_timeout(Duration::from_secs(10)) {
        if let ChildLine::Stdout(s) = line {
            if let Some(p) = s.strip_prefix("grandchild_pid=") {
                grandchild = Some(p.trim().parse::<u32>().unwrap());
                break;
            }
        }
    }
    let grandchild = grandchild.expect("fake-worker reports its grandchild pid");
    assert!(process_exists(grandchild), "grandchild should be alive before the kill");
    child.kill_tree(Duration::from_secs(1)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while process_exists(grandchild) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!process_exists(grandchild), "grandchild {grandchild} survived kill_tree");
}

#[test]
fn trap_term_is_killed_within_grace_plus_one_second() {
    let mut child = spawn(worker(&["--trap-term"]), "n0", &Limits::default()).unwrap();
    // Wait until the worker reports its handler is installed.
    while let Ok(line) = child.lines().recv_timeout(Duration::from_secs(10)) {
        if matches!(&line, ChildLine::Stdout(s) if s.trim() == "trapped") {
            break;
        }
    }
    let started = Instant::now();
    let status = child.kill_tree(Duration::from_secs(1)).unwrap();
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
    assert!(!status.success());
}

#[test]
fn flood_does_not_grow_memory() {
    let started = Instant::now();
    let child = spawn(worker(&["--flood", "1000000"]), "n0", &Limits::default()).unwrap();
    let lines = drain(&child);
    assert_eq!(lines.len(), 1_000_000);
    assert!(started.elapsed() < Duration::from_secs(30), "took {:?}", started.elapsed());
}

#[test]
fn readers_end_when_child_exits() {
    let mut child = spawn(worker(&["--exit", "3"]), "n0", &Limits::default()).unwrap();
    let _ = drain(&child);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut status = None;
    while status.is_none() && Instant::now() < deadline {
        status = child.try_wait().unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(status.unwrap().code(), Some(3));
}

#[test]
fn environment_is_minimal_and_carries_nonce() {
    let child = spawn(worker(&["--print-env"]), "abc123", &Limits::default()).unwrap();
    let lines = drain(&child);
    assert!(lines.iter().any(|l| l == "MODEL_LAB_NONCE=abc123"), "{lines:?}");
    for forbidden in ["HOME=", "USER=", "USERPROFILE="] {
        assert!(!lines.iter().any(|l| l.starts_with(forbidden)), "{forbidden} leaked: {lines:?}");
    }
}
