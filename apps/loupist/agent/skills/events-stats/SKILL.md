---
name: events-stats
description: Design or audit an event study, regression, factor analysis, hypothesis test, or statistical interpretation of market moves. Use for inference workflows; answer a fully specified one-step statistic directly.
---

# Events and statistical inference

Define the target effect, sampling unit, event or observation window, benchmark,
and hypothesis. Separate planned tests from exploratory findings.

Read [inference and diagnostics](references/inference.md) for test selection,
dependent observations, multiple comparisons, or event-study aggregation.
The market data connection's local `quantitative_*` and `econometrics_*` tools
cover descriptive statistics, normality, unit roots, correlation and OLS on
supplied rows without a runtime. Otherwise use available scientific libraries
after inspecting their actual APIs and data.

Preserve the design, coverage, code, and random seed for reproduction. Check
assumptions relevant to the estimator; a diagnostic p-value alone cannot
certify the model or select its replacement.

Lead with effect estimates and uncertainty. Report the tested family,
adjustment, sample size, and significance boundary when applicable. A p-value
is not the probability the null is true, and temporal association alone does
not establish a causal market explanation.
