pub(crate) mod as_of;
mod chat_reads;
pub(crate) mod followed;
pub(crate) mod journal;
pub(crate) mod lens_archive;
pub(crate) mod memory_controls;
pub(crate) mod memory_usage;
pub(crate) mod method_learning;
pub(crate) mod principles;
pub(crate) mod world_schema;
mod world_setup;
pub(crate) use followed::{sync_followed_assets, FollowedSync};
pub(crate) use world_setup::{
    complete_pending_world_model, complete_pending_world_models, initialize_new_world,
    release_unknown_world_models,
};
#[cfg(test)]
pub(crate) use world_setup::{set_world_model_for_test, world_model_for_test};
pub(crate) mod source_export;
mod source_presentation;
pub(crate) mod source_selection;
pub(super) use chat_reads::*;
mod selected_references;
pub(super) use selected_references::{
    attach_selected_references, selected_reference_hints, validate_selected_references,
};
mod learning;
pub(super) use learning::*;
pub(crate) mod working_notes;
use serde::{Deserialize, Serialize};
use tauri::State;
pub(super) use working_notes::{
    attach_working_context, current_working_context, read_working_context,
    validate_working_note_revision,
};

use crate::app::{AppState, CommandError};
use crate::knowledge::{
    present_world, BindingRole, BlobStore, ClaimStatus, Coverage, KnowledgeError, OwnedRecord,
    RecordKind, SourceRecordPayload, WorldGraphQuery, WorldGraphView,
};
use crate::runs::maintenance::{ActivityLease, MaintenanceActivityKind};
use crate::store::LoupistStore;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeCatalogRequest {
    pub loupist_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeObjectRefDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<crate::custom_connections::taint::CustomOrigin>,
    pub summary: Option<String>,
    pub origin_label: Option<String>,
    pub acquired_at: Option<crate::knowledge::TimePoint>,
    pub publication_time: Option<crate::knowledge::TimePoint>,
    pub processing: Option<crate::knowledge::SourceProcessing>,
    pub id: String,
    pub title: String,
    /// A Source the host wrote keeps an English title; the interface shows this key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_key: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeRecordSummaryDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_key: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub lens_kind: Option<crate::knowledge::lens_page::LensKind>,
    pub record_id: String,
    pub kind: RecordKind,
    pub title: String,
    pub revision_id: String,
    pub revision_digest: String,
    pub learned: bool,
    pub read_only: bool,
    pub indexing_state: String,
    pub objects: Vec<KnowledgeObjectRefDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub archived: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeCatalogDto {
    pub loupist_id: String,
    pub worlds: Vec<KnowledgeRecordSummaryDto>,
    pub lenses: Vec<KnowledgeRecordSummaryDto>,
    pub sources: Vec<KnowledgeRecordSummaryDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeSourceReadRequest {
    pub loupist_id: String,
    pub record_id: String,
    pub revision_id: String,
    pub source_id: Option<String>,
    pub span_id: Option<String>,
    pub purpose: String,
    pub world_record_id: Option<String>,
    pub world_revision_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSpanViewDto {
    pub span_id: String,
    pub selector: String,
    pub context_note: String,
    pub media_type: String,
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation: Option<source_presentation::SourcePresentationDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDetailDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<crate::custom_connections::taint::CustomOrigin>,
    pub related_claims: Vec<crate::knowledge::SourceWorldClaim>,
    pub processing: Option<crate::knowledge::SourceProcessing>,
    pub source_id: String,
    pub record_id: String,
    pub revision_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_key: Option<&'static str>,
    pub source_class: String,
    pub origin_label: String,
    pub coverage: String,
    pub material: String,
    pub verification: String,
    pub derivation: Option<String>,
    pub spans: Vec<SourceSpanViewDto>,
    pub selected_span_id: Option<String>,
    pub publication_time: Option<crate::knowledge::TimePoint>,
    pub acquired_at: Option<crate::knowledge::TimePoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeLensReadRequest {
    pub loupist_id: String,
    pub record_id: String,
    /// The tool pins a read to the revision it was told about.
    #[serde(default)]
    pub revision_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeLensPagesForNodeRequest {
    pub loupist_id: String,
    pub node_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeConnectionsRequest {
    pub loupist_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeConnectionRequest {
    pub loupist_id: String,
    pub connection_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemorySectionForgetRequest {
    pub loupist_id: String,
    pub record_id: String,
    pub key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryOpUndoRequest {
    pub loupist_id: String,
    pub op_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryLearnedRequest {
    pub loupist_id: String,
    pub message_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeResearchSearchRequest {
    pub loupist_id: String,
    pub query: Option<String>,
    pub claim_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgePinDto {
    pub kind: String,
    pub loupist_id: String,
    pub record_id: Option<String>,
    pub revision_id: Option<String>,
    pub object_id: String,
    pub span_id: Option<String>,
    pub label: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchRecordDto {
    pub id: String,
    pub chat_id: Option<String>,
    pub title: String,
    pub summary: Option<String>,
    pub recorded_at: String,
    pub journal: Option<ResearchJournalDto>,
    pub inputs: Vec<KnowledgePinDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchJournalDto {
    pub section_id: String,
    pub conclusion: String,
    pub uncertainty: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeResearchPageDto {
    pub loupist_id: String,
    pub records: Vec<ResearchRecordDto>,
}

#[derive(Debug, Clone)]
pub(super) struct HostReadRequest {
    pub scope: String,
    pub record_id: String,
    pub revision_id: Option<String>,
    pub span_id: Option<String>,
    pub search: String,
    pub center_node_id: Option<String>,
    pub center_claim_id: Option<String>,
    pub kinds: Vec<String>,
    pub relations: Vec<String>,
    pub binding_roles: Vec<String>,
    pub statuses: Vec<String>,
    pub node_limit: Option<u32>,
    pub edge_limit: Option<u32>,
    pub byte_limit: Option<u32>,
    pub lens_status: String,
    /// An as-of cutoff: a World view keeps only relationships published by then.
    pub as_of: Option<chrono::NaiveDate>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldQueryDto {
    #[serde(flatten)]
    pub view: WorldGraphView,
    pub origin_label: String,
    pub applied_cursor: Option<String>,
}

#[tauri::command(async)]
pub fn knowledge_catalog(
    request: KnowledgeCatalogRequest,
    state: State<'_, AppState>,
) -> Result<KnowledgeCatalogDto, CommandError> {
    catalog(state.inner(), request)
}

#[tauri::command(async)]
pub fn knowledge_growth_events(
    request: crate::knowledge::GrowthEventsQuery,
    state: State<'_, AppState>,
) -> Result<crate::knowledge::GrowthEventsPage, CommandError> {
    with_loupist_knowledge(state.inner(), &request.loupist_id, |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs).growth_events(&request)
    })
}

#[tauri::command]
pub fn knowledge_world_query(
    request: WorldGraphQuery,
    state: State<'_, AppState>,
) -> Result<WorldQueryDto, CommandError> {
    world_query(state.inner(), request)
}

#[tauri::command(async)]
pub fn knowledge_map(
    request: crate::knowledge::KnowledgeMapQuery,
    state: State<'_, AppState>,
) -> Result<crate::knowledge::KnowledgeMap, CommandError> {
    map(state.inner(), request)
}

pub(crate) fn map(
    state: &AppState,
    request: crate::knowledge::KnowledgeMapQuery,
) -> Result<crate::knowledge::KnowledgeMap, CommandError> {
    admit(state, &request.loupist_id)?;
    let context = state.store.map_context().map_err(super::map_store)?;
    with_loupist_knowledge(state, &request.loupist_id, |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs).map(&request, &context)
    })
}

#[tauri::command]
pub fn knowledge_source_read(
    request: KnowledgeSourceReadRequest,
    state: State<'_, AppState>,
) -> Result<SourceDetailDto, CommandError> {
    source_read(state.inner(), request)
}

#[tauri::command]
pub fn knowledge_lens_read(
    request: KnowledgeLensReadRequest,
    state: State<'_, AppState>,
) -> Result<crate::knowledge::PageRead, CommandError> {
    lens_read(state.inner(), request)
}

#[tauri::command]
pub fn knowledge_lens_pages_for_node(
    request: KnowledgeLensPagesForNodeRequest,
    state: State<'_, AppState>,
) -> Result<Vec<crate::knowledge::PageSummary>, CommandError> {
    admit(&state, &request.loupist_id)?;
    with_loupist_knowledge(&state, &request.loupist_id.clone(), |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs)
            .pages_for_node(&request.loupist_id, &request.node_id)
    })
}

#[tauri::command]
pub fn knowledge_connections(
    request: KnowledgeConnectionsRequest,
    state: State<'_, AppState>,
) -> Result<Vec<crate::knowledge::ConnectionView>, CommandError> {
    admit(&state, &request.loupist_id)?;
    with_loupist_knowledge(&state, &request.loupist_id.clone(), |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs).connections(&request.loupist_id)
    })
}

fn connection_set_status(
    state: &AppState,
    request: &KnowledgeConnectionRequest,
    status: crate::knowledge::ConnectionStatus,
) -> Result<crate::knowledge::ConnectionView, CommandError> {
    let _activity = admit_mutation(state, &request.loupist_id)?;
    with_loupist_knowledge(state, &request.loupist_id, |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs).connection_set_status(
            &request.loupist_id,
            &request.connection_id,
            status,
            crate::commands::now_ms(),
        )
    })
}

/// Not useful: the link stays recorded so it is never found again.
#[tauri::command]
pub fn knowledge_connection_dismiss(
    request: KnowledgeConnectionRequest,
    state: State<'_, AppState>,
) -> Result<crate::knowledge::ConnectionView, CommandError> {
    connection_set_status(
        &state,
        &request,
        crate::knowledge::ConnectionStatus::Dismissed,
    )
}

/// Look into it: the renderer started a research hand-off for this link.
#[tauri::command]
pub fn knowledge_connection_open(
    request: KnowledgeConnectionRequest,
    state: State<'_, AppState>,
) -> Result<crate::knowledge::ConnectionView, CommandError> {
    connection_set_status(&state, &request, crate::knowledge::ConnectionStatus::Opened)
}

#[tauri::command]
pub fn memory_section_forget(
    request: MemorySectionForgetRequest,
    state: State<'_, AppState>,
) -> Result<crate::knowledge::PageRead, CommandError> {
    let _activity = admit_mutation(&state, &request.loupist_id)?;
    with_loupist_knowledge(&state, &request.loupist_id.clone(), |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs).forget_section(
            &request.loupist_id,
            &request.record_id,
            &request.key,
            crate::commands::now_ms(),
        )
    })
}

#[tauri::command]
pub fn memory_op_undo(
    request: MemoryOpUndoRequest,
    state: State<'_, AppState>,
) -> Result<crate::knowledge::PageRead, CommandError> {
    let _activity = admit_mutation(&state, &request.loupist_id)?;
    with_loupist_knowledge(&state, &request.loupist_id.clone(), |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs).undo_op(
            &request.loupist_id,
            &request.op_id,
            crate::commands::now_ms(),
        )
    })
}

#[tauri::command]
pub fn memory_learned(
    request: MemoryLearnedRequest,
    state: State<'_, AppState>,
) -> Result<Vec<crate::knowledge::LearnedDigest>, CommandError> {
    if request.message_ids.len() > 64 {
        return Err(CommandError::invalid("at most 64 messages per request"));
    }
    learned_digests(&state, &request.loupist_id, &request.message_ids)
}

/// What each message filed into Lens, from the log.
pub(crate) fn learned_digests(
    state: &AppState,
    loupist_id: &str,
    message_ids: &[String],
) -> Result<Vec<crate::knowledge::LearnedDigest>, CommandError> {
    admit(state, loupist_id)?;
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs).learned_digest(loupist_id, message_ids)
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeLensSearchRequest {
    pub loupist_id: String,
    pub query: String,
    #[serde(default)]
    pub kinds: Vec<crate::knowledge::lens_page::LensKind>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LensSearchHitDto {
    pub record_id: String,
    pub revision_id: String,
    pub kind: crate::knowledge::lens_page::LensKind,
    pub title: String,
    pub summary: String,
    pub snippet: String,
}

#[tauri::command]
pub fn knowledge_lens_search(
    request: KnowledgeLensSearchRequest,
    state: State<'_, AppState>,
) -> Result<Vec<LensSearchHitDto>, CommandError> {
    lens_search(state.inner(), request)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeJournalReadRequest {
    pub loupist_id: String,
    pub journal_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeJournalSourceReadRequest {
    pub loupist_id: String,
    pub journal_id: String,
    pub receipt_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalSourceReadDto {
    pub media_type: String,
    pub byte_length: usize,
    pub text: Option<String>,
    pub pinned: crate::knowledge::JournalSourceInput,
}

#[tauri::command]
pub fn knowledge_journal_read(
    request: KnowledgeJournalReadRequest,
    state: State<'_, AppState>,
) -> Result<crate::knowledge::JournalRecord, CommandError> {
    journal_read(state.inner(), request)
}

pub(crate) fn journal_read(
    state: &AppState,
    request: KnowledgeJournalReadRequest,
) -> Result<crate::knowledge::JournalRecord, CommandError> {
    admit(state, &request.loupist_id)?;
    with_loupist_knowledge(state, &request.loupist_id.clone(), |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs)
            .get_journal(&request.loupist_id, &request.journal_id)
    })
}

#[tauri::command]
pub fn knowledge_journal_source_read(
    request: KnowledgeJournalSourceReadRequest,
    state: State<'_, AppState>,
) -> Result<JournalSourceReadDto, CommandError> {
    journal_source_read(state.inner(), request)
}

pub(crate) fn journal_source_read(
    state: &AppState,
    request: KnowledgeJournalSourceReadRequest,
) -> Result<JournalSourceReadDto, CommandError> {
    admit(state, &request.loupist_id)?;
    let (bytes, media_type, pinned) =
        with_loupist_knowledge(state, &request.loupist_id.clone(), |tx, blobs| {
            crate::knowledge::KnowledgeWrite::bind(tx, blobs).read_journal_source(
                &request.loupist_id,
                &request.journal_id,
                &request.receipt_id,
            )
        })?;
    if bytes.len() > 1_000_000 {
        return Err(CommandError::invalid("journal source is too large"));
    }
    let byte_length = bytes.len();
    let text = String::from_utf8(bytes).ok();
    Ok(JournalSourceReadDto {
        media_type,
        byte_length,
        text,
        pinned,
    })
}

#[tauri::command]
pub fn knowledge_research_search(
    request: KnowledgeResearchSearchRequest,
    state: State<'_, AppState>,
) -> Result<KnowledgeResearchPageDto, CommandError> {
    research_search(state.inner(), request)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeSourceRetainRequest {
    pub loupist_id: String,
    pub title: String,
    pub origin_locator: String,
    pub coverage: String,
    pub publication_time: Option<crate::knowledge::TimePoint>,
    pub text: String,
    #[serde(default)]
    pub file: Option<KnowledgeSourceFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeSourceFile {
    pub media_type: String,
    pub file_name: String,
    pub bytes_base64: String,
}

fn acceptable_media_type(value: &str) -> bool {
    (3..=128).contains(&value.len())
        && value.contains('/')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b' ')
}

fn source_retain_request(
    request: KnowledgeSourceRetainRequest,
    at_ms: i64,
) -> Result<crate::knowledge::RetainNewSource, CommandError> {
    let coverage = match request.coverage.as_str() {
        "included_full" => Coverage::IncludedFull,
        "included_excerpt" => Coverage::IncludedExcerpt,
        "metadata_only" => Coverage::MetadataOnly,
        _ => return Err(CommandError::invalid("coverage is invalid")),
    };
    let (bytes, media_type, document_id, context_note, source_class) =
        if let Some(file) = request.file {
            if !request.text.is_empty() {
                return Err(CommandError::invalid(
                    "send pasted text or one original file, not both",
                ));
            }
            if !coverage.retains_bytes() {
                return Err(CommandError::invalid(
                    "an original file needs included coverage",
                ));
            }
            let decoded = base64::Engine::decode(
                &base64::engine::general_purpose::STANDARD,
                file.bytes_base64.trim(),
            )
            .map_err(|_| CommandError::invalid("file bytes are invalid"))?;
            if decoded.is_empty() || decoded.len() > crate::knowledge::MAX_BLOB_BYTES {
                return Err(CommandError::invalid("file is outside the retain limit"));
            }
            let media_type = file.media_type.trim().to_string();
            if !acceptable_media_type(&media_type) {
                return Err(CommandError::invalid("file media type is invalid"));
            }
            let document_id = format!("file-{}", crate::util::sha256(file.file_name.as_bytes()));
            (
                decoded,
                media_type,
                document_id,
                "Original file bytes".to_string(),
                crate::knowledge::SourceClass::UserFile,
            )
        } else {
            (
                request.text.into_bytes(),
                "text/plain".to_string(),
                "imported-text".to_string(),
                "Exact imported text".to_string(),
                crate::knowledge::SourceClass::UserStatement,
            )
        };
    let material = if coverage.retains_bytes() {
        Some(crate::knowledge::RetainMaterial {
            media_type,
            role: if coverage == Coverage::IncludedExcerpt {
                crate::knowledge::RepresentationRole::Excerpt
            } else {
                crate::knowledge::RepresentationRole::Original
            },
            document_id,
            parser_version: None,
            parent_representation_id: None,
            input_span_ids: Vec::new(),
            selector: crate::knowledge::Selector::ByteRange {
                start: 0,
                end: bytes.len() as u64,
            },
            context_note,
            bytes,
        })
    } else {
        None
    };
    Ok(crate::knowledge::RetainNewSource {
        origin: None,
        loupist_id: request.loupist_id,
        at_ms,
        title: request.title,
        source_class,
        origin_locator: request.origin_locator,
        provider_identity: "user".to_string(),
        authorship: crate::knowledge::Authorship::User,
        coverage,
        origin_chat_id: None,
        request_ref: None,
        declared_digest: None,
        publication_time: request.publication_time,
        material,
    })
}

#[tauri::command]
pub fn knowledge_source_retain(
    request: KnowledgeSourceRetainRequest,
    state: State<'_, AppState>,
) -> Result<crate::knowledge::RetainedSource, CommandError> {
    let _activity = admit_mutation(state.inner(), &request.loupist_id)?;
    let retain = source_retain_request(request, crate::commands::now_ms())?;
    let saved = with_loupist_knowledge(state.inner(), &retain.loupist_id.clone(), |tx, blobs| {
        let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
        let loupist_id = retain.loupist_id.clone();
        let settings = crate::knowledge::load_settings(tx, &loupist_id)?;
        let saved = write.retain_new_source(retain)?;
        write.enqueue_source_world(&loupist_id, &saved, &settings, crate::commands::now_ms())?;
        Ok(saved)
    })?;
    state.graph.wake();
    Ok(saved)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeGraphJobRequest {
    pub loupist_id: String,
    pub job_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeGraphLoupistRequest {
    pub loupist_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeGraphPauseRequest {
    pub loupist_id: String,
    pub paused: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeGraphSettingsRequest {
    pub loupist_id: String,
    pub settings: crate::knowledge::GraphPublicSettings,
}

fn load_graph_settings(
    state: &AppState,
    loupist_id: &str,
) -> Result<crate::knowledge::GraphProcessingSettings, CommandError> {
    with_loupist_knowledge(state, loupist_id, |tx, _| {
        crate::knowledge::load_settings(tx, loupist_id)
    })
}

fn graph_settings_view(
    state: &AppState,
    loupist_id: &str,
    settings: &crate::knowledge::GraphProcessingSettings,
) -> Result<crate::knowledge::GraphProcessingView, CommandError> {
    let model = settings
        .helper_model_profile_id
        .as_deref()
        .and_then(|id| state.model_catalog().ok()?.resolve(id).ok());
    let ready = model.as_ref().and_then(|model| {
        crate::providers::inference::ProviderInferenceService::from_state(state)
            .ok()?
            .readiness(&model.provider, &model.model)
            .ok()
    });
    let recovery = with_loupist_knowledge(state, loupist_id, |tx, _| {
        crate::knowledge::recovery_status(tx, loupist_id)
    })?;
    let status = if let Some(recovery) = recovery {
        recovery
    } else if !settings.enrichment_enabled {
        "disabled"
    } else if settings.paused {
        "paused"
    } else if settings.helper_model_profile_id.is_none() {
        "model_required"
    } else if model.as_ref().is_none_or(|model| {
        !crate::providers::inference::supports_isolated_inference(&model.provider)
    }) {
        "model_unavailable"
    } else if !state.graph.runtime_available() {
        "runtime_unavailable"
    } else if ready.as_ref().is_none_or(|ready| !ready.available) {
        "provider_disconnected"
    } else {
        "ready"
    };
    Ok(crate::knowledge::GraphProcessingView {
        settings: settings.public_settings(),
        readiness: crate::knowledge::GraphReadiness {
            status: status.into(),
            message: match status {
                "reconciliation_required" => Some("World processing was interrupted. Recover the last confirmed World to continue. The interrupted source will not be processed again automatically.".into()),
                "recovering" => Some("Restoring the last confirmed World. This does not use a model or retry interrupted processing.".into()),
                "runtime_unavailable" => {
                    Some("World processing is unavailable. Update or reinstall the app to restore it.".into())
                }
                "model_required" => Some(if settings.awaiting_default_model { "Waiting for a supported model provider. A recommended World model will be selected when one is connected." } else { "Choose a World processing model." }.into()),
                "model_unavailable" => {
                    Some("The selected model is unavailable for World processing.".into())
                }
                "provider_disconnected" => ready
                    .as_ref()
                    .and_then(|ready| ready.reason.clone())
                    .or_else(|| Some("Connect the selected provider in Settings.".into())),
                _ => None,
            },
            provider_name: ready
                .as_ref()
                .and_then(|ready| ready.provider_label.clone()),
            model_name: model.as_ref().map(|model| model.name.clone()),
            auth_kind: ready.and_then(|ready| ready.auth_kind),
        },
    })
}

#[tauri::command]
pub fn knowledge_graph_job(
    request: KnowledgeGraphJobRequest,
    state: State<'_, AppState>,
) -> Result<crate::knowledge::GraphJobView, CommandError> {
    admit(state.inner(), &request.loupist_id)?;
    with_loupist_knowledge(state.inner(), &request.loupist_id.clone(), |tx, _blobs| {
        crate::knowledge::get_job(tx, &request.loupist_id, &request.job_id)
    })
}

pub(crate) fn host_world_jobs(
    state: &AppState,
    loupist_id: &str,
) -> Result<serde_json::Value, CommandError> {
    admit(state, loupist_id)?;
    let jobs = with_loupist_knowledge(state, loupist_id, |tx, _| {
        crate::knowledge::list_jobs(tx, loupist_id)
    })?;
    serde_json::to_value(jobs).map_err(|error| CommandError::internal(error.to_string()))
}

#[tauri::command(async)]
pub fn knowledge_graph_recover(
    request: KnowledgeGraphLoupistRequest,
    state: State<'_, AppState>,
) -> Result<crate::knowledge::GraphProcessingView, CommandError> {
    let _activity = admit_mutation(state.inner(), &request.loupist_id)?;
    if !state.graph.runtime_available() {
        return Err(CommandError::invalid(
            "World processing is unavailable. Update or reinstall the app to restore it.",
        ));
    }
    with_loupist_knowledge(state.inner(), &request.loupist_id.clone(), |tx, blobs| {
        crate::knowledge::recover_world(tx, blobs, &request.loupist_id, crate::commands::now_ms())
    })?;
    state.graph.wake();
    graph_settings_view(
        state.inner(),
        &request.loupist_id,
        &load_graph_settings(state.inner(), &request.loupist_id)?,
    )
}

#[tauri::command(async)]
pub fn knowledge_graph_pause(
    request: KnowledgeGraphPauseRequest,
    state: State<'_, AppState>,
) -> Result<crate::knowledge::GraphProcessingView, CommandError> {
    let _activity = admit_mutation(state.inner(), &request.loupist_id)?;
    let settings =
        with_loupist_knowledge(state.inner(), &request.loupist_id.clone(), |tx, blobs| {
            let mut settings = crate::knowledge::load_settings(tx, &request.loupist_id)?;
            if request.paused {
                crate::knowledge::pause_processing(tx, &mut settings, &request.loupist_id)?;
            } else {
                crate::knowledge::resume_processing(tx, &mut settings, &request.loupist_id)?;
                crate::knowledge::KnowledgeWrite::bind(tx, blobs).enqueue_existing_sources(
                    &request.loupist_id,
                    &settings,
                    crate::commands::now_ms(),
                )?;
            }
            crate::knowledge::store_settings(tx, &request.loupist_id, &settings)?;
            Ok(settings)
        })?;
    if !request.paused {
        state.graph.wake();
    }
    graph_settings_view(state.inner(), &request.loupist_id, &settings)
}

#[tauri::command(async)]
pub fn knowledge_graph_settings_get(
    request: KnowledgeGraphLoupistRequest,
    state: State<'_, AppState>,
) -> Result<crate::knowledge::GraphProcessingView, CommandError> {
    admit(state.inner(), &request.loupist_id)?;
    graph_settings_view(
        state.inner(),
        &request.loupist_id,
        &load_graph_settings(state.inner(), &request.loupist_id)?,
    )
}

#[tauri::command(async)]
pub fn knowledge_graph_settings_set(
    request: KnowledgeGraphSettingsRequest,
    state: State<'_, AppState>,
) -> Result<crate::knowledge::GraphProcessingView, CommandError> {
    let _activity = admit_mutation(state.inner(), &request.loupist_id)?;
    let model = request
        .settings
        .helper_model_profile_id
        .as_deref()
        .map(|id| state.model_catalog()?.resolve(id))
        .transpose()?;
    if let Some(model) = &model {
        model.validate_thinking_level(&request.settings.thinking_level)?;
        model.validate_run_options(&request.settings.run_options)?;
    } else if request.settings.enrichment_enabled {
        return Err(CommandError::invalid(
            "Choose a World model before enabling automatic processing",
        ));
    }
    let current =
        with_loupist_knowledge(state.inner(), &request.loupist_id.clone(), |tx, blobs| {
            let mut current = crate::knowledge::load_settings(tx, &request.loupist_id)?;
            let was_paused = current.paused;
            current.apply_public(&request.settings, model.as_ref());
            current.validate()?;
            if current.paused && !was_paused {
                crate::knowledge::pause_processing(tx, &mut current, &request.loupist_id)?;
            }
            if !current.paused && was_paused {
                crate::knowledge::resume_processing(tx, &mut current, &request.loupist_id)?;
            }
            crate::knowledge::store_settings(tx, &request.loupist_id, &current)?;
            crate::knowledge::KnowledgeWrite::bind(tx, blobs).enqueue_existing_sources(
                &request.loupist_id,
                &current,
                crate::commands::now_ms(),
            )?;
            Ok(current)
        })?;
    state.graph.wake();
    graph_settings_view(state.inner(), &request.loupist_id, &current)
}

pub(crate) fn catalog(
    state: &AppState,
    request: KnowledgeCatalogRequest,
) -> Result<KnowledgeCatalogDto, CommandError> {
    admit(state, &request.loupist_id)?;
    with_loupist_knowledge(state, &request.loupist_id.clone(), |tx, blobs| {
        let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
        let mut worlds = Vec::new();
        let mut lenses = Vec::new();
        let mut sources = Vec::new();
        // Reference Sources of a removed shared pack stay hidden.
        let removed = crate::sharing::store::removed_sources(tx, &request.loupist_id)?;
        for module in write.list_records(&request.loupist_id)? {
            if removed.contains(&module.record_id) {
                continue;
            }
            let summary = summarize(&write, &request.loupist_id, module)?;
            match summary.kind {
                RecordKind::World => worlds.push(summary),
                RecordKind::Lens => lenses.push(summary),
                RecordKind::Source => sources.push(summary),
            }
        }
        Ok(KnowledgeCatalogDto {
            loupist_id: request.loupist_id,
            worlds,
            lenses,
            sources,
        })
    })
}

pub(crate) fn world_query(
    state: &AppState,
    request: WorldGraphQuery,
) -> Result<WorldQueryDto, CommandError> {
    admit(state, &request.loupist_id)?;
    with_loupist_knowledge(state, &request.loupist_id.clone(), |tx, blobs| {
        world_view_with_sources(&crate::knowledge::KnowledgeWrite::bind(tx, blobs), request)
    })
}

fn world_view_with_sources(
    write: &crate::knowledge::KnowledgeWrite<'_>,
    mut query: WorldGraphQuery,
) -> crate::knowledge::KnowledgeResult<WorldQueryDto> {
    let (_, _, byte_limit) = query.limits()?;
    for _ in 0..16 {
        let mut view = write.query_world(query.clone())?;
        if let Some(cutoff) = as_of::Cutoff::parse(query.published_by.as_deref()) {
            // Under an as-of cutoff a name's summary may carry later facts, and
            // a relationship shows its status and evidence as they were then.
            for node in &mut view.nodes {
                node.description.clear();
                node.aliases.clear();
                node.properties.clear();
            }
            as_of::world_view_as_of(
                write,
                &query.loupist_id,
                &mut view,
                query.center_node_id.as_deref(),
                cutoff,
            )?;
        }
        let spans = view
            .edges
            .iter()
            .flat_map(|edge| {
                edge.claim
                    .evidence
                    .iter()
                    .map(|evidence| evidence.span_id.clone())
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if spans.len() > 256 {
            if view.edges.len() <= 1 {
                return Err(KnowledgeError::ItemTooLarge);
            }
            query.edge_limit = Some((view.edges.len() / 2) as u32);
            continue;
        }
        view.source_targets = write.world_source_targets(
            &query.loupist_id,
            &view.record_id,
            &view.revision_id,
            &spans,
        )?;
        let presented = present_world(view, query.cursor.clone());
        let result = WorldQueryDto {
            applied_cursor: presented.applied_cursor,
            origin_label: presented.origin_label,
            view: presented.body,
        };
        let size = serde_json::to_vec(&result)?.len() as u32;
        if size <= byte_limit {
            return Ok(result);
        }
        let inner_limit = query.byte_limit.unwrap_or(byte_limit);
        query.byte_limit = Some(
            inner_limit
                .checked_sub(size - byte_limit)
                .filter(|limit| *limit > 0)
                .ok_or(KnowledgeError::ItemTooLarge)?,
        );
    }
    Err(KnowledgeError::ItemTooLarge)
}

pub(crate) fn source_read(
    state: &AppState,
    request: KnowledgeSourceReadRequest,
) -> Result<SourceDetailDto, CommandError> {
    admit(state, &request.loupist_id)?;
    if request.purpose != "renderer_inspection" {
        return Err(CommandError::invalid(
            "source reads from the renderer are inspection only",
        ));
    }
    with_loupist_knowledge(state, &request.loupist_id.clone(), |tx, blobs| {
        let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
        let revision = match (&request.world_record_id, &request.world_revision_id) {
            (Some(world_module), Some(world_revision)) => {
                let span = request
                    .span_id
                    .as_deref()
                    .ok_or(KnowledgeError::Invalid("World inspection requires a span"))?;
                let (target, revision) = write.world_source_revision(
                    &request.loupist_id,
                    world_module,
                    world_revision,
                    span,
                )?;
                if target.record_id != request.record_id
                    || target.revision_id != request.revision_id
                    || request
                        .source_id
                        .as_ref()
                        .is_some_and(|source| source != &target.source_id)
                {
                    return Err(KnowledgeError::Unauthorized(
                        "Source target does not match the selected World",
                    ));
                }
                revision
            }
            (None, None) => write.get_revision(&request.loupist_id, &request.revision_id)?,
            _ => {
                return Err(KnowledgeError::Invalid(
                    "World inspection context is incomplete",
                ))
            }
        };
        if revision.record_id != request.record_id || revision.kind != RecordKind::Source {
            return Err(KnowledgeError::Unauthorized(
                "revision belongs to another record",
            ));
        }
        let payload: crate::knowledge::SourceRecordPayload =
            serde_json::from_value(revision.payload).map_err(KnowledgeError::from)?;
        let source = payload.source;
        if !(request
            .source_id
            .as_ref()
            .is_none_or(|id| id == &source.source_id)
            && request.span_id.as_ref().is_none_or(|span_id| {
                source.captures.iter().any(|capture| {
                    capture.representations.iter().any(|representation| {
                        representation
                            .spans
                            .iter()
                            .any(|span| &span.span_id == span_id)
                    })
                })
            }))
        {
            return Err(KnowledgeError::NotFound);
        }
        let mut spans = Vec::new();
        let mut publication_time = None;
        let mut acquired_at = None;
        let mut coverage = String::new();
        let mut material = String::new();
        let mut verification = "unresolved".to_string();
        let mut derivation = None;
        for capture in &source.captures {
            let selected_capture = request.span_id.as_ref().is_none_or(|span_id| {
                capture.representations.iter().any(|representation| {
                    representation
                        .spans
                        .iter()
                        .any(|span| &span.span_id == span_id)
                })
            });
            if selected_capture {
                publication_time = capture.publication_time.clone();
                acquired_at =
                    chrono::DateTime::from_timestamp_millis(capture.received_at_ms).map(|time| {
                        crate::knowledge::TimePoint {
                            precision: crate::knowledge::TimePrecision::Instant,
                            value: Some(time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
                            approximate: false,
                        }
                    });
                coverage = capture.coverage.as_str().to_string();
                verification = if capture.coverage == Coverage::MetadataOnly {
                    "declared_digest".to_string()
                } else if capture.material_digest.is_some() {
                    "verified_bytes".to_string()
                } else {
                    "unresolved".to_string()
                };
            }
            for representation in &capture.representations {
                if selected_capture
                    && request.span_id.as_ref().is_none_or(|span_id| {
                        representation
                            .spans
                            .iter()
                            .any(|span| &span.span_id == span_id)
                    })
                {
                    material = representation.role.as_str().to_string();
                    derivation = (representation.role
                        == crate::knowledge::RepresentationRole::Derived)
                        .then(|| "derived".to_string());
                }
                for span in &representation.spans {
                    let selected = request.span_id.as_ref() == Some(&span.span_id);
                    let inspect = request.span_id.is_none() || selected;
                    let read = if inspect && capture.coverage.retains_bytes() {
                        Some(write.read_revision_span_bytes(
                            &request.record_id,
                            &span.span_id,
                            &span.selected_digest,
                        )?)
                    } else {
                        None
                    };
                    let media_type = read
                        .as_ref()
                        .map(|item| item.1.clone())
                        .unwrap_or_else(|| representation.media_type.clone());
                    let text = read.and_then(|item| {
                        if media_type.starts_with("text/")
                            || media_type == "application/json"
                            || media_type.ends_with("+json")
                        {
                            String::from_utf8(item.0).ok()
                        } else {
                            None
                        }
                    });
                    let presentation = text.as_deref().and_then(|text| {
                        source_presentation::source_presentation(
                            source.source_class,
                            representation.role,
                            &media_type,
                            text,
                        )
                    });
                    spans.push(SourceSpanViewDto {
                        span_id: span.span_id.clone(),
                        selector: serde_json::to_string(&span.selector)
                            .unwrap_or_else(|_| "{}".to_string()),
                        context_note: span.context_note.clone(),
                        media_type,
                        text,
                        presentation,
                    });
                }
            }
        }
        Ok(SourceDetailDto {
            origin: source.origin.clone(),
            related_claims: write.source_related_claims(
                &request.loupist_id,
                &request.record_id,
                &request.revision_id,
                &source.source_id,
            )?,
            processing: Some(write.source_processing(&request.loupist_id, &request.record_id)?),
            title_key: crate::knowledge::title_key(&source.provider_identity),
            source_id: source.source_id,
            record_id: request.record_id.clone(),
            revision_id: request.revision_id.clone(),
            title: source.title,
            source_class: source.source_class.as_str().to_string(),
            origin_label: source.origin_locator.unwrap_or_default(),
            coverage,
            material,
            verification,
            derivation,
            selected_span_id: request.span_id.clone(),
            spans,
            publication_time,
            acquired_at,
        })
    })
}

pub(crate) fn lens_read(
    state: &AppState,
    request: KnowledgeLensReadRequest,
) -> Result<crate::knowledge::PageRead, CommandError> {
    admit(state, &request.loupist_id)?;
    with_loupist_knowledge(state, &request.loupist_id.clone(), |tx, blobs| {
        let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
        let page = write.read_page(&request.loupist_id, &request.record_id)?;
        if request
            .revision_id
            .as_ref()
            .is_some_and(|id| id != &page.revision_id)
        {
            return Err(KnowledgeError::NotFound);
        }
        Ok(page)
    })
}

pub(crate) fn lens_search(
    state: &AppState,
    request: KnowledgeLensSearchRequest,
) -> Result<Vec<LensSearchHitDto>, CommandError> {
    let hits = host_search(state, &request.loupist_id, &request.query, "lens")?;
    let rows = hits.as_array().cloned().unwrap_or_default();
    with_loupist_knowledge(state, &request.loupist_id.clone(), |tx, blobs| {
        let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
        let mut found: Vec<LensSearchHitDto> = Vec::new();
        for hit in rows {
            let Some(record_id) = hit.get("recordId").and_then(|v| v.as_str()) else {
                continue;
            };
            // One hit per page: the index holds a metadata and a body document.
            if found.iter().any(|seen| seen.record_id == record_id) {
                continue;
            }
            let Ok(kind) = write.lens_kind(&request.loupist_id, record_id) else {
                continue;
            };
            if !request.kinds.is_empty() && !request.kinds.contains(&kind) {
                continue;
            }
            let summary = write
                .read_page(&request.loupist_id, record_id)
                .map(|page| page.body.summary().chars().take(240).collect())
                .unwrap_or_default();
            found.push(LensSearchHitDto {
                record_id: record_id.to_string(),
                revision_id: hit
                    .get("revisionId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                kind,
                title: hit
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Lens")
                    .to_string(),
                summary,
                snippet: hit
                    .get("snippet")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .chars()
                    .take(240)
                    .collect(),
            });
        }
        Ok(found)
    })
}

pub(crate) fn research_search(
    state: &AppState,
    request: KnowledgeResearchSearchRequest,
) -> Result<KnowledgeResearchPageDto, CommandError> {
    admit(state, &request.loupist_id)?;
    let query = request.query.unwrap_or_default();
    let claim_id = request.claim_id.filter(|claim| !claim.trim().is_empty());
    let page = with_loupist_knowledge(state, &request.loupist_id.clone(), |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs).research_history_without_later_notes(
            &request.loupist_id,
            &query,
            claim_id.as_deref(),
            Some(40),
        )
    })?;
    let loupist_id = page.loupist_id.clone();
    Ok(KnowledgeResearchPageDto {
        loupist_id: loupist_id.clone(),
        records: page
            .hits
            .into_iter()
            .map(|hit| {
                let conclusion = hit.conclusion.clone();
                let uncertainty = (!hit.uncertainty.trim().is_empty()).then_some(hit.uncertainty);
                ResearchRecordDto {
                    id: hit.journal_id.clone().unwrap_or(hit.research_id),
                    chat_id: hit.chat_id,
                    title: conclusion
                        .lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(160)
                        .collect(),
                    summary: hit.journal_id.is_none().then_some(conclusion.clone()),
                    recorded_at: recorded_at_rfc3339(hit.recorded_at_ms),
                    journal: hit.journal_id.map(|id| ResearchJournalDto {
                        section_id: id,
                        conclusion: conclusion.clone(),
                        uncertainty,
                    }),
                    inputs: hit
                        .claims
                        .into_iter()
                        .map(|claim| KnowledgePinDto {
                            kind: "world_claim".to_string(),
                            loupist_id: loupist_id.clone(),
                            record_id: Some(claim.record_id),
                            revision_id: Some(claim.revision_id),
                            object_id: claim.claim_id,
                            span_id: None,
                            label: claim.label,
                        })
                        .collect(),
                }
            })
            .collect(),
    })
}

fn recorded_at_rfc3339(at_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(at_ms)
        .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

fn admit(state: &AppState, loupist_id: &str) -> Result<(), CommandError> {
    state.ensure_loupist_available(loupist_id)?;
    state
        .store
        .get_loupist(loupist_id)
        .map_err(|error| CommandError::internal(error.to_string()))?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    Ok(())
}

fn admit_mutation(state: &AppState, loupist_id: &str) -> Result<ActivityLease, CommandError> {
    admit(state, loupist_id)?;
    state
        .maintenance
        .admit_kind(MaintenanceActivityKind::MemoryMutation)
}

fn summarize(
    write: &crate::knowledge::KnowledgeWrite<'_>,
    loupist_id: &str,
    module: OwnedRecord,
) -> crate::knowledge::KnowledgeResult<KnowledgeRecordSummaryDto> {
    write.record_permissions(loupist_id, &module.record_id)?;
    let read_only = false;
    let revision_id = module.revision_id.clone().unwrap_or_default();
    let revision_digest = if revision_id.is_empty() {
        String::new()
    } else {
        write
            .get_revision(loupist_id, &revision_id)?
            .revision_digest
    };
    let objects = if module.kind == RecordKind::Source && !revision_id.is_empty() {
        let payload: SourceRecordPayload =
            serde_json::from_value(write.get_revision(loupist_id, &revision_id)?.payload)?;
        let source = payload.source;
        let capture = source.captures.last();
        let summary = capture.and_then(|capture| {
            capture.representations.iter().find_map(|rep| {
                if !rep.media_type.starts_with("text/") && rep.media_type != "application/json" {
                    return None;
                }
                rep.spans
                    .first()
                    .and_then(|span| {
                        write
                            .source_catalog_text(
                                loupist_id,
                                &module.record_id,
                                &revision_id,
                                &span.span_id,
                            )
                            .ok()
                            .flatten()
                    })
                    .map(|text| {
                        let readable = source_presentation::source_presentation(
                            source.source_class,
                            rep.role,
                            &rep.media_type,
                            &text,
                        )
                        .map(|presentation| presentation.text)
                        .unwrap_or(text);
                        readable.chars().take(240).collect()
                    })
            })
        });
        vec![KnowledgeObjectRefDto {
            origin: source.origin.clone(),
            title_key: crate::knowledge::title_key(&source.provider_identity),
            id: source.source_id,
            title: source.title,
            summary,
            origin_label: source.origin_locator,
            acquired_at: capture.map(|capture| crate::knowledge::TimePoint {
                precision: crate::knowledge::TimePrecision::Instant,
                value: Some(recorded_at_rfc3339(capture.received_at_ms)),
                approximate: false,
            }),
            publication_time: capture.and_then(|capture| capture.publication_time.clone()),
            processing: Some(write.source_processing(loupist_id, &module.record_id)?),
        }]
    } else {
        Vec::new()
    };
    let (lens_kind, summary, updated_at_ms, archived, learned) = if module.kind == RecordKind::Lens
    {
        let kind = write.lens_kind(loupist_id, &module.record_id)?;
        let summary = write
            .read_page(loupist_id, &module.record_id)
            .map(|page| lens_catalog_preview(page.body.summary()))
            .unwrap_or_default();
        let (updated_at_ms, archived, learned) =
            write.lens_listing(loupist_id, &module.record_id)?;
        (
            Some(kind),
            Some(summary),
            Some(updated_at_ms),
            archived,
            learned,
        )
    } else {
        (None, None, None, false, false)
    };
    Ok(KnowledgeRecordSummaryDto {
        title_key: objects.first().and_then(|object| object.title_key),
        summary,
        lens_kind,
        read_only,
        indexing_state: "snapshot_only".to_string(),
        updated_at_ms,
        archived,
        learned,
        record_id: module.record_id,
        kind: module.kind,
        title: module.title,
        revision_id,
        revision_digest,
        objects,
    })
}

/// True when `record_id` is one of this Loupist's Lens records.
pub(crate) fn is_loupist_lens_record(
    state: &AppState,
    loupist_id: &str,
    record_id: &str,
) -> Result<bool, CommandError> {
    with_loupist_knowledge(state, loupist_id, |tx, _| {
        Ok(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM knowledge_records WHERE owner_loupist_id=?1 AND record_id=?2 AND kind='lens')",
            rusqlite::params![loupist_id, record_id],
            |row| row.get::<_, bool>(0),
        )?)
    })
}

fn with_loupist_knowledge<T>(
    state: &AppState,
    loupist_id: &str,
    body: impl FnOnce(&rusqlite::Transaction<'_>, &BlobStore) -> crate::knowledge::KnowledgeResult<T>,
) -> Result<T, CommandError> {
    let blobs = knowledge_blobs_for_loupist(state, loupist_id)?;
    let mut connection = state
        .store
        .lock_loupist(loupist_id)
        .map_err(|error| CommandError::internal(error.to_string()))?;
    let transaction = connection
        .transaction()
        .map_err(|error| CommandError::internal(error.to_string()))?;
    let value = body(&transaction, &blobs).map_err(map_knowledge)?;
    transaction
        .commit()
        .map_err(|error| CommandError::internal(error.to_string()))?;
    Ok(value)
}

pub(crate) fn knowledge_blobs_for_loupist(
    state: &AppState,
    loupist_id: &str,
) -> Result<BlobStore, CommandError> {
    if !state.store.is_folder_catalog() {
        return knowledge_blobs(state);
    }
    let root = state
        .store
        .loupist_data_dir(loupist_id)
        .map_err(super::map_store)?;
    BlobStore::open(root.join("knowledge-blobs")).map_err(map_knowledge)
}
pub(crate) fn knowledge_blobs(state: &AppState) -> Result<BlobStore, CommandError> {
    let root = state
        .store
        .database_path()
        .and_then(|path| path.parent().map(|parent| parent.join("knowledge-blobs")))
        .unwrap_or_else(|| state.artifacts.app_data_dir.join("knowledge-blobs"));
    BlobStore::open(root).map_err(map_knowledge)
}

fn map_knowledge(error: KnowledgeError) -> CommandError {
    if let Some(message) = outdated_memory_message(&error) {
        return CommandError::new("schema_mismatch", message);
    }
    if matches!(error, KnowledgeError::Invalid("world processing is busy")) {
        return CommandError::new(
            "world_busy",
            "The map is being updated. Try again in a minute.",
        );
    }
    let code = error.code();
    match code {
        "not_found" => CommandError::not_found("knowledge record"),
        "unauthorized" => CommandError::new("unauthorized", error.to_string()),
        "revision_conflict" => CommandError::conflict(error.to_string()),
        "cursor_stale" => CommandError::conflict(error.to_string()),
        "sqlite" => CommandError::internal("Could not read saved Memory."),
        _ => CommandError::new(code, error.to_string()),
    }
}

fn outdated_memory_message(error: &KnowledgeError) -> Option<&'static str> {
    let outdated = "This Loupist’s Memory is from an older version of the app. Create a new folder to use Lens and World.";
    match error {
        KnowledgeError::Sqlite(cause) => {
            let detail = cause.to_string();
            (detail.contains("no such table") || detail.contains("no such column"))
                .then_some(outdated)
        }
        _ => None,
    }
}

#[cfg(test)]
mod memory_error_tests {
    use super::{map_knowledge, KnowledgeError};

    #[test]
    fn a_missing_memory_table_does_not_name_the_database_object() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        let error = connection
            .execute("SELECT * FROM knowledge_lens_metadata", [])
            .expect_err("missing table");
        let mapped = map_knowledge(KnowledgeError::from(error));
        assert_eq!(mapped.code, "schema_mismatch");
        assert!(!mapped.message.to_ascii_lowercase().contains("sqlite"));
        assert!(!mapped.message.contains("knowledge_lens_metadata"));
    }
}

#[cfg(feature = "e2e")]
mod e2e_fixture;
#[cfg(feature = "e2e")]
pub(crate) use e2e_fixture::seed_e2e_knowledge_fixture;

#[cfg(test)]
mod memory_tests;
#[cfg(test)]
mod selected_reference_tests;
