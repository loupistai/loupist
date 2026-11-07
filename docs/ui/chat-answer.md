# Answer anatomy

An assistant turn is derived in one place, `src/chat/turnView.ts`, from the message
alone. `deriveTurnView` returns an exhaustive state (`starting`, `working`,
`awaiting_approval`, `writing`, `stopping`, `complete`, `stopped`, `failed`), the
segments in reading order, one footer slot and one string for the live region. The
card renders what it is given and keeps no status conditionals of its own.

Segments are prose and activity rows. A commentary item is prose keyed by its id:
narration (the answer's own metrics in the secondary color) once it closed, the live
answer while the phase is writing and it is the last item. The settled answer keeps
the key of the commentary it first settled from (`answer_key`), so the Markdown node
survives the hand-off and nothing remounts or moves. A run of steps is one activity
row with a fixed height: the running step's own words while one runs, then a summary
("Read 2 sources · hanul.example · 1 failed attempt · 0:14") that never changes height.
The details hold the sources, consulted notes, plan and every step; a failed or stopped
step opens them. Sources are counted once, in that summary and in the footer's
Sources toggle.

The footer slot reserves the action row's height from the first frame. While the reply
runs it shows a status line with a clock ("Writing · 0:14", "Working · 0:05",
"Waiting for you", "Stopping") and the only live region for the turn. When the turn
settles the same slot holds Sources (n), the receipt line, Copy, feedback and any
stopped or failed state with Try again. The outline, next steps and as-of note render
below it. Nothing is inserted above the footer when a turn settles, and the Edit
control under earlier questions stays in place, disabled, while a reply runs, so the
thread does not jump at completion. On older answers only Copy and feedback wait for
hover; Sources and a failure stay visible. The status pill repeats the footer label and
the running step for a reader who scrolled up, and is quiet for screen readers.

## Streaming text

Rust streams the answer as a progress commentary item: the whole item once, then
only the text appended since the last patch (`ChatProgressPatch.append_text`), at most
every 32 ms. A commentary id is in `upsert_items` or in `append_text`, never both. When
Pi ends the message with a final answer, the `delta` that carries the authoritative text
names the commentary it replaces (`settles`); the renderer drops its live copy in the
same update and records the id as `answer_key`, and the removal patch that follows then
finds nothing. The durable progress format is unchanged.

Prose runs through `usePacedText` (word reveal over a 160 ms window) and then
`stableMarkdown`, which holds back a table block until its separator row has a newline
after it, an unterminated table row, an open link or evidence link, an open `$$` run,
and a marker that just opened. The rendered text therefore only grows while a reply
streams; `streamingMarkdown.test.tsx` replays the live scenario one character at a time
to hold that. The caret follows a paragraph or list item only. Code and math plugins
load once into a module-level store, so a second answer renders highlighted on its
first paint and a plugin arriving mid-answer re-renders blocks in place. The scroll
owner follows growth from the layout effect of the segment that grew, so a pinned
reader never sees a gap; `scripts/probe-streaming.mjs` measures text drops, height
shrinks, layout shift and the pinned gap on the `chat-live-research` scenario.

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
"Learned n · Map +n names · n relationships · n ideas · Used n pages". Sources are
not repeated here: the activity row says "Read n sources" and the footer's Sources (n)
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
