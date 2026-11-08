//! Scripted child for the supervisor's tests. Not shipped: it exists so the
//! process, limit and job tests have a deterministic partner.
use std::io::Write;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let nonce = std::env::var("MODEL_LAB_NONCE").unwrap_or_default();
    let mut out = std::io::stdout().lock();
    let mut i = 0;
    let mut unit_delay = Duration::ZERO;
    while i < args.len() {
        let next = |i: usize| args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--unit-delay-ms" => {
                unit_delay = Duration::from_millis(next(i).parse().unwrap());
                i += 1;
            }
            "--emit" => {
                let n: u64 = next(i).parse().unwrap();
                for u in 1..=n {
                    std::thread::sleep(unit_delay);
                    writeln!(out, "{}", model_lab_supervisor::protocol::encode(&nonce, model_lab_supervisor::protocol::Kind::Progress { units_done: u, units_total: Some(n), unit: "unit".into(), stage: None })).unwrap();
                    out.flush().unwrap();
                }
                i += 1;
            }
            "--spoof" => {
                writeln!(out, "{}", model_lab_supervisor::protocol::encode("00000000000000000000000000000000", model_lab_supervisor::protocol::Kind::Failed { error: "spoof".into() })).unwrap();
            }
            "--flood" => {
                let n: u64 = next(i).parse().unwrap();
                for k in 0..n {
                    writeln!(out, "line {k}").unwrap();
                }
                out.flush().unwrap();
                i += 1;
            }
            "--grandchild" => {
                let child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--sleep", "60"])
                    .spawn()
                    .unwrap();
                writeln!(out, "grandchild_pid={}", child.id()).unwrap();
                out.flush().unwrap();
                // Exit without waiting so the grandchild outlives its parent.
                std::process::exit(0);
            }
            "--grandchild-alloc" => {
                // Spawn a grandchild that allocates MB and sleeps, report its pid, exit.
                let child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--alloc", &next(i)])
                    .spawn()
                    .unwrap();
                writeln!(out, "grandchild_pid={}", child.id()).unwrap();
                out.flush().unwrap();
                std::process::exit(0);
            }
            "--sleep" => {
                std::thread::sleep(Duration::from_secs_f64(next(i).parse().unwrap()));
                i += 1;
            }
            "--trap-term" => {
                #[cfg(unix)]
                unsafe {
                    libc::signal(libc::SIGTERM, libc::SIG_IGN);
                }
                writeln!(out, "trapped").unwrap();
                out.flush().unwrap();
                std::thread::sleep(Duration::from_secs(60));
            }
            "--alloc" => {
                let mb: usize = next(i).parse().unwrap();
                let mut block = vec![0u8; mb * 1024 * 1024];
                for k in (0..block.len()).step_by(4096) {
                    block[k] = 1;
                }
                std::hint::black_box(&block);
                std::thread::sleep(Duration::from_secs(5));
                i += 1;
            }
            "--spin" => {
                let until = Instant::now() + Duration::from_secs_f64(next(i).parse().unwrap());
                let mut x = 0u64;
                while Instant::now() < until {
                    x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                }
                std::hint::black_box(x);
                i += 1;
            }
            "--stderr-lines" => {
                let n: u64 = next(i).parse().unwrap();
                let mut err = std::io::stderr().lock();
                for k in 1..=n {
                    writeln!(err, "stderr line {k}").unwrap();
                }
                i += 1;
            }
            "--print-env" => {
                let mut vars: Vec<_> = std::env::vars().collect();
                vars.sort();
                for (k, v) in vars {
                    writeln!(out, "{k}={v}").unwrap();
                }
            }
            "--exit" => {
                out.flush().unwrap();
                std::process::exit(next(i).parse().unwrap());
            }
            other => panic!("fake-worker: unknown argument {other}"),
        }
        i += 1;
    }
}
