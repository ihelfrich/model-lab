---
id: random-forests
title: Random forests
level: Beginner
prerequisites: [one-tree, boosting-on-data]
estimated_minutes: 45
concepts: [bootstrap, feature-subsampling, oob-error, oob-coverage, permutation-importance, pdp-ice-slice]
status: draft-for-review
---
# Random forests

Boosting builds trees in sequence, each correcting the last. A random
forest builds them in parallel, each on its own view of the data, and
averages. The two methods share the tree and nothing else, and the
differences are the lesson. The data are {{data.n}} synthetic community
rows with eight predictors (mechanism in `examples/generate.py`); the
outcome is a violent-crime rate per population.

## The bootstrap

Each tree in the forest is fitted to a bootstrap sample: {{rows.train}}
draws with replacement from the {{rows.train}} training rows. Some rows are
drawn twice or three times; about a third are not drawn at all. Those
left-out rows are "out of bag" for that tree, and they are the forest's
built-in validation set, one per tree.

## Feature subsampling

At every split, each tree is offered only `mtry` = {{config.mtry}} of the
eight predictors, chosen at random. This sounds like a handicap, and for a
single tree it is. For a forest it is the point: if every tree could see
every predictor, they would all split first on the strongest one and would
agree with each other too much. Averaging trees that agree gains little.
Slide `mtry` from 1 to 8 and watch the out-of-bag curve.

## Out-of-bag error, with its denominator

For each training row, take only the trees that did not see it, average
their votes, and compare with the truth. Across all rows that gives the
out-of-bag mean squared error, {{oob.mse:.3f}} at the current stage. The
plot carries a second line, the coverage: the share of the
{{oob.rows_train}} training rows that have at least one out-of-bag tree.
At the current stage it is {{oob.coverage:.2f}}. After one tree, only about
a third of the rows have any out-of-bag vote, and the error is averaged
over those few; as trees accumulate the coverage climbs toward one and the
out-of-bag error settles. An out-of-bag error quoted without its coverage
is an average whose denominator you do not know.

The separate validation set of {{rows.valid}} rows is still there, and
its curve is drawn too. On this data the two estimates should tell the same
story. When they do not, the out-of-bag estimate is the one computed from
more rows.

## Permutation importance and its spread

How much does the forest lean on each predictor? Shuffle one predictor's
column among the validation rows, re-predict, and see how much the error
rises. Do it three times, because one shuffle is one draw. The importance
plot shows the mean increase for each predictor with a range of plus or
minus one standard deviation over the three repeats; for the predictor the
generating mechanism makes strongest, housing vacancy,
the increase is {{importance.pct_house_vacant.mean:.3f}} ±
{{importance.pct_house_vacant.sd:.3f}}. A bar whose range reaches zero is
a predictor the forest may not need.

## PDP, ICE and a slice are three different pictures

The last plot draws three things for one predictor. Each thin line is an
individual conditional expectation: one reference row, with that predictor
swept across its range and everything else held at that row's own values.
The thick line is their mean, the partial dependence. The dashed line is a
slice: the same sweep with every other predictor fixed at its training
median. A slice is a single row's view, a typical row that may not exist;
a PDP averages over real rows. They can disagree, and when the ICE lines
fan out they will. The engine labels them separately so you never read one
as the other.

## The failure case: importance as a causal ranking

The importance plot ranks predictors by how much the forest's predictions
depend on them. It does not rank causes. Housing vacancy may rank high
because vacancy raises the rate, or because vacancy travels with something
that does, or because the forest found it a convenient proxy. Permutation
importance is a statement about this model on this data. The sentence "the
most important cause of crime is vacancy" is not available from this plot,
or from any plot in this lesson, and the lessons on identification later in
the course are about why.
