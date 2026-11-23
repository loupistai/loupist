# Calculation

Arithmetic and sensitivity ranges are host calculations from pinned inputs. A missing cost stays a before-cost result.

## Calculation

`calculate` (the `calculations` pack,
`apps/loupist/src-tauri/src/calculations.rs`) is the default for common finance
arithmetic and needs no runtime: percent change, CAGR,
margin, ratio, payout ratio, net debt to EBITDA, free cash flow, growth series,
returns summary (return, volatility and drawdown dates), moving average, sum,
holdings overlap, currency conversion, a savings-goal grid, regular-investing history,
and DCF and reverse-DCF grids over WACC × terminal growth (never one point value). Inputs
are JSON numbers, decimal strings, or cells and columns of a pinned dataset
revision this Loupist owns, checked against its stored digest. Arithmetic is
IEEE-754 binary64 with reported values rounded to 15 significant digits; a missing
value fails or, with `skip_missing`, is dropped, never treated as zero.

The everyday operations live in `calculations/series.rs` and `calculations/retail.rs`:

- `sum` takes a `series` and returns its total, count and mean.
- `holdings_overlap` takes `weights_a` and `weights_b` (`{holding id: weight %}`, 1–500
  each, ids compared without case, each set adding to at most 100.5%) and returns the
  overlap (Σ of the smaller weight of each shared holding), the shared count and the top
  ten shared; one company listed under two ids is not matched.
- `goal_grid` takes whole `years` (1–60), either `contribution` (where saving leads) or
  `target` (what to save each period, never below zero), an optional `start`,
  `periods_per_year` 1, 4, 12 or 52 (default 12) and 1–9 distinct `return_pct` rates
  from −20 to 30, and returns one value per rate with the low and high. Deposits fall at
  the end of each period at the periodic rate (1 + r)^(1/p) − 1; amounts are nominal and
  before costs.
- `dca_history` buys `contribution`, less an optional `fee_pct`, on the first row of
  each month (the default), each ISO week or every row of a price `series` with a date
  for every price, and returns invested, units, average cost, last price, value and
  return. A series without dividends gives a price return.

`reverse_dcf` takes an optional `reference_growth_pct` (above −50%, at most 100%;
a cited `cagr` of the same measure as `cash_flow` over the last five fiscal years of
growth, its period in `labels.reference_growth_pct`); its grid then adds
`reference` and a per-cell `above_reference`, and a note counts the cells above it. Every
input carries `label`, the name its formula prints (its `labels` entry, else the host's
word: Revenue, OCF, from, years…), and `unit`, how it is printed: `number` (years,
shares, or a unit Rust cannot place, kept as `text`), `percent` (`*_pct` inputs and a
`returns_pct` series), `points`, `basis_points`, `multiple`, `price` (a per-share price
in the request's currency) or `amount` with the ISO `currency` and power-of-ten `scale`
the request's `unit` states ("KRW billions" → KRW, 9; "KRW 100 million" → KRW, 8). The reference input keeps the
label "reference growth" and carries `labels.reference_growth_pct` as `period`.

Each result returns values with units, a one-line formula with the inputs
substituted and each input's provenance. It is a citable derived `result_ref`
whose lineage includes the reads behind its dataset inputs; it is never Source
material, so answers cite the original evidence beside it. `run_python` remains
for custom transformations, covariance, optimization, simulations and workbooks.
[Agent development](../agent/tools.md#tool-contracts-and-turn-preparation)
owns the order in which Skills choose these routes.

`input_sources` (`{ <input name>: <citation_url> }`, at most 16) names the result
each typed input was read from. Rust removes it before computing. Each name must be
an input of the request and each URL the `#evidence-…` citation of a result from
this turn; otherwise the call fails on that field (`input_sources.revenue: not a
citation_url from this turn`). The cited result joins the calculation's lineage. A
typed input then records provenance `{kind: "cited", evidence_id, status}`: Rust
[looks for](number-checks.md#number-matching) its value among the cited result's numbers within half
a unit of its last written digit, read in the input's own unit (`*_pct` inputs as
percents, `years`, `shares` and `forecast_years` as counts, `price` as a per-share
amount with or without its currency, other inputs in the request's `unit`, or, when
the request has no `unit`, as the evidence prints the amount in its own scale: `52`
matches `52억원`; a negative input also matches a fall its evidence
[states in words](number-checks.md#number-matching), `-9` for `9% 줄었고`), and
records `matched`, `rounded` or `not_found`. A dataset-cell input keeps its pinned
provenance.

`ratio` divides `numerator` by `denominator`. Two amounts in the request's
`unit` give a multiple (`x`), or a percentage with `as_percent: true`. A `unit`
that names a quantity per something (`USD per share`, `KRW/share`,
`vehicles per store`) divides an amount in that unit by a pure number: the
numerator is read in `unit`, the denominator is a `number` input, and the result
and its formula carry `unit` (`345.82 / 100 = 3.4582 USD per share`). Such a
ratio refuses `as_percent: true`. `margin` is always a percentage: it accepts
`as_percent: true` as a no-op and refuses `as_percent: false`.

`convert` takes `inputs.amount` and `inputs.rate`, a `unit` with the amount's
ISO currency code and scale (for example, `USD billions`), and a different
uppercase target code in `to`. It multiplies the amount by its scale and by
the positive rate, returning `converted` in base units of `to`. The formula uses
the amount's scale for reading and shows the rate as target units per source unit.

A cited rate must come from this turn's [`fx_rate`](../agent/turn-tools.md#turn-tools)
result in the requested direction. The rate must agree with its `to_per_from`
within both half a unit of the last typed digit and half a unit of the sixth
significant digit of the published rate. The formula shows the source and date,
and `conversion` retains them with the rate table revision. An uncited rate is provided as an
assumption. An amount from evidence must agree with the currency stated beside its
matched or pinned cell. Rust checks `<field>_currency` first, then `currency`,
`reported_currency` and `currency_code` in that row. A cited amount that matches
a passage or a row with no stated currency records `currency_basis: "declared"`,
as does a pinned cell with no stated currency. Currency codes in the requested
unit and in source rows must use consistent case.
Minor-unit markers such as `GBp`, `ZAc` and `ILA` are refused.

### Sensitivity ranges

`dcf_grid` returns a range of values across discount rates and terminal growth
assumptions. For `reverse_dcf`, the answer explains the implied growth range at
the dated price and compares it with historical growth when available. Figures
link to the retained calculation and its inputs. A link on a line of its own
opens the cited evidence: it stays a citation in its own paragraph while the
answer streams and after it is saved, and no answer stores a card for it.

## Execution and history

Native writes survive a cancelled reply. `run_python` limits and the writable-subfolder risk are in [Chat turns](../runtime/chat-turns.md#local-workspace-execution). There is no resumable analysis job.

For long calculations, save checkpoints and explicit environment/dependency
versions alongside scripts. A rerun executes those files again through `run_python`.
There is no separate resumable analysis-job engine. A [Scheduled Chat](../runtime/scheduled-runs.md#scheduled-chats) can execute a saved prompt, but does not resume a Python worker.
Code and runtime version, exact input revisions, parameters, seed, assumptions,
fees, adjustment rules and validation checks should accompany reusable research.
Use the retained dataset export/import boundary when follow-up charts must keep
an exact result even after workspace files change.

When implementing historical simulations, require completed-bar signals, explicit fill timing,
costs, cash/position accounting, no missing-price fills, and prefix stability checks
for future-dependent indicators. Custom simulations identify their engine and
limits. The optional `rule-test` workflow supplies simulation checks; simulations can also
be implemented directly. The app bundles no simulation engine or fixed Python package allowlist.

Unspecified costs produce a before-cost result. Prices without total-return adjustment produce price returns. Intrabar stop/target ordering and unsupported corporate actions are disclosed. Custom simulations identify their own engine and checks. Execution success does not validate a strategy or promise future returns.

## Code

- Calculations and sensitivity ranges: `src-tauri/src/calculations/`, `src-tauri/src/commands/tool_executor/sensitivity.rs`, `src-tauri/src/domain/answer_checks.rs`, `src/components/assets/SensitivityCard.tsx`, `src/components/assets/sensitivityText.ts`
