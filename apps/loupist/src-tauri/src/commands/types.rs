//! Renderer-facing DTOs shared by the command modules, plus their converters
//! from domain and runtime values.

use serde::{Deserialize, Serialize};

use crate::{
    agent_harness::AgentHarnessSnapshot,
    app::{CommandError, LoupistAvailability},
    chat::artifacts::{ChatArtifact, ChatArtifactRef, ChatArtifactRevision},
    chat::chart::ChartDataset,
    chat::progress::{ChatProgress, ChatProgressPatch},
    domain::{ChatMessageStatus, ChatRole, ChatSession, MemoryAccess, MemoryRefSnapshot},
    providers::catalog::ExecutionModelLock,
};

use super::{iso_time, map_internal};

pub use crate::domain::{
    ChatPerformance as ChatPerformanceDto, ChatSessionCacheStatus as ChatSessionCacheStatusDto,
};

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LoupistSummary {
    pub id: String,
    pub name: String,
    pub workspace_path: String,
    pub philosophy: String,
    pub instructions: String,
    pub avatar: String,
    pub tone: crate::domain::LoupistTone,
    /// The template the Loupist was made from; missing for imported folders and packs.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub template: Option<crate::domain::LoupistTemplate>,
    pub record_count: usize,
    pub updated_at: String,
    pub accent: String,
    pub last_model_profile_id: Option<String>,
    pub enabled_skill_ids: Vec<String>,
    pub availability: LoupistAvailability,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LoupistWorkspace {
    pub loupist: LoupistSummary,
    pub threads: Vec<ChatThreadDto>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(deny_unknown_fields)]
pub struct LoupistCreateRequest {
    pub name: String,
    pub focus: Option<String>,
    pub avatar: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    /// Missing means Balanced.
    #[serde(default)]
    pub tone: Option<crate::domain::LoupistTone>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct LoupistImportRequest {
    pub workspace_path: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoupistProfileUpdateRequest {
    pub loupist_id: String,
    pub name: Option<String>,
    pub focus: Option<String>,
    pub instructions: Option<String>,
    pub avatar: Option<String>,
    pub tone: Option<crate::domain::LoupistTone>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct LoupistReconnectRequest {
    pub loupist_id: String,
    pub workspace_path: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct LoupistDeleteRequest {
    pub loupist_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeRefDto {
    pub kind: String,
    pub record_id: String,
    pub revision_id: String,
    pub object_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub span_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub receipt_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MemoryRefDto {
    pub read_ref: String,
    pub kind: String,
    pub title: String,
    pub excerpt: String,
    pub as_of: Option<String>,
    pub section: Option<String>,
    pub access: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub knowledge: Option<KnowledgeRefDto>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChatMessageDto {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub tool_refusals: Vec<crate::domain::ChatToolRefusal>,
    /// A data read in this reply never reached its source (the network or the
    /// provider was unreachable).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub offline_reads: bool,
    /// Asset reads this reply's tool accepted but its commit did not save.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub unsaved_theses: Vec<crate::domain::UnsavedThesis>,
    pub feedback: Option<crate::domain::ChatFeedback>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub failure_kind: Option<crate::domain::ChatFailureKind>,
    pub host_composed: bool,
    /// Accepted live guidance, shown as a "Guided" row.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub steer: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub retry_after_seconds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub delivery: Option<crate::domain::HomeDelivery>,
    /// The model Rust replaced with Auto for this answer (W17.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub replaced_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub alert: Option<crate::domain::HomeAlert>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub watch: Option<crate::domain::HomeWatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub earnings: Option<crate::domain::HomeEarnings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub vocabulary: Option<crate::domain::HomeVocabulary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub link: Option<crate::domain::HomeLink>,
    pub handoff: Option<crate::domain::HomeHandoff>,
    /// The as-of date set for this request and its answer (`YYYY-MM-DD`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub as_of: Option<String>,
    /// Number links Rust checked against their evidence (W16.3).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub number_checks: Vec<crate::domain::NumberCheck>,
    /// Grid results linked as paragraphs of their own (W16.8).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub sensitivity_cards: Vec<crate::domain::SensitivityCard>,
    /// What an as-of answer could not use (W16.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub as_of_report: Option<crate::domain::AsOfReport>,
    pub id: String,
    pub role: ChatMessageRoleDto,
    pub content: String,
    /// `content` is the host's failure placeholder (`FAILED_CHAT_CONTENT`),
    /// not model text; the renderer explains the failure from its run state.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub placeholder: bool,
    pub created_at: String,
    pub status: ChatMessageStatus,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub memory_refs: Vec<MemoryRefDto>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub lens_receipts: Vec<crate::domain::LensReceipt>,
    pub observed_exact_count: u64,
    pub refs_truncated: bool,
    pub refs_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub memory_update: Option<crate::domain::MemoryUpdateResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub memory_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub execution_model: Option<ExecutionModelLock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub agent_harness: Option<AgentHarnessSnapshot>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub attachments: Vec<ChatAttachmentDto>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub artifact_refs: Vec<ChatArtifactRef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub evidence_refs: Vec<crate::domain::ChatEvidenceRef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub schedule_receipts: Vec<crate::scheduled_tasks::types::ScheduleReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub progress: Option<ChatProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub performance: Option<ChatPerformanceDto>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChatAttachmentDto {
    pub id: String,
    pub filename: String,
    pub media_type: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChatThreadDto {
    pub id: String,
    pub loupist_id: String,
    /// The Loupist's home Thread: listed first, landed on, never renamed or deleted.
    pub home: bool,
    pub title: String,
    pub updated_at: String,
    pub use_memory: bool,
    pub update_memory: bool,
    /// Principle and method ids the user set aside for this chat.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub set_aside_memory: Vec<String>,
    /// Edit forks (X18): the other versions of this chat's tail, without their messages.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub versions: Vec<ChatVersionDto>,
    pub messages: Vec<ChatMessageDto>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(deny_unknown_fields)]
pub struct ChatCreateRequest {
    pub loupist_id: String,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(deny_unknown_fields)]
pub struct ChatFeedbackRequest {
    pub loupist_id: String,
    pub thread_id: String,
    pub message_id: String,
    pub rating: Option<crate::domain::ChatFeedbackRating>,
    pub correction: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ChatRenameRequest {
    pub loupist_id: String,
    pub thread_id: String,
    pub title: String,
}

/// "Not this time": leave one principle or method out of this chat's context, or bring it back.
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(deny_unknown_fields)]
pub struct ChatBranchRequest {
    pub loupist_id: String,
    pub thread_id: String,
    /// The user message being edited; everything from it on becomes an earlier version.
    pub message_id: String,
    /// A message just sent may still carry the renderer's id; then its place in the
    /// chat and its exact text identify it.
    #[serde(default)]
    pub position: Option<u32>,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ChatVersionSwitchRequest {
    pub loupist_id: String,
    pub thread_id: String,
    pub version_id: String,
}

/// One other version of a chat's tail, as the renderer lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChatVersionDto {
    pub id: String,
    pub after_message_id: Option<String>,
    pub first_message_at_ms: i64,
    pub message_count: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ChatMemorySetAsideRequest {
    pub loupist_id: String,
    pub thread_id: String,
    pub record_id: String,
    pub set_aside: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ChatDeleteRequest {
    pub loupist_id: String,
    pub thread_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(deny_unknown_fields)]
pub struct ChatSendRequest {
    pub run_id: String,
    pub loupist_id: String,
    pub thread_id: String,
    pub prompt: String,
    pub use_memory: bool,
    pub update_memory: bool,
    #[serde(default)]
    pub as_of: Option<String>,
    pub model_profile_id: String,
    pub thinking_level: String,
    pub run_options: std::collections::BTreeMap<String, String>,
    /// Composer `@entry_id` tokens. Host keeps only enabled execute-ready ids.
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub mentioned_connection_ids: Vec<String>,
    pub attachments: Vec<ChatAttachmentInput>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub pins: Vec<ChatKnowledgePin>,
    #[serde(default)]
    pub selected_artifact: Option<ChatArtifactSelection>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ChatArtifactSelection {
    pub artifact_id: String,
    pub revision: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceTextSelection {
    pub start_utf16: u32,
    pub end_utf16: u32,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatKnowledgePin {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub selection: Option<SourceTextSelection>,
    pub kind: String,
    pub record_id: Option<String>,
    pub revision_id: Option<String>,
    pub object_id: String,
    pub span_id: Option<String>,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ChatControlRequestDto {
    pub loupist_id: String,
    pub thread_id: String,
    pub prompt: String,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChatControlReceiptDto {
    pub message_id: String,
    pub prompt: String,
    pub created_at: String,
    pub mode: ChatControlModeDto,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ChatControlModeDto {
    Steer,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ChatAttachmentInput {
    pub filename: String,
    pub media_type: String,
    pub data_base64: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ChatAttachmentReadRequest {
    pub loupist_id: String,
    pub thread_id: String,
    pub message_id: String,
    pub attachment_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChatAttachmentReadDto {
    pub data_url: String,
}

/// The Loupist's Work list: charts and documents across its chats.
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(deny_unknown_fields)]
pub struct LoupistArtifactListRequest {
    pub loupist_id: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LoupistArtifactItemDto {
    pub artifact: crate::chat::artifacts::ChatArtifact,
    pub chat_title: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ChatArtifactListRequest {
    pub loupist_id: String,
    pub thread_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(deny_unknown_fields)]
pub struct ChatArtifactReadRequest {
    pub loupist_id: String,
    pub thread_id: String,
    pub artifact_id: String,
    pub revision_id: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ChatArtifactRestoreRequest {
    pub loupist_id: String,
    pub thread_id: String,
    pub artifact_id: String,
    pub revision_id: u32,
    pub expected_current_revision: u32,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChatArtifactViewDto {
    pub artifact: ChatArtifact,
    pub revision: ChatArtifactRevision,
    pub chart_dataset: Option<ChartDataset>,
    /// A grid's cited evidence, from the reply that published the revision,
    /// so its number popovers open wherever the grid is shown.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub evidence_refs: Vec<crate::domain::ChatEvidenceRef>,
}

impl From<crate::chat::artifacts::ChatArtifactSnapshot> for ChatArtifactViewDto {
    fn from(snapshot: crate::chat::artifacts::ChatArtifactSnapshot) -> Self {
        Self {
            artifact: snapshot.artifact,
            revision: snapshot.revision,
            chart_dataset: snapshot.chart_dataset,
            evidence_refs: snapshot.evidence_refs,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatStreamEvent {
    Started {
        run_id: String,
    },
    Memory {
        run_id: String,
        memories: Vec<MemoryRefDto>,
    },
    Delta {
        run_id: String,
        text: String,
        /// The live commentary this settled text replaces, so the renderer
        /// drops its copy in the same update that adds the text.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        settles: Option<String>,
    },
    GenerationFinished {
        run_id: String,
        performance: ChatPerformanceDto,
    },
    Title {
        run_id: String,
        title: String,
    },
    Progress {
        run_id: String,
        sequence: u64,
        progress: ChatProgress,
    },
    ProgressPatch {
        run_id: String,
        patch: ChatProgressPatch,
    },
    /// Transient run phase. Not stored on the Chat message.
    Phase {
        run_id: String,
        phase: ChatRunPhase,
    },
    MemoryUpdate {
        run_id: String,
        result: Box<crate::domain::MemoryUpdateResult>,
    },
    Schedule {
        run_id: String,
        receipt: crate::scheduled_tasks::types::ScheduleReceipt,
    },
    Evidence {
        run_id: String,
        evidence: Box<crate::domain::ChatEvidenceRef>,
    },
    /// A chart or table this reply published, sent once its publish call is
    /// delivered so the card fills in place while the answer is written. It
    /// is not saved: the `Artifact` reference follows only if the reply
    /// commits, and a stopped or failed reply drops it.
    ArtifactStaged {
        run_id: String,
        view: Box<ChatArtifactViewDto>,
    },
    Artifact {
        run_id: String,
        artifact: ChatArtifactRef,
    },
    Completed {
        lens_receipts: Vec<crate::domain::LensReceipt>,
        tool_refusals: Vec<crate::domain::ChatToolRefusal>,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        #[cfg_attr(test, ts(optional = nullable))]
        offline_reads: bool,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        #[cfg_attr(test, ts(optional = nullable))]
        unsaved_theses: Vec<crate::domain::UnsavedThesis>,
        host_composed: bool,
        run_id: String,
        message_id: String,
        final_text: String,
        created_at: String,
        execution_model: Box<ExecutionModelLock>,
        agent_harness: Box<AgentHarnessSnapshot>,
        performance: ChatPerformanceDto,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        replaced_model: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        as_of: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        #[cfg_attr(test, ts(optional = nullable))]
        number_checks: Vec<crate::domain::NumberCheck>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        #[cfg_attr(test, ts(optional = nullable))]
        sensitivity_cards: Vec<crate::domain::SensitivityCard>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        as_of_report: Option<Box<crate::domain::AsOfReport>>,
    },
    Aborted {
        run_id: String,
    },
    Error {
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        retry_after_seconds: Option<u32>,
        failure_kind: crate::domain::ChatFailureKind,
        run_id: String,
        message: String,
        message_id: String,
        final_text: String,
        /// `final_text` is the host's failure placeholder, not model text.
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        #[cfg_attr(test, ts(optional = nullable))]
        placeholder: bool,
        created_at: String,
        execution_model: Box<ExecutionModelLock>,
        agent_harness: Box<AgentHarnessSnapshot>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        progress: Option<ChatProgress>,
    },
}

/// The roles a stored Chat message may cross the renderer boundary with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ChatMessageRoleDto {
    User,
    Assistant,
}

/// What a running reply is doing between progress steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ChatRunPhase {
    Thinking,
    Writing,
    /// Waiting for the model's next step after a tool call.
    Tool,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RunStarted {
    pub run_id: String,
}

pub(crate) fn memory_access_label(access: MemoryAccess) -> &'static str {
    match access {
        MemoryAccess::SearchDiscovered => "search_discovered",
        MemoryAccess::ExactRead => "exact_read",
    }
}

pub(crate) fn memory_ref_dto(memory: &MemoryRefSnapshot) -> MemoryRefDto {
    let knowledge = if memory.record_id.is_empty() && memory.receipt_id.is_none() {
        None
    } else {
        Some(KnowledgeRefDto {
            kind: memory.kind.to_ascii_lowercase(),
            record_id: memory.record_id.clone(),
            revision_id: memory.revision_id.clone(),
            object_id: memory.object_id.clone(),
            span_id: memory.span_id.clone(),
            receipt_id: memory.receipt_id.clone(),
        })
    };
    MemoryRefDto {
        read_ref: memory.read_ref.clone(),
        kind: memory.kind.clone(),
        title: memory.title.clone(),
        excerpt: memory.excerpt.clone(),
        as_of: memory.as_of.clone(),
        section: memory.section.clone(),
        access: memory_access_label(memory.access).into(),
        knowledge,
    }
}

pub(crate) fn chat_dto(chat: &ChatSession) -> Result<ChatThreadDto, CommandError> {
    chat.validate().map_err(map_internal)?;
    let messages = chat
        .messages
        .iter()
        .map(|message| {
            let role = match message.role {
                ChatRole::User => ChatMessageRoleDto::User,
                ChatRole::Assistant => ChatMessageRoleDto::Assistant,
                ChatRole::System | ChatRole::Tool => {
                    return Err(CommandError::internal(
                        "stored non-user chat role cannot cross the renderer boundary",
                    ));
                }
            };
            Ok(ChatMessageDto {
                tool_refusals: message.tool_refusals.clone(),
                offline_reads: message
                    .tool_trace
                    .as_ref()
                    .is_some_and(crate::domain::ChatToolTrace::has_offline_read),
                unsaved_theses: message
                    .tool_trace
                    .as_ref()
                    .map(crate::domain::ChatToolTrace::unsaved_theses)
                    .unwrap_or_default(),
                feedback: message.feedback.clone(),
                failure_kind: message.failure_kind,
                host_composed: message.host_composed,
                steer: message.steer,
                retry_after_seconds: message.retry_after_seconds,
                delivery: message.delivery.clone(),
                replaced_model: message.replaced_model.clone(),
                alert: message.alert.clone(),
                watch: message.watch.clone(),
                earnings: message.earnings.clone(),
                vocabulary: message.vocabulary.clone(),
                link: message.link.clone(),
                handoff: message.handoff.clone(),
                as_of: message.as_of.clone(),
                number_checks: message.number_checks.clone(),
                sensitivity_cards: message.sensitivity_cards.clone(),
                as_of_report: message.as_of_report.clone(),
                id: message.id.clone(),
                role,
                content: message.content.clone(),
                // Stored failed replies carry only the placeholder text.
                placeholder: role == ChatMessageRoleDto::Assistant
                    && message.status == ChatMessageStatus::Error
                    && message.content == super::chat_runtime::FAILED_CHAT_CONTENT,
                created_at: iso_time(message.created_at_ms)?,
                status: message.status,
                lens_receipts: message.lens_receipts.clone(),
                memory_refs: message.memory_refs.iter().map(memory_ref_dto).collect(),
                observed_exact_count: message.observed_exact_count,
                refs_truncated: message.refs_truncated,
                refs_digest: message.refs_digest.clone(),
                memory_update: message.memory_update.clone(),
                memory_revision: message.memory_revision.clone(),
                execution_model: message.execution_model.clone(),
                agent_harness: message.agent_harness.clone(),
                attachments: message
                    .attachments
                    .iter()
                    .map(|attachment| ChatAttachmentDto {
                        id: attachment.id.clone(),
                        filename: attachment.filename.clone(),
                        media_type: attachment.media_type.clone(),
                        size_bytes: attachment.size_bytes,
                    })
                    .collect(),
                artifact_refs: message.artifact_refs.clone(),
                evidence_refs: message.evidence_refs.clone(),
                schedule_receipts: message.schedule_receipts.clone(),
                progress: message.progress.clone(),
                performance: message.performance.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ChatThreadDto {
        id: chat.id.clone(),
        loupist_id: chat.loupist_id.clone(),
        home: crate::chat::home::is_home(chat),
        title: chat.title.clone(),
        updated_at: iso_time(chat.updated_at_ms)?,
        use_memory: chat.memory_policy.use_memory,
        update_memory: chat.memory_policy.update_memory,
        set_aside_memory: chat.set_aside_memory.clone(),
        versions: chat
            .versions
            .iter()
            .map(|version| ChatVersionDto {
                id: version.id.clone(),
                after_message_id: version.after_message_id.clone(),
                first_message_at_ms: version
                    .messages
                    .first()
                    .map_or(version.archived_at_ms, |message| message.created_at_ms),
                message_count: u32::try_from(version.messages.len()).unwrap_or(u32::MAX),
            })
            .collect(),
        messages,
    })
}
