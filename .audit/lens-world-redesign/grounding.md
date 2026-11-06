# Grounding (main @ 5046408)

Paths are under `apps/loupist/` unless they start with `docs/`.

## Harness
- Save policy lives only in `agent/SYSTEM.md:137-235`. Saving is mandatory for handed-over material and explicit "remember" requests; otherwise judgment ("Skip chatter, light questions and stated one-offs").
- `agent/tool-catalog.json` is the single tool schema source (embedded in the Pi extension, parsed by Rust). Permissions: `always`, `memory_read`, `memory_write`. Write tools sit in lazy components: `memory-save` (source_save, note_save, journal_*, thesis_update, earnings_snapshot_save) and `lens-learning` (principle_save, lens_update). Writes need `capability_load` first.
- Broker cardinality per turn: one note_save, ≤16 source_save, ≤8 lens_update+principle_save (`src-tauri/src/broker.rs:588`).
- Staged buffers in `ToolCapture` (`commands/tool_executor.rs:136-147`) commit in one SQLite transaction in `commit_completed_turn` (`commands/chat_runtime/completion.rs:32`, persist closure `:293-398`), then `graph.wake()` (`:423`).
- Background supervisor `GraphSupervisor::pump_once` (`knowledge/graph_service.rs:447`): schema proposals → method learning → World outbox. Isolated tool-free inference: `dispatch_inference` (`knowledge/graph_inference.rs:163`).
- Method learning (`knowledge/learning.rs`) never sees the transcript; it needs ≥5 new claims since the last review, World queue drained, 6h cooldown, ≤4/day. Its output publishes straight to Lens.
- `enqueue_source_world` silently skips World jobs without a helper model, with enrichment off, for non-text spans or shared-pack Sources (`knowledge/graph_jobs.rs:385-420`).
- Scheduled runs: briefings force use/update memory true; unattended allowlist (`src-tauri/src/briefings/policy.rs:6-49`) admits source_save, note_save, lens_update, thesis_update, earnings_snapshot_save and reads; not principle_save or journal.
- A Bot-only post-hook exists after finalize in `send_chat_inner` (`commands/chat_runtime.rs:511-517`).

## World
- Core schema: 8 types, 12 relations (`knowledge/world_extract/ontology.rs:42-162`), duplicated as constants in `knowledge/world_schema.rs:8-43`. Asset.asset_class prompt vocabulary (index|fund|bond|commodity|currency|crypto|rate|other) differs from `knowledge/assets/class.rs:6-15` (equity, real_asset, private, ...).
- Extension: ≤4 types, ≤6 relations, ≤3 fields, stored in `GraphProcessingSettings.extension` JSON (`knowledge/graph_jobs.rs:150`). Version is a counter, no history.
- Curated seeds only for semiconductors, dividends, macro and only when the focus string equals the i18n default (`world_schema.rs:45-56`, `:582-593`). 9 templates in `domain/loupist.rs:33-89`.
- Model seed/review proposals (`knowledge/schema_writer.rs`) wait for the user in `src/components/agents/LoupistTracking.tsx` (Loupist settings).
- No schema reaches the Chat agent; no agent tool edits the schema.
- Bug: `knowledge/map.rs:189-194` matches followed items by symbol/name without market; `knowledge/assets/mod.rs:1562-1563` takes the lexicographically smallest matching node id.

## Lens
- Payload `LensDocument` (`src-tauri/src/knowledge/model.rs:750`): markdown, applicability, assumptions, procedure, principles, limitations. No per-kind payload shape.
- Kind is stored three ways that can disagree: `knowledge_lens_metadata.lens_kind` CHECK in ('perspective','method','principle') (`knowledge/schema.rs:416-432`), payload `$.lensKind` for principles only, and `knowledge_standing_selection` for the perspective (whose metadata kind stays 'method'). `KNOWLEDGE_SCHEMA_VERSION = 5` (`knowledge/schema.rs:14`); older folders are refused.
- One revision per record (`knowledge_record_revisions.record_id` UNIQUE); `knowledge_lens_changes` keeps one row per record and is replaced on revision, so earlier provenance is lost.
- Links: `lens:rec_<id>` only, resolved by record id (`knowledge/lens.rs:214`, `:674`), navigation with backlinks (`:179`). Lens bodies may not reference World entities (`lens.rs:334-390`); Lens→entity edges exist only as `knowledge_lens_applications` (applies_to), written from final-answer `lens:` links and method learning.
- `lens_update` (`commands/tool_executor/authority.rs:1192-1400`) accepts kind perspective|method, needs an acknowledged exact `lens_read` for edits; stale heads become proposals. Commit in `persist_saved_turn` (`commands/knowledge/learning.rs:6`). A create whose title is taken is silently dropped (`learning.rs:134`).
- Working notes (`note_save`, one per Chat, replaced, no growth event) and Journal are not Lens records. Research conclusions therefore do not compound across sessions.
- Context: 6c catalog = standing perspective + newest 40 Lens; 6e = ≤3 Lens + ≤3 note pointers by lexical match at setup.
- Kind enumerations to touch (blast radius): Rust schema.rs, catalog.rs, lens.rs, principles.rs, learning.rs, shared_pack.rs, memory_reset.rs, commands/knowledge.rs, chat_reads.rs, authority.rs, domain/memory.rs, map.rs, sharing/*; TS contracts/knowledge.ts, contracts/memory.ts, chat/receipt.ts, MemoryPage.tsx, LensWorkspace.tsx, map/{lensReading,kinds,labels,whatsNew}.ts, MapCanvas.tsx, MapInspector.tsx, learningState.ts, LensLearningPanel.tsx, bridge/mock/knowledge.ts; agent/tool-catalog.json, skills/lens/SKILL.md, SYSTEM.md.

## Bot and research sessions
- Bot = home Thread `home-{loupist}` (`src-tauri/src/chat/home.rs:28-78`), stored title "Thread". Same `chat_send` path, SYSTEM.md, model and composer as sessions. Differences: schedule tool filter (`commands/chat_runtime/setup.rs:62-76`), cache key, and `deliver_finished_runs` after each Bot turn. Nothing tells the model it is the Bot.
- Host posts into the Bot (`home.rs:525-658`): delivery, alert, watch, earnings, schedule handoff receipts. They carry no `memory_update`; the Bot never shows what was learned.
- Handoffs: session → Bot exists only for scheduling (`schedule_handoff`). UI → new session drafts exist (Explain the disagreement, Look back, asset page). No model tool opens a research session from the Bot.
- User-facing Memory: answer receipt (`src/chat/receipt.ts`, `components/chat/ChatReceipt.tsx`, `MessageMemoryRow.tsx`), Map What's new, MemoryPage "What {Loupist} knows" (`components/memory/MemoryPage.tsx`: about, principles, methods, history), learning toasts. MemoryPage prints the raw "Thread" title; perspective receipts are dropped (`receipt.ts:61-62`).
- Templates: Rust `LoupistTemplate` (`domain/loupist.rs:33-89`): portfolio, semiconductors, dividends, crypto, macro, commodities, realAssets (+2). `AssetClass` (`knowledge/assets/class.rs`): equity, crypto, fund, commodity, rates_fx, real_asset, prediction, private.
