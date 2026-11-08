TREE ENSEMBLE LAB 0.5

QUICK START
Mac (Apple Silicon): open dist/Tree Ensemble Lab 0.5.app or Open Tree Ensemble Lab.command.
Windows: extract the COMPLETE Windows ZIP and open Tree Ensemble Lab.exe. Keep
its Python directory beside the executable. The complete Windows package includes
Python and scikit-learn; native modeling and quizzes do not require installation.
Mac Python practice uses a local interpreter selected in CODE.

INTERFACE
The original instrument-style design uses a graphite shell, optical panels,
a side mode dock and actual-tree sequence keys. The OP-XY layout was a design
reference (https://teenage.engineering/guides/op-xy/layout). The Apple2040 direction
is speculative visual interpretation, not a claim about an Apple roadmap.
EXPLORE: staged model predictions, selected observation, residual and arithmetic.
LABS: guided experiments and pinned-model comparisons.
CODE: generated Python, editable practice, local execution, highlighted lines.
QUIZ: 100 original questions in 10 topics, including14 code questions and11
select-all items. Topic filtering, shuffling, question map and missed-item review.
SCIENCE: validation metrics, OOB error, permutation importance, PDP and ICE.
Details: tree mechanics, original flow, score plots and source data preview.
The right drawer has DATA / ENGINE / TUNE layers. Encoders are draggable and
numeric entry is available. Native tuning changes refit after a short pause.

PLAYBACK
Slow:2 seconds/tree; Normal:0.5; Fast:0.1; Turbo:0.02. Fast is the default.
No extra fixed delay is added between rounds. +10, Finish now, Best validation,
the slider and stage keys jump directly. Rendering may aggregate several rounds
into one frame at high speed; cumulative prediction changes are labeled as such.
Native stage predictions and plotted slices are cached. Presentation easing does
not change the model or its scores. Slow playback is useful for the three phases;
Fast/Turbo and Finish now are useful for comparing entire fits.

DATA
CSV: UTF-8 header,4–5,000 rows,2–65 distinct nonempty columns, <=16MB. Selected
columns must be finite numeric values with magnitude <=1e12. Quoted commas,
doubled quotes, embedded newlines, CRLF and BOM are supported. There is no silent
imputation, row dropping or categorical encoding. Only numeric regression is
implemented; classification concepts appear in the practice bank.
Built-ins: nonlinear, interaction and noisy synthetic signals, plus three synthetic
example tables (income 3000 rows; diamonds 1500 rows; communities 90 rows) written by
examples/generate.py from the seeds and distributions in that file. They imitate the
shapes of common course datasets so the lessons read naturally, but no row describes
a real person, stone or place and nothing in this repository is redistributed data.

ENGINE
CART uses greedy numeric SSE splits, constant mean leaves, depth and minimum leaf
constraints. Threshold rounding at adjacent floating values is guarded so it cannot
silently create an empty child. Full-data squared-error boosting fits residuals;
stochastic boosting samples training rows without replacement. Forests bootstrap
with replacement, redraw candidate predictors per split and average response trees.
Same seed preserves the native partition across model settings on the same data.

Validation early stopping is optional for boosting: patience0 disables it. A round
must improve the stopping reference by the configured relative amount to reset
patience. The full retained path includes patience rounds; Best validation selects
the actual lowest recorded score (possibly the mean baseline). Validation labels
can select the stopping count, but do not fit the tree splits or leaves.
Forest OOB predictions use only trees that omitted each training observation.
Missing early OOB predictions remain unavailable; coverage is reported. Scores
at different tree counts can involve different subsets while coverage grows.
Held-out MSE, RMSE, MAE and R-squared are available; R-squared is undefined for a
constant validation response. Repeated tuning is not an independent final test.

SCIENCE computes three-repeat permutation importance on <=200 held-out rows,
retains negative increases and reports repeat SD, not a confidence interval.
PDP averages over <=128 sampled training reference rows; up to12 ICE curves are
drawn. The Explore line is a training-median slice, a different object. Correlated
predictors and unsupported substituted combinations complicate interpretation.
Importance, PDP/ICE and community predictions do not establish causal effects.
Pinned comparisons check data and partition identity before reporting a matched
score difference. Results describe the displayed stages, not universal rankings.

PYTHON
Transparent code uses only the standard library; class-library code uses actual
scikit-learn estimators. Generated code reflects current settings and early stopping.
Copy to practice or open a .py file; Run executes only when clicked. Stage pauses,
Next Python round, Stop, stdout/errors and actual result plots are provided. There
is no artificial delay in ordinary Python runs. Top-level executed source lines
are traced; the transparent tree helper is inspectable. Python and Rust use different
RNGs/conventions; identical seed numbers do not imply fit parity.
Code runs with the normal account's file access, not an OS security sandbox. The
full parent environment is not inherited; no package installation occurs silently.
Temporary numeric snapshots are removed when the run is replaced or app exits.
Export code+data+helpers creates a fresh folder in the selected location.

PACKAGING AND CHECKS
cargo test --locked
cargo build --release --locked
python verify_question_bank.py
The CLI --self-test --report PATH verifies four native models,100 questions and
all four models in both Python backends. --ui-smoke-test --software-renderer
--report PATH --screenshot PATH exercises native views and captures a rendered PNG.
Windows CI builds a static-CRT executable, relocates a complete Python runtime,
removes the original interpreter from PATH, verifies the relocated package and
renders the native UI. Read exact receipts for the tested platform and versions.
This is bounded engineering verification, not a guarantee for every Windows PC.
No messages, client records, credentials or original course attachments are in
student packages. Source and private build provenance stay separate. Nothing has
been sent to a student automatically.
