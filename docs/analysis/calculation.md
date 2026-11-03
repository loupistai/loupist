# Calculation

Arithmetic and sensitivity cards are host calculations from pinned inputs. A missing cost stays a before-cost result.

## Calculation

`calculate` (the `calculations` pack,
`apps/loupist/src-tauri/src/calculations.rs`) is the default for common finance
arithmetic and needs no runtime: percent change, CAGR,
margin, ratio, payout ratio, net debt to EBITDA, free cash flow, growth series,
returns summary (return, volatility and drawdown dates), moving average, sum,
holdings overlap, a savings-goal grid, regular-investing history, and DCF
and reverse-DCF grids over WACC × terminal growth (never one point value). Inputs
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
usually a cited `cagr`, its period in `labels.reference_growth_pct`); its grid then adds
`reference` and a per-cell `above_reference`, and a note counts the cells above it. Every
input carries `label`, the name its formula prints (its `labels` entry, else the host's
word: Revenue, OCF, from, years…), and `unit`, how it is printed: `number` (years,
shares, or a unit Rust cannot place, kept as `text`), `percent` (`*_pct` inputs and a
`returns_pct` series), `points`, `basis_points`, `multiple`, `price` (a per-share price
in the request's currency) or `amount` with the ISO `currency` and power-of-ten `scale`
the request's `unit` states ("KRW billions" → KRW, 9). The reference input keeps the
label "reference growth" and carries `labels.reference_growth_pct` as `period`.

Each result returns values with units, a one-line formula with the inputs
substituted and each input's provenance. It is a citable derived `result_ref`
whose lineage includes the reads behind its dataset inputs; it is never Source
material, so answers cite the original evidence beside it. Bash, Python or
JavaScript remain for custom transformations, covariance, optimization,
simulations and workbooks; [Agent development](../agent/tools.md#tool-contracts-and-turn-preparation)
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
amount with or without its currency, other inputs in the request's `unit`), and
records `matched`, `rounded` or `not_found`. A dataset-cell input keeps its pinned
provenance.

### Sensitivity cards

When the reply commits, `commands/tool_executor/sensitivity.rs` turns each top-level
paragraph that is only one evidence link (outside lists, quotes and tables) to a
`dcf_grid` or `reverse_dcf` result of this turn into a card, at most eight per answer,
stored as the answer's `sensitivity_cards`
([turn authority](../runtime/chat-turns.md#turn-authority-and-finalization)). The link text, cut
to 80 characters, names the company. Rust copies the grid, forecast years, units, up to
eight formula lines and twelve inputs from the retained result; each input names the
result it was cited from (`input_sources`) or read out of (a dataset cell), with its
check status. A reverse grid also keeps the price, dated by the matched row of its cited
result or else by that evidence's publication date, and the reference with its label
and, when cited from a `cagr` result, its years. Model text never supplies a value, and
host-composed replies have no cards.

The card (`src/components/chat/SensitivityCard.tsx`) replaces its paragraph: discount
rates down, long-run growth across, the middle cell outlined as the base case and, only
for a reverse DCF with a reference, the cells that need more growth than the reference
shaded, with a legend swatch. A cell the solver could not place within −50% to 100% shows
"—", and a plain caption counts them. The price and each input carry their source's citation (an
input that needs checking is flagged) and the reference is a number chip. How this
was calculated shows the step inline (the formulas, where there is no step); Copy data
copies tab-separated rows; Change the rates fills the composer and never sends; an as-of
answer adds "Known as of {date}". On the [results card](results.md#results-before-and-after), Open
the grid shows the host's price-implied grid the same way, without a reference; the
price and the cash flow carry their source chips, and Rust labels the cash flow with its
fiscal year ("free cash flow FY2025").

## Execution and history

Native writes survive a cancelled reply. Bash limits and the missing sandbox are in [Chat turns](../runtime/chat-turns.md#local-workspace-execution). There is no resumable analysis job.

For long calculations, save checkpoints and explicit environment/dependency
versions alongside scripts. A rerun executes those files again through Bash;
there is no separate resumable analysis-job engine. A [Scheduled Chat](../runtime/scheduled-runs.md#scheduled-chats) can execute a saved prompt, but does not resume a Bash process.
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

- Calculations and sensitivity cards: `src-tauri/src/calculations/`, `src-tauri/src/commands/tool_executor/sensitivity.rs`, `src-tauri/src/domain/answer_checks.rs`, `src/chat/sensitivity.ts`, `src/components/chat/SensitivityCard.tsx`
