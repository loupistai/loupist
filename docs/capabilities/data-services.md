# Data services

Bundled data tools are read-only and Loupist-scoped. Blockchain reads and the Korea Investment catalog have their own review rules below.

## Asset lookup

The Following search field submits its text on Enter or Search listings. The
`asset_lookup` tool uses the same native lookup under the `openfigi.lookup` grant.
A submitted lookup sends one bounded request to OpenFIGI. The projection keeps
the listing's FIGI, name, ticker, exchange code, security type and market sector.
Asset lookup is on by default for new Loupists and for existing Loupists without
a saved choice. Each Loupist can turn it off in Connections. OpenFIGI receives
the submitted text and this device's IP address.

An optional API key raises the request limit. Saving the key activates it
immediately in the native credential store. If OpenFIGI rejects the key, the
lookup reports that error. Deleting the key restores the standard request limit
and preserves each Loupist's access setting. A key that cannot be read (no
keychain, a refused or unanswered macOS prompt, a pending key change) leaves the
public route, which answers the search. Only when the connection's non-secret
status record says a key was saved does the result say so (`savedKeyUnread` in
Following, a notice in the `asset_lookup` result); a refused read's Security
status category joins the Chat entry's `connectorKeychainStatuses`.

## Agent-facing data services

The World Bank adapter reads Indicators API errors in both successful and
client-error HTTP responses. A rejected parameter uses the tool contract's
argument-error class with the query's series, country and date, and a hint to
look up the series. A parameter name appears only when the provider identifies
one. Only recognized provider diagnostic text reaches the tool error.

OpenDART and KRX recognize structured input refusals in success and client-error
responses. The bundled OpenBB sidecar sends a fixed argument-refusal marker for
FRED, SEC and REST schema rejections. An SEC request blocked by a missing contact
uses `sec_contact_required` across the sidecar and host, with instructions to use
another listed provider and add the contact in Settings. Direct SEC web reads use
the same refusal code. The MCP host preserves these typed errors after tool
activation. Custom MCP connections recognize JSON-RPC Invalid params
and structured bad-request tool errors without exposing remote error prose.

OpenDART names companies by an 8-digit corp_code. Every DART tool resolves its
`company` through one index of OpenDART's company list (`corpCode.xml`: every filer,
listed or not, with its corp_code, Korean and English names and stock code; a 3.6 MB
archive in November 2026). The host downloads it once, in its own task with a
90-second limit, keeps the archive under the app's `cache/opendart` for up to 30 days
and refreshes it in the background after a day; a call that has a copy never waits.
The index matches a corp_code, a stock code, or a Korean or English name with spaces,
punctuation and corporate-form words (Co., Ltd., Corp., (주), 주식회사) removed. A
corp_code needs no list; a ticker or exact name resolves when one filer carries it,
or one listed filer among several; otherwise the call names candidate corp_codes.
Calls without a copy wait up to 25 seconds for the one download in flight, then
return `provider_preparing` while it continues. A `dart_company_lookup` query that names one
company that way also returns its overview as `company` (name, English and stock
names, CEO, market, industry code, address, homepage, IR page, establishment date,
fiscal year-end month) from one `company.json` read, cached a day with the profile
statement reads use. Filing archives (`document.xml`) have
a 60-second limit and stay in memory for ten minutes, two at a time, so a filing's
other pages and its dividend notice read without a second download.

OpenDART answers with a status in an HTTP 200 body. The host types each one:
013 and 014 are `empty_result`; 010, 011, 012 and 901 are `credential_rejected` with
the status; 020 is `rate_limited`; 800 is `provider_unavailable`; 021 and 100 are an
argument refusal (OpenDART's message names no field, so arguments are validated
before a request). A native read past its time limit is `provider_timeout`, which
Activity shows as a timeout. `dart_filing_search` takes `page` (OpenDART's
`page_no`, newest first) and `report_type` (`annual`, `half_year`, `quarterly`:
`pblntf_detail_ty` A001 to A003); `dart_filing_read` takes `page`, 200,000 characters
of text each, up to 20 pages. Results name the next page.

Each native vendor row declares the query parameters that carry its credential
(OpenDART's `crtfc_key`; the others send theirs in headers). A request URL becomes a
locator only through `public_locator`, which removes them, and a native result whose
`source_url` still carries one is refused before it can become a Source, citation or
export.

For Yahoo Finance income statements, balance sheets and cash flows, the query model
accepts `annual` and `quarter` with a limit of at most five. Yahoo-only cards
expose those periods and the maximum. A card shared with other providers keeps
their wider schema. If Yahoo rejects an input, the sidecar returns the field and
its constraints as typed data. Yahoo statements with zero rows return
`empty_result`. Activity classifies these as invalid input or empty result.


The model loads an enabled connection by exact ID with `capability_load`; `@`
mentions preload it. Entry IDs remain credential/binding keys, while plugin names
are agent-facing capability IDs. Each component carries the enabled entry IDs it
serves (`entry_ids`, never printed in the index), so loading `fred.macro` loads
market data. Static contracts are in `agent/tool-catalog.json`.

| Connection / pack | Agent surface |
| --- | --- |
| Native workspace | `read`, `write`, `edit`, `run_python`, `grep`, `find`, `ls` |
| `datasets` | `dataset_describe`, `dataset_query`, `dataset_export`, `dataset_import` |
| `calculations` | `calculate` |
| `charts` | `chart_query`, `chart_publish`, `grid_publish` ([grid cells](../analysis/grids.md#grid-cells)) |
| `openbb` | `mcp__openbb__*` reads; the first load activates common price, fundamentals, dividend and filing tools. A FRED grant also activates `economy_fred_series`. Other reads use `activate_tools`. A warm Chat restores its previous set |
| `world-bank` | `worldbank_series` |
| `onchain` | `defillama_read` |
| `krx` | `krx_daily_quote` |
| `koreainvestment` | `kis_operation_search`, `kis_read` |
| `opendart` | `dart_company_lookup`, `dart_filing_search`, `dart_filing_read`, `dart_financials`, `dart_major_report`, `dart_ownership`, `dart_dividends`, `dart_report_items` |
| `web-research` | `web_search`, `web_fetch` |
| `openfigi.lookup` | `asset_lookup` (core tool, gated by the Loupist’s grant) |

Each native vendor (OpenDART, KRX, Korea Investment, World Bank, DefiLlama,
OpenFIGI) has one row in `apps/loupist/src-tauri/src/finance_data/vendor.rs`.
The row records the entry ID, permitted hosts, credential requirements and native
finance read tools. The bundled catalog checks that each connector declares the
vendor's hosts and credential requirements. Tests check API hosts and tool
components against the vendor rows.

Market data bundles its runtime/provider packages: base `openbb` plus only the extensions the runtime manifest reaches, not `openbb[all]` ([OpenBB runtime](../../apps/loupist/openbb/README.md#development) owns the list). `openbb.platform` supplies
keyless providers and is enabled for new Loupists; SEC additionally requires its
configured contact email. The platform reads the non-secret setting from SEC
EDGAR even when that sibling connection is off for the Loupist. Without it, the
run omits SEC from the bootstrap, its host allowlist and the available providers
and tools. The load notice says SEC is unavailable until the user adds the email
and names `connection_setup` with the entry ID for the
[setup card](connections.md#connections-and-credentials).
Credentialed providers have separate entries. The sidecar also checks each
route's credential requirement. For providers with both public and protected
routes, such as Nasdaq and BLS, the public routes remain usable without a key.
Each run boots with the credentials it could read. A missing or unreadable key
leaves credential-dependent providers and routes out of that run. Keyless reads
keep working, including anonymous CFTC reads when the optional app token cannot
be read. The load names the connection that needs setup or whose saved key could
not be read. Legacy keys still go through **Allow access** in Connections.
New Loupists enable keyless entries without required setup, including the Market data
platform, World Bank, DefiLlama, Web Research and Asset lookup. KIS, KRX and
OpenDART start disabled in the optional Korea group. Rust filters read-only tool
and provider choices against the Loupist's active bindings and rejects provider
mismatches. The host sends saved provider settings only for providers in that
run's grant. A failed platform connect marks its Settings row unavailable until
a connect succeeds. If a price setup run could not connect to the market data platform, its Bot report
names the platform and the assets still missing a price.

Keep the OpenBB wrapper thin. It owns the grant, network, credential, failure and result boundaries. Each provider-behavior exception needs a regression test and an upstream change that would let us remove it, recorded in [What the wrapper changes](../../apps/loupist/openbb/README.md#what-the-wrapper-changes).

Congress.gov bill and amendment lookups accept a congressional identifier or its
official HTTPS API URL. The sidecar extracts the identifier before the provider
constructs the request. Keyed lookups and their follow-up reads stay on
`api.congress.gov`. Public congressional documents and SEC filings use their
respective official sites. A redirect from a credentialed provider host may
continue only on the same HTTPS origin, even when the run grants other providers.

A [saved price read](../knowledge/asset-pages.md#saved-price-reads) reuses the
original provider arguments and current grants. The replay keeps the provider's
date and currency fields and the retained dataset reference. Revoking the grant
leaves the last stored price available.
KIS account reads require an account profile;
its exposed inventory contains no order operations, every account operation carries a
reviewed `holding_fields` list whose values Rust hides from every model-facing copy
([account-field boundary](../runtime/untrusted-text.md#untrusted-text)), and `kis_read` follows
bounded continuation pages. `privacy/account_fields.rs` applies this boundary before
an account result reaches the model, retained datasets, result references, charts or
exports, independently of stored positions. Its `account_fields_visible` function is
the single decision point for a connection and always returns false. Unreviewed
account operations are refused, and account results cannot become Sources or Memory.
KRX covers KOSPI and KOSDAQ with independent daily
trading service approvals. Daily reads skip provider requests for published KRX holidays while keeping them in `missing_dates`, and report unavailable dividend and split values as null.
Setup verifies both markets and keeps their coverage
with the verified key; one approved market is enough to enable the connection.
Automatic lookups continue past a denied market. If the issue cannot be found
in the accessible markets, `needs_approval` names the missing KRX daily service, with
the next step KRX's reader puts on the error (another vendor supplies its own);
an explicit `market` selects just that service. Every
OpenDART tool, including the capital-decision, ownership and dividend reads, is on the
unattended allowlist; dividend notice searches use the current Seoul date.
[Financial analysis](../analysis/datasets.md#data-and-provenance) owns their result fields.
Web search routing is the Web Research `search_policy` setting
(`apps/loupist/src-tauri/src/web/search_provider.rs`). Automatic uses the Chat
provider's own search for OpenAI (ChatGPT sign-in or API key), Anthropic and the
debug-only Codex client, then keyless Exa; model-only also allows xAI and never
falls back; Exa-only skips the model. Each provider's `native_search` in the
[provider manifest](../runtime/providers.md#provider-manifest) says which. Web results, DART filing text and OpenBB
results outside a reviewed numeric-only list raise the turn's
[untrusted-text tier](../architecture.md#trust), which pauses custom-connection calls,
selected schedule operations and instruction-file writes. For the rest of the turn,
`web_fetch` can open exact links from delivered text under the private-text check,
and composed URLs follow the known-site rule. Saved Sources and retained datasets
holding such text raise the tier again when they come back.

Search and reading follow pi-web-access 0.47's behavior where it applies to a
host-side reader. Exa results take `URL:` and `Published:` only from each result's
header, so a date quoted in page text never dates a hit. A hit without a usable
title (none, punctuation alone, or a placeholder such as Exa's `N/A`) is named by
its host and path (`source_title` in `web.rs`), and a fetched page keeps its own
title, else that name. A throttled Exa reply (`_meta["ai.exa/rateLimited"]`) counts as rate
limited and falls back like a 429. `web_fetch` (`apps/loupist/src-tauri/src/web.rs`)
decodes text the way a browser picks an encoding: byte-order mark, HTTP charset,
an HTML `<meta>` charset, UTF-8, then chardetng's guess. The guess reads
undeclared EUC-KR or Shift_JIS pages and CSV files. A few invalid bytes become
U+FFFD with the `decoding_replaced` warning. One request budget covers every
redirect hop. A Cloudflare challenge (header or interstitial) fails with a clear
error instead of returning the interstitial. Inline `data:` payloads become a
media-type and size marker. A page with almost no readable text but more than
three executable scripts gets `javascript_required`. Publication comes from the
page's article meta tags, then its schema.org JSON-LD `datePublished`, never a
modified time. Anthropic native search uses the basic `web_search_20250305` tool
on the cheapest available model (Haiku 5.5 first, at low effort) with
`max_uses: 3`. Absolute `page_age` text becomes a date.

The host writes each OpenBB tool card
(`apps/loupist/src-tauri/src/commands/tool_executor/mcp_host/openbb.rs`). The
description says what the route returns, key arguments, granted providers with
official sources first, the listing-market limits in the provider note itself,
the cutoff note for dated routes and the host-handled
`max_age`. The parameter schema is pruned to the run's enabled providers. A
property is dropped only when its title names providers, every description
sentence carries a `(provider: …)` marker, none of those providers is enabled and
upstream does not require it; upstream titles a standard field with the provider
that redefined it (`symbol` is titled `intrinio`), so the title alone never drops
one. Vendor keys (`x-widget_config`, provider-keyed `choices`) and titles are
removed and description sentences marked for other providers go. Listed choices
become the property's enum only when every enabled provider that accepts it lists
them, so a provider without choices (TMX intervals) keeps the upstream union. When
a provider rejects some of the union's values, the description names up to
twelve rejected values. Longer disjoint lists
such as screener industries keep the bare union. The sidecar also reads required
fields and literal values from each enabled fetcher's query model. A field
becomes required in the shared card when every offered provider requires it.
When requirements differ, the field identifies the providers that require it.
ECB balance of payments and IMF trade validators require fields behind optional
upstream defaults. The IMF indicator description gives its qualified symbol
format. Cboe's index constituents list its index codes, and TMX uses the
identifiers returned by index discovery, such as `^TSX`. USDA crop reports
list the commodity categories from the provider model.

The sidecar turns an empty OpenBB response into a typed `empty_result` before
MCP can reject a bodyless response. Validation failures in provider query
models name a known request field without forwarding validator text or inputs.
A successful response with null scalar measures retains those nulls. The MCP
output schema accepts null in number, integer, and string fields. Other
constraints, such as required keys, container shapes, enums, and wrong non-null
types, remain enforced.
No model-facing text names the runtime; the model and the user see "market
data", and the capability index lists no provider ids. Scraped providers
(yfinance, finviz, seeking_alpha, stockgrid, multpl, wsj) are labeled best
effort. Their failures come from individual requests such as layout changes,
consent walls or blocking. Finviz is excluded from the key metrics route because
its pinned fetcher cannot read the quote page. Without provider setup, key
metrics use Yahoo Finance. For a provider error, recovery names up to four
enabled alternatives on the same tool, with official sources first. When the
tool has no alternative, the reply keeps the existing failure guidance. For an
explicitly unadjusted Yahoo daily equity
read, the sidecar checks split events through the present before cutting the
chart to the requested range. It uses Yahoo's Close if the share basis is
unchanged. A later split, missing split information, or a non-daily interval
returns the supported adjustments `splits_only` and `splits_and_dividends`.
TMX is never described as generally preferred.

Sequential turns with matching Loupist/authority can reuse a warm OpenBB process.
The host resets its tools to the control surface before reuse; concurrent turns
lease separate processes. Credentials arrive through private bootstrap, and the
worker's configuration/cache directories are private. These processes are not an
app-global connection or a Chat transcript.

OpenBB imports its provider packages before it answers `initialize`: 21-34 s at
load 34-51 and 39-50 s at load 100-115 on an 8-core Apple Silicon Mac. The
bundled runtime therefore has 120 s to start (custom MCP servers keep 30 s), and
a start that runs out says market data was still starting, not that a request
timed out. The pool (`mcp/pool.rs`) owns every start. A turn that finds a start
with its authority under way waits for it instead of starting another, and a
start whose turn stopped waiting stays for the next turn. Two seconds after
launch the app starts one process for the first available Loupist whose enabled
market-data grants map no saved key, so it never reads the keychain at launch;
with no such grant nothing starts. A process that has served no turn may go to
any Loupist whose authority matches; after its first turn it belongs to that
Loupist. Unleased, it expires after the ordinary 8 idle minutes. App exit stops
idle processes and aborts starts under way, which ends their process groups.

Successful acknowledged data reads can supply turn-local result references and
Source material. Capability administration, Skill reads and result-list discovery
are control metadata, not original data. Common arithmetic uses the host
`calculate` tool and custom work uses `run_python`. [Financial analysis](../analysis.md)
owns calculation and dataset/import/chart provenance.

`grid_publish` belongs to the `charts` pack with `chart_publish`; unattended runs may call
`chart_query` but neither publishing tool ([allowlist](../runtime/scheduled-runs.md#scheduled-chats)).

### Public blockchains and DefiLlama

The `onchain` plugin's keyless `defillama.tvl` (DefiLlama, `finance_data/defillama.rs`) reaches only `api.llama.fi` and
`stablecoins.llama.fi`. Its agent tool `defillama_read { route: protocol | chain |
stablecoin, id, start?, end? }` returns dated daily values with `max_age` and the as-of
cutoff ([rules](../analysis/datasets.md#data-and-provenance)); protocol and chain IDs cannot be dot path segments.
It is on the unattended allowlist,
a saved result is publisher material, and like other provider reads it is refused after a
custom-connection result in the same turn, since its `id` is part of the request path.

### Maintaining the Korea Investment read catalog

`marketplace/kis-read-api-v1.json` is generated from a pinned upstream revision,
not refreshed at runtime. From a clean checkout of that pinned upstream:

```sh
python3 apps/loupist/scripts/generate-kis-read-api.py \
  --source /absolute/path/to/open-trading-api --check
```

After a reviewed pin update, omit `--check` and review the generated catalog with
connector tests. The generator rejects a dirty/wrong revision, non-GET operations
and unreviewed read/write inventory changes.

The generator also writes the account-field review: every account operation gets a sorted
`holding_fields` list of its response fields except identity, codes, names, dates and
the market's own price, so a new account field is hidden until reviewed in
`generate-kis-read-api.py`. A blank-parameter review also makes optional, with an empty
default, every tool parameter that upstream documents as blank (`공란`) but gives no
default: Rust refuses a required parameter with an empty value, so these were otherwise
unusable (`fid_org_adj_prc` in `investor_trade_by_stock_daily`). `--review-holdings`
applies both reviews to the existing `--output` catalog, without an upstream checkout.
Rust pins the catalog's SHA-256 (`KIS_MANIFEST_SHA256` in `finance_data.rs`), so update
that pin with a reviewed catalog change.
