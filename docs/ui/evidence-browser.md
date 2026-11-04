# Evidence and the browser

A retrieved website citation opens that page in the panel browser first. Other retained sources open their saved copy. Citation and number chips inspect a source without a new model read.

Reading: links open in the panel's browser (⌘/Ctrl-click or middle click opens the
system browser). A retrieved website opens its page there first; the saved passage stays in Evidence when that open fails or when the reader asks for it. Any other retained source opens its copy in Evidence. Day
dividers separate messages from different days, and "New since you left" marks the
first answer that finished while the chat was out of view (window hidden, another chat, or the app in the background with the chat on screen; per device), clearing on
scroll past or send. Answers over 600 words or with three headings get a collapsed
Outline of up to eight H2/H3 links. Editing any sent message while no reply runs
calls Rust `chat_branch`: the tail from that message becomes a stored version of the
same chat (at most 16) before the edited text is sent, and Earlier/Later version
switches through `chat_version_switch`; the Pi session cache is cleared so the next
turn starts cold from SQLite. Sent images show as thumbnails that open one lightbox for the chat (`ChatLightboxProvider` in `ChatView`): previous and next step through every image sent in the chat, the rail's thumbnails open it too, and "Add to question" attaches the shown image to the composer tray through the tray's own checks, without sending.

### Chat reading column and in-app browser

The chat title and reading pane receive the same responsive gutter. Their container query targets these descendants of `chat-workspace`, so a narrow chat beside a docked panel adapts even in a wide window. The resumed/stopping status stays in its own header row. Context belongs to the chat body below that row, so it cannot straddle the header boundary.

Failed replies show their localized `ChatErrorState` once. `ChatMessageCard` omits the host's generic terminal placeholder from the body and copy action; an actual partial answer stays visible and copyable. The stored message and terminal event remain unchanged.

`.chat-reading-pane` is the chat column. Working context, the thread, the queue, notices, and the composer share one width: `min(100% - 2 × gutter, 760px)`, with a 20px gutter that becomes 12px when the `chat-workspace` container is at most 760px. The reading column centers in its available region. When `.chat-workspace` is at least 1120px wide and cards exist, 324px of the right side is reserved for a 300px context rail with a 12px edge inset. It starts 8px below the top of `.chat-body`, after any resumed/stopping header, and stays clear of the answer and composer. Below that width, the same cards sit in their own row above the transcript, aligned right with the responsive gutter. The compact row scrolls within 30% of the chat body height; it opens no overlay. Without context the reading region fills the chat workspace. Working context inside the reading pane is `ChatWorkingContext`, not the rail. All Chats covers the conversation when the window matches `(max-width: 899px)` or the `.chat-tab-panel` is wider than 0 and under 700px. With enough room, its 280px list docks beside a thread at least 420px wide. A docked workspace panel draws at `min(saved width, stage room)`. That fit updates stage size only; a deliberate resize is what changes the saved preference, and the saved size returns when the window has room. Inside a chart card, the title and actions wrap when the `chat-artifact` container is under 600px. The chart engine header stacks when its container is under 620px. A failed chart body (`role="alert"`) uses `height: auto` and `min-height: 0`.

The tone control is `fieldset.loupist-tone-field` (`display: block`) with a full-width legend. The choices use a separate `.loupist-tone-options` grid so the legend keeps the field width in WKWebView. The same rule covers every fieldset: `connect-engine` (onboarding “use a model on this Mac”), `journal-check-fields`, the schedule editor’s day and time fields, profile fields, and connection setup. Grid or flex stays on an inner element. `npm run test:native` calls `assertFormLegendsFit` on `[data-loupist-tone]` while creating the first Loupist and again for New Loupist. `verifyNativeToneLayoutFix` (`apps/loupist/e2e/native-forms.mjs`) is a separate macOS WKWebView probe: it reinstalls the old grid-on-fieldset layout, records geometry, then reloads the fixed field. Smoke does not call it. A Chrome design preview does not show this collapse, so it cannot clear the heading.

`browser-ready`, `browser-blocked`, and `browser-failure` (`src/preview/browserScenarios.ts`) open `https://issuer.example/hanul/q3` from its answer citation through the app. Ready shows the browser viewport, the article address, and an enabled Back control, with no alert. Blocked emits `navigation_blocked` / `unsupported_protocol` after that open and keeps the article, the address, and Back; the alert is the localized web-address sentence. Failure rejects the open with an unrecognized code and keeps the requested address, with Back disabled, Try again enabled, and the localized “could not open” sentence. Rust’s internal sentence stays out of the panel. `about:blank` events do not replace the address. From `apps/loupist`:

```sh
npm test -- src/preview/scenarios.test.tsx -t browser-
npx tsc --noEmit -p tsconfig.app.json
```

Those commands check the renderer. The native step is `verifyNativeBrowserNavigation` inside `npm run test:native` ([suite table](../../apps/loupist/e2e/README.md#deterministic-native-suites)). A finished run of that step is the WebKit evidence. Preview capture is a separate Chrome matrix.

A retrieved website citation (`web_fetch`, or another `web_*` page tool other than search) opens that credential-free page in a browser tab first and reuses one tab in that chat for the same address. The saved passage is not written into the workspace layout. View saved passage opens it in the Evidence reader when the open fails and after the page has loaded. The browser tab keeps the citation that opened it, including after a redirect and beside another page. Finance, files and other non-web citations still open the Evidence reader. An equity price-quote basket stays a compact receipt anchored to the control that opened it: provider, retrieval time, the quotes `JSON.parse` could read, coverage, and the technical original on demand. A number or grid cell uses the exact pointer, or the exact ticker only when no pointer was supplied; a pointer and a different ticker are not merged. A truncated or invalid quote-tool excerpt stays that receipt with no rows taken from the broken text. A value the host already checked can be shown as a checked value. It does not open the raw reader. Retrieval time is not the quote time. `quote-evidence` in `src/preview/quoteEvidenceScenarios.ts` is that receipt.

In Chat, the price-quote receipt fits inside the thread, below the titlebar and above the composer. A short window scrolls the receipt's content without covering the input. A remounted citation is found by its evidence id, so the receipt stays beside that chip.

## Citations and number chips

Numbered citation chips use the retained source list’s number for both the visible chip and its accessible name, even when the answer supplied a different reference number. Named citation links retain their visible words in the accessible name alongside the source title.

Citation chips open a card (title,
domain and date, and either a short quote line or the passage) after 150 ms of hover or on keyboard focus; Esc closes it and
a click opens the page when the citation is a retrieved website with a credential-free URL, the quote receipt when the source is a price basket, and Evidence otherwise. In windows 720 px wide or narrower hover opens nothing: a 500 ms
press and hold opens the card as a dialog that stays until a press elsewhere or Esc, and
a tap still opens that same citation. The number stays inline; the press area is the 28px control size. A number chip ([what it shows](../analysis/surfaces.md#where-each-item-appears))
is a button named by its value and source ("8.4 percent, calculated. Show the
math"), with a status other than matched in its description. The figure stays in the line; its press area is 28px tall. Its popover is a
non-modal dialog labelled by its title, whether it opened after 150 ms of hover (not
in windows 720 px wide or narrower), on keyboard focus or on click. A click pins it
and moves focus to its first action; an unpinned popover closes 120 ms after the
pointer leaves. Tab from the number moves into the popover's controls in reading
order, Shift+Tab from the first returns to the number, Tab past the last closes it
and continues in the answer, and Esc closes it and returns focus to the number.
Opening one closes any other number popover, and it flips below the number rather
than cover the chat headers. The newest answer keeps its action row; earlier answers
show it on hover or focus.
