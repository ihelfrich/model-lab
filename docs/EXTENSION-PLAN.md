# Extension of the teaching workbench

## Product scope

The workbench should support many model families through a shared experiment
contract. No finite release can credibly promise every known method, and a model
that can run is not necessarily a model the application can teach. Track execution,
trace visibility, diagnostics, inference and lessons as separate capabilities.

The 0.6 foundation implements OLS, binary logistic regression, ridge, lasso and
elastic net alongside four existing tree methods. These are the first complete
examples of that contract, not evidence that later families already work.

## Model integration contract

Each addition must declare its outcome types, input representation, preprocessing,
objective and parameter conventions, numerical limits, prediction interface and
available explanations. A fitted experiment owns immutable data identity,
partition, transformations, configuration, diagnostics and recorded numerical
states. Views query those objects; they must not reconstruct a different model.

The next registry revision should replace presentation-specific family switches
with capability requests. Coefficients, probabilities, intervals, local
contributions, graph structure and optimization states are optional capabilities.
A method without a coefficient vector should receive an appropriate specialist
view. A backend that exposes only a final fit must never fabricate an optimizer
trace. Retain a small native solver when its internal arithmetic serves a lesson;
use a tested backend adapter for broader execution support.

Keep the existing Rust process responsible for interaction and deterministic
experiment state. Extend the existing local Python execution boundary for
scientific adapters rather than introducing another launcher or service. An
adapter must report its package/version, objective conventions, transformed data
roles, completed results and failures through a versioned protocol. Cancellation,
resource limits and output validation belong at that boundary.

## Teaching and visualization

Every fully taught model needs a mathematical specification, a small worked
example, arithmetic linked to the displayed experiment, a manipulable mechanism,
relevant diagnostics, failure cases and questions with explanations. Record which
of these are actually implemented. Distinguish a prediction slice from marginal
averaging, prediction uncertainty from parameter uncertainty, and an observed
association from an identified effect.

Use a common visual grammar: observations retain identity across plots; training,
validation and test roles retain their encodings; linked selections expose exact
values. Give each family the geometry that explains its operation. Linear models
need projections, residuals and coefficient paths; kernels need distances and
margins; neural networks need forward values, gradients and parameter updates;
mixture models need responsibilities and parameter changes. Shared metrics alone
cannot substitute for those family-specific mechanisms.

## Econometric extension

Inference requires its own layer. Start with explicitly specified OLS covariance
estimators, confidence intervals and joint restrictions, then add appropriate
logistic inference and marginal effects. State the sampling assumptions and
separate classical, heteroskedasticity-robust and cluster-robust covariance rules.
Penalized coefficients do not inherit ordinary unpenalized standard errors.

Later modules can add fixed effects, instrumental variables, GMM, time-series
models, panel estimators and causal designs. They need estimand, identification,
sample construction, dependence and inference contracts in addition to numerical
solvers. The interface must distinguish an estimator that ran from assumptions
that are defensible for a particular application. A predictive explanation does
not establish a causal effect.

## Expansion order and acceptance

1. Complete the five-model release: numerical reference comparisons, actual
   Python reproduction, saved-run checks, native controls and both platforms.
2. Add a common resampling design: repeated splits, cross-validation and a final
   untouched test partition, with every transformation fitted within each fold.
3. Add inference and econometric diagnostics under explicit assumptions.
4. Add principal components, clustering, neighbors, kernels and additional GLMs,
   each with its own teaching representation and independent reference fixtures.
5. Add neural, Bayesian, temporal and causal modules only as complete vertical
   implementations with suitable diagnostics and family-specific lessons.

Before accepting a family, verify at least one analytic case, one independent
reference, an important failure case, cancellation, reproducibility, exported
code and its actual native interactions. Performance measurements must identify
data size, dimensions, backend and hardware. A clean build or an attractive
animation cannot establish numerical correctness, educational effectiveness or
universal model coverage.
