# Portfolio risk review

Informed by Microsoft Qlib's holdings/executor and benchmark/cost reporting
(MIT). See [upstream provenance](../../NOTICE.md).

## Holdings and aligned returns

Inspect position quantities, prices, weights, valuation dates, cash, leverage,
and currency. Check weight sums against the declared convention; net exposure
and gross exposure are different. Align trading sessions and FX timestamps.
Distinguish price returns from total returns and retained assets from exclusions.

For static portfolios, calculate weighted returns explicitly. For changing
weights, use holdings and cash through time. Returns, loss rates, and currency
loss amounts have different units. If currency is unknown, preserve the supplied
portfolio units rather than inventing a currency.

## Estimation and scenarios

Name the observation window, frequency, annualization, missing-data policy, and
risk estimator. Check finite values, covariance symmetry/consistency, sufficient
common observations, and sensitivity to the window. Report concentration and
correlation alongside volatility; historical diversification can weaken in stress.

For VaR or expected shortfall, state confidence level, horizon, estimator,
sample count, loss sign convention, and portfolio value used for loss amounts.
Separate an observed historical loss from a modeled or hypothetical stress.
Inspect plausible joint shocks and explain omitted nonlinear or liquidity risks.

For an allocation model, specify the objective, expected-return model, cash rule,
weight/exposure limits, turnover/costs, and feasibility. Check the solution
against constraints and compare with a simple relevant allocation on the same
basis. A score ranking is not automatically an expected-return forecast.
Report the solution with its model assumptions and limitations.

## Return and risk report

Keep portfolio, benchmark, gross, and net series aligned. Disclose whether
risk and drawdown use the portfolio equity curve or an active-return series.
Do not label their statistics interchangeably. Explain which positions or
assumptions drive the result and where history is too short for a stable estimate.
