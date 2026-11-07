# Linear-model numerical contracts

The native teaching engine exposes actual solver states. It does not animate an
invented optimizer path. Model support and visualization capabilities are declared
separately. Existing v0.5 delivery archives remain immutable.

## Data and preprocessing

Rows retain stable IDs through a deterministic training/validation partition.
All predictor means and population standard deviations are learned from training
rows only. Centering supports an unpenalized intercept. A constant predictor has
scale one and coefficient zero. Report coefficients in original units and retain
standardized coefficients for penalty and path interpretation. Never use validation
labels or predictor moments in fitting. Logistic responses must be exactly 0/1;
both classes must occur in training.

## Objectives

Let z be the centered, optionally standardized predictor matrix, n the number of
training rows, b0 the unpenalized intercept, and b the coefficients in z units.

- OLS: minimize sum((y-b0-zb)^2)/(2n).
- Ridge: add alpha*sum(b^2)/2.
- Lasso: add alpha*sum(abs(b)).
- Elastic net: add alpha*mix*sum(abs(b)) + alpha*(1-mix)*sum(b^2)/2.
- Logistic: minimize mean(softplus(eta)-y*eta), eta=b0+zb, without a penalty.

alpha is nonnegative; mix is in [0,1]. Elastic net with mix=0 equals ridge;
mix=1 equals lasso. With alpha=0 the squared-error problems reduce to OLS,
although rank-deficient coefficient solutions need not be unique. sklearn Ridge
uses RSS + alpha_library*||b||^2; the matching alpha_library is n*alpha.
sklearn Lasso/ElasticNet use the same normalized objective as this engine.

## Solvers and recorded states

OLS uses column-pivoted, twice-orthogonalized QR on centered predictors. Each
recorded basis step is an actual nested projection, not a gradient-descent step.
Rank-deficient designs return a basic least-squares solution with a nonuniqueness
warning. No covariance or standard-error claim is implied by a point estimate.

Ridge/lasso/elastic net use cyclic coordinate descent with soft thresholding,
exact coordinate minimizers and explicit KKT residuals. Convergence is assessed
against a scale-aware stationarity threshold, never just coefficient movement.
Iteration limits remain visible. The trace records actual completed sweeps.

Logistic uses damped Newton steps with backtracking on the original objective.
Numerical Hessian stabilization affects the search direction, not the objective.
Stable sigmoid/softplus handle extreme linear predictors. A strict separating
direction is reported as complete separation; no finite unpenalized MLE is claimed.

## Verification

Use analytic orthogonal-design solutions, residual orthogonality, KKT checks,
finite-difference logistic scores, elastic-net endpoint equivalence, constant and
collinear predictors, training-only preprocessing, validation-label independence,
permutation invariance, replay, and independent NumPy/scikit-learn comparisons.
Record versions, objective conventions and tolerances with each reference run.
