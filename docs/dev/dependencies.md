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

The bundled sidecar retains OpenBB 4.7.2 and its pinned provider contracts.
FastMCP stays on 3.x and MCP on 1.x. Update those integrations together before
accepting their next majors. Never weaken read-only, credential or process
ownership checks to accommodate an update.
