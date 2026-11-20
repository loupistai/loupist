# Dependency security review

Review dependency updates through a task branch and the ordinary source, native
interaction and package gates. Lockfile changes do not authorize a release.
Do not suppress alerts merely because an automatic update cannot resolve them.

The 2026-11-12 release review updates rusqlite to 0.41.1 and its bundled
libsqlite3-sys to 0.39.1 (SQLite 3.54.1). SQLite before 3.54.0 can disclose
uninitialised process memory when a database is opened beside a crafted `-wal`
file ([RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html)).
Loupist opens databases in user-chosen folders, so the bundled library must carry
the fix. A library test checks `rusqlite::version()` against the 3.54.0 security
floor. The separate, hash-pinned Python worker and the vendored WebDriver plugin
have no rusqlite dependency and are unchanged.

The same review updates only wry's lock entry from 0.57.1 to 0.57.2. Its
[macOS fixes](https://github.com/tauri-apps/wry/releases/tag/wry-v0.57.2)
handle a nil `WKWebView.URL()` and cancel a file upload when `NSOpenPanel`
cannot be created after the running app's bundle is replaced. Keep native
macOS acceptance coverage for both cases before release.

The November 2026 review covered the 58 open Dependabot alerts:

| Lockfile | Alerts | Resolution |
| --- | ---: | --- |
| Desktop npm | 6 | Remove the shadcn CLI and its systeminformation/deepmerge closure; retain the used MIT-licensed CSS locally. Update minimatch and the Pi SDK's transitive Hono server. |
| E2E npm | 13 | WebdriverIO 10.1.0 and reviewed transitive overrides remove extract-zip and update Undici/minimatch. Browser automation remains debug-only; lifecycle scripts remain disabled. |
| OpenBB uv | 29 | Update aiohttp, Authlib, certifi, dnspython, FastMCP, joserfc, MCP, multidict, OpenTelemetry and setuptools; record compatible security floors in pyproject.toml and regenerate the manifest's lock digest. |
| Native Cargo | 9 | ZIP 9.3.0, office_oxide 0.1.16 and rustls 0.23.46. Tauri 2.12.5/updater 2.14.1 remove the updater's separate ZIP 4 dependency, including the Windows path. |
| Native Cargo, Linux only | 1 | glib 0.18.5 remains in Tauri's GTK closure, including Linux development and source-test builds. The supported Apple Silicon build does not resolve it. The advisory remains unpatched; Linux is outside the release scope. |

The glib finding is [GHSA-wrw7-89jp-8q8g](https://github.com/advisories/GHSA-wrw7-89jp-8q8g).
Do not force glib 0.20 into GTK 0.18: they use different native bindings. Verify
platform reachability with `cargo tree --locked --target <triple> -i glib@0.18.5`
before recording an alert as code not used. Do not describe the old Linux
dependency as patched.

The [extract-zip findings](https://github.com/advisories/GHSA-7pqw-9j4j-h8q3)
have no patched npm release. Removing every dependency path to it is required;
updating only WebdriverIO's direct Puppeteer browser helper leaves the older
Puppeteer peer closure installed. E2E overrides pin Puppeteer 25, current glob
and FTP/address parsers, and Cheerio's patched Undici. Verify the resolved tree
with `npm ls` and run the native suites after changing those overrides.

Provider HTTP uses rustls, except OpenDART on macOS: its servers offer only TLS 1.2
with RSA key exchange, which rustls does not implement, so `FinanceDataService` sends
OpenDART requests through reqwest's `native-tls` (Security.framework), enabled for the
macOS target only. Check with `openssl s_client -connect opendart.fss.or.kr:443 -tls1_2
-cipher ECDHE+AESGCM` before removing it; a handshake failure there means it is still needed.

An additional OSV/RustSec lockfile review updated pulldown-cmark, quinn-proto,
rustls-webpki, Tokio and webpki-root-certs. RustSec also reports maintenance
notices for transitive fxhash, paste, proc-macro-error, ttf-parser, unsafe-libyaml,
zbus_names and zvariant/derive. These have no security patch to install; track
their replacement with their owning upstreams rather than disguising versions
or forcing incompatible replacements. Re-run the review when upstreams migrate.

Direct app ZIP readers explicitly call ZIP's `validate_header_consistency`
after their entry-count bounds (and document size checks). Upgrading alone does
not enable that check on ordinary `by_index` reads. Reject mismatched local and
central filenames, duplicate names, overlapping entries and unlisted headers;
continue accepting valid CP437 names. Keep archive decompression bounded before
reading into memory, including the OpenDART corporate directory.

The bundled sidecar retains OpenBB 4.7.2 and its pinned provider contracts.
FastMCP stays on 3.x and MCP on 1.x. Update those integrations together before
accepting their next majors. Never weaken read-only, credential or process
ownership checks to accommodate an update.

## Dependabot review (2026-11-04)

The CI/toolchain review integrates updates from PRs #1 (PyInstaller 6.22.3 and
pytest 9.2.0), #4 (six Actions), #6 (rfd 0.17.2), #10 (Rust minor/patch group),
and #11 (renderer minor/patch group) into one change. Action
pins also apply to the shared setup action added after #4's original base.
The OpenBB lock digest must be refreshed with any reviewed `uv.lock` change;
#1 failed both source and package checks because its digest was stale.

Node 26 exposes its own Web Storage globals. Renderer tests explicitly bind
Vitest's jsdom storage so profiles remain isolated and do not use a Node storage
file. Development checks and package engines track the root `.nvmrc` pin.

The updated AI SDK stays `submitted` after a scheduled run starts, until content
arrives. Both `submitted` and `streaming` are active in the registry. The Stop
regression tests exercise each phase and require cancellation of the exact
native run, including retry after admission initially returns no stream.

PR #2 was closed, and Dependabot no longer proposes `@earendil-works/*` version
updates. It updated Pi AI/coding-agent to 1.4.0 while keeping
agent-core/telemetry and the signed standalone sidecar at 1.0.1. Its CI evidence
shows a missing `NOOP_TELEMETRY_METRICS` export and security-pin failures. Do not
merge that partial upgrade or relax its tests. A Pi upgrade must update the
whole package family, security pins, platform archive digests and packaged
runtime checks together, then pass host isolation and native/package tests.

## Pi 1.5.2 (2026-11-10)

The `@earendil-works/*` family, `pi-security.json`, Rust runtime validation and
both platform stage and package checks pin 1.5.2. Verify archives against the
release's [SHA256SUMS](https://github.com/earendil-works/pi/releases/download/v1.5.2/SHA256SUMS)
before staging. The archive pins undici 8.12.3 and brace-expansion 5.0.12. The
development SDK keeps the minimatch 11.0.4 override. Pi's terminal diagram
renderer replaces grok-mermaid 0.2.3 with lovely-mermaid 0.3.3 (Apache-2.0).

Pi 1.5.1 added a graceful shutdown barrier for in-progress OAuth writes and
stopped resolving OAuth tokens for public model catalogs. It also enabled idle
cache warming by default. Loupist's Chat extension declines idle cache refreshes
through Pi's warming decision hook. Pi 1.5.2 inherits these changes. Its
missing-directory session fork is an interactive CLI choice.

Pi 1.5 validates tool arguments with pi-schema, which coerces values. A lone
value becomes a one-item array, and `null` drops the field. Rust still checks
every id and reference, so a coerced value cannot pass the host. Two World
defaults followed Pi's model catalog. Baseten DeepSeek-V4.1-Flash now offers
off/high/max, so its default moves from low to high. Together no longer lists
Llama-3.3-70B. Native, package and macOS acceptance runs remain required before
release.

## Hooks lint (2026-11-11)

The renderer adds three exact-pinned devDependencies for `npm run lint:hooks`:
ESLint 10.13.0, eslint-plugin-react-hooks 7.1.1 and @typescript-eslint/parser
8.72.0. ESLint's own parser cannot read TypeScript or TSX, so the parser is the
smallest addition that lets the two hook rules run; the config enables nothing
else. The lockfile gains 77 development packages (MIT, Apache-2.0, ISC and BSD
licenses), none with install scripts, and changes no existing package. Update the
three together and keep `react-hooks/rules-of-hooks` an error.

## Local embedding model

World and knowledge search embed text with MinishLab
[blink-multilingual-64M](https://huggingface.co/minishlab/blink-multilingual-64M) at
revision `a7198ef875b9a756b8b641ab064542f7408e50f3` (2026-11-20). It replaces the
English-only potion-base-4M, whose Korean vectors were noise, so knowledge search
can find a Source written in the other language; the measurements are in
[Names across languages](../knowledge/world-search.md#names-across-languages).

The Hugging Face model card declares the MIT License; the repository has no LICENSE
file, so re-check both before moving the pin. Its token embeddings were initialized
from BAAI/bge-m3 (MIT), distilled on minishlab/tokenlearn-c4-multilingual-bge-m3
(multilingual C4) and trained on public retrieval pairs from
hotchpotch/bekko-embedding-v1-unsupervised. Loupist ships the weights, not the
training data. `src/knowledge/embed_assets/NOTICE` travels with the staged files
and `THIRD_PARTY_NOTICES.md` records the same terms.

The model is not committed. `stage-embed-assets.py` downloads the upstream
`config.json`, `tokenizer.json` and 256 MB float32 `model.safetensors` at the
revision in `src/knowledge/embed_assets/pin.json`, checks each file's sha256, and
derives the 65 MB int8 table (one float32 scale per row, levels rounded and clamped
to ±127) whose sha256 the pin also records. The derivation needs no numpy and
reproduces the pinned bytes exactly. The staged model is 73.3 MB, against 15.8 MB
for potion-base-4M. Moving the pin means re-running the
[search recall test](tests.md#search-recall) and updating both digests.

model2vec-rs moves from 0.2.1 to 0.4.0 (MIT), which reads int8 tables and reports
the model's width; the lockfile drops its ndarray and clap closure. It pins
tokenizers 0.21.4, which registers the tokenizer's 30,002 added Chinese and
Japanese words one at a time, comparing each with every earlier one. Removing
those words loads in 0.5 seconds but lowered top-1 hits on a small Chinese and
Japanese check from 15 of 16 to 13 of 16, so they stay and the app loads the model
off the launch path. The dev profile optimizes `tokenizers` and `model2vec-rs`,
where that loop is instantiated; unoptimized, a debug load takes 34 seconds.

Measured on Apple Silicon in a release build (three runs each, 2026-11-20), against
potion-base-4M with lexical-first ranking:

| | potion-base-4M | blink-multilingual-64M |
| --- | ---: | ---: |
| First load, CPU | 0.04 s | 2.3–2.5 s |
| Resident memory added by the loaded model | 48–50 MB | 223–227 MB |
| Knowledge search, 2,000 Sources and 1,200 claims, median | 51–54 ms | 53–58 ms |
| The same, 90th percentile | 55–61 ms | 60–72 ms |
