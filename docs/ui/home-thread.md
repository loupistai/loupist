# Home Thread

The persistent home Thread appears as Home (홈). Each Loupist has one Home. Other chats have separate titles and drafts. The app restores the selected conversation and its unsent draft on restart, including attached files, the exact inline references, the selected work, Skills and the as-of date. Scheduled results, crossed lines, watcher alerts and the morning message are host posts, not model calls.

Each Loupist has one persisted home Thread (`home-{loupist}`), created on workspace load
with the Loupist's Chat defaults; rename and delete refuse it. Opening a Loupist lands
there unless a saved chat or an unsent new chat was open. When a scheduled run
finishes (succeeded or failed) after the Thread exists, Rust posts one host message
into the Thread with the run's first paragraph and a `delivery` record (run, task,
cadence, status, run chat), built from the run and task only, never from holdings.
Host-card deduplication includes hidden edit versions: editing a tail never reposts
a previously delivered card. A hidden research card stays in its version until that
version is selected again. Watcher, earnings and link delivery marks commit in the
same SQLite transaction as their Thread messages.
Posting takes the chat-mutation lock; while a reply runs in the Thread, delivery
waits for the next completion or workspace load. The result reads "From: {task} ·
{cadence} · Open" and Open goes to the run's Chat; showing results marks them read.
A crossed asset-page line posts one host message per move through the same path
once the committing reply finishes (a busy Thread gets it on the next completion or
workspace load). Its `alert` record (`HomeAlert`) holds the asset, move, name,
symbol and market, the user's claim, the observed value with its unit, when the line
was set, the fact date and the first source, never holdings. Undone crossings are never announced. It
reads "{name} crossed the line you set on {date}: … Thesis marked Broken." with
Review thesis and Undo (by move id, while it is the latest move); there is no OS
notification.
[Watcher](../knowledge/watchers.md#local-watchers) alerts the morning message did not take post through the same
path as one host message per check (`watch` record: up to five dated alerts with their
sources, then "+n more", built from stored alerts only; "+n more" and the source line open Following); an alert on screen for a second
while the window is visible counts as seen. The [results task](../runtime/scheduled-runs.md#results-before-and-after)
posts one message (`earnings` record) when reported figures first arrive ("{name} reported
{period} results." with the two most material measures and See before and after) or when
a before-capture failed (Try again runs `earnings_prepare`); a successful before-capture
posts nothing, and its runs, which target the Thread itself, get no `delivery` message.
Neither carries holdings or sends an OS notification.
A research hand-off (`src-tauri/src/chat/handoff.rs`) posts one message with a `handoff`
record (`handoffId`, `chatId`, `title`, `status`, optional `learned`) when its session
starts and rewrites that same message in place as the run ends (a completion that wins a delivery race keeps its stored outcome), so the card reads
"Researching: {title} · Running", then "Researched: {title} · Done · Learned n · Open"
with the session answer's first body paragraph as plain text (headings and Markdown
markers dropped, the same 280-character summary a scheduled result uses), or
"Couldn't finish" / "Stopped". "Learned n" counts changed sections, not pages, in the
answer receipt's own words ("새로 배운 것 n개" in Korean). The Bot's
`research_open` tool and the bridge's `research_handoff_start` both commit through the
same function; the Bot is never selected and its draft never changes. A finished
`delivery` record may carry the same optional `learned` digest.
A pending-news queue read failure aborts delivery without posting; it is reported for retry instead of treated as an empty queue. An individual earnings snapshot failure is logged and left pending while other host news and hand-offs can still post. Every delivery pass that writes the Thread, whichever caller runs it, emits
`inbox-changed`, and so does the end of every unattended turn (scheduled or handed off),
since nobody streamed its reply. The renderer answers with one workspace refresh that
reconciles every idle chat it holds, so the open Bot and a session opened later show the
stored messages without a reload; a refresh that cannot read the store is retried on the
next two-second poll.
A link [discovery](../knowledge/worlds.md#discovery-after-publication) found posts one
message with a `link` record (`connectionId`, `seedName`, `farName`, `path` of
`{name, relation, reversed}`, `nodeIds` seed first) through the same delivery pass, at
most three per Loupist per UTC day, counted by when each was shown, so opening or
dismissing an earlier link leaves the day's three; further links wait as `new` for a later day. It
reads "New link: A ↔ C through B." over the path with its relations; clicking the path
opens the Map with those names highlighted. Look into it submits the same hand-off as
`research_handoff_start` (title "Link: A and C", a question built from the path, the
path's nodes as pins, request key `link-{connectionId}`) and marks the link `opened`;
Not useful marks it `dismissed`, and neither comes back. Never holdings.

An empty home Thread opens on the Loupist's morning message; a Thread with history
shows it at the end when there is news since a counted last visit and the user has
not written this visit, and an empty topic Chat shows only its questions. The message opens with a "Coming up"
strip when anything is ahead: up to five events in the next 14 days from the read-only `upcoming_events` command
(no model call, no quantities) — the next unreported results date and the sooner dividend date per watched or held
asset, and open dated Journal checks — soonest first, each opening its asset page or Journal entry; a check shown there
is not repeated below, and the strip is hidden in as-of chats and topic Chats. Then comes a lead sentence, then what changed
on the Map since the user last looked, What you follow (up to three followed assets
with a material asset-page change since then, each with its read and most material
row, then "+n more", and up to three unseen watcher alerts the first check after launch
or of the day handed to it; not on a first visit), the latest briefing and pending dated
Journal checks, on the first visit of a Loupist made from the Crypto, Macro & rates or
Commodities template a "Works best with {connection} · Open Connections" line naming the
recommended connection it lacks (Tiingo, FRED or EIA; enabled and available for the
Loupist counts as having it),
and three locally generated questions that fill the composer without sending. Until all three
are done or the user hides it (per Loupist, `loupist.setupHidden.v1` in `localStorage`), the
first message also shows the "Get set up" checklist: Follow something, Teach a principle and
Get a morning briefing, each ticking off by itself (something followed, a principle saved, the
briefing on), each open step with a chip that fills the composer without sending, and an
"Import holdings (CSV)…" link to the holdings sheet. Without an AI account it leads with
Connect an AI account, its Follow step opens the Following page, and chips that need a model
are disabled. The focus question names a preset's subject ("What matters in semiconductors
this week?"); a Loupist with a free-text focus does not get it. A first
visit counts what there is to look at instead of naming a day; general questions fill
the list when the Loupist has little to point at. A Thread with no user-written message yet
keeps this first-visit message below any host post, such as the preset's vocabulary card. It is
computed in the renderer (no model call) and is not persisted. "Last seen" is a
per-device value per Loupist in `localStorage` (`loupist.loupistVisit.v1`), written
when the user leaves a Loupist after a visit of at least five minutes.

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
alerts, watcher and results messages, hand-off cards, links), scheduled runs in
the Thread and research sessions never change it and never cost a model call for it.
After third-party text in a reply (web pages, search results, filings, saved
Sources, shared Loupist material), new or changed entries pause for the rest of the
reply, as instruction writes do; `resolve` and `forget` still work. Credential-like
text is refused with the Memory `sensitive_memory` refusal. The tool tells the model
never to keep account details, holdings quantities or values, or text from pages,
files or tools; a research finding goes to `lens_file` and an investment rule to
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

**Privacy, dates and deletion.** "Don't remember this chat" and Remember new chats
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
