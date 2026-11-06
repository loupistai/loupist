# Memory that compounds through completed work

Candidate: Codex. Grounded at `50464082b9dd20582e6c937a7509f0bd4ea30a14`. This is a design package, not an implementation or a claim of measured improvement.

## Usage (caller's view)

### A research turn files its useful conclusion back

The user asks in Bot: “Does hybrid bonding change the bottleneck for HBM suppliers?” Bot handles the initial answer. A focused research session uses the same tools, with more space for evidence, calculations and competing explanations. The model sees its host-assigned role, Memory permissions, a compact Lens index, the current World vocabulary and relevant exact-read pointers.

Illustrative calls below use opaque handles returned by prior calls. `lens_update`'s replacement contract is specified in `types.md`; other existing read/save calls retain their contracts.

```ts
const hits = await lens_search({ query: "HBM hybrid bonding yield" });
const page = hits.hits.length ? await lens_read(hits.hits[0].readArgs) : null;
const facts = await world_search({ query: "hybrid bonding thermal yield", scope: "world" });
const graph = await world_read(facts.hits[0].readArgs);
const source = await world_read(Object.values(graph.sourceTargets)[0].readArgs);
// Read a new original through an admitted research connection; rr is its result_ref.
await capability_load({ id: "memory-save" });
const retained = await source_save({ result_ref: rr, title: "Bonding process study",
  coverage: "included_excerpt", publicationTime: { precision: "date", value: "2026-10-15" } });
await capability_load({ id: "lens-learning" });
await lens_update({
  target: page
    ? { edit: { recordId: page.recordId, revisionId: page.revisionId } }
    : { create: { kind: "subject", key: "topic:hbm-hybrid-bonding", title: "HBM and hybrid bonding" } },
  edits: [{ put: {
    key: "yield-bottleneck", expectedDigest: page?.sections["yield-bottleneck"]?.digest ?? null,
    heading: "Where the bottleneck may move",
    markdown: "Yield may move the constraint from stacking capacity toward surface preparation. This does not yet establish a supplier advantage.",
    basis: { synthesis: { refs: [source.readRef, retained.saveRef],
      uncertainty: "Production yields and supplier exposure remain unverified." } }
  }}],
  rationale: "Keep the mechanism and its unresolved test for future supplier research."
});
// Reply with the answer and original citations. No explicit commit tool.
```

Rust commits the answer, retained material, Lens revision and its receipt together. The receipt says “Updated HBM and hybrid bonding · 1 source saved” only for actual changes; the previously retained original is a read, not another save. World extraction follows asynchronously; the receipt initially says “Map processing queued.” The next session can discover and read this page even before extraction finishes. A working note can still hold the session's temporary plan; it is not the reusable conclusion.

### A scheduled run changes the same pages

The morning task finds a new source contradicting the production assumption. It exact-reads the page and originals, retains the new original, and calls the same `lens_update` with the read revision and section digest. It adds the competing report and replaces only the affected synthesis, preserving uncertainty and citations. If it has no durable finding, it writes no page; a successful run is not obliged to manufacture a lesson.

```ts
// In the scheduled turn, after loading memory-save and lens-learning:
const current = await lens_read({ recordId: topicId });
const prior = await world_read(current.evidenceTargets[0].readArgs);
const latest = await source_save({ result_ref: newFilingResult, title: "Production update",
  coverage: "included_excerpt", publicationTime: { precision: "date", value: "2026-11-05" } });
await lens_update({
  target: { edit: { recordId: current.recordId, revisionId: current.revisionId } },
  edits: [{ put: {
    key: "yield-bottleneck", expectedDigest: current.sections["yield-bottleneck"].digest,
    heading: "Where the bottleneck may move",
    markdown: "The production update reports a different constraint from the process study. The earlier supplier implication is unresolved; compare the production stage and measurement basis before carrying it forward.",
    basis: { synthesis: { refs: [prior.readRef, latest.saveRef],
      uncertainty: "These reports may concern different process stages." } }
  }}],
  rationale: "Preserve the contrary report and qualify the earlier inference."
});
```

```rust
// The existing completed-reply transaction, for Bot, research and schedules alike.
let receipt = memory.commit_completed(
    tx, completed_turn, acknowledged_reads, staged_memory
)?;
// Internally: source retention -> resolve evidence handles -> checked page changes
// -> one completed-work episode and review eligibility -> committed receipt.
// No provider call occurs inside this transaction.
```

Later, the existing tool-free helper reviews a bounded batch of these completed-work episodes, newly published World claims and exact relevant Lens sections. It may make a small page edit, suggest a method as a method page, or explain one evidence path as an unverified research lead. It may also publish nothing. This is one replacement for the present method-learning review, not another agent or independent writer.

Bot receives the scheduled result with the original commit receipt attached: “Updated the HBM yield assumption; these sources now disagree.” Opening it shows the changed section and both originals. A later helper result has its own receipt and a link to the originating task. Deliveries are host posts, not another model call.

### Bot can carry deep work without becoming the expert workspace

“Compare the three suppliers and test that explanation against their filings” calls for a sustained investigation. Bot can start one research session under the user's request:

```ts
await research_start({ requestKey: "hbm-supplier-comparison", title: "HBM supplier bottlenecks",
  task: "Compare the three named suppliers using dated filings; test the bonding-yield explanation, retain contrary evidence, and identify what remains unknown.",
  references: [page.readRef, source.readRef] });
// Returns a staged session link. On Bot reply commit: create session + durable launch row.
// Host starts the existing send_chat runtime, with the captured scope and permissions.
```

Bot displays a compact running card with Open and Stop. It does not move the user's selection or draft. The research session contains the full investigation, sources, Map, calculations, context controls and follow-up composer. Its completed result returns to Bot once, with the Memory receipt and Open research. A simple question stays in Bot. An unsolicited possible connection offers “Investigate”; it does not launch paid research automatically.

Onboarding asks “What would you like to understand?” with optional interest chips. Those chips select existing template IDs and deterministic vocabulary overlays; editing their display text does not change the selection. No one needs to configure a schema. “Also track packaging processes and their inputs” lets the agent inspect and extend the vocabulary through chat, with an ordinary receipt and Undo.

Memory presents two equally visible views: **Map / 지도** for evidence-backed connections, and **Lens / 렌즈** for accumulated pages. Lens has **Topics / 주제**, **Methods / 분석 방법**, **Your rules / 내 원칙**, and **Changes / 변경 내역**; the standing perspective appears with methods as the default approach. A page offers its evidence, links, backlinks, change history, “Correct in Bot,” Pause and Forget. Map keeps the existing graph, camera, replay, export and list fallback. Selecting a possible connection highlights its actual claim path; the inspector separates “What the sources say” from “What this could mean.” Originals open in the same inspector stack and Back restores the path and camera.

## Problem

Loupist already has a strong host transaction boundary, exact-read evidence receipts, retained originals and a local GraphRAG. Its missing unit is reusable research understanding. `note_save` replaces a per-chat working note; Lens only represents principles, perspective and methods; the helper reviews claims without the research that motivated them. Bot and expert sessions share execution but lack a meaningful role and a general handoff. The design must make useful work accumulate without turning the user into a librarian, promoting assistant prose into World facts, or weakening failed-turn rollback, permissions, cutoff filtering, shared-content isolation and paid-call limits.

## Shape

### Three durable records, one publication owner

The organizing structure is a **typed document plus a checked change set**. Sources and World retain their current roles. Lens becomes a true interlinked wiki; completed work is the input to its upkeep.

1. **EvidenceRef** identifies exact retained source spans or a frozen, acknowledged claim plus its original span bindings. It carries origin, recorded/publication/validity time and schema revision where relevant. A Lens link is navigation or prior analysis, never independent original evidence.
2. **LensPage** has one immutable identity and kind (`principle`, `perspective`, `method`, `subject`), small named Markdown sections, and immutable revisions. Sections distinguish sourced statements, synthesis, questions and practices. A subject can be a company, physical process, sector, value chain or thesis; a hypothesis is a labeled synthesis section with a mechanism and falsifier, not a new factual World edge.
3. **WorkEpisode** records a successfully completed task's answer pointer/digest, admitted evidence, Lens deliveries and edits, origin and cutoff. It is research context, never a Source. **MemoryReceipt** is the commit's factual result, not the model's assertion that it learned. The existing change, growth and usage records supply its detail.

Keep `KnowledgeWrite` as the publication owner. Foreground tools stage private change sets in `ToolCapture`; the helper owns a separate frozen review input/output. Both ask the same Lens publisher to validate and publish. Neither writes shared Markdown files, maintains a second index, or edits a live page while inference runs. SQLite remains canonical; the Markdown reader/export renders sections as one ordinary interlinked page. Per `separate-before-serializing-shared-state`, actor work is private until a short commit; per `boundary-discipline`, untrusted JSON is converted to checked changes before domain operations.

The supplied `research.md` motivates three choices: the LLM-wiki query/file-back loop, incremental source-grounded graph retrieval, and small provenance-bearing edits rather than repeated summary compression. This design borrows those mechanisms, not their reported benchmark gains. It uses the repository's existing graph and helper runtime rather than importing another agent framework.

The source audit supports reuse rather than a new framework:

| Current fact, checked in code | Consequence |
| --- | --- |
| `commands/chat_runtime/completion.rs:293–423` commits staged sources/Lenses, usage and reply before `graph.wake()` | Add episodes and receipt identities to this transaction; do not invent a second finalizer. |
| `commands/tool_executor/authority.rs:1192–1400` validates exact reads and stages Lens edits | Replace this tool's whole-document payload with section edits and retain acknowledgement checks. |
| `knowledge/learning.rs:97–167,522–680` gates on five claims and publishes directly | Replace its input/decision with research-aware upkeep; route publication through the same checked change-set method. |
| `knowledge/schema.rs:230–250,396–472` makes revisions and changes unique per record | Remove that restriction for Lens history; do not build a parallel history store. |
| `knowledge/lens.rs:334–390` rejects World references | Introduce typed evidence/navigation targets with scope checks, replacing string bans for legitimate references. |
| `knowledge/model.rs:589–616` already distinguishes publication, recording, validity and expiry | Preserve these fields; there is no need to rebuild the graph as a new temporal database. |
| `scheduled_tasks/mod.rs:527–546` invokes `send_chat` | Research handoffs reuse the same admitted Chat execution. |
| `chat/home.rs:100–150` deliberately creates deliveries without Memory receipts | Add receipt references to host delivery data, never infer them from its summary. |

Paths in this table are relative to `apps/loupist/src-tauri/src/`. The implementation must also replace the relevant contracts in the owning docs listed in `plan.md`.

### The wiki grows from research, not from transcript dumping

Change the harness instruction from discretionary general saving to a concrete completion discipline: discover relevant pages before substantive research; file back a reusable explanation, contradiction or method when the task supplies one; retain its originals; leave transient work in the working note. Ordinary questions and explicit one-offs remain cheap. No new mandatory finalization tool, forced extra model turn, or automatic paid repair is introduced.

The host creates an eligible WorkEpisode only after a completed, Memory-write-enabled, untainted turn with retained/read evidence, an explicit user correction, or a committed Lens edit. A task with only already-delivered Memory still qualifies; five new claims are no longer required. Host-only delivery messages, private turns, aborted turns and host-captured temporary mode do not enter review. The existing composer Memory-off flags are the enforceable temporary boundary. Natural-language one-off constraints travel with the original user request into review and are behavioral obligations tested in the eval, not a claim that Rust understands arbitrary language. Eligibility is observable; it is not a quality score. A completed episode with no page edit records `not_filed_this_turn`, not “learned.”

Upkeep replaces `knowledge/learning.rs`'s old method-only decision. Retain one pending review per Loupist, at most one helper call per attempt, a six-hour cooldown and four attempts/day. Freeze up to eight eligible episodes, five candidate paths and the exact sections/evidence that fit a 24 KiB input budget. Whole items are selected deterministically, oldest unreviewed first; never truncate a supporting span into an apparently complete original. Oversized inputs settle explicitly as insufficient coverage. Exact evidence is loaded by the host before inference, within the input budget, through the same permission and cutoff rules. The helper has no tools, credentials or filesystem access.

Process finite ready World work before selecting a review; do not require the entire ever-growing World queue to drain. A review freezes its World revision and event/episode watermarks. Later jobs belong to the next review. Publish at most three pages/twelve section operations and one research lead. No helper edit may change user principles or the standing perspective. A method requires a concrete corrective example or supported research experience, applicability and limits; it is offered through retrieval, never inserted as a new top-level instruction. Review publication itself is excluded from episode triggers, preventing self-reflection loops.

`NotSent` can wait for availability without spending an attempt. Once inference may have been sent, success, invalid output, timeout and crash consume the attempt; an uncertain/running-on-restart review settles without replay. A new evidence batch is new work, not a retry of the failed batch. Unprocessed items and failed coverage remain visible in Changes; no endless “learning” spinner. With no helper configured or enrichment disabled, foreground file-back still works and Sources remain readable; the receipt explicitly says which background work is unavailable.

### Small edits survive concurrent work

Each section has a stable key and content digest. `lens_update` names the exact page revision read, and each touched section's old digest (or absence). The host applies all operations in that page change only if every touched section still matches. Disjoint sections can merge after another task changes the page; overlapping changes become a durable deferred change, visible in the receipt and eligible for the next ordinary review with fresh evidence. No stale whole-page replacement and no inference while holding a transaction. The entire reply still commits; a deferred patch is not counted as a published page change.

Create uses a Loupist-scoped `PageKey`: a known entity identity, normalized topic key, or method key. Exact duplicate creates return the existing read target at staging. A race produces a deferred collision with that same target at commit; it never disappears. Display titles need not be unique. Semantic near-duplicates are a later lint concern, not a reason to silently merge different subjects.

Per `make-operations-idempotent`, `(origin turn/review, operation key)` identifies each change set and its receipt. Replaying a committed operation returns its original result. Undo appends an inverse revision only where the affected section heads still match the original result; otherwise it returns a conflict and opens a correction request. Forget suppresses the page and its pending changes and excludes it from retrieval/review. Pause is reversible and excludes automatic use. Neither is a claim that the original sources ceased to exist; reset and shared-pack removal preserve their current ownership rules. Restoring an old revision gives it a new recorded time.

Lens kind has one persisted authority in metadata. Decode content according to that kind. Remove payload `lensKind`, derive standing selection from the single `perspective` row, and use a unique partial index to enforce one standing perspective. `principle_save` remains the user-intent-specific tool, backed by the same page publisher; it is not an alternative way for autonomous upkeep to create rules.

### One extensible World vocabulary, exact identities

Keep the current sealed World snapshots, extracted claims, local vectors/FTS and open fallback types/relations. Add `Concept`, `Process` and `Material` to the existing core, and general-purpose `PART_OF`, `REQUIRES`, `PRODUCES`, `INSTANCE_OF` and `ISSUED_BY` signatures. `Asset` covers instruments, funds, tokens, physical and private assets using the existing `AssetClass` enum; an asset need not have a ticker. Scientific concepts and processes are first-class nodes, not forced into Organization or branded Product.

`world_schema.rs` owns a single core/overlay registry that generates the extraction schema, host vocabulary summary, validation rules and labels. Move the duplicated definitions out of `world_extract/ontology.rs` and schema-writer prompts. Presets bind to `LoupistTemplate`, never translated focus strings. This removes hand-synced lists per `encode-lessons-in-structure`.

Choose **automatic additive vocabulary changes after host validation**. The agent can `world_schema_read` and stage `world_schema_update`; upkeep can produce the same checked delta. Existing four-type/six-relation/three-field extension limits remain. Every new kind needs a definition, EN/KO labels, valid endpoint signatures and an example from retained evidence or an explicit user request. No core overrides, arbitrary executable fields, renames, type deletion or automatic entity merge. Ordinary users see “Now tracks fabrication processes” with View/Undo, not a pending ontology decision. Invalid or over-budget changes are refused; extraction keeps unclassified material in the open fallback, so learning does not wait for a perfect vocabulary.

Schema revisions are immutable and identified in extraction job inputs/publication receipts. Undo appends a new active overlay based on the previous version. Existing facts keep their recorded schema revision and inspectable labels; changes apply to future jobs, with no automatic paid re-extraction. A queued job uses its frozen schema. A manual “reprocess this source” remains an explicit paid action. The agent sees the current registry digest and bounded exact schema pointer; it must read before editing.

Give GraphNode optional typed identity keys: `(symbol, market)` listing, namespaced identifier (LEI/ISIN/etc.), `(chain, contract)` token, or namespaced real/private asset identifier. Distinguish an issuer from its traded instrument through a source-backed relation. The initial change does not require a universal symbology service. A shared resolver used by Map and asset pages returns Exact, Ambiguous or Unmatched. Unknown market or a name alone never auto-selects a followed listing; the inspector can offer explicit linking. This fixes `map.rs:189–194` and `assets/mod.rs:1562` at the common boundary.

### Discovery begins with inspectable paths

After a World publication, use the already available snapshot to build a transient adjacency index, then enumerate bounded two/three-claim paths joining a newly changed entity with an exactly resolved followed identity or a topic linked from active Lens. Preserve endpoint roles and relation direction in the path. Exclude withdrawn claims and retain contrary/disputed evidence as warnings. Use one algorithm for upkeep candidates and an optional `world_search` connections mode; there is no separate graph store or search service.

Rank deterministically by changed-edge recency, followed-endpoint coverage, cross-kind paths and low degree. Cap work at 2,000 examined claim incidences, 20 expansions per node and five candidates; disclose truncation. A candidate must include at least one newly published claim. Stable ordered path/claim-version hashes prevent repeat announcements: an automatically published lead uses the reserved subject-page key `topic:connection-<hash>`, which remains reserved even after Forget. No separate discovery ledger is required. A candidate is “new to this Loupist,” never “unknown to the market.”

The existing helper call may turn one path into a Lens synthesis with a mechanism, uncertainty, contrary evidence and a falsifier. It cannot fabricate a missing edge or call a path causal simply because it exists. Any suggestion to investigate returns to the normal research workflow. The Map highlights the real path and displays the hypothesis in its inspector; it does not add the hypothesis as an observed relationship. Retrieval exact-reads original premises before using the lead in an answer. No PPR, community summary hierarchy, GNN or autonomous research tournament is needed for this slice.

### The harness knows where it is

Freeze `TurnRole = Bot | Research { origin? } | Scheduled { task, run }` with the existing host context and cache seals. The role is host-derived, not a tool argument. Bot owns a short conversational answer, coordination and result delivery; research owns a durable investigative workspace; scheduled work completes its saved task without requesting interactive capabilities. All three use the same Memory policy and completion path.

`research_start` is Bot-only, once per turn, forbidden in scheduled and handed-off runs. It stages a self-contained task and exact reference pointers. Commit creates the new Chat and a uniquely keyed durable launch row; the existing run coordinator starts `send_chat` with `RunKind::Chat`, the captured model/scope, a fresh transcript, inherited Memory flags/cutoff and the existing unattended tool policy. Read acknowledgements do not transfer: the receiving turn must read originals again. This avoids a second research runtime and prevents delegated privilege gain. It is user-requested task execution, distinct from tool-free Memory upkeep.

Queued launches are safe to resume before a model request. Running launches on restart are uncertain and require an explicit retry. Parent abort creates neither Chat nor launch. Child stop/failure saves no Memory, leaves a visible status and never erases native file effects. There is no chain of handoffs. An app restart recovers receipts from persisted rows; a busy Bot receives the result after its active turn, following existing home delivery locking. Retain session-to-Bot scheduling through `schedule_handoff`.

### Receipts, retrieval and corrections have one meaning

The existing `memory_update` is extended, not replaced with another learning feed. It records actual created/revised/deferred items, reasons, exact revision IDs and origin task; async World/helper receipts link to that origin. Bot delivery references the child's receipt ID. The answer's current activity can show live processing status, while its original commit receipt stays immutable. The same records drive Lens Changes and Map What's new. “Used” means host-delivered, not causally influential. Include all four Lens kinds; this fixes dropped perspective receipts. Localize a conversation by its host role, fixing raw “Thread” in Memory without renaming stored conversations.

Context remains bounded. Replace newest-40-only Lens selection with the standing approach, user rules, and query-relevant topic/method pointers through existing search. Keep the current catalog and exact-read limits; do not load full wiki bodies automatically. `lens_read` returns rendered Markdown, typed section basis, links/backlinks, and original-evidence read targets. New entity links are same-Loupist validated navigation; opening a UI reader never grants model evidence authority.

Every new path uses the existing cutoff and shared-delivery wrappers. Keep current as-of semantics: a method, perspective or subject whose current head was written after the cutoff is omitted from model reads/context; retaining old revisions for history does not silently broaden temporal access. Principles still apply as today. New hypothesis creation time and every premise must pass the cutoff. Historical World claim evidence is frozen in the Lens revision with Source bindings because old World snapshots are pruned. Removed evidence yields “basis unavailable” and excludes that section from automatic reliance. Shared text remains untrusted data in helper prompts, page text and schema examples; shared evidence cannot rewrite local user rules or turn into newly extracted local facts.

### What can be called learning

| Product claim | Observable proof | Required behavior/eval |
| --- | --- | --- |
| “Saved a finding” | Committed subject revision, exact basis, operation receipt | Broker/commit: source save + section edit + reply commit atomically; stopped/failed turn yields none. |
| “Updated my understanding” | Old/new section digests, correction rationale and contradictory evidence | Two-session fixture changes one assumption; later retrieval uses new head and still exposes opposing sources. |
| “Learned a method” | Method revision and concrete experience/correction basis | Existing learning train/probe eval extended with held-out tasks; compare same model with frozen Memory on/off. Saving or delivery alone is not a gain. |
| “Found a possible connection” | Exact ordered claim path, source spans, novelty hash, mechanism and falsifier | Frozen graph fixture connects a process to a followed listing; reject wrong-market, unsupported, duplicate and future-dated paths. |
| “Research improves with use” | Better supported answers on held-out follow-up questions without more false factual assertions | Paired evaluation: useful citation support, contradiction handling, temporal correctness, multihop recall, tool/token cost; no promise of investment return. |

Use synthetic/masked entities and sources for the transfer/connection fixtures so model pretraining cannot answer from the company name. Evaluate a temporal holdout with all premises fixed at T. Broader market novelty and profitable alpha are not established by a saved page or a graph path.

### Interface depth

The agent still needs ordinary discovery/read, `source_save` and `lens_update`; it does not coordinate publication, index rebuilding, evidence freezing, receipt delivery or conflict merging. The only new tools are schema read/update and one research handoff; connections extend `world_search`. Rust exposes checked page changes, a completed-work commit and a review state machine, not SQLite rows or Pi transport objects. The renderer submits typed navigation/correction/undo intent through `tauriBridge.ts`. Per `minimize-reader-load`, trace the main path in three places: tool authority, completed-turn integration, knowledge owner. `types.md` specifies the ownership and outcome contracts.

## Tradeoffs accepted

- We accept small section-edit complexity in exchange for concurrent sessions retaining independent discoveries without a global model-time lock.
- We accept visibly deferred overlapping changes in exchange for never silently overwriting newer understanding or buying an automatic repair call.
- We accept additive, reversible schema evolution in exchange for users never needing to approve type definitions; semantic quality still requires evidence and evals.
- We accept a bounded path heuristic and one lead per upkeep call in exchange for useful, explainable discovery within local compute and existing helper budgets.
- We accept unchanged conservative cutoff semantics for Lens heads in exchange for keeping the first slice small; history is for explanation/undo, not a new point-in-time retrieval product.
- We accept retaining ephemeral working notes alongside durable subject pages because session continuation and reusable understanding have different lifetimes, authority and entry points.

## Alternatives considered

- **Everything becomes an append-only observation log; reconstruct the wiki at read time.** Excellent isolated writes, but callers/readers inherit synthesis, contradiction resolution and revision selection on every use. It hides storage and exposes the product's hardest decision repeatedly. Use episodes as review inputs, not as the wiki itself.
- **One background librarian rewrites complete pages from all transcripts.** A small foreground API hides cost but exposes timing, privacy, factual provenance and lost-update behavior to every consumer. It also makes useful Memory dependent on helper availability. Keep immediate, checked file-back and bounded delayed upkeep through one publisher.
- **Full page CAS plus user approval queues.** Smaller implementation, shallow product interface: ordinary users must merge prose, resolve titles and understand ontology proposals. Section edits plus host validation make the callee own those routine decisions.
- **A new graph engine with community reports and separate planner agents.** Hides sophisticated retrieval but adds another persistence and worker lifecycle boundary before current provenance and identity defects are fixed. The existing graph and local index already support the first useful discovery path.

## Open questions and risks

- Can cited synthesis and new methods pass adversarial entailment and poisoning fixtures, rather than merely cite valid IDs? This is the largest risk; types prove provenance and authority, not truth. The ship gate must include false-connection and hostile-source cases.
- Does one bounded review per six hours keep useful episodes from waiting too long at realistic load? Measure queue age and disclose skipped oversized inputs before increasing paid work; do not lift the budget speculatively.
- Can the section-based Lens change land across sharing, reset, readers and exact-read contracts within the estimated day? If it cannot, reduce visual polish and automatic lead explanation first; do not ship two Lens representations or a partially ported authority path.

## Next implementation step

Build the canonical Lens kind/revision/section model and a transaction test showing two independent section edits survive, an overlapping edit defers, and an aborted turn publishes nothing; then connect that model to the existing tools.

## Synthesis decision

Reserved for the arena coordinator. This candidate recommends the checked wiki change set as the base; it does not presume the other candidates' designs.

## Implementation reconciliation

No implementation has been performed. Accepted deviations must update all three files, especially outcome contracts, authority and unit ownership, before a unit is handed off.
