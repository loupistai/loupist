# Provenance and cutoffs

Publication time, observation period and retrieval time are different. A cutoff never backdates retrieval. Snapshots and trust tags are in [Datasets](datasets.md).

Publication time, observation period and retrieval time have different meanings. A historical cutoff never backdates retrieval. Strict point-in-time finance paths exclude unknown publication times; KRX daily bars are observations known at their date's close, so under a cutoff they keep bars dated on or before it, as OpenBB price history does; providers that cannot support a historical availability request, such as `kis_read`, reject it with `as_of_unsupported`. Under a cutoff, a native result without a dated row section fails closed with `as_of_unsupported` unless it is a document filed by then, and one whose rows are all later or undated fails with `as_of_empty`; a `dart_filing_read` document is dated by its declared filing date or, failing that, the receipt date that begins its 14-digit receipt number. Price results carry declared session, timezone, bar and adjustment metadata; unknown provider semantics remain unknown.

DART statement records expose company, provider and canonical concept, period
basis, consolidation, currency, scale and filing locators. Canonical concepts are
revenue, operating income, net income and net income attributable to owners,
basic EPS, total assets, total liabilities, total equity and equity attributable
to owners, operating cash flow and shares outstanding; repeats in other statements
stay provider concepts. The requested basis is explicit and recorded as `fs_div`
(`CFS` consolidated, `OFS` separate). Period dates come from the company's fiscal
year-end (`acc_mt` in its OpenDART profile): December year-ends are dated; any
other or unavailable year-end leaves periods undated with a warning.

`dart_major_report`, `dart_ownership` and `dart_dividends` (`finance_data/dart_events.rs`)
read OpenDART's capital decisions (주요사항보고, DS005), ownership reports (DS004) and the
annual report's dividend table (DS002). `company` is a corp_code, ticker or exact name,
and every returned row must carry that corp_code. Rows are dated by their receipt
(`filed_at`), so `as_of` keeps rows filed by the cutoff.

- `dart_major_report {company, kind, start, end}` reads one kind (`rights_offering`,
  `bonus_issue`, `capital_reduction`, `convertible_bonds`, `bonds_with_warrants`,
  `exchangeable_bonds`, `buyback`, `treasury_sale`, `merger`, `split`) over a receipt
  range of at most 366 days from 2015, as up to 100 `loupist-dart-major-report/1` rows,
  newest first. Bond rows keep the face amount, conversion or exercise price, reset floor
  (최저 조정가액), coupon and maturity rates and payment date. `dilution_pct` is the
  filing's own ratio (`dilution_basis: filing`) or new shares ÷ shares before through
  `calculate` `ratio` (`calculated`, with its formula); exchangeable bonds hand over
  existing shares and have none. A convertible or warrant bond with a reset floor below
  its price reports the worst case instead (`dilution_at: floor`): the most new shares
  (face amount ÷ floor, `max_new_shares`) over the shares outstanding implied by the
  filing's own ratio (shares at the price ÷ the filed percentage), with the formulas and
  inputs in `dilution_calculation` and the filed figure kept as `dilution_filed_pct`.
  Without a floor the filed ratio stays (`dilution_at: price`). OpenDART's rights-offering decision carries no issue
  price or payment date, and the result says to read the filing for them.
- `dart_ownership {company, kind: major_holders | insiders, limit?}` returns up to 100
  (default 20) 5% holder or insider reports, newest first, with the ratio, its change
  and `ratio_before_pct` (the ratio minus its change).
- `dart_dividends {company, year}` returns dividend per share, payout, yield and totals
  by share class and basis for that fiscal year and the two before, plus `record_date`
  (배당기준일) from the newest cash-dividend decision notice filed from November of that
  year through April of the next: `found` with the date, `in_notice` when the notice
  text did not yield it, or `no_notice`. Under a cutoff a later notice counts as
  `no_notice`.

Under an `as_of` cutoff, Rust bounds bundled OpenBB reads in
`apps/loupist/src-tauri/src/commands/tool_executor/mcp_host/point_in_time.rs`.
Only an allowlist of dated routes runs: price history (equity, ETF, index,
crypto, currency, futures), dividends by ex-dividend date and filings by filing
date. The host sets a declared
`end_date` to the cutoff, rejects a later `start_date`, removes later and undated
rows from the structured result before the model or a retained dataset sees them,
rebuilds the text block and drops the provider chart. The result's `as_of` block
reports kept and excluded rows; nothing left fails with `as_of_empty`. When the
provider's `adjustment` choices include an unadjusted option, the host forces it
and the `as_of` block says so; otherwise equity and ETF price history and
dividends may reflect later splits or dividends, and the result says so. Every other OpenBB data route and every custom MCP tool is refused
with `as_of_unsupported`; OpenBB local analytics compute on supplied inputs and
stay available.

For a direct web URL, the host can fetch the page to discover its declared
publication time before applying `as_of`. It delivers and makes the result
saveable only when that publication is within the cutoff; undated, modified-only
and later pages fail with `as_of_later` and grant no evidence or save authority.
Web fetches retain a valid publisher-declared HTML publication date when the
search result has none; modified dates and retrieval time are not substitutes.

`krx_daily_quote` reads the KOSPI and KOSDAQ daily files. `market` is `auto`
(KOSPI, then KOSDAQ; the default), `kospi` or `kosdaq`. A call takes one `date` or
a `start`/`end` range of at most 31 weekdays and lists dates without a row in
`missing_dates`. Past published files are cached under the app data
`cache/krx-daily/`; today's file and empty files under a week old are not. Cache
bounds and rules live in `apps/loupist/src-tauri/src/finance_data/krx.rs`.
`kis_read` follows provider continuation up to 8 pages and 16 MiB and reports
`provider_pages`, `page_limit` and `stop_reason`; any stop other than `complete`
adds a warning. KIS results carry `timezone: Asia/Seoul`,
the retrieval time in Seoul and, for KRX-listed cash products, the session at
retrieval by KRX trading-day hours (published KRX holidays are closed; an unpublished year counts every weekday).
`defillama_read` returns daily USD rows `{date, value}` (value locked for a protocol or
chain, or a stablecoin's circulating supply) for at most three years, the last 365 days
by default. A day's value counts as known at its end, so under a cutoff the host keeps
rows dated on or before it, like other daily series. DefiLlama recomputes past values
when it adds or corrects sources, so every result warns that it is not a historical
vintage.

A native finance or web read that never reaches its server fails with the error
code `offline`, naming the host or provider. OpenBB DNS, connection and timeout
failures carry the same code from its Python worker through the Rust broker,
without exposing provider exception text. The hint says not to repeat that read
in the turn and that other sources may still be reachable; Chat then shows an
[offline-read notice](../ui/chat-answer.md#answer-failures).

Independent native reads are separate tool calls. The host runs at most two of them at once and spaces calls to the same provider, including the extra requests inside one call (KRX range dates, KIS continuation pages, the DART fiscal year-end lookup). MCP control and each server's calls retain their existing serialization.

A successful MCP read with no rows is an `ok` result with `row_count: 0` and a
hint to change the request or try another enabled provider. Blank HTTP 204 replies
from the bundled market data worker need no provider receipt; every nonblank
reply still needs its matching canonical receipt. Empty arrays and rows containing
only null, blank text or empty containers supply no evidence (zero and false do).
These bundled and custom reads keep their host `result_ref` for composition, but
have no citation and do not appear in the answer's Sources. Custom reads still
raise the untrusted-text tier and pause outbound calls, including when empty.
