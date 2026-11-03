# Backtest design and validation

Informed by Microsoft Qlib's strategy/executor/account separation and cost-aware
portfolio reporting (MIT). See [upstream provenance](../../NOTICE.md).

## Freeze the experiment

State the hypothesis, point-in-time universe, available data, signal timestamps,
execution price/time, sizing, cash rule, costs, and benchmark. Fix a bounded
search budget and retain the full trial list, including losing trials. Separate
fitting, validation, and final out-of-sample periods before parameter selection.

Use only information available at each decision time. A close-based signal
normally acts on a later eligible execution; any other convention needs an
explicit information-time justification. Respect release delays, revisions,
delistings, splits, dividends, trading calendars, and instrument eligibility.

## Simulate execution and capital

Keep signal generation separate from fills, cash, holdings, and valuation.
A predictive score need not be a calibrated expected return. Check the selected
engine's actual delay, rebalance, price, fees, minimum lot, and liquidity rules.
Do not copy an example's exchange limits or fee parameters to a different market.

Multi-asset tests need one shared cash and position ledger. Enforce exposure
constraints and distinguish gross from net returns. Do not forward-fill missing
returns or fill trades at unavailable prices. Mark final holdings consistently
with the reported equity curve.

## Check before interpreting

Hand-trace a small slice including entry, exit, costs, and a non-trading period.
Reconcile cash plus marked positions with equity and the trade ledger. Check
prefix stability: adding future bars must not change earlier signals or fills.
A shifted array alone does not prove freedom from future information.

Evaluate held-out data without repeated retuning. Walk-forward validation and
purging/embargo can help when observations or labels overlap; explain the design.
Disclose multiple testing, sensitivity to timing and costs, and universe coverage.

Report relevant trade/sample counts, return, volatility, drawdown, turnover,
exposure, and benchmark over common dates. Name whether a risk statistic uses
raw, excess, or active returns and whether aggregation sums or compounds them.
Preserve exact inputs, code, parameters, and engine version for a rerun.
