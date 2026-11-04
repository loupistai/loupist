# Visual regression

## Chat layout preview and side-panel geometry

Run `npm --prefix apps/loupist run preview:layout` after changing shell, panel or chat layout.
The web gate runs the same twenty cells in headless Chrome. A grid answer and an article opened
from a citation cover the minimum (700×560), both sides of the sheet/dock transition
(1060 and 1061×820), the default desktop (1280×820) and a wide desktop (1920×1080).
The default uses English and light theme. For a localized layout change add
`-- --locales ko,en-XA --themes light,dark`.

Each row reports measured CSS pixels for the viewport, sidebar space, panel, chat, uncovered
chat and reading column. On a sheet the reading column extends behind the panel. Check
`uncoveredChat` to see how much chat is exposed. The command exits nonzero on overflow,
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
disables smooth scrolling. Chrome restarts between scenarios to bound page lifetime.
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
