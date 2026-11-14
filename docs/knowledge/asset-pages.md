# Asset pages

An asset page is the host's read of one instrument: quotes, filings, flows, the thesis and the lines the user set. It never places an order.

## Asset pages

Each Loupist keeps one page per asset (`src-tauri/src/knowledge/assets/`), keyed
`ast_` + the first 16 hex digits of sha256(`SYMBOL:MARKET`). A page is created on
its first visit (`asset_page_get` by symbol and market), by a reason or line saved
in Chat, by a briefing's refresh, a watch check or the results task for followed assets,
or when a turn records an observation for a followed asset. It opens over the current tab like "What
{Loupist} knows": the thesis card (the read, its ladder, the latest move with Undo,
reasons, and lines with their last reading), for an equity the
[results card](../analysis/results.md#results-events) while an event is due within 14 days or was reported in
the last 30, What changed (since the last look, 7 or 30 days; 50 rows, then Show
earlier), thesis history (10, then Show all), Earlier results (up to four filed events,
each expanding to its card), At a glance, a Korean equity's
[investor flows](../analysis/korea.md#korean-asset-rows), what the Loupist checks (failed or
ungranted sources and what this class is not checked for yet), Watch for (a switch per
[watch kind](watchers.md#watchers) that applies) and supplier/customer counts from the Map. Focusing a control below the fold scrolls it into the page. "Last looked" is a
per-device value in `localStorage` (`loupist.assetVisit.v1`), written on leaving
after at least 10 seconds. Holdings quantities and values never enter pages, rows,
prompts or messages; a page shows only whether the user watches or holds the asset.

The class is equity, crypto, fund, commodity, rates & FX, real asset, prediction
market or private. The first source wins: the user's choice (`asset_class_set`),
the market code (crypto venues; `CME`, `COMEX`, `NYMEX`, `ICE`; `FX`, `FOREX`,
`RATES`, `UST` or a six-letter currency pair; `REAL`; `POLYMARKET`, `KALSHI`;
`PRIVATE`), a first-refresh `etf_info` lookup for a non-Korean equity (rows mean a
fund), the class stated on the World Asset matched by name, symbol or local name,
then equity. Real assets, prediction markets and private assets have no automatic
reads; their rows come from Sources and the Map. For `RATES` and `UST`, At a
glance's level is specifically the 10-year Treasury yield (`series: 10y`, named in
the facts); without that series the level is absent.

The Loupist database holds `knowledge_asset_pages` (identity, class and its source,
matched entity, the read and the move that set it, per-check refresh status,
revision), `knowledge_asset_observations` (kind, fact date, typed facts of at most
4,096 Unicode scalar values in serialized JSON, evidence, origin `refresh`, `chat`
or `briefing`, baseline flag; unique per
dedupe key). Oversized rows retain their evidence and structured facts with a
`truncated: true` marker: long text ends with `… [truncated]`, and large containers
retain a prefix; non-object JSON is retained under `preview` with the same marker.
The host never silently drops the row. The database also holds
`knowledge_asset_moves` and `knowledge_asset_section_deltas`.
Unknown stored classes, class sources or verdicts and malformed refresh state
fail exact reads and writes instead of being presented as default values. Page
and move listings skip unreadable rows and log once without their contents, so
the remaining assets and history stay available.

"Ask about it" fills the Chat composer with the asset's name, symbol and market
as ordinary prose. Results questions and requests to change sensitivity rates use
the same readable identity. These actions never send the question. Chat does not
infer an asset card or request a quote from ticker text.

Failed change or results reads show an error and a retry instead of an empty result. Check now reports failure if its change list could not be read. A failed reason or line archive keeps the entry visible and shows an error.

The user's reasons and lines are [Thesis and the read](thesis.md).

### Host reads and refresh

Rust creates [asset pages](asset-pages.md#asset-pages), records observations,
owns the read and its moves and decides what is new; React submits typed intent
through `asset_page_get` (by `assetId`, or by symbol and market, creating the page
on the first visit), `asset_delta` (read-only; one page or all, after `sinceMs`),
`asset_refresh` (Check now), `asset_class_set`, `asset_verdict_undo` (by `moveId`)
and `asset_page_seen` (opening the page acknowledges its crossed lines). Check now,
class changes and Undo hold a Memory-mutation maintenance activity.

A refresh makes no model call. It runs the class's planned reads (at most four; a
Korean equity adds up to two capital decisions and, when due, its investor flows:
[Korean asset rows](../analysis/korea.md#korean-asset-rows)) through the ordinary tool executor
(`AppToolExecutor::for_host`, policy without Memory, in Bot) with the
Loupist's captured connection grants: native OpenDART, KRX and Korea Investment reads
need their own grant and the unattended allowlist; OpenBB reads need a grant the bundled runtime serves and use a provider the Loupist enabled
(a named provider stays pinned; otherwise the shared
[market-aware order and bounded fallback](../capabilities/data-services.md#agent-facing-data-services)
apply). Identical reads from the last 30
minutes are reused (`max_age` 1800). Each source's status (`ok`, `failed`,
`needs_connection`) is stored on the page; one failed source does not fail the
refresh. Briefings refresh followed assets before [scheduler admission](../runtime/scheduled-runs.md#scheduled-chats).
A symbol no enabled provider can interpret for this listing records
`needs_connection`, not a retryable failure. The page keeps its existing connection
prompt ("Turn on {connection} in Connections to check {kind}"), even when the
platform is already on but cannot serve that listing; it adds no price observation.
Change rows and briefing context list each source once, even when a calculated
price move cites two rows of the same provider read; both endpoint observations
and their evidence remain stored.

At commit, Rust records the turn's observations and re-checks each staged
`thesis_update` against the committed page, applying only the first per asset. An
update that no longer fits (the user undid a move or archived a reason meanwhile) is
not saved: the message's `unsaved_theses` (also on the completed event) makes the
receipt say "{name}: read not changed (the page changed first)", and a
`thesis_update` tool-trace entry (component `asset-commit`, error
`thesis_not_saved` with the refusal code) reaches the run log. A turn with saving
off or an `as_of` cutoff records no observations and moves no read; under a cutoff
`asset_page_read` returns only reasons and lines written by then and rows and line
readings dated on or before it, without the current read or move rows. Filing sections are compared after the commit.

## Code

- Asset pages: `src/components/assets/`, `src/contracts/assets.ts`, `src-tauri/src/knowledge/assets/`, `src-tauri/src/knowledge/numbers.rs`, `src-tauri/src/commands/assets.rs`, `src-tauri/src/commands/tool_executor/assets.rs`
