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
Set `CHROME_PATH` for another executable. Capture screens with `npm --prefix apps/loupist run preview:capture`. It renders every scenario
`src/preview/scenarios.ts` registers in both themes at 700×560, 1280×820 and 1680×1050, and writes the PNGs,
`manifest.json` and a contact sheet under `apps/loupist/output/design/preview/<timestamp>/`.
The default capture locale is English; add `-- --locales en,ko,en-XA` to cover the language matrix.
Narrow a run with `-- --scenarios chat-answer,map-overview --themes light --sizes 1280x820`. Add
a scenario together with each new or redesigned screen. A scenario marked `media:
"print"` (`chat-print-view`, `briefing-print-view`) is captured with print media
emulated, as the print dialog draws it. Final proof of native behavior still uses the tools in
[Which tool](../../apps/loupist/e2e/README.md#which-tool).

WKWebView can shrink a `fieldset` legend to one character when the fieldset
uses `display: grid` or `flex`. Keep the fieldset `display: block`, set the
legend to `width: 100%`, and put grid or flex on an element inside it. This
applies to Notifications defaults, onboarding model setup, Journal checks,
schedule time and profile fields. Native smoke calls `assertFormLegendsFit`
in `apps/loupist/e2e/native-forms.mjs` on Notifications defaults in English
and Korean. The geometry check runs in the macOS WebView.

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
Cold start is the in-page paint mark once `[data-testid=loupist-header-selector]` is present: the minimum of the
positive DOM-content-loaded, first-contentful-paint, and load marks. Time from process
spawn to that observation is recorded beside it and is not the 3 s budget. A budget miss
is recorded. A missing shell fails the run.

Loupist switch is timed with a `MutationObserver` inside the page. A budget miss is recorded.
A missing control or a timeout fails the run. Chat time to first token is provider-bound
and is recorded only.

World target is a frame p95 of 33 ms for the 1,000-node overview and the bounded
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
accents, status/World colors, typography, spacing, radii and motion. Primitive palette
values stay private to that file; feature styles use semantic tokens. Do not add
decorative icons above headings/empty states, in feature lists, as card avatars or
as icon tiles. Icons belong in controls and inline status at 16 px. Two identity
marks are not icons and are the only exceptions: the Loupist's face
(`components/app/LoupistAvatar.tsx`, 20 to 64 px, including above the Bot greeting
and as a message avatar) and an asset's monogram tile (`components/market/AssetTile.tsx`).
The face's ten colours are `--gg-color-face-01` to `-10`, one per stored portrait id. `EmptyState`
accepts title, description and action; it has no icon prop.
Use shared `PageHeader`, `Button`, `Input`, `Textarea`, `SelectField`, dialog,
disclosure and empty-state components. Use app-rendered value menus for dropdown fields and
the interface locale for editable dates and times. `page.css` owns shared page layout;
`surfaces.css` owns dialog sizing/body/footer. Feature CSS owns content layout.
Use `SegmentedControl` for one-of-many choices and list filters: these are radio
groups, with arrow-key selection. Reserve `Tabs` for views with real `TabsContent`
panels. A search field or action button is not a menu item; the model picker is a
dialog containing its search, model menus and account actions. A search field's clear
control is the app's own ghost × button, shown while the field has text; WebKit's
native cancel button and decoration never draw (`shell.css`), so the two never overlap. Keep visible button
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
WebKit counts focus a script moved from a text field as keyboard focus. Keep ordinary chrome neutral. The iris-violet
accent (`--gg-color-accent`, `#5b45e0` light and `#6a55f2` dark; `--gg-color-accent-ink` for text and focus) is worn
by primary actions (`--gg-color-action` and shadcn `--primary` point at it), focus rings, selected choices, links and
identity. Data and error states keep semantic colors. Up and down are `--gg-color-price-up/-down` (and `-soft`
fills), which `data-price-colors` on the root sets from global `appearance.price_colors` (`auto`, `red-up` or
`green-up`; Settings › Appearance). Auto follows the interface language's convention (red-up for Korean, Chinese and
Japanese; the system's first language when the language follows the system). They mean price direction only, and a
change is never shown by colour alone: `ChangePill` carries a direction glyph that is its sign (the figures after it are magnitudes, "▼ 3.34 · 2.01%", never an arrow and a minus together) and a spoken direction. The type scale adds
`--gg-text-figure` (38 px) for a headline price and the weights `--gg-weight-strong` (650) and `--gg-weight-bold`
(700); radii are 10 for controls (`--gg-radius-md`), 16 for cards (`--gg-radius-lg`) and `--gg-radius-bubble` (20) for
a message bubble. A light card carries a hairline and `--gg-shadow-card`; a dark card only the hairline. The face
tokens (`--gg-color-face`, `-face-ink`, `-face-detail`) are reserved for the Loupist's avatar.

Shared market primitives live in `src/components/market/` with `src/styles/market.css`: `PriceText` (a price
through `formatPrice` with tabular figures), `ChangePill`, `Sparkline` (a trend line sized by its attributes, with
an accessible summary) and `AssetTile` (a monogram from the name in the interface language). They take plain props
and fetch nothing. The `market-primitives` preview scenario shows them in both price conventions. The monogram tile
is an asset's identity, not a decorative icon tile.

The shell's implemented destinations and reader behavior are described in
[Sidebar](../ui/sidebar.md#sidebar), [World and Library](../ui/map-lens.md#world-and-library)
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
`en-XA` pseudo-locale is not persisted. English and Korean catalogs share keys and placeholders; the pseudo-locale,
generated from English (accented, bracketed, about 40% longer), exercises text expansion in previews.
`src/i18n/pseudoLocale.test.tsx` renders every route and Settings section in it and fails on a key no
catalog has, on interface text outside the brackets that the bridge did not send (formatted dates, zone
and currency names, key names and the copy allowlist aside), and on product code outside `src/i18n` that
branches on Korean. Text keyed by language goes through `localizedValue` (`src/i18n/localized.ts`); a
locale's own rules (number words, date forms, particles) live in `src/i18n`.

Appearance also holds the optional global `appearance.display_currency`. Its select
lists As listed, then the euro and the 29 ECB reference currencies in the table's
order, each named by `Intl.DisplayNames` in the interface language with the code as
fallback; nothing is chosen from the locale or market. As listed removes the key.
A hand-set code outside the table stays selected and is named as such. The status
line under it (`appearance.displayCurrency.status.*`) words the host's `fx_display`
state for the open Loupist and is empty when the setting is unset or no Loupist is
open. `app/useDisplayFx.ts` is the window's one query: it reads only after its
`fx-changed` listener reports it is attached, rereads on every hint (a window on As
listed reloads the committed configuration instead), on becoming visible and once at
a ready table's `usable_until`, when it also hides the figure. It is shared with the asset page's
`ConvertedAmount` line: "≈", the ISO code or the locale's own word from
`currencyName`, never a symbol, then the rate with both codes and every digit the
host sent, then the source and the rate's own date. Previews:
`settings-appearance-currency-*` and `asset-page-display-*`.

Use these helpers for newly localized surfaces and preserve Source/Lens text.
`scripts/check-i18n-parity.mjs` checks catalog parity and placeholders.
`scripts/check-i18n.mjs` rejects untranslated literals in the web gate: in `.tsx`, JSX
text, labelling attributes and error setters; in `.ts` modules, prose (a capitalized
string of two or more words) unless it is a comparison with a host value, a `HOST_*`
constant, an `Error` message, a translation key or a console line. `src/contracts`,
`src/bridge`, `src/preview`, `src/test` and tests are not scanned. A literal that must
stay as written (a product, provider or license name, a library identifier, a
developer eval string) goes in `scripts/i18n-allowlist.json` with its reason. Name
catalog keys after their meaning; a second wording of the same sentence gets its own
semantic key, never a numeric suffix.
Preview capture accepts `--locales en,ko,en-XA`.

## Code

- Native UI: [Which tool](../../apps/loupist/e2e/README.md#which-tool)
