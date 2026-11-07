# Model Lab platform design

Date: 2026-11-07. Status: draft for Dr. Helfrich's approval. Nothing in
this document is built; Phase 0 (securing the existing egui application)
is complete and committed in this repository.

## Intent

Model Lab teaches statistical learning and causal machine learning by
letting the learner run real experiments and inspect their real internal
states. The existing application already does this at classroom scale for
nine models: real recorded solver states, observation-level arithmetic
from the data on screen, honest Monte Carlo accounting. What it cannot do
is render mathematics, be read by a screen reader on a Mac, host a job
longer than five minutes, or grow past nine models without editing twenty
dispatch sites. This design keeps everything that was verified and
replaces the parts that cap the ceiling.

The wedge, and the reason to build this rather than point students at
d2l.ai or a Jupyter notebook: nobody offers interactive teaching where
every lesson is an executable, inspectable experiment and the econometrics
is part of the machine learning. Identification, inference, Monte Carlo
evidence, prediction versus intervention: these are treated as the subject,
not a caveat. A student who learns boosting here also learns why a
validation score is not a test score and why a predictive fit is not a
causal effect, because the software will not let those distinctions blur.

First users are Dr. Helfrich's own tutoring students (the learners
in applied courses). The product must work on their laptops, Mac and
Windows, without them installing anything by hand.

## Goals

1. Every lesson is an executable experiment. The lesson text, the
   mathematics and the running model refer to the same object, and the
   learner can change the experiment and watch the lesson's numbers change.
2. Rendered mathematics, with derivations the learner can step through.
3. A prediction the learner commits to before the result is shown, with
   feedback that uses the actual numbers.
4. Honest accounting everywhere: real recorded states only, denominators
   beside every rate, Monte Carlo SE beside every simulated quantity,
   "known truth exists only in simulation" wherever it applies.
5. Progression from a first regression tree to cross-fitted causal
   estimators, with a persisted learner record (what was attempted, what
   was predicted, what was answered, what was mastered).
6. Real jobs: long-running, checkpointed, cancellable, resumable, with
   visible progress, an estimated time to completion that states its own
   uncertainty, and resource use the learner can see.
7. Reproducible exports (data, specification, code, figures, report
   source) and separate student and instructor packets, as today.
8. Instructor authoring by writing a lesson file, not by editing Rust.
9. Accessibility: a text alternative for every plot, keyboard operation,
   OS theme and reduced-motion settings honoured, screen-reader support.

## Non-goals (for this programme)

A hosted service, user accounts, credentials or certificates, an LLM
tutor, a mobile client, a general notebook environment, a marketplace of
third-party lessons.

## Anti-goals

These would make the product a failure even if everything else worked:

- A fabricated optimizer trace, interpolated "progress", or an ETA shown
  as a point estimate without its spread.
- A rate, coverage or score shown without its denominator or sample.
- A causal claim produced from a predictive fit, or simulated "truth"
  presented as if it applied to observed data.
- A package manager of our own.
- A model menu with more families than the product can actually teach
  (a model that runs but has no lesson, no diagnostics and no questions
  is not "supported").
- A test harness or developer-machine path compiled into a shipped binary.

## Constraints

- Mac (Apple silicon) and Windows x64 are the shipping targets. Intel Mac
  and ARM Windows are nice to have.
- No data leaves the machine. No telemetry. Network use only for
  environment downloads the user has agreed to.
- No spend without Dr. Helfrich's approval: no paid signing certificate,
  hosting or API until he decides.
- The delivered 0.5 to 0.8 archives in the tutoring outputs folder remain
  unchanged. Their checksums are in `docs/PROVENANCE.md`.
- Anything numerical that is kept must keep its existing tests and
  independent fixtures (`verify_linear_models.py`, the Engel reference,
  the simulation validation groups).

## Architecture (decided)

Four layers with versioned contracts. The decisions below were made by
Dr. Helfrich on 2026-11-07 where marked; the rest are the designer's calls
and are open to revision.

### 1. Engine crate (Rust, no UI dependency)

Extract `linear.rs`, `model.rs`, `decision.rs`, `science.rs`,
`experiment.rs`, `data.rs` and `study.rs` into a library crate
`model-lab-engine` with no `eframe`/`egui` dependency. It owns data,
partitions, native solvers, recorded states, metrics and the experiment
record. The egui application becomes one consumer of it during the
transition; the new shell is the other.

Numerical debts recorded in the review that land here, each with an
independent fixture: a reserved test split in the native path; one
`Partition` passed into tree fits (no second implementation); per-tree
RNG streams seeded from (seed, tree) so bootstrap draws do not change when
depth changes; a data-driven penalty grid starting from the smallest alpha
that zeroes every coefficient; one definition of "training median";
quasi-separation flagged in logistic fits; a deterministic sklearn
comparison for depth-1 and depth-2 trees.

### 2. Job supervisor (Rust)

One supervisor replaces `python.rs` and `adapter.rs`. It runs every
backend process, including student code from the practice editor, with:

- a process group (Unix) or Job Object (Windows) so cancel and exit kill
  the whole tree synchronously; nothing is orphaned on quit or crash;
- bounded asynchronous pipe readers (no `join()` that can block forever);
- a versioned, nonce-tagged NDJSON progress protocol on stdout
  (`{"v":3,"nonce":…,"kind":"progress",…}`); lines without the nonce are
  plain output, so student code cannot spoof events;
- per-job limits chosen by the job, not a global 300-second cap:
  wall-clock, CPU, memory (rlimits on Unix, Job Object limits on Windows),
  and optional network denial;
- checkpoint files inside the project folder, written atomically, with
  resume semantics carried over from today's study worker (reservations,
  conservative charging of interrupted work);
- resource sampling (CPU, RSS, GPU memory when available) at an interval
  the job declares, shown live;
- ETA computed from the observed per-unit times of the job's own
  declared units (replications, epochs, folds), reported as a range with
  the number of units observed, never as a bare number before at least
  three units have completed.

Backends speak this protocol: the existing scientific and study workers
(migrated, keeping their frozen-hash provenance for old projects), a
PyTorch backend for the neural lessons, and later R and Julia backends.

### 3. Environment layer (pixi) — decided

The app does not install languages. It ships `pixi.toml` and `pixi.lock`
files, one per environment it knows how to use (e.g. `scientific`,
`torch`, `r`, `julia`), and a bundled `pixi` binary. The native engine
and all native lessons work with nothing downloaded. When a lesson needs
an environment that is not present, the app shows what it would download
(packages, size, source) and asks. Environment state (present, partial,
verified against the lock) is visible in a settings view and in each
lesson's preflight. The scientific Python runtime that ships today inside
the Mac bundle is replaced by a pixi environment resolved at first use,
unless offline-first installs turn out to matter for students, in which
case the installer pre-seeds the `scientific` environment.

### 4. Content and shell (Tauri + web UI) — decided

The presentation layer is rebuilt as a Tauri application: the Rust core
(engine, supervisor, environment layer, learner record) runs in the Tauri
backend; the UI is web technology in the Tauri webview.

Designer's calls, open to revision:

- Mathematics: KaTeX, server-side rendered at lesson load so pages do not
  reflow, with a plain-text alternative in the DOM.
- Plots: Vega-Lite specifications produced by the Rust side from
  experiment data, rendered in the webview. Each plot carries its data as
  an accessible table (toggle), supports zoom/pan/export (SVG, PNG, CSV),
  and uses one palette and one tick algorithm. The three "linked
  selection" behaviours that exist today (select an observation, follow it
  across plots, see its exact numbers) are preserved through a shared
  selection store.
- Lessons as data: one directory per lesson containing `lesson.md`
  (Markdown with LaTeX and YAML front matter: id, title, prerequisites,
  level, estimated time), `experiment.json` (the experiment the lesson
  runs: data source, model family, settings, what the learner may change),
  `questions.json` (questions with full explanations, each tagged with the
  concept it tests), and optional `derivation.md` (steps the learner
  reveals one at a time). Lesson text can reference live quantities with
  a small template syntax (e.g. `{{coef.x1}}`, `{{metric.valid_mse}}`),
  which is how "the lesson's numbers change when the experiment changes"
  is achieved without code.
- Navigation: one route model (curriculum → lesson → panels), no
  integer-coded tabs. Three panels per lesson: Read (text, math,
  derivation), Run (experiment controls, plots, replay of recorded
  states, observation arithmetic), Check (predict-before-reveal, questions,
  the learner's record for this lesson).
- Learner record: a local SQLite database (one file per learner profile)
  holding attempts, predictions, answers, timings and mastery per concept.
  Exportable as JSON. Never uploaded.
- Theme: OS light/dark, reduced motion honoured, text scalable.
- Instructor mode: a toggle that reveals instructor notes, answer keys and
  the simulation truth, and that produces the instructor packet.

## The first lesson set

Drawn from Dr. Helfrich's actual tutoring (the boosting route taught on
2026-11-02 is the model). Each lesson is complete: text, math, experiment,
observation arithmetic, predict-before-reveal, questions with explanations,
diagnostics, failure case.

1. **One regression tree.** Four points. Where does the first split go
   and why (SSE reduction computed by hand). Depth, minimum leaf.
2. **Boosting by hand.** The four-point route: mean 6, residuals
   −4, −2, 2, 4, first stump, half the correction, MSE 10 → 3.25 → 1.5625.
   Predict the next move. Learning rate retrains the whole path.
3. **Boosting on real data.** Training versus validation curves, best
   stage computed not assumed, depth 1 versus 5, subsampling with hollow
   points for rows not used this round. Why validation is not a test.
4. **Random forests.** Bootstrap, feature subsampling, OOB error and its
   coverage, permutation importance with its repeat SD, PDP versus ICE
   versus a slice.
5. **OLS as projection.** QR steps as real nested projections, residual
   orthogonality, rank deficiency as something you see.
6. **Inference under stated assumptions.** Classical, HC0–HC3, cluster,
   HAC. The Engel data. What a confidence interval is not.
7. **Regularization.** Ridge, lasso, elastic net with the penalty path,
   KKT residuals as the convergence criterion, why penalized coefficients
   get no ordinary standard errors.
8. **Logistic regression.** Log odds, the threshold that changes decisions
   not probabilities, separation as an estimation failure.
9. **A simulation study.** Heteroskedasticity, then confounding: coverage
   with its denominator, Monte Carlo SE, the oracle that knows U, the
   student packet that does not.
10. **Prediction is not intervention.** The confounding simulation again,
    now as the bridge to the 0.9/0.10 roadmap (fixed effects, IV,
    cross-fitted causal estimators).

Later tracks (not in the first release): neural networks with real
forward values and gradients on PyTorch; kernels and margins; mixtures and
responsibilities; Bayesian computation with sampler diagnostics; panel and
causal estimators via PyFixest and DoubleML.

## Migration of kept assets

| Asset | From | To |
| --- | --- | --- |
| Linear solvers, CART, ensembles, science diagnostics | `src/*.rs` | engine crate, unchanged numerics, existing tests kept |
| 15 sklearn fixtures, Engel NumPy/R reference, simulation validation groups | verify scripts | engine crate CI |
| Observation arithmetic, replay of recorded states, linked selection, pinned comparison | `workbench.rs`, `ui.rs` | Run panel |
| Predict-the-next-move, `old + rate × h = new` | `ui.rs` | Lessons 2–3 |
| Scientific and study workers, checkpoint/resume, fit reservations | `python/*.py`, `adapter.rs` | supervisor backends (protocol 3), old projects still readable |
| Student/instructor/research exports | `study_worker.py` | export module, unchanged packet rules |
| 100-question bank | `questions.json` | per-lesson question files, explanations rewritten to full length, distractors replaced |
| Three linear lessons' prose | `teaching.rs` | lessons 5–8, rewritten with rendered math |

## Left to the builder

- Front-end framework inside the webview (any; keep state in Rust).
- Exact Vega-Lite theme, type scale and layout.
- The SQLite schema for the learner record.
- The pixi environment split and how the pixi binary is bundled and
  updated.
- Signing: ad-hoc remains until a Developer ID is approved; the design
  must not depend on notarization but must be ready for it.
- Order of implementation inside Phase 1, except that the engine crate
  extraction and the supervisor come before any lesson, and lesson 2
  (boosting by hand) is the first lesson built, because it is the one
  already taught and the one the first users will see.

## What "done" looks like for Phase 1

A student on a fresh Mac or Windows laptop opens Model Lab, works through
lessons 1–4 with nothing downloaded, commits predictions, answers questions
with explanations that use the live numbers, and comes back a week later to
find their record. An instructor opens the same build in instructor mode
and exports a student packet for lesson 3. A long boosting job on a
50,000-row CSV shows progress, an ETA range, CPU and memory, survives the
app being quit and resumed, and never leaves a process behind. Every plot
has a data table and every equation has a text alternative. `cargo test`
and the independent fixtures pass on both platforms in CI, and the shipped
binaries contain no harness and no developer paths.
