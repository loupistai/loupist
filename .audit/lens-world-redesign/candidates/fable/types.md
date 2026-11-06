# Types, contracts, tables and module map (candidate "fable")

Paths are under `apps/loupist/`. Bodies are `todo!()` or pseudocode. Every public shape here is derived from the usage in `design.md`.

## 1. Lens page model (Rust, `src-tauri/src/knowledge/model.rs`)

```rust
/// One Lens record's payload. The `kind` tag is the only place the kind lives;
/// `knowledge_lens_metadata.lens_kind` is written from it at commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LensPage {
    pub title: String,
    pub body: LensBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LensBody {
    /// Overall investing philosophy. Exactly one active per Loupist.
    Perspective(MethodDocument),
    /// Reusable way of checking something.
    Method(MethodDocument),
    /// The user's own rule, in the user's words.
    Principle { statement: String, why: String, example: String },
    /// What the Loupist knows about one thing: a company, sector, value chain,
    /// technology, thesis. The compounding unit of the wiki.
    Subject {
        markdown: String,
        /// World entities this page is about. Validated against the current
        /// snapshot or this turn's acknowledged World reads. Exported by name.
        about: Vec<EntityRef>,
        open_questions: String,
    },
    /// A connection the Loupist noticed and has not verified.
    Hypothesis {
        markdown: String,
        mechanism: String,
        /// Expected direction and horizon, in words ("negative for X within two quarters").
        expected: String,
        /// What dated observation would refute it.
        falsifier: String,
        /// Claim ids of the World path that produced it; empty for a user-stated one.
        path_claim_ids: Vec<String>,
        status: HypothesisStatus,
        registered_at_ms: i64,
    },
}

/// The former `LensDocument`, unchanged fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MethodDocument {
    pub markdown: String,
    pub applicability: String,
    pub assumptions: String,
    pub procedure: String,
    pub principles: String,
    pub limitations: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntityRef {
    pub node_id: String,
    /// Canonical English name at write time; the export and a removed entity use it.
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisStatus { Open, Supported, Refuted, Dropped }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LensKind { Perspective, Method, Principle, Subject, Hypothesis }
enum_str!(LensKind { Perspective => "perspective", Method => "method", Principle => "principle", Subject => "subject", Hypothesis => "hypothesis" });

impl LensKind {
    /// Portable kinds travel in shared packs and may not cite World ids.
    /// Subject and hypothesis pages are this Loupist's synthesis and may.
    pub fn portable(self) -> bool { matches!(self, Self::Perspective | Self::Method | Self::Principle) }
    /// Kinds the model may create through `lens_update`. Principles go through `principle_save`.
    pub fn model_writable(self) -> bool { !matches!(self, Self::Principle) }
}

impl LensBody {
    pub fn kind(&self) -> LensKind { todo!() }
    /// Exact model-facing text; every field participates in the digest.
    pub fn delivery_text(&self) -> KnowledgeResult<String> { todo!() }
    /// ≤240 chars, one line, for growth events and `learned` posts.
    pub fn summary(&self) -> String { todo!() }
    pub fn about(&self) -> &[EntityRef] { todo!() }
}

/// Who a revision came from, derived at commit. Never supplied by the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LensTier { User, Shared, LoupistSourced, LoupistInferred }

impl LensTier {
    /// Loupist actor with ≥1 acknowledged Source or World read → Sourced; else Inferred.
    pub fn derive(actor: ChangeActor, acknowledged_reads: usize) -> Self { todo!() }
}

/// Link relation parsed from the Markdown link title: `[t](lens:rec_x "contradicts")`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkRelation { Related, Supports, Contradicts, Supersedes }

pub struct ReviseLens {
    pub loupist_id: String,
    pub record_id: String,
    pub expected_revision_id: Option<String>,
    pub at_ms: i64,
    pub page: LensPage,                 // was `document: LensDocument`
    pub dependencies: Vec<EvidenceRevision>,
    pub change: LensChangeInput,        // gains nothing; tier is derived in the write
}

pub struct LensView {
    pub record_id: String,
    pub revision_id: String,
    pub revision_digest: String,
    pub page: LensPage,
    pub tier: LensTier,
    pub dependencies: Vec<EvidenceRevision>,
    pub revisions: Vec<RevisionSummary>, // newest first, ≤20; for Restore and Then vs now
}
```

### Write path (`src-tauri/src/knowledge/lens.rs`)

```rust
impl KnowledgeWrite<'_> {
    /// Creates a revision, moves the head, appends a change row with the derived tier,
    /// rewrites `knowledge_lens_subjects` and `knowledge_lens_links` from the page,
    /// and writes `lens_created` or `lens_revised`. Returns `title_taken` with the
    /// existing record id instead of dropping a create.
    pub fn revise_lens_with_origin(&mut self, input: ReviseLens, origin: ChangeOrigin) -> KnowledgeResult<LensSaved> { todo!() }

    /// New head at an older revision of the same record; `restore` change row.
    /// Running it twice with the same revision is a no-op with the same result.
    pub fn restore_lens_revision(&mut self, loupist_id: &str, record_id: &str, revision_id: &str, at_ms: i64) -> KnowledgeResult<LensSaved> { todo!() }
}

/// Kind-aware validation. Portable kinds keep today's World/Journal ban;
/// subject and hypothesis pages may cite `loupist://source/…` and
/// `loupist://world/claim/…` URLs that resolve in this database or in `acknowledged`.
pub(super) fn validate_page(tx: &Transaction<'_>, loupist_id: &str, page: &LensPage, acknowledged: &AcknowledgedReads, pending_lens_ids: &BTreeSet<String>) -> KnowledgeResult<()> { todo!() }

/// Refuses shared delimiters and credential-like text in any page body.
pub(super) fn body_is_memory_safe(page: &LensPage) -> Result<(), MemoryRefusal> { todo!() }

/// `(target_record_id, relation)` pairs parsed from every text field.
pub(super) fn parse_links(page: &LensPage) -> Vec<(String, LinkRelation)> { todo!() }
```

## 2. Staged tool write (`src-tauri/src/commands/tool_executor.rs`, `tool_executor/authority.rs`)

```rust
pub(super) struct StagedLensRevision {
    pub(super) record_id: String,
    pub(super) expected_revision_id: Option<String>,   // None on create
    pub(super) page: LensPage,                          // replaces title/kind/document
    pub(super) rationale: String,
    pub(super) source_reads: Vec<String>,
    pub(super) world_reads: Vec<String>,
    pub(super) journal_reads: Vec<String>,
    pub(super) lens_reads: Vec<String>,
    pub(super) previous_note_ids: Vec<String>,
    pub(super) feedback: Option<String>,
}

/// `lens_update` authority. Checks permission, loads the head when editing,
/// refuses a kind change, refuses `kind: principle`, validates `about` ids against
/// the snapshot head or this turn's World reads, reserves the title in the staged
/// set so two creates in one turn cannot collide, and returns `title_taken`
/// {recordId, readArgs} as a tool error when an active page of that kind owns the title.
pub(super) fn stage_lens_update(capture: &ToolCapture, args: LensUpdateArgs) -> Result<ToolResult, ToolRefusal> { todo!() }

/// `world_schema_update` authority: memory_write, bounds, protected names, no
/// duplicate of core or current extension, label shape; stages one
/// `StagedExtensionChange` per turn (cardinality 1).
pub(super) fn stage_world_schema_update(capture: &ToolCapture, args: WorldSchemaUpdateArgs) -> Result<ToolResult, ToolRefusal> { todo!() }

pub(super) struct StagedExtensionChange { pub add: ExtensionDelta, pub remove: ExtensionDelta, pub rationale: String }

/// `session_open` (Bot only, permission always). Creates the session chat with
/// pinned entities and pages, optionally starts its first turn with `brief`,
/// and returns {chatId, started}. Not staged: a session is not Memory.
pub(super) fn open_session(capture: &ToolCapture, args: SessionOpenArgs) -> Result<ToolResult, ToolRefusal> { todo!() }
```

### Commit (`src-tauri/src/commands/knowledge/learning.rs`, `completion.rs`)

```rust
/// Unchanged signature shape; `saved_lenses` carries `StagedLensRevision` with `page`.
/// A create whose title is taken at commit time (a parallel chat won) is recorded as
/// `operation: proposed` in the learning report, never dropped.
pub fn persist_saved_turn(tx: &Transaction<'_>, ..., saved_lenses: &[StagedLensRevision]) -> Result<Vec<MemoryChange>, CommandError> { todo!() }

/// Commits the staged extension change as version n+1 with origin `chat`,
/// writes `schema_extended`/`schema_reduced` with before/after summaries.
pub fn persist_extension_change(tx: &Transaction<'_>, loupist_id: &str, change: &StagedExtensionChange, message_id: &str, at_ms: i64) -> Result<Vec<MemoryChange>, CommandError> { todo!() }
```

`commit_completed_turn` adds, after `graph.wake()`: `if !is_bot { chat::home::post_learned(app_state, loupist_id, &completed_message) }`.

## 3. Envelope additions (`src-tauri/src/agent_harness.rs`, `commands/chat_runtime/setup.rs`)

```rust
/// Block 0, every turn, before the standing perspective.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationBlock {
    pub role: ConversationRole,         // "bot" | "session" | "scheduled"
    pub title: Option<String>,          // session title; None for the Bot
}

/// 6h gains the vocabulary. ≤ 800 characters; names only, no docs.
#[derive(Serialize)]
pub struct WorldCatalog { pub counts: WorldCounts, pub vocabulary: Vocabulary }
#[derive(Serialize)]
pub struct Vocabulary { pub entity_types: Vec<String>, pub relations: Vec<String>, pub extension_version: u32 }

/// 6e: `subject` and `hypothesis` pages whose `about` intersects the turn's
/// followed identities or the entities named in the request; ≤3, by last revision.
pub fn subject_pointers(tx: &Transaction<'_>, loupist_id: &str, entity_ids: &[String], as_of: Option<&AsOf>) -> KnowledgeResult<Vec<LensPointer>> { todo!() }

/// 6i: ≤3 lint findings on pages relevant to this turn's entities.
pub fn lint_pointers(tx: &Transaction<'_>, loupist_id: &str, entity_ids: &[String]) -> KnowledgeResult<Vec<LintPointer>> { todo!() }
```

The followed-entity set for 6e comes from `knowledge::map::match_items` after the market fix below, so pointers are by identity.

## 4. World vocabulary (`src-tauri/src/knowledge/world_extract/ontology.rs`, `world_schema.rs`)

```rust
// ontology.rs: the one list. `Technology` joins the core.
const TECHNOLOGY: &str = "A manufacturing process, process node, material science, technique, protocol or standard that more than one organization can use, such as EUV lithography, HBM3E stacking, LFP chemistry, proof-of-stake. Not a Technology: one vendor's named product (Product), or a measured figure (Metric).";

fn core_entity_types() -> Vec<EntityType> { /* + Technology {original_name}; Asset.asset_class doc built from AssetClass::ALL */ todo!() }
fn core_edge_map() -> Vec<((String, String), Vec<String>)> { /* + (Organization, Technology, [USES, OFFERS]), (Product, Technology, [USES]), (Technology, Technology, [USES, COMPETES_WITH]) */ todo!() }

/// Derived views for `world_schema.rs`; the constants there are deleted.
pub(crate) fn core_type_names() -> impl Iterator<Item = &'static str> { todo!() }
pub(crate) fn core_relation_names() -> impl Iterator<Item = &'static str> { todo!() }

// world_schema.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionOrigin { Template, ModelSeed, ModelReview, Chat, Undo }

pub struct ExtensionVersion { pub version: u32, pub extension: WorldExtension, pub origin: ExtensionOrigin, pub applied_at_ms: i64, pub summary: String }

/// Validates against bounds, protected names, core duplicates, label shape.
/// Returns the merged extension or a named refusal.
pub fn validate_delta(current: &WorldExtension, add: &ExtensionDelta, remove: &ExtensionDelta) -> Result<WorldExtension, ExtensionRefusal> { todo!() }
/// Appends version n+1. Idempotent on an identical extension: returns the current version unchanged.
pub fn apply_extension_version(tx: &Transaction<'_>, loupist_id: &str, next: WorldExtension, origin: ExtensionOrigin, at_ms: i64) -> KnowledgeResult<ExtensionVersion> { todo!() }
/// Appends a version equal to `version - 1`'s content with origin Undo. No-op if already undone.
pub fn undo_extension_version(tx: &Transaction<'_>, loupist_id: &str, version: u32, at_ms: i64) -> KnowledgeResult<ExtensionVersion> { todo!() }
pub fn current_extension(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<ExtensionVersion> { todo!() }
```

`schema_writer.rs` keeps the seed and review inference but its output goes through `validate_delta` and `apply_extension_version(origin: ModelSeed | ModelReview)` and then `chat::home::post_vocabulary`. The `knowledge_schema_proposals` table, its commands and `LoupistTracking.tsx`'s proposal UI are deleted.

## 5. Background reviews (`src-tauri/src/knowledge/learning.rs`, new `knowledge/connections.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewKind { Method, Connection }

/// Method review input now includes subject pages revised since `through_event`.
pub struct MethodReviewInput { pub claims: Vec<ClaimLine>, pub subject_pages: Vec<SubjectLine>, pub methods: Vec<MethodLine>, pub principles: Vec<String> }

/// Trigger: ≥5 added/disputed claims OR ≥2 subject revisions since the last review.
pub fn method_review_due(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<bool> { todo!() }

// connections.rs: pure, no model.
pub struct ConnectionCandidate {
    pub from_node_id: String,           // a followed entity
    pub to_node_id: String,             // 2–3 hops away
    pub path_claim_ids: Vec<String>,    // valid, non-withdrawn claims, as-of free
    pub hops: u8,
    /// 1/Π(degree of interior nodes) × 1 if `to` is in no subject page's `about` × recency weight
    pub score: f64,
}

/// Deterministic. BFS from each followed node over claims with status Active and
/// no `invalid_at` in the past; interior nodes bounded by degree ≤ 40; returns ≤ `limit`
/// candidates ordered by score, then stable by id. Excludes targets any hypothesis
/// page already names in `path_claim_ids` endpoints.
pub fn find_candidates(body: &WorldSnapshotBody, followed: &[String], already_about: &BTreeSet<String>, since_ms: i64, limit: usize) -> Vec<ConnectionCandidate> { todo!() }

/// One tool-free helper call. Input: the path's claims with evidence. Output JSON
/// {mechanism, expected, falsifier, markdown, title} or {action: "defer", rationale}.
pub const CONNECTION_PROMPT: &str = include_str!("../../../agent/prompts/connection-review.md");

/// Writes a `hypothesis` page with status Open, tier LoupistInferred, and posts `learned` to the Bot.
pub fn publish_hypothesis(tx: &Transaction<'_>, loupist_id: &str, candidate: &ConnectionCandidate, output: HypothesisOutput, at_ms: i64) -> KnowledgeResult<LensSaved> { todo!() }
```

`GraphSupervisor::pump_once` order: schema (auto-apply) → method review → connection review → lint → World jobs. Connection and method reviews share the six-hour cooldown and the four-per-day budget as one pool.

## 6. Lint (`src-tauri/src/knowledge/lint.rs`, no model)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LintFinding { Orphan, OneWayLink, Uncited, EntityRemoved, Stale, UnusedKind }

pub struct LintRow { pub record_id: Option<String>, pub finding: LintFinding, pub detail: String, pub detected_at_ms: i64 }

/// Recomputes all findings for a Loupist in one transaction (replace, not append), so it is idempotent.
pub fn run_lint(tx: &Transaction<'_>, loupist_id: &str, body: &WorldSnapshotBody, now_ms: i64) -> KnowledgeResult<Vec<LintRow>> { todo!() }
```

## 7. Bot (`src-tauri/src/chat/home.rs`)

```rust
pub(crate) const HOME_TITLE_KEY: &str = "bot"; // the renderer localizes; the stored title is no longer shown

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnedSummary {
    pub chat_id: String,
    pub message_id: String,
    pub pages: Vec<LearnedPage>,        // {recordId, title, kind, operation: created|revised, revisionId}
    pub names_added: u32,
    pub relationships_added: u32,
    pub vocabulary: Option<ExtensionVersionRef>,
}

/// One host post per committed non-Bot turn with ≥1 saved change; deduped by message id.
pub(crate) fn post_learned(state: &AppState, loupist_id: &str, message: &ChatMessage) -> Result<(), CommandError> { todo!() }
/// One host post per applied extension version from the background path.
pub(crate) fn post_vocabulary(state: &AppState, loupist_id: &str, version: &ExtensionVersion) -> Result<(), CommandError> { todo!() }
/// `delivery` posts attach `learned` built from the run chat's last committed message.
```

Host post record kinds: `delivery {…, learned}`, `alert`, `watch`, `earnings`, `bot-handoff`, `learned`, `vocabulary`, `session-opened {chatId, title}`.

## 8. Tool contracts (`agent/tool-catalog.json`, sketches)

```jsonc
// lens_update: `page` replaces `document` + `kind` + `title`.
{
  "name": "lens_update", "permission": "memory_write", "component": "lens-learning",
  "description": "Create or revise one Lens page when saving is on: a method, the perspective, a subject page (what you now know about one company, sector, technology, value chain or thesis) or a hypothesis. After substantive research, file the conclusion into the subject page for what you researched: lens_search first, lens_read it, then revise with recordId and expectedRevisionId; create only when none exists. A title already in use returns title_taken with readArgs: read and revise instead. Subject and hypothesis pages name the World entities they are about in `about` using node ids from this turn's world_read results, and may cite citation_url values from this turn. Methods and the perspective stay portable and cite no World ids. Put acknowledged evidence in sourceReads, worldReads, lensReads and journalReads. Principles are saved with principle_save, never here.",
  "parameters": {
    "type": "object", "additionalProperties": false, "required": ["page", "rationale"],
    "properties": {
      "recordId": {"type": "string", "maxLength": 128},
      "expectedRevisionId": {"type": "string", "maxLength": 128},
      "rationale": {"type": "string", "minLength": 1, "maxLength": 800},
      "page": {
        "type": "object", "additionalProperties": false, "required": ["title", "body"],
        "properties": {
          "title": {"type": "string", "minLength": 1, "maxLength": 180},
          "body": {"oneOf": [
            {"type": "object", "additionalProperties": false, "required": ["kind","markdown","applicability","assumptions","procedure","principles","limitations"],
             "properties": {"kind": {"enum": ["method", "perspective"]}, "markdown": {"type":"string","minLength":1}, "applicability": {"type":"string"}, "assumptions": {"type":"string"}, "procedure": {"type":"string"}, "principles": {"type":"string"}, "limitations": {"type":"string"}}},
            {"type": "object", "additionalProperties": false, "required": ["kind","markdown","about"],
             "properties": {"kind": {"const": "subject"}, "markdown": {"type":"string","minLength":1,"maxLength":24000},
                            "about": {"type":"array","maxItems":12,"items":{"type":"object","additionalProperties":false,"required":["nodeId"],"properties":{"nodeId":{"type":"string","maxLength":128}}}},
                            "openQuestions": {"type":"string","maxLength":2000}}},
            {"type": "object", "additionalProperties": false, "required": ["kind","markdown","mechanism","expected","falsifier","about"],
             "properties": {"kind": {"const": "hypothesis"}, "markdown": {"type":"string","minLength":1,"maxLength":12000}, "mechanism": {"type":"string","maxLength":1000}, "expected": {"type":"string","maxLength":400}, "falsifier": {"type":"string","maxLength":600},
                            "about": {"type":"array","maxItems":12,"items":{"type":"object","additionalProperties":false,"required":["nodeId"],"properties":{"nodeId":{"type":"string"}}}},
                            "status": {"enum": ["open","supported","refuted","dropped"]}}}
          ]}
        }
      },
      "sourceReads": {"type":"array","maxItems":32,"items":{"type":"string"}},
      "worldReads": {"type":"array","maxItems":8,"items":{"type":"string"}},
      "lensReads": {"type":"array","maxItems":8,"items":{"type":"string"}},
      "journalReads": {"type":"array","maxItems":8,"items":{"type":"string"}},
      "previousNoteIds": {"type":"array","maxItems":8,"items":{"type":"string"}}
    }
  }
}

// world_schema_update (new; component lens-learning; memory_write; not on the unattended allowlist)
{
  "name": "world_schema_update", "method": "world.schema_update", "permission": "memory_write", "component": "lens-learning",
  "description": "Add or remove kinds of names and links the map extracts from new sources, when the user asks to follow something the vocabulary lacks (for example clinical trial phases, process nodes, property types). At most four added kinds and six added links in total per Loupist; names in UpperCamelCase, links in UPPER_SNAKE_CASE; a link names the kinds it joins. The change applies to sources processed from now on, is shown to the user with Undo, and never reclassifies existing names.",
  "parameters": {
    "type": "object", "additionalProperties": false, "required": ["rationale"],
    "properties": {
      "add": {"type":"object","additionalProperties":false,"properties":{
        "types": {"type":"array","maxItems":4,"items":{"type":"object","additionalProperties":false,"required":["name","doc"],"properties":{"name":{"type":"string","pattern":"^[A-Z][A-Za-z0-9]{2,39}$"},"doc":{"type":"string","maxLength":600},"labels":{"type":"object","additionalProperties":false,"properties":{"en":{"type":"string","maxLength":40},"ko":{"type":"string","maxLength":40}}},"fields":{"type":"array","maxItems":3,"items":{"type":"object","additionalProperties":false,"required":["name","description"],"properties":{"name":{"type":"string","pattern":"^[a-z][a-z0-9_]{1,39}$"},"description":{"type":"string","maxLength":200}}}}}}},
        "relations": {"type":"array","maxItems":6,"items":{"type":"object","additionalProperties":false,"required":["name","doc","pairs"],"properties":{"name":{"type":"string","pattern":"^[A-Z][A-Z0-9_]{2,39}$"},"doc":{"type":"string","maxLength":600},"pairs":{"type":"array","minItems":1,"maxItems":6,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"string"}}}}}}}},
      "remove": {"type":"object","additionalProperties":false,"properties":{"types":{"type":"array","maxItems":4,"items":{"type":"string"}},"relations":{"type":"array","maxItems":6,"items":{"type":"string"}}}},
      "rationale": {"type":"string","minLength":1,"maxLength":400}
    }
  }
}

// session_open (new; permission always; component null; Bot only, filtered in setup.rs like schedule_*)
{
  "name": "session_open", "method": "session.open", "permission": "always", "component": null,
  "description": "Move work that needs sustained research out of the Bot into a research session. Give a self-contained brief with the question, scope and what a good answer contains; name the World entities and Lens pages the session should start from. With start true the session runs the brief as its first turn and the Bot shows it is working; otherwise the brief waits in the session's composer. Tell the user the session's title. Never open a session for a question you can answer from Memory and one or two reads.",
  "parameters": {
    "type": "object", "additionalProperties": false, "required": ["title", "brief"],
    "properties": {
      "title": {"type":"string","minLength":1,"maxLength":120},
      "brief": {"type":"string","minLength":1,"maxLength":8000},
      "entityIds": {"type":"array","maxItems":8,"items":{"type":"string","maxLength":128}},
      "lensRecordIds": {"type":"array","maxItems":8,"items":{"type":"string","maxLength":128}},
      "start": {"type":"boolean"}
    }
  }
}
```

`lens_read` and `lens_search` descriptions mention pages; `lens_search` gains an optional `kinds` filter array. `note_save`'s description narrows to "this Chat's progress and user-stated facts; conclusions about a subject belong in its subject page". Component `lens-learning` description: "Save a user-stated rule, revise a method or the perspective, file research into a subject page, record a hypothesis, or change the map's vocabulary."

Broker cardinality: `WorldSchemaUpdate` ≤1 per turn; `LensUpdate + PrincipleSave` stays ≤8; `SessionOpen` ≤1 per turn.

## 9. SQLite (`src-tauri/src/knowledge/schema.rs`, `KNOWLEDGE_SCHEMA_VERSION = 6`)

```sql
-- history: drop UNIQUE on record_id
CREATE TABLE knowledge_record_revisions (
  revision_id TEXT PRIMARY KEY NOT NULL,
  record_id TEXT NOT NULL REFERENCES knowledge_records(record_id) ON DELETE RESTRICT,
  revision_digest TEXT NOT NULL, payload_json TEXT NOT NULL, dependency_json TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
) STRICT;
CREATE INDEX knowledge_revisions_by_record ON knowledge_record_revisions(record_id, created_at_ms);

CREATE TABLE knowledge_lens_metadata (
  record_id TEXT PRIMARY KEY NOT NULL REFERENCES knowledge_records(record_id) ON DELETE RESTRICT,
  loupist_id TEXT NOT NULL, title TEXT NOT NULL, title_fold TEXT NOT NULL,
  lens_kind TEXT NOT NULL CHECK (lens_kind IN ('perspective','method','principle','subject','hypothesis')),
  principle_statement_fold TEXT, archived_at_ms INTEGER,
  learned INTEGER NOT NULL DEFAULT 0 CHECK (learned IN (0,1)),
  updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0)
) STRICT;
CREATE UNIQUE INDEX knowledge_lens_active_title ON knowledge_lens_metadata(loupist_id, lens_kind, title_fold) WHERE archived_at_ms IS NULL AND lens_kind <> 'principle';
CREATE UNIQUE INDEX knowledge_principle_active_statement ON knowledge_lens_metadata(loupist_id, principle_statement_fold) WHERE archived_at_ms IS NULL AND lens_kind = 'principle';
CREATE UNIQUE INDEX knowledge_one_perspective ON knowledge_lens_metadata(loupist_id) WHERE archived_at_ms IS NULL AND lens_kind = 'perspective';
-- knowledge_standing_selection: deleted

-- append-only change log: drop UNIQUE on record_id, add tier
CREATE TABLE knowledge_lens_changes (
  change_id TEXT PRIMARY KEY NOT NULL, loupist_id TEXT NOT NULL,
  record_id TEXT NOT NULL REFERENCES knowledge_records(record_id) ON DELETE RESTRICT,
  previous_revision_id TEXT, new_revision_id TEXT REFERENCES knowledge_record_revisions(revision_id) ON DELETE RESTRICT,
  rationale TEXT NOT NULL, inputs_json TEXT NOT NULL, origin_json TEXT,
  created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
  change_kind TEXT NOT NULL CHECK (change_kind IN ('create','revise','rename','archive','restore')),
  actor TEXT NOT NULL CHECK (actor IN ('user','loupist','import')),
  tier TEXT NOT NULL CHECK (tier IN ('user','shared','loupist_sourced','loupist_inferred')),
  origin_chat_id TEXT, origin_message_id TEXT, new_title TEXT
) STRICT;
CREATE INDEX knowledge_lens_changes_by_record ON knowledge_lens_changes(record_id, created_at_ms);

CREATE TABLE knowledge_lens_subjects (
  loupist_id TEXT NOT NULL, record_id TEXT NOT NULL REFERENCES knowledge_records(record_id) ON DELETE RESTRICT,
  node_id TEXT NOT NULL, name TEXT NOT NULL,
  PRIMARY KEY (record_id, node_id)
) STRICT;
CREATE INDEX knowledge_lens_subjects_by_node ON knowledge_lens_subjects(loupist_id, node_id);

CREATE TABLE knowledge_lens_links (
  loupist_id TEXT NOT NULL,
  source_record_id TEXT NOT NULL REFERENCES knowledge_records(record_id) ON DELETE RESTRICT,
  target_record_id TEXT NOT NULL,
  relation TEXT NOT NULL DEFAULT 'related' CHECK (relation IN ('related','supports','contradicts','supersedes')),
  PRIMARY KEY (source_record_id, target_record_id)
) STRICT;

CREATE TABLE knowledge_world_extensions (
  loupist_id TEXT NOT NULL, version INTEGER NOT NULL CHECK (version >= 0),
  extension_json TEXT NOT NULL,
  origin TEXT NOT NULL CHECK (origin IN ('template','model_seed','model_review','chat','undo')),
  summary TEXT NOT NULL CHECK (length(summary) <= 240),
  applied_at_ms INTEGER NOT NULL CHECK (applied_at_ms >= 0),
  PRIMARY KEY (loupist_id, version)
) STRICT;
-- knowledge_schema_proposals: deleted; GraphProcessingSettings.extension JSON: deleted (read current_extension)

ALTER knowledge_learning_reviews: ADD kind TEXT NOT NULL CHECK (kind IN ('method','connection')); the partial unique index on active status stays per Loupist (one review at a time of any kind).

CREATE TABLE knowledge_lens_lint (
  loupist_id TEXT NOT NULL, record_id TEXT,
  finding TEXT NOT NULL CHECK (finding IN ('orphan','one_way_link','uncited','entity_removed','stale','unused_kind')),
  detail TEXT NOT NULL CHECK (length(detail) <= 240),
  detected_at_ms INTEGER NOT NULL CHECK (detected_at_ms >= 0)
) STRICT;
CREATE INDEX knowledge_lens_lint_by_record ON knowledge_lens_lint(loupist_id, record_id);

-- growth events: kind gains 'hypothesis_status', 'lens_restored' is already present; 'schema_extended'/'schema_reduced' carry extension version in record_id
```

## 10. TypeScript (`src/contracts/knowledge.ts`, `src/contracts/memory.ts`, `src/chat/receipt.ts`, `src/bridge/tauriBridge.ts`)

```ts
export type LensKind = "perspective" | "method" | "principle" | "subject" | "hypothesis";
export type LensTier = "user" | "shared" | "loupist_sourced" | "loupist_inferred";
export type HypothesisStatus = "open" | "supported" | "refuted" | "dropped";

export type LensBody =
  | { kind: "perspective" | "method"; markdown: string; applicability: string; assumptions: string; procedure: string; principles: string; limitations: string }
  | { kind: "principle"; statement: string; why: string; example: string }
  | { kind: "subject"; markdown: string; about: EntityRef[]; openQuestions: string }
  | { kind: "hypothesis"; markdown: string; mechanism: string; expected: string; falsifier: string; pathClaimIds: string[]; status: HypothesisStatus; registeredAtMs: number };

export interface EntityRef { nodeId: string; name: string }
export interface LensPage { title: string; body: LensBody }
export interface LensRevisionSummary { revisionId: string; createdAtMs: number; tier: LensTier; changeKind: "create" | "revise" | "rename" | "archive" | "restore"; rationale: string }
export interface LensView { recordId: string; revisionId: string; revisionDigest: string; page: LensPage; tier: LensTier; revisions: LensRevisionSummary[]; lint: LintFinding[] }
export interface LintFinding { finding: "orphan" | "one_way_link" | "uncited" | "entity_removed" | "stale" | "unused_kind"; detail: string; detectedAtMs: number }

// memory.ts: the receipt's change record gains kind values and an undo handle
export interface MemoryChange { kind: LensKind | "source" | "journal" | "thesis" | "earnings" | "vocabulary"; operation: "created" | "revised" | "archived" | "restored" | "proposed"; recordId: string; revisionId?: string; previousRevisionId?: string; title: string; tier?: LensTier; extensionVersion?: number }

// bridge additions
interface LoupistBridge {
  lensRestoreRevision(input: { loupistId: string; recordId: string; revisionId: string }): Promise<LensView>;
  worldExtensionUndo(input: { loupistId: string; version: number }): Promise<ExtensionVersion>;
  worldExtensionHistory(input: { loupistId: string }): Promise<ExtensionVersion[]>;
  lensLint(input: { loupistId: string }): Promise<Array<LintFinding & { recordId: string | null }>>;
}
export interface ExtensionVersion { version: number; origin: "template" | "model_seed" | "model_review" | "chat" | "undo"; summary: string; appliedAtMs: number; types: string[]; relations: string[] }

// receipt.ts
const SAVED_RECEIPT_KINDS = ["subject", "hypothesis", "method", "perspective", "principle", "source", "journal", "thesis", "earnings", "vocabulary"] as const;
// usedPerspective joins usedPrinciples/usedMethods; kind "perspective" is no longer dropped.

// Host post records (src/contracts/chat.ts)
export type HomePostRecord =
  | { kind: "delivery"; runId: string; taskId: string; cadence: string; status: string; runChatId: string; learned?: LearnedSummary }
  | { kind: "learned"; learned: LearnedSummary }
  | { kind: "vocabulary"; version: ExtensionVersion }
  | { kind: "session-opened"; chatId: string; title: string; started: boolean }
  | { kind: "alert"; /* unchanged */ } | { kind: "watch" } | { kind: "earnings" } | { kind: "bot-handoff" };
export interface LearnedSummary { chatId: string; messageId: string; pages: Array<{ recordId: string; title: string; kind: LensKind; operation: "created" | "revised"; revisionId: string }>; namesAdded: number; relationshipsAdded: number; vocabulary?: ExtensionVersion }
```

## 11. Module map

| Area | Files | Owns |
| --- | --- | --- |
| Page model and write | `knowledge/model.rs`, `knowledge/lens.rs`, `knowledge/schema.rs` | `LensPage`, `LensBody`, tiers, history, restore, subjects, typed links, validation |
| Tool authority | `commands/tool_executor/authority.rs`, `commands/tool_executor.rs` | `lens_update` page staging, `world_schema_update` staging, `session_open` |
| Commit | `commands/knowledge/learning.rs`, `commands/chat_runtime/completion.rs` | persist pages, extension change, learned post hook |
| Envelope | `agent_harness.rs`, `commands/chat_runtime/setup.rs`, `commands/knowledge/context.rs` | conversation block, vocabulary in 6h, subject pointers in 6e, lint pointers 6i |
| Vocabulary | `knowledge/world_extract/ontology.rs`, `knowledge/world_schema.rs`, `knowledge/schema_writer.rs` | core list, versions, validate, apply, undo, auto-apply of seed/review |
| Background | `knowledge/graph_service.rs`, `knowledge/learning.rs`, `knowledge/connections.rs`, `knowledge/lint.rs` | review kinds, candidate finder, lint |
| Bot | `chat/home.rs`, `commands/chat_runtime/setup.rs` | learned/vocabulary/session-opened posts, Bot-only tool filter |
| Followed matching | `knowledge/map.rs`, `knowledge/assets/mod.rs` | `(symbol, market)` identity match |
| Prompt and contracts | `agent/SYSTEM.md`, `agent/tool-catalog.json`, `agent/skills/lens/SKILL.md`, `agent/prompts/connection-review.md` | role rule, file-back rule, Bot posture, schemas |
| Renderer | `src/contracts/*.ts`, `src/chat/receipt.ts`, `components/chat/ChatReceipt.tsx`, `components/memory/MemoryPage.tsx`, `components/map/MapInspector.tsx`, `components/map/whatsNew.ts`, `components/agents/LoupistTracking.tsx`, `bridge/mock/knowledge.ts`, `bridge/mock/worldSchema.ts` | Pages tab, tier badges, Restore, Undo, learned card, Bot title, vocabulary section |

Deleted: `knowledge_standing_selection` and its reads, `knowledge_schema_proposals` and `schema_writer` proposal states, payload `lensKind`, `world_schema.rs` CORE_* constants, `LoupistTracking.tsx` proposal approve/dismiss UI, `GraphProcessingSettings.extension`, lexical note pointers for subjects in 6e (notes keep their own pointer), `LensDocument` as a public name (renamed `MethodDocument`).
