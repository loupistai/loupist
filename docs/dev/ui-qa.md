# UI review

Start chat and panel changes with the ten-cell [geometry check](visual-regression.md#chat-layout-preview-and-side-panel-geometry).
Review the affected screen in both themes and implemented locales. Native proof is [Which tool](../../apps/loupist/e2e/README.md#which-tool).

## Desktop design system

Paths in this section are relative to `apps/loupist/`.

### Design rubric

This is a review standard, not a claim that every current screen passes. Review UI changes in light
and dark themes, for implemented locales, and at the minimum (700×560) and large window sizes. A defect a first-time user would notice is P1.

| Area | Pass when |
| --- | --- |
| Clarity | One obvious primary action per view. Describe the result, useful status or next action in plain words. Keep runtime rules and bookkeeping out of UI copy and ordinary assistant answers; no internal terms (revision, digest, receipt, worker, envelope, byte offset). |
| Hierarchy | The type, spacing, and radius come from tokens. Alignment is consistent. Density fits the task. |
| States | Empty, loading, partial, error with a recovery action, offline or disconnected, disabled, long text, many items, and first run are all designed. None of them shows raw errors or a blank region. |
| Feedback | Every action responds within 100 ms. Long work shows progress. Motion is purposeful and respects reduced motion. |
| Consistency | Shared components and tokens; one term per concept in every locale; recognizable icons. |
| Accessibility | WCAG AA contrast (4.5:1 for text, 3:1 for UI and focus). A complete keyboard path with a visible focus ring. Screen-reader names for every control. Hit targets of at least 28 px. |
| Internationalization | For localized surfaces, check every implemented locale and a pseudo-locale when available. Use `Intl` for numbers/dates; preserve the original language of Source and Lens text. |
| Trust | Sources are one step away. AI-generated content is identifiable and carries the short notice. UI chrome itself never tells the user to buy or sell. |

The capture script uses installed Google Chrome (the macOS app or `google-chrome` on Linux).
Set `CHROME_PATH` for another executable. Capture screens with `npm --prefix apps/loupist run preview:capture`. It renders every scenario in
`src/preview/scenarios.ts` in both themes at 700×560, 1280×820 and 1680×1050, and writes the PNGs,
`manifest.json` and a contact sheet under `apps/loupist/output/design/preview/<timestamp>/`.
The default capture locale is English; add `-- --locales en,ko,en-XA` to cover the language matrix.
Narrow a run with `-- --scenarios chat-answer,map-overview --themes light --sizes 1280x820`. Add
a scenario together with each new or redesigned screen. A scenario marked `media:
"print"` (`chat-print-view`, `briefing-print-view`) is captured with print media
emulated, as the print dialog draws it. Final proof of native behavior still uses the tools in
[Which tool](../../apps/loupist/e2e/README.md#which-tool).

A green Chrome capture is not proof of WKWebView layout. Chrome draws some rules that macOS WKWebView collapses. The known case is a `fieldset` whose computed `display` is `grid` or `flex`: its `legend` shrinks to about one character and the heading runs down the form one glyph per line. Keep the fieldset `display: block`, set the legend to `width: 100%`, and put grid or flex on an element inside it. This applies to every form, including onboarding model setup, journal checks, schedule time, and profile fields, not only the tone control. `assertFormLegendsFit` in `apps/loupist/e2e/native-forms.mjs` measures that the heading uses the field width. Run that measurement in the macOS WebView. Do not accept a preview screenshot as a substitute.

### Performance budgets

These are native e2e budgets, separate from the 100 ms feedback rubric above.
Credential-free smoke is `npm run test:native`. The six release journeys are
`npm run test:live-journeys`. Run both from `apps/loupist/e2e/`. When the
window cannot take focus (a locked screen, or someone else using the Mac), set
`LOUPIST_E2E_ALLOW_UNFOCUSED=1` for a WebView-only pass: it
checks the live WebView and does not prove OS focus, native dialogs, or a
visible window. Scope and limits are owned by
[Deterministic native suites](../../apps/loupist/e2e/README.md#deterministic-native-suites)
and [Explicit live suites](../../apps/loupist/e2e/README.md#explicit-live-suites).
Cold start is the in-page paint mark once `#main-tab-chat` is present: the minimum of the
positive DOM-content-loaded, first-contentful-paint, and load marks. Time from process
spawn to that observation is recorded beside it and is not the 3 s budget. A budget miss
is recorded. A missing shell fails the run.

Loupist switch is timed with a `MutationObserver` inside the page. A budget miss is recorded.
A missing control or a timeout fails the run. Chat time to first token is provider-bound
and is recorded only.

The map target is a frame p95 of 33 ms for the 1,000-node overview and the bounded
150-name two-hop view. An unfocused WebView throttles `requestAnimationFrame`, so
that suite does not wait on frame cadence. It times 200 consecutive canvas draws
and reports p50 and p95 draw cost as a proxy for the frame budget. A budget miss is
recorded. A missing canvas fails the run.

Record dated measurements and execution limits with the candidate's qualification evidence.
Keep development page-paint/frame measurements separate from signed-release spawn-to-visible
qualification. Repeat a missed budget on an idle machine.

Label placement reuses geometry within one React render while the camera and
node coordinates remain unchanged. A render (including selection, locale,
viewport obstacles or glow changes), camera movement, zoom, visibility change
or node movement invalidates it. Halos, labels and fades still paint on every
frame; pointer interaction and the performance probe remain enabled. The draw
proxy uses the same cache lifetime for its steady overview samples. Report its
CPU draw cost separately from actual requestAnimationFrame cadence.
During a placement pass, node classification, radii and ring clearance are
computed once per mark. Each pass gets fresh geometry for its zoom and priority
state, including while the camera moves.

`src/styles/tokens.css` owns the semantic token set: surfaces, text, borders,
accents, status/Map colors, typography, spacing, radii and motion. Primitive palette
values stay private to that file; feature styles use semantic tokens. Do not add
decorative icons above headings/empty states, in feature lists, as card avatars or
as icon tiles. Icons belong in controls and inline status at 16 px. `EmptyState`
accepts title, description and action; it has no icon prop.
Use shared `PageHeader`, `Button`, `Input`, `Textarea`, `NativeSelect`, dialog,
disclosure and empty-state components. `page.css` owns shared page layout;
`surfaces.css` owns dialog sizing/body/footer. Feature CSS owns content layout.
Use `SegmentedControl` for one-of-many choices and list filters: these are radio
groups, with arrow-key selection. Reserve `Tabs` for views with real `TabsContent`
panels. A search field or action button is not a menu item; the model picker is a
dialog containing its search, model menus and account actions. Keep visible button
labels in accessible names, and put supplementary explanations in descriptions.
Ordinary dropdown menus are non-modal; the model picker explicitly opts into its
modal focus scope. Opening an ordinary menu must not hide still-focusable app chrome
from the accessibility tree.
The searchable model picker permits Tab and Shift+Tab through its controls and restores
its trigger on Escape. Settings pages with no enabled controls (keyboard shortcuts and
an update in progress) remain focusable so keyboard users can scroll them. Workspace
item menus control the details element directly; delayed native toggle events must not
reopen a menu after selecting Files or a browser tab.
The native minimum is 700×560. Preserve keyboard focus, scroll, light/dark themes and
reduced-motion behavior. Menus never ring their container; a trigger or item a press
reached carries `data-input="pointer"` (`lib/inputModality`) and shows no ring, since
WebKit counts focus a script moved from a text field as keyboard focus. Keep ordinary chrome neutral; burnt amber `#a4530c`
is the focus/identity accent, while data and error states retain semantic colors.

The shell's implemented destinations and reader behavior are described in
[Sidebar](../ui/sidebar.md#sidebar), [Map and Library](../ui/map-lens.md#map-and-library)
and [item surfaces](../analysis/surfaces.md#where-each-item-appears). Review those flows rather
than assuming a separate World, Lens or Growth page exists.
For graph work, preserve camera/selection through navigation and use Fit for
explicit reframing. For Chat work, inspect intermediate text and tools as well as
the final reply. Keep provider recovery, source inspection and return paths reachable.

### Localization foundation

`src/i18n/` owns the `react-i18next` setup, English/Korean namespace catalogs,
`useLanguage` and cached `Intl` helpers in `format.ts`. Appearance is localized and
persists global `appearance.language` as `system`, `en` or `ko`; system follows
`navigator.languages` with an English fallback. The development/preview-only
`en-XA` pseudo-locale is not persisted. English and Korean catalogs share keys and placeholders; the pseudo-locale
exercises text expansion in previews.

Use these helpers for newly localized surfaces and preserve Source/Lens text.
`scripts/check-i18n.mjs` checks catalog parity/placeholders and tracks existing
visible literals through `scripts/i18n-allowlist.json`; it is part of the web gate.
Preview capture accepts `--locales en,ko,en-XA`.

## Code

- Native UI: [Which tool](../../apps/loupist/e2e/README.md#which-tool)
