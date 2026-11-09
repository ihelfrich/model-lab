# Model Lab architecture (as built, Phase 1)

Four layers with versioned contracts. Everything numerical lives in Rust
crates with no UI dependency; the webview renders what Rust sends.

## Crates

| Crate | Path | Role |
| --- | --- | --- |
| `model-lab-engine` | `crates/engine` | data, solvers, ensembles (stage-wise, per-tree RNG), experiment record (`Run`, trace schema 2 with the ensemble state), `byhand` arithmetic for the first lessons, the `model-lab-engine-job` binary |
| `model-lab-supervisor` | `crates/supervisor` | process trees (process group / Job Object), limits, protocol v3 (nonce-tagged NDJSON), atomic checkpoints, ETA range, resource sampling, `Supervisor`/`JobHandle`/`JobEvent` |
| `model-lab-lessons` | `crates/lessons` | lesson directory format and validation, template substitution, Markdown with mathematics to sanitized HTML, Vega-Lite plots with tables, packets |
| `model-lab-record` | `crates/record` | SQLite learner record: sessions, attempts, predictions, answers, mastery |
| `model-lab-app` | `crates/app` | the Tauri 2 shell: `AppState`, IPC commands, in-process experiments, supervised jobs, packets; front end in `crates/app/ui` (Vite + TypeScript, KaTeX, vega-embed) |
| `tree-ensemble-lab` | root | the 0.8 egui application, kept as the research tool through Phase 1 |

## Lesson directory format

One directory per lesson under `lessons/` (bundled) or the application data
directory's `lessons/` (user; same id overrides):

- `lesson.md`: YAML front matter (`id`, `title`, `level`, `prerequisites`,
  `estimated_minutes`, `concepts`, any extra scalar such as `status`) then
  Markdown with `$…$`/`$$…$$` mathematics and `{{live.numbers}}`.
- `experiment.json`: `kind` (`run` | `by_hand`), `source` (`"four_point"`,
  `{"example": "<bundled table name>"}`, `{"csv": "<path>"}`), `target`,
  `features`, `config` (any subset of `RunConfig::to_json` keys; the rest are
  defaults), `byhand` (`rate`, `depth`, `rounds`) for by-hand lessons,
  `learner_may_change` (`field`, `min`, `max`, `step`; fields are `RunConfig`
  keys), `checkpoint_every_trees` (default 10).
- `questions.json`: `predictions` (`id`, `prompt`, `kind`: `"direction"` |
  `{"number": {"tolerance"}}` | `{"choice": {"options"}}`, `reveals` template
  path, optional `row`, `stage`, `explanation`) and `questions` (`id`,
  `concept` from the front matter, `stem`, 2–6 `options`, `correct` indices,
  `explanation` ≥ 80 characters, `live`, `requires_prediction`).
- `derivation.md` (optional): steps separated by `---`.
- `instructor.md` (optional): shown in instructor mode and in the instructor packet.

Validation errors name the file and the field; a lesson that fails to load
is listed on the curriculum, never partially run.

## Template paths

`{{path}}`, `{{path:.3f}}`, `{{path:,}}`; a missing value renders as
`[missing: path]`. Column names appear with non-alphanumerics replaced by `_`
(`med.income` → `med_income`). Keys: `data.n`, `data.target`, `rows.train`,
`rows.valid`, `config.<key>`, `stage.current`, `stage.best`, `stage.last`,
`metric.train_mse`, `metric.valid_mse`, `metric.train_rmse`,
`metric.valid_rmse`, `metric.best_valid_mse`, `coef.intercept`,
`coef.<name>`, `oob.mse`, `oob.coverage`, `oob.rows_train`,
`importance.<name>.mean`, `importance.<name>.sd`, `selected.row`,
`selected.y`, `selected.before`, `selected.h`, `selected.rate`,
`selected.after`, `selected.residual_before`, `selected.residual_after`,
`selected.x.<name>`; by hand: `byhand.mean`, `byhand.sse`, `byhand.mse`,
`byhand.rate`, `byhand.round.<k>.{mse_before,mse_after,threshold,left_mean,right_mean}`,
`byhand.candidates.<j>.{threshold,reduction,sse_after,left_mean,right_mean,left_n,right_n,admissible}`.

## IPC commands

Lessons: `list_lessons`, `lesson_errors`, `read_panel`, `lesson_controls`,
`set_instructor`, `instructor`. Experiments: `start_experiment`,
`experiment_status`, `cancel_experiment`, `run_panel`, `read_panel_for_run`.
Jobs: `start_job`, `list_jobs`, `resume_job`, `cancel_job`,
`open_job_result`; events `job://<id>` carry `JobEvent`. Record:
`check_panel`, `commit_prediction`, `answer_question`, `export_record`.
Packets: `export_packet`, `save_figure`.

## Runtime directories

Tauri `app_data_dir` (`~/Library/Application Support/org.ianhelfrich.modellab`
on macOS, `%APPDATA%\org.ianhelfrich.modellab` on Windows): `lessons/`,
`record/default.sqlite`, `jobs/<id>/{spec.json,data.csv,checkpoint.json,manifest.json,result.json}`,
`packets/`.

## Sidecar rule

`model-lab-engine-job` is built in release by `packaging/sidecar.py` and
copied to `crates/app/binaries/model-lab-engine-job-<triple>`; Tauri bundles
it beside the application binary. Release builds look for it there only.
Debug builds also honour `MODEL_LAB_ENGINE_JOB` and `target/debug/`.

## Developing

From `crates/app/ui`: `npm ci`, `npm test -- --run`, `npx tsc --noEmit`.
From `crates/app`: `ui/node_modules/.bin/tauri dev` (the before-dev command
runs in `ui/`). Tests: `cargo test --workspace --locked --exclude tree-ensemble-lab`;
the egui application's tests need the scientific Python and run in the
Windows student package workflow.
