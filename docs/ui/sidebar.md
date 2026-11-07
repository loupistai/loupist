# Sidebar

The header selects a Loupist. The sidebar selects a Chat, opens Search, starts a new research session from a "New research" row under Search (the only new-session control, with its shortcut shown at the row's end), and lists Needs attention. Below 900px it opens as a sheet, and the page behind it wraps into the visible strip so a line is not sliced. The header in that strip hides the panel-kind label and keeps the selected Loupist name inside the profile row. Settings replaces it. The command palette, exports and print live here too.
On macOS the window controls sit in a full-width titlebar, 44px tall so its contents share the traffic lights' vertical center (those buttons are placed at y 22). The sidebar and its divider start below that bar.
The shell owns the viewport height. Its body uses the space remaining below the titlebar, and the desktop sidebar is positioned inside that body; it does not subtract a separate titlebar-height estimate from the viewport. The frame, sidebar footer and chat composer stay inside the body when the bar grows. Header sizing has one rule rather than competing fixed heights at each breakpoint.
The right end of the header selects Lens, World, or Chat materials, followed by top/bottom and left/right split controls. At 1100px and wider, the space before those controls shows the trading status of the markets the user follows (at most two, plus a count of the rest). It hides on a narrower window. There the other profiles keep their avatars in the row, and the selected name stays within 240px.
The application sidebar's Settings action and the Settings sidebar's Back action share one footer style:
a 32px button with 8px padding and a top divider (49px total), with the same icon and label alignment,
text color, corner radius and hover state. The footer does not shrink when the navigation scrolls.

## Chat selection

Closing New Loupist returns keyboard focus to the control that opened it, including the header plus after scrolling the form in a narrow window. Cancelling keeps the active profile and its unsent chat draft.

The upper-left Loupist selector switches the entire workspace and opens the selected Loupist’s Bot. Each profile has an avatar and name in a horizontal, scrollable row. Below 1100px an unselected profile shows its avatar, and the selected name stays within 240px so those avatars remain in the row. An arc rotates around the avatar while that Loupist has an active Chat run, including background runs. With reduced motion, the arc stays still. The adjacent controls create a profile or open the selected profile’s settings. Creation and settings offer ten illustrated navigation objects, including a loupe, caravel, compass and ship’s lantern. Existing icon IDs map to fixed portraits, and profiles without a chosen portrait use a stable choice based on their ID. The sidebar lists Bot first, then recent research sessions. The plus beside Research sessions starts a draft. Search finds older sessions. The sidebar marks the current conversation. World opens the map and Lens opens research principles. The Bot’s menu opens scheduled work. Settings remains in the sidebar footer. Existing Chat and home IDs remain valid.

## Needs attention

The sidebar's "Needs attention" group (at most five rows) comes from the Rust
`attention_list` command: refused tools still waiting for approval (re-checked with the
connection service), chats whose finished last answer asked the user a question
(`question`: Rust reads the stored answer with `asks_user` — its last line, outside
code, headings, quotes and tables, ends with a question mark and is not a closing offer
such as "Want me to…?" or "…할까요?" — until the user replies or 14 days pass), crossed
asset-page lines (`lineCrossed`, until the page
is opened), unread scheduled results (flagged when a briefing found a conflict), watcher
alerts delivered but not yet on screen (`watchAlerts`: one row per Loupist with the count,
opening its home Thread), and dated Journal checks due within seven days or overdue. The renderer
writes each as one sentence naming the Loupist (and the connection or check claim),
wrapping to two lines. Each Loupist row
shows one status: amber dot for waiting on the user, then the running spinner, the
unread badge, and a grey dot for a run that finished out of sight.
`attention_notify` builds an OS notification body from the stored answer, only with
the user's notification permission and at most once per chat per minute; it is
used for approvals and runs that finish while the window is hidden. While the main window is not focused, Rust counts the rows of the same list that arrived since the window was last in front (grouped results and alerts by their count; checks coming due are not counted), prefixes the window title with the count ("(3) Loupist") and sets the Dock badge; focusing the window clears both and marks the list seen (`commands/attention.rs`).

## Command palette

Native menu events reach React through the typed bridge's `onMenuAction` subscription.
Keyboard shortcuts come from one table (`src/app/shortcuts.ts`) that both the global
handler and Settings › Keyboard read. ⌘K opens the command palette (also from the
sidebar search and the "n more" row of Needs attention). The narrow navigation
sheet is not a modal, so ⌘K, the tab keys and the panel shortcut still work
while that sheet is open, and ⌘B still closes it. The search field keeps its 52px height and the result list scrolls. Results are grouped into Needs
attention, Chats, What you follow (opens the asset page), Map names, Principles,
Briefings and Actions, four rows per group. Map names, Principles and Briefings come from every available Loupist, at most eight (the active one first, then sidebar order), each read through its own `knowledge_map`, `principle_list` and `inbox_list` when the palette opens; a Loupist that has not answered within 2 s is left out of that opening. In each group the active Loupist's rows come first and every row names its Loupist; opening another Loupist's result selects that Loupist first.
"/" focuses the composer when no text field has focus.

## Export and print

Exports go through Rust
`export_save` (native save dialog; png, mp4, webm, md, csv; text capped at 20 MB and
the extension must match the kind): a Chat exports as Markdown with a line after
its heading naming the AI models that wrote the answers, citations as footnotes (a
checked number keeps its text and gains a footnote with its formula and inputs'
sources, or its source's title, date and URL) and the AI notice last, a data table
saves as CSV, a table card as CSV with sources or Markdown ([grid
cells](../analysis/grids.md#grid-cells)), and a chart (inline card or the panel's Chart view)
through "Save as PNG…": the drawn chart at 2× with a footer band (title, date · Loupist,
AI notice), from the snapshot its engine leaves (`components/charts/chartSnapshot.ts`).
PDF has no generator: "Print or save as PDF…" (a chat's sidebar menu and the palette,
the briefing reader) draws a light-theme sheet outside the app (`components/print/`) and
calls `window.print()`; print media shows only that sheet, so the system print dialog
offers Save as PDF. A chat prints the transcript of its Markdown export with the AI
notice after the footnotes; a briefing prints the reader as drawn, without its controls.
On macOS Tauri routes `window.print()` to `plugin:webview|print`, which the main
capability allows (`core:webview:allow-print`). One reduced-motion
guard in the tokens stops CSS animation and transition except status spinners.

## Code

- Shell / native menus: `src/components/app/`, `src/components/SettingsView.tsx`, `src/App.tsx`, `src-tauri/src/lib.rs`
- Export (Markdown, CSV, chart PNG, print to PDF): `src/chat/exportChat.ts`, `src/components/charts/chartSnapshot.ts`, `src/components/charts/SaveChartPng.tsx`, `src/components/print/`, `src-tauri/src/commands/export_file.rs`, `src-tauri/capabilities/default.json`
