use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::chat::artifacts::{ChatArtifactRef, MAX_CHAT_TURN_ARTIFACTS};
use crate::chat::progress::ChatProgress;
use crate::providers::catalog::ExecutionModelLock;

use super::{
    memory_refs_digest, require_bounded_text, require_identifier, required_option,
    validate_sha256_digest, DomainError, KnowledgeReference, MemoryAccess, MemoryRefSnapshot,
    MAX_MEMORY_REFS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryPolicy {
    pub use_memory: bool,
    pub update_memory: bool,
}

impl Default for MemoryPolicy {
    fn default() -> Self {
        Self {
            use_memory: true,
            update_memory: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatMessageStatus {
    Complete,
    Aborted,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryUpdateStatus {
    Applied,
    NoChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryUpdateChange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knowledge: Option<KnowledgeReference>,
    pub record_id: String,
    pub kind: String,
    pub operation: String,
    pub title: String,
    pub lesson: String,
    pub basis: String,
    pub future_use: String,
}

impl Serialize for MemoryUpdateChange {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("recordId", &self.record_id)?;
        map.serialize_entry("kind", &self.kind)?;
        map.serialize_entry("operation", &self.operation)?;
        map.serialize_entry("title", &self.title)?;
        map.serialize_entry("lesson", &self.lesson)?;
        map.serialize_entry("basis", &self.basis)?;
        map.serialize_entry("futureUse", &self.future_use)?;
        if let Some(value) = &self.knowledge {
            map.serialize_entry("knowledge", value)?;
        }
        if let Some(value) = &self.world_job_id {
            map.serialize_entry("worldJobId", value)?;
        }
        if let Some(value) = &self.world_status {
            map.serialize_entry("worldStatus", value)?;
        }
        if self.kind == "principle" {
            map.serialize_entry("statement", &self.title)?;
        }
        map.end()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryUpdateResult {
    pub status: MemoryUpdateStatus,
    #[serde(deserialize_with = "required_option")]
    pub commit_id: Option<String>,
    pub changes: Vec<MemoryUpdateChange>,
}

impl MemoryUpdateResult {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.changes.len() > 18 {
            return Err(DomainError::Invalid("memory update has too many changes"));
        }
        let mut ids = BTreeSet::new();
        for change in &self.changes {
            require_bounded_text(&change.record_id, 512, "memory change reference is invalid")?;
            if !matches!(
                change.kind.as_str(),
                "Source" | "World" | "Lens" | "Journal" | "Research" | "principle"
            ) || !matches!(
                change.operation.as_str(),
                "create" | "revise" | "rename" | "proposed"
            ) || !ids.insert(change.record_id.as_str())
            {
                return Err(DomainError::Invalid("memory update change is invalid"));
            }
            require_bounded_text(
                &change.title,
                if change.kind == "principle" {
                    1120
                } else if change.kind == "Journal" {
                    1000
                } else {
                    512
                },
                "memory update title is invalid",
            )?;
            require_bounded_text(&change.lesson, 2_048, "memory update lesson is invalid")?;
            require_bounded_text(&change.basis, 2_048, "memory update basis is invalid")?;
            require_bounded_text(
                &change.future_use,
                2_048,
                "memory update future use is invalid",
            )?;
        }
        match self.status {
            MemoryUpdateStatus::Applied => {
                if self.commit_id.as_deref().is_none_or(str::is_empty) || self.changes.is_empty() {
                    return Err(DomainError::Invalid("applied memory update is invalid"));
                }
            }
            MemoryUpdateStatus::NoChange => {
                if self.commit_id.is_some() || !self.changes.is_empty() {
                    return Err(DomainError::Invalid("unchanged memory update is invalid"));
                }
            }
        }
        Ok(())
    }
}

/// Content-free measurements captured before the completed message is sealed.
/// These remain attached to that response after Chat reload or app restart.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatPerformance {
    pub setup_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_text_ms: Option<u64>,
    pub generation_ms: u64,
    /// Elapsed time through preparation of the canonical completion snapshot.
    pub total_ms: u64,
    pub session_cache: ChatSessionCacheStatus,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatSessionCacheStatus {
    #[default]
    Cold,
    Warm,
}

/// One attached file at most; images have their own, smaller vision limit.
pub const MAX_CHAT_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;
/// The longest provider retry hint a busy answer counts down from.
pub const MAX_RETRY_AFTER_SECONDS: u32 = 120;
pub const MAX_CHAT_EVIDENCE_REFS: usize = 64;
pub const MAX_CHAT_EVIDENCE_EXCERPT_BYTES: usize = 8 * 1024;
// Reserve one retained excerpt and one delivered-sample excerpt per admitted
// reference. An acknowledged late passage must never lose its matching text.
pub const MAX_CHAT_EVIDENCE_TOTAL_EXCERPT_BYTES: usize =
    MAX_CHAT_EVIDENCE_REFS * 2 * MAX_CHAT_EVIDENCE_EXCERPT_BYTES;

/// A bounded, host-created excerpt of the retained tool result. When the agent
/// received a compacted sample, `model_delivery` records that distinct payload.
/// Neither snapshot verifies a claim or source. Works with Memory off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatEvidenceRef {
    pub id: String,
    pub title: String,
    pub kind: ChatEvidenceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_ref: Option<String>,
    pub retrieved_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<String>,
    pub excerpt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt_match: Option<ChatEvidenceMatch>,
    pub excerpt_truncated: bool,
    pub content_sha256: String,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset_ref: Option<crate::datasets::DatasetRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_delivery: Option<ChatEvidenceDelivery>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upstream_evidence_ids: Vec<String>,
}

/// End-exclusive UTF-8 byte offsets into the retained excerpt, supplied only
/// after the host verifies a literal passage selected from its cached source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatEvidenceMatch {
    pub start: usize,
    pub end: usize,
}

impl ChatEvidenceMatch {
    pub(crate) fn is_valid_for(&self, excerpt: &str) -> bool {
        self.start < self.end && excerpt.get(self.start..self.end).is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatEvidenceDelivery {
    pub coverage: ChatEvidenceDeliveryCoverage,
    pub excerpt: String,
    pub excerpt_truncated: bool,
    pub content_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatEvidenceDeliveryCoverage {
    BoundedSample,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatEvidenceKind {
    Original,
    Discovery,
    Derived,
    LocalFile,
    SavedContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatMessage {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_refusals: Vec<ChatToolRefusal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<ChatFeedback>,
    pub id: String,
    pub role: ChatRole,
    pub status: ChatMessageStatus,
    pub content: String,
    pub created_at_ms: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lens_receipts: Vec<super::LensReceipt>,
    pub memory_refs: Vec<MemoryRefSnapshot>,
    pub observed_exact_count: u64,
    pub refs_truncated: bool,
    pub refs_digest: String,
    #[serde(deserialize_with = "required_option")]
    pub memory_update: Option<MemoryUpdateResult>,
    #[serde(deserialize_with = "required_option")]
    pub memory_revision: Option<String>,
    #[serde(deserialize_with = "required_option")]
    pub execution_model: Option<ExecutionModelLock>,
    #[serde(deserialize_with = "required_option")]
    pub agent_harness: Option<crate::agent_harness::AgentHarnessSnapshot>,
    pub attachments: Vec<ChatAttachment>,
    pub artifact_refs: Vec<ChatArtifactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<ChatEvidenceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schedule_receipts: Vec<crate::scheduled_tasks::types::ScheduleReceipt>,
    #[serde(default)]
    pub progress: Option<ChatProgress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance: Option<ChatPerformance>,
    /// Host diagnostics only; excluded from renderer DTOs and model history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_trace: Option<ChatToolTrace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<ChatFailureKind>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub host_composed: bool,
    /// Guidance Pi accepted while a reply ran (⌘↩). It stays a user turn in
    /// model history; the transcript shows it as a "Guided" row, not a bubble.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub steer: bool,
    /// For a busy (`rate_limited`) answer: how long the provider asked to
    /// wait, read from its retry hints and bounded by Rust.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u32>,
    /// A scheduled result the host posted into the Loupist's home Thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<HomeDelivery>,
    /// The chosen model's name when its provider no longer offered it and
    /// Rust answered this turn on Auto instead (W17.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_model: Option<String>,
    /// A crossed red line the host announced in the home Thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alert: Option<HomeAlert>,
    /// Watcher alerts the host posted in the home Thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch: Option<HomeWatch>,
    /// A results snapshot (or a failed capture) the host posted in the home Thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub earnings: Option<HomeEarnings>,
    /// The as-of date the user set for this request (`YYYY-MM-DD`), kept on
    /// the request and on its answer (finance-point-in-time.md).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
    /// Number links Rust checked against their evidence when the answer
    /// finished (finance-show-the-math.md §7.2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub number_checks: Vec<crate::domain::NumberCheck>,
    /// Grid results the answer linked as paragraphs of their own, copied
    /// from the host's calculations when it finished.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sensitivity_cards: Vec<crate::domain::SensitivityCard>,
    /// What an as-of answer could not use, written when it finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of_report: Option<crate::domain::AsOfReport>,
}

/// One crossed line on an asset page, posted once into the home Thread. Built
/// only from the page and its move: names, the user's claim and the observed
/// value, never holdings quantities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeAlert {
    pub asset_id: String,
    pub move_id: String,
    pub name: String,
    pub symbol: String,
    pub market: String,
    pub claim: String,
    /// The observed value as written, with its unit ("27%").
    pub value: String,
    /// When the user wrote the line.
    pub set_at_ms: i64,
    pub fact_date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    pub source_title: String,
}

/// Watcher alerts in one host message: at most five, then "+{more} more".
/// Events are the stored `WatchEventView`s (names and dated facts, never
/// holdings); kept as JSON so the message stays a plain record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeWatch {
    pub event_ids: Vec<String>,
    pub events: Vec<serde_json::Value>,
    pub more: usize,
}

/// A results message: "{name} reported {period} results." with the two most
/// material measures, or a before-capture that failed (Try again).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeEarnings {
    pub asset_id: String,
    pub event_id: String,
    pub name: String,
    pub symbol: String,
    pub market: String,
    pub period: String,
    /// `reported` or `failed`.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    /// The card's measure rows (`MeasureView`), at most two.
    pub measures: Vec<serde_json::Value>,
}

/// Where a host-posted Thread message came from: one finished scheduled run.
/// The text is the run's first paragraph; the run's own Chat keeps the answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeDelivery {
    pub run_id: String,
    pub task_id: String,
    pub task_title: String,
    pub task_kind: crate::scheduled_tasks::types::TaskKind,
    /// None once the task was deleted.
    pub schedule: Option<crate::scheduled_tasks::types::Schedule>,
    pub run_thread_id: Option<String>,
    pub status: crate::scheduled_tasks::types::RunStatus,
    pub finished_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatToolRefusal {
    pub connection_id: String,
    pub connection_name: String,
    pub tool_name: String,
    pub reason: ChatToolRefusalReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatToolRefusalReason {
    NeedsApproval,
    BlockedUnattended,
    Taint,
    /// The host refused a Memory write that looked like a credential or secret.
    SensitiveMemory,
    /// The user chose Not now on an approval card, or the wait ran out.
    Declined,
}

/// Stable allowlist: never store provider error bodies or internal error strings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatFailureKind {
    EmptyReply,
    ProviderError,
    /// A short-term rate limit on a key or local account (X11: countdown and retry).
    RateLimited,
    /// The provider sign-in or key was rejected, revoked or expired (Reconnect).
    NeedsReconnect,
    /// A subscription plan reached its shared limit; never retried automatically.
    PlanLimit,
    /// The plan cannot be used in other apps.
    PlanNotEligible,
    /// Claude sign-in: extra usage is off or used up.
    ExtraUsageOff,
    /// A key account is out of credit.
    OutOfCredit,
    /// The chosen model is no longer offered; Rust pruned it and Auto answers the retry.
    ModelGone,
    /// The provider could not be reached (X11: offline).
    Offline,
    /// The provider reported a server error or overload.
    ProviderUnavailable,
    /// The conversation no longer fits the model's context window.
    ContextTooLong,
    ProviderTimeout,
    PiExited,
    ProtocolError,
    OutputLimit,
    MemoryCommitFailed,
    StoreFailed,
    CancelledByConflict,
    /// Also every kind this build no longer knows (for example a renamed one in
    /// an older stored message), so one old message never hides its Chat.
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatFeedback {
    pub rating: Option<ChatFeedbackRating>,
    pub correction: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatFeedbackRating {
    Up,
    Down,
}

pub const MAX_TOOL_TRACE_ENTRIES: usize = 256;
pub const MAX_TOOL_TRACE_BYTES: usize = 64 * 1024;
pub const MAX_TOOL_TRACE_VALUE_BYTES: usize = 2048;

/// Portable per-turn diagnostics. `truncated` also covers omitted calls.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatToolTrace {
    pub entries: Vec<ChatToolTraceEntry>,
    pub truncated: bool,
    /// Host trust receipt, persisted in the existing message JSON. It remains
    /// meaningful when diagnostic entries are bounded or omitted.
    #[serde(default)]
    pub untrusted_text: bool,
}

impl ChatMessage {
    pub(crate) fn has_untrusted_text(&self) -> bool {
        self.role == ChatRole::Assistant
            && self
                .tool_trace
                .as_ref()
                .is_some_and(|trace| trace.untrusted_text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatToolTraceEntry {
    pub call_id: Option<String>,
    pub name: String,
    pub component: Option<String>,
    pub args: Option<serde_json::Value>,
    pub start_offset_ms: u64,
    pub duration_ms: u64,
    pub outcome: ToolTraceOutcome,
    /// The broker's model-facing envelope, with bounded/redacted text.
    pub error: Option<serde_json::Value>,
    pub result_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolTraceOutcome {
    Ok,
    Error,
}

/// The run-log component of a thesis update the reply's commit could not save.
pub const THESIS_COMMIT_COMPONENT: &str = "asset-commit";

/// A thesis update the tool accepted but the reply's commit did not save,
/// because the page changed first. The receipt says it was not saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsavedThesis {
    pub asset_id: String,
    pub name: String,
    /// The refusal code (`thesis_after_undo`, `thesis_none`, `thesis_twice`, …).
    pub reason: String,
}

impl ChatToolTrace {
    /// Thesis updates this turn's commit recorded as not saved.
    pub fn unsaved_theses(&self) -> Vec<UnsavedThesis> {
        self.entries
            .iter()
            .filter(|entry| entry.component.as_deref() == Some(THESIS_COMMIT_COMPONENT))
            .filter_map(|entry| {
                let error = entry.error.as_ref()?;
                let text = |key: &str| error.get(key)?.as_str().map(str::to_owned);
                Some(UnsavedThesis {
                    asset_id: text("assetId")?,
                    name: text("name")?,
                    reason: text("reason").unwrap_or_default(),
                })
            })
            .collect()
    }

    /// Whether a network read in this turn failed before reaching its server
    /// (the network or that provider was unreachable).
    pub fn has_offline_read(&self) -> bool {
        self.entries.iter().any(|entry| {
            entry
                .error
                .as_ref()
                .and_then(|error| error.get("code"))
                .and_then(serde_json::Value::as_str)
                == Some("offline")
        })
    }

    pub fn push(&mut self, entry: ChatToolTraceEntry) {
        if self.truncated {
            return;
        }
        self.entries.push(entry);
        if self.entries.len() > MAX_TOOL_TRACE_ENTRIES
            || serde_json::to_vec(self).map_or(true, |json| json.len() > MAX_TOOL_TRACE_BYTES)
        {
            self.entries.pop();
            self.truncated = true;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatAttachment {
    pub id: String,
    pub filename: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub content_sha256: String,
}

impl ChatAttachment {
    pub fn validate(&self) -> Result<(), DomainError> {
        require_identifier(&self.id, "chat attachment id is empty or unsafe")?;
        require_bounded_text(&self.filename, 255, "chat attachment filename is invalid")?;
        if self.filename.contains(['/', '\\', '\n', '\r'])
            || self.filename.chars().any(char::is_control)
        {
            return Err(DomainError::Invalid("chat attachment filename is invalid"));
        }
        require_bounded_text(
            &self.media_type,
            127,
            "chat attachment media type is invalid",
        )?;
        if !self
            .media_type
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'+' | b'-' | b'.'))
            || !self.media_type.contains('/')
        {
            return Err(DomainError::Invalid(
                "chat attachment media type is invalid",
            ));
        }
        if self.size_bytes == 0 || self.size_bytes > MAX_CHAT_ATTACHMENT_BYTES as u64 {
            return Err(DomainError::Invalid("chat attachment size is invalid"));
        }
        validate_sha256_digest(&self.content_sha256, "chat attachment digest is invalid")
    }
}

impl ChatMessage {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.steer && self.role != ChatRole::User {
            return Err(DomainError::Invalid("only user messages can be steers"));
        }
        if self.retry_after_seconds.is_some_and(|seconds| {
            self.role != ChatRole::Assistant || !(1..=MAX_RETRY_AFTER_SECONDS).contains(&seconds)
        }) {
            return Err(DomainError::Invalid("retry hint is out of bounds"));
        }
        if self
            .as_of
            .as_deref()
            .is_some_and(|date| !crate::domain::valid_as_of(date))
            || (self.role != ChatRole::Assistant
                && (!self.number_checks.is_empty()
                    || !self.sensitivity_cards.is_empty()
                    || self.as_of_report.is_some()))
        {
            return Err(DomainError::Invalid("invalid as-of date or answer checks"));
        }
        crate::domain::answer_checks::validate_number_checks(&self.number_checks)?;
        crate::domain::answer_checks::validate_sensitivity_cards(&self.sensitivity_cards)?;
        if let Some(report) = &self.as_of_report {
            crate::domain::answer_checks::validate_as_of_report(report)?;
        }
        if self.tool_refusals.len() > 64
            || (self.role == ChatRole::User && !self.tool_refusals.is_empty())
            || self.tool_refusals.iter().any(|r| {
                r.connection_id.is_empty()
                    || r.connection_id.len() > 256
                    || r.connection_name.len() > 512
                    || r.tool_name.len() > 256
            })
        {
            return Err(DomainError::Invalid("invalid custom tool refusals"));
        }
        if self.schedule_receipts.len() > crate::scheduled_tasks::types::MAX_TASKS_PER_LOUPIST
            || (self.role == ChatRole::User && !self.schedule_receipts.is_empty())
            || self
                .schedule_receipts
                .iter()
                .any(|receipt| receipt.validate().is_err())
        {
            return Err(DomainError::Invalid("invalid scheduled action receipts"));
        }
        if self.feedback.as_ref().is_some_and(|feedback| {
            self.role != ChatRole::Assistant
                || self.status != ChatMessageStatus::Complete
                || feedback.updated_at < 0
                || feedback.correction.as_ref().is_some_and(|text| {
                    text.is_empty() || text.trim() != text || text.chars().count() > 4000
                })
        }) {
            return Err(DomainError::Invalid("invalid assistant feedback"));
        }
        if self.tool_trace.as_ref().is_some_and(|trace| {
            self.role != ChatRole::Assistant
                || trace.entries.len() > MAX_TOOL_TRACE_ENTRIES
                || serde_json::to_vec(trace)
                    .map_or(true, |bytes| bytes.len() > MAX_TOOL_TRACE_BYTES)
                || trace.entries.iter().any(|entry| {
                    entry.name.len() > 256
                        || entry.call_id.as_ref().is_some_and(|id| id.len() > 256)
                        || entry.component.as_ref().is_some_and(|id| id.len() > 256)
                        || entry.args.as_ref().is_some_and(|args| {
                            serde_json::to_vec(args)
                                .map_or(true, |bytes| bytes.len() > MAX_TOOL_TRACE_VALUE_BYTES)
                        })
                        || entry.error.as_ref().is_some_and(|error| {
                            serde_json::to_vec(error)
                                .map_or(true, |bytes| bytes.len() > MAX_TOOL_TRACE_VALUE_BYTES)
                        })
                })
        }) {
            return Err(DomainError::Invalid("chat tool trace exceeds its bounds"));
        }
        if self.evidence_refs.len() > MAX_CHAT_EVIDENCE_REFS
            || self
                .evidence_refs
                .iter()
                .map(|item| {
                    item.excerpt.len()
                        + item
                            .model_delivery
                            .as_ref()
                            .map_or(0, |delivery| delivery.excerpt.len())
                })
                .sum::<usize>()
                > MAX_CHAT_EVIDENCE_TOTAL_EXCERPT_BYTES
            || (self.role == ChatRole::User && !self.evidence_refs.is_empty())
        {
            return Err(DomainError::Invalid("chat evidence exceeds its bounds"));
        }
        let mut evidence_ids = std::collections::BTreeSet::new();
        for evidence in &self.evidence_refs {
            if !evidence.id.starts_with("ev_")
                || evidence.id.len() != 35
                || !evidence.id[3..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || !evidence_ids.insert(&evidence.id)
                || evidence.title.len() > 512
                || evidence.excerpt.len() > MAX_CHAT_EVIDENCE_EXCERPT_BYTES
                || evidence
                    .excerpt_match
                    .as_ref()
                    .is_some_and(|matched| !matched.is_valid_for(&evidence.excerpt))
                || evidence.model_delivery.as_ref().is_some_and(|delivery| {
                    delivery.excerpt.len() > MAX_CHAT_EVIDENCE_EXCERPT_BYTES
                })
                || evidence.upstream_evidence_ids.len() > MAX_CHAT_EVIDENCE_REFS
                || evidence.upstream_evidence_ids.iter().any(|id| {
                    !id.starts_with("ev_")
                        || id.len() != 35
                        || !id[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
                })
                || evidence.read_ref.as_deref().is_some_and(|read_ref| {
                    evidence.kind != ChatEvidenceKind::SavedContext
                        || read_ref.trim().is_empty()
                        || read_ref.len() > 256
                        || read_ref.contains('\0')
                })
                || evidence.warnings.len() > 16
                || evidence.warnings.iter().any(|warning| warning.len() > 2048)
                || evidence.url.as_ref().is_some_and(|url| {
                    url.len() > 2048
                        || reqwest::Url::parse(url).map_or(true, |value| {
                            !matches!(value.scheme(), "https" | "http")
                                || !value.username().is_empty()
                                || value.password().is_some()
                        })
                })
            {
                return Err(DomainError::Invalid("chat evidence is invalid"));
            }
            validate_sha256_digest(&evidence.content_sha256, "chat evidence digest is invalid")?;
            if let Some(delivery) = &evidence.model_delivery {
                validate_sha256_digest(
                    &delivery.content_sha256,
                    "chat evidence delivery digest is invalid",
                )?;
            }
        }

        require_identifier(&self.id, "chat message id is empty or unsafe")?;
        if self.content.trim().is_empty() && self.attachments.is_empty() {
            return Err(DomainError::Invalid("chat message content is empty"));
        }
        if self.failure_kind.is_some()
            && (self.role != ChatRole::Assistant || self.status != ChatMessageStatus::Error)
        {
            return Err(DomainError::Invalid(
                "failure category requires a failed assistant message",
            ));
        }
        if self.host_composed
            && (self.role != ChatRole::Assistant || self.status != ChatMessageStatus::Complete)
        {
            return Err(DomainError::Invalid(
                "host composition requires a completed assistant message",
            ));
        }
        if self.created_at_ms < 0 {
            return Err(DomainError::Invalid("chat message timestamp is invalid"));
        }
        if self.memory_refs.len() > MAX_MEMORY_REFS {
            return Err(DomainError::Invalid(
                "chat message has too many memory references",
            ));
        }
        if self.lens_receipts.len() > 256
            || (!self.lens_receipts.is_empty() && self.role != ChatRole::Assistant)
        {
            return Err(DomainError::Invalid(
                "Lens receipts require a bounded assistant message",
            ));
        }
        for receipt in &self.lens_receipts {
            receipt.validate()?;
        }
        let mut record_ids = BTreeSet::new();
        for memory in &self.memory_refs {
            memory.validate()?;
            if memory.access != MemoryAccess::ExactRead {
                return Err(DomainError::Invalid(
                    "durable chat memory references must be exact reads",
                ));
            }
            if !record_ids.insert(memory.read_ref.as_str()) {
                return Err(DomainError::Invalid(
                    "chat message contains duplicate memory references",
                ));
            }
        }
        let retained_count = u64::try_from(self.memory_refs.len())
            .map_err(|_| DomainError::Invalid("chat memory reference count is invalid"))?;
        if self.observed_exact_count < retained_count
            || self.refs_truncated != (self.observed_exact_count > retained_count)
        {
            return Err(DomainError::Invalid(
                "chat memory reference summary is inconsistent",
            ));
        }
        validate_sha256_digest(&self.refs_digest, "chat memory reference digest is invalid")?;
        if !self.refs_truncated && self.refs_digest != memory_refs_digest(&self.memory_refs)? {
            return Err(DomainError::Invalid(
                "chat memory reference digest does not match retained references",
            ));
        }
        if let Some(update) = &self.memory_update {
            update.validate()?;
            if self.role != ChatRole::Assistant {
                return Err(DomainError::Invalid(
                    "only assistant chat messages may contain a memory update",
                ));
            }
        }
        if self.memory_revision.as_deref().is_some_and(|value| {
            value.is_empty() || value.len() > 256 || value.contains(['\0', '\n', '\r'])
        }) {
            return Err(DomainError::Invalid(
                "chat message memory revision is invalid",
            ));
        }
        if self
            .execution_model
            .as_ref()
            .is_some_and(|model| model.validate().is_err())
        {
            return Err(DomainError::Invalid("chat message model lock is invalid"));
        }
        if self
            .agent_harness
            .as_ref()
            .is_some_and(|harness| harness.validate().is_err())
        {
            return Err(DomainError::Invalid("chat message harness lock is invalid"));
        }
        if self.replaced_model.as_deref().is_some_and(|name| {
            name.trim().is_empty() || name.len() > 80 || name.chars().any(char::is_control)
        }) {
            return Err(DomainError::Invalid(
                "chat message replaced model name is invalid",
            ));
        }
        if self.execution_model.is_some() != self.agent_harness.is_some() {
            return Err(DomainError::Invalid(
                "chat message model and harness locks must be paired",
            ));
        }
        if self.role == ChatRole::User && self.execution_model.is_some() {
            return Err(DomainError::Invalid(
                "user chat message cannot have a model lock",
            ));
        }
        if matches!(
            self.status,
            ChatMessageStatus::Aborted | ChatMessageStatus::Error
        ) {
            if self.role != ChatRole::Assistant {
                return Err(DomainError::Invalid(
                    "only assistant chat messages may be terminal failures",
                ));
            }
            if self.memory_update.is_some() || !self.artifact_refs.is_empty() {
                return Err(DomainError::Invalid(
                    "terminally failed chat messages cannot publish durable outputs",
                ));
            }
        }
        if self.attachments.len() > 4 {
            return Err(DomainError::Invalid(
                "chat message contains too many attachments",
            ));
        }
        let mut attachment_ids = BTreeSet::new();
        for attachment in &self.attachments {
            attachment.validate()?;
            if !attachment_ids.insert(attachment.id.as_str()) {
                return Err(DomainError::Invalid(
                    "chat message contains duplicate attachments",
                ));
            }
        }
        if self.role != ChatRole::User && !self.attachments.is_empty() {
            return Err(DomainError::Invalid(
                "only user chat messages may contain attachments",
            ));
        }
        if self.artifact_refs.len() > MAX_CHAT_TURN_ARTIFACTS {
            return Err(DomainError::Invalid(
                "chat message contains too many artifact references",
            ));
        }
        let mut artifact_ids = BTreeSet::new();
        for artifact in &self.artifact_refs {
            artifact
                .validate()
                .map_err(|_| DomainError::Invalid("chat artifact reference is invalid"))?;
            if !artifact_ids.insert(artifact.artifact_id.as_str()) {
                return Err(DomainError::Invalid(
                    "chat message contains duplicate artifact references",
                ));
            }
        }
        if self.role == ChatRole::User && !self.artifact_refs.is_empty() {
            return Err(DomainError::Invalid(
                "user chat message cannot publish an artifact",
            ));
        }
        if let Some(progress) = &self.progress {
            if self.role != ChatRole::Assistant {
                return Err(DomainError::Invalid(
                    "only assistant chat messages may contain progress",
                ));
            }
            progress.validate(true)?;
        }
        if self.performance.is_some()
            && (self.role != ChatRole::Assistant || self.status != ChatMessageStatus::Complete)
        {
            return Err(DomainError::Invalid(
                "only completed assistant messages may contain performance",
            ));
        }
        Ok(())
    }
}

/// Removes the latest failed or stopped assistant reply and the user request
/// that produced it, and drops the derived Pi cache. The next send rebuilds
/// that request from SQLite. A valid failed reply cannot carry Memory or a
/// saved document; this checks again so a retry cannot hide one.
pub fn prepare_chat_retry(
    chat: &ChatSession,
    now_ms: i64,
) -> Result<(String, ChatSession), DomainError> {
    if now_ms < 0 {
        return Err(DomainError::Invalid("chat retry time is invalid"));
    }
    let mut session = chat.clone();
    let assistant = session.messages.pop().ok_or(DomainError::Invalid(
        "only the latest failed or stopped reply can be tried again",
    ))?;
    if assistant.role != ChatRole::Assistant
        || !matches!(
            assistant.status,
            ChatMessageStatus::Error | ChatMessageStatus::Aborted
        )
    {
        return Err(DomainError::Invalid(
            "only the latest failed or stopped reply can be tried again",
        ));
    }
    if assistant.memory_update.is_some() || !assistant.artifact_refs.is_empty() {
        return Err(DomainError::Invalid(
            "a failed reply cannot be removed after it saved Memory or a document",
        ));
    }
    let request_index = session
        .messages
        .iter()
        .rposition(|message| !message.steer)
        .ok_or(DomainError::Invalid(
            "only the latest failed or stopped reply can be tried again",
        ))?;
    let user = session.messages.remove(request_index);
    if user.role != ChatRole::User
        || (user.content.trim().is_empty() && user.attachments.is_empty())
    {
        return Err(DomainError::Invalid(
            "only the latest failed or stopped reply can be tried again",
        ));
    }
    let prompt = user.content;
    session.pi_session_cache = None;
    session.updated_at_ms = now_ms.max(chat.updated_at_ms);
    session.validate()?;
    Ok((prompt, session))
}

/// Digest-bound derived Pi JSONL cache for one Chat thread.
/// SQLite remains the transcript authority; a mismatch forces cold rebuild.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiSessionCache {
    pub entries_sha256: String,
    #[serde(deserialize_with = "required_option")]
    pub leaf_id: Option<String>,
    pub harness_digest: String,
    /// The stable app-owned Pi execution policy. This includes transport,
    /// CWD, extension, and session-format compatibility.
    pub runtime_policy_sha256: String,
    /// Hash of the static runtime/component surface. This makes capability
    /// routing, component descriptions, and tool mappings exact across a
    /// resumed Pi JSONL, independent of per-turn Memory flags.
    pub runtime_surface_sha256: String,
    /// Digest of the non-secret connector authority snapshot: every binding,
    /// global connector config revision, and active credential revision. Pi
    /// JSONL can retain tool results, so a connector authority change always
    /// forces a cold rebuild even when the component IDs remain unchanged.
    pub connector_authority_sha256: String,
    /// Authority carried by the cached JSONL. A fresh Pi launch reinstalls a
    /// runtime profile with these flags, so all Memory controls must match
    /// exactly before we restore its prior tool-result context. Any change
    /// rebuilds from the canonical SQLite transcript.
    pub memory_access_enabled: bool,
    pub memory_update_enabled: bool,
    /// The effective Pi/provider session ID for this derived JSONL cache.
    /// It rotates whenever the cache is rebuilt cold, so a reconstructed
    /// SQLite transcript is not sent under a stale provider cache identity.
    pub derived_session_id: String,
    /// Digest of the last full turn envelope, excluding live time.
    #[serde(default)]
    pub context_envelope_sha256: String,
    /// Compaction dropped earlier envelope text, so the next turn resends it.
    #[serde(default)]
    pub context_envelope_stale: bool,
    pub execution_model: ExecutionModelLock,
}

/// The non-secret execution inputs that must match before Pi JSONL tool
/// results can be resumed for another Chat turn.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PiSessionCacheScope<'a> {
    pub harness_digest: &'a str,
    pub runtime_policy_sha256: &'a str,
    pub runtime_surface_sha256: &'a str,
    pub connector_authority_sha256: &'a str,
    pub memory_access_enabled: bool,
    pub memory_update_enabled: bool,
    pub execution_model: &'a ExecutionModelLock,
}

impl PiSessionCache {
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_sha256_digest(
            &self.entries_sha256,
            "chat Pi session cache digest is invalid",
        )?;
        if let Some(leaf_id) = &self.leaf_id {
            if leaf_id.is_empty() || leaf_id.len() > 512 || leaf_id.contains('\0') {
                return Err(DomainError::Invalid(
                    "chat Pi session cache leaf is invalid",
                ));
            }
        }
        validate_sha256_digest(
            &self.harness_digest,
            "chat Pi session cache harness digest is invalid",
        )?;
        validate_sha256_digest(
            &self.runtime_policy_sha256,
            "chat Pi session cache runtime policy digest is invalid",
        )?;
        validate_sha256_digest(
            &self.runtime_surface_sha256,
            "chat Pi session cache runtime surface digest is invalid",
        )?;
        validate_sha256_digest(
            &self.connector_authority_sha256,
            "chat Pi session cache connector authority digest is invalid",
        )?;
        if Uuid::parse_str(&self.derived_session_id)
            .ok()
            .map(|id| id.to_string())
            .as_deref()
            != Some(self.derived_session_id.as_str())
        {
            return Err(DomainError::Invalid(
                "chat Pi session cache derived session id is invalid",
            ));
        }
        if !self.context_envelope_sha256.is_empty() {
            validate_sha256_digest(
                &self.context_envelope_sha256,
                "chat Pi session cache context digest is invalid",
            )?;
        }
        if self.execution_model.validate().is_err() {
            return Err(DomainError::Invalid(
                "chat Pi session cache model lock is invalid",
            ));
        }
        Ok(())
    }

    pub(crate) fn matches(&self, scope: &PiSessionCacheScope<'_>) -> bool {
        self.harness_digest == scope.harness_digest
            && self.runtime_policy_sha256 == scope.runtime_policy_sha256
            && self.runtime_surface_sha256 == scope.runtime_surface_sha256
            && self.connector_authority_sha256 == scope.connector_authority_sha256
            && self.memory_access_enabled == scope.memory_access_enabled
            && self.memory_update_enabled == scope.memory_update_enabled
            && &self.execution_model == scope.execution_model
    }
}

pub const MAX_SET_ASIDE_MEMORY: usize = 64;

/// Edit forks (X18): another version of this chat's tail. Every version shares
/// the messages up to `after_message_id` (none: from the first message) and
/// differs from the user message after it. Versions stay inside the chat, so
/// their attachments and artifacts keep resolving under the same chat id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatVersion {
    pub id: String,
    pub after_message_id: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub archived_at_ms: i64,
}

pub const MAX_CHAT_VERSIONS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatSession {
    pub id: String,
    pub loupist_id: String,
    /// Stable identity of the derived Pi execution session for this Chat.
    /// SQLite remains canonical; Pi's JSONL is a digest-bound derived cache.
    pub pi_session_id: String,
    #[serde(default)]
    pub pi_session_cache: Option<PiSessionCache>,
    pub title: String,
    pub memory_policy: MemoryPolicy,
    /// Principles and methods the user set aside for this chat ("Not this
    /// time"). The host leaves them out of this chat's context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub set_aside_memory: Vec<String>,
    /// Edit forks (X18): versions of the tail the user is not looking at.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<ChatVersion>,
    pub messages: Vec<ChatMessage>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl ChatSession {
    pub fn validate(&self) -> Result<(), DomainError> {
        require_identifier(&self.id, "chat session id is empty or unsafe")?;
        require_identifier(&self.loupist_id, "chat loupist id is empty or unsafe")?;
        if Uuid::parse_str(&self.pi_session_id)
            .ok()
            .map(|value| value.to_string())
            .as_deref()
            != Some(self.pi_session_id.as_str())
        {
            return Err(DomainError::Invalid("chat Pi session id is invalid"));
        }
        if let Some(cache) = &self.pi_session_cache {
            cache.validate()?;
        }
        if self.versions.len() > MAX_CHAT_VERSIONS {
            return Err(DomainError::Invalid("too many chat versions"));
        }
        for version in &self.versions {
            require_identifier(&version.id, "chat version id is empty or unsafe")?;
            if let Some(after) = &version.after_message_id {
                require_identifier(after, "chat version anchor is empty or unsafe")?;
            }
            if version.messages.first().map(|message| &message.role) != Some(&ChatRole::User) {
                return Err(DomainError::Invalid(
                    "a chat version starts with a message the user sent",
                ));
            }
        }
        if self.set_aside_memory.len() > MAX_SET_ASIDE_MEMORY {
            return Err(DomainError::Invalid("too many set-aside memory records"));
        }
        for record_id in &self.set_aside_memory {
            require_identifier(record_id, "set-aside memory record id is empty or unsafe")?;
        }
        // An empty title represents an untitled chat; rename validation lives in commands.
        if self.created_at_ms < 0 || self.updated_at_ms < self.created_at_ms {
            return Err(DomainError::Invalid("chat timestamps are invalid"));
        }

        let mut ids = BTreeSet::new();
        for message in &self.messages {
            message.validate()?;
            if !ids.insert(message.id.as_str()) {
                return Err(DomainError::Invalid("duplicate chat message id"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::catalog::ExecutionModelLock;
    use std::collections::BTreeMap;

    const DIGEST_A: &str = concat!(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    const DIGEST_B: &str = concat!(
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    );
    const DIGEST_C: &str = concat!(
        "cccccccccccccccccccccccccccccccc",
        "cccccccccccccccccccccccccccccccc"
    );
    const DIGEST_D: &str = concat!(
        "dddddddddddddddddddddddddddddddd",
        "dddddddddddddddddddddddddddddddd"
    );
    const DIGEST_E: &str = concat!(
        "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
    );
    const DIGEST_F: &str = concat!(
        "ffffffffffffffffffffffffffffffff",
        "ffffffffffffffffffffffffffffffff"
    );

    #[test]
    fn evidence_limits_preserve_all_full_references_and_reject_excess() {
        let evidence_refs = (0..MAX_CHAT_EVIDENCE_REFS)
            .map(|index| ChatEvidenceRef {
                id: format!("ev_{index:032x}"),
                title: "Pinned result".into(),
                kind: ChatEvidenceKind::Original,
                url: None,
                provider: None,
                tool_name: "web_fetch".into(),
                read_ref: None,
                retrieved_at: "2026-10-11T00:00:00Z".into(),
                published_at: None,
                as_of: None,
                coverage: None,
                excerpt: "x".repeat(MAX_CHAT_EVIDENCE_EXCERPT_BYTES),
                excerpt_match: None,
                excerpt_truncated: false,
                content_sha256: DIGEST_A.into(),
                warnings: vec![],
                dataset_ref: None,
                model_delivery: Some(ChatEvidenceDelivery {
                    coverage: ChatEvidenceDeliveryCoverage::BoundedSample,
                    excerpt: "y".repeat(MAX_CHAT_EVIDENCE_EXCERPT_BYTES),
                    excerpt_truncated: false,
                    content_sha256: DIGEST_B.into(),
                }),
                upstream_evidence_ids: vec![],
            })
            .collect();
        let mut message = ChatMessage {
            evidence_refs,
            ..crate::test_fixtures::chat_message(
                "assistant-evidence-limit",
                ChatRole::Assistant,
                "Sourced answer",
            )
        };
        message.validate().unwrap();
        let reloaded: ChatMessage =
            serde_json::from_value(serde_json::to_value(&message).unwrap()).unwrap();
        reloaded.validate().unwrap();
        assert_eq!(reloaded.evidence_refs, message.evidence_refs);
        let mut extra = message.evidence_refs[0].clone();
        extra.id = format!("ev_{:032x}", MAX_CHAT_EVIDENCE_REFS);
        message.evidence_refs.push(extra);
        assert!(message.validate().is_err());
        message.evidence_refs.pop();
        message.evidence_refs[0].excerpt.push('!');
        assert!(message.validate().is_err());
        // Per-excerpt bounds still reject oversize text below the aggregate cap.
        message.evidence_refs[1].excerpt.clear();
        assert!(message.validate().is_err());
        message.evidence_refs[0].excerpt.pop();
        message.evidence_refs[0]
            .model_delivery
            .as_mut()
            .unwrap()
            .excerpt
            .push('!');
        assert!(message.validate().is_err());
    }

    #[test]
    fn evidence_match_requires_nonempty_utf8_byte_boundaries_inside_excerpt() {
        let excerpt = "앞🙂영업이익: 12.166조 원";
        let start = "앞🙂".len();
        let end = start + "영업이익:".len();
        let matched = ChatEvidenceMatch { start, end };
        assert!(matched.is_valid_for(excerpt));
        assert_eq!(&excerpt[matched.start..matched.end], "영업이익:");
        for invalid in [
            ChatEvidenceMatch { start: 0, end: 0 },
            ChatEvidenceMatch {
                start: end,
                end: start,
            },
            ChatEvidenceMatch { start: 1, end },
            ChatEvidenceMatch {
                start,
                end: end - 2,
            },
            ChatEvidenceMatch {
                start,
                end: excerpt.len() + 1,
            },
            ChatEvidenceMatch {
                start: usize::MAX,
                end: usize::MAX,
            },
        ] {
            assert!(!invalid.is_valid_for(excerpt));
        }
    }

    fn model_lock() -> ExecutionModelLock {
        ExecutionModelLock {
            profile_id: "openai-codex/gpt-5.6-luna".into(),
            name: "GPT 5.6 Luna".into(),
            provider: "openai-codex".into(),
            model: "gpt-5.6-luna".into(),
            thinking_level: "max".into(),
            run_options: BTreeMap::new(),
        }
    }

    fn cache_scope<'a>(execution_model: &'a ExecutionModelLock) -> PiSessionCacheScope<'a> {
        PiSessionCacheScope {
            harness_digest: DIGEST_B,
            runtime_policy_sha256: DIGEST_C,
            runtime_surface_sha256: DIGEST_D,
            connector_authority_sha256: DIGEST_F,
            memory_access_enabled: false,
            memory_update_enabled: false,
            execution_model,
        }
    }

    fn cache(model: &ExecutionModelLock, access: bool, update: bool) -> PiSessionCache {
        PiSessionCache {
            entries_sha256: DIGEST_A.into(),
            leaf_id: Some("leaf-1".into()),
            harness_digest: DIGEST_B.into(),
            runtime_policy_sha256: DIGEST_C.into(),
            runtime_surface_sha256: DIGEST_D.into(),
            connector_authority_sha256: DIGEST_F.into(),
            memory_access_enabled: access,
            memory_update_enabled: update,
            derived_session_id: "123e4567-e89b-42d3-a456-426614174000".into(),
            context_envelope_sha256: String::new(),
            context_envelope_stale: false,
            execution_model: model.clone(),
        }
    }

    #[test]
    fn session_cache_matches_only_the_sealed_surface_authority_and_model() {
        let model = model_lock();
        let scope = cache_scope(&model);
        let cache = cache(&model, false, false);
        cache.validate().unwrap();
        assert!(cache.matches(&scope));
        let mut other = model_lock();
        other.thinking_level = "high".into();
        for changed in [
            PiSessionCacheScope {
                harness_digest: DIGEST_E,
                ..scope
            },
            PiSessionCacheScope {
                runtime_policy_sha256: DIGEST_E,
                ..scope
            },
            PiSessionCacheScope {
                runtime_surface_sha256: DIGEST_E,
                ..scope
            },
            PiSessionCacheScope {
                connector_authority_sha256: DIGEST_E,
                ..scope
            },
            PiSessionCacheScope {
                execution_model: &other,
                ..scope
            },
        ] {
            assert!(!cache.matches(&changed));
        }
        let mut invalid = cache.clone();
        invalid.connector_authority_sha256.clear();
        assert!(invalid.validate().is_err());
        assert!(!invalid.matches(&scope));

        let profiles = [(false, false), (false, true), (true, false), (true, true)];
        for cached in profiles {
            let cache = self::cache(&model, cached.0, cached.1);
            for current in profiles {
                let scope = PiSessionCacheScope {
                    memory_access_enabled: current.0,
                    memory_update_enabled: current.1,
                    ..scope
                };
                assert_eq!(cache.matches(&scope), cached == current);
            }
        }

        // A stored cache missing an authority field is never decoded.
        let encoded = serde_json::to_value(&cache).unwrap();
        for field in [
            "runtimePolicySha256",
            "connectorAuthoritySha256",
            "memoryAccessEnabled",
            "memoryUpdateEnabled",
        ] {
            let mut missing = encoded.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<PiSessionCache>(missing).is_err(),
                "{field}"
            );
        }
    }

    fn retry_message(
        id: &str,
        role: ChatRole,
        status: ChatMessageStatus,
        content: &str,
    ) -> ChatMessage {
        ChatMessage {
            status,
            created_at_ms: 2,
            ..crate::test_fixtures::chat_message(id, role, content)
        }
    }

    #[test]
    fn retry_drops_only_a_failed_reply_and_clears_the_pi_cache() {
        let user = retry_message(
            "user-retry",
            ChatRole::User,
            ChatMessageStatus::Complete,
            "Compare margins",
        );
        let failed = retry_message(
            "assistant-retry",
            ChatRole::Assistant,
            ChatMessageStatus::Error,
            "The reply stopped.",
        );
        let chat = ChatSession {
            set_aside_memory: Vec::new(),
            messages: vec![user, failed],
            updated_at_ms: 3,
            ..crate::test_fixtures::chat_session("chat-retry", "loupist-retry")
        };
        chat.validate().unwrap();
        let (prompt, next) = prepare_chat_retry(&chat, 4).unwrap();
        assert_eq!(prompt, "Compare margins");
        assert!(next.messages.is_empty());
        assert!(next.pi_session_cache.is_none());
        assert_eq!(next.updated_at_ms, 4);
        let mut completed = chat.clone();
        completed.messages[1].status = ChatMessageStatus::Complete;
        assert!(prepare_chat_retry(&completed, 4).is_err());
        let mut saved = chat;
        saved.messages[1].memory_update = Some(MemoryUpdateResult {
            status: MemoryUpdateStatus::NoChange,
            commit_id: None,
            changes: vec![],
        });
        assert!(prepare_chat_retry(&saved, 4).is_err());
    }

    #[test]
    fn retry_resends_the_question_and_keeps_accepted_steers() {
        for status in [ChatMessageStatus::Error, ChatMessageStatus::Aborted] {
            let question = retry_message(
                "question",
                ChatRole::User,
                ChatMessageStatus::Complete,
                "Compare margins",
            );
            let steer = ChatMessage {
                steer: true,
                ..retry_message(
                    "steer",
                    ChatRole::User,
                    ChatMessageStatus::Complete,
                    "Use annual filings",
                )
            };
            let failed = retry_message("failed", ChatRole::Assistant, status, "Stopped.");
            let chat = ChatSession {
                messages: vec![question, steer.clone(), failed],
                ..crate::test_fixtures::chat_session("chat-retry", "loupist-retry")
            };
            let (prompt, next) = prepare_chat_retry(&chat, 4).unwrap();
            assert_eq!(prompt, "Compare margins");
            assert_eq!(next.messages, [steer]);
            assert_eq!(chat.messages.len(), 3);
        }
    }
}
