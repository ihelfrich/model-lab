## What to watch for

**"Down" on the first prediction.** Some students predict the last stage
will beat the best stage because "more trees is more learning". Ask them
to restate what the best stage is. The reveal shows the difference
{{metric.valid_mse_at_last_minus_best:.2f}} and that it can only be zero or
positive.

**Reporting the validation number.** When a student calls
{{metric.best_valid_mse:.2f}} the model's accuracy, ask how many settings
they tried before landing on it, and how many rows the number averages
over ({{rows.valid}}). The lesson's point is that a number chosen by
looking is not a number for reporting.

**Draft status.** Prose drafted by the implementing agent from the plan;
numbers are computed live. Awaiting Dr. Helfrich's review.
