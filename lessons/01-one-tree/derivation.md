**Step 1: the parent leaf.** With no split, the leaf predicts the mean
$\bar y = {{byhand.mean}}$ and its sum of squared errors is
$$\mathrm{SSE}_0 = (2-6)^2 + (4-6)^2 + (8-6)^2 + (10-6)^2 = {{byhand.sse}}.$$
---
**Step 2: a candidate cut.** For the threshold $x \le 2.5$ the left group
{2, 4} has mean {{byhand.candidates.1.left_mean}} and SSE 2; the right
group {8, 10} has mean {{byhand.candidates.1.right_mean}} and SSE 2. Their
sum is {{byhand.candidates.1.sse_after}}.
---
**Step 3: the reduction.** The cut is scored by how much SSE it removes:
$${{byhand.sse}} - {{byhand.candidates.1.sse_after}} = {{byhand.candidates.1.reduction}}.$$
The other two cuts remove only {{byhand.candidates.0.reduction:.3f}} each,
so the middle cut is chosen.
---
**Step 4: the prediction rule.** The fitted tree is
$$\hat f(x) = \begin{cases} {{byhand.round.0.left_mean}} & x \le {{byhand.round.0.threshold}} \\ {{byhand.round.0.right_mean}} & x > {{byhand.round.0.threshold}} \end{cases}$$
written here as the leaf means of the residuals around $\bar y$; add
$\bar y = {{byhand.mean}}$ to each to read them as predictions of $y$: 3 and 9.
