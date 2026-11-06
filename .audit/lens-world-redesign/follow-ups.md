# Follow-ups left open by this change

## Needs a product decision

- **Vocabulary changes apply without a prompt.** The user did not decide this; the orchestrator chose it from the candidates' arguments (an approval an ordinary user cannot judge never gets answered). Review producers are gated on evidence in the graph and every change has Undo. To require approval instead, route the `review` producer in `knowledge/schema_writer.rs` back to a pending state; chat and preset changes would stay immediate.

## Not verified

- No run with a real model: `lens_file`, `research_open` and discovery are covered by unit tests and scripted Pi fixtures only. The paid `compounding` style evaluation (session one files, session two must read the page with the original Sources withdrawn) is not written or run.
- No native run: WKWebView layout, the Tauri bridge and the native file dialogs were not exercised. Browser previews only (`memory-pages`, `chat-learned`, English and Korean).
- `template-seed-grade.mjs` has no recorded traces for the broader core, so curated seeds are unmeasured against it; its hand-written relation list still names the old 12.

## Product gaps from the design's Later list

- A background job that files what a research turn established but did not file (replacing claim-count method learning).
- Removing per-chat working notes (blocked on `knowledge_journals.research_id`).
- Model lint over pages (contradictions, merges, summary rewrite), communities as topic pages, model-explained links, temporal-holdout evaluation of links.
- An issuer, instrument and listing identity layer (FIGI, LEI, DTI).
- Marking a link `filed` when an idea page is filed about it; the inspector split between what sources say and what a link could mean.

## Copy and fixtures

- The chat context card calls methods "way of working" (작업 방식) while the Pages tab says "Methods" (분석 방법).
- The `memory-pages` preview fixture shows a learned date later than its updated date.

## Code structure flags from the comment review (not changed here)

1. `chat::handoff::HandoffRequest::parse` returns an anonymous `(String, String)`; return a named field error.
2. `chat::handoff::Handoff::message_id` is the session's final reply; rename it.
3. `chat::handoff::send_created` spawns its own task to break the deliver → launch → `send_chat` → `after_turn` loop; make launching a queue the delivery pass signals.
4. `chat::handoff::launch` packs three outcomes into `Result<bool, _>`; return an outcome enum.
5. `chat::handoff::for_delivery` launches and reaps while named like a read; split it and make the read order structural.
6. `ChatSurface::resolve(interactive: bool)` infers interactivity from a missing admission; pass the admission kind.
7. `ChatSurface::after_turn` may run only after the run lease and Memory writer drop; require a guard that proves it.
8. `ToolCapture::surface: Option<ChatSurface>` uses `None` for host-driven executors; add an explicit variant.
9. `ScheduleContext` also carries hand-off settings and `HandoffRun` repeats its fields; share one type.
10. `commands::knowledge::lens_search` dedupes because the index stores two documents per page; return one hit per record.
11. `search_index::documents` tells a principle from a page by trying to deserialize; use the metadata kind.
12. `SavedTurn::retained` is `Vec<(String, RetainedSource)>`; name the saveRef.
13. The `commit_turn` helper in `tool_executor/tests/lens_file.rs` copies the runtime's commit order; extract one function.
14. `world_schema::read_changes`/`recent_changes` rely on an implicit `ORDER BY rowid DESC`; make the order part of the name or type.
15. `world_schema::means` hides what it does; rename it.
16. `Provenance::origin` and `SchemaChange::origin` are anonymous tuples; name them.
17. `connections::core_category` copies the category mapping in `src/components/map/kinds.ts`; keep one source.
18. `connections::Walk::walk` breaks ties by traversal order; put the tie-break in the comparison.
19. `connections::MAX_INCIDENCES` caps edges, not claims; rename it.
20. `DetectInput::already` is an anonymous pair set; name it.
21. `GraphProcessingSettings::extension` is a skipped field filled from the change log; keep the live extension out of settings.
22. `KnowledgeStore::followed_items` must run before `transact` and turns an app-DB failure into "follows nothing".
23. `schema_writer::{RELATION_CLAIMS, RELATION_SOURCES, TYPE_NAMES}` are minimums; use `MIN_` names.
24. `schema_writer::claim` collides with the World claim noun; rename it.
25. `catalog::create_record` accepts `RecordKind::Lens` and refuses it at runtime; narrow the input type.
26. `eval_harness::world::Scenario::seed` is a string; make it a serde enum.
27. `HomeVocabulary { labels, labels_ko }` matches two arrays by index; carry labels per kind.
28. `pageTitle` detects the default perspective by a magic title, repeated in `MemoryPage.tsx`.
29. `withMapLinks` passes `world:` links as `lens:world~<index>`; add a world link type to the Markdown renderer.
30. `preview/growth-replay.ts` adapts a fixture recorded before keyed sections; re-record it and delete the adapter.
