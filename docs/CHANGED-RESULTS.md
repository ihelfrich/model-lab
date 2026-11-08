# Changed results

Numerical changes that alter what a saved experiment or a fixture reports.
Each entry names the versions, the methods affected and unaffected, the
regenerated fixture and the notice shown for older saved work. Nothing is
listed here that was a correctness fix to the arithmetic of a method: those
would be bugs, recorded in the commit history. These are deliberate changes
of convention.

## 2026-11-08 — engine 0.9.0, saved-experiment schema 2

**What changed.** Tree ensembles are now fitted one stage at a time by
`model_lab_engine::ensemble::EnsembleState`, and tree *t* draws its
bootstrap sample (random forest), its row subsample (stochastic boosting)
and its split-candidate features from its own random stream,
`StdRng::seed_from_u64(splitmix64(seed) ^ (t + 1))`. Model Lab 0.8 drew
all of these from one stream seeded with `seed + 1`, so changing the depth
of the trees changed every later tree's bootstrap sample, and a run could
not be resumed from a checkpoint without replaying it from the start.

**Why.** Resumable jobs need tree *t* to be reproducible from (seed, t)
alone, and a learner comparing depth 2 with depth 5 should be comparing
trees grown on the same samples.

**Affected.** Random forest and stochastic boosting: samples, trees, OOB
error, importance and predictions differ from 0.8 for the same seed.

**Unaffected.** Single trees and full-data boosting (the sample is the
whole training set; split-candidate order does not matter when every
feature is a candidate), all linear models, the train/validation split
(still `StdRng::seed_from_u64(seed)`), and every metric definition.
`crates/engine/tests/fixtures/ensembles-v1-stable.json`, recorded from the
0.8 code, is reproduced exactly by 0.9.

**Fixture regenerated.** `crates/engine/tests/fixtures/ensembles-v2.json`
(forest and stochastic cases, five scenarios, depths 2 and 4), written by
the ignored test `write_fixtures` with `FIXTURE=v2`.

**Saved experiments.** `TRACE_SCHEMA` is 2. Opening a schema-1 file shows:

> This experiment was saved by Model Lab 0.8. Random forests and stochastic
> boosting draw their samples differently since 0.9 (see
> CHANGED-RESULTS.md); it opens read-only and is not refitted.

In the egui application this notice appears where load errors are shown;
read-only display of the saved trace is part of the new shell.
