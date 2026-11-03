# Korean asset rows

Korean filings, flows and retail rows come from the host's read-only routes. Account quantities are redacted before a model sees them.

## Korean asset rows

A Korean equity's [asset page](../knowledge/asset-pages.md#asset-pages) adds rows that Rust
builds from OpenDART and Korea Investment without a model:

- **Capital decisions.** When a refresh's filings read lists a new 주요사항보고서 for a
  rights offering, convertible bonds, bonds with warrants or a buyback, the refresh reads
  that decision with `dart_major_report` for its receipt day (at most two per refresh,
  newest first). What changed shows one Capital row per decision in plain words: new
  shares and how they are offered; the amount, conversion price, reset floor and the
  most shares the bonds could add ("up to 8.7% more shares if reset to ₩6,370", else
  "6.1% more shares at ₩8,800"); or the buyback amount and end date. A calculated
  dilution is a calculation chip and a filed one is covered by the row's source; a
  rights offering points to the notice for its price and payment date. Amounts and share
  counts use the shared amount format. Other decisions stay filing rows.
- **Dividends.** Each refresh reads `dart_dividends` for the fiscal year whose annual
  report is due (the previous year from April 1, else the one before). The dividend row
  and At a glance show the common dividend per share with its record date (dated and
  cited by the decision notice), "record date in the notice", or the amount alone.
- **Investor flows.** With Korea Investment granted, a section after At a glance (which
  comes first at every width) covers the last ten sessions whose flows are
  final: foreign, institutional and individual net buying under a "Net buying" header
  (negatives are read to screen readers as net selling) (`calculate` `sum` over
  `investor_trade_by_stock_daily`'s `*_ntby_tr_pbmn`), short selling as a share of
  volume (Σ short volume ÷ Σ volume with `ratio`, from `daily_short_sale`) and the
  lending balance change to its own latest date (`percent_change`, from
  `daily_loan_trans`). Each value is a calculation chip with its formula and inputs,
  signed but never in price colors; each row keeps its own evidence, the section names
  Korea Investment, and an always-visible caveat says flows are final after the close
  and lending balances settle two business days later. Net buying is
  taken to be in millions of won (`KIS_AMOUNT_SCALE` in `knowledge/assets/flows.rs`);
  the KIS catalog does not state the unit. A session's flows count as final from 18:30
  Seoul time on a KRX trading day (published holidays from `market-calendar.json` are skipped). A refresh reads the three
  operations only when the stored summary ends before the latest final session and the
  last attempt was at least six hours ago: normally once a day, retried every six hours
  while that session is missing (a failed read, or a holiday in a year KRX has not published). Without the connection the
  section is one line, "Connect Korea Investment to see investor flows", with the link.
  The lending balance row carries a one-line hint of what it measures.

A Memory-saving turn's own `dart_major_report` or `dart_dividends` read records the
same capital and dividend rows. Credit balances, short-sale balances and lock-up
schedules are not read; the page lists them as not checked yet.

## Code

- Korean disclosures and flows: `src-tauri/src/finance_data/dart_events.rs`, `src-tauri/src/commands/tool_executor/finance.rs`, `src-tauri/src/knowledge/assets/flows.rs`, `src-tauri/src/commands/assets.rs`, `scripts/generate-kis-read-api.py`, `agent/skills/filings/references/korea-statements.md`
