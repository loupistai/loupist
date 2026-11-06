# Lens·World redesign, candidate "fable"

The user's goal, restated: Loupist's agent should grow its own Memory (Lens as an LLM wiki, World as a visible GraphRAG) while it does ordinary chat tasks and scheduled runs, so research quality and the discovery of non-obvious connections improve with use and the user never manages Memory. The Bot stays effortless; research sessions stay the expert space.

One decision from the user folded in: the World stays a visible graph (the Map); how it is manipulated and how evidence is shown may change. One open choice this candidate takes a position on: model-proposed vocabulary changes apply after host validation, are shown as a change, and can be undone, with no approval step. The argument is in [Vocabulary changes](#vocabulary-changes-why-auto-apply).

## Problem

Today Memory grows only by per-turn judgment and lands in the wrong places. Research conclusions go to a per-Chat working note that is replaced, so nothing compounds (`note_save`, 6g). Lens has three kinds stored three ways (`knowledge_lens_metadata.lens_kind`, payload `$.lensKind`, `knowledge_standing_selection`), one revision per record (`knowledge_record_revisions.record_id UNIQUE`), one change row per record, and bodies that may not point at World entities (`lens.rs:334-390`). Method learning never sees research, fires only on five new claims, and publishes straight to Lens. The World vocabulary is invisible to the Chat agent and changes only through a settings page the user cannot judge. Nothing finds multi-hop connections between what the user follows and what arrived. The model is never told it is the Bot, cannot hand work to a session, and the Bot never shows what was learned.

Constraints kept: Rust owns writes, permissions, as-of and shared wrapping; model writes stage in `ToolCapture` and commit in `commit_completed_turn`'s one transaction; `tool-catalog.json` is the one tool schema; broker cardinality stays (one note, ≤16 sources, ≤8 Lens writes); background inference stays on `dispatch_inference` with no paid retry; the World is a sealed snapshot of nodes and claims (`WorldSnapshotBody`) extracted from retained Sources, drawn by the Map.

## Usage (caller's view)

### A research session turn (the model)

The user, in a session titled "Hynix HBM supply", asks whether SK hynix keeps its HBM share through 2027. The envelope now opens with a conversation block (`conversation: {role: "session", title}`) and 6h carries the World vocabulary (`Organization, Person, Asset, Product, Technology, Place, Policy, Event, Metric + extension: ProcessNode, QUALIFIES_FOR`). 6e lists `subject` pages whose `about` entities match the followed assets by identity, not lexical match.

```text
world_search  {query: "SK hynix HBM share", scope: "world"}
world_read    {scope: "world", recordId: "rec_world", centerNodeId: "node_skhynix"}        -> readRef r1, sourceTargets
world_read    {scope: "source", recordId: "rec_src_42", spanId: "span_7"}                   -> readRef r2, citation_url
lens_search   {query: "HBM share"}                                                          -> hit: subject "HBM supply, 2025-27" rec_s1
lens_read     {recordId: "rec_s1", status: "applied"}                                       -> readRef r3, revisionId rev_9
web_fetch ... workspace_capture ... source_save {result_ref, title, coverage}               -> saveRef s1
lens_update   {recordId: "rec_s1", expectedRevisionId: "rev_9", rationale: "...",
               page: {kind: "subject", markdown: "...cites loupist://source/...",
                      about: [{nodeId: "node_skhynix"}, {nodeId: "node_hbm"}],
                      openQuestions: "Does Micron's 1-beta ramp close the yield gap by Q2 2027?"},
               sourceReads: ["r2", "s1"], worldReads: ["r1"], lensReads: ["r3"]}            -> staged, waiting_for_turn_commit
```

The reply commits. Rust commits the subject revision (tier `loupist_sourced` because the write carries acknowledged reads), writes `lens_revised` with before/after summaries, updates `knowledge_lens_subjects` from `about`, queues World extraction for the new Source, and the receipt under the answer shows "Updated page: HBM supply, 2025-27 · Undo". The Bot receives one host post: "Learned in Hynix HBM supply: updated 1 page, 3 names and 5 relationships" with Open.

The learns claim is checkable: `knowledge_record_revisions` gains a second row for `rec_s1`, `knowledge_lens_changes` gains a row with `tier='loupist_sourced'`, and the next turn in any chat that mentions SK hynix receives `rec_s1` in 6e. The headless eval `L-010` asserts a committed Memory change of kind `subject` with `about` naming the seeded entity, then a second session whose trace shows `lens_read rec_s1`.

### A scheduled briefing run

The briefing prompt is unchanged in shape. With saving forced on, the run reads `assetDeltas`, saves the new 10-Q with `source_save`, and now ends by filing: `lens_update {page: {kind: "subject", ...}}` for each followed asset it explained, reusing the page it found with `lens_search`. The unattended allowlist already admits `lens_update`. After the run, the Bot's `delivery` post carries `learned` (pages, names, relationships), not just the first paragraph.

### The Bot (the model and the user)

User, in the Bot: "is the Micron HBM news bad for Hynix?" The envelope says `conversation.role = "bot"`. SYSTEM.md's Bot rule: answer from Memory and one or two reads, keep it short, and when the question needs sustained work, open a session. Here the Bot answers in four sentences citing the subject page's two latest Sources, and adds: "I can dig into the 2027 share question in a research session." If the user says yes:

```text
session_open {title: "Micron vs Hynix HBM share 2027", brief: "Compare ... Check ...",
              entityIds: ["node_skhynix", "node_micron"], lensRecordIds: ["rec_s1"], start: true}
```

Rust creates the session chat, pins the entities and pages into its first request, starts the turn, and posts "Opened research session · Micron vs Hynix HBM share 2027 · Open" in the Bot. When that session's turn commits, the Bot gets the `learned` post above. The user never leaves the Bot unless they want the expert surface.

A user who says "track clinical trial phases as their own kind" in the Bot:

```text
world_schema_update {add: {types: [{name: "ClinicalTrial", doc: "...", labels: {ko: "임상시험"}}],
                           relations: [{name: "TESTS", doc: "...", pairs: [["Organization","ClinicalTrial"]]}]},
                     rationale: "user asked to follow trial phases"}
```

Rust validates (bounds, protected names, no duplicate of core), stages, commits with the reply as extension version n+1, writes `schema_extended`, and the receipt shows "Vocabulary: added ClinicalTrial, TESTS · Undo". Undo appends version n+2 equal to n. The same path serves the model-written seed and review proposals, which now apply themselves after validation and post a "Vocabulary changed" card in the Bot with Undo.

### The user's Memory views

"What {Loupist} knows" becomes five tabs: About you, Pages, Methods, Principles, History. Pages is the wiki: subject and hypothesis pages, an index line per page (title, kind, about-chips, tier badge "From 4 sources" or "Loupist's inference", last revised), a reader with revisions and Restore, and a Needs attention group from lint (orphan, uncited, stale, entity removed). The Map keeps its graph; an entity's inspector adds one "Loupist's page" line and a Hypotheses line. What's new on the Map lists vocabulary changes with Undo. The Bot's title renders as the localized "Bot", never "Thread".

## Shape

### Data shapes

**Lens page.** One record kind `lens`, one payload type `LensPage { title, body: LensBody }` where `LensBody` is a tagged enum: `Perspective{six fields}`, `Method{six fields}`, `Principle{statement, why, example}`, `Subject{markdown, about, open_questions}`, `Hypothesis{markdown, mechanism, expected, falsifier, path_claim_ids, status, registered_at_ms}`. The `kind` tag in the payload is the single source of truth; `knowledge_lens_metadata.lens_kind` is written from it at commit and exists for SQL indexes (per `model-the-domain`: a tagged enum, not three columns that can disagree). `knowledge_standing_selection` is deleted; the standing perspective is the one active `perspective` page, enforced by a partial unique index. `LensKind::portable()` (perspective, method, principle) decides which pages a shared pack exports and which may not cite World ids; subject and hypothesis pages may cite claims and Sources because they are this Loupist's own synthesis.

**Revision history.** `knowledge_record_revisions.record_id` loses UNIQUE; `knowledge_record_heads` remains the head pointer. `knowledge_lens_changes` loses UNIQUE and becomes the append-only log with `tier` (`user`, `shared`, `loupist_sourced`, `loupist_inferred`) derived at commit from actor plus acknowledged reads, never set by the model (per `boundary-discipline`). Restore is a new head pointing at an older revision with a `restore` change row, so Undo on any receipt and Restore in the reader are the same idempotent operation (per `make-operations-idempotent`).

**Lens→World.** `knowledge_lens_subjects(record_id, node_id)` is derived from the page's `about` at commit; Rust validates each node id against the current snapshot or the turn's acknowledged World reads. `knowledge_lens_applications` keeps its meaning (used in a turn). Links in bodies stay `lens:rec_` and gain an optional relation in the Markdown link title (`[t](lens:rec_x "contradicts")`); `knowledge_lens_links.relation` is derived from the body, so there is one source (per `encode-lessons-in-structure`).

**World vocabulary.** `Schema` in `world_extract/ontology.rs` is the only list; `world_schema.rs` derives CORE_TYPES and CORE_RELATIONS from it (hand-synced list removed). Core gains `Technology` (a process, material science, standard or technique that is not one vendor's product) and keeps `Entity` as the open fallback, which with Product, Place, Policy and Metric is enough for value chains in the ETO shape (stage → input technology → provider). `Asset.asset_class` takes its vocabulary from `AssetClass::ALL` so extraction and asset pages agree. The extension becomes `knowledge_world_extensions(loupist_id, version, extension_json, origin, applied_at_ms)`, append-only; current is max version; undo appends. `knowledge_schema_proposals` is deleted: a validated proposal is a version, a failed one a run-log entry.

**Background reviews.** `knowledge_learning_reviews` gains `kind IN ('method','connection')`. Both share the admission rules, the one-call budget and the tool-free path. Method reviews now take subject pages revised since the last review as input beside claims, and their trigger is five claims or two subject revisions. Connection reviews take a deterministic `ConnectionCandidate` (a path of valid claims from a followed entity to a node two or three hops away that no subject page is yet about, scored by rarity and recency) and write one `hypothesis` page with status `open`. The path finder is pure code and tested without a model.

**Lint.** `knowledge_lens_lint(record_id, finding, detected_at_ms)` written by a no-model job on the briefing schedule: orphan page, one-way link, uncited subject sentence block, about-entity removed, stale (new claims about its entities since its last revision). Findings surface in the Pages tab and as ≤3 pointers in a new envelope block 6i so the next relevant turn refreshes the page.

**Bot posts.** `HomePost` gains `learned` (from a session turn) and `vocabulary` (an applied extension version with Undo); `delivery` gains the same `LearnedSummary`. The summary is built from `memory_update` and growth events of the committing message only, never holdings.

### Vocabulary changes: why auto-apply

The research notes record a real tension: pass B says do not auto-promote induced schema; pass A and the agent-era requirement say people do not configure, agents do. This candidate auto-applies, for four reasons grounded in the code. First, the pending-approval shape has already failed in this product: proposals wait in `LoupistTracking.tsx` for a user who, by the task's own statement, does not know what an ontology is (grounding problem 4). An approval nobody can give is a silent "never". Second, the blast radius of a wrong extension is small by construction: it is bounded (≤4 types, ≤6 relations, ≤3 fields), it applies only to Sources processed from then on, extraction never reclassifies existing nodes, and an entity that fits no type stays `Entity`, so a bad type at worst collects a few new nodes under a name the user can undo. Third, the host validates what the model cannot be trusted with (protected names, duplicates of core, bounds, label shape), which is the part of pass B's objection that matters: extraction quality is guarded by the existing template seed check, not by a human reading type names. Fourth, versioned apply-with-undo is a strictly smaller surface than proposals plus a review page: one append-only table, one receipt line, one undo.

What keeps it honest: every applied version is a `schema_extended` growth event with a before/after summary, shown in the receipt, in the Bot's `vocabulary` post and in Map What's new, each with Undo; the Map labels the new kind in its category immediately, so the user sees the change where it has effect. A version that no claim used in thirty days becomes a lint finding ("unused kind"), so dead vocabulary is surfaced rather than accumulated. If the operator wants a gate, the one place to add it is `apply_extension_version`'s `origin == ModelReview` branch, and nothing else changes.

### Flow

A turn: envelope (conversation role, vocabulary, subject pointers, lint pointers) → model reads → `lens_update` stages a `LensPage` after authority checks (kind rules, title reservation, `about` validation, link validation) → commit writes revision, change row with derived tier, subjects, links, growth event → `graph.wake()` → if not the Bot, one `learned` post. Background: `pump_once` runs schema (now auto-apply), method and connection reviews, lint, then World jobs.

### Interface depth

Public surface added: one tool argument shape change (`lens_update.page` replaces `document` + `kind`), two tools (`session_open`, `world_schema_update`), one envelope block and two fields, one bridge command (`lens_restore_revision`), two Bot post kinds. Behind it: typed pages with history and rollback, derived provenance tiers, entity-identity pointers, auto-applied reversible vocabulary, deterministic connection search and lint. Removed: `knowledge_standing_selection`, `knowledge_schema_proposals` and the LoupistTracking approval UI, the payload `lensKind`, the duplicated core lists, the note-matching lexical pointer for subjects. The caller learns one page model and one undo.

### What it deliberately does not do

The model never writes World claims; World still grows only from retained Sources (fixed decision), so an injected instruction can at most become an uncited, `loupist_inferred` Lens page, which is context, never evidence, and the existing `sensitive_memory` and shared-delimiter refusals apply to page bodies. No GNN, no community summaries, no global map-reduce. No ontology terms in product copy: the UI says "kinds of names and links" and "vocabulary".

## Tradeoffs accepted

- We accept that `lens_update`'s argument shape changes (a `page` object with a kind tag) in exchange for one write tool and one page model instead of a fifth tool per kind.
- We accept keeping `note_save` for in-chat progress and user-stated facts in exchange for not rewriting the Journal, which hangs off research records; conclusions move to subject pages by prompt rule, and the eval shows the split.
- We accept that connection hypotheses are `loupist_inferred` and start as "open" with a falsifier the model writes, in exchange for a discovery feed that costs one bounded helper call per candidate and no training data.
- We accept auto-applied vocabulary with undo (this candidate's choice, argued above) in exchange for an append-only extension history, which also gives extraction a stable version id per job and makes the gate, if one is ever wanted, a one-branch change.
- We accept that `session_open` with `start: true` spends one model turn the user did not type, because the user asked the Bot for the work; the Bot says it is running and the session is visible in the sidebar.

## Alternatives considered

- **Subject pages as a new record kind (`knowledge_records.kind = 'page'`) with its own tool `page_update`.** Rejected: a second write path beside `lens_update`, a second catalog, a second receipt kind, and shared packs would need a second exporter. The tagged `LensBody` hides the difference behind one tool.
- **Let the model write World claims directly from research (a `claim_save` tool).** Rejected: it breaks the fixed decision that World is extracted from retained Sources, and it is the poisoning path the research warns about; a claim with no span cannot be shown on the Map's evidence pane.
- **Retire the working note and make the subject page the per-chat note.** Rejected: the Journal's `research_id` foreign key and 6g continuity would move too, and a subject is not a chat; this is a later cleanup once subject pages carry the load.
- **Keep proposals pending for the user (current shape).** Rejected: ordinary users cannot judge them, so the gate is a silent never (grounding problem 4). Versioned auto-apply with undo is strictly smaller: one table instead of proposals plus a settings UI.
- **Apply provisionally: a new kind extracts but is hidden from the Map until N claims use it.** Rejected: it hides the change exactly where the user would notice and undo it, and it adds a second state to every Map and inspector path. Visibility is the safeguard, not a delay.
- **Vector search over Lens pages for 6e.** Deferred: entity identity through `about` is exact and free; FTS already exists in `knowledge_search_documents`.

## Open questions and risks

- Should a `hypothesis` page expire (drop to `dropped`) when no turn reads it in N days, or stay until the user archives it?
- Is one `learned` post per committed session turn too noisy for a long session? A per-session coalescing window (post once per hour per session) is a one-line change if so.
- The biggest risk is model compliance with the file-back rule: a prompt rule plus a headless eval is the only enforcement, since Rust cannot know a turn was "substantive research". The eval `L-010` and `L-011` are the guard; if compliance is low the fallback is a host nudge (a tool result after the third exact read that says the subject page exists and expects an update).
- `about` validation against the current snapshot races with a World publication in the same minute; the commit path resolves node ids against the head at commit time, and an id that vanished becomes a lint finding, not a failure.

## Next implementation step

Bump `KNOWLEDGE_SCHEMA_VERSION` to 6 and land `LensBody`, the revision history and `knowledge_lens_subjects` with the Rust write-path tests, since every other unit reads that shape.
