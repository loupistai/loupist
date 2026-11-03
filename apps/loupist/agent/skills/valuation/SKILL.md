---
name: valuation
description: Build or audit an intrinsic valuation model, cash-flow forecast, sum-of-parts valuation, or sensitivity range. Use for a substantive valuation workflow; skip a quote lookup or an isolated arithmetic question.
---

# Valuation models

Match the model to the economics and the requested deliverable. Establish the
entity, valuation date, currency, share class, forecast horizon, and whether
the output is operating value, equity value, or value per share.

Read [DCF and valuation checks](references/dcf.md) for a cash-flow model,
discount-rate review, equity bridge, or sensitivity analysis. For a different
method, explain its economic basis and the inputs driving the result.

Keep observed inputs, forecasts, and scenario assumptions separately editable.
Use supplied assumptions; identify missing inputs that materially change the
conclusion. A supplied-input model needs no unrelated market data. A vendor
estimate is not management guidance or an observed fact.

Deliver the supported value range and decisive assumptions. For a two-stage
cash-flow model, `calculate` dcf_grid and reverse_dcf compute the WACC ×
terminal-growth grid without a runtime. For what a price implies, give
reverse_dcf the dated price and shares with their sources and, when the data
allows, a `cagr` of the same cash-flow measure over the last five fiscal years
as `reference_growth_pct` (cited in `input_sources`, its period in
`labels.reference_growth_pct`). Show either grid as a card: a paragraph that is
only a link whose text is the company name, `[Company](citation_url)`. Never
present one cell as the answer or as a target price.

Otherwise keep a rerunnable local model, recompute each sensitivity case
through it, and reconcile its base case with the reported result. Create a
workbook when useful or requested, and verify its formulas with an available
spreadsheet engine before claiming they recalculate.
