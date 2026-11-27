# Chat composer

The composer attaches context and does not send it. It is a compact pill (24 px radius) as wide as the reading column: one row holding, in reading and tab order, the + menu, the message, the name and thinking level of the model that will answer and the accent Send tile. The message starts one line high and grows with its text, line by line, up to 200 px or 30% of the window height, whichever is less; past that it scrolls and keeps the caret's line in view. The controls stay on its last line. An empty composer uses its one-line CSS height, including after the first message in a research session with a docked panel. `ChatComposer` measures height only while the textarea holds draft text: the WebKit of supported macOS releases has no `field-sizing: content`, so a CSS-only composer stayed one line high there (`assertComposerGrows` in `e2e/native-forms.mjs` measures it in the WebView). The placeholder addresses the Loupist by name ("Message Lumi", "루미에게 메시지"), and the one-line notice ("AI can be wrong. Not a licensed financial adviser.") sits under the pill in 11 px. Text and object references appear at the textarea cursor; attachments and guidance sit above the row inside the pill, and whole-message settings (private chat, Skills and the as-of date) sit under it, aligned with the text. Focus on the composer, including the automatic focus of a new chat, turns its 1px edge to the accent ink. It draws no 2px outline, which would frame the whole composer. Controls keep their own outline. Styles: `src/styles/composer.css`. `/asof` is the [date control](../analysis/point-in-time.md#composer-date-control). Width and the browser: [Evidence](evidence-browser.md).
The composer shell keeps the same bottom inset while the side panel docks or overlays
the chat and while a reply settles. Draft text rewraps to the composer width.
The Copy answer tooltip sits below its button and follows its position
as the layout changes. Answer table, code and diagram controls use the current UI language for their tooltips and accessible names. Copy confirmation clears after a short delay; refreshed snapshots of the same answer, number or grid preserve it. A stopped reply shows the localized run state without the runtime's English stop marker in the answer or its copy action.
The transcript keeps public progress notes, findings and citations in reading
order. While work runs, short action labels grow down the conversation between
public notes. Consecutive successful calls with the same action can share a
count. The conversation owns the scroll, including when details are opened.
Commands, arguments and retained output stay behind the disclosure.
When answer text starts appearing, the automatic list gives way to its summary.
If that text becomes a preamble to another tool call, the live list resumes.
Opening details explicitly keeps them open while the answer arrives.
It updates in place as calls arrive. A public note separates successive
groups. Plan changes update the live summary in place. After completion,
the summary leads with its distinct linked sources when available. Expand
it to see the checklist and calls. A pending
approval is actionable with the group closed, and delivered files and charts
remain accessible from the answer. Expand the summary to inspect individual
steps, then open a step for its retained input, output or linked result.

The renderer derives labels and counts from the host's existing activity and
evidence records. Opening a detail reads that record on demand. Grouping,
expansion and display text are local UI state and add no messages to the
model's context.

For a financial researcher, the main task is to read a finding and check its
source. A full execution trace is useful when a figure or a missing result
needs investigation. This split follows Nielsen's progressive disclosure
guidance. Cursor's desktop Agents Window offers compact results and more
detailed activity views. The open-source goose Desktop defaults to folded
tool calls. Cherry Studio's desktop search component leads with a result
count and puts the output inside a disclosure. Loupist counts distinct
retained sources linked to the stage. Search hits and calculation outputs
do not qualify as original sources.
[Progressive disclosure](https://www.nngroup.com/articles/progressive-disclosure/) ·
[Cursor compact chat responses](https://cursor.com/changelog/3-4#compact-chat-responses) ·
[goose Desktop tool output](https://goose-docs.ai/docs/guides/managing-tools/adjust-tool-output/) ·
[Cherry Studio search component](https://github.com/CherryHQ/cherry-studio/blob/90d504fa70df2efe5ab03f282a0858aa47a411f9/src/renderer/components/chat/messages/tools/agent/WebSearchTool.tsx#L16-L35).

Waiting and Thinking appear at the working end of the turn.
A live paragraph keeps its place when a tool starts or a final
answer settles. Bursts of Markdown are revealed on animation frames at word
boundaries, catching up within a 160 ms window. Reduced motion, corrections,
and terminal replies display directly. A progress resync preserves the boundary
between the live draft and text already in the answer. Copy uses the settled answer.

While a reply runs, an empty composer shows only Stop. Typing makes Send next the filled
primary control: Enter queues the message for after this reply, and Guide this response
is the smaller control beside it (⌘Enter on a Mac, Ctrl+Enter on Windows). Shift+Enter
starts a new line. Stop stays next to them. Files, an as-of date, pins, or selected work
cannot guide the live reply; Send next queues that message with them. Guidance needs no
model choice, since the running reply keeps its own; a session Bot handed off is guided the
same way once it is open ([Bot](home-thread.md#home-thread)). Guidance that has
not been accepted yet reads as waiting on the composer. Once Pi accepts it, that note
switches to guiding and then clears, and a Guiding row stays in the timeline. A screen
reader hears the waiting note, then the Guiding row once as the conversation log adds it;
the accepted note and the composer's status region do not say it again. Rust stores
accepted guidance as a user message marked `steer`; it stays a user turn for the model
and reads as a "Guided: …" row between the question and its answer after the run; a
stopped answer says what it kept. Queued messages sit in a compact list above the
composer: a count, one short row each, and Edit, reorder, Remove, and Send now in that
row's menu. When Send now cannot go, the menu says why under it (the running reply, offline,
or what to reconnect). A stopped or failed reply holds the queue with a line that says so ("Response
stopped." / "Response failed."), adds "Queued messages were kept." only while a message
is queued, and offers Send next now. If another task is saving to this Loupist’s Memory, the queue shows a waiting line. When the host’s activity list clears its last Memory writer for that Loupist, the oldest queued instruction retries automatically. Send next now still retries immediately. Removing or sending a row from its menu moves focus to the row in its
place, or to the message when none is left. The list can fold. It keeps queue order and the queued payload. Up to three
next-step chips after the latest answer come from the conflicts and names it
found and fill the composer without sending. A chip names only a company World
matched by listing to something followed; currencies, places, sectors,
metrics and other or unknown kinds offer none, so an answer may have no chips. Leaving a view or switching Loupist does not cancel an admitted
Chat. Private chat is a checkbox in the composer's + menu; while it is on, a "Private · memory off" chip with an off button sits under the message row, and directly under the chip, in the secondary text style, what it means: "Private chat: your Loupist doesn't use or add to what it remembers. The chat is still saved, and still goes to your AI provider." It wraps to a second line in a narrow composer and is visible text, not a tooltip. The composer grows by those lines without moving the conversation: a reader at the end stays at the end, and a reader in history keeps their place. Without an AI account, Connect an AI account takes the send slot and the placeholder says so. Private chat is described under
[Memory permissions](../knowledge/memory.md#memory-permissions-and-retrieval).
Focusing a control below the fold scrolls it into the conversation. A sent question
glides to the top of the thread and the reply writes beneath it; the page moves by
itself only when a block arrives, never while one grows
([reading while it streams](chat-answer.md#reading-while-it-streams)). While the end of
the reply is below the fold, the live status and Jump to latest float over the bottom of
the thread, 12 px above the composer, without taking a row; Jump to latest follows the
reply until the reader scrolls up. A settled chat opens at its newest answer and stays
there while late content (images, tables, the morning message) arrives.

Send and Stop are one round 36 px tile at the composer's trailing edge (`ComposerTile`): the
same button stays mounted from idle to running and back, so its box never changes, and
its two icons cross-fade in place (opacity and a 0.8 scale, 120 ms; a swap under reduced
motion) while its fill eases from the accent to ink. Send keeps the accent fill with
nothing typed (it then ignores presses); while a reply runs, Stop is an ink tile with a
filled square. Stop ("Stop response", Esc in its tooltip; Esc in the composer stops the reply unless a
menu is open or the activity sheet overlays the chat, which Esc closes first) dims to
55% after a press and ignores presses until the host confirms. Focus stays in the
textarea through send, stop and commit (Send or Stop pressed from the keyboard returns focus
there, and so does removing an attachment, a chip or the private mark by its button), including the first message of a new research
session: the saved Chat it creates keeps the draft's composer and thread instead of
mounting new ones.
After a reply saves a Source, a running World job, or a queued job whose processing is ready, shows “Reviewing sources…” as a quiet line on the activity sheet’s Learned tab. A queued job blocked by a disconnected provider, paused processing or another readiness condition shows the useful status or action, such as “Reconnect your AI account” or “Source review is paused”, without a spinner. If readiness cannot be read, it says “Waiting to review sources”. Polling continues so reconnecting or resuming processing updates the row. As jobs finish, their additions
appear on that tab; when all work settles, the tab shows the chat's learned names or says nothing was saved.

The composer's "+" menu opens above the composer, aligned to its left edge like the `@` and `/` menus, and holds
Files…, Paste image, Screenshot of World (disabled with a reason when World is not
open or it is empty) and Add from Evidence (this chat's saved Sources as pins).
Choosing Files… or Paste image closes the menu. On macOS, Paste image reads a PNG
from the native pasteboard or converts TIFF to PNG through the host. It attaches one
image of up to 5 MB and reports an empty or oversized image. The OS paste shortcut
also works in the message field and is offered where the native read is unavailable.
Dragging files over the chat column shows one drop overlay with the limits. The tray shows each file's size. Each file may be up to 25 MB (images 5 MB); Rust enforces it in `prepare_chat_attachments` and refuses a larger file with `attachment_too_large`, which the composer words in the reader's language.
The composer reads each added file once, into a `data:` URL: that is the image's thumbnail, the draft's
saved copy and what is sent, because the app's [Content-Security-Policy](../runtime/permissions.md#renderer-content-security-policy)
blocks `blob:` images and reads. While a file is being read the tray says "Reading 1 file…"
and the file counts toward the message: Send pressed meanwhile waits for the read and sends the file with
the message. A file that cannot be read, or does not fit the tray's four, stops that send with its alert and
keeps the message. ⌘V attaches what the clipboard holds as files through the same checks
(size, count of four, type): a screenshot or a copied image, several at once, or files copied in Finder
under their own names. A clipboard image gets a name of its own, "Pasted image 2026-11-23 21.04.png"
("붙여넣은 이미지 …"), numbered when one paste holds several. A copied spreadsheet range or web selection
carries a picture of itself beside its text; the text is pasted and no image is attached, unless the only
text is the image's own file name or address (a Finder copy, a browser's Copy Image). Rust takes an
image's type from its bytes (PNG, JPEG, GIF, WebP), not from the clipboard's label. A paste while a reply
runs adds to the tray like any file; Send next queues it.
The message's text selection paints its own glyphs in the text colour on the system selection colour,
as every other field: the message's glyphs are transparent over the highlight layer, so a transparent
selection over the pale accent once read as no selection at all.

The composer has `@` for existing context and `/` for commands and Skills.
`@` searches files, knowledge, work and connections, including existing World
references, while a reply streams as well as while idle. It does not search the watchlist or offer free-form ticker selection.
`/` lists the bundled World (`world`), Lens and Journal Skills under Memory and
the `/asof` date control under Commands. In a debug launch with user Skills
enabled, the menu also lists enabled personal and Loupist Skills by name.
Typing after the slash searches all available enabled Skills by
name, ID and description in the interface language. Selecting a Skill adds its
chip to the message. Dollar amounts and ticker text,
including old `$SYMBOL:MARKET (Name)` messages, remain ordinary text. They do not
open a menu, attach a company chip, or add an asset card to an answer. Skills keep
their `$skill_id` submission syntax.

Shipped builds keep recorded custom connection approval outcomes and their source
names as read-only text. Waiting requests are absent from the answer and the
activity sheet. When custom connections are enabled in debug and a read-only
tool needs approval during an interactive run, Rust records it on `ChatProgress.approvals` and waits up to five
minutes; the card, under the step that waits (after the steps when the step is unknown; under the header while the steps are folded), offers Allow once, Always and Not now through
`chat_approval_decide` (Rust checks the Loupist), Always reuses the stored per-tool
approval that Settings › Connections shows and revokes, and Not now records a
`declined` refusal while the Loupist continues without the tool. The card says the
custom connection receives the request and that the approved tool can only read
data; it does not promise the request stays on the device. A card still waiting
when the run ends reads "No longer needed". A card decided from the keyboard hands focus to
its settled line, as a question card does once answered. Tools that are not read-only, and
unattended runs, keep the plain refusal. This is the only approval card: a native
command or file access is allowed or refused by Rust without asking, and a refused
step explains itself in place ([Native tool permissions](../runtime/permissions.md));
a new site after third-party text is refused the same way
([Trust](../runtime/untrusted-text.md)).

An [`ask_user`](../agent/turn-tools.md#turn-tools) call waits on a question card the same
way: Rust records it on `ChatProgress.questions` (at most four per reply), under the step
that asked, and the card offers one outlined button per option (the recommended one with a primary edge, tagged
Recommended, with its description under each label; it takes focus, so Enter chooses it, only when the card
arrives after the reply stopped streaming and nothing editable has focus; while the reply streams, focus stays put), an "Or answer in your own words"
field up to 500 characters and the waiting clock. Answers go through
`chat_question_answer {loupist_id, request_id, choice | text}` (exactly one; Rust checks the
Loupist, the option index and the length). The settled line reads "You answered: …" or,
when the wait ran out or the run stopped, "No answer · continued with <recommended
option>". A waiting question counts in the activity sheet's "Waiting for you" row, which reads "Needs your
answer" when only questions wait, and turns the footer to "Waiting for you"; one still waiting when the run ends is saved as expired.

A completed `connection_setup` step leaves a setup card under the answer
(`ConnectionSetupCard`, derived by `chat/connectionSetupBlocks.ts` from the step's
target, one card per entry). The card reads the connection's live state through
`marketplace_snapshot` and `loupist_capability_list`, never from the stored step, as
one union: loading, needs setup, off for this Loupist, ready, or unavailable (the entry
left the catalog or its runtime is missing). Needs setup opens the same
`MarketplaceSetupDialog` as Settings › Connections and refreshes when it closes; off
for this Loupist calls `loupist_capability_enable`; ready offers Ask again, which puts
the user's previous question back in the composer through the same path as Change the
rates. Needs setup says what setup asks for, read from the entry's own fields
(`setupNeeds`): a connection with a credential field shows the key sentence ("Any key goes
in the setup dialog and stays on this Mac; the Loupist never sees it."); a required field
that is not secret and not a list choice, such as SEC EDGAR's contact email, is named by
its label in the first line ("Before this Loupist can use SEC EDGAR, it needs: SEC
contact email."; several are joined with commas) followed by "Enter it in the setup window. It is stored on this Mac
and sent to SEC EDGAR with each request."; a connection with both shows both. A list
choice (Tradier's account type, Korea Investment's environment) starts on its first
option in the dialog and is sent nowhere, so it adds no sentence. No key passes through
Chat, and the connection works from the next message.
Preview scenarios `chat-connection-setup-needs-setup` and `chat-connection-setup-ready`
show it. A chat with both Memory flags off shows a
private mark; the chat title in the sub-header renames in place; a Loupist row shows a
draft dot while unsent text exists. After a Chat is successfully deleted, Rust
removes its unsent prompt, context and attachment drafts from the app profile; a
failed deletion keeps them. A draft write
that arrives after deletion cannot recreate them. Drafts for other Chats stay.

Selected workspace files, connections, quoted text, knowledge and work are inserted inline at the current cursor or selection. An action elsewhere that puts words or a reference in the composer (a chart's or table's Ask, Ask in Chat from World or Lens, Explain the disagreement, a suggested question, Use in Chat, a quote) then hands it the keyboard with the caret after everything in it (`focusComposerAtEnd`); the reference it adds does not place the caret itself, because WebKit would take focus there, before any text after it. The editor uses a native textarea with a synchronized highlight layer; reference styling has the same text metrics, so wrapping, scrolling, selection and IME remain native. The draft retains bounded UTF-16 ranges alongside its existing typed pins, selected artifact and chip metadata. Typing before a reference shifts its range; editing inside it detaches the structured reference and leaves ordinary text. Backspace immediately after a reference, or Delete immediately before one, removes that occurrence atomically. Repeated references keep distinct offsets and serialize at each position; their shared typed context stays until the last occurrence is removed. The named removal action removes every occurrence of that context. Named removal controls are available to screen readers and reveal themselves when reached with Tab. Inserting or removing one reference preserves the surrounding question and other references. External draft restoration waits for the textarea to adopt the saved text before anchoring its references.

Skills are whole-message modes and stay in a compact row below the textarea with the as-of date. Privacy status and model controls also stay below the text; attached file previews retain their own tray. Inline serialization keeps the reference's place in the sentence: connections use `@entry_id`, and workspace files use relative paths. Knowledge and work show their natural labels in the sent prose while their existing typed fields carry identity and ownership. No opaque editor key is put into the prompt. Rust removes retired company chips when `chat_draft_read` restores a saved draft. An inline ticker keeps its visible text and loses its reference metadata. A tray-only ticker becomes ordinary name, symbol and market prose before the existing question, and retained inline offsets shift by the prefix's UTF-16 length. Supported chips, pins, work and the as-of date stay intact. Legacy supported chips without inline ranges retain their previous serialization until the editor migrates them. Queued messages already hold serialized prose and typed context, so they need no chip migration. Selecting at least three words in an answer
offers "Ask about this", and selecting text in a panel reader offers "Add to
question". The button sits just under the end of the selection, or over its start
when the end is at the bottom of the view, and never on the selected text. An answer
selection paints its text only (WebKit would fill line ends and the space between
blocks), takes in checked numbers, citations and links, ends at the thread's end
when a drag leaves the last message, and never reaches the composer. The floating reader action renders inside its panel so it remains
visible and keyboard-reachable above a narrow overlay. A selection in a retained
Source resolves through Rust
`knowledge_source_selection_resolve` to its existing host-created containing
span. Its typed `source_span` chip carries the Source record, revision, span and
exact selected text with UTF-16 start/end offsets in the displayed passage.
Rust checks that text against the retained revision again at Chat admission,
including the readable `/content` projection of a saved web response; inspection
creates no new span or model read receipt. The selected text reaches the model
as typed selection data beside the Source's exact-read hint, and citation
requires the existing host read. Answer selections and Evidence or file excerpts
without a retained Source binding remain quote chips. A newly selected quote or retained Source excerpt reaches the model as quoted prose at its position in the question. A retained excerpt also keeps its typed Source selection and offsets; its source label remains available in the named reference controls. Legacy quote chips still serialize as a blockquote before the question (or "Explain this." alone). Panel "Add to question" and "Ask about this" actions only
attach context and focus the composer; they never send. At narrow widths this
closes the panel overlay so the composer can take focus.
The composer names the model that will answer and its reasoning level, for example
“GPT-6.2 Sol · Medium”. Chat defaults and each Loupist’s settings use the same
account-grouped picker. The checked model carries a Default tag while Rust selects
it. Choosing that tagged row keeps the model preference unset. Choosing another
model sets an explicit choice. Each saved chat restores its own model, reasoning level,
and run options when opened, including after an app restart. If reading the choice
fails, Send stays disabled with the error. Chat retries when the window regains
focus or the chat is reopened. Choosing a model explicitly also resolves the
selection. A pick saves immediately
in a saved chat. A queued message keeps the settings it was queued with. A new chat
starts from the last model submitted from any chat in this app profile, including a
default resolved on send. Its selection saves when its first message creates it.
A choice lasts until one of these
ends it: choosing the Default row in that chat, saving a Chat model, reasoning level or
run option in Settings (for the app, every chat's choice; for a Loupist, its chats'; a
new chat then starts on the default), or its model leaving Chat (its account
disconnected, or the model hidden or no longer offered). The next reply then uses the
Loupist's default as Rust resolves it for a new session, and the control names that
model at once. Once the composer drops that choice, reconnecting the account leaves
the chat on its default.
The picker shows the answering account’s plan and exposes reasoning and performance
under Advanced. A model switch keeps the current reasoning level when supported.
A stored Chat default that cannot run (its account signed out or disconnected, or the
model hidden) gives way to the recommended model, which the control names; Chat stays
sendable, and the default applies again once it can run
([Shared providers](../runtime/providers.md#shared-providers-and-world-processing)).
With no connected account the control reads “No AI account” and opens account setup.
When a Loupist’s default changes, its notice names the previous and current models.
Changing the model, dismissing the notice or sending acknowledges it on the host. A
persistent information/not-advice disclosure appears below the composer.

File selection uses typed `pickFile { title, filters }` intent mapped to Rust
`pick_file` and `rfd`; cancellation returns no path. The current language
preference/catalog foundation and its coverage are described in
[Localization foundation](../dev/ui-qa.md#localization-foundation).

## Code

- Chat progress: `src-tauri/src/chat/progress.rs`, `src/chat/progressLabels.ts`, `src/chat/chartBlocks.ts`, `src/chat/turnView.ts`, `src/components/chat/TurnLedger.tsx`, `src/components/chat/TurnFooter.tsx`, `src-tauri/src/commands/records.rs`

## Activity sheet

Everything about how a reply was made lives in one sheet
(`ActivitySheet.tsx`, `chat/chatRail.ts`); nothing about process floats over the
conversation. It never opens by itself. It opens from the page header's Activity button
(which toggles it) and from any control that dispatches
`loupist:open-activity` on `window` with `{ messageId?, tab? }`: `messageId` selects the
reply (the latest by default), `tab` is `steps`, `sources`, `learned` or `connections`.
It closes with its close button, Esc (from inside the sheet, from the page, or anywhere
while it overlays, including from the composer, before Esc stops a reply; while it docks the
composer keeps Esc for Stop) and, while it overlays, a click
outside. It is 372px wide and docks beside the conversation when the chat keeps at
least 420px; below that it slides over the chat from the right with a shadow and dims the
rest of the conversation, and both stop above the composer so send, stop and the model
menu stay in reach. Its header is one row: the tabs, "Working" only while a reply runs,
and close. It repeats neither the Loupist's face and name, which the page already shows,
nor the answer's time, which What it did gives once ("This answer · 9:00 AM · 11s"); once
a reply succeeds, the header announces "Answered at 9:00 AM". For a failed reply, What it did shows "Failed at" with the reply time and adds "Your AI account was busy." for a rate limit. The header announces the same outcome for the latest reply. A source review still running shows only on the Learned tab. Earlier answers list replies only, never a
message the host posted on its own (a result, an alert, a "now also tracks" line). Each is
named by the question it answered, on one line that ends in an ellipsis before the time in
its own column; the whole question is the row's accessible name and native title. The row
is a button whose items WebKit does not stretch by default, so it sets `align-items:
stretch` (without it a long question ran under the time and off the sheet in WKWebView).
The layout suite pins it with `activity-sheet-earlier`.

Three tabs:
- **Activity**: what the reply did, read and checked. Every step in order, steps of
  the same kind collapsed into one row with a count and the things they read
  ("NVDA, AMD") that expands to each run; a failed, stopped or skipped step keeps
  its own row with a short reason (a security rule, a connection refusal, an
  unreachable provider, a table, chart or document revision that failed while the
  reply shows that artifact ("The change was not applied. The earlier version is
  shown.", matched by the step's title, unless a later publish of that title in the
  reply succeeded), or a generic "It ended with an error. It
  was not used in the answer."). An empty result keeps that row and its reason,
  with the neutral dot rather than the warning; the sheet's header and What it did
  do not count it as a failed run. An SEC web or market-data read refused for a
  missing contact uses the `invalid_input` category and points to contact setup
  in Data connections. Stopped, skipped and safety refusals keep their
  reasons first. Labels come from the locale catalogs: the catalog's step label, a read's
  noun ("Read current quote"; every data route of the bundled OpenBB runtime has one in
  `asOf.reads` or `activitySheet.reads`), the connection's name in the interface language ("Used Web Research", "웹 리서치 사용"), or a
  generic label for the step's kind; a tool's raw name and the host's English
  never show. When a tool result has no own title, its link also uses the locale
  catalogs. A duration shows only when the host stamped both ends of a step
  (a group sums its steps only when all have one). What it read lists the cited sources.
  File steps use the original attachment name, a workspace-relative path such as
  `outputs/report.csv`, or a scratch-relative label. A same-named attachment from
  another message adds its attachment time in UTC. Workspace links are available
  for workspace files.
  A citation to a saved Source counts here and in the answer footer. Multiple citations
  to the same Source share one row with its title, citation count and user-provided
  provenance when present. The row opens the first cited passage in its saved revision.
  The sheet also lists the figures Rust checked with their status and source, and
  earlier answers in the chat.
- **Learned**: the quiet "Reviewing sources" line, what the selected answer saved
  (with the source review status and the Lens filing card), and the names this chat
  added to World, which select in World.
- **In use**: requests waiting for the user (Show scrolls to the card in the answer),
  the principles, methods, pins and connections the chat used, the chat's files and
  images, and "Open chat materials", which opens the materials panel.
