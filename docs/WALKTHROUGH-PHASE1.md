# Phase 1 walkthrough on the built bundle

Date: 2026-11-09. Branch `tauri-shell`, worktree
`~/Developer/model-lab/.worktrees/tauri-shell`, Apple silicon Mac
(aarch64-apple-darwin), Rust 1.98.1, Node 24.20.0. Bundle built by
`packaging/sidecar.py` + `tauri build --ci` with the remap flags from
`Build Mac.command`: `target/release/bundle/macos/Model Lab.app` and
`Model Lab_0.9.0_aarch64.dmg` (6.5 MiB).

The plan's Task 18 is the one non-automated check. The interactive part
(driving the window) could not be performed by the implementing agent:
screen access to the running bundle was declined when requested, so the
items below are split into what was verified by automation on the built
artefacts and what remains for Dr. Helfrich to click through. Nothing
below is inferred from the diff; every "verified" line names its evidence.

## Verified on the built bundle

| Check | Evidence |
| --- | --- |
| Bundle carries the sidecar and the four lessons | `Model Lab.app/Contents/MacOS/{model-lab, model-lab-engine-job}`; `Contents/Resources/lessons/{01-one-tree, 02-boosting-by-hand, 03-boosting-on-data, 04-random-forests}` |
| No developer paths in either shipped binary | `strings … \| grep -c /Users/ian` = 0 for `model-lab` and for `model-lab-engine-job` (the first bundle had 16 in the sidecar; `sidecar.py` now remaps) |
| Fresh profile created at first launch | app data directory moved aside, bundle launched with `open`; `record/default.sqlite` (WAL) and `jobs/` created; no process left after quit |
| The 50,000-row job: progress with ETA range and resource samples, five checkpoints, quit, no orphan, resume from checkpoint 50, identical result, 121-stage run opens read-only | `cargo test -p model-lab-app --test jobs -- --ignored walkthrough` with `MODEL_LAB_ENGINE_JOB` pointing at the bundle's sidecar: fresh run under 1 s, resume from tree 50 in 2 s (2 predictors, release sidecar); `pgrep model-lab-engine-job` empty after the simulated quit |
| Every lesson loads, every template resolves, the taught numbers appear, the question bar is met | `cargo test -p model-lab-app --test content` (8 tests) |
| Sanitization, templating, math placeholders, plots with tables, record, mastery rule, packets | `cargo test --workspace --locked --exclude tree-ensemble-lab`: 165 tests, 0 failures |
| Front end types and unit tests | `npx tsc --noEmit` clean; `vitest` 3 tests (KaTeX rendering keeps the TeX source; selection store) |
| CI on both shipping platforms | `.github/workflows/shell.yml` on the draft pull request; see the ledger line below for the run that passed |

## Found by the fresh review and fixed before integration

The whole-branch review (a fresh reviewer, 2026-11-09) found two defects the
tests could not see and the declined window check would have: Vega's
expression compiler is blocked by the webview's Content Security Policy (so
no plot would have rendered), fixed by running Vega through its expression
interpreter with a vitest that forbids `Function` and renders a spec; and
Tauri's `run` exits the process without destructors, so a quit on macOS left
the supervised job running, fixed by killing the job trees on
`RunEvent::Exit` (tested: `shutdown_kills_the_tree_and_leaves_the_manifest_running`)
and by the engine job exiting after a checkpoint when its parent dies
(tested: `orphaned_engine_job_exits_when_its_parent_dies`). Also fixed: the
mastery rule now requires a committed prediction in the attempt (spec over
plan), repeated answers are refused, `save_figure` compares real paths, the
scatter uses O(n) masks and is thinned above 5,000 rows with the cap stated,
lesson hyperlinks are removed (they would navigate the webview away), the
Run panel's sliders show the fitted settings, and concurrent panel renders
no longer interleave.

## Not yet verified (needs a person at the window)

These are the interactive items of the plan's Task 18, in order. Each has
its expected observation.

1. Open `Model Lab.app`. Expected: the curriculum lists the four lessons
   with "Assumes: …" lines and "Status: draft-for-review"; none is locked.
2. Lesson 2, Read. Expected: every formula renders (each `.math` element has
   a `.katex` child whose `annotation` holds the TeX); the notice lists the
   numbers the experiment will fill in.
3. Lesson 2, Run. Expected: the experiment runs on its own, the rounds plot
   appears with "Show data table" producing 4 rows per round, the slider
   changes the rate and every later number, stepping to round 1 and
   selecting observation 2 shows `6.000 + 0.500 × (-3.000) = 4.500`.
4. Lesson 2, Check. Expected: both predictions can be committed and reveal
   `-3` and `1.5625`; question 3 and 5 unlock only after their prediction;
   eight questions answer with explanations carrying the live numbers.
5. Lessons 1, 3, 4 open and run (3 and 4 in a few seconds); the scatter
   shows hollow points when lesson 3's fraction is below 1; lesson 4 shows
   the OOB curve with coverage, importance with ± ranges, and the PDP/ICE/
   slice plot.
6. Tab reaches every control; switching the OS appearance switches the theme.
7. Quit and reopen: lesson 2's Check shows the record ("1 attempt, 2
   predictions, n of 8 answers correct"); change the rate, run, and the
   earlier answers appear collapsed as "from an earlier setting".
8. Instructor mode on; Run panel of lesson 3: "Export student packet" and
   "Export instructor packet". Expected: `packets/boosting-on-data-<stamp>/`
   with `student.html` (no answers), `instructor.html`, `data.csv`,
   `results.json`, `figures/*.svg` (one per plot).
9. Jobs route: choose the 50,000-row CSV, start. Expected: progress by
   tree, "ETA between … and … (from N observed trees)" after the third
   tree, CPU %, memory and process count updating each second; quit at
   about tree 50, `pgrep model-lab-engine-job` empty; reopen, the job
   reads "interrupted", Resume completes, Open result shows a 121-stage
   run with the loss curve and the best stage.

## Walked on 2026-11-09 (background window control, after the merge)

Dr. Helfrich granted application access to the running bundle; the
walkthrough ran with background control, which delivers link clicks and
scrolling to the webview but not button presses, slider moves or typing
(WebKit ignores them in a window that is not key). Full-screen control was
requested twice and timed out without an answer. So the items split again.

Seen and passing:

- Item 1 in full. Item 2: every formula renders through KaTeX; the notice
  lists the numbers the run will fill in.
- Item 3 partly: the by-hand experiment runs on its own and the rounds
  plot renders through the Vega interpreter (the review's C1 fix holds).
- Item 5 partly: lessons 1, 3 and 4 open and run in a few seconds; lesson
  3 shows the loss curve and the scatter with its slice line, and its
  sliders show the fitted settings (depth 2, rate 0.1, fraction 1, 150
  trees); lesson 4 shows the loss curve, the scatter, the OOB curve with
  coverage, permutation importance with ± ranges and the PDP/ICE/slice.
- Item 7 partly: quit from the menu left no `model-lab` process; the
  reopened app showed the Check tab asking for a run first, as a fresh
  process should.
- The Jobs page renders with its lesson selector, CSV chooser and Start.
- Question options come out in a different order on a new run.

Two defects found and fixed with failing-first tests, then rebuilt and
re-seen in the window:

- Every non-ASCII character outside math and code was mojibake (`−4`
  showed as `â` and boxes): the math extractor copied bytes as chars.
  Fixed in `markdown.rs`; test `non_ascii_text_outside_math_survives`.
- Lesson 2's first prediction prompt showed `[missing: selected.before]`:
  the Check panel rendered prompts in the panel-wide context while the
  commit used the prediction's own row and stage. Fixed in `record.rs`;
  test `prediction_prompt_is_rendered_for_its_own_row_and_stage`.

Cosmetic, not fixed: the loss curve's "best stage N" label is clipped at
the plot's right edge when the best stage is near the end; the importance
plot's x-axis tick labels collide on a wide range; a `[missing: …]`
placeholder inside display math is typeset as math (underscores become
subscripts) until the experiment runs.

Still needs a person at the window (button presses): item 3's slider,
round stepping, observation arithmetic and data table; item 4
(predictions, locks, eight questions); item 5's hollow points at a
fraction below 1; item 6; item 7's record and "earlier setting" display;
item 8; item 9.
