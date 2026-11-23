# Home Thread

The persistent home Thread appears as Bot (봇). Each Loupist has one Bot. Research sessions have separate titles and drafts. The app restores the selected conversation and its unsent draft on restart, including attached files, the exact inline references, the selected work, Skills and the as-of date. Scheduled results, crossed lines and the morning message are host posts, not model calls.

Each Loupist has one persisted home Thread (`home-{loupist}`), created on workspace load
with the Loupist's Chat defaults; rename and delete refuse it. Opening a Loupist lands
there unless a saved chat or an unsent new chat was open. When a scheduled run
finishes (succeeded, failed, or cut short by a crash that recovery found; not a
quiet run, a Stop or a quit) after the Thread exists, Rust posts one host message
into the Thread with the run's prose preview and a `delivery` record (run, task,
cadence, status, run chat), built from the run and task.
Host-card deduplication includes hidden edit versions: editing a tail never reposts
a previously delivered card. A hidden research card stays in its version until that
version is selected again. Link delivery marks commit in the
same SQLite transaction as their Thread messages.
Posting takes the chat-mutation lock; while a reply runs in the Thread, delivery
waits for the next completion or workspace load. A reply whose answer is stored and
whose save waits on its own no longer holds the Thread, so posts follow it at once
([saves that wait on their own](../runtime/chat-turns.md#saves-that-wait-on-their-own)). The post names Schedule, the task and the run’s finish date and time. Open shows the exact run in Schedule. A failed run says "This run couldn't finish." A post whose task or run has been removed keeps its preview and explains that the record was removed when opened. Seeing a delivery for one second marks it read, as does opening its run reader.
Bot’s task cards share the current Schedule list by task ID. A changed tool receipt refreshes that list for every card, including when a deletion replaces a read at the same task revision. Active tasks show their next run. A deleted task card shows its title and a muted Deleted label. Cards omit status while the list loads. Task cards, delivery posts and Needs attention use Schedule’s localized briefing title and preserve custom titles. For an unopened Loupist, the briefing task’s dedicated session ID identifies its attention row.

A [price setup](../knowledge/asset-pages.md#host-reads-and-refresh) that ran and left an asset
without a ready price read uses the same delivery pass. Its pending reply is stored in the Loupist's
database and removed in the same transaction that appends the Bot message.
A crossed asset-page line posts one host message per move through the same path
once the committing reply finishes (a busy Thread gets it on the next completion or
workspace load). Its `alert` record (`HomeAlert`) holds the asset, move, name,
symbol and market, the user's claim, the observed value with its unit, when the line
was set, the fact date and the first source. Undone crossings are never announced. It
reads "{name} crossed the line you set on {date}: … Thesis marked Broken." with
Review thesis and Undo (by move id, while it is the latest move); there is no OS
notification.
A research hand-off (`src-tauri/src/chat/handoff.rs`) posts nothing in Bot while its
session works. The Bot reply that started it says the research was handed over and that
its result arrives here. The host stores the session ID on that reply (`research_session_id`).
Directly below the acknowledgement, one button opens the session and shows its current title.
While the session carries a hand-off run, the second line reads “Researching it. The result will arrive here.”
After the run ends, the title stays linked. Removing the session removes the button.
The session’s Chat holds its guidance and Stop controls. When the session ends, the host posts one message with a `handoff`
record (`handoffId`, `chatId`, `title`, `status`, optional `connectionId`, `stopReason` and
`learned`), timed when it ended (`ThreadHandoff.tsx`). A finished session's post leads with
its answer's preview above "Session ended: {title} · Learned n things · Open"; one that failed
reads "Research: {title} · Couldn't finish · Open" and one stopped in its session
"Research: {title} · Stopped · Open". Nothing in a post runs, counts or stops. A session the
app quit in the middle of is stopped with `stopReason: interrupted`: "The app quit before
this session finished." above "Interrupted", with Open; one whose answer was already saved
when the app quit is posted as finished with that answer. Both scheduled results and
completed hand-offs preview the first prose paragraph as plain text, up to 280 characters,
skipping headings, tables and lists. When the host shortens a preview or generates a
40-character session title from the first prompt line, it ends at a whole word and
adds an ellipsis within the limit. Text without spaces ends at a grapheme boundary. A short bold-only label can serve as the preview if no
paragraph follows it. When the preview is empty the host stores "Session ended: {title}." in the history. Bot hides that fallback and its older "Researched: {title}." wording.
The `done` status records that the child turn ended with an answer. A child may answer
only part of the request. "Learned n things" counts changed sections, not pages, in the
answer receipt's own words ("새로 배운 것 n개" in Korean). A post the host stopped
without running carries `stopReason` and says why above its status, ending "Ask again
to run it." (`before_update`: "Opened before an update."; also `origin_unavailable` and
`invalid_authority`); it keeps Open and is never retried. No post offers Retry: the user
asks Bot again, and a link offers Look into it again. The Bot's
`research_open` tool and the bridge's `research_handoff_start` both commit through the
same function; the Bot is never selected and its draft never changes. A finished
`delivery` record may carry the same optional `learned` digest.
The session opens with Bot's brief, falling back to Bot's question. The host projects
this text in `content` with role `bot_handoff`. The session shows the brief as Markdown beside the Loupist avatar under **From Bot**.
Its closed **Hand-off details** disclosure shows the title and question, then quotes
`handoff_note.userMessageInBot` under **You said in Bot** when present. Copy takes the
brief. Markdown, print and PDF attribute the brief to Bot and include the quotation.
A fresh attempt at the opening goes through Bot. When the user chose an action instead (`handoff_action`, a link's Look
into it), the message reads "You chose “Look into it” on a link in Bot." in place of the
host's caption and has no Edit. "Link details" shows the stored start, directed
claims with localized relation names, and the far end. The session starts
with a localized title naming both ends.
While this app holds a hand-off's run, its session's thread carries `handoff_run`
(`handoff_id`, `run_id`, optional `connection_id`) in the workspace listing and in the joined
snapshot. Such a session is joined while it is shown: the renderer subscribes
with `chat_watch` under a fresh `subscription_id`, and the host replays the run's ordinary
Chat events before the live ones into that session's Chat (`watchHosted` in
`src/chat/sessionRegistry.ts`, `src/chat/useHostedRun.ts`). The work so far and then the
work as it happens read exactly as a reply sent there, and so do its controls: Guide
(⌘Enter) sends `chat_steer` and Enter queues a message for after the run, with no model
needed for guidance, and Stop aborts the run by its id. A host-started run never asks the
user a question; guidance is the way in. Showing another chat detaches with `chat_unwatch`
and drops the half-drawn reply, and the run goes on; coming back joins again. Bot has no
Stop and no guidance for a hand-off: its composer always talks to Bot. A `stream_reset`
from the host detaches, reads the snapshot again and rejoins. A watch that finds nothing
running reads the stored thread, since the run finished meanwhile. A session the host
stops reporting as running while it is open keeps the join until the stream's own last
event. Opening the session, from its row or a post's Open, changes only the shown chat:
Bot's unsent draft and its selected work stay with Bot.
A pending-news queue read failure aborts delivery without posting; it is reported for retry instead of treated as an empty queue. Every delivery pass that writes the Thread, whichever caller runs it, emits
`inbox-changed`, and so does the end of every unattended turn (scheduled or handed off),
since nobody streamed its reply, and a hand-off's start once its session holds the request,
since Bot posts nothing for it. The renderer answers with one workspace refresh that
reconciles every idle chat it holds, so the open Bot and a session opened later show the
stored messages without a reload; a refresh that cannot read the store is retried on the
next two-second poll.
Automatic link posts are disabled. Extracted product or segment relations remain
in World; the stored paths do not prove that both ends are followed or that
a followed asset links to a dated event. See [discovery](../knowledge/worlds.md#discovery-after-publication)
for the conditions needed to enable Bot delivery again. Previously posted link
records keep their Look into it and Not useful actions on one quiet line, with the path
on hover of "Show in World" (the button is named by those words, so voice control finds
it); a vocabulary post is one line with Undo. Look into it
sends only the Loupist, the link's `connectionId` and the Bot composer's choices at the
click (explicit or default model, thinking level, run options and Memory flags). The host
writes the request and marks the link opened in the same step. One press at a
time reaches the host. While the session works the link reads "Researching it. The
result will arrive here." from the session's `handoff_run`, with no actions; once Bot posts
how it ended, the link reads that status and offers **Look into it again** when the
session finishes, fails or stops. A link the host no longer holds says so and drops its
actions.

Host posts and answer results share one card language (`src/styles/bot-cards.css`): a
raised card with a hairline, a kind tag ("Line crossed", the task title, which wraps rather
than leaving the card) and, for a crossed line, the accent hairline. A hand-off is such a card
too: its preview or stop reason above, its status line as the foot. Every host
post sits like an answer, beside the Loupist's face (decorative there: a post has no activity
to open) with its name and time (`BotPostBody` in `BotPost.tsx`), and the news's lead is a
bubble. Each post's region is named for what it is ("Line crossed: {name}", "New in World: {list}", "Research: {title}", "Result from {task}"). A status line of "a · b · c" wraps
only between items and never ends or starts on a separator: every item carries its silent "·"
in front and the row clips the one that begins a line (`MetaLine`). In running text, such as a
tile's "As of {date} · {provider}" or the Bot trace line, `joinMeta` (a no-break space) or
`white-space: nowrap` keeps the "·" with the word after it. When an action takes its own control away (Undo, Look into it, Not useful,
Use in this Loupist, Set up, Hide this suggestion) the keyboard moves to what happened or to the
action the new state offers, never to the page; a control waiting on the host stays focusable
with `aria-disabled`, so a refusal leaves the keyboard where it was. A read returns evidence and no card: an answer that read a quote
or any other data tool shows what the agent wrote, each linked figure a number chip checked
against that read and the trace line under it, and nothing drawn from the read beside it.
A visual appears in an answer only when the
agent writes one (headings, a list or a table in the bubble) or publishes one
(`chart_publish`, `grid_publish`, `artifact_publish` or a file).

A followed asset's price on screen is the one the host stored from its saved read's last
replay ([asset pages](../knowledge/asset-pages.md)): the value, its currency (none shown when
the result states none), the date or time the provider stated ("As of Nov 13", or with the
local clock when the provider gave a time) and the provider by name. Bot's tiles and the asset
page read the same stored price; neither words it as a close or an open-session quote.

Bot is drawn as a conversation with a person (`data-surface="bot"` on the chat workspace and
its scroller; research sessions keep the document layout). The Loupist's face and name sit beside
its messages and its words are bubbles on the left; the user's are accent bubbles on the right.
While a reply runs, the face turns and one quiet line beside it says what the Loupist is doing and
for how long ("Reading the quote · NVDA · 4s"); the line and the face open the activity
sheet at that answer (`loupist:open-activity` on `window`). The line's accessible name is the
step and "Open activity" without the time, which is for the eye only, so the name holds still
while focused; the run's start and end are announced once. The thread shows no step count, chips,
failed attempts or the reply's own notes; those live in the sheet. Once settled, one line counts the
figures checked against their source and the sources used, and opens the sheet at the sources;
figures that did not match have their own button. A reply that failed as a whole says so in a
bubble of its own. Days between messages are a small pill; "New since you left" is not shown in Bot.

An empty Bot opens on the greeting (`BotGreeting.tsx`, renderer only, no model call): the face, a
greeting by the Mac's local time of day (Good morning from 05:00, Good afternoon from 12:00,
Good evening from 18:00 to midnight; from 00:00 to 05:00, which neither language greets by
name, the neutral "What would you like to research?" of an empty research session) and the date (the page header above names the Loupist), then up to
four cards and up to four of this Loupist's followed assets (`assets_list`) that have a stored price (`watchlist_quotes`) as tiles:
the name, the price and "As of {date} · {provider}", each opening its asset page by id (named
"Open {name}", with the price and its date read as the tile's description). Tiles and cards name
an asset and write its price as its asset page does (`useAssetNames`; a price stated without a
currency keeps every digit it was stated with), so a Korean listing reads "한울정밀" in Korean on both. No
sentence is built from prices, and there is no day change or price line, because the stored price
carries none. `assets-changed` reads the list and its prices again. A card names its question
("Where NVIDIA stands") and fills the composer, without sending, with the user's own request
("Where does NVIDIA stand? Give me the price, results and news in brief."). A new user's cards,
under "Start here", are the remaining setup steps:
Connect an AI account (without one), Add assets to follow (opens Following), Get a morning
briefing (while it is off) and one sample question. With assets
followed the cards, under "Suggested questions", are where the first followed asset stands, this morning's briefing (or Get a
morning briefing while it is off), another asset's next earnings date and the week's biggest movers; a
quiet line offers to teach a principle until one is saved or the line is hidden
(`loupist.bot.principleHidden.v1:{loupistId}` in `localStorage`; hiding it moves the keyboard
to the last card). Steps, the greeting and the date are read again each time Bot
is shown, so a step done elsewhere drops out. The greeting stays below host posts until the user
writes; nothing is pinned under the conversation after that.

A Thread with history ends with Bot's news (`MorningMessage.tsx`, `src/chat/morningMessage.ts`)
only after a real absence, never while the person is using the app: the later of the last counted
visit's end and their last message in Bot must be at least four hours before the current visit
began, or at least an hour before it on an earlier local day (the first open of a new day). A visit
in which the person has sent a message in any Chat of the Loupist (a send, a steer or a retry the
host accepted) is a working visit and shows no news, including news still loading when the message
was sent; news already on screen stays. It also needs something to say and goes away once the
person writes. It concerns only what the person follows: under a lead sentence
("2 things since Thursday."), one card holds the latest briefing and pending dated Journal
checks. The lead counts exactly the lines the card shows. Prices are on the tiles and the asset
pages, not in the news. World's
growth (names and relationships added) is not news here, and Bot's suggested questions are about
followed assets and checks, never built from World entries; World's What's new shows its growth.
An empty research session shows only the
questions, which may come from what changed in World. "Last seen" is a per-device value per
Loupist in `localStorage` (`loupist.loupistVisit.v1`), written when the user leaves a Loupist
after a visit of at least five minutes. The visit in progress (`currentVisit` in
`src/app/loupistVisit.ts`: when it opened and whether it is active) is kept in memory only; each
visit, including one that starts after the window was away, begins inactive.

## Bot quick asks

Most questions in the Bot are quick asks (how an asset stands, why it moved, a
figure, what changed, a short briefing, a follow-up, what a term means), and the
Bot answers them in seconds from one read, without researching, filing or saving
([prompt rule](../agent/prompt.md#quick-asks-in-the-bot)). How long the answer is
and what form it takes are the agent's to judge from the question and the user's
to change: a passing question still gets a passing answer, and a request for one
line, a table or a headed walk-through gets that in the conversation rather than
a hand-off. With saving on, a standing request about form is a `preference` the
Bot can keep in [Bot memory](#bot-memory). The answer says what matters instead
of listing what it read, each figure linked to its source, and the read adds no
card under it. A question about why an asset moved gets the move and any
reported headline; when no report or filing establishes the reason the Bot says
so and offers, or opens, a research session for it.

What stays after a quick ask is what the host keeps without a model call: the
conversation and each answer's evidence, the data reads the answer used, the dated
rows those reads add to a followed asset's page, and [Bot memory](#bot-memory).
What does not: the model's own synthesis. A quick ask writes no Lens page and
retains no Source, so World does not grow from it. Those come from research
sessions, a file the user hands over, an explicit request to keep something,
scheduled briefings and scheduled tasks. A Loupist used only for quick asks therefore
has a full conversation and full asset pages and a thin Lens.

## Bot memory

The Bot is one conversation for the life of its Loupist, but a reply that starts
fresh (any changed setting, model or connection, an app update, a failed reply, an
edit, or a restart without a usable Pi cache) reads only the newest 256 KiB of
transcript ([context bounds](../agent/context.md#context-assembly-and-bounds)). An
older message is then only a count, and Pi's own compaction summary is a derived
cache that such a start discards. Bot memory keeps what the conversation should
carry forward past that cut: at most 30 entries of up to 300 characters, each a
`preference` (how the user wants the Bot to work, limits they set), `decision`,
`open` request or `context` about the user's situation, with the ids of the user's
messages it rests on (`basis`, the current message by default). It is the Bot's
working memory of this conversation, not research knowledge: World, Lens, Journal,
Library and research sessions never read or write it, and nothing in it is filed
anywhere else.

`chat/bot_memory.rs` stores it on the home Thread's own `chat_sessions` row
(`ChatSession.botMemory`); a record outside the home Thread is refused as invalid.
Each change is kept with the Bot reply that made it, and the entries the Bot sees
are the changes whose reply is on the shown path, so an
[edit or a version switch](evidence-browser.md#evidence-and-the-browser) shows memory as it stood on that path.
Corrections replace an entry's text and keep its kind; `resolve` ends an entry from
that reply on (done, obsolete, replaced); `forget` deletes every stored copy of the
entry on every path, including one resolved or kept only on another version, and
drops the messages it rested on from review; `match` words (`*` for everything)
reach entries the Bot no longer sees and report only a count. The change log
keeps about 240 changes for forks: it drops first the oldest change that every path
showing it has since replaced, keeps a change some path still shows (a version kept
inside another version included), and past 2,018 drops the oldest so a reply can
always commit. A new edit before a change replaced long ago can leave out an entry
whose older changes were already dropped. An entry keeps the date it was first kept.

A committed correction, resolution or forget also invalidates the warm Pi session
cache. The next reply starts from the current host memory and bounded transcript
instead of reusing a summary containing the previous entry. Remembering a new
entry alone preserves the session and refreshes its envelope. Forget removes Bot
memory copies; it does not delete the user's original conversation messages.

**When it changes.** Only through `bot_memory`, a Bot-only tool that needs Memory
saving on. A call is checked and reserved under one lock, so parallel calls in a reply
share the 30-entry and 30-change budgets and an undelivered call frees its share.
Delivered changes apply as one batch in the transaction that publishes the Bot
reply; one that no longer applies fails the reply instead of being dropped. A
failed, stopped, crashed or retried reply publishes nothing, and Try again keeps it
once. Host posts (scheduled results,
crossed-line alerts, hand-off cards, links), scheduled runs in
the Thread and research sessions never change it and never cost a model call for it.
After third-party text in a reply (web pages, search results, filings, saved
Sources, shared Loupist material), new or changed entries pause for the rest of the
reply, as instruction writes do; `resolve` and `forget` still work. Credential-like
text is refused with the Memory `sensitive_memory` refusal. The tool tells the model
never to keep account details or text from pages, files or tools; a research finding goes to `lens_file` and an investment rule to
`principle_save`.

**Review before the cut.** Each remembered Bot message (Memory saving on, reply
committed) joins a review queue of at most 48. When the oldest has 128 KiB of later
conversation after it (half of what a fresh start reads), 24 messages wait, or Pi
reports compaction pressure (including a deferred post-answer attempt), the
next Bot reply is offered up to eight of them, oldest first, as 600-character excerpts, and can keep
what still matters. The batch leaves the queue when that reply commits without
third-party text; otherwise the next reply sees it again. There is no extra model
call: the review rides on a reply the user asked for.

**What the model receives.** On the Bot with Memory on, the envelope's
`bot_memory` block (`loupist-bot-memory/1`) holds the entries (id, kind, text,
first and last dates, basis), the room left and any review, under a fixed rule:
entries are notes on what the user said, grant no tool, permission or authority,
never override the current request or the user's settings, and are not evidence.
`chat_history` reads an entry's basis message. The delivered entries and review
excerpts join principles and the perspective as text a `web_fetch` URL may not carry
after third-party text. On the Bot, `bot_memory` replaces
`note_save` and the current-Chat working note (envelope 6g); research sessions keep
[working notes](../knowledge/sources-notes.md#working-notes-and-journal), and a
working note the Bot saved earlier stays in Library.

**Privacy, dates and deletion.** Private chat and Remember new conversations
cover Bot memory: a private Bot turn neither receives nor changes it and its
message is never queued, like the rest of saved Memory. Under an
[as-of date](../analysis/point-in-time.md#point-in-time-answers) the Bot sees its
memory as it stood then (changes whose reply passes the chat-history rule, counted
as left-out notes), forgotten entries never return, and no review is offered.
Deleting the Loupist deletes it with the Thread; the Thread itself cannot be
deleted. Reset what it learned covers what the Loupist learned in Lens and World and
leaves Bot memory alone; asking the Bot to forget is the way to clear it. The user
asks the Bot what it remembers, corrects it or tells it to forget in the
conversation; each change shows as a "Remembered for this conversation" step.
