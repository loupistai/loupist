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
state its uncertainty. Method and perspective sections may have an empty basis. The
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
becomes a `mention` anchor; a page created with anchors gets `subject` anchors (a
name anchor without a node stays pending until a World publication names it); a
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
to stay enabled, and the Loupist must not be paused. After at least five added or
disputed claims since the previous review, Rust records one pending review. The
review stays pending until that Loupist has no queued or running World jobs. It is
admitted at most once every six hours and four times in a rolling day, using
the selected World model on the same isolated inference path as World processing.
Pending work survives restart, includes claims committed while the batch drains,
and wakes at the next permitted time even if no later World publication arrives.
A paid or uncertain attempt is not retried automatically. A review left running
at startup is marked a failed uncertain attempt.

Publication requires at least two cited claims from that review's input. The
input includes every active principle statement. If those statements do not fit,
or they change before publication, the review defers and publishes no method. A
publication files a method page through `KnowledgeWrite::file` with origin
`learning`: the model's six fields become the summary and the sections `use-when`
(with assumptions), `steps`, `checks` and `limits`. A refine puts those sections
with the digests the review read; a section the user changed since is skipped and
the review's reason reads `stale_head_folded`. A publication that changes nothing
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
