# Answer anatomy

An assistant turn is derived in one place, `src/chat/turnView.ts`, from the message
alone. `deriveTurnView` returns an exhaustive state (`running`, `awaiting_approval`,
`stopping`, `complete`, `stopped`, `failed`), the segments in the order they happened,
one footer slot (the live line, or the settled row) and one string for the live region. The
card renders what it is given and keeps no status conditionals of its own. It keeps no
open policy either: `TurnProvider` wraps it, and a step's page, a source row or a
citation opens through the one `ChatDestination` path
([opening from chat](evidence-browser.md#opening-from-chat)).

Research answers use the full conversation column for paragraphs, headings, lists, charts,
cards and the footer. Tables fill that column and scroll horizontally inside their frame
when they need more room. In Lens pages and scheduled answers the same Markdown fills
the reader. The first numbered citation stays with the word it follows, including its
punctuation. A further citation can wrap to the next line.

For a bundled connection read, the answer’s Sources summary and source chips use the
catalog display name for the recorded provider. A World Bank Indicators read appears
as an external source alongside other connection reads.

System activity uses plain, localized status copy. Compaction appears as reviewing
the earlier conversation, with distinct running, completed and incomplete labels;
it does not expose summarization rules or session bookkeeping.

A turn has three parts with one job each: the **ledger** (what was read), the
**document** (notes, cards, questions and the answer, appended in the order they
happen) and the **live line** (the one present-tense status, always the turn's last
line). Nothing is inserted above what is on screen; the ledger and the live line change
in place. This is a research session's anatomy. In Bot the same turn view draws no ledger
and no notes: the live line is one quiet line beside the Loupist's face without glyph,
"+N" or chips, and the settled row is the figures-checked-and-sources line, copy and
feedback ([Home Thread](home-thread.md)). The reveal, commit gate and anchored scroll are
the same on both surfaces.

Segments are prose, one ledger, cards, questions and approvals. A closed commentary
item is a note, keyed by its id: text a tool call followed, drawn at the answer's size;
a short note (one paragraph under 280 characters) steps back to the secondary colour
over 240 ms in place, and gives its place to the next text when it is the turn's last
block and a ledger lists it; a longer note is content and stays primary. The commentary the host
is writing now (`ChatMessage.draft`: the transport records it when the item is first
published, with the answer's length at that moment, and marks it settled when the
authoritative delta names it) is the answer while the phase is writing. The settled
answer keeps the key of the draft it first settled from (`answer_key`, said before the
text arrives), and `sessionRegistry.reconcile` carries that key over when the store
refreshes the idle chat, so the Markdown node survives the hand-off and the refresh and
nothing remounts or moves. Once a settled draft's text has grown the answer, the live
copy is not drawn again; a draft that closed unsettled (a tool call followed, or the
final text came another way) and any older commentary stay notes, wherever a
removal leaves them.

Every step of a turn is one **ledger** (`TurnLedger.tsx`), placed where the first step
happened; later notes do not open a second one. `ask_user` is not counted and has no
row: the question stands for it; an earlier reply's stored plan step is not counted
either and its plan stays in the expanded ledger. The ledger is one
button: while the reply runs it says the step count ("10 steps") and gathers what the answer can cite as chips in arrival order (each site a
step read, data series, saved notes, calculations; loading a research tool gets none; five, or three in a turn narrower
than 560 px, then "+N"); when the turn settles the same line becomes the receipt
("Worked 30s · 10 steps · 3 sources", "Stopped after 12s · …") and the chips fade out. Its
sources are the answer's numbered sources, the list the footer's Sources (n) and the
activity sheet hold, whether or not a step's results name them, so the two never disagree. A
failed attempt is said in words (" · 1 failed attempt"), never in red, and the ledger
never opens by itself. Its timeline holds the sources read, consulted notes, every step
and note in order (label, target, Failed or Stopped, and a duration of a second or
more), and results. Memory reads with no step of their own place the ledger before the
live answer, where it stays. A reply stored before the agent's `plan_update` tool was
removed keeps its plan: the timeline still shows it with its nested steps, and its
`plan_update` calls have no row. No new reply makes a plan. A chart or table
card stands where its publish step started (`chartBlocks`/`gridBlocks` give each block
its `stepId`), so a card made before the answer sits above it, live and after reload. A
card fills as its publish call is delivered: Rust sends the staged content
(`artifact_staged`, a `ChatArtifactView`, never stored) and the card draws it while the
answer is written below, so nothing moves at commit, where only the stored reference
(`artifact`) follows. A revision the reply makes of an artifact it already showed keeps
its id and revision number and arrives as a second `artifact_staged` for that pair: the
stream keeps one part per `id:revision`, so the same card draws the corrected content in
place, and after the commit it reads the stored revision as any card does. A failed or
refused revision leaves the earlier draft on screen, and its step says so in the activity
sheet. A stopped or failed reply saved nothing, so its staged cards go. A failed publish
the model then calls again (chart or table, under any title) leaves no card of its own:
the retry's card stands for it and the ledger keeps the failed step. A
card the stopped reply takes away, still preparing or already filled from its staged
content, fades out in place as it was drawn (the same element, never mounted again;
120 ms; hidden at once under reduced motion) and is then removed. The turn holds the
height it had when the card began to leave (`--turn-hold`), so the removal never
shrinks the scroll range under the reader;
an answer that says "the chart below" can be wrong until the prompt says otherwise. A
question or approval is a block where it was asked, and settles into its one line in
the same place; answering a question holds the turn at its height (`--turn-hold`) while
the reply runs, so the shorter line never shrinks the scroll range and the page stays put.
The thread keeps its reading block at the same position when the window or side panel
rewraps the answers, context above the thread changes height, or a reply or its artifacts
finish rendering. The scroll position follows the block's layout, even if the browser
has already reset the scroll position or clamped it to the new range. A thread following
its end keeps the end in view through saving, completion and late content. A React commit
can restore the anchor before paint. Resize and browser scroll notifications share a
deferred layout pass. The status pill reads the result in a separate frame, and animated
following waits for a layout change when the browser clamps the scroll position. Behind the
full-page World or Lens the thread has no box (`display: none`): nothing is measured,
reserved or moved then, a lazy page's Suspense fallback showing it again does not reopen
it, and it shows where the reader left it; a question sent meanwhile goes to the top once
it shows. The card, the status pill and the ledger read the view alone; the pill
derives the cheaper `deriveTurnStatus`.

The footer slot keeps one control's height from the first frame. While the reply runs it
holds the **live line** (`TurnFooter.tsx`): a glyph, the label with a sheen (a 1.4 s
sweep, then a 1 s rest), the target, "+N" other running steps and the clock. The label
is, first match: Stopping; Waiting for you (an amber dot, the only amber in a running
turn); the newest running step in the catalog's localized words, whatever the host
wrote, or for a bundled market-data read, "Reading {what it reads}" in the interface
language ("Reading income statements" / "손익계산서 읽는 중") (its target reads "Still working" after 15 s without host activity); Writing;
between steps, the reply's last action, whichever came last: the latest note's last
sentence (identifiers removed, split by `Intl.Segmenter` in the interface language, one
line cut with an ellipsis, the whole sentence in the text; only what a reader of the
note sees: a link shows its label and a reference definition is dropped, an HTML anchor
shows its text, other tags go, and an autolink or bare URL shows only its host, so a
link's target is never shown or announced) with the phase glyph, or the
last finished batch with its step glyph. A batch is the trailing run of steps that
finished well and called the same tool since the last note or the last step of another
tool, in the step's own words, counted when there are several ("Read a web source ×10" /
"웹 자료 읽음 ×10"); a failed or stopped step names nothing done and is passed over
without ending the run. That line promises an accurate last action and
the clock: it never turns into Still working, and a long model round leaves it as it
is. A note is not said twice: in a reply that shows its notes on the page (a research
session), the reply's own footer reads Thinking, then Still working, under the note
instead of repeating its sentence; places that do not show the note keep the sentence
(the status pill, and Bot's working line, since Bot hides notes while it works). A
finished batch reads the same everywhere. With no note and no finished step: Still
working (15 s without host activity); Starting (no signal from the host yet); Thinking.
Bot's working line takes the same label. A label stays at least 600 ms and a finished step's label holds 800 ms before
Thinking, a note or a batch, so quick steps never strobe and the gap between steps
never flashes Thinking. The slot is the turn's only live region, spoken at
most every 5 s, then the receipt once. When the turn settles the live line fades out and
Sources (n), the receipt line, Copy and feedback fade in one after another in the same
place. A stop is one quiet line ("Stopped · kept 6 steps" and Try again, no border); a
failure is the bordered error block with "The steps and the text so far were kept." when
they were. The outline and details triggers fade into the meta row, which already has
their height; next steps and the as-of note render below the footer. The commit itself
waits until the answer's last received word is shown, at most 400 ms after the host's
`completed`, and changes no wrapping and no position of anything already read. On
older answers only Copy and feedback wait for hover; Sources and a failure stay visible.
While a turn is active, feedback on earlier answers and Edit explain that they
will be available when the current reply finishes. The answer being written has
its own completion hint. Copy, sources, receipts, figures and Activity remain
available on finished answers throughout the run.
Copy takes every piece of prose the page shows, in page order: each note that stayed on
the page, then the answer, each as written, so indentation that makes code or a nested
list survives. A short note that gave its place is in the timeline only and is not copied.

The renderer draws the reply's shell (name, time and "Starting") as soon as the
question is sent, 120 ms after the question rises in, without waiting for the host.

## Reading while it streams

The thread is grouped into turns (`<section class="turn">`, one per question). The
latest turn keeps a viewport of room below its question while it is being read, so on
send the question glides to 16 px under the top of the viewport (a question taller than
40% of it aligns its bottom there) and the reply writes beneath it. The page then moves
by itself only when a block is appended and the reader has not scrolled during the
turn: prose gets the lower 60% of the viewport, a card or question is shown whole with
room for the live line. An anchored reader keeps their place during growth, commit and
when opening the ledger or a disclosure. "Jump to latest", End, or scrolling to the very end
while the reply runs follows the end of the turn (18% of the remaining distance per
frame, at least one whole pixel, since WKWebView drops smaller scroll steps) until any
upward input. A chat opened with a reply in flight puts its latest
question at the top and does not make room; a settled chat opens at its end and stays
there while late content arrives, until the reader scrolls or a control in the thread
takes them elsewhere (an outline heading, a receipt's or table's number to check) or
pins something open in it (a number's popover by click, a citation card held open, a
quote receipt); the page then stays where the reader is. The pill
(`ChatStatusPill`) overlays the bottom of the thread, 12 px above the composer, only
while the end of the turn is more than 12 px below the fold and the reader is not
following: the live label and clock (a long label ends in an ellipsis; the clock always
stays whole), "Waiting for you", or "Couldn't finish", then Jump to latest. A 32 px fade on the composer's edge says more is below. Neither takes the
thread's height. Stop is the composer's trailing tile (Esc), never in the turn or the
pill; after a press it dims and ignores presses until the host confirms.

## Streaming text

Rust streams the answer as a progress commentary item: the whole item once, then
only the text appended since the last patch (`ChatProgressPatch.append_text`), at most
every 32 ms. A commentary id is in `upsert_items` or in `append_text`, never both. When
Pi ends the message with a final answer, the `delta` that carries the authoritative text
names the commentary it replaces (`settles`); the renderer drops its live copy in the
same update, so a stop before the removal patch keeps no draft beside its settled text,
and the removal patch that follows then finds nothing. The durable progress format is
unchanged.

Prose runs through the reveal governor (`usePacedText`), then `stableMarkdown`, then
Streamdown with a word fade. The governor keeps the reader about half a second behind
the provider: each frame its rate eases (250 ms) toward the rate that drains the
backlog in 0.5 s (40 to 900 characters a second, faster once the host has finished) and
it releases whole words (one eojeol in Korean) from a budget, so a short stall is
invisible and a burst is spread out. A segment that mounts with text already received
(a resumed run, another chat) shows it at once; a hidden window, a stop or a failure
shows everything received at once with no fade; a stop request freezes the reveal; a
note a tool call followed shows its text at once so the ledger under it never moves.
Under reduced motion it reveals whole sentences or blocks at most four times a second.
`stableMarkdown` holds a heading until its line ends and a table whole until a line that
is not a row follows it (a 100 px ruled placeholder stands in meanwhile, so rows never
arrive one by one and columns never re-flow), plus an open link or evidence link, an
open `$$` run and a marker that just opened. A paragraph that is only one evidence link
(to a calculation, a results read or any other result) is prose like any other: it
streams as a citation in its own paragraph, stays in the same prose segment at commit
and after a restart, and opens the evidence the answer cited; no card replaces it, so
the answer is never split around one and the number checks keep the links Rust counted.
A Mermaid fence is a 240 px ruled frame while the reply
streams and until the diagram renderer has loaded; the diagram block keeps at least that
height, and if the renderer fails the code shows. The rendered text therefore only grows;
`streamingMarkdown.test.tsx` replays the live scenario one character at a time to hold
that. Each released word fades in over 220 ms through Streamdown's `animated` option
(opacity only, 20 ms apart, the spans removed when the stream ends;
`src/styles/vendor/streamdown-animate.css`); headings and tables fade in whole. There is
no caret. Streamdown's table, code and diagram controls are hidden while the text
streams. Code and math plugins load once into a module-level store, so a second answer
renders highlighted on its first paint and a plugin arriving mid-answer re-renders
blocks in place.

Each update of a streaming reply redraws only that reply. Earlier answers keep their render
because every callback a card receives keeps its identity across app renders, the run flags
reach only the cards that use them, a filled chart or table card compares its block by value
(`sameBlock`), and Bot's setup steps under the thread are memoized.

`scripts/probe-streaming.mjs` plays a live preview scenario (`chat-live-research`,
`-stopped`, `-failed`, `-question`, `-quick`) on a real clock and records the live
labels, ledger texts, text update gaps and sizes, any block that moves down, every
page scroll, what changes at commit, layout shifts, long tasks and running animations,
with a filmstrip.

# Answer failures

Provider failures are typed in Rust. No provider text reaches the renderer. Account and plan limits are not retried as ordinary rate limits.

Provider failures are classified in Rust (`pi/provider_failure.rs`; at most 8,192
characters inspected, then dropped) into typed kinds: `plan_limit`,
`plan_not_eligible`, `out_of_credit`, `model_gone`,
`needs_reconnect`, `rate_limited`, `offline`, `provider_unavailable` and
`context_too_long`. Exact plan and account codes match before the generic 429 and
403 patterns, so a plan limit never enters the busy retry and an ineligible plan
never reads as a sign-in problem. A stored kind this build does not know reads as
`other`. No provider text reaches the renderer. A busy key or local account counts down
and retries by itself twice, then says "try again later". The wait is the provider's own
hint when it sent one (`retry-after-ms`, then `Retry-After`, then the latest OpenAI
`x-ratelimit-reset-*` or Anthropic `anthropic-ratelimit-*-reset`, then an error-body
hint such as Gemini's `retryDelay` or "try again in 6.5s"), read in
`pi/provider_failure.rs` from the headers the Pi extension forwards for 429 and 503
responses, bounded to 1–120 s and stored as the answer's `retry_after_seconds`;
otherwise 20 s, then 40 s. A plan limit
is never retried automatically; it offers Manage usage, Try again and Use another
account. An ineligible plan offers an API key, needs reconnect offers Reconnect, and a context that is too long has no
retry. The account kinds also set the [provider status](../runtime/providers.md#shared-providers-and-world-processing).
A `model_gone` failure (404 `model_not_found`) before any text, tool call or
accepted steer prunes the model and asks once more using the current default, in the same turn on a
fresh cold Pi (`commands/chat_runtime/model_gone.rs`); the answer stores
`replaced_model` and names the model that answered instead. While offline a banner
shows and sent messages wait in the ordinary queue, sending on reconnect. Otherwise, when a data read in
the latest answer failed with the tool error `offline` (the completed message's
`offline_reads`), the banner says some data couldn't be fetched because a source
or the network couldn't be reached.

## Learned line

Under every finished answer one line says what it learned, in this order:
"Read your principle · Learned n things · n names added to World · n relationships · n ideas · Used n pages". Sources are
not repeated here: the ledger's receipt says "n sources" and the footer's Sources (n)
toggle opens the numbered list. Only the parts that are not zero appear. Learned counts the sections the answer added,
replaced or retired, read by message id from `memory_learned`; ideas counts filed pages
that are ideas. The principles the host delivered lead the line; Used n pages counts every
other Lens page it delivered (methods, topics, ideas and a perspective), so no
page is counted twice, and both open the same quoted list. A private chat, where saving is
off, shows no Learned part and does not ask Rust for one, and a running or failed answer
shows no line.

When Memory saving is skipped, the receipt gives the reason and the retained
answer stays as written. If the host recorded staged items it discarded, the
next line says that the answer's claimed saves were not kept. The receipt lists
their kinds and titles under Not kept. Bot's scheduled and hand-off posts and the Schedule
run reader show the same notice.

Choosing Learned or ideas opens the card under the answer: each filed page, with every
added, replaced or retired section. Fix opens the Bot composer with the page pinned
and "Correct this:" ahead of the cursor (the Lens pin path; it never sends). Forget
retires that section through `memory_section_forget` and the row offers Undo. Nothing waits
for approval. Used pages open a quoted list with Not this time and Edit.
