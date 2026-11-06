# Type sketch

Bodies are `todo!()` or a short comment. Names match current modules. New ids are branded strings so a claim id cannot be passed as a filing id.

The caller's view this sketch serves:

- The model calls `answer_file` and, only in the Bot, `open_research`.
- React calls `memoryForget`, `memoryKeepHypothesis`, `memoryUndoSchema`, `openResearchSession`, and `applyPreset` on `LoupistBridge`.
- The graph supervisor calls `project_filing` and `fresh_paths`. Nothing else writes subject pages.

## Module map

| Module | Owns | Does not own |
| --- | --- | --- |
| `knowledge/filing.rs` | `parse_filing`, citation extraction | SQLite, the pump |
| `knowledge/project.rs` | `project_filing`, `rebuild_subjects`, `apply_projection` | Path ranking, schema |
| `knowledge/discover.rs` | `fresh_paths`, hypothesis status transitions | Page Markdown |
| `knowledge/world_extract/ontology.rs` | Core types, `InstrumentFacet`, `GOES_INTO`, `product_role` | Extension rows |
| `knowledge/world_schema.rs` | Extension validation, version history, apply, undo, preset seeds | Chat tools |
| `knowledge/map.rs` and `knowledge/assets/mod.rs` | `match_followed` | Page kind |
| `knowledge/schema.rs` | Version 6 DDL | Projection policy |
| `broker.rs`, `commands/tool_executor.rs`, `commands/knowledge/learning.rs`, `commands/chat_runtime/completion.rs` | Stage and commit filings with the reply | Subject pages |
| `agent_harness.rs`, `commands/chat_runtime/setup.rs` | `Seat` on the runtime profile, tool filter | Projection |
| `chat/home.rs` | Bot card delivery | Filing parse |
| `agent/tool-catalog.json`, `agent/SYSTEM.md`, `briefings/policy.rs` | The two new tool contracts and the seat paragraph | Storage |
| `src/contracts/memory.ts`, `src/chat/receipt.ts`, `src/components/memory/MemoryPage.tsx` | Card kinds and the reader | Write policy |
| `src/bridge/tauriBridge.ts` | The five intents | Tool JSON |

`project.rs`, `filing.rs`, and `discover.rs` are `pub(crate)`. A use from `src/` or from a Pi extension does not compile.

## Domain types (Rust)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    Perspective,
    Method,
    Principle,
    Subject,
    Hypothesis,
}

/// The only kinds `lens_update` may name. Subject and hypothesis are not in this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelLensKind {
    Perspective,
    Method,
}

impl ModelLensKind {
    pub fn page_kind(self) -> PageKind {
        match self {
            Self::Perspective => PageKind::Perspective,
            Self::Method => PageKind::Method,
        }
    }
}

/// Kinds a shared pack may export. Subject and hypothesis are absent on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharableKind {
    Perspective,
    Method,
    Principle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    Bot,
    Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginTier {
    Source,
    User,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HypothesisStatus {
    Unverified,
    Kept,
    Answered,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentFacet {
    Equity,
    Index,
    Fund,
    Bond,
    Commodity,
    Currency,
    Crypto,
    Rate,
    RealAsset,
    Prediction,
    Private,
    Other,
}

/// `None` means no asset page. Equity returns `Some(Equity)` and matches an Organization, not an Asset node.
pub fn facet_page_class(facet: InstrumentFacet) -> Option<AssetClass> {
    todo!("one match, used by the prompt builder and by asset-page linking")
}

/// Prompt text for Asset.asset_class. Built from `InstrumentFacet`, excluding Equity.
pub fn asset_facet_prompt() -> &'static str {
    todo!("join the non-equity facet names in enum order")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingId(pub String);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimId(pub String);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeId(pub String);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HypothesisId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filing {
    pub filing_id: FilingId,
    pub loupist_id: String,
    pub chat_id: String,
    pub message_id: String,
    pub title: String,
    pub conclusion: String,
    pub claim_ids: Vec<ClaimId>,
    pub entity_ids: Vec<NodeId>,
    pub lens_ids: Vec<String>,
    pub supersedes: Option<FilingId>,
    pub hypothesis_id: Option<HypothesisId>,
    pub untyped: Vec<String>,
    pub as_of: Option<chrono::NaiveDate>,
    pub origin: OriginTier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilingError {
    Empty,
    UnknownClaim(ClaimId),
    UnknownEntity(NodeId),
    NumberNotInEvidence(String),
    InstructionShaped,
    Budget,
    TitleTaken { existing_record_id: String, existing_kind: PageKind },
}

/// Reads the broker already admitted this turn, including as-of filtering.
pub struct TurnReads<'a> {
    pub claim_ids: &'a BTreeSet<String>,
    pub entity_ids: &'a BTreeSet<String>,
    pub lens_ids: &'a BTreeSet<String>,
    pub evidence_excerpts: &'a [String],
    pub claim_propositions: &'a BTreeMap<String, String>,
    pub as_of: Option<chrono::NaiveDate>,
}

pub fn parse_filing(raw: &serde_json::Value, turn: &TurnReads<'_>) -> Result<Filing, FilingError> {
    todo!("require claim: and world: links in conclusion to be a subset of the arrays; reject digits that none of the cited texts contain; reject shared-wrapper markers; origin is always Agent")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Match {
    One(NodeId),
    None,
    Ambiguous,
}

pub fn match_followed(item: &MapContextItem, nodes: &[GraphNode]) -> Match {
    todo!("key is normalized symbol plus normalized venue; symbol-only matches only when one node has that symbol; never the lexicographic first id")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageSection {
    pub filing_id: FilingId,
    pub at_ms: i64,
    pub as_of: Option<chrono::NaiveDate>,
    pub markdown: String,
    pub disagree_with: Option<FilingId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectPage {
    pub record_id: String,
    pub title: String,
    pub sections: Vec<PageSection>,
    pub continued_from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub page_record_id: String,
    pub target: MentionTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MentionTarget {
    Node(NodeId),
    Claim(ClaimId),
    Lens(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HypothesisDraft {
    pub hypothesis_id: HypothesisId,
    pub claim_ids: Vec<ClaimId>,
    pub node_ids: Vec<NodeId>,
    pub title: String,
    pub sentence: String,
    pub status: HypothesisStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectState<'a> {
    pub subjects: &'a [SubjectPage],
    pub applied_filing_ids: &'a BTreeSet<String>,
    pub claims: &'a BTreeMap<String, GraphClaim>,
    pub tombstoned: &'a BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projection {
    pub pages: Vec<SubjectPage>,
    pub mentions: Vec<Mention>,
    pub hypotheses_answered: Vec<HypothesisId>,
}

/// Pure. Does not write. Incremental result equals `rebuild_subjects` over the same live filings.
pub fn project_filing(state: &ProjectState<'_>, filing: &Filing) -> Projection {
    todo!("append one section to the title match, or create one page; cap 8000 chars by splitting a continued page; drop withdrawn claims; emit no page when no live citation remains")
}

pub fn rebuild_subjects(live_filings: &[Filing], claims: &BTreeMap<String, GraphClaim>) -> Projection {
    todo!("fold project_filing from an empty state in filing_id order")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverInput<'a> {
    pub claims_since_cursor: &'a [GraphClaim],
    pub followed: &'a [NodeId],
    pub already: &'a BTreeSet<String>,
    pub made_today: u32,
}

pub fn fresh_paths(input: &DiscoverInput<'_>) -> Vec<HypothesisDraft> {
    todo!("paths of 2 or 3 hops, ends on followed nodes, middle not followed, at most one, none when made_today >= 4")
}

pub enum ApplyOutcome {
    Applied { filing_id: FilingId, page_record_ids: Vec<String> },
    AlreadyApplied { filing_id: FilingId },
    NothingToAdd { filing_id: FilingId },
}

pub fn apply_projection(
    tx: &rusqlite::Transaction<'_>,
    loupist_id: &str,
    filing_id: &FilingId,
    projection: &Projection,
    at_ms: i64,
) -> KnowledgeResult<ApplyOutcome> {
    todo!("insert knowledge_projection_applied and the page head in this transaction; a second call returns AlreadyApplied")
}
```

`GraphClaim`, `GraphNode`, `ClaimStatus`, and `AssetClass` stay the types in `knowledge/model.rs` and `knowledge/assets/class.rs`. The projector reads `status`, `expired_at`, `invalid_at`, and evidence role `opposes`. It does not invent a parallel claim struct.

## Schema apply

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaVersion {
    pub version: u32,
    pub extension_json: String,
    pub applied_at_ms: i64,
}

pub enum SchemaApply {
    Applied { version: u32, blurb_en: String, blurb_ko: String },
    Unchanged,
    Failed { reason: &'static str },
}

pub fn apply_validated_extension(
    tx: &rusqlite::Transaction<'_>,
    loupist_id: &str,
    next: &WorldExtension,
    at_ms: i64,
) -> KnowledgeResult<SchemaApply> {
    todo!("validate_extension, append knowledge_schema_versions, do not enqueue old episodes")
}

pub fn undo_schema(
    tx: &rusqlite::Transaction<'_>,
    loupist_id: &str,
    at_ms: i64,
) -> KnowledgeResult<SchemaApply> {
    todo!("append a new version whose JSON equals the previous snapshot")
}

pub fn apply_preset(
    tx: &rusqlite::Transaction<'_>,
    loupist_id: &str,
    template_id: &str,
    at_ms: i64,
) -> KnowledgeResult<SchemaApply> {
    todo!("the seed table keyed by LoupistTemplate id, through apply_validated_extension, with no focus-string compare")
}
```

## Seat and open research

```rust
pub struct OpenResearch {
    pub loupist_id: String,
    pub origin_chat_id: String,
    pub origin_message_id: String,
    pub question: String,
    pub entity_ids: Vec<NodeId>,
    pub lens_ids: Vec<String>,
    pub filing_id: Option<FilingId>,
    pub hypothesis_id: Option<HypothesisId>,
}

pub struct OpenResearchResult {
    pub chat_id: String,
}

pub fn open_research(state: &AppState, request: OpenResearch) -> Result<OpenResearchResult, CommandError> {
    todo!("create the chat and one knowledge_session_seeds row; do not enqueue a run")
}
```

`AgentRuntimeProfile::new` gains `seat: Seat`. Setup passes `Seat::Bot` when `thread_id` is `home_thread_id`. The runtime JSON the model already receives includes `"seat": "bot" | "session"`. The component filter that already strips schedule tools also strips `open_research` unless the seat is `Bot`.

## Commit path

```rust
impl ToolCapture {
    // existing staged_sources, staged_lenses, staged_principles, note
    pub staged_filings: Mutex<Vec<Filing>>,
}

pub fn commit_filings(
    tx: &rusqlite::Transaction<'_>,
    filings: &[Filing],
    at_ms: i64,
) -> Result<Vec<MemoryUpdateChange>, CommandError> {
    todo!("insert knowledge_filings; return one Filing change per row; do not write subject pages")
}
```

`commit_completed_turn` calls `commit_filings` inside the transaction that already commits sources and lenses, then `graph.wake()`. The pump later calls `apply_projection`. Abort and failure drop `staged_filings` with the rest of `ToolCapture`.

Broker cardinality gains `filings: u8`, capped at 1, separate from the note cap and the eight-proposal cap.

Title collision on `lens_update` and on `answer_file` returns `FilingError::TitleTaken` to the tool result. The `continue` that drops the create is deleted.

## SQLite (version 6)

`KNOWLEDGE_SCHEMA_VERSION` becomes 6. A folder at 5 is refused. No migration.

```sql
-- lens_kind check becomes
-- ('perspective','method','principle','subject','hypothesis')
-- drop any write of payload lensKind
-- knowledge_lens_changes loses UNIQUE(record_id) so method edits append

CREATE TABLE knowledge_standing (
    loupist_id TEXT PRIMARY KEY NOT NULL,
    record_id TEXT NOT NULL REFERENCES knowledge_records(record_id)
) STRICT;

CREATE TABLE knowledge_filings (
    filing_id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    title TEXT NOT NULL,
    title_fold TEXT NOT NULL,
    conclusion TEXT NOT NULL,
    claim_ids_json TEXT NOT NULL,
    entity_ids_json TEXT NOT NULL,
    lens_ids_json TEXT NOT NULL,
    supersedes TEXT,
    hypothesis_id TEXT,
    untyped_json TEXT NOT NULL,
    as_of TEXT,
    origin TEXT NOT NULL CHECK (origin IN ('source','user','agent')),
    tombstoned_at_ms INTEGER,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
) STRICT;

CREATE TABLE knowledge_projection_applied (
    loupist_id TEXT NOT NULL,
    filing_id TEXT NOT NULL,
    applied_at_ms INTEGER NOT NULL,
    PRIMARY KEY (loupist_id, filing_id)
) STRICT;

CREATE TABLE knowledge_lens_mentions (
    loupist_id TEXT NOT NULL,
    page_record_id TEXT NOT NULL,
    target_kind TEXT NOT NULL CHECK (target_kind IN ('node','claim','lens')),
    target_id TEXT NOT NULL,
    PRIMARY KEY (page_record_id, target_kind, target_id)
) STRICT;

CREATE TABLE knowledge_hypotheses (
    hypothesis_id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL,
    claim_ids_json TEXT NOT NULL,
    node_ids_json TEXT NOT NULL,
    path_key TEXT NOT NULL,
    title TEXT NOT NULL,
    sentence TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('unverified','kept','answered','rejected')),
    created_at_ms INTEGER NOT NULL,
    UNIQUE (loupist_id, path_key)
) STRICT;

CREATE TABLE knowledge_schema_versions (
    loupist_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    extension_json TEXT NOT NULL,
    applied_at_ms INTEGER NOT NULL,
    PRIMARY KEY (loupist_id, version)
) STRICT;

CREATE TABLE knowledge_session_seeds (
    chat_id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL,
    question TEXT NOT NULL,
    entity_ids_json TEXT NOT NULL,
    lens_ids_json TEXT NOT NULL,
    origin_message_id TEXT NOT NULL,
    filing_id TEXT,
    hypothesis_id TEXT,
    created_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE knowledge_discovery_cursor (
    loupist_id TEXT PRIMARY KEY NOT NULL,
    claim_recorded_at_ms INTEGER NOT NULL,
    made_on TEXT NOT NULL,
    made_count INTEGER NOT NULL
) STRICT;
```

`knowledge_lens_applications` stays the per-turn usage log. Mentions are body citations and are a different table.

Index lines are not stored. `index_lines(pages) -> Vec<IndexLine>` derives them at context assembly.

```rust
pub struct IndexLine {
    pub kind: PageKind,
    pub record_id: String,
    pub title: String,
    pub gist: String,
}
```

## Tool contracts

Add to `agent/tool-catalog.json`.

```json
{
  "name": "answer_file",
  "method": "memory.file",
  "permission": "memory_write",
  "component": "memory-save",
  "label": "File this answer",
  "description": "File one reusable conclusion from this turn. Required when saving is on and the reply reached a conclusion someone would look up later. Cite only claims and entities this turn exact-read. Do not file chatter, one-offs, or a conclusion with no claim citation. A number in conclusion must appear in a cited claim or a retained excerpt. This stages until the reply commits.",
  "parameters": {
    "type": "object",
    "additionalProperties": false,
    "required": ["title", "conclusion", "claimIds"],
    "properties": {
      "title": { "type": "string", "minLength": 1, "maxLength": 120 },
      "conclusion": { "type": "string", "minLength": 1, "maxLength": 4000 },
      "claimIds": { "type": "array", "minItems": 1, "maxItems": 12, "items": { "type": "string" } },
      "entityIds": { "type": "array", "maxItems": 12, "items": { "type": "string" } },
      "supersedes": { "type": "string" },
      "hypothesisId": { "type": "string" },
      "untyped": { "type": "array", "maxItems": 4, "items": { "type": "string", "maxLength": 80 } }
    }
  }
}
```

```json
{
  "name": "open_research",
  "method": "chat.open_research",
  "permission": "always",
  "component": "core",
  "label": "Open a research session",
  "description": "Bot only. Open a research session for a sustained evidence pass. Does not start the pass. Say that the session is ready. Do not repeat the deep work in this thread.",
  "parameters": {
    "type": "object",
    "additionalProperties": false,
    "required": ["question"],
    "properties": {
      "question": { "type": "string", "minLength": 1, "maxLength": 500 },
      "entityIds": { "type": "array", "maxItems": 8, "items": { "type": "string" } },
      "lensIds": { "type": "array", "maxItems": 6, "items": { "type": "string" } }
    }
  }
}
```

`lens_update.parameters.properties.kind.enum` stays `["perspective", "method"]`. Its description drops the sentence that forbids World references for those two kinds only. Subject pages are not written by this tool, so the forbid-world check stays for method and perspective documents and is lifted only inside `project_filing` for subject Markdown.

`briefings/policy.rs` `allowed_name` gains `"answer_file"` and `"open_research"` is not added. Unattended work files. It does not open sessions.

## TypeScript

`src/contracts/memory.ts`:

```typescript
export type PageKind = "perspective" | "method" | "principle" | "subject" | "hypothesis";

export type MemoryUpdateChange = {
  worldJobId?: string | null;
  worldStatus?: string | null;
  recordId: string;
  operation: "create" | "revise" | "rename" | "proposed" | "undo";
  title: string;
  lesson: string;
  basis: string;
  futureUse: string;
} & (
  | { kind: "Research"; knowledge?: never }
  | { kind: "principle"; statement: string; knowledge: KnowledgeReference }
  | { kind: "Filing"; filingId: string; knowledge?: never }
  | { kind: "Subject"; knowledge: KnowledgeReference }
  | { kind: "Hypothesis"; hypothesisId: string; from: string; via: string; to: string; knowledge?: never }
  | { kind: "Schema"; version: number; blurb: string; knowledge?: never }
  | { kind: Exclude<MemoryKind, "Research">; knowledge: KnowledgeReference }
);
```

`SAVED_RECEIPT_KINDS` in `receipt.ts` becomes `["Source", "Lens", "perspective", "principle", "Filing", "Subject", "Hypothesis", "Schema", "Journal", "Research", "World"]`. Perspective is its own group. The filter that keeps only `principle` and `method` from `lens_receipts` also keeps `perspective`.

Bridge intents on `LoupistBridge`:

```typescript
memoryForget(request: {
  loupistId: string;
  target: { kind: "filing"; filingId: string } | { kind: "hypothesis"; hypothesisId: string };
}): Promise<MemoryUpdateResult>;

memoryKeepHypothesis(request: { loupistId: string; hypothesisId: string }): Promise<MemoryUpdateResult>;

memoryUndoSchema(request: { loupistId: string }): Promise<MemoryUpdateResult>;

openResearchSession(request: {
  loupistId: string;
  originChatId: string;
  originMessageId: string;
  question: string;
  entityIds?: string[];
  lensIds?: string[];
  filingId?: string;
  hypothesisId?: string;
}): Promise<{ chatId: string }>;

applyPreset(request: { loupistId: string; templateId: string }): Promise<MemoryUpdateResult>;
```

`memoryLearningSettingsSet` and the `learnMethods` field are deleted with the publisher. Callers in the settings UI go in the same change.

Home history display title:

```typescript
export function chatDisplayTitle(chat: { id: string; loupistId: string; title: string }, homeLabel: string): string {
  return chat.id === `home-${chat.loupistId}` ? homeLabel : chat.title;
}
```

Memory page and receipt history both call it. The stored title `"Thread"` stays in the database.

## Pump order

`GraphSupervisor::pump_once` becomes:

1. `pump_schema`, and on a valid pending proposal call `apply_validated_extension`.
2. One World outbox job, unchanged.
3. When that Loupist's outbox is idle, `project_filing` for each unapplied live filing, then `fresh_paths`.
4. `pump_learning` is deleted. Nothing in the pump publishes a method.

One pump invocation still does one unit of work and returns, matching today's loop. Schema apply, one World job, or one filing projection is one unit. Discovery runs in the same unit as the filing projection that cleared the filing queue, so a path can see the new mentions.

## Observable learning record

| Claim | Record | Test |
| --- | --- | --- |
| The turn filed a conclusion | `knowledge_filings` row whose `message_id` is the committed reply, and a `Filing` change on that message | Commit-path Rust test |
| The wiki grew from it | Subject page Markdown contains the claim id, and `knowledge_projection_applied` contains the filing id | Projector Rust test, including rebuild equality |
| A connection was proposed | `knowledge_hypotheses` row, status `unverified`, and a Bot host message whose `memory_update` contains `Hypothesis` | `fresh_paths` Rust test plus home delivery test |
| The next task can see it | Context index text contains the subject title and record id | Context-assembly Rust test |
| The user rejected it | `tombstoned_at_ms` set, rebuild omits the section | Forget Rust test |
| A stopped turn learned nothing | No filing row for that message id | Commit-path Rust test |
