# Chat composer

The composer and rail attach context and do not send it. Text and object references appear at the textarea cursor; whole-message settings (Skills, as-of date, privacy and model) sit below the text. Focus on the composer, including the automatic focus of a new chat, turns its 1px edge to the focus color and adds a soft 3px halo of the same color. It draws no 2px outline, which would frame the whole composer. Footer controls keep their own outline. When the tools wrap onto another row, Send stays at the trailing edge of that row. `/asof` is the [date control](../analysis/point-in-time.md#composer-date-control). Width and the browser: [Evidence](evidence-browser.md).
Answer table, code and diagram controls use the current UI language for their tooltips and accessible names. A stopped reply shows the localized run state without the runtime's English stop marker in the answer or its copy action.
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
cannot guide the live reply; Send next queues that message with them. Guidance that has
not been accepted yet reads as waiting on the composer. Once Pi accepts it, that note
switches to guiding and then clears, and a Guiding row stays in the timeline. Rust stores
accepted guidance as a user message marked `steer`; it stays a user turn for the model
and reads as a "Guided: …" row between the question and its answer after the run; a
stopped answer says what it kept. Queued messages sit in a compact list above the
composer: a count, one short row each, and Edit, reorder, Remove, and Send now in that
row's menu. The list can fold. It keeps queue order and the queued payload. Up to three
next-step chips after the latest answer come from the conflicts and names it
found and fill the composer without sending. Leaving a view or switching Loupist does not cancel an admitted
Chat. The composer has one private-chat switch, described under
[Memory permissions](../knowledge/memory.md#memory-permissions-and-retrieval).
Focusing a control below the fold scrolls it into the conversation. When the reader scrolls away from the bottom, the live status or Jump to latest control
occupies its own row between the scroll viewport and composer. It shares the
conversation’s scroll state, so jumping re-pins the reply while the control stays clear
of answer text and actions. A short conversation that becomes scrollable when the
window narrows stays pinned to its newest answer. The viewport follows the
reply in the same commit that grew it, through tool updates and table rows, even
when the browser temporarily moves the scroll position up. An
upward scroll, even in single-pixel increments, keeps the reader's position
once they leave the bottom.
After a reply saves a Source, its queued or running World job shows “Reviewing
sources…” in the Learned row, including when it is folded. In a narrow chat the
compact context card shows the same status above the transcript, with approval requests alongside it. As
jobs finish, their additions become available in the row. When all work settles,
the row shows the chat's learned counts or disappears if there are none.

The "In this chat" rail lists Files & images (up to eight thumbnails; files linked
in answers marked "from Loupist"), Using (principles, methods and connections the
answers used, plus pins), Learned (this chat's growth events, with name chips that
highlight on the mini map) and Needs you: one "Needs your OK" row naming the waiting request (or how many) that scrolls to its card in the answer and focuses it; the buttons are only on that card. Its placement follows
the chat workspace’s width. At 1120 px and wider, the sections share a 300px panel in a reserved right column of the chat body, inset 8px from its top and 12px from its right edge. The conversation and composer center in the remaining space; a transient status header uses that same column. At narrower widths, including beside a split panel, the same cards occupy their own right-aligned row above the transcript. A collapsed card's summary wraps so its counts stay visible. The Needs you target stays on one line, so Show stays at the end of that row. That row can scroll within 30% of the chat body height. Opening a section changes its row height instead of floating over the answer or composer. An empty status header reserves no space. The rail is absent when there is no context. Fold state is remembered per Loupist on the device. The composer's "+" menu opens above the composer, aligned to its left edge like the `@` and `/` menus, and holds
Files…, Paste image, Screenshot of the map (disabled with a reason when no map is
open or it is empty) and Add from Evidence (this chat's saved Sources as pins);
dragging files over the chat column shows one drop overlay with the limits. The tray shows each file's size. Each file may be up to 25 MB (images 5 MB); Rust enforces it in `prepare_chat_attachments` and refuses a larger file with `attachment_too_large`, which the composer words in the reader's language.

The composer has `@` for existing context and `/` for commands and Skills.
`@` searches files, knowledge, work and connections, including existing World
references. It does not search the watchlist or offer free-form ticker selection.
`/` lists `/brief`, `/compare`, `/check`, `/asof` and then Skills by name.
`/brief` and `/compare` start editable prose. Dollar amounts and ticker text,
including old `$SYMBOL:MARKET (Name)` messages, remain ordinary text. They do not
open a menu, attach a company chip, or add an asset card to an answer. Skills keep
their `$skill_id` submission syntax.

When a read-only tool of a custom connection needs approval during an
interactive run, Rust records it on `ChatProgress.approvals` and waits up to five
minutes; the card, under the step that waits (after the steps when the step is unknown; under the header while the steps are folded), offers Allow once, Always and Not now through
`chat_approval_decide` (Rust checks the Loupist), Always reuses the stored per-tool
approval that Settings › Connections shows and revokes, and Not now records a
`declined` refusal while the Loupist continues without the tool. The card says the
custom connection receives the request and that the approved tool can only read
data; it does not promise the request stays on the device. A card still waiting
when the run ends reads "No longer needed". Tools that are not read-only, and
unattended runs, keep the plain refusal. After third-party text, a `web_fetch` of a
new site waits on the same kind of card (`kind: web_site`) with only Allow once and
Not now ([Trust](../runtime/untrusted-text.md)). A native command (`kind: command`)
shows the whole command in a scrollable block and offers Allow once, Not now and a
secondary blanket option, "Allow commands for this reply" or, when only earlier
replies read other sites, "Allow commands in this chat"; that settled line offers
"Stop allowing" (`chat_command_grant_revoke`). A read or write outside the workspace
(`file_read`, `file_write`) offers Allow and Not now; an instruction file
(`instructions`) offers Allow once and Not now. The rail's "Needs your OK" row names a
native card by its path or command line ([Native tool permissions](../runtime/permissions.md)).

An [`ask_user`](../agent/turn-tools.md#turn-tools) call waits on a question card the same
way: Rust records it on `ChatProgress.questions` (at most four per reply), under the step
that asked, and the card offers one button per option (the recommended one primary, tagged
Recommended, with its description under each label), an "Or answer in your own words"
field up to 500 characters and the waiting clock. Answers go through
`chat_question_answer {loupist_id, request_id, choice | text}` (exactly one; Rust checks the
Loupist, the option index and the length). The settled line reads "You answered: …" or,
when the wait ran out or the run stopped, "No answer · continued with <recommended
option>". A waiting question counts in the rail's "Needs your OK" row and turns the
footer to "Waiting for you"; one still waiting when the run ends is saved as expired.

A completed `connection_setup` step leaves a setup card under the answer
(`ConnectionSetupCard`, derived by `chat/connectionSetupBlocks.ts` from the step's
target, one card per entry). The card reads the connection's live state through
`marketplace_snapshot` and `loupist_capability_list`, never from the stored step, as
one union: loading, needs setup, off for this Loupist, ready, or unavailable (the entry
left the catalog or its runtime is missing). Needs setup opens the same
`MarketplaceSetupDialog` as Settings › Connections and refreshes when it closes; off
for this Loupist calls `loupist_capability_enable`; ready offers Ask again, which puts
the user's previous question back in the composer through the same path as Change the
rates. No key passes through Chat, and the connection works from the next message.
Preview scenarios `chat-connection-setup-needs-setup` and `chat-connection-setup-ready`
show it. A chat with both Memory flags off shows a
private mark; the chat title in the sub-header renames in place; a Loupist row shows a
draft dot while unsent text exists.

Selected workspace files, connections, quoted text, knowledge and work are inserted inline at the current cursor or selection. The editor uses a native textarea with a synchronized highlight layer; reference styling has the same text metrics, so wrapping, scrolling, selection and IME remain native. The draft retains bounded UTF-16 ranges alongside its existing typed pins, selected artifact and chip metadata. Typing before a reference shifts its range; editing inside it detaches the structured reference and leaves ordinary text. Backspace immediately after a reference, or Delete immediately before one, removes that occurrence atomically. Repeated references keep distinct offsets and serialize at each position; their shared typed context stays until the last occurrence is removed. The named removal action removes every occurrence of that context. Named removal controls are available to screen readers and reveal themselves when reached with Tab. Inserting or removing one reference preserves the surrounding question and other references. External draft restoration waits for the textarea to adopt the saved text before anchoring its references.

Skills are whole-message modes and stay in a compact row below the textarea with the as-of date. Privacy status and model controls also stay below the text; attached file previews retain their own tray. Inline serialization keeps the reference's place in the sentence: connections use `@entry_id`, and workspace files use relative paths. Knowledge and work show their natural labels in the sent prose while their existing typed fields carry identity and ownership. No opaque editor key is put into the prompt. Rust removes retired company chips when `chat_draft_read` restores a saved draft. An inline ticker keeps its visible text and loses its reference metadata. A tray-only ticker becomes ordinary name, symbol and market prose before the existing question, and retained inline offsets shift by the prefix's UTF-16 length. Supported chips, pins, work and the as-of date stay intact. Legacy supported chips without inline ranges retain their previous serialization until the editor migrates them. Queued messages already hold serialized prose and typed context, so they need no chip migration. Selecting at least three words in an answer
offers "Ask about this", and selecting text in a panel reader offers "Add to
question". The floating reader action renders inside its panel so it remains
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
The composer's run-profile control shows "Auto" while nothing is chosen for the
Loupist (Chat defaults apply), the chosen model's name as a chip otherwise, and
"Connect an AI account" when no account is connected; the resolved model is in
its tooltip. With nothing chosen and no Chat default, a send names no model and Rust
resolves [Auto](../runtime/providers.md#shared-providers-and-world-processing) at admission. The menu
starts with Auto (or Reset to Auto); for a ChatGPT sign-in its plan line
("Using your ChatGPT plan · Manage usage") follows the account that answers. Reasoning options are inside the model menu's
Advanced section. The one-time Auto notice ("Now using …" with Change, or "… is no
longer offered. Chat uses Auto now.") sits above the composer until a send or ×. A
persistent information/not-advice disclosure appears below the composer.

File selection uses typed `pickFile { title, filters }` intent mapped to Rust
`pick_file` and `rfd`; cancellation returns no path. The current language
preference/catalog foundation and its coverage are described in
[Localization foundation](../dev/ui-qa.md#localization-foundation).

## Code

- Chat progress / feedback: `src-tauri/src/chat/progress.rs`, `src/chat/progressLabels.ts`, `src/chat/chartBlocks.ts`, `src/chat/turnView.ts`, `src/components/chat/ActivityRow.tsx`, `src/components/chat/TurnFooter.tsx`, `src/components/chat/AnswerFeedback.tsx`, `src-tauri/src/commands/records.rs`
