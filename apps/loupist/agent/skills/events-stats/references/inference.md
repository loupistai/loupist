# Inference and diagnostics

Adapted from K-Dense Scientific Agent Skills, statistical-analysis and its
selection/diagnostics references (MIT). See [upstream provenance](../../NOTICE.md).

## Select for the design

Specify the estimand, independent sampling unit, pairing/clustering, and
comparison family before selecting a test. Independent mean comparisons can
use a prespecified Welch design; paired comparisons need aligned differences.
Repeated, clustered, or serial observations need an appropriate dependence model.
Rank-based tests answer rank/distribution questions and do not automatically
test medians. Choose correlation by the relationship being estimated.

Inspect raw observations, missingness, tails, outliers, and model residuals as
appropriate. A non-significant normality or variance screen does not verify an
assumption. A threshold diagnostic alone should not trigger a test-shopping
loop. Retain justified sensitivity analyses and mark post-hoc exploration.

## Event studies and market data

Distinguish announcement, effective, and ingestion timestamps. Map after-close
events to the next relevant session and disclose overlaps or other material news.
Separate normal-return estimation from the event window.

Pair asset and benchmark sessions before subtracting returns. State whether a
reported abnormal-return total sums daily abnormal returns or compounds wealth;
a difference of compounded returns is another quantity. Verify session-level
values against the aggregate. A fitted association is not a causal event effect.

For regression, inspect controls, degrees of freedom, heteroskedasticity, and
residual dependence. Heteroskedasticity-only robust errors do not repair serial
correlation, clustering, confounding, or a misspecified mean model. Choose robust,
clustered, HAC, or bootstrap uncertainty for the actual sampling design.

## Multiplicity and interpretation

Name all attempted tests, events, windows, or factors in the relevant family.
Use an appropriate correction or held-out design; retain non-significant tests.
For Bonferroni, cap adjusted p at one. State the threshold and strict/inclusive
comparison and compare unrounded values, including equality cases.

Report effect sizes and intervals with p-values. Non-rejection is not proof
of no effect, and significance is not practical importance. Bayesian probability
claims need an explicit likelihood, prior, and sensitivity to the prior; posterior
probabilities are not automatically Bayes factors. Preserve random seeds and
inspect convergence before interpreting simulation output.
