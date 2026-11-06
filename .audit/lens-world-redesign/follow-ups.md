# Follow-ups left open by this change

## Product decisions

- **Decided (2026-11-07): vocabulary changes keep applying without a prompt.** The owner chose this over approval for background reviews or for all changes. Review producers stay gated on evidence in the graph, and every change has Undo. To add a prompt later, route the `review` producer in `knowledge/schema_writer.rs` back to a pending state.
- **Decided (2026-11-07): an interrupted World extraction runs once more.** Before, no extraction call was retried, so quitting the app during a job or a provider timeout dropped that Source's facts for good (10 of 15 attempted Sources in the native walk). Now a paid attempt that published nothing runs again once (`retry_once`); a second quarantines as before, and the World rebuilds itself.
- **Kept for later (2026-11-07): the map trails research by half an hour.** One graph worker serves every Loupist, one Source at a time, and the reviewed default runs GPT-6-Luna at high thinking for 3–5 calls per Source. Of the 14 Sources the battery Loupist saved between 03:33 and 03:48, one was on the map at 03:52 and 11 were still queued behind another Loupist's work. Medium thinking for extraction (`world-model-defaults.json`) or one worker per Loupist would shorten it; either needs an extraction-quality check first.

## Not verified

- The real-model walk (GPT-6.1-Sol medium, ChatGPT sign-in) covered a research session, a Bot hand-off and World growth on the development build ([journey](journey.md)). The paid `compounding` style evaluation (session one files, session two must read the page with the original Sources withdrawn) is not written or run.
- `npm run test:native` and `test:persistence` were not run after the walk; the development window held the GUI lease throughout.
- `template-seed-grade.mjs` has no recorded traces for the broader core, so curated seeds are unmeasured against it; its hand-written relation list still names the old 12.

## Product gaps from the design's Later list

- A background job that files what a research turn established but did not file (replacing claim-count method learning).
- Removing per-chat working notes (blocked on `knowledge_journals.research_id`).
- Model lint over pages (contradictions, merges, summary rewrite), communities as topic pages, model-explained links, temporal-holdout evaluation of links.
- An issuer, instrument and listing identity layer (FIGI, LEI, DTI).
- Marking a link `filed` when an idea page is filed about it; the inspector split between what sources say and what a link could mean.

## Copy and fixtures

- The `memory-pages` preview fixture shows a learned date later than its updated date.
- World extraction named a metric in English from a Korean KRX filing; the Bot's suggested question then reads "TC BONDER 4.5 GRIFFIN equipment order contract amount KRW 44.2 billion (VAT excluded) 변화는 어떻게 설명돼?". Names should follow the Source's language, and long metric names need a short label in questions.
- A link card showed the relation "developing": extraction emitted a relation outside the vocabulary, and the card prints an unknown relation as is.
- The Bot's chat materials panel says nothing was discussed while the Bot's own catch-up lists 17 new names.
- A hand-off card posted before `briefings::summary` keeps its stored "# heading" summary; only new cards are fixed.
- Opening a connection's tools reads "Opened OpenDART" in the Korean interface: the host composes the English verb with the name, and the renderer passes composed labels through. A step that carries the connection name as its own field would let the interface word it.

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
