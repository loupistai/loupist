# Visual regression

## Chat layout preview and side-panel geometry

Run `npm --prefix apps/loupist run preview:layout` after changing shell, panel or chat layout.
The web gate runs the same twenty cells in headless Chrome. A grid answer and an article opened
from a citation cover the minimum (700×560), both sides of the sheet/dock transition
(1060 and 1061×820), the default desktop (1280×820) and a wide desktop (1920×1080).
The default uses English and light theme. For a localized layout change add
`-- --locales ko,en-XA --themes light,dark`.

Each row reports measured CSS pixels for the viewport, sidebar space, panel, chat, uncovered
chat and reading column. Beside a side sheet the chat wraps into the exposed strip;
its right edge meets the sheet's left edge. The command exits nonzero on overflow,
misalignment, clipped grid controls or an incorrect panel mode. Inspect the PNGs and
`manifest.json` at the printed path. The web gate keeps these with its receipt.
In a newly initialized repository, captures keep `gitHead: null` until the first
commit; capture and layout checks still run normally.

This preview runs the real React components with fixture data in headless Chrome. It does
not qualify WKWebView. A `fieldset` set to `grid` or `flex` keeps a normal legend in
Chrome and collapses that legend to one character in WKWebView, so the heading stacks
down the sheet. Measure those headings with `assertFormLegendsFit`
(`apps/loupist/e2e/native-forms.mjs`) in the macOS WebView. Use the native geometry
suite for the system WebView and the native browser smoke for child-WebView/Rust behavior.
See [Which tool](../../apps/loupist/e2e/README.md#which-tool).

## Whole-app UI review

`src/preview/scenarios.ts` is the registry: it assembles each feature's scenarios from
`src/preview/scenarios/<feature>.ts` (helpers in `scenarios/kit.ts`) and the older
`src/preview/*Scenarios.ts` modules, in capture order. Captures, the manifest and
baselines key on scenario ids, so keep an id and its place when moving a scenario.
Scenario fixtures start from `src/preview/fixtures.ts` (`PreviewBridge`,
`prepareFixture`); a feature's own fixture is in `src/preview/fixtures/<feature>.ts`.

The complete scenario registry includes onboarding, every Settings destination,
per-Loupist preferences, Skill management dialogs, update states, research readers,
sharing, connections, financial cards and their empty/error/loading states.
Run the full registry in both languages and themes at the minimum and wide sizes:

```sh
npm --prefix apps/loupist run preview:capture -- --locales en,ko --themes light,dark --sizes 700x560,1680x1050
```

Every non-print cell checks shell geometry, title clearance from tab-close buttons,
and usable Lens reader height. `browser-long-title` updates a browser title through
the host event, and `lens-reader-many` opens twenty additional methods in an expanded
Lens; these catch regressions absent from short-title and one-method fixtures.
Inspect the screenshots as well as the manifest. A successful preview does not
qualify native file dialogs, the child browser or WKWebView form legends.

## Pixel comparisons

Capture readiness first waits for each scenario’s visible landmark. Inline and revised financial-chart
scenarios require their chart canvas in a browser. It then waits for rendered Markdown, loaded
fonts, settled lazy requests and DOM, and unchanged Map canvas pixels. Readable Markdown
fallbacks must disappear before capture, even when their text has no formatting markers.
Preview framing waits for rendered content and fonts before scrolling the target into view;
Thread alert and results previews also wait for morning content to finish its automatic scroll.
Reduced motion is forced through Chrome media emulation;
the app's CSS guards own animations and transitions. Before each screenshot, one browser
snapshot checks running CSS animations and transitions: infinite effects or more than 1 ms
of total duration (including repetitions) fail the cell and are recorded with target, effect,
duration and iteration count in `manifest.json`. The threshold permits near-zero guard durations;
the explicit allowlist in `scripts/capture-preview.mjs` is empty. This checks the settled scenario,
not completed effects or motion triggered by later interactions. Injected CSS hides carets and
disables smooth scrolling. Chrome restarts between scenarios by default; `--browser-scenarios 1..15` permits bounded reuse for whole-app audits. Each navigation still resets storage and bypasses the network cache. Capture servers disable filesystem watching.
Chrome has a bounded 45-second startup window for cold hosted runners running the
parallel contract suite; early process exits still fail immediately with browser diagnostics.
Capture freezes the fixture clock before app initialization
(including generated map IDs) and hides native scrollbars to avoid their platform fade animation. Compare runs on the same machine and Chrome version
(`CHROME_PATH` can select a different executable). A changed browser or font environment can
change rasterization even when the app is unchanged.

Run from the repository root (CLI paths passed after `npm --prefix` resolve from the app directory):

```sh
node apps/loupist/scripts/capture-preview.mjs --locales en,ko,en-XA --themes light,dark --sizes 700x560,1680x1050 --out output/design/candidate
node apps/loupist/scripts/visual-diff.mjs --baseline output/design/baselines/phase-cd --candidate output/design/candidate --out output/design/diff
node apps/loupist/scripts/visual-baseline.mjs accept --from output/design/candidate --name phase-cd --reason "Reviewed design change" --replace
```

The npm aliases are `preview:diff` and `preview:baseline`. Accept a baseline only after reviewing
an intentional design change, with a meaningful reason; do not accept a regression just to pass
a comparison. Baselines live in repository-root `output/design/baselines/<name>/`, are Git-ignored,
and record capture commit, acceptance date, reason and cell count in `BASELINE.md`. Omit `--replace`
for the first acceptance; replacement moves the prior baseline to `<name>.prev` (one prior version).

Open the diff's `index.html`: highest mismatch first, baseline/candidate/diff thumbnails and
side-by-side links. Pink marks pixels whose RGBA channels differ by more than 16; unchanged
candidate pixels are dimmed to 30%. Size changes count as 100%; new and missing cells are listed
separately and fail the comparison. `diff.json` contains exact percentages and totals. The default
threshold is 0.2 percent; use `--threshold 0` for determinism checks. Exceeding the threshold exits 1;
`--report` writes the same report without a regression exit failure. Invalid or incomplete captures
still fail. To prove determinism, capture the same full matrix twice without source edits and compare
at zero threshold before accepting it.
