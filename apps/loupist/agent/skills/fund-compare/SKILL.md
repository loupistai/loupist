---
name: fund-compare
description: Compare ETFs or funds side by side on cost, size, age, holdings overlap and returns over one shared window. Use when choosing between funds or checking whether two funds hold the same things; skip a single fund quote.
---

# Fund comparison

For each fund read `etf_info` (expense ratio, assets, inception date) and
`etf_holdings` (holdings, weights and their as-of date) from the market data
connection; activate them if they are not active. Then:

1. Overlap: `calculate` `holdings_overlap` with each fund's holdings as
   `{holding id: weight %}` in `weights_a` and `weights_b` and the fund names in
   `labels`. Use the same identifier on both sides and say when a list is partial
   or dated differently.
2. Returns over one shared window: the same start and end for every fund, from
   the later inception, with `returns_summary` on each price series. Say whether
   distributions are included; price-only returns leave them out.
3. Cost, size and age: compare them plainly; a fee difference compounds.

Korean-listed (KRX) ETFs: the connections give prices only, so compare price
returns from `krx_daily_quote` and leave holdings, overlap and cost as empty
`not_checked` cells. Never fill them from memory.

Publish one `grid_publish` table: one row per fund; columns for cost, size,
start date, overlap and return, each with its period. Read
[fund checks](references/fund-checks.md) for index, currency, tracking and
leveraged-product differences. End with what really differs and which trade-off
matters for the purpose the user stated, without asking about their finances.
