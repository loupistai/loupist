# Lens learning

Lens is the Loupist's wiki: Markdown pages with keyed sections, written only through `KnowledgeWrite::file`. For the World and Lens pages, see [World and Lens](../ui/map-lens.md).

## Lens

A Lens record has one of five kinds, stored once in `knowledge_lens_metadata.kind`:
`principle` (the user's rule: statement, why, example, origin), `perspective` (the
Loupist's standing view, one active page per Loupist), `method` (a reusable way of
checking something), `subject` (notes about one company, asset,
sector, technology or theme) and `idea` (an unverified connection or thesis with a
falsifier and an `IdeaState`: open, supported, weakened, refuted or retired). Every
kind but principle is a page: a summary of at most 400 characters and at most 16
sections in display order. A section has a slug key (`[a-z0-9-]{1,48}`, its identity
inside the page), a free heading, Markdown of at most 2,000 characters, a basis, an
origin (`user`, `loupist`, `learning` or `shared`), a state (`current`,
`contested` or `superseded`), an optional role and statement date, and an update
time. The Contested and Superseded states store claim IDs and the time the state
began. `src-tauri/src/knowledge/lens_page.rs`
holds this model, the write language and the fold, with no SQL.

A basis is at most eight references: a World claim read this turn, a retained
Source span, the user's message of the filing turn (`said`), or another page
(synthesis). Subject and idea facts, views and sections without a role need a
reference. A check, open question or refutation can omit references when it
states its uncertainty and the page has an evidence-backed section, including
one accepted in the same filing. A view or synthesis with references states
its uncertainty. The host infers the role for `checks`, `open-questions`,
`would-break`, `evidence-for` and `evidence-against` when omitted. Method and
perspective sections may have an empty basis, except
that every turn `lens_file` op on a method page must cite the user's message. The
reader derives a chip from the basis (source, said, view or none); agent prose never
becomes World data. Suggested keys per kind (`suggested_sections`) are an ordering
hint, not a closed set: subject `overview, facts, value-chain, drivers, risks, views,
open-questions`; method `use-when, steps, checks, limits`; idea `claim, mechanism,
evidence-for, evidence-against, would-break, checks`; perspective `beliefs,
priorities, avoid`.

A new Loupist has no Lens page. Its focus (what it researches) lives on the Loupist
profile and the World settings' `research_focus`; Loupist settings and `loupist_setup`
change it there, and every turn's context carries it from the profile
([context assembly](../agent/context.md#context-assembly-and-bounds)). The first
perspective the agent files creates the perspective page. The interface shows the
English default heading of a suggested section key, and the `statement` section a
principle save records in History, in its own language.

Bodies link `lens:rec_…` (navigation, resolved by record id, with backlinks from the
link index) and, on subject and idea pages, `world:<node id>` of this Loupist's
World. Principle, perspective and method pages keep the World-reference ban; no
page may reference a Journal, a snapshot or a World record id. Links are what the
Markdown parser reads (inline, reference-style and autolinks, any scheme case),
for the index and the policy alike. A `world:` link
becomes a `mention` anchor; a page created with anchors gets `subject` anchors. A
name anchor names the node whose stored spelling or identifying code is that
subject, and otherwise stays pending until a World publication stores that
spelling or code ([Worlds](worlds.md#discovery-after-publication)). A shared
entity key refuses a second subject page and lets method learning read the
pending page as that company; it does not store a node. A cited Source
never sets a node. Two nodes that both match leave the anchor pending. A later
create of the same subject, under any of its spellings, is refused rather than
filed as a second page.

Method learning may still read a pending page as its company for one review
([Method learning](#method-learning)); that reading is never stored. A
final-answer `lens:` link records `applied` anchors to the World nodes
delivered in that turn. Anchors drive World's applies-to edges and the Lens
index order.

### Filing

With saving on, every surface files an answer based on an original read this turn
about a named company, asset, sector or theme, without being asked; chatter,
explainers, arithmetic, a single looked-up figure, re-reads and stated one-offs
file nothing. A short standing rule is one principle; a method is a multi-step
procedure the user teaches or a page or check they ask to keep, never both.

`KnowledgeWrite::file(loupist, origin, deltas, at_ms)` is the only page writer. A
delta names a target, either an existing record or a create (kind, title, anchors),
and at most 12 ops: `put` (add or replace a section; `expected` is the digest from
the exact read, absent means add only), `retire` (needs the digest), `summary` and
`idea_status`. A create is idempotent: it resolves to the active page with the same
kind and folded title, or the active perspective, before minting a record. A
subject create whose title or anchors are another spelling of a subject page, or
name a node that page already anchors, is refused as `existing_page` with that
page's record id and title (`Apple`, `Apple Inc.` and `Apple Inc. (AAPL)` are one
page). The same folded title still lands on the page and only adds absent sections:
its summary, status, retire and replacing puts are skipped as stale. The fold never fails on one bad op; each op gets an outcome: added, replaced, retired,
summarized, status_set, already_known (identical content), skipped_stale (digest
mismatch, missing key, or an edit through a create), refused_forgotten (the user
retired that key), refused_basis, refused_full (16 sections), refused_kind or
refused_text (the text policy: a private reference, a `world:` link on a portable
page, or a node this World lacks; a refused title or new page's `about` node refuses
every op of its delta) or refused_inactive (every op of a delta whose existing page was
archived or removed; the user restores a page before editing it). Disjoint edits from the same read both
land; an edit of a section that changed since is skipped, never a proposal. A create
whose ops all refuse leaves no record. A `lens_file` create requires a non-empty
summary, and a revision keeps the existing summary when the filing omits it.
`lens_file` fills each section's `asOf` with the turn date or cutoff when
omitted and rejects a later date. A written
section is contested when its basis cites a claim the live World withdrew, disputed
or does not hold, whoever writes it, undo included; text may hold no control
character but line breaks and tabs in Markdown, uncertainty and reasons.

Before staging, `lens_file` checks the figures of each section whose role is `fact`
or absent against what its basis cites and this turn delivered as text: the cited
claims' propositions, Source reads and saved Sources (`knowledge/figure_check.rs`,
as [World figure checks](worlds.md#figure-checks) read them). A figure that `differs`
refuses the call with `figure_differs`, naming each stated amount and the cited one,
and nothing is staged. A `not_found` figure, such as a margin computed from two cited
amounts, is staged and listed in the result's `number_checks`
(`{field, text, status}`). Sections of role `view`, `check`, `open_question` or
`refutation` are not checked: they hold derived figures, thresholds and targets.

A section citing an ended claim is Superseded until its basis cites the
ending claim, the ending passage or evidence published on or after the end.
Contested takes precedence. World publications preserve the section text and
record a `lens_superseded` growth event for each affected page. A scheduled ending
is reconciled once it takes effect, even if the World head stays the same. The
resulting Lens revision and growth event are recorded together. Superseded
sections stay eligible as method-learning evidence.

Each filing publishes one revision per changed page (the head is the only revision;
history is the log), appends one `knowledge_lens_log` row per op (refused and skipped
included, with the op JSON, outcome, key, before and after digests and the section
or summary that op found, folding ops one at a time), rebuilds links and mention anchors, reindexes search and writes one
growth event per changed page (`lens_created` or `lens_revised`). The log origin is
`turn`, `user`, `learning` or `import`. `undo_op` appends the inverse op with
`undoes_op_id` and refuses when a later op changed that section, summary or status,
or the op was undone already. `undo_message` restores every section that one message
filed, in that same transaction, and leaves a section a later edit changed; it records
the decision, that message, and the restored and kept counts, so a second call and a
reloaded scheduled run report those same counts, including after a reload. A marker
written before those counts has no message id; the run still finds it when
`undoes_op_id` names one of that message's ops, and reports the sections that undo
restored and the ones it left. Only inverses written with that marker count as
restored: an earlier undo of one section is why that section was kept. `forget_section` is a user retire: a later non-user
`put` of that key is refused until the user puts it back. `learned_digest` lists,
per message id, the pages and outcomes that message filed. The agent reaches this
writer through `lens_file` ([turn tools](../agent/turn-tools.md#turn-tools)) with
`FilingOrigin::Turn`. Principles are saved by `principle_save` through the same
metadata row and log.

The bridge reads a page as `PageView` (`knowledge_lens_read`): kind, title, archived,
paused, learned, the body with section keys, digests, basis and chip, suggested
sections, anchors, links, backlinks and the newest 20 log entries. Each section
entry shows the heading recorded by that operation. A retired section takes its
heading from the stored section the retire removed. Undo uses its own inverse
operation. Both the page history and Lens-wide History use that recorded heading.
Older entries show the localized section kind, or “Section” for a custom kind. These history labels belong to the reader and stay out of model
deliveries. `knowledge_lens_search`
accepts a `kinds` filter; `knowledge_lens_pages_for_node` lists the active pages
anchored to a World node; `memory_section_forget`, `memory_op_undo`, `memory_message_undo` and
`memory_learned` are the user's correction and receipt commands. `memory_message_undo`
returns `{undone, kept}`, the counts of sections restored and sections kept, and a
later call returns the counts stored on the marker.

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
it, or World withdrawing a claim it cites, withdraws it. Each turn commit
checks the Lens log rows no review that reached inference has shown, once
something was filed after the head the last review read. When two independent
Chats filed that evidence about two different cases, Rust records one pending
review. Chats are independent when each cites a Source the other never cited. A
page's case is who its subject anchors (`about` on a subject or idea page) name,
whatever the node's kind: a company, a macro series, a theme or a market. One
function, `About::of`, decides it. A page with no node and no name for World to
confirm is a case by its title's name keys when two of its sections cite
different preserved originals, so two Chats about two series or two markets learn
as two companies do, while two Chats about one series, or one Chat about two,
do not.
With World building on, World improves identity and never gates liveness:
a page whose anchors World resolved counts as those nodes, and a page whose
subject it has not resolved counts once World is through with the Sources
the page cites, however that processing ended, so a failed, empty or slow
extraction never gates liveness. A case is the World nodes of the page's subject
anchors, plus the company keys of each node's current World names and aliases
(`entity_name_keys`: accents, apostrophes and punctuation
except `&` dropped, NFKC and casefold, a leading `The` and legal forms such as
`Inc.`, `Co., Ltd.`, `& Co.`, `L.L.C.`, `S.A.`, `S.A.B. de C.V.`, `ASA`,
`(주)`, `(유)`, `주식회사` or `株式会社` stripped while a name remains; each
spelling also keyed without its spaces, so `U.S. Bancorp` meets `US Bancorp`
and `J.P. Morgan` meets `JPMorgan`; a parenthetical in another script,
`삼성전자(Samsung Electronics)`, as a second spelling, while a qualifier in the
name's own script, `(한국)`, `(Holdings)`, `(Class A)`, is no key; a listing
code, parenthesized or after the name, `(005930)`, `(NASDAQ:AAPL)`, as a key of
its own, so `Apple` meets `Apple Inc. (NASDAQ:AAPL)`; a spelling that is only
`EXCH:AG` kept whole, keyed `exch ag` and `ag`; a leading `AB` or `AS` and
`and` for `&` left as words, so `AB Volvo` and `Volvo AB`, or `Johnson &
Johnson` and `Johnson and Johnson`, share no key) and the component of the
nodes whose keys meet through other nodes, so `삼성전자` and `Samsung
Electronics` are one case when a third node spells both. Two cases differ when
they share no node, key or component. So two companies that held one name at different
times stay two cases, while `Acme` and `Acme Inc.` are one case whether the
World names one node or two. Two different companies that share a key only
defer learning; a key never touches a page. The World is read only when a
pending page or the nodes alone call for it, and the snapshot is parsed once
per World revision: a process-wide identity index of the last four revisions
keys a node the first time learning asks about it and joins the components
once. A page whose subject node the current World no longer holds (a shared
pack removed it) is not evidence: it is consumed like an unanchored page, never
counted. A page with no subject anchor and fewer than two grounded sections
citing different Sources could concern anything, so its sections are not
evidence, are not shown to a review, and are consumed like any other row.

A page whose subject World has not confirmed is read against the Sources
its current sections cite, for that read only, once World is through with
every one of them. A Source is processed when the World job table
(`knowledge_graph_outbox`) holds no queued or running job for it and a job for
its current revision has any other status: completed; failed, cancelled,
quarantined or reconciled, since a World that gave up on a Source cannot reveal
more from it; or paused, since a chunk whose frozen allowance cannot fund an
extraction (a report split into more chunks than its call budget feeds two calls
each; the first claim parks them and rechecks learning) or a queue the user
paused is not read without the user's action, and the resume that lets it settle
rechecks learning then. The revision's dependency list is not the ledger: a
Source read without yielding a claim leaves that list on the next publication.
A Source with no job, or none for its latest revision, holds the page. The page
is then a case with the keys of each name it was filed under, with every World
node whose names or aliases carry one of those keys, so with that node's keys
and component too, and with every company in the current World (a node of the
core Organization kind or an extension type of the company category) whose
name or alias may name the page's subject, whichever Source World met it
in and whether or not a claim names it (`WorldIdentity::companies_naming`). A
node may name the subject when the words of one of its keys contain, or are
contained in, the words of one of the page's keys (`identity::may_name`):
`hana` may name `hana financial group` and `hana co`, `halvorsen cellworks` may
name `halvorsen cellworks asa` or `halvorsen cellworks group`, and neither may
name `loupist`, `kpmg` or `nordic auto`. So a page filed under `Halvorsen
Cellworks` is one case with the node extraction canonicalized as `Halvorsen
Cellworks ASA`, whether the report it cites claimed that node, claimed it and
withdrew, or yielded nothing; a `Hana` page is one case with `Hana Financial
Group` and `Hana Co.` wherever World holds them, never independent of
either, even when its own report's extraction failed or found nothing: a short
name cannot pick the counterparty on its own, nor deny a bank World already
knows; and a `Samsung Electronics Inc.` page whose report published nothing is
one case with a page about `삼성전자` when a node spells `삼성전자(Samsung
Electronics)`. The auditor, exchange, bank, customer or regulator a report
names beside its company is no case of the page, since its name does not match
the page's: two reports whose disclaimers both name `Loupist` or `KPMG` leave
their two companies two cases, so the review queues. A name World never
extracted, because the extraction failed or found nothing, still counts under
its own key, so two Chats about `Halvorsen Cellworks` and `Ferroluz
Componentes` learn with no World output at all, while `Kestrel Foundry` and
`Kestrel Foundry Inc.` stay one case. A Product named like the page is no
company it could be about.
The anchor stays pending in storage either way. A pending page citing a Source
World is not through with is not evidence, but its rows are not consumed: a
review that reaches inference advances its watermark past them and stores their
ids as `held` in its snapshot, carried forward and rebuilt on every read, so the
job or publication that frees them still finds them, however many reviews ran
meanwhile.
`held` also keeps confirmed rows below the watermark that did not fit the
input, and the watermark never moves below the one already consumed, so a
row leaves `held` only when shown or no longer current; rows above the
watermark that a review showed are stored as `shown`, and the latest started
review's snapshot is the one read. A name on a Source World never
reads never counts. So one Chat, one Source, one subject however many subject
and idea pages it has, an unanchored page without two grounded Sources, a page citing an unread Source, or a
Chat repeating another's Sources never starts one. A World publication that resolves pending subject
anchors, that changes the name or alias set of a node a confirmed subject
anchor names, or that lands while any live page has a pending subject anchor,
rechecks the evidence at once, without waiting for a new filing, under the
same rate limits and active-review rule; a publication that spells every
subject as before, with no pending page, does not. A Source job that reaches a
final status rechecks too while any live page has a pending subject: after a
failed or quarantined attempt, after the completed status is written behind a
publication (the seal rechecked while the job still ran, so its own Source was
not yet processed), and at startup once interrupted jobs are given their
final status. A review queued or running
at that moment rechecks when it settles: its row stored how many subject
anchors World had confirmed when it was queued, its claim snapshot how
many at the claim, and a larger count then means a publication landed
meanwhile. A settle after inference whose snapshot still holds rows rechecks
too, so held leftovers reach their next review without a filing, while a
review deferred before inference held nothing and never requeues itself.
Rows already shown to a review are never offered again, even when a later
publication renames their company.
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

The bridge read `knowledge_learning_state {loupistId}` returns `{state, reason,
cases}`, decided by the checks the queue and the claim make, in their order.
`state` is `waiting`, `idle` (the research passes the gate, but a review that
settled before inference already read it), `queued` or `running`; `reason` is
set exactly while it waits: `off` (a switch above is off or the Loupist is
paused), `rate_limited {untilMs}`, `allowance` (a zero call budget or a spent
daily background allowance), then, with no review active, `sources_unread {n}`
(held pages cite `n` Sources World is not through with), `subjects_pending {n}`
(`n` held pages whose subject only World can confirm: a name that is only
notation, or no World yet) or `no_second_case`. `cases` counts the distinct
cases in the research no review has consumed; pages sharing a node, key or
component are one case.

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
cited observations were filed on. Every active method is shown, most recently
updated first: all by title and the first 100 characters of their summary, then
the newest with their summary and those four sections in full while the methods
stay within 8,000 characters. When even that listing does not fit the input, the
review defers with `methods_exceed_input_limit` before inference, as it does for
principles. Observations that the methods leave no room for stay `held` or past
the watermark, never dropped. A refine may target only
a method shown in full; any other target is `invalid_output`. The prompt tells it
to keep every condition, check and limit the new evidence does not address. A
refine puts those sections with the digests the review read and never replaces
a section the user wrote, by hand or in a Chat turn whose basis cites the user's
message (`said`); a section the user
changed since is skipped and the review's reason reads `stale_head_folded`. A
publication that changes nothing, or a refine left with no section to write,
defers with `method_unchanged`.
Invalid model output defers a review; storage and revision-conflict errors roll
back publication and settle it as failed with `publication_failed`, preserving
usage and allowing later reviews. Only a failed settlement propagates the error.
Deferral and failure write a review
record and no growth event. Learned means a retained method, not model-weight
training or a measured quality gain. A saved
method alone does not demonstrate improved later work; use
[evaluation](../../apps/loupist/evals/README.md) to test transfer. Loupist settings has the Learn methods from new information switch. Lens → History
shows completed reviews, including **Nothing worth keeping** for a deferral. A
successful review announces the method and links to its document in Lens. When
that Loupist's World is visible, World entities it applies to glow for four
seconds, suppressed under reduced motion. An
unacknowledged learned publication stays marked New for seven days.

### User principles

User principles are Lens records of kind `principle` with a statement, optional
why and example, and user origin (`settings`, `feedback` or `chat`), or `shared` for one
accepted from a [shared Loupist](sharing.md#sharing). Lens shows the statement, its origin, how many answers read it and which chats those answers came from. Asking your Loupist in Bot for a change uses the admitted Memory tools. A principle linked from a World entity opens in Lens. An origin Chat
link opens that conversation without changing its draft. Reply in Chat to give a
correction and ask your Loupist to save it as a principle. The Memory row offers
Undo, which archives, and View, which selects the principle in Lens. Older principles
with a feedback origin stay active and keep their link to the original Chat. The
host still accepts verified feedback origins through the existing wire contract.
Active statements are unique after
normalization. Listing titles are limited to 80 characters; delivery receipts keep
the full statement.

Memory-enabled turns receive active, unpaused principles in their own block,
newest first, at most 30 rules and 6,000 characters including the overflow notice.
They outrank the standing perspective. Memory-off turns omit them. Only statements
actually attached to that bounded block create context receipts and usage deliveries.
Delivered context counts as acknowledgement for replacement and does not by
itself create World `applies_to` edges. An exact Lens read returns `lens_url` to open the rule or method from an answer. A final-answer `lens:` link to a current
same-Loupist principle or method records applications to World entities delivered
in that turn. `principle_save` stages a Chat-origin change only with Memory read
and write permission. Replacement requires the admitted or explicitly read
current revision. Successful turn finalization commits the change with the reply.
Archived principles stay readable and leave automatic context.
