# Lens and World as a filing log

## Problem

Loupist should get better at research, and better at noticing connections the user did not ask for, while the user does ordinary chat and scheduled tasks. The user should not curate Memory. Lens stays an agent-maintained Markdown wiki. World stays a claim graph extracted from retained Sources. The Bot stays the effortless thread. A research session stays the expert room. The app is unreleased, so `KNOWLEDGE_SCHEMA_VERSION` can move from 5 to 6 and old folders can keep being refused.

The current shape cannot compound. A finished research answer is not a Memory record. `note_save` keeps one working note per chat and replaces it, so the conclusion dies with the next save (`agent/SYSTEM.md` tells the model to put company facts in that note). `lens_update` rewrites a method or a perspective in the same turn, one revision per record, and a create whose title is taken is dropped (`commands/knowledge/learning.rs` around the `lens_title_taken` branch). Kind lives in three places that can disagree: `knowledge_lens_metadata.lens_kind`, a payload `lensKind` for principles, and `knowledge_standing_selection` for a perspective whose metadata kind stays `method`. Lens prose is forbidden from naming World entities (`knowledge/lens.rs` rejects `world://` and non-source record ids). Background method learning never reads the transcript, waits for five new claims, and publishes straight to Lens. The chat agent never sees the World schema. Schema proposals wait in `LoupistTracking` for a person who cannot judge types. Followed-asset matching ignores venue (`knowledge/map.rs` keys only the symbol, and `knowledge/assets/mod.rs` `match_entity` takes the first id from a sorted set). The home thread is stored as "Thread" and Memory history prints that title. Perspective deliveries exist on `LensReceipt` and `receipt.ts` drops them. Nothing proposes a multi-hop path between followed names.

Rust already has the boundaries this design must keep. Tool writes stage on `ToolCapture` and commit in one SQLite transaction inside `commit_completed_turn`, or they vanish when the turn fails or stops. The broker caps `note_save` at one and `lens_update` plus `principle_save` at eight. As-of turns and shared-content wrappers stay in force. Background model calls stay on the isolated tool-free helper, and this design removes the need for a helper call on the compounding path. The Map stays the visible graph.

## Usage (caller's view)

The model does two new things. It files a conclusion. In the Bot it can open a research session. It never edits a subject page, never registers a hypothesis, and never edits the schema. Those happen after commit, in host code.

A research turn that reached a conclusion, with saving on:

```text
world_search { query: "EUV photoresist suppliers" }
world_read   { claim ids from the search, unchanged read args }
lens_search  { query: "photoresist" }
lens_read    { recordId, expected revision from the hit }
answer_file  {
  title: "EUV photoresist suppliers",
  conclusion: "JSR and Tokyo Ohka supply the resist ASML's EUV tools expose. [JSR supply](claim:clm_jsr) [ASML tool](world:nod_asml)",
  claimIds: ["clm_jsr"],
  entityIds: ["nod_asml", "nod_jsr"],
  untyped: []
}
```

`answer_file` returns `{ "filingId": "fil_…", "status": "staged" }`. The model finishes the reply and does not say the finding was saved until the host receipt says so. A second `answer_file` in the same turn is a budget error. A claim id that this turn did not exact-read is a tool error. A number in `conclusion` that appears in none of the cited claim propositions or retained evidence excerpts is a tool error. Stopped and failed replies leave no filing.

A scheduled briefing uses the same `answer_file`. The unattended allowlist gains that name and still excludes `principle_save`. The briefing's existing host post in the Bot carries the filing on `memory_update`.

The Bot, told `seat: bot` in the runtime block it already receives:

```text
open_research {
  question: "Compare EUV resist suppliers on 2026 capacity",
  entityIds: ["nod_asml", "nod_jsr"],
  lensIds: ["rec_photoresist"]
}
```

The tool returns `{ "chatId": "cht_…", "status": "opened" }` and does not start a model run. The model tells the user the session is there. The user opens it and continues. A session runtime does not have `open_research`. Session to Bot stays `schedule_handoff` only.

What the user sees:

- In the Bot, the answer receipt says "Kept this for next time: EUV photoresist suppliers." Actions are Fix and Forget. Fix calls the same open-research host function with that filing as the seed. Forget tombstones the filing.
- When the projector finishes, the Bot gets one host card, not a second copy of the answer. "Updated what you know about EUV photoresist suppliers." If a new path exists, the same card adds "A connection you have not written down: ASML, EUV resist, JSR." Actions are Keep, Fix, and Forget. Keep lets the projector add one sentence to the subject page. Forget tombstones the hypothesis. The Map highlights that path.
- A schema change that passed validation posts "Loupist will also track process steps. Undo." The blurb is the proposal's existing user-facing line. The type name is not shown. Undo restores the previous extension.
- A research session shows the filing on its own answer receipt and does not repeat the Bot card. The session is where the user argues, edits a method, or states a principle.
- Memory is a reader. It lists the standing perspective, principles, methods, and subjects from the derived index. History names the home thread with the localized product label, never the stored title "Thread". There is no schema form and no method-learning toggle.
- First open of an empty Bot offers the existing template chips, such as semiconductors or dividends. Choosing one applies that template's seed through the same schema-apply function. The user can also ignore the chips and talk.

## Shape

The compounding record is an append-only filing log. Subject pages and hypotheses are a projection of that log plus the World claims the filings cite. The projection is a pure function of those inputs, so an incremental apply and a full rebuild of subject pages must produce the same Markdown. That equality is the learning invariant. A test feeds one filing, applies it, rebuilds, and compares the page bytes.

Per foundational thinking, the log is the structure, and the pages are derived. Per model-the-domain, a filing, a subject, a hypothesis, a method, and a principle are different lifetimes, so they are different variants of one page-kind enum rather than flags. Per separate-before-serializing, a chat turn never writes a subject page. Two chats append two filings. The graph supervisor, which already serializes World work under `pump_lock`, is the only writer of subject and hypothesis pages. Per make-operations-idempotent, `knowledge_projection_applied` records the filing id in the same transaction as the page write. A retry finds the id and does nothing. A crash mid-apply leaves no page write, and the next pump applies the filing. Forget tombstones the filing and rebuilds the touched pages from the live log, so a second Forget is a no-op.

`answer_file` is the missing file-back operation. `note_save` stays the one scratch note per chat and stops being where conclusions live. `lens_update` still creates and revises only `perspective` and `method`, after an exact read, and still stages a proposal when the revision head moved. `principle_save` stays a separate tool because the locked rule is that only the user's stated words become principles. The model cannot reach subject or hypothesis through `lens_update`. The catalog enum for `kind` stays `perspective | method`. Storage uses one column, `knowledge_lens_metadata.lens_kind`, now including `subject` and `hypothesis`. The payload `lensKind` field and the trick of storing a perspective as a method go away. The standing perspective is a single row, `knowledge_standing(loupist_id, record_id)`, pointing at a record whose kind is `perspective`.

A filing cites claim ids and node ids. The broker checks those ids against reads staged this turn, including the as-of set when the turn has a cutoff. The filing stores the cutoff. The projector will append it, dated, and will not withdraw a newer section because of an older cutoff. Shared wrappers and instruction-shaped text are rejected at the tool boundary. A filing's origin is `agent`. It is not a Source, and extraction never turns it into a claim. Per boundary-discipline, those checks sit in `parse_filing` at the tool boundary. `project_filing` trusts a `Filing` value.

The projector copies the filing's conclusion into one subject page. It does not call a model. The conclusion was already written by the turn that did the research. Copying it avoids a second prose pass that can drift from the cited claims. The page title is the filing title. Mention edges point at the cited nodes and claims. A later filing with the same title appends a dated section. The page holds at most 8,000 characters. Past that, the projector starts a new page, "{title} continued", and links it with `lens:`. It does not rewrite earlier sections. If the new filing cites a claim that opposes, or a claim marked disputed or withdrawn, the new section still appends and the host adds one line naming the date of the earlier section. Disagreement stays visible. A cited claim that is withdrawn at project time is dropped from that section, and if no cited claim remains the filing produces no section and the card says nothing was added.

Discovery is another pure function, `fresh_paths`, called from the same pump after filings for that Loupist are applied. It looks at active claims recorded since the discovery cursor. It keeps a path of two or three hops whose ends touch a followed listing, whose middle node is not itself followed, and whose claim-id sequence is not already on a subject page or a hypothesis. The match key is normalized symbol plus normalized venue, from the followed item's market and the node's `exchange` or `venue`. A symbol that hits two venues and a follow item with no market is `Ambiguous` and matches nothing. The lexicographic first id is gone. At most one new hypothesis per pump and four per day are stored, reusing the old learning daily cap. The row stores the claim ids, the end names, and status `unverified`. Prose is a host sentence built from the node names. No helper call. Keep, Fix, and Forget are the only ways it changes. A research session that files an answer about that path supersedes it by naming the hypothesis id on the filing. The hypothesis becomes `answered` and stops being offered.

World stays the fact layer. Core types stay the eight in `world_extract/ontology.rs`. Two additions land in that one list, and `world_schema.rs` stops keeping a second copy of the names. `Product` gains `product_role` with values `offering`, `material`, `process`, and `technology`, defaulting to `offering` when extraction omits it. A new core relation `GOES_INTO` runs from Product to Product, meaning the source is an input to the target. `USES` stays "relies on". Asset gains a `venue` field. `asset_class` stops being a free string. Its prompt values are generated from one `InstrumentFacet` enum: `index`, `fund`, `bond`, `commodity`, `currency`, `crypto`, `rate`, `real_asset`, `prediction`, `private`, `other`. `facet_page_class` maps fund, commodity, crypto, rate, real_asset, prediction, and private onto `AssetClass`. Index, bond, currency, and other map to no asset page. Equity pages match Organizations through ticker plus exchange, which is the facet `equity` on the Organization side only. Semiconductor process knowledge is a path of Products with roles, linked by `GOES_INTO`, not a new type that spends the four-type extension cap.

Schema proposals keep the existing writer and the existing caps of four types, six relations, and three fields. A proposal that `validate_extension` accepts is applied in `pump_schema` to future extraction only. Episodes already extracted are not re-queued. Each apply appends the new extension JSON to `knowledge_schema_versions`. Undo appends another row whose JSON is the previous snapshot, so history is never rewritten. An invalid proposal is marked failed and does not change the live extension. The chat agent has no schema tool. Names it could not place go in `answer_file.untyped`, and the existing schema writer sees those strings on its next pass. Preset chips call the same apply function with the seed that today runs only when the focus string equals the i18n default. That string guard is deleted.

Context delivery replaces "newest 40 Lens" with a host-built index, one line per active page: kind, title, record id, and the first 120 characters of the newest section. The standing perspective is still included in full. The small selection that used to be lexical only prefers pages whose mention edges overlap the turn's followed entities and the query, and falls back to lexical match, still at most three Lens and three notes. Exact `lens_read` is still required before use. Shared export continues to allow perspective, method, and principle only. Subject and hypothesis pages stay on this computer. `TopicAnnotation` stays inside the World snapshot as extractor output. It is not a Lens page.

The Bot and a session share `SYSTEM.md`, the model, and `chat_send`. `AgentRuntimeProfile` gains `seat`. Setup sets it from the existing `is_bot` check. The prompt states the seat in one paragraph. In the Bot the model answers here and calls `open_research` for work that needs a sustained evidence pass. In a session it files a reusable conclusion and does not treat the scratch note as Memory. `open_research` inserts a chat plus one `knowledge_session_seeds` row holding the question, entity ids, lens ids, and the origin message id. Context assembly for that chat pins those ids as read-required hints. The user still gets exact reads. No run is enqueued, so opening a session spends nothing until the user sends.

Receipts are host-built from the commit and from the pump, never from answer text. `MemoryUpdateChange` gains `Filing`, `Subject`, `Hypothesis`, and `Schema`. Perspective deliveries render in the receipt. The home-thread title shown to people is the localized label. The stored id `home-{loupist}` stays.

What the model can call grows by `answer_file` and, on the Bot only, `open_research`. Projection, path ranking, venue matching, schema apply, mention parsing, rebuild, and card text stay behind those two calls and the existing bridge intents `memoryForget`, `memoryKeepHypothesis`, `memoryUndoSchema`, `openResearchSession`, and `applyPreset`. React never sees SQLite rows, extension JSON, or claim status enums except the card fields it renders. Per minimize-reader-load, there is no Memory service wrapper. The supervisor calls `project_filing` and `fresh_paths`. Per laziness-protocol, method learning's publisher is deleted in the same change as the projector, including the `learnMethods` setting and its UI, because a control that no longer does anything is a second way. Per migrate-callers-then-delete and subtract-before-you-add, the three kind stores, the silent title drop, the focus-string seed guard, and the approval-only schema screen go away in the units that replace them. Per encode-lessons-in-structure, venue is part of the match key, title collision is a returned record id, and the asset-class prompt is generated from `InstrumentFacet`. Per experience-first, the user sees a card and a Map highlight, and never a type editor. Per type-system-discipline, `PageKind`, `Seat`, `Match`, and `InstrumentFacet` make the illegal combinations unrepresentable. `ModelLensKind` is the subset the tool schema can name.

This design does not add a graph network, community summaries, a vector index, a second wiki, automatic research runs, or a path from agent prose to World claims. It does not calculate figures. Existing tools already do that, and the filing boundary rejects numbers those tools did not return.

A reader can see who writes what. Turns write filings, methods, principles, sources, and journal entries. The supervisor writes subject pages, hypotheses, and applied extensions. React reads cards and sends five intents.

Red-flag screen. Callers do not sequence compile steps. The public calls hide the merge policy. Schema names are not repeated in a second list. The pump is one owner, not a load module plus a save module. `open_research` from the model and from the card is one host function. Subject pages have no second editor. Projection internals are `pub(crate)` and reached through the pump.

## Synthesis decision

Pending. The arena orchestrator fills this after it compares candidates.

## Tradeoffs accepted

- We accept a subject page that quotes the filing, in exchange for a projection that a rebuild can reproduce without a model call.
- We accept that a turn which never calls `answer_file` leaves no compounding record, in exchange for not inventing Memory the model did not commit to. The receipt makes the miss visible, and the eval counts filings against substantive turns.
- We accept hypothesis cards with no model-written mechanism, in exchange for discovery that still appears when the helper model is absent or rate-limited.
- We accept schema changes applying to future extraction without a confirmation dialog, in exchange for Memory that specializes when the user cannot judge types. Undo and the appended version row are the reverse path. Old claims keep the schema they were extracted with.
- We accept one scratch note beside the filing log, in exchange for a place to park in-progress text that must not become a page.
- We accept refusing to share subject pages, in exchange for not handing another person agent-written conclusions wrapped as if they were methods.
- We accept a four-per-day hypothesis cap, in exchange for a Bot that does not fill with paths.

## Alternatives considered

Chat-edited subject pages. Add `subject` to `lens_update` and let the turn rewrite the wiki directly. The caller would have to know revision heads, title collisions, and how two chats merge. That is a wide interface hiding almost nothing. It is today's method path with a new kind, and it is how a replaced note and a silent drop already lose work. Rejected.

A sleep-time agent with tools that rewrites the wiki. That hides a lot behind one loop, and it adds a second writer with the same tools as chat. It breaks the isolated tool-free helper rule and spends a paid call to rephrase text the turn already wrote. Rejected.

New core types for Listing, Process, Material, RealAsset, and PrivateHolding. The caller of extraction would learn a larger ontology, and the four-type extension budget would be spent on things the core should have handled. Venue on the match key, `product_role`, `GOES_INTO`, and `InstrumentFacet` hide that complexity inside one schema module. Rejected.

Leaving schema proposals in the settings screen until the user approves them. The interface is small and the capability is stalled, because the user cannot tell a good type from a bad one. Rejected in favor of validate-then-apply, forward only, with undo.

Auto-applying a schema and re-extracting history. That hides the queue and rewrites claims the user already saw. A bad type would poison old facts. Rejected. Forward-only apply keeps the graph interpretable.

## Implementation reconciliation

None. This candidate has not been implemented.

## Open questions and risks

- Is a filing that cites only Lens methods, and no World claim, worth keeping for a pure how-to conclusion? This design rejects it. If that cut drops real lessons, allow a filing whose only citations are `lens:` ids the turn read, and still forbid uncited sentences.
- Will four hypothesis cards a day feel like noise on a quiet watchlist? The cap is a constant in `fresh_paths`. Lowering it does not change the log.
- Should Undo on a schema card step back one version, or jump to the extension the user last saw in the Bot? This design steps back one version. A jump is a different intent.
- The biggest risk is file rate. Memory grows only when the model calls `answer_file` and the broker accepts it. A strict number check or a missed `capability_load` will look like a model that learned nothing. The commit-path test and the context-index eval are what show the difference between a rejected filing and a projector bug.

## Next implementation step

Bump `KNOWLEDGE_SCHEMA_VERSION` to 6 and add the filing log, the single `lens_kind` column, the standing-perspective row, mention edges, and the projection-applied table, with a Rust test that a version 5 folder is refused and a subject cannot be stored as a method.
