# One-day implementation slice

This is a proposed implementation plan. This candidate run changes only its three design files; no builds, tests, installs or product edits were run.

The slice delivers subject pages with automatic file-back, research-aware bounded upkeep, additive vocabulary changes, a visible evidence-path lead, Bot-to-session work and truthful Memory receipts. It reuses SQLite, GraphSupervisor, exact-read receipts, `send_chat`, the run coordinator, existing readers and graph. It does not introduce a framework, new graph database or migration.

Target: approximately 24–28 engineering hours for four implementers in one working day, including integration reserve, with the existing grounding available. Keep new UI to existing readers, cards and inspectors. The largest estimate uncertainty is the Lens kind/history change across sharing and reset; validate that early. Do not spend the day on a redesigned canvas or a new search stack.

## Dependency and ownership map

```text
1  Canonical contracts and Lens publisher
├─ 2  Completed-work file-back and tool boundary
│  ├─ 4  Research-aware upkeep ──────────────┐
│  └─ 6  Bot research handoff               │
├─ 3  World vocabulary and identity ── 5  Connection paths
└─ 7  Lens/Memory/Map UI                    │
                         4 + 5 ── 8  Connect leads and deliveries
                         all ──── 9  Behavioral/eval gate and cleanup
```

**Actual parallel sets:** freeze and land the shared type/command declarations from `types.md` in the first hour of unit 1. Units 2, 3 and 7 can then be authored alongside the remaining core work in unit 1; their implementation file sets are disjoint, and mocks/fixtures use the agreed contracts. Their acceptance depends on unit 1's publisher, so no unit is declared complete against a stub. After 2, units 4 and 6 can run together while 5 follows 3; those file sets are disjoint. Unit 8 is integration, not parallel with 4/5/6/7. Unit 9 follows the completed slice. An integrator owns shared domain/bridge/catalog declarations; later signature changes go through that owner rather than concurrent edits. The graph shows acceptance dependencies, not a requirement to leave every other implementer idle while the core is written.

A practical allocation is core/integration, tools then upkeep, World then paths then host handoff, and UI then verification. Start the handoff's isolated `chat/handoff.rs` work as soon as contracts are frozen, but sequence its completion/broker/catalog edits after unit 2. This keeps the critical path near one day; native acceptance or failed tests can extend it and must be reported honestly.

Paths below are relative to `apps/loupist/` unless prefixed with `docs/`. Named tests are behavioral cases to add to the indicated existing test surfaces, not tests already run or claimed to exist. Run `python3 scripts/guide.py <topic>` and then its returned `path#heading` before changing each owning section. No commit, push or release is implied.

## 1. Make Lens a versioned wiki with one publisher

**Estimate:** 4 hours, including first-hour shared contract wiring. **Dependencies:** none. **Owner:** Memory core/integrator.

**Files:** `src-tauri/src/knowledge/{model,schema,catalog,lens,principles,search_index,shared_pack,memory_reset}.rs`; directly affected sharing import/export adapters; `src-tauri/src/domain/memory.rs`; `src/contracts/{knowledge,memory,chat}.ts`; bridge contract declarations. Reserve new enum/DTO/tool-name declarations needed by later units here, but do not expose tools with unimplemented handlers. Bring current callers to the typed model, without keeping two persistent representations. `docs/knowledge/lens.md` and `docs/knowledge/memory.md` own the resulting contracts.

**Behavior:** bump Memory schema to 6, one canonical Lens kind, Subject content, named sections, append-only Lens revisions/change records, scoped page keys, exact evidence basis, link parsing, typed deferred changes and compare-and-set undo. Existing method and principle entry points use the same publisher. Non-Lens Source/World history policy stays as specified. Lens history is not automatically admissible under an as-of cutoff.

**Proof:** a Rust transaction test through `KnowledgeWrite` publishes a source-backed subject; reads its Markdown/evidence; applies disjoint edits A/B from the same read revision and observes both; defers overlapping C without replacing B; replays B and sees no extra revision/event. Duplicate create keys return/defer to the existing record, duplicate display titles do not silently discard a page. Undo only touches unchanged result sections. Round-trip a shared page with an entity link and remove the pack; reset preserves user/shared origins, and forgotten keys cannot be auto-created again. Exercise search through current heads, not direct SQL assertions alone.

**Deletes:** `knowledge_standing_selection` and its selector logic; payload `lensKind`; active-title uniqueness as identity; latest-only Lens revision/change replacement; old whole-document deferred drafts; legitimate-World-link bans superseded by scoped typed links. Remove their dedicated tests only after equivalent behavior is tested through the new publisher. Preserve existing principle user-origin checks and portable/share authority tests.

## 2. File reusable findings through the completed-turn boundary

**Estimate:** 2 hours. **Dependencies:** 1. **Owner:** harness/turn boundary.

**Files:** `src-tauri/src/commands/tool_executor.rs`, `commands/tool_executor/{authority,principles}.rs`, `commands/knowledge/learning.rs`, `commands/chat_runtime/completion.rs`, `broker.rs`, `agent/tool-catalog.json`, `agent/SYSTEM.md`, `agent/skills/lens/SKILL.md`; current Memory authority/commit tests. `docs/agent/turn-tools.md` and `docs/agent/context-delivery.md`.

**Behavior:** replace `lens_update`'s document payload with checked section edits, acknowledged evidence resolution and explicit staging outcomes. Add WorkEpisode and receipt insertion to the existing completed reply transaction. The model is instructed to file useful research conclusions while keeping working notes for continuation. Produce explicit unavailable/skipped World processing outcomes instead of implying a queued job when `enqueue_source_world` returns none. Retain 16 Sources, one note and eight shared Lens/principle/schema changes.

**Proof:** drive the real broker with source save + subject update, complete the reply and read the saved page and receipt from a second Chat. The same sequence stopped, failed, private or tainted by Don't save produces no page/episode/Source publication. Reject forged/renderer-only read refs, wrong-owner claims and an invented source. At a forced commit collision, preserve the successful reply and report Deferred; at a transaction failure, roll back all Memory. The ninth shared Memory proposal and seventeenth source are refused. No helper or World model is required for the new subject to be readable.

**Deletes:** old six-sibling-field `lens_update` parsing/schema and tool prose; direct commit branch that silently `continue`s on a taken title; separate report assembly that can disagree with the receipt. Keep `note_save` as the sole working-note writer and move its misleading reusable-learning instructions to Lens. Do not add a second `memory_save` tool.

## 3. One World registry, reversible additions and safe followed identities

**Estimate:** 2–3 hours. **Dependencies:** 1. **Owner:** World core.

**Files:** `src-tauri/src/knowledge/{world_schema,schema_writer}.rs`, `knowledge/world_extract/ontology.rs`, `knowledge/map.rs`, `knowledge/assets/{mod,class}.rs`, `commands/knowledge/{world_schema,world_setup}.rs`, focused schema/matching fixtures; new private `commands/tool_executor/world_schema.rs`. `docs/knowledge/worlds.md`. Add catalog/tool routing only after unit 2 releases those files; this small integration is owned by the integrator, not a parallel write.

**Behavior:** registry generates extractor definitions, validation and display; add Concept/Process/Material and general relations; AssetClass comes from the Rust enum. Bind curated overlay selection to template ID. Store schema revision/head, freeze extraction inputs, implement read/add/revert using host validation. Keep open fallback and current overlay size limits. Replace Map/asset-page symbol/name-first matching with the same typed full-identity resolver; do not guess missing venues or issuer/instrument equivalence.

**Proof:** Rust broker test auto-applies a source-backed additive type only on completed reply, refuses core override, unknown endpoint, oversized extension and stale base, and shows a new revision on Undo. A frozen queued extraction retains its original schema after an edit. Existing extraction fixture runs without any user-approved schema and retains scientific process/material, crypto, real/private-asset facts. Map and asset page tests use identical symbols on two markets and assert neither crosses markets; missing market and two exact duplicate identities stay unresolved.

**Deletes:** duplicate core name lists and prompt copies, focus-text matching, pending-proposal apply/dismiss state, obsolete schema-proposal tables/commands and dedicated UI-contract tests. Replace `schema_writer.rs` with a private output adapter in the upkeep owner and delete the old file at integration; unit 4 owns removing its supervisor call sites, so these changes land together rather than leaving dangling callers. No paid re-extraction or new ontology approval screen.

## 4. Replace claim-count method learning with completed-work upkeep

**Estimate:** 2 hours. **Dependencies:** 2; coordinate the schema-pump deletion with 3. **Owner:** Memory upkeep.

**Files:** `src-tauri/src/knowledge/learning.rs`, `knowledge/learning/tests.rs`, `knowledge/graph_service.rs`, `knowledge/graph_service/tests.rs`, `commands/knowledge/method_learning.rs`, `agent/prompts/method-learning.md` (rename to a single upkeep prompt and update its sole loader). `docs/knowledge/lens.md` method-learning section.

**Behavior:** eligible episodes or World deltas make one pending review; select a finite frozen batch instead of waiting for the whole World queue. Build bounded input including answer context, exact evidence, relevant page sections, principles and optional schema additions. Use existing tool-free `dispatch_inference`; apply through the common publisher. Keep six-hour/four-per-day/one-call limits and no paid retry. Suppress self-triggered reviews and forbidden edits to principles/perspective. Set explicit terminal outcomes for oversized coverage and no-change.

**Proof:** inject a helper fixture into GraphSupervisor. One source-backed completed research task and zero new claims are sufficient to review and publish a subject edit; an explicit learned method requires its example/limits. Inspect the actual request: no tools, only admitted bounded items, preserved shared wrappers. Test restart from pending, running uncertain, NotSent, invalid output and cooldown, ensuring no extra sent call. Inject changed principles, removed source, paused page and reset epoch before finish; no stale publication. Continually add World jobs and demonstrate a finite frozen review still runs without swallowing later events.

**Deletes:** five-claim admission dependency, World-queue-empty requirement, method-only input/output decoder and direct publishing branch, standalone schema helper pump. Move valuable existing rate-limit/recovery tests onto the new GraphSupervisor entry point; remove tests that merely pin retired counters or prompts.

## 5. Enumerate and read a bounded evidence path

**Estimate:** 1–2 hours. **Dependencies:** 3. **Owner:** graph discovery.

**Files:** new `src-tauri/src/knowledge/connections.rs`; existing `knowledge/world_extract/search.rs`, World search/read adapter and graph fixtures. `docs/knowledge/world-search.md`.

**Behavior:** adjacency indexed by node/claim incidence; two/three-step paths with exact roles, source basis, cutoff and a changed claim; deterministic rank and novelty hash; explicit scan/truncation budget. `world_search` connections mode and upkeep candidate selection use this one function. Selection adds no inferred factual edge and makes no provider call.

**Proof:** exercise World search through the broker on a frozen graph: a new material/process claim connects to an exactly followed listing in three steps and returns readArgs for all premises. A same-symbol foreign listing, withdrawn edge, missing source, future premise and repeated path do not produce a new eligible lead. With a high-degree hub, the query returns bounded/truncated output rather than exhausting the graph. Exact-read those result targets to establish they are real inspectable claims.

**Deletes:** no replacement graph engine; remove any duplicate local path enumeration introduced during implementation. Preserve existing relevant/hybrid search behavior with its current tests. Do not replace Map layout or derive new causal edges from adjacency.

## 6. Give Bot a role and a durable research handoff

**Estimate:** 3 hours. **Dependencies:** 2; domain declarations from 1. **Owner:** chat lifecycle.

**Files:** `src-tauri/src/agent_harness.rs`, `commands/chat_runtime/{setup,launch}.rs`, new `chat/handoff.rs`, `chat/home.rs`, `commands/tool_executor/research.rs`, `briefings/policy.rs`, relevant internal scheduled/unattended execution binding, host command routing and `commands/tests/home_thread.rs`. After unit 2 releases completion/broker/catalog files, this owner adds their narrow handoff integration. `docs/ui/home-thread.md`, `docs/agent/context.md`, relevant scheduled-run docs.

**Behavior:** host-derived role with correct cache sealing; Bot-only `research_start`; staged create/launch on parent completion; new session executes via `send_chat` with the captured scope and shared unattended restrictions. Existing RunCoordinator owns the worker; no new runtime. Add child outcome delivery to the current Bot posting path and attach original Memory receipt IDs. Private/as-of scope follows the task, exact-read acknowledgements do not. Keep session scheduling routed to Bot.

**Proof:** Rust broker/runtime test gives Bot a research request, commits one handoff, replays its operation and gets one child Chat/launch; child fixture returns a result and Memory receipt to Bot once. A stopped parent creates none. Scheduled/child attempts to hand off are refused. Inspect captured child request for private flags and cutoff, then prove a later source is rejected under that cutoff. A busy Bot delays delivery without stealing a run or changing history; restart marks running launch uncertain and does not charge again. Assert the cache differs when the same runtime resources have a different host role.

**Deletes:** implicit “everything is chat” role assumptions, any new-session clone path used for this flow in favor of the common handoff service, raw Thread-title presentation assumptions. Keep existing explicit draft-only actions such as Explain disagreement as drafts; they are not automatically paid starts.

## 7. Make accumulated understanding readable in the existing UI

**Estimate:** 3 hours. **Dependencies:** 1; use mock bridge fixtures for later host outcomes. **Owner:** renderer.

**Files:** `src/components/memory/MemoryPage.tsx`, `src/components/knowledge/LensWorkspace.tsx`, `src/components/map/MapInspector.tsx`, `src/components/chat/{ChatReceipt,MessageMemoryRow}.tsx`, `src/chat/receipt.ts`, `src/components/map/{lensReading,kinds,labels,whatsNew}.ts`, `src/components/agents/LoupistTracking.tsx`, `src/bridge/mock/knowledge.ts`, `src/bridge/tauriBridge.ts`, corresponding EN/KO strings and current UI tests. `docs/ui/map-lens.md`, Memory UI sections.

**Behavior:** Topics/Methods/Your rules/Changes reuse the Lens reader. Show section evidence and changes, pending/deferred reasons, correction draft and undo intent. A selected lead opens the existing graph inspector and real path highlight. Add a compact Bot work card without navigating automatically. Existing receipt presentation supports Subject and Perspective rather than silently dropping kinds; localized Bot labels replace raw stored Thread. Remove schema approvals from settings; ordinary View/Undo receipts explain additive changes. Default-on capabilities still respect existing user-paused learning/model settings.

**Proof:** Vitest through the mock bridge: a topic opens its original evidence and Back restores the page/path; correction attaches the exact section and fills Bot draft without sending; Undo sends its change identity and handles a conflict. An applied perspective-only receipt is visible. A private/failed message never claims a save. Busy/running child work does not change the selected session or its unsent draft. Run the same key flows with English and Korean text and existing narrow-panel fixtures.

**Deletes:** obsolete pending ontology proposal controls and their dedicated tests after replacement coverage; method-only assumptions in receipt/reader routing; duplicated learn toast vs Memory report decisions. Retain the existing graph, reduced-motion behavior, list fallback, evidence inspectors and user-rule controls. No new visual design system.

## 8. Wire one lead and one receipt through the complete loop

**Estimate:** 1–2 hours. **Dependencies:** 3–7. **Owner:** integrator; no concurrent ownership of touched files.

**Files:** integration points in `knowledge/{learning,graph_service,graph_jobs}.rs`, `chat/home.rs`, `agent/tool-catalog.json`, `briefings/policy.rs`, `commands/knowledge` receipt projection, relevant bridge DTO mapping and mock fixtures. Update all owning docs to the final behavior, not the temporary unit wiring.

**Behavior:** candidate path enters the existing upkeep call; helper can produce one Unverified synthesis with mechanism/falsifier; same Lens publisher produces a receipt; Bot/Map/Lens show that exact result. Live processing status is separate from immutable commit outcomes. Source extraction no-model/nontext/shared exclusions are explicit. A future research answer can discover the page and exact-read its original premises without refetching an unchanged source.

**Proof:** one deterministic headless integration scenario: completed Bot research saves original+topic, a scheduled task retains new contrary evidence, a fixture extraction creates a real cross-domain path, one upkeep attempt publishes an Unverified lead, Bot receives it once, and a new session reads the changed topic plus original evidence. Assert origin/receipt IDs across all boundaries. With the helper disabled, topic/file-back still works and the UI says Map/upkeep unavailable. This is the release behavior spine, not a source scan.

**Deletes:** any temporary old-contract adapters from units 1–7, duplicate helper/publisher path, independent saved-count calculations and obsolete proposal delivery rows. Search current app/preview/native/test callers before deleting; follow `docs/dev/repository-guidance.md#code-cleanup`.

## 9. Gate behavior, transfer claims and the final handoff

**Estimate:** 1–2 hours plus any failure repair. **Dependencies:** all. **Owner:** integrator/verification.

**Files:** extend `evals/agent/suites/learning-v1.json` and its current harness contracts; add graph/temporal fixtures to the existing World eval runner; maintain existing Rust broker/commit and Vitest suites. Use `scripts/verify.sh` as the local handoff gate; generated output is not edited or committed.

**Required proof:**

1. Broker/commit suite covers atomicity, acknowledgements, taint/private/as-of, stale edits, idempotent create/undo, shared-content boundaries and cardinality.
2. Headless eval contract contains: uninstructed research file-back followed by cross-session recall; scheduled contradiction revision; process/material-to-followed-asset connection with the wrong-market decoy; explicit temporary work; hostile source asking to rewrite user rules; future evidence that must be excluded. The fixture oracle checks committed records and exact citations, not “I remembered” text.
3. A paired Memory-on/off train/probe configuration measures supported follow-up answer quality, contradiction handling, false factual assertions, cutoff correctness and tool/token cost with masked entities and a frozen temporal split. Saving and delivering a method is not marked as transfer success. Live paid model evaluation is a separately budgeted run; this task's design does not imply it was executed or that gains exist.
4. `check:unused` and caller tracing remove orphaned UI/schema-learning implementations; `scripts/verify.sh` runs once after narrow checks pass. Run native macOS WKWebView smoke for the receipt→Lens→Source→Back and Bot→research→result paths, preserve all other session/window ownership, and record any native result not actually observed. Linux/browser preview cannot substitute for native acceptance.

**Deletes:** dedicated tests for retired representations and branches once behavior coverage is at the current entry point. Do not add broad source-string tests except established security invariants/release pins. Leave a brief task handoff with changed behavior, checks actually run, risks and missing acceptance evidence. No commit/push unless requested.

## Explicit later list

| Later work | Why it is outside this slice |
| --- | --- |
| Community clustering, PPR, global map-reduce summaries, graph embeddings/GNN link prediction | More retrieval machinery than the bounded path feature needs; requires scale and evaluation evidence. |
| Automated novelty search, hypothesis tournaments, outcome return tracking/backtests | A path is a lead, not investment alpha; point-in-time data and rigorous baselines are separate products. |
| Full issuer/instrument/listing reference-data service or every asset standard | Exact scoped identities and honest ambiguity fix today's bug without a new symbology system. |
| Automatic semantic page merging, wiki-wide rewriting, section reordering and rich visual diff editor | Page keys, bounded patches and ordinary before/after readers suffice; these introduce additional destructive decisions. |
| Historical as-of Lens head selection | Changes the established cutoff product semantics; first slice keeps current-head exclusion even though audit history is retained. |
| Automatic schema renames/deletions/entity merges and paid historical re-extraction | Additive revisions preserve correctness and reversibility; these need separate compatibility/reconciliation semantics. |
| Rich onboarding dialogue, a new full-screen Memory dashboard, canvas redesign | Existing interest chips, readers, Map and inspectors can demonstrate the feature now. |
| Proactive paid investigations of leads or additional helper models | The current user did not ask the app to buy unbounded research; one existing bounded upkeep call and explicit Investigate are enough. |

If the day runs short, defer visual polish and model-written lead explanations while retaining deterministic path discovery and its inspectable evidence. Do not cut atomic commits, provenance, scope/cutoff enforcement, truthful receipts, kind unification or the wrong-market fix to meet a time estimate. Report incomplete work explicitly instead of declaring the whole redesign shipped.
