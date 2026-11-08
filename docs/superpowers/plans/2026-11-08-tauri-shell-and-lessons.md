# Tauri shell, lesson format and lessons 1–4 — Phase 1, plan 2 of 3

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the egui presentation layer with a Tauri application that
loads lessons from data, renders mathematics, runs the engine in-process for
interactive fits and under the supervisor for large ones, keeps a learner
record, and ships lessons 1–4 (one tree, boosting by hand, boosting on real
data, random forests).

**Architecture:** Three new crates under `crates/`. `model-lab-lessons`
(UI-free): lesson directory loading and validation, template substitution of
live numbers, Markdown with math placeholders to sanitized HTML, and
Vega-Lite plot specifications with their accessible tables. `model-lab-record`
(UI-free): the SQLite learner record and the mastery rule.
`model-lab-app`: the Tauri 2 backend holding all state (runs, jobs, record,
instructor flag) behind IPC commands, with a Vite + vanilla TypeScript front
end in `crates/app/ui` that renders what Rust sends (KaTeX for math,
vega-embed for plots) and keeps only view state (route, selected observation,
replay stage). The engine gains a read-only constructor for supervised
results and a `byhand` module for the four-point lessons. The egui
application is left as it is (it remains the research tool through Phase 1)
and is not extended.

**Tech Stack:** Rust 2021; `tauri =2.12.5`, `tauri-build =2.7.4`,
`tauri-plugin-dialog =2.8.4`; `rusqlite =0.41.1` (feature `bundled`);
`pulldown-cmark =0.13.6`; `ammonia =4.2.4`; `serde_yaml_ng =0.10.0`;
serde/serde_json/rand/sha2 at the root pins. Front end: Node 24 (fnm),
`@tauri-apps/cli 2.12.4`, `@tauri-apps/api 2.12.4`,
`@tauri-apps/plugin-dialog 2.8.4`, `katex 0.19.1`, `vega 6.4.0`,
`vega-lite 6.5.0`, `vega-embed 7.3.0`, `vite 8.3.6`, `typescript 7.0.2`,
`vitest 5.1.0`, all pinned exactly in `package.json` (no `^`).

**Spec:** `docs/superpowers/specs/2026-11-07-model-lab-platform-design.md`
(sections "4. Content and shell", "Phase 1 scope", "The first lesson set"
items 1–4, "Migration of kept assets", "Left to the builder: Reading back a
supervised result", "What done looks like for Phase 1").

**Scope note.** This is plan 2 of 3 for Phase 1. Plan 1 (merged 2026-11-08)
delivered the engine crate, the supervisor and `model-lab-engine-job`. Plan 3
covers the pixi environment layer, the scientific/study workers on protocol
3, the practice editor and lessons 5–10. This plan produces a working,
installable application on its own: lessons 1–4 with nothing downloaded, a
learner record, instructor packets, and 50,000-row boosting jobs under the
supervisor from inside the shell.

## Global Constraints

- Shipping targets: Mac (Apple silicon) and Windows x64. Every crate and the
  Tauri bundle build on both in CI.
- No data leaves the machine; no telemetry; the webview loads no remote
  resource (KaTeX, Vega and fonts are bundled by Vite; the Tauri CSP forbids
  remote origins).
- No spend without Dr. Helfrich's approval: ad-hoc signing stays; no
  Developer ID, no notarization, no hosted anything.
- The delivered 0.5–0.8 archives are not modified.
- Anything numerical that is kept keeps its tests and fixtures
  (`crates/engine/tests/fixtures/ensembles-v1-stable.json` is never
  regenerated; `verify_linear_models.py` still passes).
- Anti-goals: no fabricated trace or interpolated progress; an ETA only as a
  range with its observation count; a rate never without its denominator;
  no causal wording for a predictive fit; no harness or developer-machine
  path in a shipped binary (a dev-only sidecar lookup is allowed under
  `cfg!(debug_assertions)` only).
- Lesson files are untrusted input: Markdown is sanitized, templates are
  plain substitution with a format spec, experiment specs are validated
  before anything runs, and a lesson that fails validation is shown as an
  error naming the file and field, never partially run.
- Honest states only: every number in a lesson comes from a `Run`, a
  `byhand` result or the learner record; a missing template value renders as
  a visible `[missing: path]`, never as blank or zero.
- Accessibility: every plot has a data table; every equation keeps its TeX
  source in the DOM (KaTeX `htmlAndMathml` output); all controls are
  keyboard operable; OS light/dark and reduced motion are honoured; type is
  in `rem`.
- Dependencies pinned with `=` in Cargo and exact versions in npm.
- Commit after every task, plain messages in the repository's style, ending
  with the attribution line used in recent commits.

## Review Focus

Failure modes the spec implies but no task's tests would otherwise
exercise; each is pinned to a test in the owning task.

1. A lesson's `lesson.md` contains a `<script>` tag, an `onclick` attribute
   and a `javascript:` link. Expected: none survive sanitization; the rest
   of the lesson renders. → Task 5.
2. A template references a quantity the experiment does not produce
   (`{{coef.x9}}` on a tree lesson). Expected: the rendered text shows
   `[missing: coef.x9]` and the lesson's validation report lists it; the
   lesson still opens. → Task 4.
3. A learner quits the app while a supervised job is running and relaunches.
   Expected: the job appears as interrupted with its last checkpoint stage,
   Resume continues from that stage, and the result equals an uninterrupted
   run. → Task 10.
4. The learner record file is read-only or on a full disk. Expected: the
   lesson still works, the Check panel shows "your answers are not being
   saved" with the path, and nothing panics. → Task 7 and Task 11.
5. A learner changes the experiment settings after answering a question
   whose explanation used live numbers. Expected: the stored answer keeps
   the numbers it was answered against (experiment hash and rendered
   explanation at answer time), and the panel marks it as from an earlier
   setting. → Task 11.

---

## File structure

```
Cargo.toml                                workspace gains crates/lessons, crates/record, crates/app
crates/engine/src/experiment.rs           Run::to_json gains "ensemble"; Run::from_trace (Task 1)
crates/engine/src/byhand.rs               candidate splits and boosting rounds on all rows (Task 2)
crates/lessons/Cargo.toml                 model-lab-lessons (lib)
crates/lessons/src/lib.rs                 pub mod lesson, template, markdown, plots, packet
crates/lessons/src/lesson.rs              Lesson, Experiment, Question, load_dir, validate (Task 3)
crates/lessons/src/template.rs            Context, render (Task 4)
crates/lessons/src/markdown.rs            to_html with math placeholders and sanitization (Task 5)
crates/lessons/src/plots.rs               Plot { spec, table }, theme, the six plot builders (Task 6)
crates/lessons/src/packet.rs              student/instructor packet writer (Task 12)
crates/lessons/src/examples.rs            csv(name) for the three synthetic tables (include_str from ../../examples) (Task 9)
crates/lessons/tests/fixtures/            minimal lesson directories for tests
crates/record/Cargo.toml                  model-lab-record (lib), rusqlite bundled
crates/record/src/lib.rs                  Record, schema, mastery, export (Task 7)
crates/app/Cargo.toml                     model-lab-app, binary "model-lab"
crates/app/build.rs                       tauri_build::build()
crates/app/tauri.conf.json                identifier, CSP, bundle (resources: lessons/, externalBin)
crates/app/capabilities/default.json      core + dialog permissions
crates/app/src/main.rs                    tauri::Builder, state, command registration
crates/app/src/state.rs                   AppState (Task 8)
crates/app/src/commands/{lessons,experiment,jobs,record,packet}.rs
crates/app/src/experiment.rs              in-process runner thread with cancel (Task 9)
crates/app/src/jobs.rs                    job directory, spec writing, supervisor bridge, resume (Task 10)
crates/app/binaries/                      sidecar copies made by packaging/sidecar.py (git-ignored)
crates/app/ui/package.json, vite.config.ts, tsconfig.json, index.html
crates/app/ui/src/main.ts                 router: curriculum → lesson → Read | Run | Check
crates/app/ui/src/{api,math,plots,selection,theme}.ts
crates/app/ui/src/panels/{read,run,check,jobs}.ts
crates/app/ui/src/styles.css              tokens, light/dark, reduced motion
crates/app/ui/src/*.test.ts               vitest for math.ts, selection.ts, format helpers
lessons/01-one-tree/{lesson.md,experiment.json,questions.json,derivation.md}
lessons/02-boosting-by-hand/…  lessons/03-boosting-on-data/…  lessons/04-random-forests/…
packaging/sidecar.py                      build engine-job in release, copy with target triple (Task 13)
.github/workflows/shell.yml               macOS + Windows: cargo test, npm test, tauri build (Task 13)
Build Mac.command / Build Windows.cmd     add the Tauri build (Task 13)
docs/ARCHITECTURE.md                      the four layers as built (Task 13)
```

Runtime directories (Tauri `app_data_dir`, i.e. `~/Library/Application
Support/org.ianhelfrich.modellab` on Mac, `%APPDATA%\org.ianhelfrich.modellab`
on Windows): `lessons/` (user lessons, read after the bundled ones),
`record/<profile>.sqlite`, `jobs/<job_id>/`, `packets/`.

---

### Task 1: Read-only `Run` from a saved trace

**Files:**
- Modify: `crates/engine/src/experiment.rs` (`to_json`, new `from_trace`)
- Modify: `crates/engine/src/bin/engine-job.rs` (no code change expected;
  `Completed.result` already is `Run::to_json`, which now carries the key)
- Test: `crates/engine/tests/trace_readback.rs`

**Interfaces:**
- Consumes: `ensemble::EnsembleState::{to_json, from_json(value, data), into_fit}`,
  `Run::from_fitted`, `job_spec::MAX_ROWS`.
- Produces: `Run::to_json` for `FittedModel::Trees` includes
  `"ensemble": EnsembleState::to_json` (the trees and per-stage losses;
  `TRACE_SCHEMA` stays 2 because the document is a superset).
  `pub fn Run::from_trace(value: &Value, max_rows: usize) -> Result<Run, String>`:
  rebuilds a tree-model `Run` by replaying the stored trees through
  `EnsembleState::from_json` (prediction replay only, no split search) and
  `Run::from_fitted`; checks `fingerprint`, `partition`, `final_predictions`
  and the `trace` losses against the replay exactly as `from_json` does;
  rejects rows above `max_rows`, a missing `ensemble` key ("This saved
  experiment has no ensemble state; open it with the interactive loader."),
  and linear models ("Linear experiments are reloaded by refitting."). It
  takes no cancel flag: replay of 50,000 rows × 300 trees is bounded work.

- [ ] **Step 1: Write the failing tests** in `crates/engine/tests/trace_readback.rs`:

```rust
fn big_boosting_run() -> Run  // 6000 rows, 3 predictors, seed 7, Boosting, 20 trees, depth 2, via Run::fit_with_limit(..., 50_000)

#[test] fn to_json_carries_the_ensemble_state()            // value["ensemble"]["v"] == 2, ["trees"].len() == 20
#[test] fn from_trace_rebuilds_without_refitting()         // from_trace(&json, 50_000): predictions(20) == original, trace == original, name/config equal
#[test] fn from_json_still_rejects_above_interactive_rows()// Run::from_json(&json, cancel) is Err containing "teaching-workspace limits"
#[test] fn from_trace_honours_the_row_bound()              // from_trace(&json, 5000) is Err mentioning 5000
#[test] fn from_trace_rejects_tampered_losses()            // set json["trace"][5]["train_loss"] += 1.0 → Err containing "does not reproduce"
#[test] fn from_trace_rejects_linear_and_missing_state()   // OLS run's json → Err "refitting"; tree json with "ensemble" removed → Err "no ensemble state"
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p model-lab-engine --test trace_readback`
Expected: compile error, `from_trace` not found.

- [ ] **Step 3: Implement.** Add
`pub fn ensemble::state_document(fit: &Fit) -> Value`, producing the same
JSON `EnsembleState::to_json` does from `Fit { data, cfg, trees, train_mse, valid_mse }`
with `"stopped": fit.trees.len() < fit.cfg.trees`; `Run::to_json` emits it
under `"ensemble"` for `FittedModel::Trees` only. Then `Run::from_trace` as
specified: `EnsembleState::from_json(&value["ensemble"], data)?.into_fit()`,
`Run::from_fitted` with a never-set cancel flag, then the same checks
`from_json` makes.

- [ ] **Step 4: Run the engine suite**

Run: `cargo test -p model-lab-engine --locked`
Expected: all previous tests pass (the `ensembles-v2.json` fixture test
compares `EnsembleState::to_json`, which is unchanged) plus the six new ones.
The end-to-end test `crates/engine/tests/end_to_end.rs` still passes (fresh
and resumed JSON remain identical because both carry the same trees).

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "Engine: saved traces carry the ensemble state and open read-only with Run::from_trace"
```

---

### Task 2: `byhand`: candidate splits and boosting rounds on every row

**Files:**
- Create: `crates/engine/src/byhand.rs`
- Modify: `crates/engine/src/lib.rs` (`pub mod byhand;`)
- Test: unit tests in `byhand.rs`

**Interfaces:**
- Consumes: `model::{build_tree, Node}`, `data::Data`.
- Produces:

```rust
pub struct Candidate { pub threshold: f64, pub left: Vec<usize>, pub right: Vec<usize>,
                       pub left_mean: f64, pub right_mean: f64, pub sse_after: f64, pub reduction: f64 }
/// Every admissible split of `feature` over `rows` (midpoints between distinct sorted values), in threshold order.
pub fn candidate_splits(data: &Data, rows: &[usize], feature: usize) -> Vec<Candidate>;
pub struct Round { pub stage: usize, pub tree: Node, pub before: Vec<f64>, pub correction: Vec<f64>,
                   pub after: Vec<f64>, pub residual_before: Vec<f64>, pub residual_after: Vec<f64>,
                   pub mse_before: f64, pub mse_after: f64 }
/// Boosting with stumps of `depth` on ALL rows (no validation split), min_leaf 1, every feature
/// considered, `rounds` ≤ 50, rate in (0, 1]. Round 0's `before` is the mean of y.
pub fn boosting_rounds(data: &Data, rate: f64, depth: usize, rounds: usize) -> Result<Vec<Round>, String>;
```

  `after = before + rate × tree.predict(x)`; `correction = rate × tree.predict(x)`.
  Deterministic: `build_tree` is called with `mtry = p` so the RNG is never
  consulted (pass `StdRng::seed_from_u64(0)`).

- [ ] **Step 1: Write the failing tests** (four-point fixture `x = 1,2,3,4`, `y = 2,4,8,10`):

```rust
#[test] fn four_point_candidates_match_hand_arithmetic()
// thresholds [1.5, 2.5, 3.5]; reductions [21.333…, 36.0, 21.333…] within 1e-9; the 2.5 candidate has left_mean 3, right_mean 9, sse_after 4
#[test] fn four_point_boosting_matches_the_taught_route()
// rate 0.5, depth 1, 2 rounds: round0 before all 6, residual_before [-4,-2,2,4], after [4.5,4.5,7.5,7.5], mse_before 10, mse_after 3.25;
// round1 after [3.75,3.75,8.25,8.25], mse_after 1.5625
#[test] fn boosting_rounds_rejects_bad_settings()  // rate 0 → Err, rate 1.5 → Err, rounds 51 → Err, depth 0 → Err
#[test] fn correction_is_rate_times_tree_prediction() // for every round and row: after - before == correction and correction == rate * tree.predict(x) exactly
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-engine byhand`
Expected: compile error, module missing.

- [ ] **Step 3: Implement `byhand.rs`** as specified.

- [ ] **Step 4: Run**

Run: `cargo test -p model-lab-engine --locked`
Expected: PASS, four new tests.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/byhand.rs crates/engine/src/lib.rs
git commit -m "Engine: byhand module for candidate splits and all-row boosting rounds"
```

---

### Task 3: Lesson directory format, loading and validation

**Files:**
- Create: `crates/lessons/Cargo.toml`, `crates/lessons/src/lib.rs`,
  `crates/lessons/src/lesson.rs`
- Create: `crates/lessons/tests/fixtures/valid-lesson/{lesson.md,experiment.json,questions.json,derivation.md}`,
  `…/bad-experiment/`, `…/bad-question/`, `…/no-front-matter/`
- Modify: root `Cargo.toml` workspace members
- Test: `crates/lessons/tests/loading.rs`

**Interfaces:**
- Consumes: `model_lab_engine::experiment::{RunConfig, ModelId}`.
- Produces:

```rust
pub struct FrontMatter { pub id: String, pub title: String, pub level: Level /* Beginner|Intermediate|Advanced */,
                         pub prerequisites: Vec<String>, pub estimated_minutes: u32, pub concepts: Vec<String> }
pub enum Source { Example { name: String }, FourPoint, Csv { path: String } }   // examples are the three synthetic tables by their examples.rs names
pub enum Kind { Run, ByHand }
pub struct Learner { pub field: String, pub min: f64, pub max: f64, pub step: f64 } // what the learner may change
pub struct Experiment { pub kind: Kind, pub source: Source, pub target: String, pub features: Vec<String>,
                        pub config: RunConfig /* Run */, pub byhand: Option<ByHandSettings> /* rate, depth, rounds */,
                        pub learner_may_change: Vec<Learner>, pub checkpoint_every_trees: usize /* default 10 */ }
pub struct Question { pub id: String, pub concept: String, pub stem: String, pub options: Vec<String>,
                      pub correct: Vec<usize>, pub explanation: String, pub live: bool, pub requires_prediction: Option<String> /* prompt id */ }
pub struct Prediction { pub id: String, pub prompt: String, pub kind: PredictionKind /* Direction | Number { tolerance } | Choice { options } */, pub reveals: String /* template path */ }
pub struct Lesson { pub dir: PathBuf, pub front: FrontMatter, pub body: String /* lesson.md after front matter */,
                    pub derivation: Vec<String> /* derivation.md split on "\n---\n" */, pub experiment: Experiment,
                    pub predictions: Vec<Prediction>, pub questions: Vec<Question>, pub instructor_notes: Option<String> /* instructor.md */ }
pub struct LessonError { pub file: PathBuf, pub field: String, pub message: String }
pub fn load_dir(dir: &Path) -> Result<Lesson, LessonError>;
pub fn load_all(roots: &[PathBuf]) -> (Vec<Lesson>, Vec<LessonError>);  // bundled first, then user; later ids override earlier
```

  `experiment.json` is the JSON form of `Experiment` with `config` in the
  `RunConfig::to_json` layout (so a lesson's config is also a job config).
  Validation: `features` non-empty and distinct; `target` not in `features`;
  every `learner_may_change.field` is a `RunConfig` key, `min < max`,
  `step > 0`; `kind == ByHand` requires `source == FourPoint` and `byhand`;
  question `correct` indices in range, `options.len() ∈ 2..=6`,
  `explanation.len() ≥ 80` characters; `requires_prediction` names a
  prediction id; ids unique. `questions.json` is `{ "predictions": [...],
  "questions": [...] }`.

- [ ] **Step 1: Write the failing tests** in `crates/lessons/tests/loading.rs`:

```rust
#[test] fn valid_lesson_loads_with_every_part()        // id, 2 derivation steps, experiment.kind Run, 1 prediction, 2 questions, instructor_notes None
#[test] fn experiment_with_unknown_learner_field_is_rejected()  // bad-experiment: LessonError.file ends with experiment.json, field == "learner_may_change[0].field"
#[test] fn question_with_short_explanation_is_rejected()        // bad-question: field == "questions[0].explanation"
#[test] fn lesson_without_front_matter_is_rejected()            // no-front-matter: field == "front_matter"
#[test] fn user_lessons_override_bundled_by_id()                // load_all([bundled, user]) with the same id: title from user; errors empty
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-lessons --test loading`
Expected: compile error, no such package.

- [ ] **Step 3: Create the crate and implement `lesson.rs`.** Front matter is
the leading `---\n…\n---\n` block parsed with `serde_yaml_ng`; the engine's
`RunConfig::from_json` parses `config`.

- [ ] **Step 4: Run**

Run: `cargo test -p model-lab-lessons --locked`
Expected: PASS, 5 tests.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/lessons
git commit -m "Lessons crate: directory format, loading and validation"
```

---

### Task 4: Template substitution of live quantities

**Files:**
- Create: `crates/lessons/src/template.rs`
- Test: unit tests in `template.rs`

**Interfaces:**
- Produces:

```rust
pub struct Context(pub serde_json::Value);   // nested object; paths are dotted, indices as `.1`
pub struct Rendered { pub text: String, pub missing: Vec<String> }
/// `{{path}}` → shortest round-trip float / integer / string; `{{path:.3f}}` → fixed decimals (0..=10);
/// `{{path:,}}` → integer with thousands separators. Unknown path → "[missing: path]" and listed once.
/// Anything else inside braces (spaces, `#`, `if`, parentheses) is left verbatim and listed in `missing` as "invalid: …".
pub fn render(template: &str, ctx: &Context) -> Rendered;
```

  The context the shell builds for a `Run` (Task 9 documents it; this task
  fixes the names): `data.n`, `data.target`, `rows.train`, `rows.valid`,
  `config.<RunConfig key>`, `stage` (current replay stage),
  `stage.best`, `metric.train_mse`, `metric.valid_mse`,
  `metric.train_rmse`, `metric.valid_rmse` (at the current stage),
  `metric.best_valid_mse`, `coef.intercept`, `coef.<feature>` (linear only),
  `oob.mse`, `oob.coverage` (forest), `importance.<feature>.mean`,
  `importance.<feature>.sd`, `selected.row`, `selected.y`,
  `selected.before`, `selected.h`, `selected.after`, `selected.x.<feature>`.
  For `ByHand`: `byhand.mean`, `byhand.sse`, `byhand.round.<k>.mse_before`,
  `byhand.round.<k>.mse_after`, `byhand.round.<k>.threshold`,
  `byhand.round.<k>.left_mean`, `byhand.round.<k>.right_mean`,
  `byhand.candidates.<j>.threshold`, `byhand.candidates.<j>.reduction`,
  `byhand.candidates.<j>.admissible`. Lessons 3 and 4 add three more keys
  (Tasks 16 and 17), each with its own unit test.

- [ ] **Step 1: Write the failing tests**:

```rust
#[test] fn substitutes_nested_paths_and_formats()      // ctx {"metric":{"valid_mse":3.25},"rows":{"train":2250}} ; "{{metric.valid_mse:.3f}} on {{rows.train:,}} rows" == "3.250 on 2,250 rows"
#[test] fn missing_path_is_visible_and_reported()      // "{{coef.x9}}" → "[missing: coef.x9]", missing == ["coef.x9"]
#[test] fn expressions_are_not_evaluated()             // "{{#if a}}x{{/if}}" is returned verbatim, missing contains "invalid: #if a"
#[test] fn array_indices_and_integers()                // {"byhand":{"round":[{"mse_after":3.25}]}} ; "{{byhand.round.0.mse_after}}" == "3.25"; {"n":4} "{{n}}" == "4"
#[test] fn bad_format_spec_is_invalid()                // "{{x:.12f}}" → verbatim + "invalid: x:.12f"
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-lessons template`
Expected: compile error.

- [ ] **Step 3: Implement `template.rs`** with a hand-written scanner (no
regex crate).

- [ ] **Step 4: Run**

Run: `cargo test -p model-lab-lessons --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/lessons/src
git commit -m "Lessons: template substitution with visible missing values"
```

---

### Task 5: Markdown with mathematics to sanitized HTML

**Files:**
- Create: `crates/lessons/src/markdown.rs`
- Test: unit tests in `markdown.rs`

**Interfaces:**
- Produces: `pub fn to_html(markdown: &str) -> String`. Pipeline: extract
  `$$…$$` (display) and `$…$` (inline; not inside code spans or fenced code)
  into placeholders → `pulldown-cmark` with tables, footnotes and strikethrough
  → `ammonia` with the default allow-list plus `class` on `span`/`div` and
  `data-tex` → placeholders replaced by
  `<span class="math" data-tex="…">TeX</span>` /
  `<div class="math math-display" data-tex="…">TeX</div>` where TeX is
  HTML-escaped. The front end renders `[data-tex]` with KaTeX
  (`throwOnError: false`, `output: "htmlAndMathml"`); the element's text
  content before rendering is the TeX source, which is the text alternative
  if KaTeX fails.

- [ ] **Step 1: Write the failing tests**:

```rust
#[test] fn math_survives_markdown_and_sanitizer()   // "Loss $\\frac{1}{n}\\sum e_i^2$ here" → contains `data-tex="\frac{1}{n}\sum e_i^2"` and class "math"
#[test] fn display_math_becomes_a_block()           // "$$ y = f(x) $$" → `<div class="math math-display"`
#[test] fn scripts_handlers_and_javascript_links_are_removed()  // Review Focus 1: "<script>x</script>", "<a onclick=…>", "[a](javascript:alert(1))" → none of "script", "onclick", "javascript:" in output; a following paragraph is present
#[test] fn dollar_inside_code_is_not_math()         // "`$5`" and a fenced block containing "$x$" → no data-tex
#[test] fn tables_render()                          // a pipe table → "<table>"
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-lessons markdown`
Expected: compile error.

- [ ] **Step 3: Implement `markdown.rs`.**

- [ ] **Step 4: Run**

Run: `cargo test -p model-lab-lessons --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/lessons
git commit -m "Lessons: Markdown with math placeholders to sanitized HTML"
```

---

### Task 6: Plots as Vega-Lite specifications with accessible tables

**Files:**
- Create: `crates/lessons/src/plots.rs`
- Test: unit tests in `plots.rs`

**Interfaces:**
- Consumes: `model_lab_engine::experiment::Run`, `model_lab_engine::model::Fit`,
  `model_lab_engine::science::Analysis`, `model_lab_engine::byhand::Round`.
- Produces:

```rust
pub struct Table { pub columns: Vec<String>, pub rows: Vec<Vec<serde_json::Value>> }
pub struct Plot { pub id: String, pub title: String, pub description: String /* one sentence, what the eye should find */,
                  pub spec: serde_json::Value /* Vega-Lite v6 with "config": theme() and "datasets" */, pub table: Table }
pub fn theme() -> serde_json::Value;   // one palette: train #1f77b4-like replaced by the five tokens below; font "inherit"; "axis": {"tickCount": 6}
pub fn loss_curve(run: &Run) -> Plot;                       // stage vs train_loss & valid_loss, rule at stage.best, legend "training (n=…)" / "validation (n=…)"
pub fn scatter_with_slice(run: &Run, feature: usize, stage: usize, selected: Option<usize>) -> Plot; // points train/valid shape-coded, slice line from Run::slice, selected row ringed
pub fn byhand_rounds(rounds: &[Round], upto: usize) -> Plot; // four points, before/after predictions, residual segments
pub fn oob_curve(fit: &Fit) -> Plot;                        // oob_mse vs stage with oob_coverage as a second row; coverage never without the denominator (rows)
pub fn importance(analysis: &Analysis) -> Plot;             // bars with ±sd error bars, title names the number of repeats (3) and validation rows used
pub fn pdp_ice(analysis: &Analysis, slice: &[(f64, f64)]) -> Plot; // ICE lines, PDP mean, the slice as a dashed line, legend distinguishes the three
```

  Palette tokens (used in `theme()` and in `styles.css`): `--c-train`
  `#2A6F97`, `--c-valid` `#C8553D`, `--c-oob` `#5B8E7D`, `--c-slice`
  `#7B6D8D`, `--c-highlight` `#F2A541`; each pair has ≥ 3:1 contrast on
  white and on `#121417`. Every `Plot.table` has the same rows the spec plots.

- [ ] **Step 1: Write the failing tests** (a 60-row seeded `Run` from
`experiment::scenario(0, 1, 0.5)`, Boosting, 15 trees):

```rust
#[test] fn every_plot_has_a_table_matching_its_data()   // for each builder: table.rows.len() == length of spec["datasets"]["main"] (or sum over datasets)
#[test] fn loss_curve_marks_the_best_stage_and_denominators() // spec contains a "rule" layer at run best stage; legend labels contain "n=45" and "n=15"
#[test] fn specs_use_one_theme()                          // every spec["config"] == theme()
#[test] fn oob_table_carries_coverage_with_rows()         // oob_curve table columns == ["stage","oob_mse","oob_coverage","rows_covered","rows_train"]
#[test] fn scatter_rings_the_selected_row()               // selected Some(7): a layer with "stroke" highlight and exactly one datum with row == 7
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-lessons plots`
Expected: compile error.

- [ ] **Step 3: Implement `plots.rs`.** Specs carry data inline under
`datasets` and reference by name; no URLs.

- [ ] **Step 4: Run**

Run: `cargo test -p model-lab-lessons --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/lessons
git commit -m "Lessons: Vega-Lite plot builders with one theme and a table behind every plot"
```

---

### Task 7: The learner record

**Files:**
- Create: `crates/record/Cargo.toml`, `crates/record/src/lib.rs`
- Modify: root `Cargo.toml` members
- Test: `crates/record/tests/record.rs`

**Interfaces:**
- Produces:

```rust
pub struct Record { /* rusqlite::Connection, path */ }
pub enum RecordError { Io(String), Sql(String) }
impl Record {
    pub fn open(path: &Path) -> Result<Record, RecordError>;   // creates schema v1; WAL; busy_timeout 2 s
    pub fn open_in_memory() -> Result<Record, RecordError>;
    pub fn start_session(&self) -> Result<i64, RecordError>;   // sessions(id, started_at)
    pub fn attempt(&self, session: i64, lesson: &str, experiment_hash: &str) -> Result<i64, RecordError>;
    pub fn prediction(&self, attempt: i64, prompt_id: &str, predicted: &str, actual: &str, correct: bool) -> Result<(), RecordError>;
    pub fn answer(&self, attempt: i64, q: &str, concept: &str, chosen: &[usize], correct: bool,
                  after_prediction: bool, rendered_explanation: &str) -> Result<(), RecordError>;
    pub fn lesson_history(&self, lesson: &str) -> Result<LessonHistory, RecordError>;  // attempts with their predictions and answers, newest first
    pub fn mastery(&self) -> Result<Vec<Mastery>, RecordError>;  // per concept: mastered bool, evidence (lesson, session) pairs
    pub fn export_json(&self) -> Result<serde_json::Value, RecordError>;  // everything, no file paths
}
pub struct Mastery { pub concept: String, pub mastered: bool, pub evidence: Vec<(String /*lesson*/, i64 /*session*/)> }
```

  Mastery rule (spec): a concept is mastered when it has correct answers,
  each given after a committed prediction in the same attempt
  (`after_prediction`), in at least two distinct (lesson, session) pairs that
  differ in lesson OR session. Schema v1 tables: `meta(key, value)`,
  `sessions`, `attempts(id, session_id, lesson_id, experiment_hash, started_at)`,
  `predictions(attempt_id, prompt_id, predicted, actual, correct, at)`,
  `answers(attempt_id, question_id, concept, chosen_json, correct, after_prediction, rendered_explanation, at)`.
  Timestamps are Unix milliseconds from `std::time`.

- [ ] **Step 1: Write the failing tests**:

```rust
#[test] fn schema_is_created_and_versioned()            // open(tmp) twice; meta schema_version == "1"
#[test] fn mastery_needs_two_distinct_lesson_or_session_pairs()
// same lesson, same session, two correct after-prediction answers → not mastered; second answer in a new session → mastered; evidence has 2 pairs
#[test] fn answers_without_a_prediction_do_not_count()  // after_prediction false twice in two sessions → not mastered
#[test] fn history_keeps_the_rendered_explanation()     // Review Focus 5 (record half): the stored string equals what was passed, with its experiment_hash
#[test] fn export_has_no_paths()                        // export_json string form contains no "/" path of the db
#[test] fn read_only_location_is_an_error_not_a_panic() // Review Focus 4: open on a path inside a read-only dir (tmp dir chmod 0o500 on Unix; skip on Windows) → Err(RecordError::Io)
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-record`
Expected: compile error, no such package.

- [ ] **Step 3: Implement.** `rusqlite = { version = "=0.41.1", features = ["bundled"] }`.

- [ ] **Step 4: Run**

Run: `cargo test -p model-lab-record --locked`
Expected: PASS, 6 tests.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/record
git commit -m "Record crate: SQLite learner record with the two-context mastery rule"
```

---

### Task 8: Tauri application scaffold, lesson commands, Read panel

**Files:**
- Create: `crates/app/Cargo.toml`, `build.rs`, `tauri.conf.json`,
  `capabilities/default.json`, `src/main.rs`, `src/state.rs`,
  `src/commands/lessons.rs`, `src/commands/mod.rs`
- Create: `crates/app/ui/{package.json,package-lock.json,vite.config.ts,tsconfig.json,index.html}`,
  `ui/src/{main.ts,api.ts,math.ts,theme.ts,styles.css}`, `ui/src/panels/read.ts`,
  `ui/src/math.test.ts`
- Modify: root `Cargo.toml` members; `.gitignore` (`crates/app/ui/node_modules/`,
  `crates/app/ui/dist/`, `crates/app/binaries/`)
- Test: `crates/app/src/commands/lessons.rs` unit tests; `vitest`

**Interfaces:**
- Consumes: `model_lab_lessons::{lesson::*, template::*, markdown::to_html}`.
- Produces: `tauri.conf.json`: `identifier "org.ianhelfrich.modellab"`,
  `productName "Model Lab"`, `version 0.9.0`, `app.security.csp`
  `"default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:"`,
  `build.frontendDist "ui/dist"`, `build.devUrl "http://localhost:1420"`,
  `build.beforeDevCommand "npm --prefix ui run dev"`,
  `build.beforeBuildCommand "npm --prefix ui run build"`,
  `bundle.resources ["../../lessons/**"]`, `bundle.externalBin` left for
  Task 13. `AppState { lessons: RwLock<Vec<Lesson>>, lesson_errors: RwLock<Vec<LessonError>>,
  runs: Mutex<HashMap<String, Arc<RunView>>>, jobs: Mutex<Jobs>, record: Mutex<Option<Record>>,
  record_error: Mutex<Option<String>>, instructor: AtomicBool, dirs: Dirs { bundled_lessons, user_lessons, record, jobs, packets } }`.
  Commands (all `Result<T, String>`): `list_lessons() -> Vec<LessonCard { id, title, level, estimated_minutes, prerequisites, concepts, prerequisite_titles }>`;
  `lesson_errors() -> Vec<LessonError>`; `read_panel(id) -> ReadPanel { html, derivation_html: Vec<String>, missing: Vec<String> }`
  rendered against an empty context when no run exists (so `[missing: …]`
  markers show until the learner runs the experiment) or against the
  current run's context; `set_instructor(on: bool)`; `instructor() -> bool`.
  Front end: route hash `#/` (curriculum), `#/lesson/<id>/read|run|check`,
  `#/jobs`; `math.ts` exports `renderMath(root: HTMLElement)` calling KaTeX
  on `[data-tex]`; `theme.ts` applies `prefers-color-scheme` and
  `prefers-reduced-motion` via `data-theme` / `data-motion` on `<html>`.

- [ ] **Step 1: Write the failing Rust tests** in `commands/lessons.rs` (the
inner functions take `&AppState`, not `tauri::State`):

```rust
#[test] fn read_panel_renders_html_with_math_and_missing_markers()  // fixture lesson from crates/lessons/tests/fixtures/valid-lesson; html contains data-tex; missing lists the template paths
#[test] fn curriculum_lists_prerequisite_titles_without_gating()     // two fixture lessons, B requires A: card B.prerequisite_titles == ["A title"]; both listed
```

and the failing front-end test `ui/src/math.test.ts`:

```ts
test("renderMath renders every data-tex element and keeps the source", …) // jsdom; two spans; after renderMath each has .katex and getAttribute("data-tex") unchanged
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-app lessons` and `npm --prefix crates/app/ui test`
Expected: compile error / "no such package"; npm: no package.json.

- [ ] **Step 3: Scaffold.** `npm --prefix crates/app/ui init -y`, install the
pinned packages with `npm --prefix crates/app/ui install --save-exact …`,
write `vite.config.ts` (port 1420, `clearScreen: false`), `tsconfig.json`
(`strict: true`), `index.html` with `<main id="app">`. Rust: `tauri::Builder`
with `tauri_plugin_dialog`, `manage(AppState)`, `invoke_handler`. Lessons are
loaded at setup from `resource_dir()/lessons` then `app_data_dir()/lessons`.

- [ ] **Step 4: Run the tests and the dev app**

Run: `cargo test -p model-lab-app --locked` and `npm --prefix crates/app/ui test -- --run`
Expected: PASS.
Run: `npm --prefix crates/app/ui run tauri dev` (stop it after the window appears)
Expected: a window titled "Model Lab" showing the curriculum with the fixture
lesson absent (bundled `lessons/` is empty until Task 14) and the message
"No lessons found in <bundled path> or <user path>." Switching the OS
appearance switches the theme without restart.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore crates/app
git commit -m "Tauri shell: application state, lesson commands, Read panel and theme"
```

---

### Task 9: Run panel: in-process experiments, replay, observation arithmetic

**Files:**
- Create: `crates/app/src/experiment.rs`, `crates/app/src/commands/experiment.rs`,
  `ui/src/panels/run.ts`, `ui/src/plots.ts`, `ui/src/selection.ts`, `ui/src/selection.test.ts`
- Test: unit tests in `crates/app/src/experiment.rs` and `commands/experiment.rs`

**Interfaces:**
- Consumes: `Run::{fit_with_limit, predictions, slice, scores, coefficients}`,
  `model::Fit::best_stage`, `science::analyze`, `byhand::{candidate_splits, boosting_rounds}`,
  `plots::*`, `template::{Context, render}`, `data::Table`, `examples` data
  (the three CSVs move from `src/examples.rs` `include_str!` paths to
  `model_lab_lessons::examples::csv(name) -> Option<&'static str>`, the egui
  app keeps its own copy untouched).
- Produces:

```rust
pub enum RunView { Fitted { run: Run, fit_analysis: Option<Analysis> /* forests, feature 0 */ }, ByHand { data: Data, candidates: Vec<Candidate>, rounds: Vec<Round> } }
pub fn context(view: &RunView, stage: usize, selected: Option<usize>) -> Context;   // the names fixed in Task 4
pub fn experiment_hash(lesson_id: &str, config: &RunConfig, data_fingerprint: &str) -> String;  // sha256 of the three, hex
```

  Commands: `start_experiment(lesson_id, overrides: serde_json::Value) -> String /* run_id */`
  (overrides are `RunConfig::to_json` keys restricted to the lesson's
  `learner_may_change` fields and clamped to their ranges; the fit runs on a
  `std::thread` with a cancel flag; rows above `Run::INTERACTIVE_ROWS` are
  refused here with the message "Use a supervised job for more than 5,000
  rows" and the Jobs route offered); `experiment_status(run_id) -> Status { Running | Done | Failed(String) | Canceled }`;
  `cancel_experiment(run_id)`; `run_panel(run_id, stage, selected) -> RunPanel { stages: usize, best_stage: Option<usize>, plots: Vec<Plot>, observation: Option<Observation>, rendered_text: Rendered, controls: Vec<Learner>, config: Value }`
  where `Observation { row, x: Vec<(String,f64)>, y, before, h, rate, after, residual_before, residual_after, rules: Vec<String> /* Node::rules path */, in_training: bool }`
  so the panel can show `old + rate × h = new` exactly as `ui.rs:1005` does.
  Front end: `selection.ts` is a tiny store `{ stage, selected }` with
  subscribers; `plots.ts` wraps `vegaEmbed` with `actions: { export: true, source: false, editor: false }`,
  a "Show data table" toggle rendering `Plot.table` as `<table>` with a
  caption, and click-to-select on scatter points (Vega signal
  `select_row`) feeding the store; the Run panel re-requests `run_panel`
  on every store change (Rust renders; the webview displays).

- [ ] **Step 1: Write the failing tests**:

```rust
#[test] fn context_for_a_fitted_run_has_the_fixed_names()  // keys: data.n, rows.train, rows.valid, stage.best, metric.valid_mse, selected.after present when selected Some(0)
#[test] fn context_for_byhand_has_rounds_and_candidates()   // byhand.round.1.mse_after == 1.5625; byhand.candidates.1.reduction == 36
#[test] fn overrides_are_restricted_and_clamped()           // lesson allows "rate" in [0.05,1]; overrides {"rate":5,"depth":9} → rate 1.0, depth unchanged
#[test] fn observation_arithmetic_is_exact()                // boosting run: after == before + rate*h to 1e-12; residual_after == y - after
#[test] fn large_data_is_refused_in_process()              // 5001 rows → Err containing "supervised job"
#[test] fn cancel_stops_a_running_fit()                    // 5000 rows × 300 trees depth 6 started, cancel after 50 ms → status Canceled within 2 s
```

`selection.test.ts`: subscribers are called once per change; setting the same
stage twice notifies once.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-app experiment` / `npm --prefix crates/app/ui test -- --run`
Expected: compile errors / missing module.

- [ ] **Step 3: Implement** Rust and the Run panel.

- [ ] **Step 4: Run**

Run: `cargo test -p model-lab-app --locked` and `npm --prefix crates/app/ui test -- --run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app crates/lessons
git commit -m "Shell: Run panel with in-process experiments, replay, linked selection and observation arithmetic"
```

---

### Task 10: Supervised jobs from the shell, resumable across relaunch

**Files:**
- Create: `crates/app/src/jobs.rs`, `crates/app/src/commands/jobs.rs`, `ui/src/panels/jobs.ts`
- Test: `crates/app/tests/jobs.rs`. `CARGO_BIN_EXE_*` is not defined across
  crates, so the test runs `cargo build -p model-lab-engine --bin model-lab-engine-job`
  once through `std::process::Command` and sets `MODEL_LAB_ENGINE_JOB` to
  `target/debug/model-lab-engine-job` (debug builds honour that variable;
  see `engine_job_path` below).

**Interfaces:**
- Consumes: `model_lab_supervisor::job::{Supervisor, JobSpec, JobHandle, JobEvent, CHECKPOINT_ENV}`,
  `limits::Limits`, `process::Command`, `protocol::NONCE_ENV`,
  `model_lab_engine::job_spec::{JobSpecFile, MAX_ROWS}`, `Run::from_trace`,
  `data::Table::parse_with_limit`.
- Produces: job directory `jobs/<ulid-like id: unix_ms-hex4>/` with
  `spec.json` (a `JobSpecFile`, `v 3`, `job "fit"`, `max_rows 50_000`,
  `checkpoint_every_trees` from the lesson or 10), `data.csv` (copied),
  `checkpoint.json`, `manifest.json`
  `{ "id", "label", "lesson_id", "created_ms", "status": "running"|"interrupted"|"completed"|"failed"|"canceled", "last_checkpoint_stage": Option<usize>, "error": Option<String> }`,
  and `result.json` (`Completed.result`) on completion.
  `fn engine_job_path() -> Result<PathBuf, String>`: beside
  `current_exe()`; under `cfg!(debug_assertions)` also `MODEL_LAB_ENGINE_JOB`
  then `target/debug/model-lab-engine-job`; release builds look beside the
  executable only. Limits: `cpu_seconds 3600`, `memory_bytes 4 GiB`,
  `wall 2 h`, `sample_interval 1 s`. Commands: `start_job(lesson_id, csv_path, overrides) -> job_id`;
  `list_jobs() -> Vec<Manifest>`; `resume_job(job_id)`; `cancel_job(job_id)`;
  `open_job_result(job_id) -> String /* run_id */` (via `Run::from_trace(result, MAX_ROWS)`,
  registered as a `RunView::Fitted` so the Run panel displays it). Events are
  forwarded to the webview as Tauri events `job://<id>` carrying `JobEvent`
  serialized; the manifest is updated on `Checkpoint`, `Completed`,
  `Failed`; on app setup every manifest with status `running` becomes
  `interrupted` (the process cannot still be alive: the supervisor's `Drop`
  killed its tree, and a crash left an orphan-free tree because of the
  process group / Job Object). The Jobs panel shows per job: label, stage /
  total, ETA as "between X and Y (from N observed trees)" or "estimating
  after 3 trees", CPU %, RSS, process count, last checkpoint, and buttons
  Cancel / Resume / Open result; progress is announced through an
  `aria-live="polite"` region at most every 5 s.

- [ ] **Step 1: Write the failing tests** in `crates/app/tests/jobs.rs`
(a 12,000-row synthetic CSV written to a temp dir, Boosting, 40 trees, depth 2,
checkpoint every 5):

```rust
#[test] fn start_job_writes_spec_manifest_and_completes()  // status completed; result.json parses; open_job_result gives a run with 41 stages
#[test] fn interrupted_job_resumes_from_its_checkpoint_after_relaunch()  // Review Focus 3: cancel after the 2nd Checkpoint event → drop the Jobs struct → new Jobs::load(dir) marks it interrupted with last_checkpoint_stage 10 → resume → completed; result.json == fresh run's result.json byte for byte
#[test] fn release_lookup_never_uses_developer_paths()      // engine_job_path_release_only(exe_dir_without_binary) is Err naming the expected sibling path (function split so the release branch is testable)
#[test] fn eta_is_shown_only_as_a_range_with_its_count()    // the Progress events forwarded contain eta None before 3 units and Some{low<=high, units_observed>=3} after
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-app --test jobs`
Expected: compile error.

- [ ] **Step 3: Implement `jobs.rs`, the commands and the panel.**

- [ ] **Step 4: Run**

Run: `cargo build -p model-lab-engine --bin model-lab-engine-job && cargo test -p model-lab-app --locked`
Expected: PASS; the resume test completes in under 60 s locally.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "Shell: supervised jobs with progress, ETA range, resources, cancel and resume across relaunch"
```

---

### Task 11: Check panel: predict before reveal, questions, the learner's record

**Files:**
- Create: `crates/app/src/commands/record.rs`, `ui/src/panels/check.ts`
- Modify: `crates/app/src/state.rs` (record opening at setup; Review Focus 4)
- Test: unit tests in `commands/record.rs`

**Interfaces:**
- Consumes: `Record`, `Lesson::{predictions, questions}`, `context`, `render`.
- Produces: at setup `Record::open(dirs.record/default.sqlite)`; on error the
  state keeps `record_error = Some(format!("Your answers are not being saved: {path}: {e}"))`
  and every record command returns `Ok` with `saved: false`. Commands:
  `check_panel(lesson_id, run_id) -> CheckPanel { predictions: Vec<PredictionView { id, prompt_html, kind, committed: Option<String>, revealed: Option<Reveal { actual, correct, explanation_html }> }>, questions: Vec<QuestionView { id, concept, stem_html, options_html, answered: Option<Answered { chosen, correct, explanation_html, from_earlier_setting: bool }> , locked_until_prediction: Option<String> }>, record_notice: Option<String>, history: LessonHistorySummary }`;
  `commit_prediction(lesson_id, run_id, prompt_id, predicted) -> Reveal`
  (the actual value is read from the run context at `Prediction.reveals`;
  `Direction` compares the sign of the revealed quantity; `Number` uses the
  prompt's tolerance; stored immediately); `answer_question(lesson_id, run_id, question_id, chosen) -> Answered`
  (correct iff `chosen` set equals `correct` set; `after_prediction` true iff
  `requires_prediction` is None or that prediction was committed in this
  attempt; the explanation is rendered with the run's context at answer time
  and stored; `from_earlier_setting` is true when the stored attempt's
  `experiment_hash` differs from the current run's: Review Focus 5);
  `export_record() -> PathBuf` (JSON to `packets/record-<date>.json`).
  Options are shuffled per attempt with a seed from the attempt id so the
  correct position varies; the stored `chosen` are original indices.

- [ ] **Step 1: Write the failing tests** (in-memory record, fixture lesson,
a `RunView::ByHand`):

```rust
#[test] fn direction_prediction_is_scored_from_the_live_quantity()  // prompt reveals selected.h for row 0 (negative) ; predicted "down" → correct
#[test] fn question_locked_until_its_prediction_is_committed()      // answer before commit → Err containing "prediction"; after commit → Ok with after_prediction true in the record
#[test] fn answer_keeps_its_rendered_numbers_and_flags_setting_changes()  // Review Focus 5: answer with rate 0.5; new run with rate 0.25 → check_panel shows from_earlier_setting true and the old explanation_html containing "3.25"
#[test] fn unsaved_record_is_reported_not_fatal()                   // Review Focus 4: state with record None and record_error Some → check_panel.record_notice Some; commit_prediction Ok
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-app record`
Expected: compile error.

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run**

Run: `cargo test -p model-lab-app --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "Shell: Check panel with predict-before-reveal, questions and the learner record"
```

---

### Task 12: Instructor mode and packets

**Files:**
- Create: `crates/lessons/src/packet.rs`, `crates/app/src/commands/packet.rs`
- Modify: `ui/src/main.ts` (instructor toggle in the header, persisted in the record `meta`), `ui/src/panels/{read,check}.ts` (instructor notes and answer keys when on)
- Test: unit tests in `packet.rs`

**Interfaces:**
- Consumes: `Lesson`, `RunView`, `context`, `render`, `to_html`, `plots`.
- Produces:

```rust
pub struct PacketFiles { pub dir: PathBuf, pub files: Vec<PathBuf> }
/// Writes packets/<lesson_id>-<yyyymmdd-hhmm>/ with student.html, experiment.json, data.csv, results.json (Run::to_json or the byhand rounds),
/// figures/README.txt listing the figure ids the shell saves next; instructor=true adds instructor.html (notes, answer keys, each
/// question's explanation rendered with the live numbers). student.html never contains instructor notes, correct indices or
/// explanations. All HTML is self-contained (inline CSS, TeX left as text with its source).
pub fn write(lesson: &Lesson, view: &RunView, stage: usize, instructor: bool, out_root: &Path) -> Result<PacketFiles, String>;
```

  Command `export_packet(lesson_id, run_id, student_only: bool) -> PacketFiles`
  (instructor packet only when `instructor` is on); `save_figure(packet_dir, id, svg: String)`
  writes `figures/<id>.svg` (the webview exports SVG via vega-embed's view
  `toSVG()` for every plot in the Run panel and calls this once per plot).

- [ ] **Step 1: Write the failing tests**:

```rust
#[test] fn student_packet_has_no_answers_or_notes()   // fixture lesson with instructor.md and questions: student.html lacks "correct", the explanation text and the notes text; files has no instructor.html
#[test] fn instructor_packet_renders_live_numbers()   // instructor.html contains "3.25" from the byhand context
#[test] fn packet_is_self_contained()                 // no "http://", "https://" or "src=\"/" in either HTML
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-lessons packet`
Expected: compile error.

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run**

Run: `cargo test --workspace --locked --exclude tree-ensemble-lab`
Expected: PASS (the egui package is excluded here because its tests need
the scientific Python; it is run in Task 13's CI step as before).

- [ ] **Step 5: Commit**

```bash
git add crates/lessons crates/app
git commit -m "Instructor mode and student/instructor packets"
```

---

### Task 13: Sidecar packaging, CI on both platforms, build scripts, documentation

**Files:**
- Create: `packaging/sidecar.py`, `.github/workflows/shell.yml`
- Modify: `crates/app/tauri.conf.json` (`bundle.externalBin: ["binaries/model-lab-engine-job"]`,
  `bundle.macOS.signingIdentity "-"`); create the platform overlays
  `crates/app/tauri.macos.conf.json` (`bundle.targets: ["app", "dmg"]`) and
  `crates/app/tauri.windows.conf.json` (`bundle.targets: ["nsis"]`), which
  Tauri merges automatically,
  `Build Mac.command`, `Build Windows.cmd`, `docs/ARCHITECTURE.md`, `README.txt`
- Test: the workflow itself; `packaging/sidecar.py --check`

**Interfaces:**
- Produces: `packaging/sidecar.py` (stdlib only, run with `uv run --offline python`):
  `cargo build -p model-lab-engine --bin model-lab-engine-job --release --locked`,
  read the host triple from `rustc -vV`, copy to
  `crates/app/binaries/model-lab-engine-job-<triple>[.exe]`; `--check`
  verifies the copy exists and matches the build's SHA-256 and exits 2
  otherwise. `shell.yml`: `macos-latest` and `windows-2022`;
  `actions/setup-node@v4` with `node-version: 24`; `npm --prefix crates/app/ui ci`;
  `npm --prefix crates/app/ui test -- --run`; `cargo test --workspace --locked --exclude tree-ensemble-lab`;
  `uv run --offline python packaging/sidecar.py` (install `uv` with
  `astral-sh/setup-uv@v5`); `npm --prefix crates/app/ui run tauri build -- --ci`;
  upload `target/release/bundle/**` as artifacts; a final step asserts the
  bundle contains the sidecar (`Model Lab.app/Contents/MacOS/model-lab-engine-job`
  on Mac, `model-lab-engine-job.exe` beside the exe in the NSIS staging dir on Windows)
  and that `strings`/`Select-String` of the main binary finds no `/Users/ian`
  (the Mac build sets `RUSTFLAGS` remap flags as `Build Mac.command` does).
  The existing `engine.yml` and `windows.yml` are unchanged.

- [ ] **Step 1: Write the check first**: `packaging/sidecar.py --check` is run
before any build in a fresh clone.

Run: `uv run --offline python packaging/sidecar.py --check`
Expected: exit 2, "sidecar missing: crates/app/binaries/model-lab-engine-job-aarch64-apple-darwin".

- [ ] **Step 2: Implement `sidecar.py`, the config changes, the workflow and the build scripts.**
`Build Mac.command` gains `uv run --offline python packaging/sidecar.py` and
`npm --prefix crates/app/ui ci && npm --prefix crates/app/ui run tauri build`
after the existing egui steps; `Build Windows.cmd` likewise.

- [ ] **Step 3: Run locally**

Run: `uv run --offline python packaging/sidecar.py && uv run --offline python packaging/sidecar.py --check && npm --prefix crates/app/ui run tauri build`
Expected: check exits 0; `target/release/bundle/macos/Model Lab.app` exists
and contains `Contents/MacOS/model-lab-engine-job`; launching the app and
starting a 12,000-row job from the Jobs route (CSV from
`examples/income.csv` duplicated to 12,000 rows in a temp file) shows
progress with an ETA range.

- [ ] **Step 4: Push a branch and read the workflow**

Run: `git push -u origin HEAD && gh run watch --exit-status $(gh run list --workflow shell.yml --limit 1 --json databaseId -q '.[0].databaseId')`
Expected: both matrix jobs green; artifacts listed.

- [ ] **Step 5: Document** in `docs/ARCHITECTURE.md`: the four layers as
built, the IPC command list, the lesson directory format (copy the
interfaces from Task 3), the runtime directories, and the sidecar rule.
`README.txt` gains a "Model Lab (new shell)" section and keeps the egui
instructions under "Research tool (0.8 shell)".

- [ ] **Step 6: Commit**

```bash
git add packaging crates/app/tauri.conf.json .github "Build Mac.command" "Build Windows.cmd" docs/ARCHITECTURE.md README.txt
git commit -m "Package the engine job as a Tauri sidecar; shell CI on macOS and Windows"
```

---

### Task 14: Lesson 2, boosting by hand (built first, per the spec)

**Files:**
- Create: `lessons/02-boosting-by-hand/{lesson.md,experiment.json,questions.json,derivation.md,instructor.md}`
- Test: `crates/lessons/tests/content.rs` (shared by Tasks 14–17; each task adds its lesson id to the list)

**Interfaces:**
- Consumes: the lesson format (Task 3), template names (Task 4),
  `RunView::ByHand` context (Task 9).
- Produces: `experiment.json` `{ "kind": "by_hand", "source": "four_point", "target": "y", "features": ["x"], "byhand": { "rate": 0.5, "depth": 1, "rounds": 6 }, "learner_may_change": [ { "field": "rate", "min": 0.1, "max": 1.0, "step": 0.1 } ], "config": <RunConfig::to_json default with model "boosting", trees 6, depth 1, min_leaf 1, rate 0.5> }`.
  Front matter: `id: boosting-by-hand`, `level: Beginner`,
  `prerequisites: [one-tree]`, `estimated_minutes: 35`,
  `concepts: [residual, stump, learning-rate, additive-model, training-loss]`.
  `lesson.md` sections, in this order: the four points and the mean
  (`{{byhand.mean}}` = 6); residuals as the thing to predict next; the first
  stump and where it splits (`{{byhand.round.0.threshold}}` = 2.5, leaf means
  −3 and 3); "half the correction" with the arithmetic
  `6 + 0.5 × (−3) = 4.5`; the loss path `{{byhand.round.0.mse_before:.4f}} → {{byhand.round.0.mse_after:.4f}} → {{byhand.round.1.mse_after:.4f}}`
  (10 → 3.25 → 1.5625); why the learning rate retrains the whole path (change
  the slider, every later number moves); a failure case: rate 1.0 reaches zero
  training loss on four points, and what that does and does not mean.
  `derivation.md`: five steps (mean, residuals, SSE of each candidate split,
  the update rule $F_1(x) = F_0 + \nu h_1(x)$, the loss after one round).
  Predictions: `p1` Direction "Before round 2: does the prediction for
  the point x = 2 go up or down?" reveals `selected.h` with `selected.row` 1;
  `p2` Number "What is the training MSE after round 2?" tolerance 0.01 reveals
  `byhand.round.1.mse_after`. Questions: at least 8, each tagged with one
  concept above, options 4, explanations ≥ 80 characters that name the
  mechanism, at least 3 marked `live: true` using templated numbers, at least
  2 with `requires_prediction`. Content drawn from the 100-bank topics
  "Gradient boosting" and "Decision trees" with distractors rewritten (no
  throwaway options) and explanations rewritten in full. `instructor.md`:
  what to watch for when a student predicts the wrong direction, and the
  rate-1.0 discussion.

- [ ] **Step 1: Write the failing content test** `crates/lessons/tests/content.rs`:

```rust
const LESSONS: &[&str] = &["boosting-by-hand"];   // grows in Tasks 15–17
#[test] fn every_bundled_lesson_loads()                      // load_all([repo lessons/]) has no errors and every id in LESSONS is present
#[test] fn every_template_resolves_against_its_experiment()  // build the RunView for the experiment (ByHand or a Run on the example data), render lesson.md, derivation and every live question: missing is empty
#[test] fn boosting_by_hand_numbers_are_the_taught_ones()    // rendered lesson.md contains "10.0000", "3.2500", "1.5625" and "2.5"
#[test] fn questions_meet_the_bar()                          // ≥ 8 questions, each concept in front.concepts, explanations ≥ 80 chars, ≥ 2 requires_prediction, ≥ 3 live
#[test] fn prerequisites_name_existing_lessons_or_planned_ids() // every prerequisite id ∈ LESSONS ∪ {"one-tree","boosting-on-data","random-forests"}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-lessons --test content`
Expected: FAIL, the lesson directory does not exist.

- [ ] **Step 3: Write the lesson files.** Mark the front matter
`status: draft-for-review` (an extra key the loader keeps in
`FrontMatter.extra: BTreeMap<String, String>`; shown in instructor mode as
"Draft, awaiting Dr. Helfrich's review") because the prose is the author's
to approve.

- [ ] **Step 4: Run**

Run: `cargo test -p model-lab-lessons --test content`
Expected: PASS.

- [ ] **Step 5: Walk the lesson in the dev app** (`npm --prefix crates/app/ui run tauri dev`):
Read shows rendered math; Run shows the four points, the slider, round
stepping and `4.5 = 6 + 0.5 × (−3)` for the selected point; Check commits
the direction prediction, reveals, then unlocks the two questions.

- [ ] **Step 6: Commit**

```bash
git add lessons/02-boosting-by-hand crates/lessons/tests/content.rs
git commit -m "Lesson 2: boosting by hand on four points"
```

---

### Task 15: Lesson 1, one regression tree

**Files:**
- Create: `lessons/01-one-tree/{lesson.md,experiment.json,questions.json,derivation.md,instructor.md}`
- Modify: `crates/lessons/tests/content.rs` (`LESSONS` gains `one-tree`; a numbers test)

**Interfaces:**
- Produces: `experiment.json` `kind "by_hand"`, `source "four_point"`,
  `byhand { rate: 1.0, depth: 1, rounds: 1 }` (a single stump is round 0 at
  rate 1), `learner_may_change: [ { "field": "depth", "min": 1, "max": 2, "step": 1 }, { "field": "min_leaf", "min": 1, "max": 2, "step": 1 } ]`
  (the `RunView::ByHand` applies `depth` to `boosting_rounds` and `min_leaf`
  by filtering `candidate_splits` to candidates whose sides both have at
  least `min_leaf` rows; Task 9's context exposes
  `byhand.candidates.<j>.admissible`). Front matter `id: one-tree`, no
  prerequisites, `concepts: [sse, split-rule, leaf-mean, depth, min-leaf]`,
  `estimated_minutes: 25`. Sections: what a leaf predicts (the mean,
  SSE `{{byhand.sse}}` = 40); the three candidate thresholds and their SSE
  reductions (`{{byhand.candidates.1.reduction}}` = 36 against 21.33); why the
  split is at 2.5 and not at a data point; depth 2 on four points (every
  point its own leaf, training SSE 0) as the failure case; minimum leaf as
  the brake. Prediction `p1` Choice "Which threshold will the tree choose?"
  options `1.5 | 2.5 | 3.5` reveals `byhand.round.0.threshold`. Questions ≥ 8
  from the 100-bank "Decision trees" topic rewritten, same bar as Task 14.

- [ ] **Step 1: Extend the failing content test**: add `one-tree` to
`LESSONS` and `one_tree_shows_all_three_candidates()` asserting the rendered
text contains "36" and "21.33" and "2.5".

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-lessons --test content`
Expected: FAIL, missing lesson.

- [ ] **Step 3: Write the lesson files** (`status: draft-for-review`). Add
`byhand.sse` and `byhand.candidates.<j>.admissible` to `context` in Task 9's
`experiment.rs` if not present.

- [ ] **Step 4: Run**

Run: `cargo test -p model-lab-lessons --test content && cargo test -p model-lab-app --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add lessons/01-one-tree crates/lessons/tests/content.rs crates/app
git commit -m "Lesson 1: one regression tree on four points"
```

---

### Task 16: Lesson 3, boosting on real data

**Files:**
- Create: `lessons/03-boosting-on-data/{lesson.md,experiment.json,questions.json,instructor.md}`
- Modify: `crates/lessons/tests/content.rs`

**Interfaces:**
- Produces: `experiment.json` `kind "run"`, `source { "example": "Income, age and year" }`
  (3,000 synthetic rows), `target "income"`, features: the table's other
  numeric columns as named in `examples/generate.py`, `config`: model
  `boosting`, `trees 150`, `depth 2`, `min_leaf 5`, `rate 0.1`,
  `validation 0.25`, `seed 4710`, `patience 0`; `learner_may_change`:
  `depth 1..=5 step 1`, `rate 0.02..=0.5 step 0.02`, `fraction 0.3..=1.0 step 0.1`
  (fraction below 1 switches `model` to `stochastic`: the shell applies this
  rule, documented in `experiment.rs`), `trees 20..=300 step 10`.
  Front matter `id: boosting-on-data`, `prerequisites: [boosting-by-hand]`,
  `concepts: [validation-curve, best-stage, overfitting, depth-capacity, subsampling, validation-is-not-test]`,
  `estimated_minutes: 45`. Sections: training versus validation curves with
  denominators (`{{rows.train:,}}` / `{{rows.valid:,}}`); the best stage is
  computed (`{{stage.best}}`, `{{metric.best_valid_mse:.2f}}`), not assumed;
  depth 1 versus depth 5 (what the learner should see when sliding); subsampling
  with hollow points for rows not used this round (the scatter marks
  `in_training && !in_sample` rows hollow: `Observation.in_sample` from
  `Fit.samples[stage-1]`, added in Task 9's `Observation`); the failure case:
  choosing the stage on the validation set and then reporting that validation
  score as "test performance"; why validation is not a test (the plan's
  honest-accounting line). Prediction `p1` Direction "Will the validation MSE
  at stage 150 be lower than at the best stage?" reveals
  `metric.valid_mse_at_last_minus_best` (add to context: valid loss at the last
  stage minus at `stage.best`, ≥ 0 by construction; "down" is never right,
  which the explanation makes the point of). Questions ≥ 8 from the
  "Validation and metrics", "Gradient boosting" and "Stochastic boosting and
  tuning" topics, rewritten.

- [ ] **Step 1: Extend the failing content test**: add `boosting-on-data`;
`boosting_on_data_renders_its_denominators()` asserts the rendered text
contains "2,250" and "750" (3,000 rows at validation 0.25).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-lessons --test content`
Expected: FAIL.

- [ ] **Step 3: Write the lesson files**; add `in_sample`, the fraction→stochastic rule and `metric.valid_mse_at_last_minus_best` to Task 9's code with a unit test each (`in_sample_marks_rows_outside_this_rounds_sample`, `fraction_below_one_selects_stochastic`, `last_minus_best_is_nonnegative`).

- [ ] **Step 4: Run**

Run: `cargo test -p model-lab-lessons --test content && cargo test -p model-lab-app --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add lessons/03-boosting-on-data crates/lessons/tests/content.rs crates/app
git commit -m "Lesson 3: boosting on real data, validation is not a test"
```

---

### Task 17: Lesson 4, random forests

**Files:**
- Create: `lessons/04-random-forests/{lesson.md,experiment.json,questions.json,instructor.md}`
- Modify: `crates/lessons/tests/content.rs`

**Interfaces:**
- Produces: `experiment.json` `kind "run"`, `source { "example": "Florida communities" }`
  (90 rows, eight predictors), `config`: model `forest`, `trees 200`,
  `depth 6`, `min_leaf 3`, `mtry 3`, `validation 0.25`, `seed 4710`;
  `learner_may_change`: `mtry 1..=8 step 1`, `trees 10..=300 step 10`,
  `depth 1..=10 step 1`. Front matter `id: random-forests`,
  `prerequisites: [one-tree, boosting-on-data]`,
  `concepts: [bootstrap, feature-subsampling, oob-error, oob-coverage, permutation-importance, pdp-ice-slice]`,
  `estimated_minutes: 45`. Sections: the bootstrap and why about a third of
  rows are out of bag (`{{oob.coverage:.2f}}` at the current stage, with the
  row denominator from the `oob_curve` table); feature subsampling (`mtry`)
  and what changes as it moves from 1 to 8; OOB error beside validation
  error, both with denominators; permutation importance with its repeat SD
  (`{{importance.<top feature>.mean:.3f}} ± {{importance.<top feature>.sd:.3f}}`,
  the top feature named in the text as the one the synthetic mechanism
  makes strongest per `examples/generate.py`); PDP versus ICE versus a
  slice, with the sentence that a slice fixes other predictors at their
  training medians and is not a PDP; the failure case: reading importance
  as a causal ranking. Prediction `p1` Direction "When mtry goes from 3 to 8,
  does OOB error rise or fall for this data?" reveals
  `oob.mse_delta_mtry_full` (context key computed by fitting the forest once
  more at `mtry = p` with the same seed, cached per run; it is a real second
  fit, labelled as such in the reveal). Questions ≥ 8 from "Bagging and random
  forests" and "Interpretation and diagnostics", rewritten.

- [ ] **Step 1: Extend the failing content test**: add `random-forests`;
`random_forests_shows_oob_with_its_rows()` asserts the rendered text has a
coverage number and the word "rows" in the same sentence, and the importance
appears with "±".

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p model-lab-lessons --test content`
Expected: FAIL.

- [ ] **Step 3: Write the lesson files**; add `oob.mse_delta_mtry_full` to Task 9's context with a unit test (`mtry_comparison_is_a_real_second_fit`: the comparison run's `cfg.mtry == p` and its predictions differ from the main run's).

- [ ] **Step 4: Run**

Run: `cargo test --workspace --locked --exclude tree-ensemble-lab`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add lessons/04-random-forests crates/lessons/tests/content.rs crates/app
git commit -m "Lesson 4: random forests, OOB, importance with its spread, PDP versus ICE versus slice"
```

---

### Task 18: The "done" walkthrough on a built bundle

**Files:**
- Create: `docs/WALKTHROUGH-PHASE1.md` (the checklist and the observed results, dated)
- Test: this task is the one non-automated check in the plan; every item has
  an expected observation.

- [ ] **Step 1: Build the bundle** as in Task 13 Step 3.

- [ ] **Step 2: Fresh-profile walkthrough.** Move any existing
`app_data_dir` aside. Open `Model Lab.app`. Expected, in order: curriculum
lists lessons 1–4 with prerequisites shown and none locked; lesson 2 Read
renders math (inspect: every `.math` element has a `.katex` child and an
`annotation` with the TeX); Run steps six rounds and shows the arithmetic
for a selected point; Check commits both predictions and answers eight
questions; lessons 1, 3, 4 open and run (lesson 3 fits in under 3 s; lesson
4 under 5 s); every plot's "Show data table" produces a table whose row
count matches the legend's n; Tab reaches every control; OS dark mode
switches the theme. Quit and reopen: the lesson 2 record is present with
the answers and the "from an earlier setting" marker after changing the
rate.

- [ ] **Step 3: Instructor packet.** Toggle instructor mode, export lesson 3
packets. Expected: `packets/boosting-on-data-<stamp>/` has `student.html`
without answers, `instructor.html` with them, `figures/*.svg` (one per plot),
`data.csv`, `results.json`.

- [ ] **Step 4: The 50,000-row job.** Write a 50,000-row CSV from
`examples/generate.py`'s income mechanism (a one-off script in the
scratch directory, not committed), start a boosting job (120 trees, depth
3, checkpoint every 10) from the Jobs route. Expected: progress by tree, an
ETA shown as a range with its observed count after the third tree, CPU and
RSS updating each second, process count 1. Quit the app at about tree 50.
Expected: `ps` / Task Manager shows no `model-lab-engine-job`. Reopen:
the job is "interrupted at checkpoint 50"; Resume; completes; Open result
shows a 121-stage run in the Run panel with the loss curve and the best
stage.

- [ ] **Step 5: Binary hygiene.** `strings "target/release/bundle/macos/Model Lab.app/Contents/MacOS/model-lab" | grep -c "/Users/ian"` → `0`;
the same for the sidecar.

- [ ] **Step 6: Record the results** in `docs/WALKTHROUGH-PHASE1.md` with
the date, commit, machine, and any deviation.

- [ ] **Step 7: Commit**

```bash
git add docs/WALKTHROUGH-PHASE1.md
git commit -m "Phase 1 walkthrough on the built bundle"
```

---

## Self-review (done at writing time)

**Spec coverage.** Section 4 "Content and shell": KaTeX with text
alternative → Tasks 5, 8; lessons loaded at runtime from bundled plus user
directories, sanitized, plain substitution with a format spec, schema
validation → Tasks 3, 4, 5; Vega-Lite from Rust, table toggle, zoom/pan/
export, one palette and tick algorithm, linked selection → Tasks 6, 9;
lesson directory contents → Task 3; three panels per lesson and one route
model → Tasks 8, 9, 11; mastery rule and advisory prerequisites → Tasks 7,
8; SQLite learner record, exportable, never uploaded → Tasks 7, 11; theme,
reduced motion, scalable type → Task 8; instructor mode and packets → Task
12. "Phase 1 scope": supervised jobs above 5,000 rows from the shell, 50k
job in "done" → Tasks 10, 18; 0.8 experiments read-only with the notice →
`Run::from_json` already returns `SCHEMA_ONE_NOTICE`, and `open_job_result`
surfaces it (no new task). "Left to the builder: reading back a supervised
result" → Task 1. "First lesson set" 1–4 → Tasks 14–17 (lesson 2 first, as
the spec orders). "Migration of kept assets": observation arithmetic,
replay, linked selection → Task 9; predict-the-next-move → Tasks 11, 14;
question bank → Tasks 14–17; OOB/importance/PDP/ICE with the slice
distinction → Tasks 6, 17. "Done": every item is in Task 18. Not in this
plan by design: the practice editor and pixi (plan 3); porting the old
studies' read-only display (plan 3, with the study workers); Intel Mac
builds (nice to have).

**Step scan.** Every code step names the file, signature and the values the
spec pins; bodies appear only for the lesson content requirements, which
are values, not code. Lesson prose is explicitly the executor's draft for
Dr. Helfrich's review (`status: draft-for-review`).

**Type consistency.** `Context`, `Rendered`, `Plot`, `Table`, `RunView`,
`Observation`, `Lesson`, `Experiment`, `Learner`, `Record`, `Manifest` are
named identically across Tasks 3–17. Template path names are fixed once in
Task 4 and extended in Tasks 15–17 with a unit test each.

**Review Focus.** Items 1–5 are pinned to Tasks 5, 4, 10, 7+11, 11.

**Proportion.** Eighteen tasks for the shell, four content lessons and the
packaging. The plan is shorter than the code it describes; the longest
blocks are the interface listings, which are the decisions the implementer
cannot make alone.

**Known risks the executor should expect.** Tauri 2 on `windows-2022` needs
no extra setup (WebView2 is present); on the Mac the first `tauri build`
compiles ~600 crates and takes several minutes. `vite 8` and `typescript 7`
are current releases at writing time; if either breaks the Vite plugin
surface, pin one major lower and ledger the ruling. The `rusqlite` bundled
build adds ~40 s to a clean build. KaTeX fonts must be copied by Vite from
`katex/dist/fonts` (import `katex/dist/katex.min.css` in `main.ts`).
