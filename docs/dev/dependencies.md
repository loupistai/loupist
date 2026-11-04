# Dependency security review

Review dependency updates through a task branch and the ordinary source, native
interaction and package gates. Lockfile changes do not authorize a release.
Do not suppress alerts merely because an automatic update cannot resolve them.

The November 2026 review covered the 58 open Dependabot alerts:

| Lockfile | Alerts | Resolution |
| --- | ---: | --- |
| Desktop npm | 6 | Remove the shadcn CLI and its systeminformation/deepmerge closure; retain the used MIT-licensed CSS locally. Update minimatch and the Pi SDK's transitive Hono server. |
| E2E npm | 13 | WebdriverIO 10.1.0 and reviewed transitive overrides remove extract-zip and update Undici/minimatch. Browser automation remains debug-only; lifecycle scripts remain disabled. |
| OpenBB uv | 29 | Update aiohttp, Authlib, certifi, dnspython, FastMCP, joserfc, MCP, multidict, OpenTelemetry and setuptools; record compatible security floors in pyproject.toml and regenerate the manifest's lock digest. |
| Native Cargo | 9 | ZIP 9.3.0, office_oxide 0.1.16 and rustls 0.23.46. Tauri 2.12.5/updater 2.14.1 remove the updater's separate ZIP 4 dependency, including the Windows path. |
| Native Cargo, Linux only | 1 | glib 0.18.5 remains in Tauri's GTK closure. Neither the supported Apple Silicon build nor the future Windows target resolves it. Linux is outside the release scope; revisit before adding Linux builds. |

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

PR #2 is deferred: it updates Pi AI/coding-agent to 1.4.0 while keeping
agent-core/telemetry and the signed standalone sidecar at 1.0.1. Its CI evidence
shows a missing `NOOP_TELEMETRY_METRICS` export and security-pin failures. Do not
merge that partial upgrade or relax its tests. A Pi upgrade must update the
whole package family, security pins, platform archive digests and packaged
runtime checks together, then pass host isolation and native/package tests.
The pinned 1.0.1 family and existing security overrides remain in place.
