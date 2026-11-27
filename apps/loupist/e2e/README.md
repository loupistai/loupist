# Loupist native UI and E2E

This guide owns app launch, the tool that drives it, and native tests. Read
[Which tool](#which-tool), then only the section the flow needs. Commands run
from the repository root unless stated otherwise.

## Which tool

Pick the tool from the surface that has to be observed. One session owns a
visible window; WebDriver and computer use must not both click it.

| Surface | Tool |
| --- | --- |
| Renderer logic and bridge call shape | Vitest with `MockLoupistBridge`. `src/test/setup.ts` only fills jsdom gaps such as `matchMedia` and canvas. |
| Prompt, Skill, Memory, and tool-contract behavior | Headless eval harness, no window. See [Agent evals](../evals/README.md#agent-evals). |
| Read a macOS development window, including a manual window already in use | `node apps/loupist/e2e/agent-driver.mjs native-snapshot`. Captures the owned window without activating it. |
| Read native menu titles, enabled states and AppKit actions | `node apps/loupist/e2e/agent-driver.mjs --session <absolute e2e receipt> native-menu-snapshot` in an isolated macOS E2E window. |
| DOM, settings, chat progress, and layout inside the Tauri webview | `agent-driver.mjs` on the shared or isolated window. On Linux the window is WebKitGTK under Xvfb ([Run native suites on Linux](#run-native-suites-on-linux)). |
| Native file chooser, OS dialog, OS drag or wheel, or proof that pixels are on screen | The owning session's computer-use tool on a visible window; use a [manual](#manual-testing) window when WebDriver cannot attach. |
| Fixed-state visual design review of the real React app | `npm --prefix apps/loupist run preview:capture` (isolated headless Chrome, no native window). |

Native suites find an element by its role and accessible name first, then by a
behavior attribute the app already reads (`data-thread-id`, `data-home`,
`data-workspace-close`), then by a `data-testid` hook named after the element,
such as `[data-testid=map-view]`. They do not select by CSS class, so a styling
rename cannot break them only after merge. A hook sits on exactly the elements
that carry its class today; when a class moves, move its hook with it. Class
selectors remain only for elements that have no hook yet (layout probes that
measure a styled box and the paid live journeys).

The mock bridge is the Vitest stand-in for the Tauri bridge contract. Extend it
only so a renderer test can reach a path a Rust command already owns; a mock
result proves no persistence, credentials, process isolation, or native window.
The design preview (`preview.html?scenario=<id>&theme=<light|dark>`) renders the
real `App` with preview-only fixtures from `src/preview/`. It is development-only,
starts Vite at port 1430 (falling upward) and leaves the native dev port 1420 alone.
It uses headless Chrome. A capture that looks right there is not evidence for
WKWebView. In particular, `display: grid` or `display: flex` on a `fieldset`
collapses its `legend` to about one character in WKWebView (the heading runs down
the form) while Chrome keeps the line. Form-heading proof is `assertFormLegendsFit`
in the macOS WebView (`native-forms.mjs`), not a preview PNG. Likewise WKWebView
has no `field-sizing: content`, so a textarea styled to grow with its text stays
at its row height there; the composer measures its own height, and
`assertComposerGrows` (native smoke) types into it in the WebView. A `<button>`
with `display: flex; flex-direction: column` does not stretch its items in
WKWebView unless it sets `align-items: stretch`, so a child meant to end in an
ellipsis keeps its full width there (X-015), while Chrome stretches it.
`browser-ready`, `browser-blocked`, and `browser-failure` are the in-app browser
fixtures in `src/preview/browserScenarios.ts`: one fictional article opened from
an answer link, then the ready page, a typed blocked navigation that keeps that
page, or a localized open failure. Reading-column review lives in
[Chat reading column and in-app browser](../../../docs/ui/evidence-browser.md#chat-reading-column-and-in-app-browser).

`shell-sidebar-home` captures the navigation with one Loupist and its home chat.
`shell-sidebar-many` covers long names, recent chats and status indicators.
Both open the sidebar sheet at the minimum window size. The sidebar destinations are
Home, Following, World, Lens and Schedule. Following owns followed assets;
Lens contains Pages and History. World toolbar opens Library with Sources, Journal
and Notes tabs. Custom data connections are behind the Custom connections disclosure.
Every non-print shell capture checks the titlebar, body, frame, navigation footer,
header controls and composer against their rendered boundaries (`shell-layout.mjs`).
The icon rail also checks the face and every action, including Following, World,
Lens and Schedule, for clipping and horizontal centering; the sidebar and the page
column must run the full window height, and on macOS no control may sit under the
native window controls (the first 80 x 44 px).
`shell-titlebar-grown` raises the page header to 72px to catch a page column that
still assumes the default height. It runs in both `preview:layout` and the native
geometry suite. Chat captures also check the responsive title and reading gutters.
`chat-header-resuming` checks a recovered run's status above the chat body.
`activity-sheet` and `activity-sheet-panel` check the activity sheet: at most 372px,
held to the right edge of the chat layout under the page header; docked, it leaves
the chat at least 420px and covers neither the reading column nor the composer;
it overlays only when the chat layout is too narrow to dock.
`activity-sheet-earlier` adds earlier turns with long questions. Every shell capture
measures the lists in the sidebar and side panels whose rows pair a one-line title with a
time, count or status at the row's end (Earlier answers, sidebar chat and place rows,
Schedule's running list, workspace files) and fails when a title runs under that end or
either leaves its row. `preview:layout` runs English and Korean. Narrow overlay panels
make the covered chat inert, so split fixtures open docked panels only above 1060px.
The panel opens with ⌘\ in these fixtures; the page header's button is the
activity sheet's. `work-progress.mjs` reads steps from the inline ledger while the
thread still draws one, and otherwise opens the sheet per reply and reads its
`data-progress-*` step lines.

Native automation uses the embedded `tauri-plugin-wdio-webdriver` 1.3.0 with a
[pinned local patch](../src-tauri/vendor/tauri-plugin-wdio-webdriver/LOUPIST_PATCH.md)
and locked `webdriverio` 10.1.0. Browser tools (Playwright, IDE browsers) do not
attach to this webview and do not qualify the native bridge or WKWebView; do not
add a second automation plugin without a demonstrated gap.

A locked Mac can run WebDriver suites with `LOUPIST_E2E_ALLOW_UNFOCUSED=1`
(a **WebView-only** pass). Computer use needs the window visible and the Mac
unlocked.

| Launch | App data | WebDriver |
| --- | --- | --- |
| `up.sh` | `~/.loupist/development/` | yes, port 14440 unless the receipt records another loopback port |
| `up.sh --manual [--profile <name>]` | `~/.loupist/profiles/com.monarchjuno.loupist.<name>` (default `manual-preview`) | no |
| `up.sh --manual --fresh` | a new `com.monarchjuno.loupist.manual-<time>-<hex>` profile | no |
| `up.sh --live [--profile <name>]` | `e2e/artifacts/live-profiles/<name>/app-data` (default `live`) | yes |
| isolated suite | temporary state, kept only when `LOUPIST_E2E_STATE_DIR` is an absolute path | yes, its own port |
| installed app | `~/.loupist/` | never attach |

| Request | Mode |
| --- | --- |
| Let the user test; native file chooser | [Manual](#manual-testing): `up.sh --manual` |
| Inspect DOM/bridge behavior | [Shared development](#shared-development-window): `up.sh` |
| Regression or folder-copy/reopen contract | [Isolated suite](#deterministic-native-suites) |
| Authorized live Chat in a separate library | [Live](#live-webdriver-library): `up.sh --live` |

Reuse the recorded session and profile. Report the open mode/profile, the
observed flow and any unrun boundary. Opening a window does not call for model
usage or the full test gate. Install the driver once with
`npm ci --prefix apps/loupist/e2e`.

## Shared development window

```bash
apps/loupist/e2e/up.sh
node apps/loupist/e2e/agent-driver.mjs inspect
node apps/loupist/e2e/agent-driver.mjs window-default
node apps/loupist/e2e/agent-driver.mjs wait-chat-idle --strict
node apps/loupist/e2e/agent-driver.mjs --help
apps/loupist/e2e/down.sh
```

`up.sh` attaches to the watched `npm run tauri dev` window, waits for an ongoing
Rust rebuild, or starts that mode. State lives under `~/.loupist/development/`,
separate from the installed app. Debug connector credentials last only until
process restart and never touch Keychain or Credential Manager. Vite updates React
in place; Rust rebuilds restart the app and WebDriver, so retry attach instead of
opening another window. `down.sh` stops only the recorded owner.

The driver offers bounded inspection, selector-based input, screenshots under
`e2e/artifacts/` and waits, without arbitrary JavaScript, uploads or navigation.
`inspect` reports rendered text and controls, including disclosure summaries,
labels associated with inputs and title-backed icon buttons; collapsed disclosure
content is excluded.
Pass `--session /absolute/path/to/receipt.json` before the command to target an isolated launcher.
`wait-chat-idle` waits up to ten minutes; `--strict` also requires a nonempty
completed answer. The progress dump includes each tool group in transcript order
in either UI language. New WebDriver windows start at 1280×820 logical pixels;
`window-size` accepts 700–2400 × 560–1600. Record an existing user window's size
before a temporary resize and restore it afterward.

Before an authorized real-model Chat action, visibly verify the requested model.
If none was requested, select the preset (`live-preset.mjs`, model ID and effort from `chat-model-defaults.json`,
shown as `GPT-6.1-Sol · 보통` in Korean)
rather than trusting a saved selection:

```bash
node apps/loupist/e2e/agent-driver.mjs live-preset
```

## Manual testing

Use normal development mode without WebDriver for user operation or a native
file chooser:

```bash
apps/loupist/e2e/up.sh --manual
apps/loupist/e2e/up.sh --manual --profile knowledge-preview
apps/loupist/e2e/up.sh --manual --fresh
```

`--fresh` creates a new profile; it never repairs, resets or deletes an existing
one. The launcher reuses a matching live session and refuses an occupied session
or port. With Screen Recording already granted, `native-snapshot` returns a
PNG path under `e2e/artifacts/`, the outer frame in points and the
frontmost/on-screen flags. It requires one unambiguous app-sized window and
checks the capture target before and after capture. A window can supply an
image from another Space. Verify OS focus and input separately when required.

On macOS the dev launchers run the debug binary inside a local `.app` bundle
(Cargo runner `macos-dev-runner.toml`) so app-oriented computer use can find it.
Resolve the exact target after the session is ready, and again after a Rust restart:

```sh
node apps/loupist/e2e/wait-session.mjs --native-app apps/loupist/e2e/artifacts/current-session.json
```

Use the returned `appBundlePath`, not the product name (which can resolve to an
installed release), and confirm actual app content in a screenshot. The command
reports `visibleWindowVerified: false`; process discovery is not visual proof.
Never fall back to launching the installed app.

## Live WebDriver library

An authorized live Chat uses an isolated library and the existing development Pi
directory in place. If sign-in belongs to another existing directory, set
`LOUPIST_LIVE_PI_AGENT_DATA_DIR` to its absolute path and keep it across
restarts. Check presence only; never read, copy, print, reset or delete
credentials to fix a test. An authentication rejection does not authorize
changing the requested model.

```bash
apps/loupist/e2e/up.sh --live --profile qa-example
node apps/loupist/e2e/env-diagnostic.mjs --profile qa-example
```

`--live` freezes the native build; restart the owned window after Rust or agent
resource edits.

## Runtime preparation

Stage only missing runtimes the flow needs, with scripts under `apps/loupist/scripts/`:

| Flow | macOS arm64 script |
| --- | --- |
| Host UI, retained Sources, Lens, sealed World | none |
| Chat/model runtime | `stage-pi-macos-arm64.sh` |
| Market data tools | `stage-openbb-macos-arm64.sh` |
| World local embeddings | `stage-embed-assets.py` |
| Python analysis journeys | `stage_python_runtime.py` |

Pi staging replaces `resources/pi-runtime`: stage Pi first, then OpenBB.
`stage-macos-arm64.sh` stages the full set. Finish staging before the final build
so embedded digests match. Windows is not supported in this release.

Live journey reports use that same default model ID. Set
`LOUPIST_E2E_LIVE_MODEL_ID`, `LOUPIST_E2E_LIVE_MODEL_LABEL` and
`LOUPIST_E2E_LIVE_EFFORT` together when exercising another model.

## Native troubleshooting

Keep one window-input owner and one WebDriver session; tell the user before
closing a visible window and stop only its recorded owner. Never kill all port
listeners or delete profiles to resolve a failure.

| Observation | Action |
| --- | --- |
| Unsupported database schema | Preserve it; use a fresh profile. |
| WebDriver disappears during a Rust build | Read the launch log, wait, then attach again. |
| Port 1420 belongs to another session | Identify the owner and arrange a handoff; never start a competing window. |
| Debug Keychain dialogs repeat | Stop the task-owned app and inspect its credential backend. Do not approve every dialog or delete keys. |
| Canvas blank or camera unsettled | Unlock the Mac, restore the window and wait for visible frames. DOM presence proves neither rendering nor motion. |
| Synthetic graph drag/wheel fails | The pinned driver omits fields D3 needs. Verify OS input with computer use; do not alter product handlers. |
| A clicked Radix menu stays closed | The pinned driver may omit `pointerdown`. The shared `clickControl` helper uses the menu's keyboard activation and requires its actual expanded state before continuing. |
| macOS WebDriver file input opens no chooser | Its `WKUIDelegate` lacks open-panel forwarding. Use manual mode. |

POSIX launchers and `npm run tauri dev` hold a `flock` lease at
`~/.cache/loupist/native-ui.lock`, so one user owns one native GUI run
across checkouts. A competing launch fails before opening a window; arrange a
handoff instead of retrying or deleting the lock. The kernel releases the lease
when its last inherited descriptor closes; `lsof` on the lock file identifies
surviving holders. Headless tests can run independently; GUI suites stay serial.

The recorded-session cleanup test uses a real sleep process through a symlink
on macOS so it can be reaped. On Linux it removes the fixture's launch path
before stopping the recorded tree, which exercises cleanup with a missing path.

## Deterministic native suites

Run from `apps/loupist/e2e/`. Isolated runners use `--no-watch`, the
`com.monarchjuno.loupist.e2e` identifier, private session receipts and
temporary state, so they never rewrite a development or persistent profile.

| Command | Coverage |
| --- | --- |
| `npm run test:native` | Credential-free notice-to-Loupist onboarding, the header profile strip, Chat, Lens and World panels, Loupist settings, Settings → Data connections, World empty state, and cold-start / Loupist-switch timings, map-probe frame times for the 1,000-node overview and the 150-name two-hop view, and the number chip's dotted underline as WebKit computes it. On macOS Apple Silicon it first runs a Bot hand-off from the knowledge fixture's World link with the scripted slow child (`LOUPIST_E2E_KNOWLEDGE_SEED=1 LOUPIST_E2E_SLOW_HANDOFF=1`, set by the runner): nothing in Bot while the session works and the link reading as researched, the session's sidebar row into the live session, leaving and joining again with Bot's draft kept, Guide from the session's composer, Stop, and Bot's one Stopped post with Look into it again on the link. Other hosts print it as skipped. |
| `npm run test:native:full` | The core smoke plus connection filtering, appearance, updates, rejected retired commands and the minimum window |
| `npm run test:native:release` | The core smoke on the [release renderer](#release-renderer): the built pages under the shipped Content-Security-Policy; any violation fails it |
| `npm run test:persistence:release` | The persistence launches on one release-renderer build; the run fails on any reported violation |
| `npm run test:persistence` | Five launches. **Seed**: fresh app data plus the knowledge fixture; through the UI it creates a Loupist, a Chat (Look back from a Journal entry, then rename the session), a Source and Journal entry in World Library, a principle in Lens, two new assets followed by name through Following's "Follow '{text}' as typed" (no listing search, so nothing leaves the Mac; one then unfollowed with its row's Stop following, the other opened, where the page says it has no price yet and a reason is saved), Loupist instructions, global and per-Loupist Chat defaults, the theme, and the display currency (KRW, written to the global `config.toml` without any rate read). Typing the exact name of a fixture asset opens its page and leaves one follow. **Restart**: the same app data shows all of it, with the notice acknowledgment, language and theme applied without setting them again and the display currency still selected; the typed asset is a Following row (found by its `data-row-key`) marked "Name only", the unfollowed one stays gone, the created Loupist's Following lists neither it nor the fixture asset (follows are per Loupist), and its page opens from the row with the reason and the no-price line. **Copy** and **fork**: a fresh app opens a copy of the Loupist folder; its Chats, Library, World, principle, revised method with Then/Now history, frozen Journal inputs, instructions, the typed asset's reason (in the Library's Journal) and Loupist preferences follow the copy, while its follows (kept by the old app for this Loupist), the other Loupist, global settings and the notice stay with the old app data. **Reconnect**: the fork's folder is put at the copy's registered path; the copy's app reports the folder unavailable, reconnects it through Reconnect folder, and finds the reason. A saved price read needs a model turn and is not covered here. |

The Loupist switcher helpers read the trigger's expanded state and wait for
open menu content. A selection or Escape can leave the menu in the DOM during
its exit animation. Helpers that select a Loupist or dismiss the menu wait
for it to unmount before returning. The switch timing still ends when the
header reports the new Loupist.

On macOS Apple Silicon the smoke also attaches an image and a CSV through the composer's file input
(what the + menu's Files… chooser fills; `native-attachments.mjs`), checks that the tray decodes the image
from a `data:` URL, sends them with the fixture model (which accepts images) and finds both on the sent
message. `assertComposerSelectAll` (`native-forms.mjs`) checks that the app menu's Edit carries enabled
Undo, Redo, Cut, Copy, Paste and Select All (`selectAll:`, what ⌘A reaches in WKWebView), that WebKit's
SelectAll selects the whole message and that the selection paints its text.

`test:native` runs the Data connections controls with `LOUPIST_DEV_CUSTOM_CONNECTIONS=1`. Run `LOUPIST_DEV_CUSTOM_CONNECTIONS=0 npm run test:native` to assert that the custom section is absent. The live launcher enables the switch for the default set and explicit selections that include journey 6. The smoke and live runners forward it through the isolated debug launcher.

`test:native` calls `assertFormLegendsFit` (`native-forms.mjs`) on `[data-loupist-tone]` on New Loupist. Its later step “native browser preserves the article while blocking unsafe navigation” calls `verifyNativeBrowserNavigation` (`native-browser.mjs`). A row above names the command. The WebKit result is that step’s finished run, recorded by the session that ran it. Preview Vitest and `preview:capture` check the React fixtures in Chrome and jsdom.

The composer smoke checks wait for the requested preview URL and its ready
marker. Before interacting with the composer, they require a separate script
response within two seconds so a hung renderer reports a deadline failure.

For chat layout changes, run the separate geometry suite against an owned
WebDriver session. From the repository root:

```sh
node apps/loupist/e2e/native-layout.mjs apps/loupist/e2e/artifacts/current-session.json --widths=700,800,899,900,1061,1280,1680,2560 --locales=en,ko --themes=light,dark
```

It checks reading/composer alignment, context and status controls, chart and
table bounds, receipt labels, and restoration of the saved
panel width. It writes screenshots and measured geometry to
`apps/loupist/e2e/artifacts/layout/`, then restores the window size, URL and local
UI preferences. The CLI uses the attached window's URL when the shared development
session has no recorded dev URL. A display that cannot fit 2560px records that
boundary as skipped;
it is not a pass. Fixture rendering uses the mock bridge; the browser-policy
step in `test:native` exercises the real child WebView and Rust commands.

The native geometry suite also checks that the Bot answer's Activity action keeps
the end or the current reading element in place while docking, switching tabs,
resizing through the overlay breakpoint and closing. It checks both reading positions
in a research session through streaming, saving, completion with a checked figure and
table, and a late chart. It measures the composer's bottom and empty editor height and
checks answer action tooltip positions in English and Korean. Use `--transitions-only`
to run these cases without the static geometry matrix.

Linux runs the same commands; see [Run native suites on Linux](#run-native-suites-on-linux).

| Variable | Effect |
| --- | --- |
| `LOUPIST_E2E_ALLOW_UNFOCUSED=1` | WebView-only pass on a locked screen: checks the live webview, disables CSS motion, and measures World budget as 200 canvas draws at 1,000 nodes instead of frame cadence. It does not prove on-screen rendering, OS input or native dialogs. |
| `LOUPIST_E2E_REQUIRE_STAGED_RUNTIMES=1` | CI: Market data must report Ready. |
| `LOUPIST_E2E_DEV_PORT=<port>` | Pin the isolated Vite listener to a port other than 1420; 0 (the default) asks the OS to bind an available port and records its URL in the session receipt. |
| `LOUPIST_E2E_REUSE_DEV_SERVER=1` | Reuse this workspace's Vite server on 1420; app data and WebDriver port stay separate. |
| `LOUPIST_E2E_STARTUP_TIMEOUT_MS` | Startup wait for isolated suites and `up.sh`/`run-dev-app.sh`, default five minutes. A failed new `up.sh` launch stops its detached process group. |
| `LOUPIST_E2E_PROVIDER_READY=1` | Set by the credential-free smoke and persistence suites. An `e2e` build marks ChatGPT (the `openai` provider) ready without a secret or network call. The knowledge fixture can list local scripted models. |
| `LOUPIST_E2E_KNOWLEDGE_SEED=1` | Set by persistence for its first launch, and by the smoke on macOS Apple Silicon for its hand-off journey: on fresh app data an `e2e` build adds a sealed World, a Source, a Lens method and a saved Chat turn with its research note and Journal entry to the first Loupist. |
| `LOUPIST_E2E_SLOW_HANDOFF=1` | Set by the smoke on macOS Apple Silicon. On a macOS Apple Silicon debug `e2e` build, runs only host-started sessions using the **Fixture research model** (`e2e-link-research`) against a scripted Pi child. No provider credentials or network are used for that turn. |
| `LOUPIST_E2E_RELEASE_LEARNING=1` | Set during persistence seed on macOS Apple Silicon. The **Fixture learning model** (`e2e-release-learning`) scripts a Bot proposal and a scheduled run through the real broker. The restart checks the saved program, finished run and Lens Undo. |
| `LOUPIST_E2E_PICK_FILE` | An absolute existing file that `pick_file` returns in an `e2e` build. |

For a live hand-off journey, start `run-app.sh` with isolated, fresh state and
`LOUPIST_E2E_PROVIDER_READY=1 LOUPIST_E2E_KNOWLEDGE_SEED=1 LOUPIST_E2E_SLOW_HANDOFF=1`.
In Bot, select **Fixture research model** and click **Look into it** on the
seeded World link. A journey can also submit `research_handoff_start` for
`con_fixture_hub` using that model profile and thinking level `off`. The first
step starts immediately, subsequent steps arrive two seconds apart, and the
reply stays open for another twelve seconds. Bot posts nothing meanwhile: open the
session from its sidebar row to watch, send an instruction to steer, then Stop;
Bot should then post one Stopped item with Open.
Leave the slow flag unset for the normal persistence fixture.

Builds without the `e2e` feature ignore these app-side variables. Suites verify
their session owner, then show and focus the window (on macOS retrying activation
for 25 seconds) before asserting. The isolated app seeds one Loupist from
`<state>/workspace`. The smoke saves `appearance.language = "en"` and removes that
Loupist so onboarding starts at step 1; persistence keeps it as the folder Loupist it
copies. Isolated runners bind Vite to port 0 and record the serving URL without releasing the listener. A Source added
from a file needs the OS chooser (see the troubleshooting table) and is not part
of persistence. Window checks compare the native size with
`innerWidth`/`innerHeight`. Evidence stays under `artifacts/` (persistence:
`artifacts/persistence/<run>/`).

## Release renderer

Desktop `tauri dev` loads Vite directly, and Tauri attaches `app.security.csp` only to its own
`tauri://` responses, so every Vite-backed window runs with no policy. `release-renderer.mjs` builds
`index.html` and `preview.html` with `vite build` and serves them with the exact policy from
`tauri.conf.json` and a `report-uri`; a recorder script, first in every page, keeps each
`securitypolicyviolation` in `sessionStorage`. Set `LOUPIST_E2E_RELEASE_RENDERER=<absolute report file>`
for `run-app.sh` to launch on it (`LOUPIST_E2E_RELEASE_RENDERER_DIR` reuses a prebuilt folder).
`run-native-smoke.sh --release-renderer`, `run-native-persistence.sh --release-renderer`, the layout suite
and `capture-preview.mjs --release-policy` fail on any violation the page or the report file holds and
list each blocked source. The built pages have no `/src` modules; `release-renderer.mjs` builds the few a
suite imports by path (`src/bridge/tauriBridge.ts`) as entries and redirects to them. The web gate's
`release-policy` step (`npm run preview:policy`) runs a core set of scenarios in Chrome:
attachments, the lightbox, a chart, the grid export menu, a Source selection, print, the map export
menu and the in-app browser. The policy and its reasons: [Renderer Content-Security-Policy](../../../docs/runtime/permissions.md#renderer-content-security-policy).

## Run native suites on Linux

The same launchers, suites and `agent-driver.mjs` commands run on Linux against
the Tauri app in WebKitGTK. Use them in a cloud session for any change that the
real bridge, Rust commands or persistence can break. Install the host packages
from the [Linux procedure](../../../docs/dev/checks.md#linux-cloud-development),
including `xvfb`, then run from `apps/loupist/e2e/`:

```sh
npm ci
npm run test:native          # add :full for the full smoke
npm run test:persistence     # storage, restart and folder-copy changes
```

`run-app.sh` starts the app with a cleaned environment and passes
`RUSTUP_HOME` and `CARGO_HOME` through when they are set, so `cargo metadata`
uses the pinned toolchain. When neither `DISPLAY` nor `WAYLAND_DISPLAY` is set, the GUI lease
(`native_ui_lease.py`) starts a private 2560x1600 Xvfb before it runs the
launcher, and Xvfb exits with that launcher. `up.sh` and `down.sh` work the same
way, so an agent can open the shared development window and drive it with
`agent-driver.mjs`. The native smoke clears an inherited display before it
takes that lease. A desktop window manager draws a title bar outside the
700 by 560 minimum client size, so the outer window cannot be the size the
smoke requests; the private Xvfb has no window manager. `up.sh` still uses
the current display. The first `e2e` build takes several minutes; each later app
start takes about 25 seconds.

A Linux pass proves renderer behavior against the real bridge, Rust commands
and app data. It does not prove WKWebView layout or rendering, Keychain, OS
dialogs, OS input or macOS window chrome, and deep links, single instance and
the updater are off on Linux. Qualify those on macOS. Xvfb renders in software,
so the smoke's cold-start and map-frame budgets are reported but are not
comparable with macOS. `native-snapshot` and computer use need a macOS window.

## Explicit live suites

Opt-in, paid, and governed by the [live Pi directory rules](#live-webdriver-library).
Run from `apps/loupist/e2e/`:

| Command | Purpose |
| --- | --- |
| `npm run test:live-journeys` | Release journeys on the preset `openai/gpt-6.1-sol` at medium (the first model in `chat-model-defaults.json`, read by `live-preset.mjs`), through the OpenAI connection (`openai`: a ChatGPT sign-in or an OpenAI key) already configured in the live Pi directory; onboarding uses that account and does not automate Sign in with ChatGPT, which stays owner-run. They cover notice-to-Loupist onboarding with that pre-provisioned account, a useful cited answer, map growth, a principle applied later, a briefing that must finish, and one allowed tool on a public URL MCP connection. Screenshots go to `artifacts/journeys/<run>/`. |
| `LOUPIST_E2E_JOURNEYS=7 npm run test:live-journeys` | Connects a custom OpenAI-compatible provider from onboarding against a local stand-in server, sends two turns, checks that the second request retains the first, reloads the saved Chat, then removes the provider and checks that the Chat remains. It uses an empty Pi directory inside the run's state root. |
| `LOUPIST_E2E_JOURNEYS=compaction npm run test:live-journeys` | Uses the same isolated local provider with reported token pressure and one-token retained history. Checks that the first answer settles without summarization, the next input compacts through real Pi, host input and summary provenance reach the model, progress has no failed deferred step, and the answer survives reload. No paid model call. |
| `LOUPIST_E2E_JOURNEYS=private-document,research-handoff npm run test:live-journeys` | Opt-in live checks: a Memory-off Markdown document opens in Preview and Source; the Loupist opens a separate arithmetic chat, receives its completed handoff and opens the answer. |
| `LOUPIST_E2E_JOURNEYS=bot-memory npm run test:live-journeys` | Uses the live Loupist to remember a preference, reload the UI, correct it and forget all retained wording. Visible activity and read-only inspection of the isolated workspace's committed Chat record establish that the model called the real tool and the reply persisted its changes. Long-history cold starts, process restarts, private turns and branches are covered by the Rust chat-memory turn tests. |
| `LOUPIST_E2E_JOURNEYS=python-chart,python-cancel,python-denial,python-after-web npm run test:live-journeys` | Opt-in only. After `python3 apps/loupist/scripts/stage_python_runtime.py`, one serialized GUI run checks a matplotlib PNG in Files, a stopped `run_python` sleep whose `outputs/kept.txt` is still in Files after Stop, a `run_python` probe whose `outputs/denial.txt` records blocked import, network, and process start, and `web_fetch` of https://example.com followed by `run_python` writing `outputs/after-web.txt`. These ids stay out of the default journey set. |

These journeys do not substitute for model evaluation.

## Security boundary

- WebDriver exists only with debug `webdriver`/`e2e` features; release builds
  reject both. Its unauthenticated loopback endpoint must never target the
  installed profile.
- Launchers use private permissions and a minimal `env -i` environment. E2E builds
  ignore provider credential variables; connector credentials are process-local.
- Session waits and the driver verify the launcher's process identity and its
  ownership of the listening port before attaching; an occupied port gets a
  replacement and is never proof of ownership.
- E2E dependencies are locked and `.npmrc` disables lifecycle scripts.
  `npm run verify-security` checks these invariants in the web gate.
- Artifacts, profiles, databases, credentials and screenshots stay ignored or
  outside the repository.
