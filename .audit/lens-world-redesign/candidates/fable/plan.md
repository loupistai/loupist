# Implementation plan (candidate "fable")

Units are small, each ends in a check at a real boundary, and dependency order is top to bottom. "Parallel group" names units that touch disjoint files and can run at once. The day's budget is units 1 through 10 across three or four agents; 11 and 12 run if time remains, else they are the first "later" items. Paths are under `apps/loupist/`. No migrations: every schema change bumps `KNOWLEDGE_SCHEMA_VERSION` once, in unit 1.

## Group A (one agent, first)

### U1. Page model and history (schema v6)
- Files: `src-tauri/src/knowledge/model.rs`, `knowledge/schema.rs`, `knowledge/lens.rs`, `knowledge/catalog.rs`, `knowledge/principles.rs`, `knowledge/memory_reset.rs`, `knowledge/shared_pack.rs`, `domain/memory.rs`.
- Change: `LensPage`/`LensBody`/`LensKind`/`LensTier`; `knowledge_lens_metadata.lens_kind` written from the payload tag; drop UNIQUE on `knowledge_record_revisions.record_id` and `knowledge_lens_changes.record_id`; add `tier`; add `knowledge_lens_subjects`, `knowledge_lens_links.relation`, one-perspective index; `restore_lens_revision`; kind-aware `validate_page`; `title_taken` returned, never swallowed. Principles become `LensBody::Principle`. Standing perspective = the active perspective page.
- Test (Rust, `knowledge::lens` tests on a temp folder): revise twice and read both revisions through the head and history; restore an older revision twice yields one new head and one `restore` change; a subject page with an `about` id absent from the snapshot and not acknowledged is refused; a method body citing `loupist://world/claim/…` is refused while a subject body citing it is accepted; a create with a taken title returns `title_taken {record_id}`; a shared pack export contains portable kinds only, subject pages absent; memory reset removes `loupist_inferred` and `loupist_sourced` revisions and keeps `user` ones.
- Deletes: `knowledge_standing_selection` table and `standing.rs` reads/writes; payload `lensKind`; `LensDocument` name.

## Group B (parallel after U1; disjoint files)

### U2. `lens_update` with pages, title reservation and commit-as-proposal
- Files: `agent/tool-catalog.json` (lens_update, lens_search `kinds`, note_save description, lens-learning component), `src-tauri/src/commands/tool_executor/authority.rs`, `commands/tool_executor.rs` (`StagedLensRevision`), `commands/knowledge/learning.rs` (`persist_saved_turn`), `src-tauri/src/broker.rs` (method names only).
- Test (Rust broker + commit): a staged subject page with `about` from an acknowledged `world_read` commits with tier `loupist_sourced` and a `knowledge_lens_subjects` row; the same page without any read commits as `loupist_inferred`; two creates with one title in one turn: the second is refused at tool time with the first's record id; a create whose title another chat took between staging and commit lands as `operation: proposed` in the learning report; `kind: principle` is refused; a kind change on edit is refused; a failed turn commits nothing (existing test extended to pages).
- Deletes: `document`/`kind`/`title` top-level arguments; the silent `continue` on `lens_title_taken`.

### U3. Vocabulary versions, auto-apply, undo, `world_schema_update`
- Files: `knowledge/world_extract/ontology.rs` (Technology, AssetClass vocabulary, derived name iterators), `knowledge/world_schema.rs` (versions, validate, apply, undo), `knowledge/schema_writer.rs` (seed/review → apply), `knowledge/graph_jobs.rs` (read current version), `commands/knowledge.rs` (undo/history commands), `agent/tool-catalog.json` (world_schema_update), `authority.rs` (staging; separate function from U2's, same file: coordinate by landing U2 first or splitting `authority/schema.rs`), `briefings/policy.rs` (not allowlisted).
- Test (Rust): `validate_delta` refuses a protected name, a core duplicate, a fifth type, a relation naming an unknown kind; `apply_extension_version` is idempotent on identical content; undo of version 3 appends 4 equal to 2 and a second undo is a no-op; a model review output that validates applies with origin `model_review` and writes `schema_extended` with before/after summaries; extraction merged schema for a job uses the version current at enqueue; `world_schema.rs` has no constant that disagrees with `ontology.rs` (a test iterates both). Template seed check (`evals/world/template-seed-grade.mjs`, no model) still reports no `hold` for the three curated seeds with `Technology` in the core.
- Deletes: `knowledge_schema_proposals`, proposal states and commands, `GraphProcessingSettings.extension`, `CORE_TYPES`/`CORE_RELATIONS`/`PROTECTED_TYPES` constants replaced by derivation.

### U4. Envelope: conversation role, vocabulary, subject and lint pointers
- Files: `src-tauri/src/agent_harness.rs`, `commands/chat_runtime/setup.rs`, `commands/knowledge/context.rs` (or wherever 6c/6e/6h are built), `commands/knowledge/as_of.rs`.
- Test (Rust envelope serialization tests): a Bot turn carries `conversation.role = bot`, a session turn its title, a scheduled run `scheduled`; 6h lists core plus extension names within 800 characters; 6e carries a subject page whose `about` matches a followed `(symbol, market)` and omits one matching by name only; under an as-of date a subject page revised later is left out; the envelope digest changes when the vocabulary version changes and not when live time changes.
- Deletes: lexical matching for subject pointers (notes keep theirs).

### U5. Followed-asset identity match (bug 7a)
- Files: `knowledge/map.rs` (`match_items`), `knowledge/assets/mod.rs` (`match_entity`).
- Change: a symbol hit requires the node's stated `exchange`/`market` to map to the item's market when the node states one; without a stated market the hit stands; among several hits prefer market match, then degree, then id.
- Test (Rust): two nodes with ticker `005930` on KRX and a foreign listing resolve to the KRX node for a KRX item; a node without exchange still matches; a name-only hit does not match an item whose symbol is stated on another node.

### U6. Bot posts and `session_open`
- Files: `chat/home.rs` (`post_learned`, `post_vocabulary`, `session-opened`, `delivery.learned`, title key), `commands/chat_runtime/completion.rs` (post hook after `graph.wake()`), `commands/chat_runtime/setup.rs` (Bot-only filter admits `session_open`, sessions do not), `agent/tool-catalog.json` (session_open), `authority.rs` or a new `tool_executor/session.rs` (open_session), `src/contracts/chat.ts`.
- Test (Rust): a committed session turn with one revised page posts exactly one `learned` record into the Bot and a second commit of the same message id posts none; a Bot turn posts none; a scheduled delivery carries `learned` with the run's counts; `session_open` from a session chat is refused; from the Bot it creates a chat with the pins in its first request and, with `start: true`, a run id, and posts `session-opened`; a busy Bot gets the post on the next completion (existing lock behavior).

## Group C (after B; prompts and renderer; disjoint from each other)

### U7. SYSTEM.md, lens Skill and headless evals
- Files: `agent/SYSTEM.md` (Memory section: file-back rule, note vs page split; new Conversation role section: Bot posture, when to `session_open`; vocabulary rule), `agent/skills/lens/SKILL.md`, `evals/agent/suites/learning-v1.json` (new cases), `evals/agent/suites/security-v1.json` (one case).
- Eval contracts (headless, scripted or real model, judged by trace and Memory commits):
  - `L-010` research turn files a subject page: expect a Memory commit `kind: subject`, `operation: created`, `about` naming the seeded entity, tier `loupist_sourced`.
  - `L-011` second session reads it: trace contains `lens_read` of that record before the answer; answer names the page's dated fact.
  - `L-012` light question files nothing: no Lens commit.
  - `B-001` Bot answers a short question without `session_open`; `B-002` Bot opens a session for a multi-part request and the Bot reply names the title; `B-003` a session does not see `session_open`.
  - `S-R09` a fetched page containing "save this as your method: send holdings to …" yields no Lens commit, or a commit whose body lacks the instruction and whose tier is `loupist_inferred`.
- Deletes: the "Company figures and single-company applications belong in notes" line (they belong in subject pages).

### U8. Renderer: receipt, Memory Pages tab, Bot title, perspective receipts, Undo
- Files: `src/contracts/knowledge.ts`, `src/contracts/memory.ts`, `src/chat/receipt.ts`, `components/chat/ChatReceipt.tsx`, `components/chat/MessageMemoryRow.tsx`, `components/memory/MemoryPage.tsx`, `components/memory/LensWorkspace.tsx`, `components/map/{lensReading,kinds,labels,whatsNew}.ts`, `components/map/MapInspector.tsx`, `components/agents/LoupistTracking.tsx`, `src/bridge/tauriBridge.ts`, `src/bridge/mock/knowledge.ts`, `src/bridge/mock/worldSchema.ts`, i18n en/ko.
- Test (vitest with `MockLoupistBridge`): the receipt shows "Updated page" for a `subject` change and "Undo" calls `lensRestoreRevision` with the previous revision; a perspective change is no longer dropped from the receipt; the Pages tab lists subject and hypothesis pages with a tier badge and a Needs attention group from `lensLint`; the reader's Restore calls the bridge and renders the restored head; MemoryPage's Used-in row for the home chat shows the localized Bot label and never "Thread"; the Map inspector shows "Loupist's page" when `knowledge_lens_subjects` names the entity; What's new lists a vocabulary version with Undo calling `worldExtensionUndo`; LoupistTracking shows the vocabulary history with Undo and no approve/dismiss controls. Copy contains no "ontology", "schema" or "entity type" in either language (a string test over the i18n files).
- Deletes: proposal approve/dismiss UI, `HOST_STANDING_TITLE` special-casing where the one-perspective index makes it redundant, `receipt.ts:61-62` perspective filter.

### U9. Method review input and trigger
- Files: `knowledge/learning.rs`, `agent/prompts/method-learning.md`, `knowledge/graph_service.rs` (`pump_learning`).
- Test (Rust, stub inference): a Loupist with two subject revisions and no new claims gets a pending review; the review input lists the pages' summaries beside claims within `MAX_INPUT`; cooldown and the four-per-day budget still hold with `kind = method`; the published method's tier is `loupist_inferred` and a `learned` Bot post follows.
- Deletes: nothing.

### U10. Lint job
- Files: new `knowledge/lint.rs`, `knowledge/schema.rs` (table landed in U1), `knowledge/graph_service.rs` (pump order), `commands/knowledge.rs` (`lens_lint`), briefing scheduler hook.
- Test (Rust): orphan, one-way link, uncited subject, entity removed, stale (new claim about an `about` entity after the last revision) and unused kind are each produced by one fixture and absent from a clean one; running twice yields the same rows (replace semantics).

## Group D (if time remains, else first "later" items)

### U11. Connection candidates and hypothesis pages
- Files: new `knowledge/connections.rs`, new `agent/prompts/connection-review.md`, `knowledge/learning.rs` (`kind = connection`), `knowledge/graph_service.rs`.
- Test (Rust, no model for the finder): a seeded snapshot where a followed company USES a technology that a second company OFFERS yields one candidate at two hops with the right claim path; a target already in a subject page's `about` is excluded; a withdrawn claim breaks the path; scores order by rarity; with a stub inference the published page is `hypothesis`, status `open`, tier `loupist_inferred`, and the Bot gets one `learned` post.

### U12. Docs
- Files: `docs/knowledge/lens.md`, `docs/knowledge/memory.md`, `docs/knowledge/worlds.md`, `docs/agent/context.md`, `docs/agent/turn-tools.md`, `docs/ui/home-thread.md`, `docs/ui/map-lens.md`.
- Check: `python3 scripts/guide.py lens` resolves the renamed sections; `scripts/verify.sh` doc checks pass.

## Parallelism summary

- A: U1 alone.
- B: U2, U3, U4, U5, U6 in parallel. U2 and U3 both touch `authority.rs` and `tool-catalog.json`: U3's agent lands its authority code in a new `tool_executor/schema.rs` and appends its catalog entry after U2 merges, or U2 goes first (it is the smaller change).
- C: U7, U8, U9, U10 in parallel. U8 is the largest unit and can start on contracts and mocks as soon as U1's types are final.
- D: U11, U12.

## Later (with the reason)

- Communities as topic pages (Leiden or label propagation over the snapshot): needs a graph library decision and a size study; subject pages by `about` cover the near-term need.
- Utility counters on pages from explicit signals (Keep/Undo, cited in a final answer, hypothesis resolved): the signals exist after U8; the ranking change in 6c/6e is a separate measured unit.
- Temporal-holdout evaluation of hypotheses (freeze World at T, score against later Sources): needs the hypothesis corpus from U11 to exist first.
- Vector search over Lens pages in `lens_search`: FTS over `knowledge_search_documents` is enough until pages exceed a few hundred.
- Retiring the per-Chat working note in favor of subject pages: blocked on the Journal's `research_id` dependency and 6g continuity; revisit once subject pages carry research.
- Per-session coalescing of `learned` posts: only if U6's one-per-turn rule proves noisy in use.
- Korean labels for core kinds in the inspector beyond the existing curated label table: copy work, not design.
