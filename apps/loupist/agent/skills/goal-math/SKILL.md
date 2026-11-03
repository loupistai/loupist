---
name: goal-math
description: Do the arithmetic for a savings or investing goal, such as what regular saving could grow to, what a target needs each period, or how regular investing in an asset would have turned out. Use when someone gives an amount, a period or a target; skip choosing products for them.
---

# Goal math

Use only the numbers the user gives. Never ask about income, assets, debts, risk
tolerance or horizon beyond what the requested sum needs: ask only for a number
it cannot be done without, and otherwise state a plain assumption and continue.

Forward: `calculate` `goal_grid` with `years`, `contribution` (or `target` to
find the saving needed each period), the user's `start` amount if they gave one,
`periods_per_year` matching how often they save, and three to five yearly rates
in `return_pct`, from cash-like to stock-like. Show every rate in a small table,
never one headline number, and say no rate is a forecast or a promise.

Backward ("what if I had invested every month"): read a dated price series, then
`calculate` `dca_history` with the `contribution`, the `schedule` and any fee.
Say whether the series includes dividends.

Every result is nominal, before costs and taxes unless the user gives them; say
so, and that inflation reduces what the money buys. Read
[goal assumptions](references/goal-assumptions.md) for choosing rates,
inflation, fees and what to leave out. Do not recommend a product, an amount to
save or an allocation.
