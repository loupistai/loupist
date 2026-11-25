# Sidebar

The sidebar runs the full height of the window and is the map of the product, in this order: the Loupist (its face, name and focus; the row opens the switcher), Search (⌘K), Needs attention, then one row each for Bot (⌘1), Following (⌘2), World (⌘3), Lens (⌘4) and Schedule (⌘5, with the selected Loupist's unread scheduled results as a count), then Research sessions under their own heading with a plus (New research, ⌘N; its tooltip says new research digs into one question and that scheduled runs live in Schedule and continued runs also appear here), then Settings at the foot. With no research sessions, the list says what new research is for instead of showing a bare heading. Settings replaces the sidebar with its own sections and a Back row; it opens on General, which holds the menu-bar setting, says in one sentence that research stays on this Mac, puts where each kind of data goes behind a disclosure, offers the AI notice again and, on a computer whose holdings were exported, keeps the export date with Show file and Save a copy… ([holdings export](../runtime/storage-authority.md)). The pseudo-locale (en-XA) is offered in Appearance only in development builds and the preview.
Every row is a destination: the place on screen is a raised row with an accent icon, and choosing it again keeps it open. World and Lens are full pages in the main area (`map` and `lens` in `AppTab`, `src/app/navigation.ts`), each under its own page header; the conversation’s view of World, Evidence and Work stay in its [side panel](workspace-panel.md). Following opens the `following` page (no ⌘ key).
From 940px the sidebar starts expanded; from 700px (the window minimum) to 939px it starts as a 76px icon rail with the face, Search, the places, the plus and Settings on one centerline (`useSidebarOpen` in `AppNavigation.tsx`; each width range keeps its own choice; ⌘B and View › Toggle Sidebar fold and restore it). In the rail each item is only its icon: hover, keyboard focus and press do not reveal another control, and a click anywhere on the item opens it. Research sessions are not listed in the rail. Only previews narrower than 640px make it a drawer. On macOS the window controls (placed at x 12, y 22) sit over the sidebar's first 44px row, an empty drag region; no control sits under them. From 940px the expanded sidebar has no toggle button; while it is a rail or a drawer, and below 940px after the toggle has expanded the rail, the toggle sits at the start of the page header, so the button that expanded the rail folds it back.
The sidebar's right edge is its resize handle (`SidebarResizeHandle`): an 8px target with a col-resize cursor that shows a 2px accent line on hover, keyboard focus and while dragging. That line is its only focus mark (the app-wide focus ring would box the sidebar's own content), and it starts below the window-control row. Dragging sets the labelled width between 208px (the face row and "Research sessions" with its plus fit in English and Korean) and about a third of the window, at most 420px; CSS holds the same limits as the window changes. Ending a drag left of 140px folds it into the icon rail, and dragging out of the rail restores it. Double-click returns to 248px. A plain click does nothing, so the edge never toggles by accident. It is a focusable vertical separator (`aria-valuemin` 76, the rail; `aria-valuemax` the current limit; `aria-valuenow` the width): the arrow keys step along a 20px grid from 208px that passes through the 248px default, so one press back returns to it (left of 208px folds to the rail, right from the rail restores 208px), Home folds to the rail and End goes to the widest. While dragging only the root's `--loupist-sidebar-width` changes, transitions are off and the page column reflows with it. The width is a per-machine window preference kept in local storage (`loupist.sidebarWidth`) beside the last view and conversation; the default is not stored.
Each page starts with one 56px header (`AppHeader`) that identifies the current Loupist, conversation or page. On Bot it is the Loupist's face and name (its focus is in the sidebar), and "Working" with the face's ring while it has a run. A research session, Following, World, Lens, Schedule, an asset page and Settings show their title. A World or Lens page entered from a conversation leads its header with "Back to chat" (대화로 돌아가기), which returns to that conversation with its draft and panel as they were; moving between World and Lens keeps it, and a page entered from anywhere else has none. The page below does not repeat it: a Settings section opens with its one-line description (the section name stays an off-screen heading for assistive technology), and Loupist settings heads its page with the Loupist's name. A conversation's header has three actions. **Split top and bottom** and **Split left and right** open chat materials in the selected position. Pressing the active split again closes the panel. **Activity** toggles the [activity sheet](chat-composer.md#activity-sheet). The sheet's In use tab also opens chat materials. The header owns ⌘1–⌘5 and ⌘\, and is a drag region.
The application sidebar's Settings action and the Settings sidebar's Back action share one footer style:
a 34px row with the same icon and label alignment, text color, corner radius and hover state. The footer does not shrink when the navigation scrolls. Both footers stay at the foot of the window: on a short window the Settings section list scrolls inside the sidebar above Back, and scrolls as far as it takes to keep the open section's row in view there (Updates at 700 x 560).

Settings search finds the controls and help text shown by each settings page. A label matches in English or Korean. Choosing a result keeps the search in place until the user clears it or leaves Settings. A result names its section and opens it at the matching control, expanding its disclosure when needed. The index reads the labels presented by the available settings and excludes input values.

## Chat selection

Comparing preset templates keeps the Custom draft until New Loupist closes. Closing New Loupist returns keyboard focus to the control that opened it, or to the Loupist at the top of the sidebar when the switcher it came from has closed. Cancelling keeps the active profile and its unsent chat draft. On a window 620 px tall or shorter (down to the 700 x 560 minimum) its template cards show their titles alone, so the preview card with the face, name and the chosen template's focus stands whole above the fold of the scrolling form; each description stays in its card's accessible name. At that height the fixed footer places the background-reading notice to the left of Cancel and Create. Taller windows give the notice its own row above the buttons.

The Loupist at the top of the sidebar opens the switcher (`LoupistSwitcher`): every Loupist with its face, name and unread count, the current one checked, then Loupist settings and New Loupist. The switcher, Settings and Search distinguish identical names with a short focus excerpt. If the excerpts are identical or missing, each receives a stable profile number within the group. Choosing one switches the entire workspace and opens that Loupist's Bot. The same trigger can reopen the switcher while its previous menu is fading out. A face shows its working ring while that Loupist has an active Chat run, including background runs, and each running Loupist keeps a live status while the switcher is closed; another Loupist's unread results put a dot on the current face. With reduced motion, the ring stays still. The avatar is the Loupist's face, a round character holding a loupe (`LoupistAvatar`). Creation, onboarding's name step and Loupist settings offer ten face colours; each of the ten stored portrait IDs is one colour. Older icon IDs map to fixed colours, and profiles without a chosen one use a stable colour based on their ID. The sidebar lists Bot first, then recent research sessions. A session the host started for its own work is not one of them: a price setup, and a scheduled run's session until the user writes in it ([run sessions](../runtime/scheduled-runs.md#scheduled-chats)); its results reach Bot and Schedule. Search finds older sessions and these, until such a session goes with its run. The sidebar marks the current conversation, and lists it while it is open even when it is one the host keeps out (`getLoupistChatList`). Before anything is typed, the palette's Research sessions are the same recent ones. World opens the names and relationships view, and Lens opens research principles. On the expanded row, the Bot’s menu opens scheduled work; the rail does not show it. Settings remains in the sidebar footer. Existing Chat and home IDs remain valid.

### Running replies

A research session's row shows a small spinner while a reply runs in it, the open session's
row included. The renderer knows a run from what it already holds, without polling of its own
(`AppNavigation.tsx`):

- a reply the user sent there, or a run this window streams into it: the Chat's status while it
  is submitted or streaming. That covers a scheduled run writing into a listed session, which the
  schedule poll joins (`useScheduledRuns`), and a hand-off the open session has joined with
  `chat_watch`;
- a run Rust still has after the window reloads (`LoupistRunRegistry`, from `run_activity_list`),
  until the host reports it gone;
- a research hand-off from Bot, a typed one or a link's Look into it, whose session's thread
  carries `handoff_run` (`runningHandoffChats` in `src/chat/useHostedRun.ts`), whether or not the
  session is shown. Bot posts nothing while it works. The host lists the run on the session while
  this app holds it, announces its start (once the session holds the request) and its end with
  `inbox-changed`, and after a quit the next delivery pass settles it, so it is not listed.

The spinner stops when the reply finishes, fails or is stopped, and a run the app quit in the
middle of does not spin after the restart. Bot's row spins for a reply in Bot or any running hand-off only while Bot is not the
open view: there its header shows its own reply, and the session's row the hand-off. The spinner takes the row's one status
slot ahead of the finished dot, so a Bot reply that ended out of sight shows its grey dot again
once nothing runs. Runs the renderer is not told about do not spin: a scheduled run whose stream
the window could not join would need a host event when it starts and ends. A session below the five recent rows is not listed, running or not.

The spinner is the app's `Spinner` (`components/ui/spinner.tsx`) at 14 px in the accent ink, in
the slot the private mark uses, so the title never moves; the options button keeps its place at
the row's end on hover or keyboard focus, and is not shown in the icon rail. In the icon rail the spinner is 8 px in the button's corner. With reduced motion
the tokens' guard stops it, leaving its open ring still. Its name is "Running" ("진행 중"), a
fixed name with no live region, so nothing is announced as it turns; the
row button keeps the chat title as its name and is described by its marks' words ("Unsent
draft", "Running"), since a button's own marks are not read inside it.

## Needs attention

The sidebar's "Needs attention" group (at most five rows) comes from the Rust
`attention_list` command: refused tools still waiting for approval (re-checked with the
connection service), interactive chats with a live `ask_user` request
(`question`, until the user answers, the wait ends, or its task or session is deleted), crossed
asset-page lines (`lineCrossed`, until the page
is opened), unread scheduled results (flagged when a briefing found a conflict),
and dated Journal checks due within seven days or overdue. The renderer
writes each as one sentence naming the Loupist (and the connection or check claim),
wrapping to two lines. Each chat row
shows one status at its end, first match: the spinner while a reply runs
([running replies](#running-replies)), a grey dot for a run that finished out of sight
(another chat or page showing, or the window hidden; a visible window behind another app
still shows its chat), then the private-chat mark.
`attention_notify` builds an OS notification body from the stored answer, only with
the user's notification permission and at most once per chat per minute; it is
used for approvals and runs that finish while the window is hidden. While the main window is not focused, Rust counts the rows of the same list that arrived since the window was last in front (grouped results and alerts by their count; checks coming due are not counted), prefixes the window title with the count ("(3) Loupist") and sets the Dock badge; focusing the window clears both and marks the list seen (`commands/attention.rs`).

## Command palette

The native View menu, sidebar and page header share the order in `src/app/nativeMenu.json`.
⌘1 opens Bot and selects the Loupist’s home conversation. ⌘2 opens Following,
⌘3 opens World, ⌘4 opens Lens and ⌘5 opens Schedule. The command palette and
Settings › Keyboard list those same destinations.
`APP_PAGES` supplies their location headings. Each keyboard destination has a row in
`APP_DESTINATIONS` and a page entry. `src/app/useAppSurface.ts` holds the page and the layers over it
(Loupist settings, an asset page, the Gallery, a pack review) and derives what the
sidebar, the workspace panel and the stage may show. ⌘K opens the command palette (also from the
sidebar search and the "n more" row of Needs attention). The narrow navigation
sheet is not a modal, so ⌘K, the tab keys and the panel shortcut still work
while that sheet is open, and ⌘B still closes it. The search field keeps its 52px height and the result list scrolls. The dialog and its field are named Search, as the sidebar row is. When nothing matches, a status line under the field says so and suggests another word, a ticker or a session title; opened from "n more" with nothing waiting, it says nothing needs attention. Results are grouped into Needs
attention, Bot (home threads), Research sessions, Following (opens the asset page), World names, Principles,
Briefings and Actions, four rows per group. A research session is found by its title, and by its Loupist's name only from two typed characters (one letter is in most names), with title matches first; Following lists an asset by the name its page shows and finds it by either name. Searching for `map` or `지도` finds the Open World action. Actions include New Journal entry, which opens World's Library on its Journal tab with the new-entry form open. World names, Principles and Briefings come from every available Loupist, at most eight (the active one first, then sidebar order), each read through its own `knowledge_map`, `principle_list` and `inbox_list` when the palette opens; a Loupist that has not answered within 2 s is left out of that opening. In each group the active Loupist's rows come first and every row names its Loupist; opening another Loupist's result selects that Loupist first.
"/" focuses the composer when no text field has focus.

### Native menu and keyboard shortcuts

The app rebuilds its menus when the interface language changes in Appearance. The
shared menu table supplies titles in that language for every item. It uses the
sidebar’s New research and destination names and fills in the running bundle’s
display name for About, Hide and Quit on macOS. AppKit handles the predefined
actions and their key equivalents. The pinned menu wrapper disables AppKit title
validation, so the app synchronizes the predefined full-screen item’s title with
the native window and disables AppKit’s automatic full-screen item.
AppKit also adds AutoFill, Start Dictation… and Emoji & Symbols to the Edit menu,
titled in the bundle’s development language (English) whatever the interface
language. The app turns off the last two at launch and, after setting each menu,
removes the AutoFill item AppKit appends (`keep_own_edit_items` in
`src-tauri/src/app_menu/macos.rs`), so Edit lists only the table’s items, each once.
File › New Loupist opens the creation dialog. File › Print
(⌘P) uses the command palette’s chat print action when that action is available.
Close Window (⌘W) sends the same close request as the red window button,
including the setting that keeps Loupist running for scheduled work.

The main window reports which actions are possible from the current shell,
selection and mounted dialogs. It sends the current state again on focus and
when a webview file picker finishes. Without an available Loupist, research and
destination commands are disabled. Settings, updates and creation are available
when the shell is ready. During onboarding those shell commands are disabled.
When the webview starts loading or its process exits, the host clears the last
document’s state and enables commands for the available window until the
renderer reports again. Renderer commands still check whether they can run.

On macOS, window command availability follows the visible main window and
AppKit’s current modal window or attached sheet. A picker, save panel or print
sheet disables commands that would act behind it for the life of the panel.
The OS handles application and editing actions on their current target.

View › Zoom In, Zoom Out and Actual Size give the main webview ⌘plus, ⌘minus and
⌘0. The app saves the chosen scale in its local application data and restores it
at launch. The browser panel translates its viewport from CSS pixels to window
coordinates at that scale. Window › Zoom on macOS remains the system’s window
size action.

Windows and Linux use their window menu bar and Ctrl shortcuts. They omit the
macOS Services and application-hiding items and use F11 for full screen. The
Linux window actions use the host’s window methods where the predefined items
are unavailable. Edit actions retain the platform’s support for those native
items. A GTK3 build supplies Undo and Redo through the focused webview’s
keyboard shortcuts, and the native menu leaves the unsupported items disabled.


## Export and print

Exports go through Rust
`export_save` (native save dialog titled by the renderer in the interface language,
"Save image" / "이미지 저장" by kind (`exportTitle`); png, mp4, webm, md, csv; text capped at 20 MB and
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
guard in the tokens stops CSS animation and transition; a status spinner stays on screen as a
still glyph whose name carries the state.

## Code

- Shell / native menus: `src/components/app/`, `src/components/SettingsView.tsx`, `src/App.tsx`, `src/app/navigation.ts`, `src/app/useAppSurface.ts`, `src/app/useChatViewModel.ts`, `src-tauri/src/lib.rs`
- Export (Markdown, CSV, chart PNG, print to PDF): `src/chat/exportChat.ts`, `src/components/charts/chartSnapshot.ts`, `src/components/charts/SaveChartPng.tsx`, `src/components/print/`, `src-tauri/src/commands/export_file.rs`, `src-tauri/capabilities/default.json`
