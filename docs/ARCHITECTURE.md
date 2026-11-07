# Computational teaching workbench

## Scope of this implementation

Extend the existing native Rust/egui application with OLS, logistic regression,
ridge, lasso and elastic net, while preserving its tree/ensemble laboratory.
The release must connect fitted predictions, numerical states, code, diagnostic
plots and original teaching content. Broad future adapters are an extension
contract, not a claim that unimplemented model families already work.

## Boundaries

1. **Data and partition:** named observations, predictor/response roles, task,
   deterministic row partition and training-only transforms.
2. **Model registry:** model identity, task and capabilities; family-specific
   configuration validated before fitting.
3. **Numerical engine:** linear solvers and existing tree implementation return
   model-owned fitted state and actual snapshots. They do not depend on egui.
4. **Experiment:** one fitted run joins data identity, partition, preprocessing,
   configuration, metrics, trace and prediction methods. A comparison must use
   the same observations and split.
5. **Visual grammar:** reusable plot axes, model curves, residuals, coefficient
   paths, objective trajectories and two-parameter objective contours.
6. **Teaching:** model-specific objectives, mechanism, worked arithmetic,
   questions and explanations grounded in the displayed experiment.
7. **Execution/export:** generated Python states the exact objective/scaling,
   and saved experiments support reproducible replay without private source data.

## Interaction design

Use a restrained scientific workspace with clear hierarchy, legible typography,
semantic color and progressive disclosure. A persistent model/data bar anchors
the experiment. The main canvas leads; controls explain their statistical meaning.
Visible task labels distinguish regression and binary classification. Changing a
decision threshold changes classification decisions without refitting probabilities.

Separate Explore, Coefficients, Optimization, Learn, Code and Data views, retaining
the current tree laboratory as a complete specialist workspace. Linked observation
selection connects plots with exact numerical predictions. Report slices as slices,
partial dependence as an average over reference rows, and optimization states as
actual recorded steps. Preserve keyboard operation and reduced-motion use.

## Release gates

Numerical contracts and reference checks; regression tests for existing ensembles;
native view and control verification at normal and compact widths; deterministic
saved-run replay; Python export execution; Mac packaging/signature checks and
Windows CI. Keep the student-delivered v0.5 ZIP and application unchanged.
