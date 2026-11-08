# Engine crate and job supervisor — Phase 1, plan 1 of 3

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the numerical core into a UI-free library crate, add the
Rust job supervisor, and run a 50,000-row native boosting job under it with
progress, an ETA range, resource readings, checkpoints that survive a kill,
and no orphaned process.

**Architecture:** The repository becomes a Cargo workspace whose root
package stays the existing egui application (so every build script, CI step
and `verify_*.py` path is unchanged). Two new crates appear under `crates/`:
`model-lab-engine` (data, solvers, ensembles, experiment record, study
documents; no `eframe`) with a small `model-lab-engine-job` binary that
speaks the supervisor protocol, and `model-lab-supervisor` (process tree
control, limits, nonce-tagged NDJSON protocol v3, checkpoints, ETA,
resource sampling). The egui app consumes the engine crate through
re-exports so `crate::data::…` paths keep compiling; its Python sessions
(`python.rs`, `adapter.rs`) are left as they are and retire with the shell
in plan 2.

**Tech Stack:** Rust 2021, serde/serde_json (pinned as in the root
manifest), `libc` (Unix process groups and rlimits), `windows-sys` (Job
Objects), `sysinfo` (CPU and RSS sampling), `sha2`. No async runtime: the
supervisor uses threads and bounded `std::sync::mpsc` channels.

**Spec:** `docs/superpowers/specs/2026-11-07-model-lab-platform-design.md`
(sections "1. Engine crate", "2. Job supervisor", "Phase 1 scope: Every job
goes through the supervisor", "Engine extraction is a pure move first").

**Scope note.** Phase 1 in the spec spans four subsystems. This plan is the
first of three and produces working, tested software on its own:
1. this plan — engine crate, supervisor, supervised native job;
2. Tauri shell, lesson format, lessons 1–4, learner record;
3. pixi environment layer, scientific/study workers on protocol 3,
   practice editor, lessons 5–10.
The spec fixes the order: the engine crate and the supervisor come before
any lesson.

## Global Constraints

- Shipping targets: Mac (Apple silicon) and Windows x64; Intel Mac and ARM
  Windows are nice to have. Every new crate must build on both shipping
  targets in CI.
- No data leaves the machine; no telemetry; network only for environment
  downloads the user agreed to (none in this plan).
- No spend without Dr. Helfrich's approval.
- The delivered 0.5–0.8 archives are not modified.
- Anything numerical that is kept keeps its existing tests and independent
  fixtures: `verify_linear_models.py` (15 sklearn fixtures), the Engel
  NumPy/R reference, the simulation validation groups.
- Anti-goals: no fabricated progress or ETA shown as a point estimate; a
  rate never without its denominator; no harness or developer-machine path
  compiled into a shipped binary (the `acceptance` feature stays non-default).
- Dependencies are pinned with `=` exactly as the root `Cargo.toml` does.
- Numerical changes land as separately versioned changes with regenerated
  fixtures and an entry in `docs/CHANGED-RESULTS.md`; saved 0.8 experiments
  open read-only with a notice naming the change.
- Commit after every task; messages in the repository's plain style, ending
  with the attribution line used in this repository's recent commits.

## Review Focus

Failure modes the spec implies but no task's tests would otherwise exercise;
each is pinned to a test in the task that owns the code.

1. A worker floods stdout (a million lines of plain output). Expected: the
   supervisor applies backpressure and keeps a bounded buffer; memory stays
   flat and the job still completes. → Task 7.
2. A worker emits a line that carries the right nonce but malformed JSON.
   Expected: it is delivered as plain output, the job continues, nothing
   panics. → Task 4.
3. A checkpoint is truncated mid-write (simulated power loss on the
   temporary file). Expected: resume refuses the damaged file with a message
   naming the path, and the job starts fresh rather than crashing. → Task 6.
4. Unit times are wildly uneven (the first three units take 10×, the rest
   are fast). Expected: the ETA range widens, the low bound never goes
   negative, and it is still labelled with the number of units observed.
   → Task 5.
5. A worker ignores SIGTERM (traps it). Expected: cancel still returns
   within the grace period plus one second, the whole process group is gone,
   and the event stream ends with `Failed("canceled")`. → Task 7.

---

## File structure

```
Cargo.toml                         root package (app) + [workspace]
crates/engine/Cargo.toml           model-lab-engine (lib) + bin model-lab-engine-job
crates/engine/src/lib.rs           pub mod data, linear, model, science, experiment, study, decision, provenance
crates/engine/src/{data,linear,model,science,experiment,study}.rs   moved from src/, unchanged numerics
crates/engine/src/decision.rs      numerical half of src/decision.rs (Evidence, evaluate, NativeStress …)
crates/engine/src/provenance.rs    sha256(), identity() (moved from adapter.rs)
crates/engine/src/ensemble.rs      resumable stage-wise ensemble stepper (Task 11)
crates/engine/src/bin/engine-job.rs   `model-lab-engine-job <spec.json>` (Task 12)
crates/engine/tests/fixtures/      regenerated tree fixtures (Task 11)
crates/supervisor/Cargo.toml       model-lab-supervisor (lib) + bin fake-worker (tests only)
crates/supervisor/src/lib.rs       pub mod protocol, eta, checkpoint, limits, process, resources, job
crates/supervisor/src/protocol.rs  v3 envelope, nonce, parse_line
crates/supervisor/src/eta.rs       Eta accumulator → EtaRange
crates/supervisor/src/checkpoint.rs atomic write, verified read
crates/supervisor/src/limits.rs    Limits + platform application
crates/supervisor/src/process.rs   spawn in group/Job Object, readers, synchronous kill
crates/supervisor/src/resources.rs sampler thread
crates/supervisor/src/job.rs       Supervisor, JobSpec, JobHandle, JobEvent
crates/supervisor/src/bin/fake-worker.rs   scripted child for tests
crates/supervisor/tests/end_to_end.rs      supervisor × engine-job (Task 13)
src/main.rs                        `mod data;` … become `use model_lab_engine::data;` …
src/decision.rs                    UI half only; numerics imported from the engine
src/adapter.rs                     keeps WORKER constants; sha256/identity re-exported from engine
docs/CHANGED-RESULTS.md            created in Task 11
.github/workflows/engine.yml       macOS + Windows: cargo test --workspace (Task 3)
```

---

### Task 1: Workspace and the pure move of the five UI-free modules

**Files:**
- Modify: `Cargo.toml` (add `[workspace] members = ["crates/engine"]`, add
  `model-lab-engine = { path = "crates/engine" }` to `[dependencies]`)
- Create: `crates/engine/Cargo.toml`, `crates/engine/src/lib.rs`
- Move: `src/data.rs`, `src/linear.rs`, `src/model.rs`, `src/science.rs`,
  `src/experiment.rs` → `crates/engine/src/` (use `git mv`; do not edit the
  numerics)
- Modify: `src/main.rs:5-28` (replace each moved `mod x;` with
  `use model_lab_engine::x;`)

**Interfaces:**
- Produces: crate `model_lab_engine` with `pub mod data; pub mod linear;
  pub mod model; pub mod science; pub mod experiment;`. Every item that was
  `pub` in the moved files stays `pub` with the same path shape
  (`model_lab_engine::experiment::Run`, `::model::fit`, `::data::Table` …).
- Dependencies of the engine crate: `serde`, `serde_json`, `rand` at the
  root's pinned versions. Nothing else.

- [ ] **Step 1: Write the failing test** — in `crates/engine/src/lib.rs`:

```rust
#[cfg(test)]
mod boundary {
    #[test]
    fn engine_has_no_ui_dependency() {
        let manifest = include_str!("../Cargo.toml");
        for banned in ["eframe", "egui", "wgpu", "png"] {
            assert!(!manifest.contains(banned), "{banned} in engine manifest");
        }
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p model-lab-engine engine_has_no_ui_dependency`
Expected: fails to compile (no such package) or the test panics because the
crate does not yet exist.

- [ ] **Step 3: Create the workspace and move the files**

`crates/engine/Cargo.toml`: `[package] name = "model-lab-engine", version =
"0.9.0", edition = "2021", publish = false`; `[lib] path = "src/lib.rs"`;
dependencies copied from the root with their `=` pins. Root `Cargo.toml`
gains the `[workspace]` table and the path dependency. In `src/main.rs`,
`mod data;` → `use model_lab_engine::data;` for the five modules. Inside the
moved files, `use crate::data` stays valid because `crate` is now the engine.
`science.rs` uses `crate::model` — also valid.

- [ ] **Step 4: Run the whole suite and the boundary test**

Run: `cargo test --workspace --locked` and `cargo test --workspace --locked --features tree-ensemble-lab/acceptance`
Expected: 64 tests pass in total across both crates (the moved tests now
run under `model-lab-engine`); `engine_has_no_ui_dependency` passes.
`Cargo.lock` changes only by adding the new package entries.

- [ ] **Step 5: Verify the fixture script is untouched**

Run: `cargo build --release --features acceptance && uv run --offline python verify_linear_models.py --binary target/release/tree-ensemble-lab`
Expected: 15 fixtures pass, max error ≤ 1e-8 (same as Phase 0).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/engine src/main.rs
git commit -m "Extract the UI-free numerical modules into the model-lab-engine crate"
```

---

### Task 2: Move study documents, decision numerics and provenance helpers

**Files:**
- Create: `crates/engine/src/provenance.rs`
- Move: `src/study.rs` → `crates/engine/src/study.rs`
- Split: `src/decision.rs:1-222` → `crates/engine/src/decision.rs`;
  `src/decision.rs:223-602` stays and begins `use model_lab_engine::decision::*;`
- Modify: `src/adapter.rs:22-37` (delete `sha256`, `identity`; add
  `pub use model_lab_engine::provenance::{sha256, identity};`), `src/main.rs`
- Modify: `crates/engine/src/lib.rs` (add `pub mod study; pub mod decision;
  pub mod provenance;`)

**Interfaces:**
- Produces: `provenance::sha256(bytes: &[u8]) -> String` (hex, lowercase)
  and `provenance::identity() -> String` (unchanged bodies). Engine
  `Cargo.toml` gains `sha2` at the root pin.
- `study.rs` keeps every public item. Its two functions that need the
  Python worker sources, `manifest()` and `bundle_hash()` (`study.rs:592-597`),
  move to `src/adapter.rs` as `pub fn study_manifest() -> Value` and
  `pub fn study_bundle_hash() -> String`; `study.rs` loses its `WORKER`
  constant (the app's `adapter.rs` gains `pub const STUDY_WORKER: &str =
  include_str!("../python/study_worker.py");`). Call sites in `study_ui.rs`
  and `study_acceptance.rs` follow the rename.

- [ ] **Step 1: Write the failing tests** — in `crates/engine/src/provenance.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
    #[test]
    fn identities_are_distinct() {
        assert_ne!(identity(), identity());
    }
}
```

and in `crates/engine/src/decision.rs` keep the existing `evaluate` tests
(if `src/decision.rs` has none, add one: `evaluate` on a two-product
`Evidence` built in the test returns an `Outcome` whose fields equal the
hand-computed values from the struct's documented formula).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-engine provenance`
Expected: compile error, module missing.

- [ ] **Step 3: Move and split as listed in Files; fix imports**

`study.rs`: `use crate::adapter;` → `use crate::provenance::{sha256, identity};`
and the three call sites follow. The egui half of `decision.rs` keeps
`impl Decision` and the `*_ui` methods only.

- [ ] **Step 4: Run the whole suite**

Run: `cargo test --workspace --locked --features tree-ensemble-lab/acceptance`
Expected: all previous tests plus the two provenance tests pass. The
`acceptance` build still writes identical `study_manifest()` hashes
(compare `--study-self-test --report` output's `worker_sha256` with the
value in `verification-study-native-mac.json`).

- [ ] **Step 5: Commit**

```bash
git add -A crates/engine src
git commit -m "Move study documents, decision numerics and provenance helpers into the engine"
```

---

### Task 3: Engine CI on both shipping platforms

**Files:**
- Create: `.github/workflows/engine.yml`
- Modify: `Build Mac.command` (the `cargo test` line becomes
  `cargo test --workspace --locked --features tree-ensemble-lab/acceptance`)
- Modify: `.github/workflows/windows.yml:27-33` (same flag change in the
  test step)

**Interfaces:**
- Produces: a required check named `engine` with a job matrix
  `[macos-14, windows-2022]` running `cargo test --workspace --locked` and
  `cargo build -p model-lab-engine --release --locked`; no Python, no
  packaging. Triggers: push to `main`, pull requests.

- [ ] **Step 1: Write the workflow** (`permissions: contents: read`,
  `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`, two steps).

- [ ] **Step 2: Verify locally that the flag spelling is right**

Run: `cargo test --workspace --locked --features tree-ensemble-lab/acceptance`
Expected: PASS.

- [ ] **Step 3: Commit and push; confirm both matrix legs are green**

```bash
git add .github/workflows/engine.yml "Build Mac.command" .github/workflows/windows.yml
git commit -m "CI: test the workspace on macOS and Windows"
git push
gh run list --limit 3
```

Expected: `engine` succeeds on both legs; `Windows student package` still
succeeds.

---

### Task 4: Protocol v3 — envelope, nonce, line parser

**Files:**
- Create: `crates/supervisor/Cargo.toml`, `crates/supervisor/src/lib.rs`,
  `crates/supervisor/src/protocol.rs`
- Modify: root `Cargo.toml` workspace members (add `crates/supervisor`)

**Interfaces:**
- Produces:

```rust
pub const VERSION: u32 = 3;
pub fn nonce() -> String;                       // 32 hex chars from OS randomness
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Kind {
    Progress { units_done: u64, units_total: Option<u64>, unit: String, stage: Option<String> },
    Checkpoint { path: String, sha256: String },
    Completed { result: serde_json::Value },
    Failed { error: String },
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Envelope { pub v: u32, pub nonce: String, #[serde(flatten)] pub kind: Kind }
pub enum Line { Event(Envelope), Output(String) }
pub fn parse_line(line: &str, nonce: &str) -> Line;
pub fn encode(nonce: &str, kind: Kind) -> String;   // one JSON object, no newline
```

`parse_line` returns `Event` only when the line parses as JSON, `v == 3`
and `nonce` equals the expected nonce; everything else is `Output(line)`.
Workers receive the nonce in the environment variable `MODEL_LAB_NONCE`.

- [ ] **Step 1: Write the failing tests** — `crates/supervisor/src/protocol.rs`:

```rust
#[test] fn round_trip_progress() {
    let n = nonce();
    let k = Kind::Progress { units_done: 3, units_total: Some(10), unit: "tree".into(), stage: None };
    match parse_line(&encode(&n, k.clone()), &n) { Line::Event(e) => assert_eq!(e.kind, k), _ => panic!() }
}
#[test] fn wrong_nonce_is_plain_output() {
    let line = encode("deadbeef", Kind::Failed { error: "spoof".into() });
    assert!(matches!(parse_line(&line, &nonce()), Line::Output(_)));
}
#[test] fn right_nonce_malformed_json_is_plain_output() {           // Review Focus 2
    let n = nonce();
    let line = format!("{{\"v\":3,\"nonce\":\"{n}\",\"kind\":\"progress\",\"units_done\":");
    assert!(matches!(parse_line(&line, &n), Line::Output(_)));
}
#[test] fn wrong_version_is_plain_output() {
    let n = nonce();
    let line = encode(&n, Kind::Failed { error: "x".into() }).replace("\"v\":3", "\"v\":2");
    assert!(matches!(parse_line(&line, &n), Line::Output(_)));
}
#[test] fn nonces_are_32_hex_and_distinct() {
    let a = nonce(); assert_eq!(a.len(), 32); assert!(a.chars().all(|c| c.is_ascii_hexdigit())); assert_ne!(a, nonce());
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p model-lab-supervisor protocol` → compile error.
- [ ] **Step 3: Implement** the items in Interfaces (nonce from `getrandom`
  or `rand::rngs::OsRng` at the root pin; prefer `rand` since it is already
  pinned).
- [ ] **Step 4: Run** — `cargo test -p model-lab-supervisor protocol` → 5 passed.
- [ ] **Step 5: Commit** — `git add Cargo.toml Cargo.lock crates/supervisor && git commit -m "Supervisor: protocol v3 envelope with nonce-tagged events"`

---

### Task 5: ETA as a range with its own uncertainty

**Files:**
- Create: `crates/supervisor/src/eta.rs`

**Interfaces:**
- Produces:

```rust
pub struct Eta { /* per-unit durations observed so far */ }
#[derive(Debug, Clone, PartialEq)]
pub struct EtaRange { pub low: Duration, pub high: Duration, pub units_observed: u32, pub units_remaining: u64 }
impl Eta {
    pub fn new() -> Self;
    pub fn observe(&mut self, units_done: u64, at: Instant);   // monotone; ignores repeats
    pub fn estimate(&self, units_total: u64, now: Instant) -> Option<EtaRange>;
}
```

Rules fixed by the spec: `None` until at least three units have completed.
Range: `low = remaining × p10(per-unit)`, `high = remaining × p90(per-unit)`
where per-unit durations come from consecutive `observe` calls (if several
units complete between calls, the interval is split evenly among them); with
fewer than ten observations p10/p90 are the min and max. Both bounds are
reduced by the time already elapsed on the unit in progress, floored at zero.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn no_estimate_before_three_units() { /* observe units 1 and 2 → estimate() is None */ }
#[test] fn steady_units_give_tight_range() {
    // 1 s per unit, 3 observed of 10 → low == high == 7 s (within 1 ms)
}
#[test] fn uneven_units_widen_but_never_go_negative() {      // Review Focus 4
    // units 1..=3 at 10 s each, units 4..=8 at 1 s each, total 20:
    // estimate at unit 8 has low == 12 s (12 remaining × 1 s), high == 120 s, units_observed == 8
    // then advance `now` by 200 s without new units → low == 0, high == 0 (floored), still Some
}
#[test] fn repeated_units_done_is_ignored() { /* observe(3, t) twice adds one unit, not two */ }
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p model-lab-supervisor eta` → compile error.
- [ ] **Step 3: Implement `Eta`** per the rules above (percentile by sorting a copy; sizes are tiny).
- [ ] **Step 4: Run** — 4 passed.
- [ ] **Step 5: Commit** — `git commit -am "Supervisor: ETA as a range from observed unit times"`

---

### Task 6: Atomic, verified checkpoints

**Files:**
- Create: `crates/supervisor/src/checkpoint.rs`

**Interfaces:**
- Produces:

```rust
pub fn write_atomic(path: &Path, document: &serde_json::Value) -> Result<String, CheckpointError>; // returns sha256 of the document bytes
pub fn read_verified(path: &Path) -> Result<serde_json::Value, CheckpointError>;
#[derive(Debug)] pub enum CheckpointError { Io(std::io::Error, PathBuf), Corrupt(PathBuf), WrongVersion(PathBuf, u64) }
```

On-disk envelope: `{"v":3,"sha256":"<hex of document bytes>","document":<document>}`
where document bytes are `serde_json::to_vec(document)` (canonical because
`serde_json` preserves map insertion order and the writer builds the
document itself). Write to `<path>.tmp-<pid>`, `sync_all`, then
`std::fs::rename` over `path`. `read_verified` fails `Corrupt` when the
envelope does not parse, the hash differs or `document` is missing, and
`WrongVersion` when `v != 3`. Maximum file size 64 MiB, as today's study
worker enforces.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn write_then_read_round_trips_and_returns_hash() { /* value == read; returned hash == sha256 of to_vec */ }
#[test] fn truncated_file_is_reported_corrupt_with_path() {       // Review Focus 3
    // write_atomic, then truncate the file to half its length; read_verified → Err(Corrupt(p)) with p == path
}
#[test] fn stale_temporary_file_is_not_read() {
    // create "<path>.tmp-1" with junk and no final file → read_verified(path) is Err(Io(NotFound, _))
}
#[test] fn version_two_study_checkpoint_is_wrong_version_not_corrupt() {
    // write {"schema_version":2,"document_wire":"{}","document_sha256":"…"} by hand → Err(WrongVersion(_, 2))
}
```

- [ ] **Step 2: Run to verify failure.** — `cargo test -p model-lab-supervisor checkpoint`
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — 4 passed.
- [ ] **Step 5: Commit** — `git commit -am "Supervisor: atomic checkpoints verified by hash"`

---

### Task 7: Process spawn, process group / Job Object, readers, synchronous kill

**Files:**
- Create: `crates/supervisor/src/process.rs`, `crates/supervisor/src/bin/fake-worker.rs`
- Modify: `crates/supervisor/Cargo.toml` (add `libc` for `cfg(unix)`,
  `windows-sys` with features `Win32_System_JobObjects`,
  `Win32_System_Threading`, `Win32_Foundation` for `cfg(windows)`; add
  `[[bin]] name = "fake-worker"`)

**Interfaces:**
- Produces:

```rust
pub struct Command { pub program: PathBuf, pub args: Vec<String>, pub cwd: PathBuf, pub env: Vec<(String, String)> }
pub struct Child { /* pid, group/job handle, reader threads, bounded receiver */ }
pub enum ChildLine { Stdout(String), Stderr(String) }
pub fn spawn(cmd: Command, nonce: &str, limits: &Limits) -> std::io::Result<Child>;   // Limits from Task 8; pass Limits::default() until then
impl Child {
    pub fn lines(&self) -> &Receiver<ChildLine>;           // bounded, capacity 1024
    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>>;
    pub fn kill_tree(&mut self, grace: Duration) -> std::io::Result<ExitStatus>;  // SIGTERM group → wait grace → SIGKILL group; Windows: TerminateJobObject; blocks until exit
    pub fn pid(&self) -> u32;
}
```

The child's environment is built from scratch: `PATH`, `SystemRoot`,
`WINDIR`, `TEMP`, `TMP`, `TMPDIR` copied when present (the list
`src/python.rs:238` uses today), plus `MODEL_LAB_NONCE=<nonce>` and the
command's own `env`. Unix: `pre_exec` calls `setpgid(0, 0)`; kill uses
`killpg`. Windows: create a Job Object with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, assign the process (spawned suspended
is not required; assign immediately after spawn, before reading). Reader
threads use `BufRead::lines()` on each pipe and `send` (blocking) into the
bounded channel; they end when the pipe closes, which `kill_tree`
guarantees by killing every group member. `Drop for Child` calls
`kill_tree(Duration::from_secs(2))` if the process is still running.

`fake-worker` (a tiny CLI, test-only) understands: `--emit N` (N progress
lines with the nonce from `MODEL_LAB_NONCE`), `--spoof` (one progress line
with nonce `0000…`), `--flood N` (N plain lines), `--grandchild` (spawns
itself with `--sleep 60` inheriting stdout, then exits), `--sleep S`,
`--trap-term` (ignores SIGTERM on Unix, then sleeps 60), `--alloc MB`
(allocates and touches MB megabytes, then sleeps 5), `--spin S` (busy loop S
seconds), `--exit CODE`.

- [ ] **Step 1: Write the failing tests** (in `process.rs`, each spawning
  `env!("CARGO_BIN_EXE_fake-worker")`):

```rust
#[test] fn grandchild_dies_with_the_group() {
    // spawn --grandchild; wait for the child to exit; collect the grandchild pid it printed;
    // kill_tree(1 s) returns; assert the grandchild pid no longer exists (kill(pid, 0) == ESRCH on Unix;
    // OpenProcess fails or exit code != STILL_ACTIVE on Windows)
}
#[test] fn trap_term_is_killed_within_grace_plus_one_second() {   // Review Focus 5
    // spawn --trap-term; kill_tree(Duration::from_secs(1)); elapsed < 2 s; status is not success
}
#[test] fn flood_does_not_grow_memory() {                         // Review Focus 1
    // spawn --flood 1_000_000; drain lines() counting them; count == 1_000_000;
    // the receiver never held more than 1024 (structural: channel capacity), and the test finishes < 30 s
}
#[test] fn readers_end_when_child_exits() {
    // spawn --exit 3; lines() yields until Disconnected; try_wait → Some(code 3)
}
#[test] fn environment_is_minimal_and_carries_nonce() {
    // spawn with args ["--print-env"]: output has MODEL_LAB_NONCE=<nonce>, has no HOME/USER/USERPROFILE line
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p model-lab-supervisor process`.
- [ ] **Step 3: Implement `process.rs` and `fake-worker`.**
- [ ] **Step 4: Run** on this Mac: 5 passed. Push and confirm the Windows leg of `engine` passes too (the Job Object path has no local test machine).
- [ ] **Step 5: Commit** — `git commit -am "Supervisor: spawn in a process group or Job Object; synchronous tree kill; bounded readers"`

---

### Task 8: Per-job limits

**Files:**
- Create: `crates/supervisor/src/limits.rs`
- Modify: `crates/supervisor/src/process.rs` (`spawn` applies them)

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Limits { pub wall: Option<Duration>, pub cpu: Option<Duration>, pub memory_bytes: Option<u64>, pub deny_network: bool }
pub enum LimitHit { Wall, Cpu, Memory }
```

Unix: `cpu` → `setrlimit(RLIMIT_CPU)`, `memory_bytes` → `RLIMIT_AS` (Linux)
and `RLIMIT_RSS` is unreliable on macOS, so on macOS memory is enforced by
the supervisor's sampler (Task 9) killing the tree when RSS exceeds the
limit for two consecutive samples. Windows: `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`
with `JOB_OBJECT_LIMIT_PROCESS_MEMORY` and
`JOB_OBJECT_LIMIT_JOB_TIME` (CPU). `wall` is always enforced by the
supervisor clock (Task 10). `deny_network` is **recorded but not enforced**
in this plan: there is no portable per-process network denial without a
sandbox; the job manifest reports `"network": "not restricted"` and the
spec's "optional network denial" moves to plan 3 with the pixi layer. This
is a designer's call to record in the design document's "Left to the
builder" list.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn memory_limit_ends_the_job() {
    // --alloc 512 with memory_bytes = 128 MiB → child exits non-zero (or is killed by the sampler) within 10 s
}
#[test] fn cpu_limit_ends_a_spinning_job() {
    // --spin 30 with cpu = 1 s → exits within 5 s, non-zero
}
#[test] fn deny_network_is_recorded_not_claimed() {
    // Limits { deny_network: true, .. }.describe() == "network: not restricted (requested denial is not enforced in this build)"
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement** `Limits`, `impl Limits { pub fn describe(&self) -> String }`, and the platform application inside `spawn`.
- [ ] **Step 4: Run** locally, then on the Windows CI leg.
- [ ] **Step 5: Commit** — `git commit -am "Supervisor: per-job CPU and memory limits; network denial recorded, not claimed"`

---

### Task 9: Resource sampling

**Files:**
- Create: `crates/supervisor/src/resources.rs`
- Modify: `crates/supervisor/Cargo.toml` (add `sysinfo`, pinned)

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Sample { pub at_ms: u64, pub cpu_percent: f32, pub rss_bytes: u64, pub processes: u32, pub gpu_memory_bytes: Option<u64> /* always None in this plan */ }
pub struct Sampler { /* thread + stop flag */ }
impl Sampler {
    pub fn start(root_pid: u32, interval: Duration, sink: SyncSender<Sample>) -> Sampler;  // sums over the process tree rooted at root_pid
    pub fn stop(self);
}
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn samples_arrive_at_the_declared_interval() { /* --sleep 3, interval 250 ms → between 8 and 14 samples in 3 s */ }
#[test] fn tree_rss_includes_the_grandchild() { /* --grandchild --alloc 64 in the grandchild → a sample with rss ≥ 64 MiB and processes ≥ 2 */ }
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement** with `sysinfo::System::refresh_processes` filtered by parent chain.
- [ ] **Step 4: Run** — 2 passed (allow the interval test a generous window; it is a smoke test, not a timing benchmark).
- [ ] **Step 5: Commit** — `git commit -am "Supervisor: CPU and memory samples over the job's process tree"`

---

### Task 10: `Supervisor`, `JobSpec`, `JobHandle`, `JobEvent`

**Files:**
- Create: `crates/supervisor/src/job.rs`
- Modify: `crates/supervisor/src/lib.rs`

**Interfaces:**
- Produces:

```rust
pub struct JobSpec {
    pub label: String, pub command: Command, pub limits: Limits,
    pub units_total: Option<u64>, pub sample_interval: Duration,          // default 1 s
    pub checkpoint: Option<PathBuf>,                                      // passed to the worker as MODEL_LAB_CHECKPOINT
}
#[derive(Debug, Clone, Serialize)]
pub enum JobEvent {
    Started { pid: u32, started_at_ms: u64, limits: String },            // limits = Limits::describe()
    Progress { units_done: u64, units_total: Option<u64>, unit: String, stage: Option<String>, eta: Option<EtaRange>, latest: Option<Sample> },
    Output(String),
    Checkpoint { path: String, sha256: String },
    Completed { result: serde_json::Value, wall: Duration },
    Failed { error: String, limit: Option<LimitHit>, wall: Duration },
}
pub struct JobHandle { /* … */ }
impl JobHandle {
    pub fn events(&self) -> &Receiver<JobEvent>;
    pub fn cancel(&self);                       // non-blocking request; the job thread performs kill_tree(2 s) and emits Failed{error:"canceled"}
    pub fn is_finished(&self) -> bool;
}
pub struct Supervisor { /* … */ }
impl Supervisor {
    pub fn new() -> Self;
    pub fn start(&self, spec: JobSpec) -> std::io::Result<JobHandle>;
    pub fn shutdown(self);                      // cancels every running job and waits; also run by Drop
}
```

One thread per job: owns the `Child`, drains `lines()`, parses each stdout
line with `protocol::parse_line`, feeds `Eta`, merges the latest `Sample`,
enforces `wall`, and emits `JobEvent`s into a bounded channel (capacity
256). `Failed` carries the last 20 lines of stderr in `error` when the
worker died without a `Failed` event.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn progress_events_carry_eta_after_three_units() { /* fake-worker --emit 10 --unit-delay-ms 50: the fourth Progress has eta Some, the first has None */ }
#[test] fn spoofed_event_is_output_not_progress() { /* --spoof → exactly one Output containing "spoof", zero Progress */ }
#[test] fn wall_limit_fails_the_job_with_limit_wall() { /* --sleep 30, wall 1 s → Failed{limit: Some(Wall)} within 4 s */ }
#[test] fn cancel_emits_failed_canceled_and_nothing_after() { /* --sleep 30; cancel(); next event is Failed{error:"canceled"}; receiver then disconnects */ }
#[test] fn dropping_the_supervisor_kills_running_jobs() { /* start --grandchild --sleep 60, drop Supervisor, grandchild pid is gone */ }
#[test] fn stderr_tail_is_in_the_failure_message() { /* --exit 2 after writing 30 stderr lines → Failed.error contains line 30 and not line 1 */ }
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement `job.rs`.**
- [ ] **Step 4: Run** — 6 passed locally and on the Windows leg.
- [ ] **Step 5: Commit** — `git commit -am "Supervisor: job handles with progress, ETA, samples, limits and synchronous shutdown"`

---

### Task 11: Resumable stage-wise ensembles with per-tree RNG streams (versioned numerical change)

**Files:**
- Create: `crates/engine/src/ensemble.rs`, `docs/CHANGED-RESULTS.md`,
  `crates/engine/tests/fixtures/ensembles-v2.json`,
  `crates/engine/tests/ensembles.rs`
- Modify: `crates/engine/src/model.rs:219-335` (`fit` becomes a thin loop
  over the stepper), `crates/engine/src/experiment.rs:15` (`TRACE_SCHEMA`
  1 → 2), `src/workbench.rs` and `src/ui.rs` wherever a saved run with
  `TRACE_SCHEMA` 1 is opened (show the read-only notice; grep
  `TRACE_SCHEMA`)

**Interfaces:**
- Produces:

```rust
pub struct EnsembleState { /* data, cfg, train, valid, trees so far, running predictions, oob accumulators, early-stopping state */ }
impl EnsembleState {
    pub fn new(data: Data, cfg: Config) -> Result<Self, String>;        // same validation as model::fit
    pub fn stage(&self) -> usize;                                       // trees fitted so far
    pub fn finished(&self) -> bool;                                     // reached cfg.trees or early stopping
    pub fn step(&mut self) -> Result<(), String>;                       // fits exactly one more tree
    pub fn into_fit(self) -> Fit;
    pub fn to_json(&self) -> serde_json::Value;                         // complete state, resumable
    pub fn from_json(v: &serde_json::Value) -> Result<Self, String>;
}
```

RNG rule (the spec's numerical debt, required here because resume needs
it): tree `t` draws its bootstrap/subsample and its split-candidate
features from `StdRng::seed_from_u64(splitmix64(cfg.seed) ^ (t as u64 + 1))`;
the validation partition keeps `StdRng::seed_from_u64(cfg.seed)` so the
train/validation split is unchanged from 0.8. Consequence: single trees and
full-data boosting are bit-identical to 0.8; forests and stochastic
boosting change. `model::fit(data, cfg, cancel)` becomes
`EnsembleState::new` + `while !finished { step }` + `into_fit`, so every
existing caller and test keeps compiling.

Serialization: `to_json` must round-trip through `from_json` exactly
(`f64` via `serde_json` with `float_roundtrip`, already enabled). `Node`
gains `Serialize`/`Deserialize` derives.

- [ ] **Step 1: Write the failing tests** (`crates/engine/tests/ensembles.rs`)

```rust
#[test] fn stepping_to_the_end_equals_fit() { /* for each Method: fit(data,cfg) == step-loop + into_fit on trees, train_mse, valid_mse, importance (exact equality) */ }
#[test] fn resume_from_json_mid_way_is_bit_identical() { /* forest, 40 trees: step 17, to_json, from_json, step to end; compare with uninterrupted: identical stage_predictions */ }
#[test] fn tree_depth_change_does_not_change_bootstrap_samples() { /* forest depth 2 vs depth 5, same seed: samples[t] identical for every t */ }
#[test] fn single_tree_and_full_boosting_match_0_8_fixtures() { /* existing in-tree tests still pass; add: fixtures/ensembles-v1-stable.json cases (Tree, Boosting) reproduced exactly */ }
#[test] fn forest_matches_regenerated_fixture() { /* fixtures/ensembles-v2.json: forest + stochastic cases, 1e-12 tolerance */ }
```

Fixture generation: a `#[ignore]`d test `write_fixtures` that regenerates
`ensembles-v2.json` from named scenarios (`experiment::scenario(i, seed, noise)`
for i in 0..5, the four tree methods, two depths) and is run once by hand
in this task; the committed fixture is the independent record. The sklearn
depth-1/depth-2 comparison from the spec stays a later task (it needs the
pixi `scientific` environment).

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement `ensemble.rs`; rewrite `model::fit` as the loop; bump `TRACE_SCHEMA` to 2.**
- [ ] **Step 4: Write `docs/CHANGED-RESULTS.md`** with one entry: date, versions (engine 0.9.0, trace schema 2), what changed (per-tree RNG streams), which methods are affected (Forest, Stochastic) and which are not (Tree, Boosting, all linear), the fixture regenerated, and the read-only notice text shown for trace-schema-1 runs: "This experiment was saved by Model Lab 0.8. Random forests and stochastic boosting draw their samples differently since 0.9 (see CHANGED-RESULTS.md); it opens read-only and is not refitted."
- [ ] **Step 5: Run everything** — `cargo test --workspace --locked --features tree-ensemble-lab/acceptance` and `verify_linear_models.py` (unaffected but run anyway).
- [ ] **Step 6: Commit** — `git commit -am "Engine 0.9: resumable stage-wise ensembles with per-tree RNG streams (trace schema 2)"`

---

### Task 12: The `model-lab-engine-job` binary

**Files:**
- Create: `crates/engine/src/bin/engine-job.rs`, `crates/engine/src/job_spec.rs`
- Modify: `crates/engine/Cargo.toml` (`[[bin]] name = "model-lab-engine-job"`;
  the engine crate depends on `model-lab-supervisor` **only for the
  `protocol` and `checkpoint` modules**, so add the path dependency and use
  nothing from `process`/`job`), `crates/engine/src/data.rs:12-16` (a
  `parse_with_limit(name, text, max_rows)` that both existing parse
  functions call), `crates/engine/src/experiment.rs:323-340`
  (`Run::fit_with_limit(..., max_rows: usize)`; `Run::fit` calls it with 5000)

**Interfaces:**
- Produces: `model-lab-engine-job <spec.json>` where the spec is

```json
{"v":3,"job":"fit","label":"…","data_csv":"<path>","target":"<column>","features":["…"],
 "config":<RunConfig as experiment::Run::to_json writes it>,"max_rows":50000,
 "checkpoint_every_trees":10}
```

It reads `MODEL_LAB_NONCE` and `MODEL_LAB_CHECKPOINT` from the environment
(both required; exit 2 with a message otherwise), parses the CSV with
`max_rows`, builds `EnsembleState` (or resumes it when the checkpoint
exists, its `document.spec_sha256` equals the sha256 of the spec file bytes
and `document.data_sha256` equals `experiment::data_fingerprint`), emits a
`Progress{unit:"tree", units_total: Some(cfg.trees)}` after every step,
writes the checkpoint every `checkpoint_every_trees` steps and on
completion, emits `Checkpoint{path, sha256}` after each write, and ends with
`Completed{result: Run::to_json}` or `Failed{error}`. Linear models are
accepted (`job:"fit"` with a linear `config`) and run in one step with
`units_total: Some(1)`; they are not checkpointed. On SIGTERM (Unix) or
console close (Windows) it writes a final checkpoint if one is due and
exits 143.

```rust
pub struct JobSpecFile { pub v: u32, pub job: String, pub label: String, pub data_csv: PathBuf, pub target: String, pub features: Vec<String>, pub config: serde_json::Value, pub max_rows: usize, pub checkpoint_every_trees: usize }
pub fn load(path: &Path) -> Result<JobSpecFile, String>;  // validates v == 3, job == "fit", 1 ≤ checkpoint_every_trees ≤ 100, 4 ≤ max_rows ≤ 50_000
```

- [ ] **Step 1: Write the failing tests** (`crates/engine/tests/engine_job.rs`, running `env!("CARGO_BIN_EXE_model-lab-engine-job")` directly with a nonce in the environment):

```rust
#[test] fn missing_nonce_exits_two_with_message() {}
#[test] fn fifty_thousand_and_one_rows_is_a_clear_error_not_a_panic() { /* writes a 50_001-row CSV; Failed event names the limit; exit code 1 */ }
#[test] fn forest_job_emits_one_progress_per_tree_and_checkpoints_every_ten() { /* 30 trees → 30 Progress, 3 Checkpoint events, 1 Completed; Completed.result parses with Run::from_json */ }
#[test] fn resumed_job_completes_identically() { /* run with SIGTERM after the 2nd Checkpoint event (Unix; Windows: kill after the 2nd Checkpoint); rerun same spec → Completed.result equals an uninterrupted run's result exactly */ }
#[test] fn spec_or_data_mismatch_refuses_the_checkpoint() { /* change the seed in the spec → Failed mentions "checkpoint" and "spec" */ }
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement** `job_spec.rs`, `engine-job.rs`, `parse_with_limit`, `fit_with_limit`.
- [ ] **Step 4: Run** — 5 passed; `cargo build --release -p model-lab-engine --locked` then `strings target/release/model-lab-engine-job | grep -c /Users/` is 0 with the RUSTFLAGS remap used in `Build Mac.command`.
- [ ] **Step 5: Commit** — `git commit -am "Engine job binary: supervised fits with per-tree progress and resumable checkpoints"`

---

### Task 13: End to end — a 50,000-row boosting job under the supervisor, killed and resumed

**Files:**
- Create: `crates/supervisor/tests/end_to_end.rs`,
  `crates/supervisor/tests/data/generate_large.rs` (a helper module
  writing a deterministic 50,000-row × 8-predictor CSV with
  `rand::StdRng::seed_from_u64(20261108)` into the test's temp dir)
- Modify: `crates/supervisor/Cargo.toml` (`[dev-dependencies] model-lab-engine = { path = "../engine" }` so `CARGO_BIN_EXE_model-lab-engine-job` is available), `.github/workflows/engine.yml` (run this test on both legs; mark it `#[ignore]` only if a leg exceeds 10 minutes, and say so in the workflow)

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Write the failing test**

```rust
#[test] fn large_boosting_job_reports_progress_survives_kill_and_resumes() {
    // JobSpec: engine-job, boosting depth 3, 120 trees, rate 0.1, checkpoint_every_trees 10, wall 10 min, memory 2 GiB, sample 500 ms
    // 1. events include Started, ≥ 3 Progress with eta Some after the third tree, ≥ 1 Progress with latest Some(sample with rss > 0)
    // 2. after the 4th Checkpoint event: handle.cancel(); the next event is Failed{error:"canceled"}; no process from the job's tree remains
    // 3. start the same JobSpec again: first Progress has units_done ≥ 40; Completed arrives; wall of the second run < wall of a fresh uninterrupted run (measured in the same test)
    // 4. Completed.result of the resumed run == Completed.result of the uninterrupted run (exact JSON equality)
    // 5. every Progress has units_total Some(120) and unit "tree"; no Progress reports units_done decreasing
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p model-lab-supervisor --test end_to_end`.
- [ ] **Step 3: Fix whatever the test exposes** (expected: nothing new to build; this task is integration).
- [ ] **Step 4: Run on both CI legs**; record wall time of the uninterrupted run per platform in the commit message.
- [ ] **Step 5: Update the design document**: in "Left to the builder", add the two calls made in this plan (network denial recorded not enforced until plan 3; forest/stochastic RNG change shipped as engine 0.9 with trace schema 2). In "Phase 1 scope", mark "Every job goes through the supervisor" as implemented for native fits.
- [ ] **Step 6: Commit and push**

```bash
git add -A
git commit -m "End to end: a 50,000-row boosting job under the supervisor survives a kill and resumes identically"
git push
```

---

## Self-review (done at writing time)

- **Spec coverage.** Engine crate with no UI dependency: Tasks 1–2. Debts
  landing in the engine: per-tree RNG (Task 11); the others (reserved test
  split, one `Partition`, data-driven penalty grid, one median,
  quasi-separation, sklearn tree comparison) are *not* in this plan — they
  are separately versioned numerical changes that belong after the shell
  exists to show the notice, and the sklearn comparison needs the pixi
  environment; plan 3 carries them. Supervisor bullets: process group / Job
  Object (7), bounded readers (7), nonce-tagged v3 protocol (4), per-job
  limits (8; network denial explicitly deferred), checkpoints with resume
  semantics (6, 11, 12), resource sampling (9), ETA as a range after three
  units (5, 10). "Every job goes through the supervisor" for native fits
  above the interactive bound: Tasks 12–13; the egui app's in-process fits
  are unchanged, as the spec allows. Backends on protocol 3 (scientific,
  study, PyTorch, R, Julia): plan 3. Spec's "done" long-job sentence: Task 13.
- **Step scan.** Each test step carries names and assertions; each code
  step a signature and file. Bodies appear only for the fixture JSON shape,
  the checkpoint envelope and the RNG seeding rule, which the tests do not
  determine alone.
- **Type consistency.** `Limits`, `LimitHit`, `Sample`, `EtaRange`,
  `Envelope`/`Kind`, `Command`, `Child`, `JobSpec`, `JobEvent`,
  `EnsembleState`, `JobSpecFile` are named identically wherever used.
  `Run::fit_with_limit` is introduced in Task 12 and used only there.
- **Review Focus.** Five modes, each pinned: flood (7), malformed JSON with
  right nonce (4), truncated checkpoint (6), uneven units (5), SIGTERM trap
  (7). A sixth, the 50,001-row CSV, is pinned in Task 12.
- **Proportion.** Spec section implemented here is about 90 lines; the plan
  is about 870 lines (wrapped at 78 columns), most of it interfaces and test names rather than
  bodies.
