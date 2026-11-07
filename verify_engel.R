# Independent base-R QR/HC3 reference, original data units, classical t reference.
args <- commandArgs(trailingOnly = TRUE)
d <- read.csv(args[1])
answer <- list()
for (degree in 1:2) {
  raw <- matrix(d$income, ncol = 1)
  if (degree == 2) raw <- cbind(raw, d$income^2)
  center <- colMeans(raw)
  scales <- sqrt(colMeans(sweep(raw, 2, center, "-")^2))
  Z <- cbind(1, sweep(sweep(raw, 2, center, "-"), 2, scales, "/"))
  fit <- lm.fit(Z, d$foodexp)
  B <- solve(crossprod(Z))
  leverage <- rowSums((Z %*% B) * Z)
  scores <- Z * as.vector(fit$residuals / (1 - leverage))
  covariance <- B %*% crossprod(scores) %*% B
  J <- diag(ncol(Z)); J[1, -1] <- -center / scales
  J[-1, -1] <- diag(1 / scales, nrow = length(scales))
  beta <- as.vector(J %*% fit$coefficients)
  errors <- sqrt(diag(J %*% covariance %*% t(J)))
  critical <- qt(.975, nrow(Z) - ncol(Z))
  answer[[degree]] <- data.frame(model = if (degree == 1) "linear" else "quadratic",
    term = c("Intercept", "income", if (degree == 2) "income^2"),
    estimate = beta, std_error = errors, lower = beta - critical * errors,
    upper = beta + critical * errors)
}
write.csv(do.call(rbind, answer), args[2], row.names = FALSE)
cat(R.version.string, "\n")
