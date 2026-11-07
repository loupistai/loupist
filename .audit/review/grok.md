# Lens eval and method-learning review

Scope: `git diff 3914a31..bafcb76` at `bafcb76`. Read-only. No tests executed and no model calls.

## 1. A pre-inference defer burns filings the model never saw

`apps/loupist/src-tauri/src/knowledge/learning.rs:122-128` sets `through_event` to the loupist's lens-log head when the full window passes `recurs`. `build_input` (`379`, `416-426`) then keeps observations newest-first and drops the rest once the JSON passes 12,000 characters; existing methods already stop the input near 7,000. `claim` (`553-564`) runs `recurs` on what remains and settles `insufficient_turns`. `settle` (`575-587`) leaves `through_event` where enqueue put it.

Scenario: Chat A files two span-cited subject sections. Chat B later files a dozen sections on one company (the catalog allows 40 ops on a reply). Enqueue sees two chats and two sources and queues a review watermarked at the log head. Claim keeps Chat B, drops Chat A, and settles `deferred` / `insufficient_turns`. Chat A's rows are at or below `through_event`, so no later review reads them. That defer has no `started_at_ms`, so `rate_limit_until` (`154-161`) counts `created_at_ms` and holds the next review for six hours.

The same consume happens when active principle text alone exceeds 12,000 characters (`379`). Claim settles `principles_exceed_input_limit` before it copies any observation. Shortening the principles afterward does not reopen that window.

Fix: while fitting the input, drop extra sections from turns already represented, and keep one observation from every chat and source in the window. When the untruncated window passes `recurs` and the fitted input does not, or the principles do not fit, settle without moving `through_event` past the previous review.

## 2. Undone turn filings still count as evidence

`evidence` (`learning.rs:250-256`) keeps `outcome IN ('added','replaced')` and never reads `undoes_op_id`. Undo appends a new row and leaves the original outcome as `added`.

Scenario: two chats file a qualifying pair and a review is pending or queued. The user undoes one chat's put. Claim still loads that op's markdown and spans, and publish can file a method from the section the user reverted. If the undo removed the only section from the second chat, the gate still passes on the ghost row.

Fix: skip an op when a later log row has `undoes_op_id` equal to its `op_id`.

## 3. The lifecycle grade passes a stale or wrong page, and the judge never sees the page

`apps/loupist/evals/agent/grade.mjs:363-375` accepts a page when an unanchored regex hits the summary or any section. `basis: "span"` passes when any section has a span, including one that does not hold the figure. Update checks do not require the previous figure to be gone. `judge.mjs:83-93` and `141-152` send the answer and tool metadata. `save_precision` adds `page_kind` and basis kinds (`38-43`) and the criterion at line 14 tells the judge to fail a superseded value kept as current. The judge has no section markdown to apply that to. Update rubrics score the answer.

Concrete passes of a bad page, and one false fail of a good answer:

- LL-002 (`suites/lens-lifecycle-v1.json:237`) passes when the 한빛 page contains `3,600` or `3600` anywhere, including inside `13,600`, while `4,200억` is still the current guidance. The create check `4,?200` (`195`) matches the `4,200` inside `14,200`.
- LL-003 (`363`) passes when the Corvane page contains `terminat` and still says the Pelham contract runs through 2028. The probe pattern at `400` fails a correct answer that contains the words `runs through 2028`, including "it no longer runs through 2028".
- LL-004 (`481`) treats `71` as a hit inside `710` or `171`. `count: 1` on `halvren|할브렌` still allows a second subject titled "Penang fab".
- LH-001 (`heldout/lens-lifecycle-heldout-v1.json:87`) matches `miss|endpoint|failed` inside "has not missed its endpoint and has not failed". LH-002 (`202`) matches `14` inside any larger number that contains those digits.
- LL-008: `lens_read` (`grade.mjs:348-353`) matches any exact read whose title contains `capacity`. The answer rubric's facts (49% utilization, negative incremental margin) are in the attachment, so the answer can pass without using the seeded method.

Held-out companies are in other sectors (Vellam biotech, 동진해운, Pinecrest foods). The scenarios are the same templates as the development file: file a subject, forbid every method with `title_matches: "."` and `max: 0`, update on one unanchored token, then a read-only probe. A change tuned to copy the newest attachment onto a subject page and never create a method passes both. No lifecycle turn expects `kind: method` with a minimum of one when the user asks for a method, so that refusal is unmeasured.

`eval_harness/research.rs:437-451` treats `deferred` as the successful two-chat review, and checks `method_spans_sources` only when a method exists. A run that defers, with probes that write nothing, exits clean. The module comment says the test proves a method is learned. `measures` records quality and does not fail the test.

Fix: anchor numbers so `4,200` does not match inside `14,200`, and require the old figure to be absent or marked as the previous value on update turns. Put `lens.pages` section markdown in the judge input. Add one development case and one held-out case where the user asks to save a method and the store must contain it. Fail the live test on a two-chat deferral when the claim is that a method was learned.

## 4. Two chats still generalize one company, and learning can rewrite a user method

`recurs` (`learning.rs:221-228`) counts distinct messages, chats, and source record ids. It does not look at subject anchors.

- Same company. Both chats put sections on one subject page, citing two filings. Enqueue, claim, and publish all pass. The learning prompt says not to turn a company fact into a method; `publish` does not check the text or the anchors.
- Second chat adds no source. Chat A cites Filing A and Filing B. Chat B cites only Filing A. Chat and source counts are both 2.
- A turn-filed method is evidence. The query includes `method` (`255`). Two chats that put span-cited method sections, with no subject research, start a review.
- Refine replaces the user's sections. `publish` (`739-780`) puts `use-when`, `steps`, `checks`, and `limits` at the digests claim read, with the cited spans as basis. A user-origin section whose digest still matches is overwritten. Two later subject filings can rewrite a method the user wrote. `stale_head_folded` applies only when the digest changed.

Fix: drop any chat whose source set is a proper subset of another chat's, then require two chats and two sources. When the cited pages have subject anchors, require two distinct anchors. Remove `method` from the evidence kinds. On refine, skip sections whose stored origin is `user`, and defer `method_unchanged` when that leaves nothing to write.

## 5. The prompt refuses a method the user asked to save

`apps/loupist/agent/SYSTEM.md:176-179`, `apps/loupist/agent/skills/lens/SKILL.md:26-27`, and the `lens_file` description in `apps/loupist/agent/tool-catalog.json:898` allow a method only when the user teaches or corrects one, and they forbid a method from the agent's own analysis or from a single company's case. None of the three says to file one when the user asks to keep a check the conversation just worked out.

Scenario: the user says to save a Lens method titled "Headline share count" and states the 15% dilution rule with Marrow Glass's share counts (`evals/agent/suites/learning-v1.json`, the Headline share count turn). That is one company and a check just derived in the chat. The skill's single-company ban covers it. The agent stores a principle or a subject note. Every lifecycle `max: 0` method assertion rewards the refusal.

Fix: in all three texts, say that an explicit request to keep a check files a method, with `limits` naming the scope the user actually covered, and that a method the user did not ask for stays on the subject or idea page.

## 6. A rejected citation URL is told to copy some other result

`apps/loupist/src-tauri/src/commands/tool_executor/evidence.rs:43-49`. The model submits a `citation_url` this turn did not return. The error lists up to six recent URLs and says "copy one this turn returned."

Scenario: the turn read a filing and ran a search. The model mistypes the filing URL. The refusal offers the search URL with the same instruction. The model copies it onto the filing's figure, and the section basis names the wrong result.

Fix: list each URL with its tool name and say to use the one whose result contains the figure, or to leave the input unsourced.

## Checked, no finding

- Watermark unit. `through_event` is that loupist's `knowledge_lens_log.rowid` (`117-124`, `132-137`), so two filings in the same millisecond stay ordered.
- Settle to enqueue. `settle` calls `enqueue` once (`585-587`). `enqueue` does not call `claim` or `settle`. A `queued` or `running` review does not start a second one (`110-116`). Filings while `pending` are included at admission (`150-151`). Filings after the watermark is frozen are reconsidered when that review settles. The restart test covers this.
- Rate limit. `pending` does not consume a slot (`154-161`). The six-hour gap, the four-review day, and `next_wake_at` match the docs.
- Rollback. An error after `enqueue` in the filing transaction leaves neither the page nor the review (test `a_rolled_back_turn_leaves_no_review`). `finish` publishes inside `SAVEPOINT learning_publication` and rolls that savepoint back before the `invalid_output` settle (`657-675`).
- Span join (`240-248`). It keeps an included publisher, user, or tool capture whose representation is not `derived`, excludes `shared_pack:`, and drops paused or removed record ids (`274-298`). Analysis authorship and shared packs are covered by `only_preserved_originals_count`. Eligibility follows the span row's own representation; I found no path where skipping `revision_digest` admits a span that join would reject.

Verdict: Do not adopt `bafcb76`; qualifying research can be watermarked out before the model sees it, and the lifecycle grade passes a page that still states the stale figure.
