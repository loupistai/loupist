# Loupist OpenBB runtime

This project freezes OpenBB Platform and the official OpenBB MCP server into a
private, read-only sidecar. It is not a public CLI. Rust owns process creation,
credentials, capability checks, result limits, and shutdown.

The frozen OpenBB distribution and its pinned provider packages are trusted
product code. Exact-host checks cover the audited Python HTTP stacks and catch
undeclared provider egress, but they are not an OS network sandbox against a
compromised native dependency. HOME/cache redirection, private bootstrap,
process-tree cleanup, MCP filtering, and result bounds remain enforced at the
host boundary.

## Wire contract

The process reads one newline-terminated Loupist bootstrap frame before the MCP
client sends `initialize`:

```json
{"type":"loupist.bootstrap","protocol_version":1,"run_id":"run-1","scratch_dir":"/private/run/directory","credentials":{},"settings":{"allowed_categories":["equity"],"enabled_provider_ids":["yfinance"],"allowed_network_hosts":["consent.yahoo.com","fc.yahoo.com","finance.yahoo.com","guce.yahoo.com","markets.businessinsider.com","query1.finance.yahoo.com","query2.finance.yahoo.com"],"provider_config":{}}}
```

The frame is at most 65,536 bytes. `scratch_dir` must already exist, be an
absolute non-symlink directory, and be private to the current user. Credential
keys are OpenBB credential names authorized by the enabled provider entries in
`runtime-manifest.json`. After the newline, stdin and stdout speak the official
MCP stdio protocol.

DNS, connection and timeout exceptions are classified before OpenBB's REST
handlers mask them. The MCP error result carries only the fixed structured
marker `{"loupist_error":{"kind":"offline"}}`; Rust converts it to the same
`offline` broker failure as native data reads, without forwarding exception
text, URLs or credentials. HTTP status errors, invalid arguments and rejected
network policy remain distinct from offline reads.

The wrapper points OpenBB's home, settings, caches, temporary files, Matplotlib,
Numba, and Python bytecode at the run scratch directory before importing any
OpenBB module. Credentials are installed into OpenBB's process-local user
settings service without writing `user_settings.json`; they are never accepted
in command-line arguments or environment variables. Provider configuration is
strictly mapped by the manifest. In particular, the SEC contact email replaces
all bundled SEC request identities, and Tradier's account type is limited to
`sandbox` or `live`.

Provider authority is also enforced for Tools that do not expose a top-level
`provider` input. The runtime manifest explicitly classifies the audited local
analytics Tools and maps direct provider Tools to their one implicit provider.
Unknown providerless routes are omitted. For example, the `imf_utils` surface is
not discoverable unless the Loupist has enabled the `imf` provider.

When a generated OpenAPI output schema exceeds the host's 64 KiB limit, the
wrapper advertises a generic object schema and preserves FastMCP's result
wrapping. The schema substitution preserves structured results. For routes with
an implicit provider, the wrapper also fills in a missing provider receipt.

The source manifest names the executable `loupist-openbb`. Windows is not
supported in this release.

## What the wrapper changes

Keep the wrapper small. Loupist owns the grant, network, credential, failure and result boundaries. A change to a provider's inputs, data or interpretation of a response is an exception. Record the upstream behavior, the test and the upstream change that will let us delete it.

| Security boundary or host contract | Where and why |
| --- | --- |
| Private startup frame | `bootstrap.read_bootstrap` consumes one bounded line before MCP, validates its fields and leaves subsequent bytes for stdio. `server.run` clears bootstrap credentials after setup. |
| Private scratch | `bootstrap.configure_scratch_environment` isolates settings, caches, bytecode and temp files before OpenBB imports. Startup disables automatic builds, updates and debug output. |
| Grant validation | `manifest.authorize_bootstrap` checks categories, enabled providers, their exact host union and which credentials and config fields they can receive. |
| Process-local settings | `server.apply_credentials` supplies the SDK and REST with in-memory settings. `create_server` passes the configured Tradier account type and requires a configured contact for SEC access. |
| Read-only routes | `server.retain_read_only_routes` isolates the router and admits granted categories, GETs and reviewed POSTs. Providerless routes require a manifest entry as local analysis or an enabled implicit provider. Rust checks provider choices. |
| MCP discovery | `server.create_server` starts with discovery controls, pages listings, masks errors and disables prompts, resources and skill installation. MCP 1.4.1 treats an empty module exclusion map as its default. A nonmatching entry keeps reviewed local analytics discoverable (`openbb_mcp_server/utils/fastapi.py:42–52`). |
| Tool metadata and size | `server.mark_registered_tools_read_only` annotates admitted tools and replaces output schemas over 64 KiB with a generic object, retaining FastMCP's wrapping flag. Rust enforces result size. |
| Provider receipts | `server._canonicalize_implicit_provider_result` validates the successful result against the grant, rejects contradictory or non-object receipts and preserves errors. The provider field it adds is recorded below. |
| Exact-host egress | `network.apply_network_policy` checks HTTPS origins on OpenBB request helpers and imported aliases, requests, aiohttp, httpx, urllib and curl_cffi. It disables proxy overrides and curl redirects and permits in-process ASGI dispatch. |
| Credential origin | `destinations.guard_tool_destinations` and `network` bind a credentialed call and its redirects to the approved origins. Congress.gov's pinned bill and amendment fetchers need an upstream fix to keep keys on the API origin on initial and follow-up reads. The guard accepts identifiers and canonicalizes official API URLs. |
| Public document origins | `destinations.checked_arguments` and `check_tool_destination` restrict congressional document and SEC filing reads to their official sites, including follow-ups. The pinned document readers do not consistently validate the parsed origin. Only the listed committee membership and search requests receive exceptions. |
| Result size | `server.mark_registered_tools_read_only` measures each result as the MCP stdio writer serializes it. One over `failures.MAX_RESULT_BYTES` (the host's 32 MiB frame less 64 KiB for the envelope) becomes the typed `result_too_large` refusal with its size, so that call fails alone and the host's session keeps serving. On 2026-11-24 an undated EIA petroleum status report (57.1 MB) and Federal Reserve Svensson curve (73.2 MB) were refused this way, and dated reads of both fit (`test_result_size.py`). |
| Failure categories | `failures.classify_rest_failures` and `call_tool_with_failures` carry typed offline, key, plan and adjustment failures through REST and MCP. They use fixed markers and bounded cause inspection to keep provider text and credentials out of diagnostics. |

Provider patches below refer to installed files under Python 3.12's `site-packages/` for the versions in `uv.lock`. Wrapper files are in `src/loupist_openbb/` and tests are in `tests/`.

| Wrapper file and function | Pinned OpenBB behavior | What changes for the user | Test | Delete when upstream… |
| --- | --- | --- | --- | --- |
| `server.py:mark_registered_tools_read_only` | Alpha Vantage 1.6.1, `openbb_alpha_vantage/models/equity_historical.py:40–43`: `default="splits_only"`. Its equity and ETF routes share this fetcher. | An omitted daily adjustment requests unadjusted bars, so a recent price read can use the free endpoint. Explicit adjustments keep their meaning. | `test_alpha_vantage.py::test_alpha_vantage_daily_prices_and_plan_refusals` (equity) | …supports a configurable raw daily default for both routes, or uses it by default. |
| `server.py:apply_alpha_vantage_history` | Same fetcher, lines 134–138 and 198–203: `outputsize=full`, even for raw daily prices and daily downloads used for resampling. | Raw daily reads request the compact window for all keys, including paid keys. Weekly and monthly reads keep full history. | `test_alpha_vantage.py::test_alpha_vantage_daily_prices_and_plan_refusals` | …can request compact raw daily history without truncating the feed for resampling. |
| `server.py:apply_alpha_vantage_history` (CSV callback) | Same fetcher, lines 146–155 and 203–207: `warn(str(result.get("Information")))`, then pass the body to the CSV reader. | A premium-endpoint notice becomes a paid-plan refusal. The free key's one-request-per-second notice, which also offers premium endpoints, is retried once after 1.1 s and then becomes a `rate_limited` refusal. Valid adjusted data still succeeds. | `test_alpha_vantage.py::test_alpha_vantage_daily_prices_and_plan_refusals` | …raises a distinct subscription error before decoding CSV, distinguishable from an empty series or throttling. |
| `server.py:mark_registered_tools_read_only` and `yahoo_history.py:apply_yahoo_history` | Yahoo Finance 1.6.3, `openbb_yfinance/models/equity_historical.py:76–78,143–153`: only `"splits_only", "splits_and_dividends"`. `openbb_yfinance/utils/helpers.py:531–537` downloads through the present with `actions=actions`. | Explicit unadjusted daily equity history returns chart Close only when split evidence supports the as-traded share basis for every symbol. It requests actions for verification, drops unrequested action columns, and reports the supported alternatives for other intervals or inconclusive data. | `test_yfinance.py::test_yahoo_daily_chart_close_and_split_boundary` | …exposes an as-traded daily mode with a verified share basis (or authoritative raw bars), refuses unverifiable values and honors the requested action columns. |
| `yahoo_history.py:apply_yahoo_history` (end-date filter) | `openbb_yfinance/utils/helpers.py:589–596` applies the upper bound only for `to_datetime(end_date) > to_datetime(start_date)`. | A one-day raw lookup returns only that day's bars, after later splits have been checked. | `test_yfinance.py::test_yahoo_daily_chart_close_and_split_boundary` | …honors an inclusive end date for a one-day lookup after downloading the split history. |
| `server.py:mark_registered_tools_read_only` and `failures.py:_request_refusal` | Yahoo Finance 1.6.3 limits the three statement query models to five rows. OpenBB's shared REST schema omits that limit, and FastMCP masks the provider's query-validation error. Yahoo may also return a successful response with zero rows. | Yahoo-only cards advertise the five-row maximum. Argument and empty-result refusals carry typed facts across REST and MCP. | `test_statement_reads.py::test_keyless_statement_reads_validate_and_fetch_through_mcp` | …exposes per-provider statement bounds and carries safe argument and empty-result diagnostics through MCP. |
| `failures.py:reject_no_content`, `failures.py:_request_refusal`, and `server.py:mark_registered_tools_read_only` | OpenBB Core 1.6.13 sends a bodyless 204 for `EmptyDataError`. The Seeking Alpha EPS and sales estimate fetchers raise a generic OpenBB exception for no estimates. An OpenBB envelope can also carry empty rows. | All become the existing typed empty-result refusal before output validation. Seeking Alpha's exact no-estimate errors are recognized only on those two routes. | `test_tool_schemas.py::test_openapi_tool_delivers_nulls_and_reports_empty_answers` | …preserves a machine-readable empty result from provider to MCP. |
| `schemas.py:allow_nullable_output_scalars` | The MCP SDK validates FastMCP's decoded structured JSON against the advertised OpenAPI-derived output schema. OpenBB can serialize null for a measure declared as only a number or string. | Generated scalar schemas accept null while the JSON payload and remaining validation stay intact. The existing large-output schema budget still applies. | `test_tool_schemas.py` | …advertises OpenBB's null scalar values accurately in its output schemas. |
| `schemas.py:apply_query_metadata` and `failures.py:_request_refusal` | OpenBB merges provider inputs into one REST schema and may leave fetcher requirements optional or drop literal choices. Some validators put the field error inside another exception or omit a field location. | The host uses enabled fetchers' metadata to shape cards. Reviewed omissions cover ECB country, IMF trade countries, and IMF and TMX identifier forms. Arguments are rejected with safe field identifiers. | `test_server_policy.py::test_official_server_is_admin_only_and_reaches_every_manifest_grant` | …preserves each fetcher's required fields, literals, and field-level validation failures through MCP. |
| `server.py:apply_fred_error_guard` | FRED 1.6.2, `openbb_fred/utils/rate_limiter.py:150–156,168–181` checks for 429 or a rate-limit message. `openbb_fred/models/series.py:148–168` can `return {}` on other error bodies. | A rejected key fails connection verification with a format or rejection reason. Other FRED error bodies fail the read. A genuine empty series or successful observation keeps its meaning. | `test_failures.py::test_fred_key_errors_fail_with_a_fixed_reason_instead_of_an_empty_series`, `test_fred_other_errors_fail_without_provider_text` | …rejects non-rate-limit error envelopes before parsing/caching data and distinguishes malformed or rejected keys from other errors without echoing secrets. |
| `server.py:_canonicalize_implicit_provider_result` | IMF 2.1.3 returns `OBBject(results=dataflows)` or plain lists (`openbb_imf/imf_router.py:129,220–233`). Quantitative 1.6.2 reads Fama–French factors and returns `OBBject(results=results)` (`openbb_quantitative/quantitative_router.py:140–161`). | The successful MCP receipt carries the manifest provider even when the route omitted it. The wrapper rebuilds the tool result from structured content, preserving metadata and data values. | `test_server_policy.py::test_implicit_provider_receipt_is_canonical_and_contradictions_fail`, `test_dynamic_mcp_call_enforces_manifest_implicit_provider_contract` | …returns the canonical source in structured MCP receipts across the registered single-provider routes. Keep validation against the grant. |
| `server.py:apply_sec_contact_email` | SEC 1.6.7, `openbb_sec/utils/definitions.py:9–18`, `utils/form4.py:8–9` supply fixed identities. `models/latest_financial_reports.py:117–118,144` sends its own `headers=SEARCH_HEADERS`. | SEC receives the user's configured contact on all request paths. The request guard returns the typed `sec_contact_required` refusal before fetching if the contact is missing. | `test_credentials.py::test_credentials_feed_rest_context_without_persisting`, `test_server_policy.py::test_official_server_is_admin_only_and_reaches_every_manifest_grant` | …accepts one process-local contact setting and uses it on every SEC request, with a typed refusal when it is missing. Loupist still requires the contact in the grant. |
| `server.py:_available_route_providers` | OpenBB MCP 1.4.1 builds discovery and provider schemas from REST routes. OpenBB Core 1.6.13 checks fetcher credentials only when executing the call. | The sidecar narrows discovery and provider choices to the bootstrap grant and each fetcher's credential requirements. Mixed providers keep their anonymous routes. | `test_server_policy.py::test_official_server_is_admin_only_and_reaches_every_manifest_grant` | …supports a per-run provider grant and filters discovery and schemas by each route's credential requirements. |
| `server.py:_available_route_providers` (Finviz key metrics) | Finviz 1.5.1's `openbb_finviz/models/key_metrics.py` swallows quote-page HTTP 403 responses and raises `EmptyDataError` when all symbols fail. OpenBB responds with a bodyless 204, which FastMCP reports as missing structured output. | Finviz is omitted from key metrics discovery and dispatch. Its other routes remain available under the usual provider grant. | `test_server_policy.py::test_official_server_is_admin_only_and_reaches_every_manifest_grant` | …retrieves key metrics successfully through an authorized Finviz endpoint. |
| `server.py:_available_route_providers` (FINRA short interest) | FINRA 1.6.1's `openbb_finra/models/equity_short_interest.py` is a synchronous fetcher whose `utils/data_storage.py:prepare_data` downloads every bi-weekly short-interest file since July 2021 (128 files of about 2 MB on 2026-11-23) into a cache under the run's scratch. OpenBB Core calls synchronous fetchers on the server's event loop, so the server answers nothing for about three minutes and requests sent meanwhile time out; cancellation cannot interrupt it. | FINRA is omitted from short-interest discovery and dispatch. No other provider serves the route, so it is not offered. FINRA's OTC aggregate route remains available. | `test_server_policy.py::test_official_server_is_admin_only_and_reaches_every_manifest_grant` | …fetches only the requested settlement dates, asynchronously or off the event loop, with a cache that outlives one run. |

**Packaging.** `build_sidecar.py` generates the SDK, checks the lock digest, package versions, provider inventory and credential fields, and freezes the runtime with its manifest and license archive. It strips build-machine paths from distribution metadata. The package entry point and version, source/frozen manifest lookup, PyInstaller hooks, platform checks and signing belong to the build. `materialize.py` copies bundle-internal symlink targets into place and rejects links outside the bundle. Packaging tests cover collection, license contents and symlink handling.

**On an OpenBB version bump**

1. Disable each provider patch in turn and run its listed tests against the new pin. Keep the host boundaries active.
2. Delete each patch whose tests pass without it, and remove its ledger row.
3. Add a provider patch only with a ledger row, a regression test and a specific upstream change that would retire it.

## Development

Run the live inventory manually from the repository root with the pinned
development environment. The replay boots a source sidecar in private scratch
with no credentials. It discovers all keyless routes and emits one JSON line
per provider and tool, followed by totals. Unit tests and CI stay offline.

```sh
apps/loupist/openbb/.venv/bin/python apps/loupist/scripts/replay-openbb-keyless.py --allow-network
```

Use `--original-inputs --as-of 2026-11-23` to replay the v0.1.4 audit fixtures,
including the requests that hit undocumented query requirements. The replay
calls the sidecar directly and uses its own deadlines. It captures neither the
app's broker output nor provider response bodies.

```sh
uv sync --locked --python 3.12
uv run --locked pytest
uv run --locked ruff check .
uv run --locked python build_sidecar.py --check
```

`build_sidecar.py` runs `openbb-build` and creates a PyInstaller one-directory
bundle named `loupist-openbb`. Platform staging scripts live in the parent
`scripts/` directory.

The sidecar depends on base `openbb` plus only the extensions
`runtime-manifest.json` can reach: the package of every listed provider and the
`econometrics`, `quantitative` and `technical` routers named in
`allowed_categories`. It does not use `openbb[all]`, which adds
`openbb-charting` and Plotly; no Loupist Tool asks OpenBB for a chart.
`openbb-platform-api` stays because base `openbb` requires it. Adding a provider
or category means pinning its package in `pyproject.toml`, running `uv lock`,
and updating the manifest (including `uv_lock_sha256`). `tests/test_manifest.py`
fails when installed provider packages and the manifest differ, and
`tests/test_server_policy.py` starts the real server and fails unless every
allowed category and every provider is reachable through a Tool.

The bundle also contains `THIRD_PARTY_LICENSES/python-distributions.json` and
its referenced compliance archive. The builder resolves the locked runtime
dependency closure, copies every distribution's full metadata plus
package-level license/NOTICE files, adds the CPython and PyInstaller license
payloads, and verifies every archived byte by SHA-256.
