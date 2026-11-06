# Contracts and ownership

Companion to `design.md`. These are sketches, not copy-paste implementations. Omitted pre-existing fields stay unchanged. Rust newtype constructors and enum validation are private to their owning module; adapter DTOs do not become public domain APIs. No generated files are edited.

## Caller contracts before storage

There are three meaningful entry points, all through existing ownership:

```rust
// commands/tool_executor/authority.rs: parse tool JSON, enforce broker authority,
// resolve acknowledged handles, then stage the resulting domain command.
let change: CheckedLensChange = knowledge.check_lens_edit(scope, reads, input)?;
capture.stage_lens(delivery_id, change).await; // not durable

// commands/chat_runtime/completion.rs -> commands/knowledge/learning.rs.
// This method uses the caller's reply transaction; it must not open its own.
let receipt = write.commit_completed(completed, reads, staged)?;

// knowledge/learning.rs, reached by existing GraphSupervisor.
// A frozen review's evidence is its authority; it cannot issue tools.
let outcome = write.finish_review(attempt, helper_result, now)?;
```

The caller never saves an index, creates a growth event or reconstructs the receipt separately. All three entry points return explicit outcomes, including deferred changes and unavailable background work.

## Rust: pages and evidence

```rust
// knowledge/model.rs; constructors validate ownership, syntax, limits and digests.
struct LoupistId(String);
struct RecordId(String);
struct RevisionId(String);
struct Digest(String);
struct SectionKey(String);       // ASCII key, 1..64; heading remains localized prose
struct PageKey(String);          // entity:<node-id> | topic:<slug> | method:<slug>
// Host reserves perspective:standing and principle:<normalized-statement-hash>.
// Automatic leads use topic:connection-<path-hash>; their existing page-key index
// supplies novelty lookup, including forgotten/archived pages, without another ledger.
struct ChangeSetId(String);      // derived from actor origin + operation key
struct ReceiptId(String);
struct EpisodeId(String);        // deterministically bound to completed message

enum LensKind { Principle, Perspective, Method, Subject }

// Only knowledge_lens_metadata.lens_kind is persisted as kind.
// No payload lensKind; no standing-selection table; no renderer inference of kind.
struct LensPage {
    owner: LoupistId,
    id: RecordId,
    kind: LensKind,
    key: PageKey,
    head: RevisionId,
    title: String,
    content: LensContent,
}

enum LensContent {
    Principle(UserPrinciple),       // existing statement/why/example + verified origin
    Perspective(MethodDocument),
    Method(MethodDocument),
    Subject(SubjectDocument),
}

// Serialized revision payloads have no variant tag. Metadata selects the decoder.
// Domain variants are constructed by that decoder, never by serde's untagged guess.
struct MethodDocument {
    sections: BTreeMap<MethodSection, WikiSection>,
}
enum MethodSection { Markdown, Applicability, Assumptions, Procedure, Principles, Limitations }
struct SubjectDocument {
    sections: BTreeMap<SectionKey, WikiSection>,
    // Initial insertion order is retained; concurrent insertions tie-break by section key.
    order: Vec<SectionKey>,
}
struct WikiSection {
    heading: String,
    markdown: String,
    basis: SectionBasis,
    // Derived from all fields, including basis; not supplied as new content by the model.
    digest: Digest,
}

enum SectionBasis {
    Sourced { evidence: NonEmpty<RetainedEvidence> },
    Synthesis {
        premises: NonEmpty<RetainedEvidence>,
        uncertainty: NonEmptyText,
        lead: Option<ResearchLead>,
    },
    Question,                       // contains no established assertion
    Practice { basis: PracticeBasis },
}
enum PracticeBasis {
    UserTeaching(VerifiedUserIntent), // teaching/correction bound to actual user message
    ResearchExperience(NonEmpty<EpisodeEvidence>),
}
struct EpisodeEvidence {
    episode: EpisodeId,
    original_basis: Vec<RetainedEvidence>,
    example: String,
    limitation: String,
}

// A World claim is attributed analysis of Sources, not an original.
enum RetainedEvidence {
    Source(ExactSourceSpan),
    Claim(FrozenClaimEvidence),
}
struct ExactSourceSpan {
    owner: LoupistId,
    record: RecordId,
    revision: RevisionId,
    capture: String,
    span: String,
    selected_digest: Digest,
    origin: EvidenceOrigin,
    // Reuse the current PublishedCapture/TimePoint semantics and cutoff evaluator.
    publication: Option<TimePoint>,
    recorded_at_ms: i64,
}
enum EvidenceOrigin { External, UserProvided, SharedPack { pack_id: String } }
struct FrozenClaimEvidence {
    world: RecordId,
    world_revision: RevisionId,
    claim: GraphClaim,              // exact proposition, roles, status and time fields
    claim_digest: Digest,
    schema_revision: SchemaRevisionId,
    sources: NonEmpty<ExactSourceSpan>,
}

// Historical World revisions may be pruned; the captured claim and original spans
// remain inspectable. The current World status is a separate checked lookup.
// A frozen claim never becomes new extraction input merely because Lens cites it.
enum WikiLink {
    Page { record: RecordId },      // [title](lens:rec_...) follows current head
    Entity { node: String },        // [title](entity:node_...) is navigation only
}
// Markdown parser derives link rows. No second caller-maintained links array.
// Evidence citations are rendered from SectionBasis, with exact Source actions.

struct ResearchLead {
    path: NonEmpty<ClaimStep>,      // 2..3 steps; a cited subset cannot invent an edge
    novelty_key: Digest,            // ordered entity/claim-version path
    mechanism: NonEmptyText,
    falsifier: NonEmptyText,
    contrary: Vec<RetainedEvidence>,
    status: LeadStatus,
}
enum LeadStatus { Unverified, Contradicted, SupportedByLaterEvidence }
struct ClaimStep {
    claim: FrozenClaimEvidence,
    from_node: String,
    from_role: String,
    to_node: String,
    to_role: String,
}
```

`SupportedByLaterEvidence` requires a later sourced revision; it is not “profitable” or “market novel.” All helper-created leads start Unverified. The status, mechanism and falsifier belong to the synthesis section and participate in its digest. Ordinary factual page sections require original basis; practices require real examples and cannot grant runtime permissions. The host can validate these structures and evidence availability, but semantic entailment is evaluated rather than falsely promised by the type system.

Limits: 24 sections/page, 32 KiB rendered body/page, 8 KiB/section, 6 operations/tool call; retain the existing six method slots and principle bounds. Order is explanatory, never an instruction-precedence mechanism. A full rewrite is expressed as a bounded set of section replacements; there is no second full-document tool. Principle edits remain under `principle_save` and existing user-origin validation.

## Rust: changes, conflict outcomes and transactions

```rust
// These checked types can only be constructed in the knowledge owner after the
// adapter resolves current-turn acknowledgements and same-Loupist ownership.
enum LensTarget {
    New { key: PageKey, kind: LensKind, title: String },
    Read { page: ReadLensRevision },
}
// Only the principle adapter with VerifiedUserIntent can construct a new Principle.
enum WritableLensKind { Perspective, Method, Subject } // tool DTO subset, not persistence
struct ReadLensRevision {
    record: RecordId,
    revision: RevisionId,
    section_digests: BTreeMap<SectionKey, Digest>,
    authority: LensEditAuthority,  // interactive exact ACK or frozen helper input
}
enum SectionEdit {
    Put { key: SectionKey, expected: Option<Digest>, section: WikiSection },
    Remove { key: SectionKey, expected: Digest },
}
struct CheckedLensChange {
    id: ChangeSetId,
    target: LensTarget,
    patch: LensPatch,
    title: Option<TitleEdit>,      // optional CAS on current title digest
    rationale: NonEmptyText,
    origin: ChangeOrigin,
}
enum LensPatch {
    Wiki(NonEmpty<SectionEdit>),
    Principle { value: UserPrinciple, authority: VerifiedUserIntent },
}
// Principle replacement keeps its existing whole-rule exact-revision check; it cannot
// be manufactured with wiki section operations or helper authority.
struct TitleEdit { expected: Digest, value: String }
enum ChangeOrigin {
    Turn { chat: String, message: String },
    Upkeep { review: String, episodes: Vec<EpisodeId> },
    UserAction { action_id: String },
    Import { pack_id: String },
}
enum LensChangeOutcome {
    Published { record: RecordId, before: Option<RevisionId>, after: RevisionId },
    Unchanged { record: RecordId, revision: RevisionId },
    Deferred { draft_id: String, record: RecordId, reason: DeferredReason },
}
enum DeferredReason { SectionChanged, CreateCollision, TitleChanged, ScopeChanged }

impl KnowledgeWrite<'_> {
    // Tool adapters call this with parsed, untrusted input and turn-local authority;
    // helper completion uses the same validation with its frozen review authority.
    pub(crate) fn check_lens_edit(&self, scope: &MemoryScope,
        reads: &AcknowledgedReads, input: ProposedLensEdit)
        -> KnowledgeResult<CheckedLensChange> { todo!() }

    // Checks -> entire page patch applies OR entire page patch defers; never partial.
    // Changed untouched sections do not prevent publication. Same id is idempotent.
    pub(crate) fn publish_lens_change(
        &self, change: CheckedLensChange, now: i64,
    ) -> KnowledgeResult<LensChangeOutcome> { todo!() }

    // Used by both UI correction/undo intents and tool commits, not a second writer.
    pub(crate) fn undo_lens_change(
        &self, action: VerifiedUndo, now: i64,
    ) -> KnowledgeResult<LensChangeOutcome> { todo!() }

    // CompletedTurn cannot be constructed from failed/stopped replies.
    // Uses the reply's existing SQLite transaction and finalization/cancellation gate.
    pub(crate) fn commit_completed(
        &self, turn: CompletedTurnMemory, reads: AcknowledgedReads,
        staged: StagedMemory,
    ) -> KnowledgeResult<MemoryReceipt> { todo!() }
}

struct CompletedTurnMemory {
    owner: LoupistId,
    chat_id: String,
    message_id: String,
    role: TurnRole,
    permission: CapturedMemoryPermission,
    as_of: Option<Cutoff>,
    answer_digest: Digest,
    memory_revision: u64,           // existing finalization/reset epoch
    completed_at_ms: i64,
    explicit_temporary: bool,
}
// StagedMemory reuses Sources/working note/principles/asset changes, and adds the
// checked Lens changes and schema deltas. TurnCapture remains per-turn state.
struct StagedMemory { /* existing fields; lenses: Vec<CheckedLensChange>, schema: ... */ }

struct WorkEpisode {
    id: EpisodeId,
    owner: LoupistId,
    chat_id: String,
    message_id: String,
    role: TurnRole,
    answer_digest: Digest,         // bounded text loaded from completed transcript
    user_request_digest: Digest,   // load original intent too, preserving one-off constraints
    evidence: Vec<RetainedEvidence>,
    lens_reads: Vec<LensReceipt>,
    committed_changes: Vec<ChangeSetId>,
    cutoff: Option<Cutoff>,
    created_at_ms: i64,
    state: EpisodeState,
}
enum EpisodeState {
    Pending,
    Reviewed { review: String },
    Excluded { reason: EpisodeExclusion },
}
enum EpisodeExclusion { Temporary, RemovedBasis, TooLarge, NoReusableEvidence }

struct MemoryReceipt {
    id: ReceiptId,
    origin: ChangeOrigin,
    changes: Vec<MemoryChange>,    // existing source/asset/journal changes + typed Lens/schema
    processing: Vec<ProcessingDisposition>,
    episode: Option<EpisodeId>,
    created_at_ms: i64,
}
enum ProcessingDisposition {
    Queued { job_id: String, target: ProcessingTarget },
    Unavailable { target: ProcessingTarget, reason: ProcessingUnavailable },
    NotNeeded { target: ProcessingTarget },
}
enum ProcessingTarget { World, LensUpkeep }
enum ProcessingUnavailable { Disabled, NoModel, NoExtractedText, SharedSource, Paused }
```

Commit pseudocode, replacing the existing implementation rather than wrapping several competing publishers:

```text
1. Finalization gate verifies completed reply, captured permissions, cancellation and epoch.
2. If saving is off or the turn is tainted: suppress all staged Memory and episode creation.
3. Retain staged Sources; resolve acknowledged saveRefs/readRefs to exact evidence.
4. Recheck source ownership/removal, Lens controls, schema base and touched section digests.
5. Publish each valid page change or persist its explicit deferred result; use one transaction.
6. Record actual changes/usage; create one eligible WorkEpisode and pending review marker.
7. Freeze MemoryReceipt with reply; enqueue existing Source World jobs and processing outcomes.
8. COMMIT reply + Memory; only then wake existing graph/upkeep and research handoff dispatch.
```

A malformed/forged evidence input fails at the broker. A discovered race at commit has a typed Deferred outcome. An infrastructure/SQLite failure rolls back the entire completed-reply transaction, with the existing failure reporting path. No receipt says “saved” before COMMIT. Exact same change-set replay returns its original outcome rather than minting another revision/event.

## Rust: review state machine and discovery

```rust
// knowledge/learning.rs owns episodes and reviews; no second maintenance scheduler.
enum ReviewState {
    Pending,
    Claimed { attempt: String, input_digest: Digest, started_at_ms: i64 },
    Finished { outcome: ReviewOutcome },
}
enum ReviewOutcome {
    Published { receipt: ReceiptId },
    NoChange { reason: String },
    Deferred { reason: String },
    Failed { reason: String, may_have_been_paid: bool },
}
struct ReviewInput {
    episodes: Vec<WorkEpisode>,
    evidence: Vec<FrozenReviewEvidence>,
    pages: Vec<ReadLensRevisionAndSections>,
    new_claims: Vec<FrozenClaimEvidence>,
    paths: Vec<ConnectionCandidate>,
    schema: SchemaSnapshot,
    principles: Vec<WrappedPrinciple>,
    memory_revision: u64,
    through_episode: i64,
    through_growth_event: i64,
}
struct ReviewDecision {
    pages: Vec<ProposedLensEdit>,   // <=3; converted through same validation as tool inputs
    schema_addition: Option<ProposedSchemaAddition>,
    skipped: Vec<ReviewSkip>,
}
enum ReviewClaim { Ready(ReviewAttempt), WaitUntil(i64), Unavailable(String), Empty }

impl KnowledgeWrite<'_> {
    pub(crate) fn claim_review(&self, owner: &LoupistId, now: i64)
        -> KnowledgeResult<ReviewClaim> { todo!() }
    pub(crate) fn finish_review(&self, attempt: ReviewAttempt,
        result: InferenceReply, now: i64) -> KnowledgeResult<ReviewOutcome> { todo!() }
}

// knowledge/connections.rs: pure graph-selection algorithm, no provider or storage writes.
struct ConnectionQuery {
    anchors: Vec<ResolvedEntity>,
    changed_claim_ids: BTreeSet<String>,
    cutoff: Option<Cutoff>,
}
struct ConnectionCandidate { steps: Vec<ClaimStep>, novelty_key: Digest, score: PathRank }
struct ConnectionPage { candidates: Vec<ConnectionCandidate>, truncated: bool, scanned: usize }
fn connection_candidates(world: &VisibleWorld, query: &ConnectionQuery,
    seen: &BTreeSet<Digest>) -> ConnectionPage { todo!() }
```

Selection is not a model call. `VisibleWorld` already applies cutoff, source visibility and ownership. The adjacency map is built once per selected snapshot/query (or reused from the current graph index); no new canonical store. Index by node ID to claim incidences, preserve roles, bound the frontier and total scans, and skip cyclic paths. Retain the existing hybrid search when `world_search` mode is omitted. A connections search is discovery-only and returns exact readArgs for every path claim. Evidence authority requires subsequent acknowledged exact reads.

Before helper publication, recheck current Memory permissions/epoch, active principles, evidence availability, page pause/removal and schema head. A changed principle set defers the review rather than publishing under outdated instructions. Disjoint page edits can merge; conflicting sections defer. The helper's output consumes no new foreground budget, but has its own explicit three-page/twelve-operation cap. Paid-failure states cannot transition back to Claimed automatically. Removing a source/page invalidates pending inputs and suppresses affected automatic reads even when an immutable historical revision remains.

## Rust: one World registry and identity resolver

```rust
// knowledge/world_schema.rs owns both core and overlays.
struct SchemaRevisionId(String);
struct SchemaSnapshot {
    revision: SchemaRevisionId,
    core_digest: Digest,
    overlay: WorldExtension,       // existing bounded definition shapes
}
struct WorldVocabulary { core: CoreRegistry, active: SchemaSnapshot }
struct CheckedSchemaAddition {
    base: ReadSchemaSnapshot,
    types: Vec<ExtensionType>,
    relations: Vec<ExtensionRelation>,
    examples: NonEmpty<SchemaExample>,
    origin: ChangeOrigin,
    rationale: String,
}
enum SchemaExample { Evidence(RetainedEvidence), UserRequest(VerifiedUserIntent) }
enum SchemaChangeOutcome { Applied(SchemaRevisionId), Unchanged(SchemaRevisionId), StaleBase }

impl WorldVocabulary {
    fn extraction_schema(&self) -> ExtractionSchema { todo!() }
    fn discovery_summary(&self, budget: usize) -> VocabularyPointer { todo!() }
    fn preset(template: LoupistTemplate) -> WorldExtension { todo!() }
}
impl KnowledgeWrite<'_> {
    pub(crate) fn add_schema(&self, addition: CheckedSchemaAddition, now: i64)
        -> KnowledgeResult<SchemaChangeOutcome> { todo!() }
    pub(crate) fn revert_schema(&self, undo: VerifiedSchemaUndo, now: i64)
        -> KnowledgeResult<SchemaChangeOutcome> { todo!() }
}

// knowledge/model.rs: optional identity keys are part of canonical GraphNode.
enum EntityIdentity {
    Listing { symbol: NormalizedSymbol, market: NormalizedMarket },
    Identifier { namespace: String, value: String },
    Token { chain: String, contract_or_native_id: String },
}
// Physical/private assets can have an authoritative namespaced Identifier, or none.
// Display address/name, ticker alone and issuer similarity do not create identities.
struct AssetProperties { class: Option<assets::AssetClass> } // existing enum, no string list
enum IdentityMatch { Exact(ResolvedEntity), Ambiguous(Vec<String>), Unmatched }
fn resolve_followed(item: &MapContextItem, index: &WorldIdentityIndex) -> IdentityMatch { todo!() }
```

Core registry entries define display labels, extraction fields and allowed relation endpoint signatures together. `AssetClass` values are generated from the Rust enum into the extraction declaration; if enum metadata is unavailable, one declarative macro must generate both the enum and its value list rather than keeping a second literal vocabulary. Extend the core from the existing eight types to eleven with Concept, Process and Material; retain `Entity` and specific open relation names as fallback. `USES` accepts process/material/concept targets where appropriate; PART_OF/REQUIRES/PRODUCES/INSTANCE_OF/ISSUED_BY have explicit directed signatures. Validation does not accept arbitrary class names as proof of identity.

`WorldIdentityIndex` maps full keys to **sets** of node IDs, not a single first match. Both asset-page binding and Map followed markers use this resolver. Normalize case only for identity systems where case is irrelevant; never uppercase token contracts blindly. Broad market aliases such as US are not silently equated with a specific exchange. Unknown/ambiguous matches remain unresolved. Preserve existing name aliases for search, independent of followed-identity binding.

## Tool contracts: sole definitions in `agent/tool-catalog.json`

Existing `source_save`, `note_save`, `principle_save`, reads, Journal and asset tools keep their permission semantics. Remove the old whole-document `lens_update` schema and its implementation in the same unit. Every object below uses `additionalProperties: false`; `$defs` names are sketch abbreviations to be expanded or locally referenced in the catalog. All IDs are opaque bounded strings, never authority on their own.

```json
{
  "name": "lens_update", "method": "lens.update",
  "permission": "memory_write", "component": "lens-learning",
  "parameters": {
    "type": "object", "additionalProperties": false,
    "required": ["target", "edits", "rationale"],
    "properties": {
      "target": {"oneOf": [
        {"type":"object","additionalProperties":false,"required":["create"],"properties":{
          "create":{"type":"object","additionalProperties":false,"required":["kind","key","title"],"properties":{
            "kind":{"enum":["subject","method","perspective"]},
            "key":{"type":"string","maxLength":160},"title":{"type":"string","minLength":1,"maxLength":180}
          }}
        }},
        {"type":"object","additionalProperties":false,"required":["edit"],"properties":{
          "edit":{"type":"object","additionalProperties":false,"required":["recordId","revisionId"],"properties":{
            "recordId":{"$ref":"#/$defs/id"},"revisionId":{"$ref":"#/$defs/id"}
          }}
        }}
      ]},
      "edits":{"type":"array","minItems":1,"maxItems":6,"items":{"oneOf":[
        {"$ref":"#/$defs/sectionPut"},{"$ref":"#/$defs/sectionRemove"}
      ]}},
      "title":{"$ref":"#/$defs/titleCompareAndSet"},
      "rationale":{"type":"string","minLength":1,"maxLength":1200}
    }
  }
}
```

`sectionPut = {put:{key, expectedDigest: string|null, heading, markdown, basis}}`; `sectionRemove = {remove:{key, expectedDigest:string}}`. `basis` is exactly one of `{sourced:{refs:[readRef|saveRef]}}`, `{synthesis:{refs:[...],uncertainty,lead?}}`, `{question:{}}`, `{practice:{teachingMessageId? , episodeIds? , example, limitation}}`. Source/claim reference lists are bounded to 8/section and 32/change; a `lead.path` contains exact claim references from acknowledged World views, not arbitrary node pairs. Practice input must resolve to an actual host-admitted teaching or completed episode. The host does not trust an `episodeIds` list to prove a successful outcome. Subject pages forbid procedure/rule impersonation as typed practices; methods use their fixed six section keys. A purpose is exposed through kind; no silent default to method.

Tool results:

```ts
// Existing lens_read extended for the new document, separate from the UI DTO.
type LensReadToolResult = {
  recordId: string; revisionId: string; readRef: string; kind: LensKind;
  markdown: string; sections: Record<string, { digest: string; heading: string;
    markdown: string; basis: SectionBasisView }>;
  evidenceTargets: { readArgs: SourceReadArgs }[];
  links: NavigationLink[]; backlinks: NavigationLink[];
};
type LensStageResult =
  | { status: "staged"; durable: false; changeId: string; recordId: string }
  | { status: "existing"; durable: false; reason: "key_taken"; readArgs: LensReadArgs }
  | { status: "refused"; durable: false; reason: "paused" | "forgotten" | "invalid_basis" };
// Commit can still defer a staged change if the page changes before the reply commits.
```

Two schema tools:

```json
[
  {"name":"world_schema_read","method":"world.schema_read","permission":"memory_read","component":null,
   "parameters":{"type":"object","additionalProperties":false,"properties":{}}},
  {"name":"world_schema_update","method":"world.schema_update","permission":"memory_write","component":"lens-learning",
   "parameters":{"type":"object","additionalProperties":false,"required":["baseReadRef","types","relations","examples","rationale"],"properties":{
     "baseReadRef":{"$ref":"#/$defs/id"},
     "types":{"type":"array","maxItems":2,"items":{"$ref":"#/$defs/extensionType"}},
     "relations":{"type":"array","maxItems":3,"items":{"$ref":"#/$defs/extensionRelation"}},
     "examples":{"type":"array","minItems":1,"maxItems":8,"items":{"$ref":"#/$defs/schemaExample"}},
     "rationale":{"type":"string","minLength":1,"maxLength":800}
   }}}
]
```

Read returns the exact core/overlay, schema revision, digest and an edit-only `readRef`; it grants no factual evidence receipt. Under a cutoff, an overlay recorded later is omitted with a disclosure, rather than delivering future examples or definitions. Update is additive only, stages until completion and returns Applied/Unchanged/StaleBase at commit; a historical/omitted overlay is not current edit authority. Full active overlay caps remain four types/six relations/three optional fields per type. Definitions and bilingual labels are bounded inert data. UI Undo goes through `worldSchemaRevert` with the expected active revision; automatic upkeep cannot revert or remove types. Chat corrections can add future vocabulary only within these rules; renaming, redefining existing kinds and destructive recategorization are later work.

`world_search` adds optional `mode: "relevant" | "connections"` (default relevant); connections applies only to `scope:"world"`. The host chooses followed anchors and current-view visibility. Output is path discovery with original `readArgs`, `truncated` and coverage; it is not an acknowledged claim read.

`research_start`:

```json
{"name":"research_start","method":"research.start","permission":"always","component":null,
 "parameters":{"type":"object","additionalProperties":false,"required":["requestKey","title","task"],"properties":{
   "requestKey":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,64}$"},
   "title":{"type":"string","minLength":1,"maxLength":180},
   "task":{"type":"string","minLength":1,"maxLength":8000},
   "references":{"type":"array","maxItems":8,"uniqueItems":true,"items":{"$ref":"#/$defs/id"}}
 }}}
```

The host binds Loupist/parent message/role/permissions/model/cutoff and checks references. No arbitrary destination, credentials, connector grants, role, immediate execution flag or schedule enters tool JSON. `always` means independent of Memory, not unrestricted: this tool only exists for interactive Bot, requires the live user-request turn, and has a broker cardinality of one. It stages a session start, never schedules recurrence. In private work, only the user's request/attachments and permitted transient input can carry over; no saved Memory pointer is introduced.

The handoff tool returns `{status:"staged", durable:false, handoffId, sessionId}`. IDs are provisional until the parent reply commits; UI Open/Stop controls come from the committed host card, not streamed model prose. A staging response never starts a worker by itself.

Budget invariant: keep `source_save <=16`, `note_save <=1`, and the shared `lens_update + principle_save + world_schema_update <=8`; extend the existing `TransactionCardinality` counter, do not create eight extra schema writes. Existing ordinary read/tool/time budgets remain. Add schema read/update to the explicit unattended allowlist after their broker tests. Keep principle writes, scheduling mutations and `research_start` unavailable there. No load operation grants new authority.

## Rust: Bot/session handoff

```rust
// agent_harness.rs; derived from captured host state, included in cache sealing.
enum TurnRole {
    Bot,
    Research { origin: Option<HandoffId> },
    Scheduled { task_id: String, run_id: String },
}
struct HandoffId(String);
struct ResearchHandoff {
    id: HandoffId,
    owner: LoupistId,
    parent_chat: String,
    parent_message: String,
    request_key: String,
    child_chat: String,
    task: String,
    references: Vec<PermittedReferencePointer>, // no inherited ACK authority
    execution: CapturedResearchScope,            // contains permissions/cutoff, not secrets
    state: HandoffState,
}
enum HandoffState {
    Queued,
    Running { run_id: String },
    Succeeded { message_id: String, receipt: Option<ReceiptId> },
    Failed { reason: String, uncertain: bool },
    Stopped,
}

// chat/handoff.rs owns durable launch/settlement; the existing run coordinator owns lifetime.
fn commit_handoff(tx: &Transaction, parent: CompletedBotTurn, draft: CheckedHandoff)
    -> Result<ResearchHandoff, CommandError> { todo!() }
async fn dispatch_handoff(state: &AppState, id: HandoffId)
    -> Result<(), CommandError> { todo!() }
// TODO: CAS Queued->Running; acquire existing RunKind::Chat lease; call send_chat;
// settle from terminal event; post a host receipt after child completion.
// Startup: Queued is resumable, Running becomes Failed{uncertain:true}, never auto-retry.
```

No new scheduler or `RunKind` is necessary. Extend the internal unattended execution binding to `Scheduled(run_id) | Research(handoff_id)` so tool restrictions and refusal recording are shared. The scheduled task's own trigger/state remain in `scheduled_tasks`; handoffs have no fake schedules. `chat/home.rs` selects undelivered handoff outcomes as well as existing runs, using deterministic message IDs. Idle/Bot-load/post-completion delivery reuses the current chat-mutation lock; it never mutates a busy Bot or changes another window's selection.

## SQLite changes

`KNOWLEDGE_SCHEMA_VERSION = 6` for this candidate; refuse old folders as today. No migration. SQL here shows changed/new canonical structures, not a second database. Names may retain current ones when their meaning stays coherent.

```sql
-- Existing knowledge_record_revisions: remove UNIQUE(record_id), keep revision_id PK.
-- Lens append preserves revisions. Existing Source/World publication keeps its current
-- pruning policy; do not accidentally retain full World snapshots forever.
CREATE INDEX knowledge_revisions_record_time
  ON knowledge_record_revisions(record_id, created_at_ms, revision_id);

-- Replace knowledge_lens_metadata, rather than adding another metadata table.
CREATE TABLE knowledge_lens_metadata (
  record_id TEXT PRIMARY KEY REFERENCES knowledge_records(record_id),
  loupist_id TEXT NOT NULL,
  lens_kind TEXT NOT NULL CHECK(lens_kind IN ('principle','perspective','method','subject')),
  page_key TEXT NOT NULL,
  title_fold TEXT NOT NULL,       -- derived search field, NOT unique
  principle_statement_fold TEXT,
  archived_at_ms INTEGER,
  learned INTEGER NOT NULL CHECK(learned IN (0,1)),
  updated_at_ms INTEGER NOT NULL,
  UNIQUE(loupist_id, page_key)
) STRICT;
CREATE UNIQUE INDEX knowledge_one_perspective ON knowledge_lens_metadata(loupist_id)
  WHERE lens_kind='perspective';
CREATE UNIQUE INDEX knowledge_principle_active_statement
  ON knowledge_lens_metadata(loupist_id, principle_statement_fold)
  WHERE lens_kind='principle' AND archived_at_ms IS NULL;
-- Page keys remain reserved while forgotten to prevent automatic recreation.
-- Display title is owned by knowledge_records.title; read it through the join.

-- Existing knowledge_lens_changes becomes append-only: remove UNIQUE(record_id).
-- Add change_set_id UNIQUE, before_revision_id, outcome_json and parent receipt_id.
-- Existing knowledge_lens_drafts stores deferred section operations/base digests;
-- replace its whole-document payload; one row per change_set_id, not latest-wins.
-- Existing knowledge_lens_links gains target_kind ('lens','entity'), section_key and
-- target_id. Derived from current head only; index (loupist_id,target_kind,target_id).
-- Evidence refs live in canonical revision payloads. This derived table indexes the
-- current heads; it can always be rebuilt and never authorizes a factual read.
CREATE TABLE knowledge_lens_dependencies (
  loupist_id TEXT NOT NULL,
  record_id TEXT NOT NULL REFERENCES knowledge_records(record_id),
  revision_id TEXT NOT NULL REFERENCES knowledge_record_revisions(revision_id),
  section_key TEXT NOT NULL,
  target_kind TEXT NOT NULL CHECK(target_kind IN ('source_span','world_claim')),
  target_record_id TEXT NOT NULL,
  target_object_id TEXT NOT NULL,
  target_digest TEXT NOT NULL,
  PRIMARY KEY(record_id,section_key,target_kind,target_record_id,target_object_id,target_digest)
) STRICT;
CREATE INDEX knowledge_lens_dependents ON knowledge_lens_dependencies
  (loupist_id,target_kind,target_record_id,target_object_id);

CREATE TABLE knowledge_work_episodes (
  episode_seq INTEGER PRIMARY KEY,
  episode_id TEXT NOT NULL UNIQUE,
  loupist_id TEXT NOT NULL,
  chat_id TEXT NOT NULL,
  message_id TEXT NOT NULL,
  body_json TEXT NOT NULL,        -- bounded evidence refs, deliveries, origin, cutoff
  answer_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('pending','reviewed','excluded')),
  review_id TEXT,
  exclusion_reason TEXT,
  UNIQUE(loupist_id, message_id)
) STRICT;
CREATE INDEX knowledge_episode_queue ON knowledge_work_episodes(loupist_id,state,episode_seq);

-- Replace knowledge_learning_reviews, retaining rate-limit timestamps/usage fields.
-- through_event + through_episode define the frozen input, not a mutable global batch.
-- status = pending|claimed|published|no_change|deferred|failed.
-- input_digest, input_json, output_json, settings_json, usage_json, receipt_id,
-- started/finished times and paid_disposition support recovery and accountability.
-- Keep one active row per Loupist and call_budget BETWEEN 0 AND 1.

CREATE TABLE knowledge_memory_receipts (
  receipt_id TEXT PRIMARY KEY,
  loupist_id TEXT NOT NULL,
  origin_kind TEXT NOT NULL CHECK(origin_kind IN ('turn','upkeep','schema','user_action','world')),
  origin_id TEXT NOT NULL,
  body_json TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  UNIQUE(loupist_id,origin_kind,origin_id)
) STRICT;
-- ChatMessage.memory_update is a projection/reference to this exact committed receipt.
-- No second independently authored report; eliminate current duplicated report assembly.

CREATE TABLE knowledge_world_schema_revisions (
  revision_id TEXT PRIMARY KEY,
  loupist_id TEXT NOT NULL,
  parent_revision_id TEXT,
  core_digest TEXT NOT NULL,
  overlay_json TEXT NOT NULL,
  basis_json TEXT NOT NULL,
  rationale TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE knowledge_world_schema_heads (
  loupist_id TEXT PRIMARY KEY,
  revision_id TEXT NOT NULL REFERENCES knowledge_world_schema_revisions(revision_id)
) STRICT;
CREATE INDEX knowledge_world_schema_history ON knowledge_world_schema_revisions
  (loupist_id,created_at_ms,revision_id);
-- Replace GraphProcessingSettings.extension with a resolved runtime snapshot derived
-- from this head, never a separately persisted second copy. Jobs freeze schema revision.
-- Delete knowledge_schema_proposals and knowledge_standing_selection.

CREATE TABLE chat_research_handoffs (
  handoff_id TEXT PRIMARY KEY,
  loupist_id TEXT NOT NULL,
  parent_chat_id TEXT NOT NULL,
  parent_message_id TEXT NOT NULL,
  request_key TEXT NOT NULL,
  child_chat_id TEXT NOT NULL UNIQUE,
  state TEXT NOT NULL CHECK(state IN ('queued','running','succeeded','failed','stopped')),
  payload_json TEXT NOT NULL,
  result_json TEXT,
  created_at_ms INTEGER NOT NULL,
  UNIQUE(loupist_id,parent_message_id,request_key)
) STRICT;
CREATE INDEX chat_handoff_queue ON chat_research_handoffs(loupist_id,state,created_at_ms);
```

The episode row is not an unlimited second transcript: retain references and bounded review material; load the answer by verified pointer/digest. Review input is a frozen bounded record needed for audit; a resolved review need not retain full output apart from accepted deltas and outcome. Use existing immutable-record erasure rules for reset/removal; do not delete user-owned/shared origins through a blanket cascade. Derive search/link/dependency rows transactionally from canonical heads and rebuild them on repair. No persistent graph-to-Lens mirror of whole claims beyond exact evidence actually cited.

The new receipt table replaces the current message-only identity of a Memory report because helper/World publications have no originating assistant reply of their own. Current chat/report/receipt commands read that table or its exact frozen DTO; they do not calculate independent saved counts. Existing growth events still own graph changes and usage still owns delivery counts.

## TypeScript and bridge

```ts
// src/contracts/knowledge.ts and memory.ts: renderer/domain DTOs, no SQLite payloads.
export type LensKind = "principle" | "perspective" | "method" | "subject";
export type LensReceipt = {
  recordId: string; revisionId: string; revisionDigest: string;
  kind: LensKind; title: string; delivery: "context" | "exact_read";
};
export type SectionBasisView =
  | { kind: "sourced"; sources: EvidenceLink[] }
  | { kind: "synthesis"; premises: EvidenceLink[]; uncertainty: string; lead?: LeadView }
  | { kind: "question" }
  | { kind: "practice"; examples: ExperienceLink[] };
export type LensPageView = {
  recordId: string; revisionId: string; kind: LensKind; title: string;
  markdown: string; sections: SectionView[]; links: NavigationLink[];
  backlinks: NavigationLink[]; changes: LensChangeView[];
  paused: boolean; forgotten: boolean; basisUnavailable: boolean;
};
export type MemoryUpdateResult = {
  status: "applied" | "no_change";
  commitId: string;
  changes: MemoryUpdateChange[]; // adds lensKind + before/after for Lens, schema change kind
  processing: ProcessingDispositionView[];
  episodeId?: string;
};
export type BotWork = {
  handoffId: string; sessionId: string; title: string;
  state: "queued" | "running" | "succeeded" | "failed" | "stopped";
  resultMessageId?: string; memoryReceiptId?: string;
};
export type HomeDelivery = ExistingScheduledDelivery | {
  kind: "research"; handoffId: string; sessionId: string;
  status: BotWork["state"]; memoryReceiptId?: string;
};

// Existing LoupistBridge extended in contracts and tauriBridge; mock implements same shape.
interface MemoryIntents {
  knowledgeLensRead(args: ExactLensLocation): Promise<LensPageView>;
  knowledgeLensUndo(args: { loupistId: string; changeId: string }): Promise<UndoResult>;
  worldSchemaRevert(args: { loupistId: string; changeId: string; expectedRevisionId: string }): Promise<UndoResult>;
  memoryReceiptRead(args: { loupistId: string; receiptId: string }): Promise<MemoryUpdateResult>;
  researchStop(args: { loupistId: string; handoffId: string }): Promise<BotWork>;
  // “Correct in Bot” is existing chat draft/pin intent; no direct document textarea API.
  // User-invoked Open as research dispatches the same checked handoff service,
  // while model invocation stages through the parent completed-turn boundary.
}
```

Use exhaustive LensKind switches for receipt presentation. A perspective must be rendered even when no method was delivered. Bot display names come from host role/ID, not a stored title string comparison. All new keys have EN/KO translations. UI actions target exact revisions/changes and same-Loupist scopes. The graph view receives a selected path of actual claim IDs with a lead label, not synthetic causal edges. A missing current claim can show its frozen evidence with a stale warning without silently highlighting a different claim.

## Module map and dominant access paths

| Owner | Responsibility and private decisions | Access path |
| --- | --- | --- |
| `knowledge/lens.rs`, `model.rs`, `catalog.rs` | Kind, page sections, immutable revisions, scoped links, merge/undo, dedup keys; only publisher | Exact read → current head → sections + original evidence targets. Search uses existing current-head document index. |
| `commands/tool_executor/authority.rs`, `principles.rs` | JSON adaptation, acknowledged handles, user-teaching authority, staging | `lens_update`/`principle_save` → checked change → `ToolCapture`; never SQL writes. |
| `commands/chat_runtime/completion.rs`, `commands/knowledge/learning.rs` | One reply transaction, source-handle resolution, receipts and episodes | Successful reply → `KnowledgeWrite::commit_completed` → persisted reply/Memory. |
| `knowledge/learning.rs`, existing `graph_service.rs` | Episode/review admission, budgets, frozen inputs, terminal paid states | Episode or World event → eligible review → isolated inference → same Lens publisher. |
| `knowledge/world_schema.rs` | Core/overlay definitions, immutable schema revisions, auto-add/revert | Template ID or checked addition → registry → extractor/context/Map labels. `ontology.rs` is only an extractor adapter. |
| `knowledge/map.rs` + private identity helper | One identity index/matcher | `(symbol,market)` → set of candidates → Exact/Ambiguous/Unmatched; asset pages consume same result. |
| `knowledge/connections.rs` | Bounded evidence-path selection/ranking | Current visible World + changed claims + exact anchors → candidate paths, reused by search/upkeep. |
| `agent_harness.rs`, runtime setup/launch | Role and Memory/vocabulary context, cache seals, wrappers | Captured host intent → bounded envelope; no model-derived role or grants. |
| `chat/handoff.rs`, `chat/home.rs`, existing run coordinator | Atomic handoff launch record, existing worker lifetime, deterministic Bot deliveries | Parent commit → queued child Chat → same `send_chat` → terminal receipt → Bot. |
| React `MemoryPage`, `LensWorkspace`, `ChatReceipt`, Map inspector | One page reader, one change view, path explanation, typed intent | Bridge DTO → localized UI; no inference of saved state from answer prose. |

`shared_pack.rs`, sharing export/import, reset, search and as-of code consume the same new page model in the contract unit. Exports remap local page/entity IDs with their included World dependencies; omitted entities become explicitly unresolved navigation, never dangling authority. Shared evidence retains wrappers and original provenance on import. The complete dependency/owner checks happen in the owner before publishing, not duplicated in React or helper prompt parsing.
