# DCF and valuation checks

Adapted from Anthropic financial-services, dcf-model (Apache-2.0), with
Loupist-specific scope and corrected financing-weight conventions.
See [upstream provenance](../../NOTICE.md).

## Model construction

Match cash flows and discount rate: FCFF to the cost of capital; FCFE or
distributions to an appropriate cost of equity. For banks or insurers, assess
whether equity/distribution or excess-return methods better fit their economics.
Separate historical actuals, forecast drivers, and outputs. Model segments when
one blended growth rate hides material differences.

For an operating FCFF build, reconcile EBIT, cash taxes, D&A, capital spending,
and the change in non-cash operating working capital. A working-capital increase
uses cash. Avoid counting financing interest in an unlevered cash-flow schedule.
Use the issuer's cash-flow and accounting definitions rather than fixed industry
percentages. Forecast reinvestment consistently with the growth being assumed.

## Discounting and terminal value

Use a risk-free rate, inflation basis, and discount rate consistent with the
cash-flow currency and nominal/real convention. Name the valuation date and
discount timing. Do not substitute a current input into a historical valuation.

For the standard operating-business WACC, weight market-value equity and
interest-bearing debt (plus other financing claims when modeled). Cash belongs
in the equity bridge; substituting negative net debt into this formula can
produce negative financing weights. A different cash-adjusted approach needs
consistent cash flows, beta, financing definitions, and an explanation.

State when forecast cash flows and terminal value occur; discount each from
its actual time. A mid-year cash-flow convention does not by itself establish
the terminal value date. With perpetual growth, require discount rate > growth
and compute terminal cash flow from the first post-forecast period. Explain a
steady-state growth/reinvestment relationship. With an exit multiple, align the
multiple and terminal metric and avoid mixing transaction and trading multiples.

## Equity bridge and sensitivity

Reconcile operating enterprise value to equity by accounting explicitly for
non-operating assets/cash, debt, preferred claims, minority interests, and any
other material claims. Treat leases consistently with operating profit and
cash flow; avoid double counting. Match diluted shares and convertible treatment
to the value allocated to common equity.

Test the assumptions that actually move value. Rerun the full model for each
sensitivity point, carry units through the bridge, and reconcile the base
sensitivity output to the headline valuation. Inspect invalid denominators,
non-finite outputs, sign errors, and the portion of value from the terminal
assumption. Terminal-value share has no universal acceptable percentage.

A reverse DCF reads the growth the price needs, so its reference should be the
same measure that seeds the grid: the free-cash-flow `cagr` from the first to
the last full fiscal year (FY2020–FY2025 when five years exist). When a base
year is negative or distorted by a one-off, use fewer years or a steadier
measure such as revenue growth, and say which. Cells marked as needing more
growth than the reference compare the price with the past; they are not a
forecast.

For a reusable model, separate inputs from derived calculations and retain
provenance beside the inputs. A requested spreadsheet needs live formulas and
an available recalculation engine; a local script can provide a rerunnable model
without imposing a workbook, color scheme, fixed grid size, or approval gate.
