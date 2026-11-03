# Data services

Bundled data tools are read-only and Loupist-scoped. Blockchain reads and the Korea Investment catalog have their own review rules below.

## Agent-facing data services

The model loads an enabled connection by exact ID with `capability_load`; `@`
mentions preload it. Entry IDs remain credential/binding keys, while plugin names
are agent-facing capability IDs. Static contracts are in `agent/tool-catalog.json`.

| Connection / pack | Agent surface |
| --- | --- |
| Native workspace | `read`, `write`, `edit`, `bash`, `grep`, `find`, `ls` |
| `datasets` | `dataset_describe`, `dataset_query`, `dataset_export`, `dataset_import` |
| `calculations` | `calculate` |
| `charts` | `chart_query`, `chart_publish`, `grid_publish` ([grid cells](../analysis/grids.md#grid-cells)) |
| `openbb` | `mcp__openbb__*` reads; nine common price, fundamentals, dividend and filing tools are active on load, others after discovery/activation |
| `world-bank` | `worldbank_series` |
| `onchain` | `defillama_read` (the wallet entry has no agent tool) |
| `krx` | `krx_daily_quote` |
| `koreainvestment` | `kis_operation_search`, `kis_read` |
| `opendart` | `dart_company_lookup`, `dart_filing_search`, `dart_filing_read`, `dart_financials`, `dart_major_report`, `dart_ownership`, `dart_dividends` |
| `web-research` | `web_search`, `web_fetch` |

Market data bundles its runtime/provider packages: base `openbb` plus only the extensions the runtime manifest reaches, not `openbb[all]` ([OpenBB runtime](../../apps/loupist/openbb/README.md#development) owns the list). `openbb.platform` supplies
keyless providers and is enabled for new Loupists; SEC additionally requires its
configured contact email. Without it, runs leave the SEC provider out and the load
result carries a model-visible notice that SEC is unavailable until the user adds
the email in Connections. Credentialed providers have separate entries. New
Loupists enable only keyless entries without required setup: Market data platform, World
Bank, DefiLlama and Web Research. Public blockchains is host-only: it starts disabled and
turns on for every Loupist when the user adds the first wallet address, and a later
opt-out stays. KIS, KRX and OpenDART appear in the optional Korea group and start disabled, with setup
only when the user opens their connection details. Rust
filters read-only tools/provider choices to the Loupist's active bindings and rejects
response-provider mismatches. KIS account reads require an account profile;
its exposed inventory contains no order operations, every account operation carries a
reviewed `holding_fields` list whose values Rust hides from every model-facing copy
([holdings redaction](../architecture.md#trust)), and `kis_read` follows
bounded continuation pages. KRX covers KOSPI and KOSDAQ with independent daily
trading service approvals. Setup verifies both markets and keeps their coverage
with the verified key; one approved market is enough to enable the connection.
Automatic lookups continue past a denied market. If the issue cannot be found
in the accessible markets, `needs_approval` names the missing KRX daily service;
an explicit `market` selects just that service. Every
OpenDART tool, including the capital-decision, ownership and dividend reads, is on the
unattended allowlist; `dart_major_report` and `dart_dividends` also serve asset-page
refreshes ([Korean asset rows](../analysis/korea.md#korean-asset-rows)).
[Financial analysis](../analysis/datasets.md#data-and-provenance) owns their result fields.
Web search routing is the Web Research `search_policy` setting
(`apps/loupist/src-tauri/src/web/search_provider.rs`). Automatic uses the Chat
provider's own search for OpenAI (ChatGPT sign-in or API key), Anthropic and the
debug-only Codex client, then keyless Exa; model-only also allows xAI and never
falls back; Exa-only skips the model. Web results, DART filing text and OpenBB
results outside a reviewed numeric-only list raise the turn's
[untrusted-text tier](../architecture.md#trust), which pauses commands and
instruction-file writes and keeps `web_fetch` to sites the turn already knows for the
rest of that turn. Saved Sources and retained datasets holding such text raise it again
when they come back.

The host writes each OpenBB tool description
(`apps/loupist/src-tauri/src/commands/tool_executor/mcp_host/openbb.rs`):
what the route returns, key arguments, granted providers with official sources
first, the cutoff note for dated routes and the host-handled `max_age`. Scraped
providers (yfinance, finviz, seeking_alpha, stockgrid, multpl, wsj) are labeled
best effort. Their failures come from individual requests (layout changes,
consent walls, blocking), so there is no active health probe: the first failure
of one in a turn returns a hint to use an official provider instead of retrying.

Sequential turns with matching Loupist/authority can reuse a warm OpenBB process.
The host resets its tools to the control surface before reuse; concurrent turns
lease separate processes. Credentials arrive through private bootstrap, and the
worker's configuration/cache directories are private. These processes are not an
app-global connection or a Chat transcript.

Successful acknowledged data reads can supply turn-local result references and
Source material. Capability administration, Skill reads and result-list discovery
are control metadata, not original data. Common arithmetic uses the host
`calculate` tool and custom work uses Bash; [Financial analysis](../analysis.md)
owns calculation and dataset/import/chart provenance.

`grid_publish` belongs to the `charts` pack with `chart_publish`; unattended runs may call
`chart_query` but neither publishing tool ([allowlist](../runtime/scheduled-runs.md#scheduled-chats)).

### Public blockchains and DefiLlama

The `onchain` plugin holds two keyless entries. `onchain.wallets` (Public blockchains,
`finance_data/onchain.rs`) reads the public balances of the wallet addresses in
[Your holdings](../runtime/storage-authority.md#where-data-lives) from fixed https hosts: `mempool.space`
for Bitcoin (confirmed funded minus spent), the Blockscout explorers for Ethereum, Base,
Arbitrum, Optimism and Polygon (the native coin plus ERC-20 tokens the explorer prices;
unpriced tokens are only counted) and Solana's mainnet RPC for SOL. Requests follow no
redirects, responses are bounded to 2 MiB, and each host gets at most one request per
second from the app; a failure on any network fails that wallet's read, which keeps its
last good snapshot. The connection has no tool: addresses and balances go straight into the
local store, and the agent sees only held symbols.

`defillama.tvl` (DefiLlama, `finance_data/defillama.rs`) reaches only `api.llama.fi` and
`stablecoins.llama.fi`. Its agent tool `defillama_read { route: protocol | chain |
stablecoin, id, start?, end? }` returns dated daily values with `max_age` and the as-of
cutoff ([rules](../analysis/datasets.md#data-and-provenance)); it is on the unattended allowlist, and
a saved result is publisher material. Watchers read it on the host for a coin's chain or protocol (value locked).

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

The generator also writes the holdings review: every account operation gets a sorted
`holding_fields` list of its response fields except identity, codes, names, dates and
the market's own price, so a new account field is hidden until reviewed in
`generate-kis-read-api.py`. A blank-parameter review also makes optional, with an empty
default, every tool parameter that upstream documents as blank (`공란`) but gives no
default: Rust refuses a required parameter with an empty value, so these were otherwise
unusable (`fid_org_adj_prc` in `investor_trade_by_stock_daily`). `--review-holdings`
applies both reviews to the existing `--output` catalog, without an upstream checkout.
Rust pins the catalog's SHA-256 (`KIS_MANIFEST_SHA256` in `finance_data.rs`), so update
that pin with a reviewed catalog change.
