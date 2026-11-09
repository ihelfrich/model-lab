---
id: one-tree
title: One regression tree
level: Beginner
prerequisites: []
estimated_minutes: 25
concepts: [sse, split-rule, leaf-mean, depth, min-leaf]
status: draft-for-review
---
# One regression tree

A regression tree is a set of yes-or-no questions about the predictors,
ending in a number. Everything else about trees, boosting and forests
included, is built from this one object, so it is worth seeing exactly how
a single tree decides where to cut. Four points will do:

| x | 1 | 2 | 3 | 4 |
|---|---|---|---|---|
| y | 2 | 4 | 8 | 10 |

## What a leaf predicts

Suppose the tree asks no questions at all. Then it has one leaf containing
all four points, and the number it should predict is the one that makes the
squared errors smallest: the mean, $\bar y = {{byhand.mean}}$. The sum of
squared errors around that mean,

$$\mathrm{SSE} = \sum_{i=1}^{4}(y_i - \bar y)^2 = {{byhand.sse}},$$

is the quantity a split will try to reduce. Divide by four and you have the
mean squared error, {{byhand.mse}}; the two are the same measure in
different units.

## Where could a cut go?

A cut on $x$ is a threshold: points with $x$ at or below it go left, the
rest go right. Between four distinct values of $x$ there are exactly three
places a cut can make a difference, and the engine puts each threshold
halfway between neighbouring points:

| threshold | left points | right points | SSE after | reduction |
|---|---|---|---|---|
| {{byhand.candidates.0.threshold}} | {{byhand.candidates.0.left_n}} | {{byhand.candidates.0.right_n}} | {{byhand.candidates.0.sse_after:.3f}} | {{byhand.candidates.0.reduction:.3f}} |
| {{byhand.candidates.1.threshold}} | {{byhand.candidates.1.left_n}} | {{byhand.candidates.1.right_n}} | {{byhand.candidates.1.sse_after:.3f}} | {{byhand.candidates.1.reduction:.3f}} |
| {{byhand.candidates.2.threshold}} | {{byhand.candidates.2.left_n}} | {{byhand.candidates.2.right_n}} | {{byhand.candidates.2.sse_after:.3f}} | {{byhand.candidates.2.reduction:.3f}} |

"SSE after" is the sum of the two groups' squared errors around their own
means. The reduction is the parent's {{byhand.sse}} minus that. The tree
picks the cut with the largest reduction: $x \le {{byhand.round.0.threshold}}$,
which separates {2, 4} from {8, 10} and takes the SSE from {{byhand.sse}}
down to {{byhand.candidates.1.sse_after}}. The two leaves predict their own
means, {{byhand.round.0.left_mean}} and {{byhand.round.0.right_mean}}.

Note what the tree did not do. It did not fit a line. It did not assume
anything about how $y$ changes with $x$ between the points. It found the
one place where splitting the data into two groups bought the most.

## Why 2.5 and not 2

Any threshold strictly between 2 and 3 produces the same two groups, so the
data cannot prefer one such threshold over another. The engine reports the
midpoint. When you later see a tree split at an odd-looking value such as
37.5, that is all it means: halfway between two neighbouring observed
values.

## Going deeper, and the failure it brings

Set the depth to 2. Each leaf is now allowed to split again. On the left,
{2, 4} splits into two leaves of one point each; on the right, {8, 10} does
the same. Every point is its own leaf, every leaf predicts its own outcome,
and the training SSE is zero. The tree has stopped describing a pattern and
started memorising a table.

This is the central tension of tree models. Depth buys fit on the data you
have and costs you the ability to say anything general. With four points
there is no way to tell a good split from a lucky one; on real data the
validation set does that job, and the next lessons lean on it.

## The brake: minimum leaf size

A minimum leaf size refuses any cut that would leave fewer than that many
points on either side. Set it to 2 and the outer cuts (1.5 and 3.5) become
inadmissible, because each would strand a single point; only the middle cut
remains. The table above marks which candidates survive your current
setting. It is a blunt tool, and that is its virtue: it does not care how
tempting a cut looks, only how little evidence stands behind it.
