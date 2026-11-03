---
name: portfolio-risk
description: Review portfolio exposure, diversification, drawdown, covariance, tail risk, stress scenarios, risk contribution, or hypothetical allocation models the user specifies. Use for a portfolio analysis workflow; skip a simple weighted-return calculation.
---

# Portfolio and risk review

Establish the holdings or hypothetical universe, valuation date, base currency,
return frequency, window, and weight convention. Separate actual positions
from candidate allocations.

Allocation-model and optimizer outputs are hypothetical calculations over the objective and
constraints the user stated. Label model assumptions and distinguish them from observed holdings.

Read [risk review](references/risk-review.md) for return alignment, a risk
report, optimization, or stress analysis. Choose estimators and constraints
for the question rather than defaulting to an optimizer. For one return
series, `calculate` returns_summary gives return, volatility and drawdown
dates without a runtime; the market data connection's local `quantitative_*`
tools add Sharpe, Sortino and rolling statistics. Covariance and optimization
need local code.

For historical rebalancing, use the available rule-test Skill when helpful
and retain one shared cash/position ledger. Independent per-asset backtests
do not simulate a shared-capital portfolio.

Report requested metrics with units, estimation window, and material coverage
limits. Pair returns with concentration and plausible losses. Preserve a
runnable analysis when requested. An optimizer output is an estimate and does
not authorize trades; recording the user's own judgment follows the Journal
Skill.
