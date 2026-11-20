# Evidence and the browser

Evidence and Source readers localize coverage, source class, and material labels. A value the renderer does not recognize reads “Unspecified” (미지정). Saved excerpts retain the source text.

A retrieved website citation opens that page in the panel browser first. Other retained sources open their saved copy. Citation and number chips inspect a source without a new model read.

Reading: links open in the panel's browser. ⌘-click on macOS, Ctrl-click elsewhere, or a middle click opens the system browser instead; the same rule holds for citation and number chips, source rows, host-post source chips and Evidence's Open live page. A retrieved website opens its page there first; the saved passage stays in Evidence when that open fails or when the reader asks for it. Any other retained source opens its copy in Evidence. Day
dividers separate messages from different days, and "New since you left" marks the
first answer that finished while the chat was out of view (window hidden, or another chat or page showing; a visible window behind another app still shows the chat; per device), clearing on
scroll past or send. Answers over 600 words or with three headings get a collapsed
Outline of up to eight H2/H3 links. Editing any sent message while no reply runs
calls Rust `chat_branch`: the tail from that message becomes a stored version of the
same chat (at most 16) before the edited text is sent, and Earlier/Later version
switches through `chat_version_switch`; the Pi session cache is cleared so the next
turn starts cold from SQLite. Sent images show as thumbnails that open one lightbox for the chat (`ChatLightboxProvider` in `ChatView`): previous and next step through every image sent in the chat, its loaded images are released when switching chats, the activity sheet's image rows open it too, and "Add to question" attaches the shown image to the composer tray through the tray's own checks, without sending.

### Chat reading column and in-app browser

The chat title and reading pane receive the same responsive gutter. Their container query targets these descendants of `chat-workspace`, so a narrow chat beside a docked panel adapts even in a wide window. The resumed/stopping status stays in its own header row. Context belongs to the chat body below that row, so it cannot straddle the header boundary.

Failed replies show their localized `ChatErrorState` once. `ChatMessageCard` omits the host's generic terminal placeholder from the body and copy action; an actual partial answer stays visible and copyable. Rust marks the placeholder with `placeholder` on the terminal `error` event and on the stored message's DTO; stored replies keep only the placeholder text (Pi transcripts and exports read it), so Rust recognizes that text when projecting a thread and the renderer never matches it.

`.chat-reading-pane` is the chat column. Working context, the thread, the queue, notices, and the composer share one width: `min(100% - 2 × gutter, 760px)`, with a 20px gutter that becomes 12px when the `chat-workspace` container is at most 760px. The reading column centers in its available region. Nothing is reserved beside it: the [activity sheet](chat-composer.md#activity-sheet) docks beside `.chat-workspace` inside `.chat-layout` only while open and only when the chat keeps 420px, and otherwise overlays from the right. Working context inside the reading pane is `ChatWorkingContext`. All Chats covers the conversation when the window matches `(max-width: 899px)` or the `.chat-tab-panel` is wider than 0 and under 700px. With enough room, its 280px list docks beside a thread at least 420px wide. A docked workspace panel draws at `min(saved width, stage room)`. That fit updates stage size only; a deliberate resize is what changes the saved preference, and the saved size returns when the window has room. Inside a chart card, the title and actions wrap when the `chat-artifact` container is under 600px. The chart engine header stacks when its container is under 620px. A failed chart body (`role="alert"`) uses `height: auto` and `min-height: 0`.

The tone control is `fieldset.loupist-tone-field` (`display: block`) with a full-width legend. The choices use a separate `.loupist-tone-options` grid so the legend keeps the field width in WKWebView. The same rule covers every fieldset: `connect-engine` (onboarding “use a model on this Mac”), `journal-check-fields`, the schedule editor’s day and time fields, profile fields, and connection setup. Grid or flex stays on an inner element. `npm run test:native` calls `assertFormLegendsFit` on `[data-loupist-tone]` while creating the first Loupist and again for New Loupist. `verifyNativeToneLayoutFix` (`apps/loupist/e2e/native-forms.mjs`) is a separate macOS WKWebView probe: it reinstalls the old grid-on-fieldset layout, records geometry, then reloads the fixed field. Smoke does not call it. A Chrome design preview does not show this collapse, so it cannot clear the heading.

`browser-ready`, `browser-blocked`, and `browser-failure` (`src/preview/browserScenarios.ts`) open `https://issuer.example/hanul/q3` from its answer citation through the app. Ready shows the browser viewport, the article address, and an enabled Back control, with no alert. Blocked emits `navigation_blocked` / `unsupported_protocol` after that open and keeps the article, the address, and Back; the alert is the localized web-address sentence. Failure rejects the open with an unrecognized code and keeps the requested address, with Back disabled, Try again enabled, and the localized “could not open” sentence. Rust’s internal sentence stays out of the panel. `about:blank` events do not replace the address. From `apps/loupist`:

```sh
npm test -- src/preview/scenarios.test.tsx -t browser-
npx tsc --noEmit -p tsconfig.app.json
```

Those commands check the renderer. The native step is `verifyNativeBrowserNavigation` inside `npm run test:native` ([suite table](../../apps/loupist/e2e/README.md#deterministic-native-suites)). A finished run of that step is the WebKit evidence. Preview capture is a separate Chrome matrix.

A retrieved website citation (`web_fetch`, or another `web_*` page tool other than search) opens that credential-free page in a browser tab first and reuses one tab in that chat for the same address. The saved passage is not written into the workspace layout. View saved passage opens it in the Evidence reader when the open fails and after the page has loaded. The browser tab keeps the citation that opened it, including after a redirect and beside another page. Finance, files and other non-web citations still open the Evidence reader. An equity price-quote basket stays a compact receipt anchored to the control that opened it: provider, retrieval time, the quotes `JSON.parse` could read, coverage, and the technical original on demand. A number or grid cell uses the exact pointer, or the exact ticker only when no pointer was supplied; a pointer and a different ticker are not merged. A truncated or invalid quote-tool excerpt stays that receipt with no rows taken from the broken text. A value the host already checked can be shown as a checked value. It does not open the raw reader. Retrieval time is not the quote time. `quote-evidence` in `src/preview/quoteEvidenceScenarios.ts` is that receipt.

In Chat, the price-quote receipt fits inside the thread, below the page header and above the composer. A short window scrolls the receipt's content without covering the input. A remounted citation is found by its evidence id, so the receipt stays beside that chip.

## Opening from chat

Every chat control opens one typed `ChatDestination` (`src/chat/navigation.ts`) through `useOpenProps`, and `OpenProvider` layers add policy before anything opens. Each layer is one `useOpenLayer` over its parent (`src/components/navigation/Open.tsx`): it passes the destination up, substitutes another, or opens it itself, and a system-browser open passes through every layer. `TurnProvider` resolves a citation against its answer: an exact Memory read opens that Memory in Evidence, an `artifact_read` receipt opens the latest saved document, chart or grid in its chat through `chatArtifactRead`, other execution output reveals its step in the thread, a quote basket opens its receipt, an `asset_page_read` result opens its asset page when the receipt names the asset, and everything else goes up with the step that produced it. `ChatView` turns a link to a page this chat already kept into that retained source. A reader drawn over the chat closes itself first (`useOpenDismissing`). `useAppOpen` (`src/app/useAppOpen.ts`) routes the rest: a panel reader opens beside the visible chat, or beside another chat of the same Loupist that it shows first (`openIn`, which the briefing reader uses), and a page, asset, schedule or settings destination opens its own view. A `lens` destination becomes that record's Memory location in Evidence, titled in the interface language (`lensMemoryLocation`). An `Open` resolves once the destination is routed and rejects only on a real failure; `useGo` wraps it for a page or view switch that has no failure state of its own. `wantsExternal` is the only place that reads the modifier keys. The [ledger](chat-answer.md) and the footer's Sources list are leaves of the same path: a step's page is a `link`, a source is `evidence`, and neither the ledger nor the card takes an open callback.

Document readers receive the same workspace path as chat. Relative file links open Files, and local raster images are loaded through the scoped host `workspaceRead` reader. Only complete PNG, JPEG, GIF or WebP data URLs returned by that reader are previewed; no filesystem or remote URL is loaded by this path. Clicking a preview opens its file. Invalid paths, refused reads and unsupported images keep a text fallback.

Artifact read links use only the leading host metadata object, which survives a truncated result body; text inside the document cannot supply the target. Later answers may reuse an artifact-read citation retained in the same visible chat, even when the later turn has no new evidence. The chat-scoped lookup contains only identified artifact reads; other missing citations remain unavailable. Rust rechecks Loupist/chat ownership when resolving the latest revision. An unidentifiable receipt retains its existing execution-step fallback.

Every numbered citation opens something: `evidenceTarget` always resolves to a target. A leaf rendered outside every opener (one component in a test or preview) is plain text, not a button that does nothing. An original whose text was not retained, such as a host post's source, opens its page in the browser tab instead of a reader that would only say the text is gone. Crossed-line posts open a source through the citation or its asset page. A stored price chip opens the provider URL or a receipt naming the retained dataset and revision.

Icons describe where a control opens. The panel icon (`PanelRightOpen`) marks controls that open in the side panel: source rows and Open live page. `ExternalLink` marks only a system-browser action, such as the browser tab's Open in system browser. A link inside answer prose shows no icon, because its underline already marks it as a link.

## Citations and number chips

Text the user pasted into a Chat (evidence kind `user_provided`, or a saved read marked `user_provided`) is labelled "User-provided" ("사용자 제공") in its chip, card and number popover, and a capture with no saved title reads "Pasted text"; each capture is its own numbered source. Citation links preserve the answer’s visible text, including bare integers such as 100 and 79. A numeric label may be a value rather than a footnote, so it is never replaced with the retained source ordinal. The accessible name includes the visible label and the retained source title/ordinal. Host-matched number chips keep their value and verification status. A link written as `citation_url=#evidence-…` (the tool result's field name kept in the target) is read as that `#evidence-…` link. A link whose evidence has not arrived, or never will, is plain text, and a link the renderer refuses (such as `javascript:` or `data:`) keeps only its text, never a "[blocked]" marker. A citation or number link keeps what touches it on its line: the punctuation or Korean particle after it ("8.4%였고,", "[source].") and an opening bracket before it, so a period never starts a line alone.

Citation chips open a card (title,
domain and date, and either a short quote line or the passage) after 150 ms of hover or on keyboard focus; Esc closes it and
a click opens the page when the citation is a retrieved website with a credential-free URL (⌘/Ctrl-click or a middle click opens it in the system browser), the quote receipt when the source is a price basket, and Evidence otherwise. In windows 720 px wide or narrower hover opens nothing: a 500 ms
press and hold opens the card as a dialog that stays until a press elsewhere or Esc, and
a tap still opens that same citation. The number stays inline; the press area is the 28px control size. A number chip ([what it shows](../analysis/surfaces.md#where-each-item-appears))
is a button named by its value and source ("8.4 percent, calculated. Show the
math"), with a status other than matched in its description. ⌘/Ctrl-click or a middle click on a number with a source address opens that page in the system browser and leaves the popover closed. The figure stays in the line; its press area is 28px tall. Its popover is a
non-modal dialog labelled by its title, whether it opened after 150 ms of hover (not
in windows 720 px wide or narrower), on keyboard focus or on click. A click pins it
and moves focus to its first action; an unpinned popover closes 120 ms after the
pointer leaves. Tab from the number moves into the popover's controls in reading
order, Shift+Tab from the first returns to the number, Tab past the last closes it
and continues in the answer, and Esc closes it and returns focus to the number.
A passage popover marks the number in the retained excerpt by the check's `span`. When
the excerpt lacks it and the host matched it in the full retained result, the check
carries `context` instead (at most 240 characters of the matched field, UTF-8 byte
offsets of the number) and the popover shows that text with the number marked the same
way, under the same source and published date. A `span` wins; offsets that do not
decode inside the text leave the excerpt unmarked.
A number linked to a `calculate convert` result opens a popover titled "Currency
conversion" with the host's formula as retained (the rate's source and date, or "assumed
rate"). Its two inputs read as follows. The amount is printed in the currency and scale
the request declared ("$412.6m"), beside its own source and published date. When Rust
recorded `currency_basis: "declared"` on it (the citation matched a row or passage that
states no currency), one line under the amount says the currency was taken from the
request, not the source. No other input, operation or older answer shows it. The rate is
a plain number with the host's unit text, target units per one source unit ("1,334.87
KRW per USD"), beside the `fx_rate` source by provider name (European Central Bank) and
the rate's publication date, not the day it was read; a reference without a publication
date shows its provider and citation and no date. An uncited rate is the request's
assumption: it shows "Not linked to a source" and no provider or date. The formula is
printed exactly as the host wrote it, custom input labels included, and an input whose
saved value is missing or blank has no row (a stored 0 is a value). The popover
words what the saved check holds; it never refreshes, recomputes or re-dates a
conversion.
Opening a number popover or citation card closes any other number popover or citation card; their content and placement are prepared only while open. A number popover flips below the number rather
than cover the chat headers. The newest answer keeps its action row; earlier answers
show it on hover or focus.
