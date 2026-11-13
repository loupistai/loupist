# Answer anatomy

An assistant turn is derived in one place, `src/chat/turnView.ts`, from the message
alone. `deriveTurnView` returns an exhaustive state (`running`, `awaiting_approval`,
`stopping`, `complete`, `stopped`, `failed`), the segments in the order they happened,
one footer slot (the live line, or the settled row) and one string for the live region. The
card renders what it is given and keeps no status conditionals of its own. It keeps no
open policy either: `TurnProvider` wraps it, and a step's page, a source row or a
citation opens through the one `ChatDestination` path
([opening from chat](evidence-browser.md#opening-from-chat)).

System activity uses plain, localized status copy. Compaction appears as reviewing
the earlier conversation, with distinct running, completed and incomplete labels;
it does not expose summarization rules or session bookkeeping.

A turn has three parts with one job each: the **ledger** (what was read), the
**document** (notes, cards, questions and the answer, appended in the order they
happen) and the **live line** (the one present-tense status, always the turn's last
line). Nothing is inserted above what is on screen; the ledger and the live line change
in place.

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
happened; later notes do not open a second one. `plan_update` and `ask_user` are not
counted and have no row: the plan and the question stand for them. The ledger is one
button: while the reply runs it says the step count ("10 steps", or "Plan 2 of 3 ·
{item}") and gathers what the answer can cite as chips in arrival order (each site a
step read, data series, saved notes, calculations; five, or three in a turn narrower
than 560 px, then "+N"); when the turn settles the same line becomes the receipt
("Worked 30s · 10 steps · 3 sources", "Stopped after 12s · …") and the chips fade out. A
failed attempt is said in words (" · 1 failed attempt"), never in red, and the ledger
never opens by itself. Its timeline holds the sources read, consulted notes, the plan
with its nested steps, every step and note in order (label, target, Failed or Stopped,
and a duration of a second or more), and results. A plan or Memory reads with no step of
their own place the ledger before the live answer, where it stays. A chart or table
card stands where its publish step started (`chartBlocks`/`gridBlocks` give each block
its `stepId`), so a card made before the answer sits above it, live and after reload;
an answer that says "the chart below" can be wrong until the prompt says otherwise. A
question or approval is a block where it was asked, and settles into its one line in
the same place. The card, the status pill and the ledger read the view alone; the pill
derives the cheaper `deriveTurnStatus`.

The footer slot keeps one control's height from the first frame. While the reply runs it
holds the **live line** (`TurnFooter.tsx`): a glyph, the label with a sheen (a 1.4 s
sweep, then a 1 s rest), the target, "+N" other running steps and the clock. The label
is, first match: Stopping; Waiting for you (an amber dot, the only amber in a running
turn); the newest running step in the catalog's localized words, whatever the host
wrote (its target reads "Still working" after 15 s without host activity); Writing;
Still working; Starting (no signal from the host yet); Thinking. A label stays at least
600 ms and a finished step's label holds 800 ms, so quick steps never strobe and the gap
between steps never flashes Thinking. The slot is the turn's only live region, spoken at
most every 5 s, then the receipt once. When the turn settles the live line fades out and
Sources (n), the receipt line, Copy and feedback fade in one after another in the same
place. A stop is one quiet line ("Stopped · kept 6 steps" and Try again, no border); a
failure is the bordered error block with "The steps and the text so far were kept." when
they were. The outline and details triggers fade into the meta row, which already has
their height; next steps and the as-of note render below the footer. The commit itself
waits until the answer's last received word is shown, at most 400 ms after the host's
`completed`, and changes no wrapping and no position of anything already read. On
older answers only Copy and feedback wait for hover; Sources and a failure stay visible.

The renderer draws the reply's shell (name, time and "Starting") as soon as the
question is sent, 120 ms after the question rises in, without waiting for the host.

## Reading while it streams

The thread is grouped into turns (`<section class="turn">`, one per question). The
latest turn keeps a viewport of room below its question while it is being read, so on
send the question glides to 16 px under the top of the viewport (a question taller than
40% of it aligns its bottom there) and the reply writes beneath it. The page then moves
by itself only when a block is appended and the reader has not scrolled during the
turn: prose gets the lower 60% of the viewport, a card or question is shown whole with
room for the live line. Growth never scrolls, commit never scrolls, and opening the
ledger or a disclosure never scrolls. "Jump to latest", End, or scrolling to the very end
while the reply runs follows the end of the turn (18% of the remaining distance per
frame) until any upward input. A chat opened with a reply in flight puts its latest
question at the top and does not make room; a settled chat opens at its end. The pill
(`ChatStatusPill`) overlays the bottom of the thread, 12 px above the composer, only
while the end of the turn is more than 12 px below the fold and the reader is not
following: the live label and clock, "Waiting for you", or "Couldn't finish", then Jump
to latest. A 32 px fade on the composer's edge says more is below. Neither takes the
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
open `$$` run and a marker that just opened. The rendered text therefore only grows;
`streamingMarkdown.test.tsx` replays the live scenario one character at a time to hold
that. Each released word fades in over 220 ms through Streamdown's `animated` option
(opacity only, 20 ms apart, the spans removed when the stream ends;
`src/styles/vendor/streamdown-animate.css`); headings and tables fade in whole. There is
no caret. Streamdown's table, code and diagram controls are hidden while the text
streams. Code and math plugins load once into a module-level store, so a second answer
renders highlighted on its first paint and a plugin arriving mid-answer re-renders
blocks in place.

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
accepted steer prunes the model and asks once more on Auto, in the same turn on a
fresh cold Pi (`commands/chat_runtime/model_gone.rs`); the answer stores
`replaced_model` and names the model that answered instead. While offline a banner
shows and sent messages wait in the ordinary queue, sending on reconnect. Otherwise, when a data read in
the latest answer failed with the tool error `offline` (the completed message's
`offline_reads`), the banner says some data couldn't be fetched because a source
or the network couldn't be reached.

## Learned line

Under every finished answer one line says what it learned, in this order:
"Learned n · n names added to the map · n relationships · n ideas · Used n pages". Sources are
not repeated here: the ledger's receipt says "n sources" and the footer's Sources (n)
toggle opens the numbered list. Only the parts that are not zero appear. Learned counts the sections the answer added,
replaced or retired, read by message id from `memory_learned`; ideas counts filed pages
that are ideas. Used counts every Lens page the host delivered, so an Outlook (perspective)
counts as well as principles, methods, topics and ideas. A private chat, where saving is
off, shows no Learned part and does not ask Rust for one, and a running or failed answer
shows no line.

Choosing Learned or ideas opens the card under the answer: each filed page, with every
added, replaced or retired section. Fix opens the Bot composer with the page pinned
and "Correct this:" ahead of the cursor (the Lens pin path; it never sends). Forget
retires that section through `memory_section_forget` and the row offers Undo. Nothing waits
for approval. Used pages open a quoted list with Not this time and Edit.
