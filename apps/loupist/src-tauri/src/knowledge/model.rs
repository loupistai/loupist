use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{KnowledgeError, KnowledgeResult};
use world_episodes::EndBasis;

#[path = "world_episodes.rs"]
#[allow(dead_code)] // Live publication and change feeds land after the schema step.
pub mod world_episodes;

pub const SUPERSEDED_BY_SPAN: &str = "superseded_by_span";
pub const SUPERSEDED_BY_CLAIM: &str = "superseded_by_claim";
pub const END_BASIS: &str = "end_basis";
pub const CONFLICTS_WITH_CLAIM: &str = "conflicts_with_claim";

/// A restatement keeps the claim current against a pair that once ended it:
/// the approximate day it became current again, and the Source that
/// witnessed the restatement.
pub const REOPENED_AT: &str = "reopened_at";
pub const REOPENED_BY_SPAN: &str = "reopened_by_span";
/// The gaps when it was not current, as `from/to` day pairs separated by
/// spaces, the latest [`MAX_LAPSED`].
pub const LAPSED: &str = "lapsed";

pub(crate) fn is_temporal_qualifier(key: &str) -> bool {
    matches!(
        key,
        SUPERSEDED_BY_SPAN
            | SUPERSEDED_BY_CLAIM
            | END_BASIS
            | CONFLICTS_WITH_CLAIM
            | REOPENED_AT
            | REOPENED_BY_SPAN
            | LAPSED
    )
}

/// The host qualifiers of a reopening (master plan 9.10). Every time in them
/// is approximate, so only the value is written.
pub(crate) fn reopening_qualifiers(
    reopened: Option<&Reopened>,
    lapsed: &[Lapse],
) -> Vec<(&'static str, String)> {
    let Some(reopened) = reopened else {
        return Vec::new();
    };
    let value = |time: &TimePoint| time.value.clone().unwrap_or_default();
    let mut result = vec![
        (REOPENED_AT, value(&reopened.at)),
        (REOPENED_BY_SPAN, reopened.by_span.clone()),
    ];
    if !lapsed.is_empty() {
        let gaps: Vec<String> = lapsed
            .iter()
            .map(|lapse| format!("{}/{}", value(&lapse.from), value(&lapse.to)))
            .collect();
        result.push((LAPSED, gaps.join(" ")));
    }
    result
}

#[cfg(test)]
#[test]
fn times_compare_symmetrically_at_common_precision() {
    let point = |precision, value: &str| TimePoint {
        precision,
        value: Some(value.to_owned()),
        approximate: false,
    };
    let month = point(TimePrecision::Month, "2026-04");
    let day = point(TimePrecision::Date, "2026-04-10");
    let future = point(TimePrecision::Instant, "2026-06-30T10:00:00Z");
    assert_eq!(month.compare_at_common_precision(&day), None);
    assert_eq!(day.compare_at_common_precision(&month), None);
    assert_eq!(
        day.compare_at_common_precision(&future),
        Some(Ordering::Less)
    );
    assert_eq!(
        future.compare_at_common_precision(&day),
        Some(Ordering::Greater)
    );
    let claim: GraphClaim = serde_json::from_value(serde_json::json!({
        "id": "claim",
        "proposition": "Current appointment",
        "attribution": "report",
        "relation": "holds",
        "endpoints": [],
        "qualifiers": [],
        "evidence": [],
        "status": "active",
        "invalidAt": future,
    }))
    .unwrap();
    assert!(claim.is_current(&day));
    assert!(!claim.is_current(&point(TimePrecision::Date, "2026-06-30")));

    for utc in ["2026-06-30T00:01:00Z", "2026-06-30T23:59:00Z"] {
        let millis = chrono::DateTime::parse_from_rfc3339(utc)
            .unwrap()
            .timestamp_millis();
        let date = crate::local_time::date_at_ms(millis).unwrap().to_string();
        let at = TimePoint::from_unix_ms(millis);
        assert_eq!(
            at.compare_at_common_precision(&point(TimePrecision::Date, &date)),
            None,
        );
    }
}

/// Research queries are bounded by Unicode scalar values, independent of UTF-8 width.
pub const RESEARCH_QUERY_MAX_CHARS: usize = 200;
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
#[cfg_attr(test, derive(ts_rs::TS))]
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
    /// Original material the user pasted into a Chat message (a filing
    /// excerpt, an article, a notice, a table), kept verbatim. Its origin is
    /// what the user stated, never verified (`ProvidedOrigin`).
    UserProvided,
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
    UserProvided => "user_provided",
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
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Active,
    Disputed,
    Withdrawn,
}
enum_str!(ClaimStatus { Active => "active", Disputed => "disputed", Withdrawn => "withdrawn" });

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LensUseStatus {
    Considered,
    Applied,
    Adapted,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum EarlierRelation {
    Review,
    Supersedes,
    Contradicts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum GraphCoverage {
    SealedSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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

// Also a request field (retained Sources): its TypeScript is the accepted shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimePoint {
    pub precision: TimePrecision,
    pub value: Option<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub approximate: bool,
}

impl TimePoint {
    pub fn from_unix_ms(at_ms: i64) -> Self {
        let value = crate::local_time::at_ms(at_ms).map(|instant| instant.to_rfc3339());
        let precision = if value.is_some() {
            TimePrecision::Instant
        } else {
            TimePrecision::Unknown
        };
        Self {
            precision,
            value,
            approximate: false,
        }
    }
    /// No order when either time is unknown or both occupy the same bucket.
    pub fn compare_at_common_precision(&self, other: &Self) -> Option<Ordering> {
        self.compare_buckets(other).filter(|order| !order.is_eq())
    }

    pub fn at_or_before(&self, other: &Self) -> bool {
        self.compare_buckets(other)
            .is_some_and(|order| !order.is_gt())
    }

    fn compare_buckets(&self, other: &Self) -> Option<Ordering> {
        use TimePrecision::*;
        let left = self.value.as_deref()?;
        let right = other.value.as_deref()?;
        let width = match (self.precision, other.precision) {
            (Unknown, _) | (_, Unknown) => return None,
            (Year, _) | (_, Year) => 4,
            (Month, _) | (_, Month) => 7,
            (Date, _) | (_, Date) => 10,
            (Instant, Instant) => {
                let left = chrono::DateTime::parse_from_rfc3339(left).ok()?;
                let right = chrono::DateTime::parse_from_rfc3339(right).ok()?;
                return Some(left.cmp(&right));
            }
        };
        Some(left.get(..width)?.cmp(right.get(..width)?))
    }
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
#[cfg_attr(test, derive(ts_rs::TS))]
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

/// Where a user-provided capture (`SourceClass::UserProvided`) came from: the
/// Chat message its bytes were copied from verbatim, when that message was
/// sent, and the link and publisher the user named for the material. The host
/// checked that the text, the link and the publisher appear in the user's own
/// messages in that Chat; it never checked that the material comes from that
/// link or publisher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProvidedOrigin {
    pub chat_id: String,
    pub message_id: String,
    pub pasted_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub stated_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub stated_publisher: Option<String>,
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
    /// Set exactly for a user-provided capture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provided: Option<ProvidedOrigin>,
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Qualifier {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimEndpoint {
    pub node_id: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceBinding {
    pub span_id: String,
    pub role: BindingRole,
    pub attribution: String,
    /// How the cited passage's figures stood against the proposition when
    /// World published this support. Absent: no figure was checked, or the
    /// snapshot predates figure checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub figure_check: Option<EvidenceFigure>,
    /// The chunk of the Source this support was extracted from. Absent: unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub passage: Option<PassageRange>,
}

/// A stored figure check. A figure that differs is never published, and an
/// unchecked one stores nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum EvidenceFigure {
    Matched,
    Rounded,
    NotFound,
}

impl EvidenceFigure {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::Rounded => "rounded",
            Self::NotFound => "not_found",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "matched" => Some(Self::Matched),
            "rounded" => Some(Self::Rounded),
            "not_found" => Some(Self::NotFound),
            _ => None,
        }
    }
}

/// A chunk of a Source's text, in Unicode scalar values from its start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PassageRange {
    pub chunk_index: u32,
    pub char_start: u32,
    pub char_end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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

impl GraphClaim {
    pub fn is_current(&self, at: &TimePoint) -> bool {
        self.status == ClaimStatus::Active && !self.has_ended(at)
    }

    pub fn has_ended(&self, at: &TimePoint) -> bool {
        self.invalid_at
            .as_ref()
            .is_some_and(|end| end.at_or_before(at))
    }

    pub fn qualifier(&self, key: &str) -> Option<&str> {
        self.qualifiers
            .iter()
            .find(|qualifier| qualifier.key == key)
            .map(|qualifier| qualifier.value.as_str())
    }

    pub(crate) fn connects_names(&self) -> bool {
        self.endpoints.first().is_some_and(|first| {
            self.endpoints[1..]
                .iter()
                .any(|end| end.node_id != first.node_id)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TopicAnnotation {
    pub topic_id: String,
    pub markdown: String,
    pub source_span_ids: Vec<String>,
}

/// Resolver selections are retained even when the public qualifiers change.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorldTemporal {
    pub links: Vec<AssertionLink>,
    pub assertions: BTreeMap<String, AssertionSources>,
}

impl WorldTemporal {
    fn is_empty(&self) -> bool {
        self.links.is_empty() && self.assertions.is_empty()
    }

    pub(crate) fn witness_spans(&self) -> impl Iterator<Item = &str> {
        let assertions = self.assertions.values().flat_map(|assertion| {
            assertion
                .witness
                .iter()
                .map(|witness| witness.span.as_str())
                .chain(
                    assertion
                        .stated
                        .iter()
                        .map(|stated| stated.witness.span.as_str()),
                )
        });
        assertions.chain(self.links.iter().flat_map(|link| {
            link.selected_by
                .iter()
                .chain(&link.same_assertion_by)
                .map(|witness| witness.span.as_str())
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AssertionLink {
    pub left: String,
    pub right: String,
    pub same_assertion: bool,
    /// The episode that selected these claims as a pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_by: Option<AssertionWitness>,
    /// The episode that selected them as the same assertion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub same_assertion_by: Option<AssertionWitness>,
    /// The published episodes that selected them as the same assertion, in
    /// [`witness_order`], the latest [`MAX_REAFFIRMED`] kept. A restatement
    /// after the other side's last day counts only with one of these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reaffirmed_by: Vec<AssertionWitness>,
}

pub(crate) const MAX_REAFFIRMED: usize = 8;
pub(crate) const MAX_LAPSED: usize = 8;

impl AssertionLink {
    pub(crate) fn reaffirm(&mut self, witness: &AssertionWitness) {
        if witness.publication.is_none() || self.reaffirmed_by.contains(witness) {
            return;
        }
        self.reaffirmed_by.push(witness.clone());
        self.reaffirmed_by.sort_by(witness_order);
        let excess = self.reaffirmed_by.len().saturating_sub(MAX_REAFFIRMED);
        self.reaffirmed_by.drain(..excess);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AssertionSources {
    pub source_ids: BTreeSet<String>,
    pub witness: Option<AssertionWitness>,
    pub stated: Option<StatedEnd>,
    /// Publication days of the published-basis Sources that assert it, one
    /// per day in [`day_key`] order. Empty on older claims: the witness's day.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub days: Vec<TimePoint>,
    /// Set while a restatement keeps it current against a pair that once
    /// ended it by reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reopened: Option<Reopened>,
    /// The gaps between that reference ending and each restatement, the
    /// latest [`MAX_LAPSED`] kept.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lapsed: Vec<Lapse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Reopened {
    pub at: TimePoint,
    pub by_span: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Lapse {
    pub from: TimePoint,
    pub to: TimePoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AssertionWitness {
    pub span: String,
    pub publication: Option<TimePoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StatedEnd {
    pub time: TimePoint,
    pub witness: AssertionWitness,
}

pub(crate) struct Assertion<'a> {
    pub relation: &'a str,
    pub endpoints: [&'a str; 2],
    pub valid_at: Option<&'a TimePoint>,
    pub assertions: &'a AssertionSources,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ending {
    pub time: TimePoint,
    pub by: Option<String>,
    pub span: String,
    pub basis: EndBasis,
    /// The ending claim states no end of its own: a successor, not an end
    /// fact. Never stored; the order of two endings reads it.
    pub ender_open: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PairOutcome {
    Independent,
    Conflict,
    End {
        left: bool,
        time: TimePoint,
        basis: EndBasis,
        /// Set when a restatement keeps the other side current: the Source
        /// that witnessed it, which also establishes this ending.
        restated: Option<Restated>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Restated {
    pub witness: AssertionWitness,
    /// When the current side was not current, latest [`MAX_LAPSED`] kept.
    pub lapsed: Vec<Lapse>,
}

/// What the resolver selected about a pair, as far as it is known.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PairSelection<'a> {
    pub same_assertion: bool,
    pub reaffirmed_by: &'a [AssertionWitness],
}

pub(crate) fn decide_pair(
    left: Assertion<'_>,
    right: Assertion<'_>,
    selection: PairSelection<'_>,
) -> PairOutcome {
    // A later repeat from a common Source leaves independent assertions intact.
    if left.assertions.source_ids.len() == 1
        && right.assertions.source_ids.len() == 1
        && left.assertions.source_ids == right.assertions.source_ids
    {
        return PairOutcome::Independent;
    }

    let left_start = left.valid_at;
    let right_start = right.valid_at;
    let left_end = left.assertions.stated.as_ref().map(|end| &end.time);
    let right_end = right.assertions.stated.as_ref().map(|end| &end.time);
    let before = |left: Option<&TimePoint>, right: Option<&TimePoint>| {
        left.zip(right)
            .and_then(|(a, b)| a.compare_at_common_precision(b))
            == Some(Ordering::Less)
    };
    let ends_before = |end: Option<&TimePoint>, start: Option<&TimePoint>| {
        end.zip(start)
            .is_some_and(|(end, start)| end.at_or_before(start))
    };
    if ends_before(left_end, right_start) || ends_before(right_end, left_start) {
        return PairOutcome::Independent;
    }
    let stated = |left, time: &TimePoint| PairOutcome::End {
        left,
        time: time.clone(),
        basis: EndBasis::Stated,
        restated: None,
    };
    // The resolver selected the earlier assertion. A separate termination can
    // omit the start or restate the same start while adding the boundary.
    if let Some(end) = right_end {
        if (right_start.is_none() || same_start(left_start, right_start))
            && before(left_start, Some(end))
        {
            return stated(true, end);
        }
    }
    if let Some(end) = left_end {
        if (left_start.is_none() || same_start(left_start, right_start))
            && before(right_start, Some(end))
        {
            return stated(false, end);
        }
    }
    if let (Some(a), Some(b)) = (left_start, right_start) {
        return match a.compare_at_common_precision(b) {
            Some(Ordering::Less) => stated(true, b),
            Some(Ordering::Greater) => stated(false, a),
            _ => PairOutcome::Conflict,
        };
    }
    if left_end.is_some() || right_end.is_some() {
        return PairOutcome::Conflict;
    }
    if !selection.same_assertion
        || left.relation != right.relation
        || !left
            .endpoints
            .iter()
            .any(|endpoint| right.endpoints.contains(endpoint))
    {
        return PairOutcome::Conflict;
    }
    reference_outcome(
        &left.assertions.days(),
        &right.assertions.days(),
        selection.reaffirmed_by,
    )
}

/// Same-assertion pairs without explicit dates, by publication day.
fn reference_outcome(
    left: &[&TimePoint],
    right: &[&TimePoint],
    reaffirmed_by: &[AssertionWitness],
) -> PairOutcome {
    let (Some(&left_first), Some(&left_last), Some(&right_first), Some(&right_last)) =
        (left.first(), left.last(), right.first(), right.last())
    else {
        return PairOutcome::Conflict;
    };
    let precedes = |a: &TimePoint, b: &TimePoint| day_order(a, b) == Some(Ordering::Less);
    let end = |left, time: &TimePoint, restated| {
        let mut time = time.clone();
        time.approximate = true;
        PairOutcome::End {
            left,
            time,
            basis: EndBasis::Reference,
            restated,
        }
    };
    if precedes(left_last, right_first) {
        return end(true, right_first, None);
    }
    if precedes(right_last, left_first) {
        return end(false, left_first, None);
    }
    // Interleaved: the side asserted last stays current only when a Source
    // that restated it after the other side's last day selected this pair.
    let (ended_left, current, other) = match day_order(left_last, right_last) {
        Some(Ordering::Greater) => (false, left, right),
        Some(Ordering::Less) => (true, right, left),
        _ => return PairOutcome::Conflict,
    };
    let other_last = *other.last().expect("nonempty");
    let Some(witness) = reaffirmed_by
        .iter()
        .filter(|witness| {
            witness
                .publication
                .as_ref()
                .is_some_and(|day| precedes(other_last, day))
        })
        .min_by(|a, b| witness_order(a, b))
    else {
        return PairOutcome::Conflict;
    };
    let time = current
        .iter()
        .find(|day| precedes(other_last, day))
        .expect("the current side has a later day");
    let restated = Restated {
        witness: witness.clone(),
        lapsed: lapses(current, other),
    };
    end(ended_left, time, Some(restated))
}

/// The intervals from a day of `other` after a day of `current` to the next
/// day of `current`. A day both sides assert changes nothing.
fn lapses(current: &[&TimePoint], other: &[&TimePoint]) -> Vec<Lapse> {
    let mut days: Vec<(&TimePoint, bool)> = current
        .iter()
        .map(|day| (*day, true))
        .chain(other.iter().map(|day| (*day, false)))
        .collect();
    days.sort_by(|a, b| day_sort(a.0, b.0).then(b.1.cmp(&a.1)));
    let mut result = Vec::new();
    let mut was_current = None;
    let mut from = None;
    let mut index = 0;
    while index < days.len() {
        let key = day_key(days[index].0);
        let same: Vec<_> = days[index..]
            .iter()
            .take_while(|(day, _)| day_key(day) == key)
            .collect();
        index += same.len();
        let (has_current, has_other) = (
            same.iter().any(|(_, current)| *current),
            same.iter().any(|(_, current)| !*current),
        );
        let mut day = same[0].0.clone();
        day.approximate = true;
        match (has_current, has_other) {
            (true, false) => {
                if let Some(from) = from.take() {
                    result.push(Lapse {
                        from,
                        to: day.clone(),
                    });
                }
                was_current = Some(true);
            }
            (false, true) => {
                if was_current == Some(true) {
                    from = Some(day);
                }
                was_current = Some(false);
            }
            _ => {}
        }
    }
    canonical_lapses(result)
}

/// One spelling for a set of lapses: in day order, the latest [`MAX_LAPSED`].
pub(crate) fn canonical_lapses(mut lapses: Vec<Lapse>) -> Vec<Lapse> {
    lapses.sort_by(|a, b| day_sort(&a.from, &b.from).then_with(|| day_sort(&a.to, &b.to)));
    lapses.dedup();
    let excess = lapses.len().saturating_sub(MAX_LAPSED);
    lapses.drain(..excess);
    lapses
}

/// What the resolver-selected pairs of a set of claims decide, keyed by `K`:
/// each claim's earliest ending by [`ending_order`], its conflict partners, and
/// the reopening a restatement gives it. The pipeline's `recompute_endings`
/// and the cutoff projection both collect their pairs here, so a World and the
/// same World read under a cutoff apply one rule.
#[derive(Debug)]
pub(crate) struct PairEffects<K> {
    pub endings: BTreeMap<K, Ending>,
    pub conflicts: BTreeMap<K, BTreeSet<K>>,
    pub reopenings: BTreeMap<K, (Reopened, Vec<Lapse>)>,
}

impl<K> Default for PairEffects<K> {
    fn default() -> Self {
        Self {
            endings: BTreeMap::new(),
            conflicts: BTreeMap::new(),
            reopenings: BTreeMap::new(),
        }
    }
}

/// One side of a pair as [`PairEffects::record`] reads it: its key, the claim
/// id an ending names, and its assertions.
pub(crate) type PairSide<'a, K> = (&'a K, &'a str, &'a AssertionSources);

impl<K: Ord + Clone> PairEffects<K> {
    pub(crate) fn consider(&mut self, claim: &K, ending: Ending) {
        if self
            .endings
            .get(claim)
            .is_none_or(|old| ending_order(&ending, old).is_lt())
        {
            self.endings.insert(claim.clone(), ending);
        }
    }

    pub(crate) fn record(
        &mut self,
        left: PairSide<'_, K>,
        right: PairSide<'_, K>,
        outcome: PairOutcome,
    ) {
        match outcome {
            PairOutcome::Independent => {}
            PairOutcome::Conflict => {
                self.conflicts
                    .entry(left.0.clone())
                    .or_default()
                    .insert(right.0.clone());
                self.conflicts
                    .entry(right.0.clone())
                    .or_default()
                    .insert(left.0.clone());
            }
            PairOutcome::End {
                left: ends_left,
                time,
                basis,
                restated,
            } => {
                let (ended, ender) = if ends_left {
                    (left, right)
                } else {
                    (right, left)
                };
                let Some(mut ending) = Ending::paired(time.clone(), basis, ender.1, ender.2) else {
                    return;
                };
                if let Some(restated) = restated {
                    ending.span = restated.witness.span.clone();
                    if !restated.lapsed.is_empty() {
                        let reopened = Reopened {
                            at: time,
                            by_span: restated.witness.span,
                        };
                        let entry = self
                            .reopenings
                            .entry(ender.0.clone())
                            .or_insert_with(|| (reopened.clone(), Vec::new()));
                        if reopening_order(&reopened, &entry.0).is_gt() {
                            entry.0 = reopened;
                        }
                        entry.1.extend(restated.lapsed);
                    }
                }
                self.consider(ended.0, ending);
            }
        }
    }

    /// The reopening of `claim`, its lapses in one spelling.
    pub(crate) fn take_reopening(&mut self, claim: &K) -> (Option<Reopened>, Vec<Lapse>) {
        match self.reopenings.remove(claim) {
            Some((reopened, lapsed)) => (Some(reopened), canonical_lapses(lapsed)),
            None => (None, Vec::new()),
        }
    }

    /// Drop each conflict link between a claim and the claim whose only
    /// Source witnessed its final ending ([`Ending::witnessed_only_by`]).
    pub(crate) fn drop_witnessed_conflicts<'a>(
        &mut self,
        ending: impl Fn(&K) -> Option<&'a Ending>,
        sources: impl Fn(&K) -> Option<&'a AssertionSources>,
    ) {
        let witnessed = |ended: &K, other: &K| {
            ending(ended)
                .zip(sources(other))
                .is_some_and(|(end, other)| end.witnessed_only_by(other))
        };
        for (claim, partners) in &mut self.conflicts {
            partners.retain(|other| !witnessed(claim, other) && !witnessed(other, claim));
        }
    }
}

/// The latest reopening is the one a claim shows.
pub(crate) fn reopening_order(a: &Reopened, b: &Reopened) -> Ordering {
    day_sort(&a.at, &b.at).then_with(|| a.by_span.cmp(&b.by_span))
}

/// The calendar day a publication states, or its year or month when that is
/// all it states. `None` when it states none.
pub(crate) fn day_key(time: &TimePoint) -> Option<&str> {
    let value = time.value.as_deref()?;
    match time.precision {
        TimePrecision::Unknown => None,
        TimePrecision::Year | TimePrecision::Month => Some(value),
        TimePrecision::Date | TimePrecision::Instant => value.get(..10),
    }
}

/// Days compare at their common precision; a month contains its days.
fn day_order(a: &TimePoint, b: &TimePoint) -> Option<Ordering> {
    let (a, b) = (day_key(a)?, day_key(b)?);
    let width = a.len().min(b.len());
    Some(a.get(..width)?.cmp(b.get(..width)?))
}

/// A total order for storage, so a set of days has one spelling.
fn day_sort(a: &TimePoint, b: &TimePoint) -> Ordering {
    day_key(a)
        .cmp(&day_key(b))
        .then_with(|| a.value.cmp(&b.value))
}

fn same_start(left: Option<&TimePoint>, right: Option<&TimePoint>) -> bool {
    left.zip(right)
        .is_some_and(|(a, b)| a.precision == b.precision && a.at_or_before(b) && b.at_or_before(a))
}

fn time_order(a: &TimePoint, b: &TimePoint) -> Ordering {
    a.compare_at_common_precision(b)
        .unwrap_or_else(|| a.value.cmp(&b.value))
}

pub(crate) fn witness_order(a: &AssertionWitness, b: &AssertionWitness) -> Ordering {
    match (&a.publication, &b.publication) {
        (Some(a), Some(b)) => time_order(a, b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    .then_with(|| a.span.cmp(&b.span))
}

/// Which of two endings of one claim holds: a stated one before a reference
/// one, the earlier first, then the one whose ender is itself open, so
/// "replaced by" names the successor rather than an end fact; ids break what
/// is left.
pub(crate) fn ending_order(a: &Ending, b: &Ending) -> Ordering {
    (a.basis == EndBasis::Reference)
        .cmp(&(b.basis == EndBasis::Reference))
        .then_with(|| time_order(&a.time, &b.time))
        .then_with(|| b.ender_open.cmp(&a.ender_open))
        .then_with(|| a.by.cmp(&b.by))
        .then_with(|| a.span.cmp(&b.span))
}

impl Ending {
    /// Whether `other`'s only Source witnessed this ending. The Source that
    /// replaced a figure does not also disagree with it, so the ended claim
    /// carries no conflict link to such a claim; one that another Source also
    /// supports keeps it.
    pub(crate) fn witnessed_only_by(&self, other: &AssertionSources) -> bool {
        other.source_ids.len() == 1
            && other
                .witness
                .as_ref()
                .is_some_and(|witness| witness.span == self.span)
    }

    pub(crate) fn stated(stated: &StatedEnd) -> Self {
        Self {
            time: stated.time.clone(),
            by: None,
            span: stated.witness.span.clone(),
            basis: EndBasis::Stated,
            ender_open: false,
        }
    }

    pub(crate) fn paired(
        time: TimePoint,
        basis: EndBasis,
        by: &str,
        assertions: &AssertionSources,
    ) -> Option<Self> {
        let stated = assertions
            .stated
            .as_ref()
            .filter(|stated| basis == EndBasis::Stated && stated.time == time);
        let witness = stated
            .map(|stated| &stated.witness)
            .or(assertions.witness.as_ref())?;
        Some(Self {
            time,
            by: Some(by.to_owned()),
            span: witness.span.clone(),
            basis,
            ender_open: assertions.stated.is_none(),
        })
    }
}

impl AssertionSources {
    pub(crate) fn publication(&self) -> Option<&TimePoint> {
        self.witness.as_ref()?.publication.as_ref()
    }

    /// The publication days that assert it, earliest first.
    pub(crate) fn days(&self) -> Vec<&TimePoint> {
        if self.days.is_empty() {
            return self
                .publication()
                .filter(|day| day_key(day).is_some())
                .into_iter()
                .collect();
        }
        self.days.iter().collect()
    }

    /// Record one more published assertion. Older claims start from the
    /// witness's day.
    pub(crate) fn add_day(&mut self, day: &TimePoint) {
        if day_key(day).is_none() {
            return;
        }
        if self.days.is_empty() {
            self.days = self.days().into_iter().cloned().collect();
        }
        match self.days.binary_search_by(|stored| day_sort(stored, day)) {
            Ok(_) => {}
            Err(at) => {
                let same = |index: usize| {
                    self.days
                        .get(index)
                        .is_some_and(|stored| day_key(stored) == day_key(day))
                };
                // One entry per day: the earliest value of that day.
                if at > 0 && same(at - 1) {
                    return;
                }
                if same(at) {
                    self.days[at] = day.clone();
                } else {
                    self.days.insert(at, day.clone());
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldSnapshotBody {
    pub nodes: Vec<GraphNode>,
    pub claims: Vec<GraphClaim>,
    pub annotations: Vec<TopicAnnotation>,
    /// Observed names of relationships between distinct endpoints, counted at seal time.
    #[serde(default)]
    pub relation_vocabulary: Vec<RelationCount>,
    #[serde(default, skip_serializing_if = "WorldTemporal::is_empty")]
    pub(crate) temporal: WorldTemporal,
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphEdge {
    pub id: String,
    pub relation: String,
    pub endpoints: Vec<ClaimEndpoint>,
    pub status: ClaimStatus,
    pub claim: GraphClaim,
    /// `true` when the claim is no longer current. Omitted while it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ended: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub end: Option<TimePoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// Received as a request: missing optional fields are accepted.
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldGraphQuery {
    pub loupist_id: String,
    pub record_id: String,
    pub revision_id: String,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub search: String,
    #[serde(default)]
    pub center_node_id: Option<String>,
    #[serde(default)]
    pub center_claim_id: Option<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub kinds: Vec<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub relations: Vec<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub binding_roles: Vec<BindingRole>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub statuses: Vec<ClaimStatus>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub publication_times: Vec<String>,
    /// World as of a calendar day (`YYYY-MM-DD`, today or earlier): only
    /// relationships published by then, as they stood then, and only the names
    /// they connect, reduced to name and kind. The response's `asOfReport`
    /// counts what was left out. The chat as-of reads and World's As of
    /// control share this one cutoff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub as_of: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub node_limit: Option<u32>,
    #[serde(default)]
    pub edge_limit: Option<u32>,
    #[serde(default)]
    pub byte_limit: Option<u32>,
}

/// What a read of World under an as-of date left out: relationships with no
/// evidence published by then, and Sources World cites with no evidence that was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldAsOfReport {
    pub facts: u32,
    pub sources: u32,
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
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalCheck {
    pub claim: String,
    pub metric: String,
    pub direction: String,
    pub threshold: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub unit: Option<String>,
    /// Required for a dated check; optional only for an asset page's line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub by_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalOutcome {
    pub observed: f64,
    pub observed_at: String,
    pub result: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub note: Option<String>,
    /// Set when the review ran under an as-of date ("Reviewed as of …").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaterNote {
    pub note_id: String,
    pub chat_id: String,
    pub title: String,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkingNote {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub later_notes: Vec<LaterNote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
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
    #[cfg_attr(test, ts(optional))]
    pub publication_time: Option<TimePoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalLensInput {
    pub delivery_id: String,
    pub record_id: String,
    pub revision_id: String,
    pub revision_digest: String,
    pub body_digest: String,
    pub markdown: String,
    pub status: LensUseStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalInputs {
    pub sources: Vec<JournalSourceInput>,
    pub worlds: Vec<JournalWorldInput>,
    pub lenses: Vec<JournalLensInput>,
    pub earlier: Vec<EarlierResearch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub check: Option<JournalCheck>,
    /// A reason for following an asset, or a line that would change the user's mind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub thesis: Option<super::assets::JournalThesis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
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

#[cfg(test)]
#[path = "model_pair_tests.rs"]
mod pair_tests;
