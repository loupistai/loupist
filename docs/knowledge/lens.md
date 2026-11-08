# Lens learning

Lens is the Loupist's wiki: Markdown pages with keyed sections, written only through `KnowledgeWrite::file`. The Map control is [Map and Lens](../ui/map-lens.md).

## Lens

A Lens record has one of five kinds, stored once in `knowledge_lens_metadata.kind`:
`principle` (the user's rule: statement, why, example, origin), `perspective` (the
Loupist's standing view, one active page per Loupist), `method` (a reusable way of
checking something), `subject` (what the Loupist knows about one company, asset,
sector, technology or theme) and `idea` (an unverified connection or thesis with a
falsifier and an `IdeaState`: open, supported, weakened, refuted or retired). Every
kind but principle is a page: a summary of at most 400 characters and at most 16
sections in display order. A section has a slug key (`[a-z0-9-]{1,48}`, its identity
inside the page), a free heading, Markdown of at most 2,000 characters, a basis, an
origin (`user`, `loupist`, `learning` or `shared`), a state (`current` or
`contested` with claim ids) and an update time. `src-tauri/src/knowledge/lens_page.rs`
holds this model, the write language and the fold, with no SQL.

A basis is at most eight references: a World claim read this turn, a retained
Source span, the user's message of the filing turn (`said`), or another page
(synthesis). Subject and idea sections need at least one reference; a section whose
references include a page, or carry no claim, span or `said`, is synthesis and must
state its uncertainty. Method and perspective sections may have an empty basis, except
that every turn `lens_file` op on a method page must cite the user's message. The
reader derives a chip from the basis (source, said, view or none); agent prose never
becomes World data. Suggested keys per kind (`suggested_sections`) are an ordering
hint, not a closed set: subject `overview, facts, value-chain, drivers, risks, views,
open-questions`; method `use-when, steps, checks, limits`; idea `claim, mechanism,
evidence-for, evidence-against, would-break, checks`; perspective `beliefs,
priorities, avoid`.

A new Loupist with a focus gets its perspective page at creation
(`ensure_perspective` in `commands/knowledge/chat_reads.rs`): the title "Outlook" and
one `focus` section headed "Focus" holding the focus text. The focus is what the
Loupist researches, not a belief, so it is not seeded under `beliefs`. The interface
shows the host-default title and heading in its own language. An empty focus seeds
no page; the first filed perspective becomes it. A later focus change, from Loupist
settings or `loupist_setup`, rewrites only that `focus` section
(`sync_perspective_focus`); clearing the focus leaves the page as it was.

Bodies link `lens:rec_…` (navigation, resolved by record id, with backlinks from the
link index) and, on subject and idea pages, `world:<node id>` of this Loupist's
World. Principle, perspective and method pages keep the World-reference ban; no
page may reference a Journal, a snapshot or a World record id. Links are what the
Markdown parser reads (inline, reference-style and autolinks, any scheme case),
for the index and the policy alike. A `world:` link
becomes a `mention` anchor; a page created with anchors gets `subject` anchors. A
name anchor names the node whose name or alias folds to the same text, and
otherwise stays pending until a World publication spells it so. Nothing looser
binds an anchor or merges subject pages: `Hana` and `Hana Co.` may be two
companies, so their pages stay apart until the World says who each is. A
final-answer `lens:` link records `applied` anchors to the World nodes delivered
in that turn. Anchors drive the Map's applies-to edges and the Lens index order.

### Filing

`KnowledgeWrite::file(loupist, origin, deltas, at_ms)` is the only page writer. A
delta names a target, either an existing record or a create (kind, title, anchors),
and at most 12 ops: `put` (add or replace a section; `expected` is the digest from
the exact read, absent means add only), `retire` (needs the digest), `summary` and
`idea_status`. A create is idempotent: it resolves to the active page with the same
kind and folded title, or for a subject the same primary anchor, or the active
perspective, before minting a record. A create that lands on an existing page only
adds absent sections: its summary, status, retire and replacing puts are skipped as
stale. The fold never fails on one bad op; each op gets an outcome: added, replaced, retired,
summarized, status_set, already_known (identical content), skipped_stale (digest
mismatch, missing key, or an edit through a create), refused_forgotten (the user
retired that key), refused_basis, refused_full (16 sections), refused_kind or
refused_text (the text policy: a private reference, a `world:` link on a portable
page, or a node this World lacks; a refused title or new page's `about` node refuses
every op of its delta) or refused_inactive (every op of a delta whose existing page was
archived or removed; the user restores a page before editing it). Disjoint edits from the same read both
land; an edit of a section that changed since is skipped, never a proposal. A create
whose ops all refuse leaves no record. An empty summary clears it. A written
section is contested when its basis cites a claim the live World withdrew, disputed
or does not hold, whoever writes it, undo included; text may hold no control
character but line breaks and tabs in Markdown, uncertainty and reasons.

Each filing publishes one revision per changed page (the head is the only revision;
history is the log), appends one `knowledge_lens_log` row per op (refused and skipped
included, with the op JSON, outcome, key, before and after digests and the section
or summary that op found, folding ops one at a time), rebuilds links and mention anchors, reindexes search and writes one
growth event per changed page (`lens_created` or `lens_revised`). The log origin is
`turn`, `user`, `learning` or `import`. `undo_op` appends the inverse op with
`undoes_op_id` and refuses when a later op changed that section, summary or status,
or the op was undone already. `forget_section` is a user retire: a later non-user
`put` of that key is refused until the user puts it back. `learned_digest` lists,
per message id, the pages and outcomes that message filed. The agent reaches this
writer through `lens_file` ([turn tools](../agent/turn-tools.md#turn-tools)) with
`FilingOrigin::Turn`. Principles are saved by `principle_save` through the same
metadata row and log.

The bridge reads a page as `PageView` (`knowledge_lens_read`): kind, title, archived,
paused, learned, the body with section keys, digests, basis and chip, suggested
sections, anchors, links, backlinks and the newest 20 log entries. `knowledge_lens_search`
accepts a `kinds` filter; `knowledge_lens_pages_for_node` lists the active pages
anchored to a World node; `memory_section_forget`, `memory_op_undo` and
`memory_learned` are the user's correction and receipt commands.

Sharing exports only portable kinds (principle, perspective, method), as summary and
keyed sections without their basis; an accepted section starts with an empty basis and
`shared` origin. Subject and idea pages stay local. Memory reset removes Loupist-origin
pages and retires Loupist-origin sections from pages the user keeps. A principle or
page can be paused independently of learning; the perspective cannot be paused.
Archiving releases the title; restoring refuses a taken one.

### Method learning

Method learning defaults on. It also requires World processing and Memory saving
to stay enabled, and the Loupist must not be paused. Evidence is a turn's added or
replaced section on an active subject or idea page whose basis cites a span of a
preserved original: an included publisher, user or tool capture that is not from
a shared pack, removed or paused. Method pages are never evidence, so a turn's own
method edits cannot feed learning. A put counts only while it is still that
section's current, uncontested state: undoing, forgetting, retiring or replacing
it, or the World withdrawing a claim it cites, withdraws it. Each turn commit
checks the Lens log rows no review that reached inference has shown, once
something was filed after the head the last review read. When two independent
Chats filed that evidence about two different cases, Rust records one pending
review. Chats are independent when each cites a Source the other never cited. A
page's case is who its subject anchors (`about` on a subject or idea page) name.
Only companies the World confirmed count: a case is the World nodes of the
page's subject anchors, plus the company keys of each node's current World
names and aliases (`entity_name_keys`: accents, apostrophes and punctuation
except `&` dropped, NFKC and casefold, a leading `The` and legal forms such as
`Inc.`, `Co., Ltd.`, `& Co.`, `L.L.C.`, `S.A.`, `ASA`, `(주)`, `주식회사` or
`株式会社` stripped while a name remains, and a parenthetical such as
`삼성전자(Samsung Electronics)` as a second spelling). Two cases differ when they
share no node and no key. So two companies that held one name at different times
stay two cases, while `Acme` and `Acme Inc.` are one case whether the World
names one node or two. Two different companies that share a key only defer
learning; a key never touches a page. The keys are read from the World only when
the nodes alone would start a review. A page with no subject anchor could concern
any company, so its sections are not evidence, are not shown to a review, and
are consumed like any other row. A page whose subject the World has not confirmed
yet is not evidence either, but a review that reaches inference leaves its rows
unconsumed, below its watermark like rows that did not fit, so the publication
that confirms the company still finds them. A name the World never confirms
never counts. So one Chat, one Source, one company however many subject and idea
pages it has, an unanchored or unconfirmed page, or a Chat repeating another's
Sources never starts one. A World publication that resolves pending subject
anchors rechecks the evidence at once, without waiting for a new filing, under
the same rate limits and active-review rule. A review running at that moment
rechecks when it settles: its claim stored how many subject anchors the World
had confirmed, and a larger count then means a publication landed meanwhile.
Rows with a non-empty `world_job_id` come from the earlier claim-count
trigger and hold growth-event rowids: they never set the watermark, and startup
defers any still pending or queued with `trigger_changed`.

A review is admitted at most once every six hours and four times in a rolling day,
counting only reviews that reached inference, using the selected World model on
the same isolated inference path as World processing. Pending work survives
restart, includes turns filed while it waits, and wakes at the next permitted time
even if no later turn arrives. A review that defers before inference consumes
neither its evidence nor a rate-limit slot. A settled review rechecks evidence
filed past the watermark. A paid or uncertain attempt is not retried
automatically. A review left running at startup is marked a failed uncertain
attempt, and its Loupist rechecks evidence filed past that attempt's watermark.

The input lists that evidence newest first as observations: the page, heading,
up to 600 characters of text and the cited Source titles, with turns and Chats
as ordinals rather than ids. It first takes the newest pair that passes the gate
on its own, then existing methods, then the remaining observations oldest first
until the 12,000-character input is full. A review that reaches inference
consumes only what it showed: its `through_event` becomes the row before the
oldest observation left out, and it stores the shown rows above that in
`input_json`. What did not fit starts the next review once it passes the gate
and the rate limit allows. It includes every active principle statement.
If those statements do not fit, or they change before publication, the review
defers and publishes no method. When the evidence no longer passes at claim, the
review defers with `insufficient_turns`. Publication re-resolves the cited
observations against current evidence and Source availability; withdrawn ones
drop out and the rest must pass the same gate. A publication files a method page
through `KnowledgeWrite::file` with origin `learning`: the model's six fields
become the summary and the sections `use-when` (with assumptions), `steps`,
`checks` and `limits`, each with a basis of the cited spans (at most eight, one
per Source first). Its applied anchors are the subject anchors of the pages the
cited observations were filed on. Methods fill the input up to 8,000 characters,
most recently updated first, each with its summary and those four sections in
full when they fit and by title and use-when otherwise. A refine may target only
a method shown in full; any other target is `invalid_output`. The prompt tells it
to keep every condition, check and limit the new evidence does not address. A
refine puts those sections with the digests the review read and never replaces
a section the user wrote, by hand or in a Chat turn whose basis cites the user's
message (`said`); a section the user
changed since is skipped and the review's reason reads `stale_head_folded`. A
publication that changes nothing, or a refine left with no section to write,
defers with `method_unchanged`.
Deferral and failure write a review record and no growth event. Learned means a
retained method, not model-weight training or a measured quality gain. A saved
method alone does not demonstrate improved later work; use
[evaluation](../../apps/loupist/evals/README.md) to test transfer. Loupist settings
shows Learn methods from new information and the ten most recent reviews. A
successful review announces the method and links to its document in Lens. When
that Loupist's Map is visible, the World entities it applies to glow for four
seconds, suppressed under reduced motion. An
unacknowledged learned publication stays marked New for seven days.

### User principles

User principles are Lens records of kind `principle` with a statement, optional
why and example, and user origin (`settings`, `feedback` or `chat`), or `shared` for one
accepted from a [shared Loupist](sharing.md#sharing). The Lens panel shows the statement, its origin and application history. Asking the Bot for a change uses the admitted Memory tools. A principle linked from a World entity opens in Lens. An origin Chat
link opens that conversation without changing its draft. Saved answer corrections
can become principles. Feedback and the Memory row offer Undo, which archives, and
View, which selects the principle in Lens. Direct creation accepts a
settings or verified answer-feedback origin. Active statements are unique after
normalization. Listing titles are limited to 80 characters; delivery receipts keep
the full statement.

Memory-enabled turns receive active, unpaused principles in their own block,
newest first, at most 30 rules and 6,000 characters including the overflow notice.
They outrank the standing perspective. Memory-off turns omit them. Only statements
actually attached to that bounded block create context receipts and usage deliveries.
Delivered context counts as acknowledgement for replacement and does not by
itself create Map `applies_to` edges. An exact Lens read returns `lens_url` to open the rule or method from an answer. A final-answer `lens:` link to a current
same-Loupist principle or method records applications to World entities delivered
in that turn. `principle_save` stages a Chat-origin change only with Memory read
and write permission. Replacement requires the admitted or explicitly read
current revision. Successful turn finalization commits the change with the reply.
Archived principles stay readable and leave automatic context.
