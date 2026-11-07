# Model Lab 0.8 implementation and acceptance contract

## Implemented scope

Extend the native Rust application and its existing local Python boundary with
three versioned generating mechanisms, repeated-sampling studies, failure-aware
Monte Carlo summaries, verified replication inspection, and research/instructor/
student exports. Existing nine-model ordinary analysis remains available.
No general equation editor, new estimator family, cloud service, or UI rewrite.
The later fixed-effects/IV, columnar-data and causal-ML work remains a roadmap.

The user subsequently stated that only this Mac is available and that participants
and a physical Windows desktop cannot be supplied. Automated numerical checks,
the public Engel workflow, Mac native inspection and Windows CI are executable
acceptance evidence. Participant usability, learning effectiveness, physical
Windows acceptance and full accessibility remain unverified. Do not invent
sessions, task completion rates, consent receipts or learning gains.

## Mathematical correspondence

Primitive X, U, nu and epsilon are independent standard normals. Cluster effects
B_g are independent standard normals shared only within their group. Variables
use dimensionless synthetic units; generated outcomes are not claimed to have
unit variance.

| Template | Implementation | Known target |
| --- | --- | --- |
| Heteroskedasticity | `Y=1+beta*X+sigma*sqrt(1+h*X^2)*epsilon` | beta, or intercept 1 |
| Confounding | `D=X+c*U+nu; Y=tau*D+X+gamma*U+sigma*epsilon` | structural tau; U is simulator-only oracle information |
| Clustering | `Y=1+beta*X+sigma*(sqrt(rho)*B_group+sqrt(1-rho)*epsilon)` | beta, or intercept 1 |

At h=0, conditional errors are homoskedastic. At rho=0, the shared error vanishes.
Setting c=0 or gamma=0 removes the confounding path conditional on X. Robust
covariance does not remove confounding bias. Independent regressors mean that
group dependence need not damage slope coverage as strongly as intercept
coverage; the interface exposes both targets instead of promising a universal
ranking. Prediction MSE uses independent generated evaluation observations.
For grouped data, existing-group and new-group evaluations are separate targets.

Each replication is addressed by SHA-256(seed, replication, stream), feeding
NumPy SeedSequence/PCG64. Methods share exactly the same training dataset and
evaluation stream. Method inclusion and resume order cannot change those data.

## Protocol and project ownership

Protocol 1 and `python/research_worker.py` are frozen at the verified 0.7 source
hash. Protocol 2 studies use `python/study_worker.py` and declare a bundle hash
covering both entry point and frozen dependency. Requests bind an exact
specification wire, request identity and source bundle. Old projects preserve
history, request/result hashes and test-disclosure records when read by 0.8.

Typed records: ScenarioSpec, StudySpec, MethodSpec, ReplicateResult,
FitReservation, StudySummary and StudyRef. A project version-2 JSON manifest
references canonical checkpoint files in its associated `<project>.studies`
folder. Keep these together. Save As copies verified study artifacts and changes
relative references without modifying the original project. Paths cannot escape
the selected project folder through traversal or symlinks. Project opening only
parses data; it never imports embedded source.

Completed records are shared by immutable references in native views. Editing
a draft does not alter a stored study. Saved learner responses are separate from
the frozen scientific specification. A recorded preview remains identified when
the draft changes, rather than masquerading as an updated sample.

## Resource and failure accounting

Defaults: n=200, 200 replications, seed 471008, two inference variants. Bounds:
50-2,000 observations; 5-50 groups with 10-40 observations/group; 2-500
replications; one to four methods. Preflight includes both prediction-backend and
inferential-backend fits: two per OLS inference, one per predictive fit. More than
1,000 backend fits is rejected before execution. Each worker invocation has a
maximum 300-second deadline. Existing 32 MiB request, 50,000 research-row,
5,000 native-row and encoded predictor limits remain.

An OS file lock prevents concurrent workers from owning the same checkpoint.
An atomic, flushed checkpoint reserves fit slots before numerical execution.
After abrupt interruption, outstanding reservations are charged conservatively.
Resume requires identical specification, sources, Python and package versions;
completed and failed outcomes are not silently replaced. Inspection consumes an
additional recorded reservation. Cancellation preserves committed records.

Failures remain in method-specific attempted counts. Coverage among valid
intervals and the rate of producing a valid, covering interval have separate
denominators. Paired comparisons use jointly successful replications and state
when an oracle has additional information. Monte Carlo SE is shown separately
from coefficient SE. Empirical-SD uncertainty uses a labelled fourth-moment delta
approximation. Boundary plug-in MC SEs of zero do not imply certainty.

## Exports and teaching

Student packets contain observed data and worksheets; no hidden U, true
coefficients, seed, generating recipe or instructor key. This is content
separation, not examination security. Instructor packets contain a separate
student subpacket. Research packets include the exact specification, checkpoint,
worker sources, version pins, CSV summaries, PDF/SVG figures, LaTeX report source
and executable reproduction. Reproduction checks source/specification hashes,
observation fingerprints, recorded outcomes and numerical tolerance, and reports
version drift. Fingerprints detect inconsistency, not authorship or authenticity.

## Verification

Preserve previous tests. New tests check independent OLS/HC3 formulas, exact
generating streams and zero limits, outcome scaling, cluster-intercept coverage,
nonconvergence/failure denominators, preflight/deadlines, interruption/resume,
source/specification integrity, training-only transforms, prediction information
sets and export privacy. Sensitivity studies use n=80/320, R=80 and two seeds;
deliberately shifted coefficients and misnormalized intervals must fail.

The Engel workflow compares linear/quadratic HC3 results with independent
NumPy and base-R QR/matrix calculations, then executes native project restore
and the exported reproduction. These are reference estimates, not truth.
CPU layout timing is not end-to-end input latency. Native workflow execution,
screenshot capture, manual controls and accessibility have separate receipts.
Capture timeouts are bounded and explicitly partial. Mac AccessKit remains off
until a verified repair; full VoiceOver support is not claimed.
