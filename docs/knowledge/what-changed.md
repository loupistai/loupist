# What changed

What changed is the host diff since the user last looked: Map growth, asset-page moves and followed names. It is not a recommendation.

### What changed

`asset_delta` returns, for one page or all, the rows after a date: observations, one
computed price row (or one per rate series), Map rows for claims added or disputed
that touch the matched entity or a one-hop neighbor, Sources whose title names the
asset (its name, or its symbol as a whole word; a ticker of three characters or
fewer also needs its exchange), and the Loupist's standing moves. Model text never
becomes a row, and Rust keeps no observation and returns no row without a fact date
and a source. The first observations of each kind (each close series on its own) are
a baseline: they never count as new and show only when their fact date falls in the
window. Each row carries `new` and `material`; for Home and briefings, assets
order by a moved read, a crossed line, a filing or results row, a material price
move, then the rest.

[Host refreshes](asset-pages.md#asset-pages) read at most four sources per class
(`plan.rs`): OpenDART filings, financials and dividends and KRX closes for Korean
equities; SEC filings, quarterly income, price history and dividends for other
equities; price history for crypto and commodities; price, `etf_info` and
`etf_holdings` for funds; Treasury rates (`RATES`, `UST`) or currency prices for rates
& FX. A Korean equity's refresh also reads the capital decisions its new filings name
and, at most once per final session, Korea Investment's investor flows
([Korean asset rows](../analysis/korea.md#korean-asset-rows)). Memory-saving turns without an as-of date record observations from the same routes (plus earnings
dates and index prices) when an acknowledged result names a followed asset; they
commit with the turn and cite its result. Observations come from structured fields
only: new filings (by OpenDART receipt or SEC accession number), a newer results
period, earnings and ex-dividend dates, Korean capital decisions and dividend record
dates, expense-ratio changes, top-10 holdings entering, leaving or moving at least one
point, Treasury 2y, 10y and 2s10s levels, and closes.

Korean equities add two observation kinds. `capital` holds one capital decision per
receipt number and ranks with filings. `investor_flows` holds one summary per final
session (dedupe key `investor_flows:{date}`) with each row's value, unit, formula and
source and the last date covered; it is never a What changed row. The page shows the
latest summary in its own section, and `asset_page_read` returns it as `investorFlows`
(values and dates without formulas) unless an as-of cutoff precedes its last date.
Observation and watcher kinds are validated in Rust since Knowledge schema version 7,
so a new kind needs no schema step ([Schema versions and upgrades](../runtime/storage-authority.md#schema-versions-and-upgrades)).

Closes are recorded only for completed sessions in the exchange's time zone (crypto
in UTC; a bar flagged final may be today's), and a later read of the same session
replaces the stored close. Price series are `price:adjusted` (adjusted closes),
`price:raw` (the provider states unadjusted) and `price` (unstated). The price row
compares the latest close with the last one on or before the date within one series,
preferring adjusted, and never mixes series. A change is material at ±5 % (equity,
fund, commodity), ±10 % (crypto), ±2 % (rates & FX prices) or ±25 bp (rate series);
a change of 20 % or more across a split recorded in an unadjusted series is not.

After a Memory-saving reply commits, Rust compares each 10-K, 10-Q (or amendment),
사업보고서 or 분기보고서 that the reply retained and a filing row links to against the
latest earlier same-form filing of that asset retained as a Source, section by
section: risk factors, management's discussion, 사업의 내용 and 이사의 경영진단.
Paragraphs are unchanged, changed (word-set Jaccard ≥ 0.6), added or removed; counts and byte spans of added and
changed paragraphs are stored. The comparison runs with no lock held, on at most four
filings per reply, 4 MiB per copy and 400 paragraphs per section. A filing row
without one says "Sections not compared yet".

## On the Map

What's new lists growth events since the last visit (the same per-device value as
the morning message), 7 or 30 days, or a date, grouped by day with one row per
subject: names, relationships, conflicts, methods and principles. Relationship and
conflict rows are built from local names and the translated relation. Rows open the
inspector or the Lens entry; rows for items no longer on the Map are dimmed. The
toolbar badge counts changes since the last visit. "See all in Map" in the morning
message opens this drawer. Schema and truncation events are excluded.

Replay reveals the current projection by first-seen timestamps; it is not a
historical snapshot or revision browser. Export creates a PNG or a replay video
(MP4 when supported, otherwise WebM) with Loupist/date/count/disclosure context.
React captures the canvas; Rust `export_save` owns the native save dialog,
size/extension validation and file write. Cancellation returns no saved path.
