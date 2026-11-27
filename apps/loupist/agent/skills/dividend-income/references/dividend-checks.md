# Dividend checks

## Coverage

Payout on earnings is cash dividends ÷ net income attributable to owners (the
consolidated figure where the company consolidates); keep totals with totals and
per-share figures with per-share figures. Payout on free cash flow is dividends
÷ (operating cash flow − capital spending). A payout above 100% is paid from
savings or borrowing; one year can be distorted by a one-off, so read the
pattern across years. Rising debt beside a steady dividend weakens it. Banks,
insurers and REITs need their own measures (capital ratios, funds from
operations); say when an earnings payout misleads.

## History, growth and yield

Name each cut, suspension and special dividend. Leave specials out of growth or
say they are in. Adjust per-share history for splits and bonus issues. Growth is
`cagr` from the first to the last full fiscal year, with the period written as
"FY2020–FY2025". Yield is the dividend per share (latest full year or trailing
twelve months, say which) ÷ a dated close.

## Dates

A buyer before the ex-dividend date receives the dividend. The ex-date follows
from the market's settlement cycle: with next-day settlement (T+1) it is the
record date, with T+2 one business day before it. A record date is not always
the fiscal year-end: a company may set it when it decides the dividend, and
then the decision notice states it. When `dart_dividends` reports the record
date as `in_notice` or `no_notice`, say so rather than assuming the year-end.

## Implied dividend growth

To show what dividend growth a price implies, run `reverse_dcf` with the
dividend per share as `cash_flow`, `shares` 1, the dated `price`, rates as costs
of equity, and the dividend `cagr` as `reference_growth_pct` (cited in
`input_sources`, its period in `labels.reference_growth_pct`). Compare the implied range
with that historical growth rate. It is a range, not a forecast or a target price.

## Income

Only with a known share count: shares × dividend per share, before tax, from the
latest full year, never projected as a promise.
