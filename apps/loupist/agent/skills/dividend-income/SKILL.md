---
name: dividend-income
description: Check how safe and how steady a company's dividend looks from its history, payout on earnings and free cash flow, cuts, growth, yield and record dates. Use when someone asks whether a dividend is safe, sustainable or growing; skip a single dividend-amount lookup.
---

# Dividend income

Read the dividend history first: `equity_fundamental_dividends` from the market
data connection, or for a Korean company `dart_dividends` for each fiscal year
needed (it also reads the record date from the decision notice). Use five or
more years when the sources have them. Then work through these steps, each
derived figure from `calculate` with its sources in `input_sources`:

1. Payout on earnings and on free cash flow (`payout_ratio`, after
   `free_cash_flow`), naming the basis.
2. Years with a cut, a suspension or a special dividend, and what happened then.
3. Growth of the dividend per share: `cagr` over the last five fiscal years
   when the data allows, beside earnings growth.
4. Yield on a dated price (`ratio`), never an undated one.
5. Record, ex-dividend and payment dates; say when a date is only in the notice.

Read [dividend checks](references/dividend-checks.md) for coverage traps and
Korean dividend practice, and before showing what dividend growth the price
implies: that is a `reverse_dcf` card with the dividend `cagr` as its reference,
linked alone in its own paragraph. Compare two or more
companies in one `grid_publish` table. Work out yearly income only when the user
has stated a share count; never ask what they hold. End with a conditional view:
what supports the dividend and what would threaten it.
