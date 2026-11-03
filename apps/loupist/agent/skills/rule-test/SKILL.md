---
name: rule-test
description: Design, implement, or audit a historical strategy test, screening rule, rebalance simulation, or parameter search. Use for backtesting workflows; skip an isolated return calculation.
---

# Strategy research and backtesting

Turn the question into a testable hypothesis and explicit rules. State the
universe, available information, signal, execution timing, sizing, rebalance
rule, costs, and benchmark. Generated strategies remain research hypotheses.

Check a single-series condition, such as price against its moving average,
with `calculate` or the market data connection's local `technical_*` tools
before writing a simulation; a simulation itself needs code.
Read [backtest design](references/backtest-design.md) before implementing or
auditing a simulation or optimization. Choose an installed library or custom
code whose execution and accounting match the rules. Qlib is a methodological
reference, not a required runtime or data source.

Keep data revisions, parameters, the trial list, and runnable code in the
workspace. Separate the signal from execution and portfolio accounting so a
timing change cannot silently alter all three. Verify a small hand-traced
example before relying on a full run.

Return the requested performance and trade evidence, validation period, and
sensitivity to costs or assumptions. State what the test cannot establish.
A historical result does not establish a live signal or authorize orders.
