# Model Lab 0.7 implementation contract

## Execution and provenance

Protocol version 1 uses one JSON request and one atomic JSON response per job.
Every response identifies the request, request SHA-256, worker-source SHA-256,
software versions, exact observation identities, configuration and warnings.
Progress is a separate JSON-lines channel. Unsupported versions, nonfinite
results, invalid roles, stale responses and incompatible capabilities fail.
The native UI remains responsive and can cancel the process. No shell evaluates
request content. Reproduction uses the same worker source and saved request.

## Inference

Inference is available for full-rank, unpenalized OLS and binary logistic models.
It refers to the displayed estimation sample, not automatically to the population
or a causal estimand. The intercept is included in the parameter count. OLS
supports classical, HC0–HC3, one-way cluster CRV1, and Bartlett HAC covariance.
OLS uses Student t with n-k degrees of freedom, or G-1 with cluster correction.
Cluster covariance uses G/(G-1)*(n-1)/(n-k). HAC uses n/(n-k), with explicitly
specified lag order and chronological ordering. Logistic supports model-based,
HC0 and one-way cluster covariance, using asymptotic normal inference.
Rank deficiency, separation, failed convergence and singular covariance are
reported rather than given ordinary inferential tables. Classical standard
errors are never attached to penalized fits. Continuous-variable average
marginal effects use the delta method; categorical effects are identified
separately and must not be presented as continuous derivatives.

## Data and resampling

Missing outcomes fail unless explicit omission is enabled and recorded. Numeric
median imputation, categorical mode imputation, encoding, polynomial expansion
and scaling are fitted independently within each training fold. Encoded
categorical variables have a training-defined reference level. Groups and time
are explicit roles; they are excluded from predictors unless explicitly selected.
All sample and fold records retain original zero-based row identities.

Resampling supports repeated random/stratified K-fold, disjoint group folds and
expanding temporal folds. Temporal partitions keep equal timestamps together;
gap counts distinct time blocks. A final test partition is reserved before
tuning. Nested tuning chooses settings using inner training folds only; outer
losses evaluate that selection procedure. Fold variation is descriptive and
must not be labeled a confidence interval. Final test evaluation is an explicit
operation and its disclosure is retained in project history.

## Research and teaching artifacts

A project owns the dataset, variable roles, requests, completed results,
comparison history and learning state. Export includes the request, response,
exact worker, reproduction entry point, data/sample records, tables and vector
figures. Reopening validates schema and identity. Reproduction verifies numerical
outputs with declared tolerances and reports version drift. Teaching uses actual
sample/fold/inference values; unavailable backend optimizer states stay unavailable.

## Release gates

Analytic and independent covariance checks; interval/coverage simulations with
Monte Carlo uncertainty and a deliberately misnormalized baseline; fold leakage,
group/time-boundary and held-out-label mutation checks; cancellation and malformed
protocol checks; project replay/export execution; native research/teaching
interactions; preserved earlier archives; packaged Mac and Windows scientific
runtime tests. Mac AccessKit remains disabled pending a verified native fix.

## Release bounds
Research CSV input is capped at 32 MiB and 50,000 rows; animated native fits
retain 5,000 rows. Research uses at most 64 raw and 512 encoded predictors,
1,000 model fits and a 300-second worker deadline. Median/mode imputation
recognizes blank/whitespace cells and explicitly declared exact missing tokens.
Formula grammar accepts an intercept, selected column names (backtick quoted
when necessary), C(name), squares and degree-two numeric interactions. It does
not evaluate Python expressions. Centered/scaled inference is transformed back
to original encoded units; HAC lags count ordered observations.
Mac builds target Apple silicon and use ad-hoc signing, without notarization.
Windows acceptance targets x86-64 on Windows Server 2022. Neither substitutes
for acceptance on a particular student's computer.
