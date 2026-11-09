**Step 1: the starting model.** With no predictor in play, the number that
minimises the sum of squared errors is the mean:
$$F_0(x) = \bar y = \frac{2 + 4 + 8 + 10}{4} = {{byhand.mean}}.$$
---
**Step 2: the residuals.** Each point's residual is what $F_0$ left
unexplained, $r_i = y_i - F_0(x_i)$: $-4,\ -2,\ 2,\ 4$. Their mean is zero,
which is always true after fitting a mean.
---
**Step 3: the best cut.** For every cut between neighbouring $x$ values,
compute the sum of squared errors of the two groups around their own means.
The reduction from the parent's SSE of {{byhand.sse}} is
{{byhand.candidates.0.reduction:.3f}} at $x \le 1.5$,
{{byhand.candidates.1.reduction:.3f}} at $x \le 2.5$ and
{{byhand.candidates.2.reduction:.3f}} at $x \le 3.5$. The middle cut wins.
---
**Step 4: the update rule.** The new model adds a fraction $\nu$ of the
stump's prediction:
$$F_1(x) = F_0(x) + \nu\, h_1(x).$$
With $\nu = {{byhand.rate}}$, the left points move to
{{byhand.round.0.after.0}} and the right points to {{byhand.round.0.after.2}}.
---
**Step 5: the loss after one round.** The new residuals are
$y_i - F_1(x_i)$, and their mean square is
$${{byhand.round.0.mse_after:.4f}},$$
down from {{byhand.round.0.mse_before:.4f}}. Repeating steps 2 to 5 with
$F_1$ in place of $F_0$ gives {{byhand.round.1.mse_after:.4f}} after two rounds.
