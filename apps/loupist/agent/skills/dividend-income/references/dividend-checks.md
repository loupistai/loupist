# Dividend checks

## Coverage

Payout on earnings is cash dividends ÷ net income attributable to owners (for a
Korean filer, consolidated 지배기업 소유주지분 순이익); keep totals with totals and
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

A buyer before the ex-dividend date receives the dividend. In the US (T+1
settlement) the ex-date is the record date. In Korea (T+2) it is one business
day before the record date (배당기준일). Since 2023 many Korean companies set the
record date after the dividend is decided, often in February to April; the cash
dividend decision notice (현금ㆍ현물배당결정) states it. When `dart_dividends`
reports the record date as `in_notice` or `no_notice`, say so rather than
assuming December 31.

## Implied dividend growth (card)

To show what dividend growth a price implies, run `reverse_dcf` with the
dividend per share as `cash_flow`, `shares` 1, the dated `price`, rates as costs
of equity, and the dividend `cagr` as `reference_growth_pct` (cited in
`input_sources`, its period in `labels.reference_growth_pct`). Write a paragraph
that is only a link whose text is the company name, `[Company](citation_url)`;
the app shows it as a card. It is a range, not a forecast or a target price.

## Income

Only with a share count the user gave: shares × dividend per share, before tax,
from the latest full year, never projected as a promise.
