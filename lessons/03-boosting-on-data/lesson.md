---
id: boosting-on-data
title: Boosting on real data
level: Beginner
prerequisites: [boosting-by-hand]
estimated_minutes: 45
concepts: [validation-curve, best-stage, overfitting, depth-capacity, subsampling, validation-is-not-test]
status: draft-for-review
---
# Boosting on real data

The four-point lesson showed the mechanism. This lesson runs the same
mechanism on {{data.n:,}} rows of a synthetic income table (age, survey
year, income; the generating mechanism is documented in
`examples/generate.py`) and asks the question four points could not: when
should we stop adding trees?

## Two curves, two samples

Before any tree is fitted the engine sets aside a random quarter of the
rows. The remaining {{rows.train:,}} rows are the training set, the only
rows any tree ever sees. The {{rows.valid:,}} held-out rows are the
validation set: the model is scored on them at every stage but never
fitted to them.

Run the experiment and look at the loss plot. Two curves, one per set, with
the number of rows each is averaged over printed in the legend. The
training curve falls at every stage; it has to, because each tree is chosen
to reduce exactly that number. The validation curve falls, flattens, and
eventually turns up. Where it turns is the interesting place.

## The best stage is computed, not assumed

The engine records the validation loss at every stage and reports the stage
where it is lowest: stage {{stage.best}}, with a validation MSE of
{{metric.best_valid_mse:.2f}}. By the last stage,
{{stage.last}}, the validation loss has moved by
{{metric.valid_mse_at_last_minus_best:.2f}} from that minimum. That
difference can only be zero or positive: the minimum is the minimum.

Nothing here was assumed in advance. Nobody decided that 150 trees was
right or wrong; the data, through the held-out rows, said where the extra
trees stopped helping. Use the "Go to best stage" button and read the
arithmetic panel for a selected observation at that stage; then step
forward and watch its residual.

## Depth is capacity

Each tree's depth is how many questions it may ask. Slide the depth from 1
to 5 and refit. At depth 1 every tree is a stump, each correction is a
single step in one predictor, and the model climbs slowly and smoothly. At
depth 5 each tree can carve the predictor space into up to 32 cells; the
training loss falls fast and the validation curve turns up early. More
capacity per tree means fewer trees before the model starts fitting the
noise in {{rows.train:,}} particular rows rather than the pattern behind them.

## Subsampling

Set the sample fraction below 1. Each round now fits its tree to a random
share of the training rows, and the method is called stochastic boosting. In
the scatter plot the rows not used by the current stage's tree are drawn
hollow: that tree never saw them, yet the correction it produced is applied
to them too, because a tree is a rule, and a rule applies everywhere. The
randomness costs a little on each tree and often buys a flatter validation
curve, for the same reason a bagged average is steadier than one estimate.

## The failure case: calling validation "test"

Here is the mistake this lesson exists to prevent. You slide the depth,
you slide the rate, you pick the stage with the lowest validation loss,
and you report that loss, {{metric.best_valid_mse:.2f}}, as the model's
performance on new data. It is not. You used the validation set to make
every one of those choices. The number is the best of many looks at the
same {{rows.valid:,}} rows, and the best of many looks is optimistic. A
test set is a set you look at once, after all the choosing is done, and
this lesson has none: the engine's native path keeps one validation set
and says so.

The validation curve answers "where should I stop?". It does not answer
"how well will this do next year?". Keeping those two questions apart is
most of what it means to use these methods honestly.
