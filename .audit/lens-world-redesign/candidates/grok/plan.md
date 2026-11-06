# Implementation plan

Eight units, sized so a few agents can land them in one working day. Each unit ends in a behavior test at a real boundary. Doc edits ride with the unit that changes the behavior. Version 6 refuses older folders, so there is no migration.

Parallel groups name units that do not share files. A later unit in the same group waits.

## Unit 1. Version 6 tables and one kind column

Depends on nothing.

Files:

- `apps/loupist/src-tauri/src/knowledge/schema.rs`
- `apps/loupist/src-tauri/src/knowledge/model.rs` only if a payload `lensKind` helper lives next to `LensDocument`
- `apps/loupist/src-tauri/src/knowledge/lens.rs` for the standing row read and write
- `docs/knowledge/lens.md`, `docs/knowledge/memory.md`

Behavior test, Rust, schema open path. A folder stamped version 5 is refused. A new folder is version 6 and contains `knowledge_filings`, `knowledge_projection_applied`, `knowledge_lens_mentions`, `knowledge_hypotheses`, `knowledge_schema_versions`, `knowledge_session_seeds`, `knowledge_discovery_cursor`, and `knowledge_standing`. Inserting a lens row with kind `subject` succeeds. Inserting kind `note` fails the check. The standing row points at a perspective record. A second change row for one method record inserts, which the old unique constraint forbade.

Deletes the payload `lensKind` write, the path that stores a perspective with metadata kind `method`, and any test that only asserts those two stores.

Parallel with units 2 and 3.

## Unit 2. Venue-qualified follow matching

Depends on nothing.

Files:

- `apps/loupist/src-tauri/src/knowledge/map.rs`
- `apps/loupist/src-tauri/src/knowledge/assets/mod.rs`
- the existing map and asset tests beside those files
- `docs/knowledge/worlds.md` only for the match rule

Behavior test, Rust, `match_followed`. Two nodes share the symbol and differ in exchange. A followed item with that symbol and one market matches that node only. A followed item with the symbol and no market, against those two nodes, is `Ambiguous` and matches nothing. One node with that symbol and no venue still matches. `match_entity` returns that same decision and does not take the first id of a sorted set.

Deletes the symbol-only map from `map.rs` and the `into_iter().next()` pick in `match_entity`.

Parallel with units 1 and 3.

## Unit 3. Title collision is visible

Depends on nothing.

Files:

- `apps/loupist/src-tauri/src/commands/knowledge/learning.rs`
- the tool result mapping in `commands/tool_executor/authority.rs` if that is where the model-facing error is built
- the lens create test next to `learning.rs`

Behavior test, Rust, the lens create path used by `lens_update`. Creating a method whose title fold is already an active page returns the existing record id and kind, and leaves the row count unchanged. The tool result contains that id. It does not return success.

Deletes the `continue` on `lens_title_taken`.

Parallel with units 1 and 2.

## Unit 4. File the answer, and tell the model which seat it is in

Depends on unit 1 for the filings table. Depends on unit 3 for the shared title-taken error.

Files:

- `apps/loupist/agent/tool-catalog.json`
- `apps/loupist/agent/SYSTEM.md`
- `apps/loupist/src-tauri/src/broker.rs`
- `apps/loupist/src-tauri/src/commands/tool_executor.rs`
- `apps/loupist/src-tauri/src/commands/tool_executor/authority.rs`
- `apps/loupist/src-tauri/src/commands/chat_runtime/completion.rs`
- `apps/loupist/src-tauri/src/commands/chat_runtime/setup.rs`
- `apps/loupist/src-tauri/src/agent_harness.rs`
- `apps/loupist/src-tauri/src/briefings/policy.rs`
- `apps/loupist/src-tauri/src/chat/home.rs` for `open_research` only
- `apps/loupist/src-tauri/src/knowledge/filing.rs` (new)
- `docs/agent/turn-tools.md`, `docs/agent/context.md`, `docs/ui/home-thread.md`

Behavior tests, Rust:

- Broker. A turn may stage one `answer_file`. A second call is `BudgetExceeded`. A claim id absent from the turn's exact reads is rejected. The conclusion `"margin 14.8%"` is rejected when no cited proposition or excerpt contains `14.8`. The same conclusion is accepted when the cited claim contains it. `open_research` is absent from a session runtime and present on a Bot runtime. An unattended allowlist check accepts `answer_file` and rejects `open_research` and `principle_save`.
- Commit path. A completed turn inserts one `knowledge_filings` row and a `Filing` change on the assistant message. An aborted turn with the same staged filing inserts nothing. `open_research` creates a chat and a `knowledge_session_seeds` row and does not enqueue a run.

Deletes the SYSTEM.md sentences that tell the model to park reusable company conclusions in the working note. `note_save` stays, described as scratch. No subject-page writes in this unit.

Not parallel with unit 3. Parallel with unit 5, which does not touch these files. Not parallel with unit 6.

## Unit 5. One schema vocabulary, forward-only apply, preset seeds

Depends on unit 1 for `knowledge_schema_versions`.

Files:

- `apps/loupist/src-tauri/src/knowledge/world_extract/ontology.rs`
- `apps/loupist/src-tauri/src/knowledge/world_schema.rs`
- `apps/loupist/src-tauri/src/knowledge/schema_writer.rs` only to call apply after a valid proposal and to read `untyped` names from filings
- `apps/loupist/src-tauri/src/knowledge/graph_service.rs` only the `pump_schema` branch
- `apps/loupist/src-tauri/src/domain/loupist.rs` if the template id is the seed key
- `docs/knowledge/worlds.md`

Behavior tests, Rust:

- `asset_facet_prompt()` equals the non-equity `InstrumentFacet` names in order, including `real_asset`, `prediction`, and `private`. `facet_page_class(Rate)` is `RatesFx`. `facet_page_class(Index)` is `None`. `facet_page_class(Equity)` is `Equity`. The merged schema contains `GOES_INTO` and `product_role`, and `world_schema.rs` has no second handwritten core-name list.
- A valid pending proposal appends a version row and becomes the live extension. A proposal that breaks `validate_extension` is failed and the live extension is unchanged. `undo_schema` appends a row equal to the previous JSON. `apply_preset` with the semiconductor template id installs that seed when the focus string is unrelated.

Deletes the focus-string equality guard on curated seeds, and the user-pending gate that left a valid proposal unapplied. The settings screen that only existed to approve proposals is deleted in unit 7, after this function exists. This unit stops calling that screen's command.

Parallel with unit 4. Not parallel with unit 6, because both touch `graph_service.rs`.

## Unit 6. Projector and structural discovery

Depends on units 1, 2, 4, and 5. Unit 2's matcher is what discovery uses to resolve followed listings. Unit 5 leaves `pump_schema` as the schema half of the pump so this unit can replace the learning half without editing the schema branch.

Files:

- `apps/loupist/src-tauri/src/knowledge/project.rs` (new)
- `apps/loupist/src-tauri/src/knowledge/discover.rs` (new)
- `apps/loupist/src-tauri/src/knowledge/graph_service.rs`
- `apps/loupist/src-tauri/src/knowledge/learning.rs` for the publisher removal
- context assembly files that build the 6c catalog and the 6e selection, under `src-tauri/src/knowledge/` and the chat context builder they already call
- `docs/agent/context-delivery.md`, `docs/knowledge/lens.md`

Behavior tests, Rust:

- Projector. Given a committed filing that cites one active claim, `apply_projection` writes one subject page whose Markdown contains that claim id, one mention row, and one applied row. A second apply returns `AlreadyApplied` and does not duplicate the section. `rebuild_subjects` over the same filing yields the same Markdown. A filing whose only claim is withdrawn yields `NothingToAdd`. A filing that would pass 8,000 characters creates a continued page and links it. Tombstoning the filing and rebuilding removes the section.
- Discovery. A two-hop active path between two venue-qualified followed nodes, with an unfollowed middle, yields one `unverified` hypothesis. The same path already named on a subject page yields none. A fifth hypothesis on the same day yields none. No helper model is consulted. The test constructs the graph in SQLite and calls `fresh_paths`.
- Context. After projection, the assembled index contains the subject title and record id, and the three-slot selection prefers that page when the turn's followed entities overlap its mentions.
- Pump. `pump_once` no longer publishes a Lens method from claim count. The `learnMethods` setting is unread. Unit 7 deletes the command and the toggle together so the UI never calls a missing command.

Deletes `pump_learning`'s Lens publication, the five-claim gate, the 6-hour method cooldown as a publisher, and tests that only assert that publisher.

Not parallel with unit 5. Parallel with unit 7 once the `MemoryUpdateChange` variants from unit 4 are in `contracts/memory.ts`. Unit 7 can fixture those variants without waiting for the pump. Ship unit 7's contract edits at the start of unit 4 if both agents would touch `memory.ts`. Unit 4 owns `memory.ts` change kinds. Unit 7 only reads them.

## Unit 7. Cards, receipts, Memory reader, preset chips

Depends on unit 4 for change kinds and `open_research`. Depends on unit 5 for `apply_preset` and `undo_schema`. The Bot card for a hypothesis can render from a fixture before unit 6, and the delivery test uses the host message builder with a constructed `MemoryUpdateResult`.

Files:

- `apps/loupist/src/contracts/memory.ts` (only if unit 4 did not already add the kinds)
- `apps/loupist/src/contracts/bridge.ts`
- `apps/loupist/src/bridge/tauriBridge.ts`
- `apps/loupist/src/bridge/mock/knowledge.ts`
- `apps/loupist/src/chat/receipt.ts`
- `apps/loupist/src/chat/receipt.test.ts`
- `apps/loupist/src/components/memory/MemoryPage.tsx` and its test
- `apps/loupist/src/components/chat/ChatReceipt.tsx` and `MessageMemoryRow.tsx` if the new kinds need a row
- the Bot host-message builder in `src-tauri/src/chat/home.rs`
- the schema approval UI in `src/components/agents/LoupistTracking.tsx`
- `apps/loupist/src-tauri/src/commands/knowledge.rs` and the bridge method for `learnMethods`, deleted with the toggle
- locale strings in `src/i18n/locales/en` and `src/i18n/locales/ko`
- `docs/ui/home-thread.md`, `docs/ui/map-lens.md`

Behavior tests:

- Vitest, `receipt.test.ts`. A perspective `lens_receipt` appears in the receipt. A `Filing` change appears. A `Hypothesis` change renders from, via, and to. Save-off still forces an empty receipt.
- Vitest, Memory page, mock bridge. History for `home-{id}` uses the localized thread label and does not render the stored title `Thread`. The subject list shows a subject record. No control sets `learnMethods`. No control approves a schema proposal.
- Vitest, mock bridge. Forget on a filing calls `memoryForget` and the following catalog omits that title. Keep calls `memoryKeepHypothesis`. Undo calls `memoryUndoSchema`. A preset chip calls `applyPreset` with the template id.
- Rust, home delivery. The host message built for a projection batch carries `memory_update` with the subject and hypothesis changes. A scheduled-run host message carries the run's filing change on the same `memory_update` field it currently leaves empty.

Deletes the schema approval UI, the method-learning toggle, the `learnMethods` setting command, and the receipt branch that drops `perspective`.

Parallel with unit 6 if unit 4 already landed the contract kinds. Not parallel with unit 4.

## Unit 8. Headless eval contract for "the next task can see it"

Depends on units 4 and 6.

Files:

- `apps/loupist/evals/memory/filing.md` (new contract: inputs, the context text that must appear, and the rows that must exist)
- the smallest existing headless runner those evals already use, only if the contract needs a fixture hook
- no second runner

Behavior test. The contract is executable by the current eval harness, or by the context-assembly Rust test from unit 6 if the harness cannot run without a model. Preferred check, no model call: build a Loupist, commit a filing through the test broker, run one pump, assemble the next turn's context, and assert the subject title and the claim id are in the index and absent from a turn whose as-of date excludes the claim. The markdown file records that command and the assertion so a later agent can re-run it.

Deletes nothing.

Not parallel with unit 6.

## Suggested day split

Wave A, together: units 1, 2, and 3.

Wave B, together: units 4 and 5.

Wave C: unit 6, then unit 8. Unit 7 starts as soon as unit 4's contract kinds exist, and lands beside unit 6.

## Later, and why it is not in the day

- Helper prose for hypothesis mechanism, sign, lag, and falsifier. The structural card already teaches that a path exists. A helper sentence needs a rate-limit policy and a test that empty helper output still keeps the row. That is a second pass.
- A lint pump on the briefing schedule for orphan pages, one-way `lens:` links, and sections whose claims later expired. The projector already drops a withdrawn claim at apply time. Sweeping old pages is a separate cron and a separate test.
- Re-extracting watched entities after a schema apply. Forward-only is the safety property. Re-queueing is easy to get wrong and is not required for new research to use the new types.
- Filings that cite only Lens methods and no claim. Open question in the design. Adding it changes the broker rule and the projector's "no sentence without a claim" check.
- Vector search once the index passes a few hundred lines. The derived index is the navigation that fits the current catalog size.
- Community summaries and graph embeddings. They do not file the answer, and they add a model call on the path this design made deterministic.
- Sharing subject pages. Blocked until a shared pack can wrap agent-written conclusions without a receiver treating them as instructions. The current wrapper is necessary but not sufficient while the page is still a new kind.
- In-turn edits of subject Markdown. That reopens two writers. Do not add it because a card feels slow. Speed up the pump instead.
