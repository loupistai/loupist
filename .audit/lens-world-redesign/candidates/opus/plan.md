# Implementation plan (candidate: opus)

Goal: the Loupist grows Lens (itemized wiki pages anchored to the Map) and World (a
broader core plus evidence-gated tracking) as a byproduct of chat turns, scheduled
runs and delegated research, with every growth step in an append-only record a test
can read; the Bot knows it is the Bot, delegates depth and shows what was learned.

Paths are under `apps/loupist/` unless they start with `docs/`. Rust tests run with
`cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml <filter>`; renderer tests
with `npm --prefix apps/loupist exec vitest run <path>`; eval contracts with
`node --test apps/loupist/evals/**/*.test.mjs`. Each unit ends with
`sh apps/loupist/scripts/check.sh <rust|web>` for what it touched.

## Tracks and order

Four agents, one day. The long pole is track A (units 4 to 6, then 11).

| Track | Agent | Units in order |
| --- | --- | --- |
| A, Lens core | strongest model | 4 → 5a → 6 → 11 |
| B, World | model 2 | 1 → 3 → 9 → 8 (8 waits for 5a) |
| C, Bot and filing | model 3 | 10 → 7 (7 waits for 6) |
| D, Renderer | model 4 | 2 → 5b (after 5a's commands) → 12 → 13 → 14 |

Parallel without shared files: units 1, 2, 3 and 10 can start at once; 5b, 8, 9 and
7's module run beside each other once 5a lands.

Shared files edited by several units: `agent/tool-catalog.json` (4, 6, 9, 10, 11),
`src-tauri/src/broker.rs` (4, 6, 10, 11), `agent/SYSTEM.md` (6, 10, 11),
`src-tauri/src/agent_harness.rs` (9, 10, 11), `src-tauri/src/knowledge/graph_service.rs`
(4, 7, 8, 9). Each unit edits only its own entries or functions; land in unit number
order and rebase. `src-tauri/src/knowledge/schema.rs` is edited only by unit 5a, which
adds every version-6 table, so later units never touch it.

## Units

### 1. Followed-asset matching uses the market (bug)

- Files: `src-tauri/src/knowledge/map.rs` (`match_items`), `src-tauri/src/knowledge/assets/mod.rs` (`match_entity`).
- Change: index node identifiers by `(symbol, exchange→market)` when the node states an
  exchange; a followed item matches the node whose market agrees. With several hits and
  no market to decide, return no match instead of the smallest node id.
- Test (Rust, `knowledge::map` and `knowledge::assets` tests): two Organization nodes with
  ticker `A`, one on KRX and one on NASDAQ; followed `(A, KRX)` matches only the KRX node;
  two hits without exchange data leave the item unmatched; `match_entity` returns `None`
  on ambiguity.
- Deletes: nothing.

### 2. Renderer fixes: Bot title and perspective receipts (bug)

- Files: `src/components/memory/MemoryPage.tsx` (`UsedIn`), `src/chat/receipt.ts`.
- Change: a home thread renders the localized Bot label instead of its stored title
  "Thread"; perspective deliveries count in the receipt's used list instead of being
  filtered out at `receipt.ts:61-62`.
- Test (vitest): `MemoryPage.test.tsx` renders Used in for a `home: true` thread and
  finds the Bot label, not "Thread"; `ChatReceipt.test.tsx` with a perspective
  `lens_receipts` entry shows it as used.
- Deletes: nothing.

### 3. World core: technology, industries, real assets; one asset-class list

- Files: `src-tauri/src/knowledge/world_extract/ontology.rs`, `src-tauri/src/knowledge/world_schema.rs`
  (core lists and `curated_seed` only), `src-tauri/src/knowledge/assets/class.rs`
  (`AssetClass::ALL`), `src-tauri/src/knowledge/world_extract/golden/*`,
  `src/components/map/kinds.ts`, `src/components/map/labels.ts`, i18n files.
- Change: add `Technology`, `Segment`, `RealAsset`; add `PART_OF`, `OWNS`,
  `OPERATES_IN`; widen `USES`; narrow `Place` and `Product` text; render `asset_class`
  from `AssetClass::ALL` and add `instrument`; derive `world_schema.rs` core lists from
  `ontology.rs`; key curated seeds by template and add crypto, real-asset and commodity
  seeds; Map categories Technology and Industry.
- Test: Rust ontology test asserts the Asset prompt names every `AssetClass` value and
  no other; `validate_extension` rejects `Technology` as an extension type (now core);
  `curated_seed(LoupistTemplate::RealAssets, _)` is `Some` regardless of focus text;
  golden extraction fixtures updated; `node apps/loupist/evals/world/template-seed-grade.mjs`
  (no model) reports no `hold` for any seed; vitest `kinds.test.ts` maps `Technology` and
  `Segment` to their categories.
- Deletes: hand-copied `CORE_TYPES` and `CORE_RELATIONS`, the focus-equality seed gate
  (`SEMICONDUCTOR_FOCUS`, `DIVIDEND_FOCUS`, `MACRO_FOCUS`).

### 4. Subtract the paths the page model replaces

- Files: `agent/tool-catalog.json` (`lens_update`), `src-tauri/src/broker.rs`
  (`LensUpdate`), `src-tauri/src/commands/tool_executor/authority.rs` (`stage_lens_update`),
  `src-tauri/src/commands/tool_executor.rs` (`StagedLensRevision`, `staged_lenses`),
  `src-tauri/src/commands/knowledge/learning.rs` (Lens revision block of `persist_saved_turn`),
  `src-tauri/src/knowledge/lens.rs` (drafts and proposals), `src-tauri/src/knowledge/learning.rs`
  (the review queue: `enqueue`, `claim`, `finish`, `publish`, `build_input`; `settings` and
  `save_settings` stay until unit 7 moves them), `agent/prompts/method-learning.md`,
  `src-tauri/src/knowledge/graph_service.rs` (`pump_learning`),
  `src-tauri/src/commands/knowledge/method_learning.rs` (review list; the settings commands
  stay), `src-tauri/src/eval_harness/growth.rs` (method-review steps), bridge entries
  `knowledgeLensProposals` and `knowledgeLensProposalDismiss`, the proposal and review-list
  UI in `LensWorkspace.tsx` and `LoupistLearningSettings.tsx`, their dedicated tests.
- Change: nothing new; the build and remaining tests stay green. Lens methods are
  temporarily write-only through sharing until unit 6 (planned breakage on an unreleased app).
- Test: Rust and web gates pass; the catalog contract test asserts `lens_update` is absent;
  `knowledge::lens` tests for read, rename and archive still pass.
- Deletes: everything listed. Run the [cleanup checks](../../../../docs/dev/repository-guidance.md#code-cleanup) first.

### 5a. Page model and schema version 6 (Rust)

- Files: new `src-tauri/src/knowledge/lens_page.rs`; `src-tauri/src/knowledge/schema.rs`
  (version 6: metadata `kind`, `knowledge_lens_log`, `knowledge_lens_anchors`,
  `knowledge_compile_jobs`, `knowledge_connections`, `knowledge_schema_changes`,
  `chat_handoffs`, growth kinds; drop `knowledge_lens_changes`, `knowledge_lens_applications`,
  `knowledge_lens_drafts`, `knowledge_standing_selection`, `knowledge_learning_reviews`,
  `knowledge_schema_proposals`); `src-tauri/src/knowledge/model.rs` (`LensDocument` → `LensBody`);
  `src-tauri/src/knowledge/lens.rs` (`read_page`, `file`, `undo_op`, anchors, links);
  `knowledge/principles.rs`, `knowledge/catalog.rs`, `knowledge/research.rs` (standing
  selection), `knowledge/shared_pack.rs` (methods, principles and Outlook as pages),
  `knowledge/memory_reset.rs`, `knowledge/map.rs` (Lens projection from anchors),
  `commands/knowledge/chat_reads.rs` (`lens_read` output with items), `domain/memory.rs`
  (`LensReceipt` kinds), `commands/knowledge/principles.rs` (`persist_citations` writes
  `applied` anchors).
- Test (Rust): `lens_page` unit tests cover every `OpOutcome` (section refused for the
  kind, evidence rule refused, already known, forgotten refused, full section, stale
  revise, idea status on a non-idea); `lens::file` tests: create by an existing
  `(kind, title)` merges and logs both ops, subject create with an existing primary anchor
  merges, a second active perspective is refused by the index, one log row per op
  including refused ones, `undo_op` restores a retired item and refuses after a later edit;
  as-of read hides an item revised after the cutoff; shared-pack export and import of a
  method page round-trips; Memory reset still removes only Loupist-authored pages.
- Deletes: `LensDocument`, payload `$.lensKind`, `select_standing_lens` and
  `load_standing_selection`, `list_lens_changes`, `knowledge_lens_applications` writers.

### 5b. Page contracts and reader (renderer)

- Files: new `src/contracts/lens.ts`; `src/contracts/knowledge.ts`, `src/contracts/memory.ts`
  (drop `LensDocumentBody` and `lensKind`), `src/bridge/tauriBridge.ts`
  (`knowledgePageRead` replaces `knowledgeLensRead`; `knowledgeLensChanges` removed),
  `src/bridge/mock/knowledge.ts`, new `src/components/memory/PageReader.tsx`,
  `src/components/knowledge/LensWorkspace.tsx` (reader uses `PageReader`),
  `src/components/map/lensReading.ts`.
- Test (vitest, mock bridge): `PageReader` renders a subject page's sections in
  `PageKind` order, a source chip for a span basis, "you said" for a user basis, "Loupist's
  view" for a view with item basis, and a contested marker; a principle still renders
  statement, why and example.
- Deletes: six-field section rendering and View Markdown's per-field headings.

### 6. `lens_file`: filing in the turn

- Files: `agent/tool-catalog.json` (`lens_file`; `principle_save` moves into `memory-save`;
  `lens-learning` removed), `src-tauri/src/broker.rs` (`LensFile`, cardinality),
  new `src-tauri/src/commands/tool_executor/lens_file.rs`, `src-tauri/src/commands/tool_executor.rs`
  (`staged_filing`), `src-tauri/src/commands/knowledge/learning.rs` (commit through
  `KnowledgeWrite::file` with `FilingOrigin::Turn`), `src-tauri/src/commands/chat_runtime/completion.rs`
  (pass staged filing), `src-tauri/src/briefings/policy.rs`, `agent/SYSTEM.md` (filing
  rules in the Memory section), `agent/skills/lens/SKILL.md`.
- Test (Rust, at the broker and commit path with scripted Pi): a turn that `world_read`s
  and files a fact with `{world, claimId}` commits a page item with `Basis::Claim` and a
  log row carrying the assistant message id; the same turn stopped before completion leaves
  no page and no log row; a fact without basis returns a tool refusal naming `basis` with a
  repair; `revise` without an acknowledged `lens_read` names `readRef`; a create whose title
  exists merges; in an unattended run `said` is refused and `lens_file` is admitted; a third
  call returns `BudgetExceeded`; credential-like text returns `sensitive_memory`.
- Deletes: the `lens-learning` component and its capability-index text.

### 7. Compile job: background filing

- Files: new `src-tauri/src/knowledge/compile.rs`, new `agent/prompts/compile.md`,
  `src-tauri/src/knowledge/graph_service.rs` (compile slot where `pump_learning` was),
  `src-tauri/src/commands/chat_runtime/completion.rs` (one `compile::enqueue` call inside
  the persist closure), `src-tauri/src/knowledge/learning.rs` (remaining `settings` and
  `save_settings` move into `compile.rs` as `learn_from_research`; the file is deleted),
  `src-tauri/src/commands/knowledge/method_learning.rs` (renamed for the setting),
  `src/components/agents/LoupistLearningSettings.tsx` (one toggle).
- Test (Rust with the scripted helper inference used by the World job tests): a qualifying
  committed turn enqueues one job and a second turn in that Chat joins it; a short or
  evidence-free turn enqueues none; the pump sends one request whose input holds `e1`
  handles and no record ids; a reply citing `e1` lands items with origin `compile` and
  `compile_job_id`; a reply citing an unknown handle marks the job `refused` and files
  nothing; an uncertain reply marks it `uncertain` and it is never claimed again; a reply
  re-adding text the user forgot yields `refused_forgotten`; learning off yields no job;
  a `memory-learned` event carries the message ids.
- Deletes: `knowledge/learning.rs` and the `learn_methods` name.

### 8. After every World publication: contested items, anchors, links

- Files: new `src-tauri/src/knowledge/world_followups.rs`, new `src-tauri/src/knowledge/connections.rs`,
  `src-tauri/src/knowledge/world.rs` (call beside `growth::world_diff`),
  `src-tauri/src/knowledge/graph_service.rs` (pass followed identities into the seal context),
  `src-tauri/src/chat/home.rs` (`link` host post, at most three a day),
  `src-tauri/src/commands/knowledge/memory_items.rs` (`knowledge_connections`,
  `knowledge_connection_dismiss`).
- Test (Rust): sealing a snapshot that withdraws a claim cited by an item marks the item
  contested and writes one `item_contested` event; a pending anchor resolves when a later
  snapshot adds the name; detector fixture with followed A, prior A–B, new B–C across
  categories yields one candidate A→C via B; a middle node above the hub degree yields none;
  sealing the same snapshot twice inserts no second row; followed `(A, KRX)` seeds only the
  KRX node (uses unit 1); the fourth candidate of a day is stored but not posted.
- Deletes: nothing.

### 9. Tracking changes apply at once, gated by evidence, with undo

- Files: `src-tauri/src/knowledge/world_schema.rs` (`apply_change`, `undo_change`,
  `evidence_gate`), `src-tauri/src/knowledge/schema_writer.rs` (review only),
  `src-tauri/src/knowledge/graph_service.rs` (`pump_schema` applies through the gate),
  new `src-tauri/src/commands/tool_executor/world_schema.rs`, `agent/tool-catalog.json`
  (`world_schema_update`), `src-tauri/src/agent_harness.rs` (`worldKinds`),
  `src-tauri/src/commands/knowledge/world_schema.rs` (`world_schema_changes`,
  `world_schema_undo`), `src/bridge/tauriBridge.ts`, `src/components/agents/LoupistTracking.tsx`.
- Test (Rust): a review proposing a relation with seven claims across three Sources under
  that open name applies, writes a change row with evidence and a `schema_extended` event;
  the same with two claims is refused by the gate and changes nothing; `world_schema_update`
  in a completed turn applies with the origin message id, and a stopped turn applies
  nothing; undo restores the previous extension, and undo of an older change after a newer
  one removes only its own kinds; creating a Loupist from the crypto template writes one
  `template` change. vitest: "What the map tracks" lists a change with Undo and calls
  `worldSchemaUndo` on the mock bridge.
- Deletes: the model-written custom-focus seed (seed request, `settle_model_proposal`
  seed branch), proposal apply and dismiss commands and bridge entries, the Apply/Dismiss UI,
  `evals/world/seed-grade.mjs`, `seed-corpus-v2.json`, `seed-oracle-v2.json` and their test.

### 10. `ChatSurface`, `research_open` and delegated runs

- Files: new `src-tauri/src/chat/surface.rs`, new `src-tauri/src/chat/handoff.rs`,
  `src-tauri/src/commands/chat_runtime/setup.rs` (tool filter), `src-tauri/src/commands/chat_runtime.rs`
  (after-commit hook), new `src-tauri/src/commands/tool_executor/research_open.rs`,
  `agent/tool-catalog.json` (`research_open`), `src-tauri/src/broker.rs` (`ResearchOpen`),
  `src-tauri/src/agent_harness.rs` (`chat_surface`), `src-tauri/src/scheduled_tasks/types.rs`
  (`TaskKind::Handoff`, excluded from the task list), `src-tauri/src/chat/home.rs`
  (`handoff` record), `agent/SYSTEM.md` (Bot and research sessions), new
  `src/components/chat/ThreadHandoff.tsx`, `src/components/chat/MessageSchedules.tsx`
  (surface flag instead of the id prefix check).
- Test (Rust): a Bot turn calling `research_open` commits one `chat_handoffs` row;
  `launch_pending` creates the session `handoff-…` with the question as its first message and
  starts one run with the unattended policy; calling it again creates no second session or
  run; a session turn and a scheduled run do not list `research_open`; the envelope carries
  `chat_surface` for Bot, session and Memory-off turns; a finished hand-off run posts one
  `handoff` record into the Bot; a stopped Bot turn creates no hand-off. vitest:
  `ThreadHandoff` shows the title, status and Open.
- Deletes: the `is_bot` filter in `setup.rs:62-76` and the Bot-only hook in
  `chat_runtime.rs:511-517` (both move into `ChatSurface`).

### 11. Delete working notes; index, anchored pointers, chat pages

- Files: `agent/tool-catalog.json` (`note_save` removed; `conversation_memory` loses note
  reads), `src-tauri/src/broker.rs` (`NoteSave`), `src-tauri/src/commands/tool_executor/research.rs`
  (working-note staging), `src-tauri/src/knowledge/research.rs` (working-note functions),
  `src-tauri/src/commands/knowledge/working_notes*`, `src-tauri/src/commands/knowledge/chat_reads.rs`
  (`relevant_memory` by anchors), `src-tauri/src/agent_harness.rs` (6c `index`, 6g `chatPages`),
  `src-tauri/src/commands/chat_runtime/launch.rs`, `src-tauri/src/commands/knowledge/as_of.rs`,
  `src/components/map/MapLibrary.tsx` (Notes are Journal only), `src/bridge/*`,
  `agent/SYSTEM.md`, `evals/world/memory-grade.mjs`, `evals/agent/taxonomy.mjs`,
  `evals/agent/heldout/tool-use-heldout-v1.json` expectations, `evals/lens/isolate-snapshot.py`.
- Test (Rust, harness envelope tests): the index lists pages about followed names first and
  discloses truncation; a message naming "Hanwha Semitech" yields a pointer to the page
  anchored to that entity; `chatPages` lists pages this Chat filed; under an as-of date the
  index and pointers drop pages and items written later; catalog contract test asserts
  `note_save` is absent; eval contract tests (`suites.test.mjs`, `grade.test.mjs`,
  `memory-grade.test.mjs`) pass with expectations rewritten to `lens_file`.
- Deletes: `note_save`, `WorkingNoteDraft`, the 6g working note, note pointers in 6e,
  `knowledgeWorkingNotes` and `knowledgeWorkingNoteRead`, the Library's working-note rows.

### 12. "What I learned" under every answer

- Files: new `src-tauri/src/commands/knowledge/memory_items.rs` commands (`memory_learned`,
  `memory_item_keep`, `memory_item_forget`, `memory_op_undo`), `src-tauri/src/lib.rs`
  (registration), `src/bridge/tauriBridge.ts`, `src/bridge/mock/knowledge.ts`,
  new `src/components/chat/LearnedCard.tsx`, `src/components/chat/ChatReceipt.tsx`,
  `src/components/chat/MessageMemoryRow.tsx`, `src/chat/receipt.ts`,
  `src/components/chat/ThreadDelivery.tsx` (digest for the run's message), i18n.
- Test (Rust): forget appends a `retire` row with origin user; undo restores it; the digest
  for a message lists its pages and reports `pending` while its compile job is pending.
  vitest (mock bridge): an answer with two filed pages shows "Learned 3 · 1 idea"; Forget
  calls `memoryItemForget` and offers Undo; a `memory-learned` event refreshes the digest;
  a delivery card shows the run's learned count; a private Chat shows no learned line.
- Deletes: the Lens and note parts of `MessageMemoryRow.tsx`, the method-publication
  `LearningToast` path.

### 13. Notebook, Map notes, link cards, morning line

- Files: `src/components/memory/MemoryPage.tsx` (Pages and History tabs from the log),
  `src/components/map/MapInspector.tsx` (Notes), `src/components/map/MapClaimInspector.tsx`
  (Cited in), `src/components/map/MapToolbar.tsx` (Links found filter), new
  `src/components/chat/ThreadLink.tsx`, `src/components/chat/MorningMessage.tsx`
  ("Learned since you last looked" from `lens_filed` growth events), i18n.
- Test (vitest, mock bridge): the Pages tab groups pages as Topics, Ideas, Methods, Your
  principles and Outlook; History shows "Filed after: {chat}" and "Forgot by you" rows; the
  inspector lists pages anchored to the selected entity and opens one; the claim inspector
  lists the pages citing it; the link card's Look into it calls `researchHandoffStart` and
  Not useful calls `knowledgeConnectionDismiss`; the morning message shows the learned line
  only when pages changed since the last visit.
- Deletes: the Methods and Your principles tabs' separate list code (one Pages list).

### 14. Docs and the compounding eval contract

- Files: `docs/knowledge/lens.md`, `docs/knowledge/worlds.md`, `docs/knowledge/memory.md`,
  `docs/agent/context.md`, `docs/agent/context-delivery.md`, `docs/agent/turn-tools.md`,
  `docs/ui/home-thread.md`, `docs/ui/map-lens.md`; new `evals/agent/suites/compounding-v1.json`.
- Change: owning sections describe pages, filing, the compile job, follow-ups, tracking
  changes and the Bot surface; the suite (fictional companies) runs session 1 with fixture
  Sources, then session 2 in a fresh Chat with those Sources withdrawn from tools, and grades
  that session 2 exact-reads a page session 1 filed and answers the hidden facts; the judge
  scores item faithfulness against the fixture Sources.
- Test: `python3 scripts/guide.py lens` resolves the new sections; `suites.test.mjs`
  validates `compounding-v1`; no paid run in this unit.
- Deletes: doc text for working notes, method learning, schema proposals and the
  six-field method.

## Later

- Model lint on a schedule (contradictions between items, summary rewrite from items,
  near-duplicate merge, orphan pages). Waits for compile-job faithfulness data.
- Topic pages from graph communities (label propagation, built lazily). Needs scale the
  current Worlds do not have.
- Background path explanation and Personalized PageRank discovery. Adds a paid call per
  candidate; wait for open and dismiss rates from the deterministic detector.
- Dated checks on idea pages evaluated like Journal checks, with outcomes. Needs a shared
  check model with the Journal.
- Temporal-holdout evaluation of links (freeze World at a date, score against later
  Sources). Needs a dated fixture corpus and a paid run.
- An identity layer (issuer, instrument, listing, venue; FIGI, LEI, DTI). Unit 1 fixes the
  immediate market bug; the layer is a World model change of its own.
- Re-extracting watched names under a new tracking kind. Paid extraction, and undo
  semantics for re-typed nodes.
- Sharing topic and idea pages in shared Loupists. Needs portability rules for map links.
- Ranking retrieval by Keep, Forget and applied counts. Needs the counts to accumulate first.
- A page history browser with page-level rollback (the log already holds it; only item
  undo ships).
