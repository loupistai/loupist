# Korean statements and disclosures

## Statement basis

Use consolidated statements (`basis` consolidated) by default. For EPS,
P/E and payout use net income attributable to owners of the parent
(`net_income_attributable_to_owners`), not total net
income. Use separate statements for the parent company alone and for
dividend capacity, and always name the basis. A company with no subsidiaries
files only separate statements: a `dart_financials` request for `basis`
consolidated then returns the separate statements of the same period, marked
`requested_basis` consolidated with a warning. Use
the separate figures for the whole answer and tell the user the company filed
no consolidated statements for that period; that is the reason for the basis,
and a lookup that returned nothing is not. Never mix bases in one ratio, table
column or comparison.

## Periods

Korean quarterly reports carry the quarter and the cumulative year to date
(`cumulative_value`). The fourth quarter alone is the annual figure minus the
nine-month cumulative figure from the third-quarter report: compute it with
`calculate` (`sum` over the annual figure and the negated nine-month figure),
cite both reports, and label the result derived, never reported. Preliminary
results (a fair-disclosure notice) are unaudited and can change in the
quarterly or annual report; say "preliminary" beside them. When a company's
fiscal year does not end in December, `dart_financials` leaves periods undated:
give the fiscal year as the report names it and do not assign calendar dates.

## Capital decisions, ownership and dividends

- `dart_major_report` reads one kind of material-event decision over at most 366
  days: rights offerings, bonus issues, capital reductions, convertible
  bonds (CB), bonds with warrants (BW), exchangeable bonds, buybacks, treasury
  stock sales, mergers and splits. Explain them in plain words: how many new
  shares, at what price, to whom, for what; for a CB, the conversion price, the
  floor it can be reset down to (a refixing clause) and the most shares it
  could add. Cite `dilution_pct` with its basis (the filing's ratio or a host
  calculation); for a bond with a reset floor it is the most shares at the
  floor (`dilution_at` `floor`), and `dilution_filed_pct` is the filed ratio at
  the starting price: give both. Issue prices of rights offerings, allottees and lock-ups are in
  the filing: read it with `dart_filing_read`.
- `dart_ownership` reads 5% holder reports or insider reports (executives and
  major shareholders), with the ratio before and after.
- `dart_dividends` reads one fiscal year's dividend table and the record date
  from the decision notice; when its status is `in_notice` or
  `no_notice`, say the date is in the notice or unknown.

## Investor flows

With Korea Investment connected, `kis_read` operations
`domestic_stock.investor_trade_by_stock_daily` (net buying by foreign investors,
institutions and individuals; `*_ntby_tr_pbmn` in millions of won),
`domestic_stock.daily_short_sale` (short-sale volume beside total volume) and
`domestic_stock.daily_loan_trans` (lending balance) give daily rows. Sum a
window with `calculate` `sum` (`unit` "KRW millions"), the short-selling share
with `ratio`, and the lending balance change with `percent_change`. Flows are
final only after the close; lending balances settle two business days later.
State the last date each figure covers.

## Not read

These connections do not read margin-loan (credit) balances, short-sale balances
or lock-up schedules. Say they were not checked; never
estimate them.
