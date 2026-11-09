---
id: boosting-by-hand
title: Boosting by hand
level: Beginner
prerequisites: [one-tree]
estimated_minutes: 35
concepts: [residual, stump, learning-rate, additive-model, training-loss]
status: draft-for-review
---
# Boosting by hand

Four points are enough to see the whole mechanism. Here they are:

| x | 1 | 2 | 3 | 4 |
|---|---|---|---|---|
| y | 2 | 4 | 8 | 10 |

We want a rule that turns $x$ into a guess for $y$. Boosting builds that rule
one small step at a time, and every step is arithmetic you can check.

## Start with the dullest possible model

Before looking at $x$ at all, the best single number to guess for every
point is the mean of the outcomes, $\bar y = {{byhand.mean}}$. The engine
calls this stage 0. Its training loss, the mean of the squared errors, is

$$\frac{1}{4}\sum_{i=1}^{4}(y_i - \bar y)^2 = {{byhand.round.0.mse_before:.4f}}.$$

That number is the thing we are going to drive down.

## What is left over

Subtract the guess from each outcome and you get the residuals: what the
current model has not explained.

| x | 1 | 2 | 3 | 4 |
|---|---|---|---|---|
| residual | −4 | −2 | 2 | 4 |

The residuals are the new target. Boosting never fits a tree to $y$ again
after stage 0; from here on, every tree is fitted to whatever is left over.

## The first stump

A stump is a tree with one split. Fitted to the residuals, it asks: where
should one cut on $x$ go so that the two groups have as little spread as
possible? The engine tries every cut between neighbouring points and picks
$x \le {{byhand.round.0.threshold}}$. The left leaf holds the residuals −4
and −2 and predicts their mean, {{byhand.round.0.left_mean}}; the right leaf
holds 2 and 4 and predicts {{byhand.round.0.right_mean}}.

That stump, by itself, is not a model of $y$. It is a correction to the
model we already have.

## Half the correction

Here is the step that makes boosting boosting. We do not add the whole
correction. We add a fraction of it, the learning rate $\nu$, currently
{{byhand.rate}}. For the point $x = 2$:

$$F_1(2) = F_0(2) + \nu\, h_1(2) = {{byhand.mean}} + {{byhand.rate}} \times ({{byhand.round.0.left_mean}}) = {{byhand.round.0.after.1}}.$$

Do the same for every point and the training loss falls from
{{byhand.round.0.mse_before:.4f}} to {{byhand.round.0.mse_after:.4f}}.
Take the new residuals, fit a second stump to them, add half of it again,
and the loss falls to {{byhand.round.1.mse_after:.4f}}. The path so far is
{{byhand.round.0.mse_before:.4f}} → {{byhand.round.0.mse_after:.4f}} →
{{byhand.round.1.mse_after:.4f}}.

Notice what each round does. It never revisits an earlier tree. The model
after $m$ rounds is a sum,

$$F_m(x) = \bar y + \nu \sum_{k=1}^{m} h_k(x),$$

and each new term is chosen to explain what the previous sum missed. That is
why the method is called additive: the trees are added, never replaced.

## Why the rate retrains everything

Move the learning-rate slider in the Run panel and watch. Not only does the
first update change; every residual after it changes, so every later stump
is fitted to different numbers and may split in a different place. The
learning rate is not a dial on the finished model. It is a dial on the path
the model took to get there. A smaller rate takes smaller steps and needs
more of them; it also tends to leave a smoother path of losses, which is the
reason people pair a small rate with many trees on real data.

## A failure case worth seeing

Set the rate to 1.0. The first stump is added whole, and on four points a
few rounds are enough to drive the training loss to zero. Every point is
predicted exactly. This is not a triumph. It tells you the model has
memorised four numbers; it says nothing about a fifth point you have not
seen. On real data this is where the validation set earns its keep, and
that is the subject of the next lesson. Here, with four points and no
validation set, the only honest reading of a zero training loss is: we have
run out of things to learn from these four points.
