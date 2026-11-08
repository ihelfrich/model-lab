use model_lab_supervisor::limits::Limits;
use model_lab_supervisor::process::*;
use model_lab_supervisor::resources::{Sample, Sampler};
use std::sync::mpsc::sync_channel;
use std::time::{Duration, Instant};

fn worker(args: &[&str]) -> Command {
    Command {
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_fake-worker")),
        args: args.iter().map(|s| s.to_string()).collect(),
        cwd: std::env::temp_dir(),
        env: vec![],
    }
}

#[test]
fn samples_arrive_at_the_declared_interval() {
    let child = spawn(worker(&["--sleep", "5"]), "n0", &Limits::default()).unwrap();
    let (tx, rx) = sync_channel::<Sample>(64);
    let sampler = Sampler::start(child.pid(), Duration::from_millis(250), tx);
    let started = Instant::now();
    let mut samples = vec![];
    while started.elapsed() < Duration::from_secs(3) {
        if let Ok(s) = rx.recv_timeout(Duration::from_millis(500)) {
            samples.push(s);
        }
    }
    sampler.stop();
    assert!((8..=14).contains(&samples.len()), "{} samples in 3 s", samples.len());
    assert!(samples.iter().all(|s| s.processes >= 1), "{samples:?}");
    assert!(samples.iter().all(|s| s.gpu_memory_bytes.is_none()));
    assert!(samples.windows(2).all(|w| w[1].at_ms >= w[0].at_ms));
}

#[test]
fn tree_rss_includes_the_grandchild() {
    // The worker spawns a grandchild that allocates 64 MiB and sleeps, then exits
    // itself; the sampler must still see the grandchild through the tree.
    let child = spawn(worker(&["--grandchild-alloc", "64"]), "n0", &Limits::default()).unwrap();
    let (tx, rx) = sync_channel::<Sample>(64);
    let sampler = Sampler::start(child.pid(), Duration::from_millis(200), tx);
    let started = Instant::now();
    let mut best: Option<Sample> = None;
    while started.elapsed() < Duration::from_secs(4) {
        if let Ok(s) = rx.recv_timeout(Duration::from_millis(500)) {
            if best.as_ref().is_none_or(|b| s.rss_bytes > b.rss_bytes) {
                best = Some(s);
            }
        }
    }
    sampler.stop();
    let best = best.expect("at least one sample");
    assert!(best.rss_bytes >= 64 * 1024 * 1024, "{best:?}");
    assert!(best.processes >= 1, "{best:?}");
}
