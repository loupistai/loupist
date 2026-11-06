use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{KnowledgeError, KnowledgeResult};

pub const MAX_BLOB_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_NODE_LIMIT: u32 = 25;
pub const MAX_NODE_LIMIT: u32 = 100;
pub const DEFAULT_BYTE_LIMIT: u32 = 262_144;
pub const MAX_BYTE_LIMIT: u32 = 1_048_576;
pub const MAX_WORLD_NODES: usize = 10_000;
pub const MAX_WORLD_DEPENDENCIES: usize = 10_000;
pub const MAX_WORLD_CLAIMS: usize = 10_000;

macro_rules! enum_str {
    ($ty:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        impl $ty {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)+
                }
            }

            pub fn parse(value: &str) -> KnowledgeResult<Self> {
                match value {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(KnowledgeError::Invalid("stored value is invalid")),
                }
            }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Source,
    World,
    Lens,
}
enum_str!(RecordKind { Source => "source", World => "world", Lens => "lens" });

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceClass {
    PublisherDocument,
    Dataset,
    UserStatement,
    ToolResult,
    Analysis,
    ImportedContributor,
    CodeRevision,
    WebPage,
    HistoricalDocument,
    UserFile,
}
enum_str!(SourceClass {
    PublisherDocument => "publisher_document",
    Dataset => "dataset",
    UserStatement => "user_statement",
    ToolResult => "tool_result",
    Analysis => "analysis",
    ImportedContributor => "imported_contributor",
    CodeRevision => "code_revision",
    WebPage => "web_page",
    HistoricalDocument => "historical_document",
    UserFile => "user_file",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    IncludedFull,
    IncludedExcerpt,
    UpstreamTruncated,
    MetadataOnly,
    Omitted,
}
enum_str!(Coverage {
    IncludedFull => "included_full",
    IncludedExcerpt => "included_excerpt",
    UpstreamTruncated => "upstream_truncated",
    MetadataOnly => "metadata_only",
    Omitted => "omitted",
});

impl Coverage {
    pub fn retains_bytes(self) -> bool {
        !matches!(self, Self::MetadataOnly | Self::Omitted)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authorship {
    Publisher,
    User,
    Tool,
    Analysis,
    ImportedContributor,
}
enum_str!(Authorship {
    Publisher => "publisher",
    User => "user",
    Tool => "tool",
    Analysis => "analysis",
    ImportedContributor => "imported_contributor",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepresentationRole {
    Original,
    ProviderPayload,
    ExtractedText,
    Structured,
    Excerpt,
    Derived,
}
enum_str!(RepresentationRole {
    Original => "original",
    ProviderPayload => "provider_payload",
    ExtractedText => "extracted_text",
    Structured => "structured",
    Excerpt => "excerpt",
    Derived => "derived",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyRole {
    MethodologicalSource,
    ComposedLens,
    WorldSource,
    ComposedWorld,
    SourceDerivation,
}
impl DependencyRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MethodologicalSource => "methodological_source",
            Self::ComposedLens => "composed_lens",
            Self::WorldSource => "world_source",
            Self::ComposedWorld => "composed_world",
            Self::SourceDerivation => "source_derivation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingRole {
    ExtractedFrom,
    Supports,
    Opposes,
    Context,
    Analysis,
}
enum_str!(BindingRole {
    ExtractedFrom => "extracted_from",
    Supports => "supports",
    Opposes => "opposes",
    Context => "context",
    Analysis => "analysis",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Active,
    Disputed,
    Withdrawn,
}
enum_str!(ClaimStatus { Active => "active", Disputed => "disputed", Withdrawn => "withdrawn" });

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimePrecision {
    Unknown,
    Year,
    Month,
    Date,
    Instant,
}
enum_str!(TimePrecision {
    Unknown => "unknown",
    Year => "year",
    Month => "month",
    Date => "date",
    Instant => "instant",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LensUseStatus {
    Considered,
    Applied,
    Adapted,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EarlierRelation {
    Review,
    Supersedes,
    Contradicts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphCoverage {
    SealedSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexingState {
    SnapshotOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "purpose",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ReadPurpose {
    #[serde(rename = "renderer_inspection")]
    RendererInspection,
    #[serde(rename = "chat_turn")]
    ChatTurn {
        #[serde(rename = "chatSessionId")]
        chat_session_id: String,
        #[serde(rename = "turnId")]
        turn_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Selector {
    #[serde(rename = "byte_range")]
    ByteRange { start: u64, end: u64 },
    #[serde(rename = "utf8_line_range")]
    Utf8LineRange {
        #[serde(rename = "startLine")]
        start_line: u32,
        #[serde(rename = "endLine")]
        end_line: u32,
    },
    #[serde(rename = "json_pointer")]
    JsonPointer { pointer: String },
    #[serde(rename = "row")]
    Row { index: u32 },
    #[serde(rename = "part")]
    Part {
        #[serde(rename = "documentId")]
        document_id: String,
        #[serde(rename = "partId")]
        part_id: String,
        start: u64,
        end: u64,
    },
    #[serde(rename = "multi")]
    Multi { documents: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimePoint {
    pub precision: TimePrecision,
    pub value: Option<String>,
    #[serde(default)]
    pub approximate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceRevision {
    pub record_id: String,
    pub revision_digest: String,
    pub role: DependencyRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateRecord {
    pub loupist_id: String,
    pub kind: RecordKind,
    pub title: String,
    pub at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedRecord {
    pub record_id: String,
    pub kind: RecordKind,
    pub owner_loupist_id: String,
    pub title: String,
    pub revision_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordRevision {
    pub record_id: String,
    pub kind: RecordKind,
    pub revision_id: String,
    pub revision_digest: String,
    pub dependencies: Vec<EvidenceRevision>,
    pub payload: Value,
    pub created_at_ms: i64,
}

#[cfg(feature = "eval-harness")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordSnapshot {
    pub format_version: u32,
    pub record_id: String,
    pub kind: RecordKind,
    pub revision_id: String,
    pub revision_digest: String,
    pub dependencies: Vec<EvidenceRevision>,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetainMaterial {
    pub media_type: String,
    pub role: RepresentationRole,
    pub document_id: String,
    pub parser_version: Option<String>,
    pub parent_representation_id: Option<String>,
    pub input_span_ids: Vec<String>,
    pub bytes: Vec<u8>,
    pub selector: Selector,
    pub context_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetainSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<crate::custom_connections::taint::CustomOrigin>,
    pub loupist_id: String,
    pub record_id: String,
    pub expected_revision_id: Option<String>,
    pub at_ms: i64,
    pub source_id: Option<String>,
    pub title: String,
    pub source_class: SourceClass,
    pub origin_locator: String,
    pub provider_identity: String,
    pub authorship: Authorship,
    pub coverage: Coverage,
    pub origin_chat_id: Option<String>,
    pub request_ref: Option<String>,
    pub declared_digest: Option<String>,
    pub material: Option<RetainMaterial>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_time: Option<TimePoint>,
}

/// A new Loupist-owned acquisition. Callers never choose a destination record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetainNewSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<crate::custom_connections::taint::CustomOrigin>,
    pub loupist_id: String,
    pub at_ms: i64,
    pub title: String,
    pub source_class: SourceClass,
    pub origin_locator: String,
    pub provider_identity: String,
    pub authorship: Authorship,
    pub coverage: Coverage,
    pub origin_chat_id: Option<String>,
    pub request_ref: Option<String>,
    pub declared_digest: Option<String>,
    pub material: Option<RetainMaterial>,
    pub publication_time: Option<TimePoint>,
}

impl RetainNewSource {
    pub fn into_capture(
        self,
        record_id: String,
        expected_revision_id: Option<String>,
        source_id: Option<String>,
    ) -> RetainSource {
        RetainSource {
            origin: self.origin,
            loupist_id: self.loupist_id,
            record_id,
            expected_revision_id,
            source_id,
            at_ms: self.at_ms,
            title: self.title,
            source_class: self.source_class,
            origin_locator: self.origin_locator,
            provider_identity: self.provider_identity,
            authorship: self.authorship,
            coverage: self.coverage,
            origin_chat_id: self.origin_chat_id,
            request_ref: self.request_ref,
            declared_digest: self.declared_digest,
            material: self.material,
            publication_time: self.publication_time,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetainedSource {
    pub record_id: String,
    pub source_id: String,
    pub capture_id: String,
    pub representation_id: Option<String>,
    pub span_id: Option<String>,
    pub selected_digest: Option<String>,
    pub material_digest: Option<String>,
    pub revision_id: String,
    pub revision_digest: String,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceSummary {
    pub source_id: String,
    pub title: String,
    pub source_class: SourceClass,
    pub capture_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishedSpan {
    pub span_id: String,
    pub selector: Selector,
    pub selected_digest: String,
    pub context_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishedRepresentation {
    pub representation_id: String,
    pub parent_representation_id: Option<String>,
    pub document_id: String,
    pub digest: String,
    pub media_type: String,
    pub byte_length: u64,
    pub role: RepresentationRole,
    pub parser_version: Option<String>,
    pub input_span_ids: Vec<String>,
    pub spans: Vec<PublishedSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishedCapture {
    pub capture_id: String,
    pub received_at_ms: i64,
    pub coverage: Coverage,
    pub declared_digest: Option<String>,
    pub material_digest: Option<String>,
    pub authorship: Authorship,
    pub representations: Vec<PublishedRepresentation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_time: Option<TimePoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishedSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<crate::custom_connections::taint::CustomOrigin>,
    pub source_id: String,
    pub title: String,
    pub source_class: SourceClass,
    pub origin_locator: Option<String>,
    pub provider_identity: String,
    pub captures: Vec<PublishedCapture>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceRecordPayload {
    pub source: PublishedSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceReadReceipt {
    pub receipt_id: String,
    pub loupist_id: String,
    pub span_id: String,
    pub capture_id: String,
    pub representation_id: String,
    pub selected_digest: String,
    pub purpose: ReadPurpose,
    pub issued_at_ms: i64,
}

#[cfg(feature = "eval-harness")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpanRead {
    pub bytes: Vec<u8>,
    pub media_type: String,
    pub representation_role: RepresentationRole,
    pub coverage: Coverage,
    pub selected_digest: String,
    pub receipt: SourceReadReceipt,
}

#[cfg(feature = "eval-harness")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadSpan {
    pub loupist_id: String,
    pub record_id: String,
    pub span_id: String,
    pub at_ms: i64,
}

#[cfg(feature = "eval-harness")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadCapture {
    pub loupist_id: String,
    pub record_id: String,
    pub capture_id: String,
    pub at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Qualifier {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub kind: String,
    pub source_role: String,
    pub properties: std::collections::BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimEndpoint {
    pub node_id: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceBinding {
    pub span_id: String,
    pub role: BindingRole,
    pub attribution: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphClaim {
    pub id: String,
    pub proposition: String,
    pub attribution: String,
    pub relation: String,
    pub endpoints: Vec<ClaimEndpoint>,
    pub qualifiers: Vec<Qualifier>,
    pub evidence: Vec<EvidenceBinding>,
    pub status: ClaimStatus,
    pub recorded_at_ms: Option<i64>,
    pub publication_time: Option<TimePoint>,
    pub effective_time: Option<TimePoint>,
    /// Fact validity start. Independent of claim status and of `expired_at`.
    pub valid_at: Option<TimePoint>,
    /// Fact validity end. Independent of claim status and of `expired_at`.
    pub invalid_at: Option<TimePoint>,
    /// Graphiti system-time expiry. Independent of fact validity and claim status.
    pub expired_at: Option<TimePoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TopicAnnotation {
    pub topic_id: String,
    pub markdown: String,
    pub source_span_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldSnapshotBody {
    pub nodes: Vec<GraphNode>,
    pub claims: Vec<GraphClaim>,
    pub annotations: Vec<TopicAnnotation>,
    /// Observed relationship names. Open vocabulary, counted at seal time.
    #[serde(default)]
    pub relation_vocabulary: Vec<RelationCount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelationCount {
    pub relation: String,
    pub claims: u32,
    #[serde(default)]
    pub last_recorded_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SealWorld {
    pub loupist_id: String,
    pub record_id: String,
    pub expected_revision_id: Option<String>,
    pub at_ms: i64,
    pub nodes: Vec<GraphNode>,
    pub claims: Vec<GraphClaim>,
    pub annotations: Vec<TopicAnnotation>,
    pub dependencies: Vec<EvidenceRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldRevision {
    pub snapshot_id: String,
    pub record_id: String,
    pub revision_id: String,
    pub revision_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphEdge {
    pub id: String,
    pub relation: String,
    pub endpoints: Vec<ClaimEndpoint>,
    pub status: ClaimStatus,
    pub claim: GraphClaim,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldGraphQuery {
    pub loupist_id: String,
    pub record_id: String,
    pub revision_id: String,
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub center_node_id: Option<String>,
    #[serde(default)]
    pub center_claim_id: Option<String>,
    #[serde(default)]
    pub kinds: Vec<String>,
    #[serde(default)]
    pub relations: Vec<String>,
    #[serde(default)]
    pub binding_roles: Vec<BindingRole>,
    #[serde(default)]
    pub statuses: Vec<ClaimStatus>,
    #[serde(default)]
    pub publication_times: Vec<String>,
    /// An as-of cutoff (`YYYY-MM-DD`): only relationships published by then,
    /// and only the names they connect, reduced to name and kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_by: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub node_limit: Option<u32>,
    #[serde(default)]
    pub edge_limit: Option<u32>,
    #[serde(default)]
    pub byte_limit: Option<u32>,
}

impl WorldGraphQuery {
    pub fn limits(&self) -> KnowledgeResult<(u32, u32, u32)> {
        let nodes = self.node_limit.unwrap_or(DEFAULT_NODE_LIMIT);
        let edges = self.edge_limit.unwrap_or(DEFAULT_NODE_LIMIT);
        let bytes = self.byte_limit.unwrap_or(DEFAULT_BYTE_LIMIT);
        if nodes == 0
            || nodes > MAX_NODE_LIMIT
            || edges == 0
            || edges > MAX_NODE_LIMIT
            || bytes == 0
            || bytes > MAX_BYTE_LIMIT
        {
            return Err(KnowledgeError::Invalid("query limit is invalid"));
        }
        Ok((nodes, edges, bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldGraphView {
    #[serde(default)]
    pub source_targets: std::collections::BTreeMap<String, super::WorldSourceTarget>,
    pub record_id: String,
    pub revision_id: String,
    pub revision_digest: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub next_cursor: Option<String>,
    pub coverage: GraphCoverage,
    pub indexing_state: IndexingState,
    pub truncated: bool,
    pub scan: QueryScan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryScan {
    pub strategy: String,
    pub nodes_scanned: u32,
    pub claims_scanned: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LensAcknowledgement {
    pub delivery_id: String,
    pub status: LensUseStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EarlierResearch {
    pub research_id: String,
    pub journal_id: Option<String>,
    pub relation: EarlierRelation,
    /// Exact text knowledge.read returned for this earlier journal.
    #[serde(default)]
    pub delivered_text: String,
    #[serde(default)]
    pub delivered_digest: String,
    #[serde(default)]
    pub coverage: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalCheck {
    pub claim: String,
    pub metric: String,
    pub direction: String,
    pub threshold: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Required for a dated check; optional only for an asset page's line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalOutcomeDraft {
    pub observed: f64,
    pub observed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default)]
    pub unclear: bool,
    /// The as-of date the review ran under; set by the host, never the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_as_of: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalOutcome {
    pub observed: f64,
    pub observed_at: String,
    pub result: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Set when the review ran under an as-of date ("Reviewed as of …").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_as_of: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<JournalCheck>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thesis: Option<super::assets::JournalThesis>,
    pub outcome: Option<JournalOutcomeDraft>,
    pub judgment: String,
    pub reasoning: String,
    pub source_receipt_ids: Vec<String>,
    pub world_delivery_ids: Vec<String>,
    pub lenses: Vec<LensAcknowledgement>,
    pub earlier: Vec<EarlierResearch>,
    #[serde(default)]
    pub entities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveResearch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_inputs: Option<NoteInputDraft>,
    pub loupist_id: String,
    pub turn_id: String,
    pub at_ms: i64,
    pub observations: String,
    pub interpretation: String,
    pub assumptions: String,
    pub evaluation: String,
    pub journal: Option<JournalDraft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_note: Option<WorkingNoteDraft>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoteInputDraft {
    pub source_receipt_ids: Vec<String>,
    pub world_delivery_ids: Vec<String>,
    pub lenses: Vec<LensAcknowledgement>,
    pub previous_note_ids: Vec<String>,
}

/// A task-shaped summary, not a source or an independently verified judgment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkingNoteDraft {
    pub title: String,
    pub markdown: String,
    pub change_summary: String,
    pub expected_research_id: Option<String>,
}

/// Shared by tool acceptance and the canonical research transaction.
pub(crate) fn validate_working_note(note: &WorkingNoteDraft) -> KnowledgeResult<()> {
    validate_text(&note.title, 160, false, "working note title is invalid")?;
    validate_text(
        &note.markdown,
        16_000,
        false,
        "working note body is invalid",
    )?;
    validate_text(
        &note.change_summary,
        500,
        false,
        "working note change is invalid",
    )?;
    validate_optional_knowledge_id(&note.expected_research_id, "rsh")
}

/// Discovery pointers to later committed notes, not evidence or read authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaterNote {
    pub note_id: String,
    pub chat_id: String,
    pub title: String,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkingNote {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub later_notes: Vec<LaterNote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<JournalInputs>,
    #[serde(default)]
    pub previous_note_ids: Vec<String>,
    pub research_id: String,
    pub chat_id: String,
    pub title: String,
    pub markdown: String,
    pub change_summary: String,
    pub recorded_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalSourceInput {
    pub capture_id: String,
    pub representation_id: String,
    pub span_id: String,
    pub selected_digest: String,
    pub receipt_id: String,
    pub record_id: String,
    pub revision_id: String,
    pub revision_digest: String,
    pub attribution: String,
    pub coverage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_time: Option<TimePoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalWorldInput {
    pub delivered_view_json: String,
    pub delivery_id: String,
    pub record_id: String,
    pub revision_id: String,
    pub revision_digest: String,
    pub view_digest: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalLensInput {
    pub delivery_id: String,
    pub record_id: String,
    pub revision_id: String,
    pub revision_digest: String,
    pub body_digest: String,
    /// The page as delivered, rendered to Markdown with section keys.
    pub markdown: String,
    pub status: LensUseStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalInputs {
    pub sources: Vec<JournalSourceInput>,
    pub worlds: Vec<JournalWorldInput>,
    pub lenses: Vec<JournalLensInput>,
    pub earlier: Vec<EarlierResearch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<JournalCheck>,
    /// A reason for following an asset, or a line that would change the user's mind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thesis: Option<super::assets::JournalThesis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<JournalOutcome>,
    pub journal_id: String,
    pub research_id: String,
    pub loupist_id: String,
    pub judgment: String,
    pub reasoning: String,
    pub inputs: JournalInputs,
    pub input_digest: String,
    pub created_at_ms: i64,
    #[serde(default)]
    pub entities: Vec<String>,
    pub origin_chat_id: Option<String>,
    pub updated_at_ms: i64,
    pub archived: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedResearch {
    pub research_id: String,
    pub record_digest: String,
    pub journal: Option<JournalRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenTurn {
    pub loupist_id: String,
    pub chat_session_id: String,
    pub turn_id: String,
    pub at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchClaimPin {
    pub record_id: String,
    pub revision_id: String,
    pub claim_id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchHit {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub later_notes: Vec<LaterNote>,
    pub research_id: String,
    pub chat_id: Option<String>,
    pub journal_id: Option<String>,
    pub conclusion: String,
    pub uncertainty: String,
    pub recorded_at_ms: i64,
    pub world_view_digests: Vec<String>,
    pub lens_bodies: Vec<String>,
    pub claims: Vec<ResearchClaimPin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchSearch {
    pub loupist_id: String,
    pub query: String,
    pub records_scanned: u32,
    pub hits: Vec<ResearchHit>,
}

pub(super) fn validate_loupist_id(value: &str) -> KnowledgeResult<()> {
    validate_identifier(value, 128, "loupist id is invalid")
}

pub(super) fn validate_external_id(value: &str, label: &'static str) -> KnowledgeResult<()> {
    validate_identifier(value, 128, label)
}

pub(super) fn validate_knowledge_id(value: &str, prefix: &str) -> KnowledgeResult<()> {
    let Some(rest) = value
        .strip_prefix(prefix)
        .and_then(|suffix| suffix.strip_prefix('_'))
    else {
        return Err(KnowledgeError::Invalid("identifier is invalid"));
    };
    if rest.len() == 32 && rest.bytes().all(is_lower_hex) {
        Ok(())
    } else {
        Err(KnowledgeError::Invalid("identifier is invalid"))
    }
}

pub(super) fn validate_optional_knowledge_id(
    value: &Option<String>,
    prefix: &str,
) -> KnowledgeResult<()> {
    if let Some(value) = value {
        validate_knowledge_id(value, prefix)?;
    }
    Ok(())
}

pub(super) fn validate_digest(value: &str) -> KnowledgeResult<()> {
    if value.len() == 64 && value.bytes().all(is_lower_hex) {
        Ok(())
    } else {
        Err(KnowledgeError::Invalid("digest is invalid"))
    }
}

pub(super) fn validate_optional_digest(value: &Option<String>) -> KnowledgeResult<()> {
    if let Some(value) = value {
        validate_digest(value)?;
    }
    Ok(())
}

pub(super) fn validate_time(at_ms: i64) -> KnowledgeResult<()> {
    if at_ms < 0 {
        Err(KnowledgeError::Invalid("timestamp is invalid"))
    } else {
        Ok(())
    }
}

pub(super) fn validate_title(value: &str) -> KnowledgeResult<()> {
    if value.trim().is_empty() || value.chars().count() > 200 || value.chars().any(char::is_control)
    {
        return Err(KnowledgeError::Invalid("title is invalid"));
    }
    Ok(())
}

pub(crate) fn validate_text(
    value: &str,
    max: usize,
    empty_ok: bool,
    label: &'static str,
) -> KnowledgeResult<()> {
    if value.len() > max || value.chars().any(is_disallowed_text) {
        return Err(KnowledgeError::Invalid(label));
    }
    if !empty_ok && value.trim().is_empty() {
        return Err(KnowledgeError::Invalid(label));
    }
    Ok(())
}

pub(super) fn validate_token(value: &str, max: usize, label: &'static str) -> KnowledgeResult<()> {
    if value.is_empty()
        || value.len() > max
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
    {
        return Err(KnowledgeError::Invalid(label));
    }
    Ok(())
}

pub(super) fn validate_media_type(value: &str) -> KnowledgeResult<()> {
    if (3..=128).contains(&value.len())
        && value.contains('/')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b' ')
    {
        Ok(())
    } else {
        Err(KnowledgeError::Invalid("media type is invalid"))
    }
}

pub(crate) fn validate_time_point(point: &TimePoint) -> KnowledgeResult<TimePoint> {
    super::time::normalize_time_point(point)
}

fn validate_identifier(value: &str, max: usize, label: &'static str) -> KnowledgeResult<()> {
    if value.is_empty()
        || value.len() > max
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(KnowledgeError::Invalid(label));
    }
    Ok(())
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn is_disallowed_text(character: char) -> bool {
    character == '\0' || (character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
}

pub(super) fn mint_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}
