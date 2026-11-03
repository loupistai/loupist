# Goal assumptions

## Choosing rates

Pick three to five yearly rates that span the realistic range, for example 0%
(cash under the mattress), 3% (deposits or short bonds), 5% (a mix) and 7%
(mostly stocks). Rates above 10% are not a central case. Add a negative rate
only when the user asks about a bad run. Long-run averages of past markets are
history, not a promise; a single bad decade can sit far below them.

## How `goal_grid` counts

Deposits arrive at the end of each period; `periods_per_year` 12 for monthly,
52 for weekly, 4 for quarterly, 1 for yearly. The yearly rate becomes a periodic
rate as (1 + r)^(1/p) − 1. `start` is money already set aside. With `target`
instead of `contribution`, each rate returns the saving needed per period.

## Inflation, costs and taxes

Results are nominal. To show buying power, repeat the grid with each rate
reduced by an assumed inflation rate (say which, such as 2%) and label it "in
today's money". A yearly fee lowers the rate by roughly the fee; say that is an
approximation. Taxes are left out unless the user gives a rate; tax-advantaged
accounts exist, but describe them only when asked.

## Looking back with `dca_history`

Use a dated close series covering the whole period (`dates` or the series'
`date_column`). `monthly` buys on the first trading row of each month, `weekly`
on the first of each week, `every_row` on every row; `fee_pct` is charged on
each purchase. Price-only series leave out dividends, so the result understates
a total return. Report invested, units, average cost, value and return with the
dates, and say it shows what happened, not what will.

## What to leave out

No suitability questions, no recommendation of how much to save, which product
to buy or how to split money, and no single promised figure.
