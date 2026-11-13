//! Durable graph outbox. Job inputs live in SQLite, not in process memory.
//!
//! An inference attempt is not an exactly-once key, so a retry may be charged.
//! A Source extraction whose paid attempt published nothing runs once more;
//! after a second such attempt, or a mutation, the job becomes
//! `reconcile_required` and is not dispatched again.

use std::collections::BTreeSet;

use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::blob::BlobStore;
use super::canonical::digest_json;
use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{
    mint_id, validate_knowledge_id, validate_loupist_id, validate_text, validate_time, TimePoint,
};
use super::source::load_revision_span;
use crate::util::sha256;

mod worker_output;
use worker_output::project_worker_artifact;

pub const JOB_QUEUED: &str = "queued";
pub const JOB_RUNNING: &str = "running";
pub const JOB_COMPLETED: &str = "completed";
pub const JOB_CANCELLED: &str = "cancelled_before_start";

/// Absolute budget for a local extraction or index operation and its attempt lease.
/// Provider callbacks stay on `extraction.timeout_ms`; see [`attempt_budget_ms`].
pub const LOCAL_OPERATION_BUDGET_MS: u64 = 600_000;
/// Per-request World model limit. A high-thinking extraction of a full episode
/// can take minutes; the shared provider path accepts up to five.
pub const DEFAULT_TIMEOUT_MS: u64 = 240_000;
pub const MAX_TIMEOUT_MS: u64 = 300_000;
/// Sequential model waves in one episode (entities, entity resolution with
/// relations, relation resolution with attributes, summaries) plus one spare.
const EXTRACTION_WAVES: u64 = 5;
/// Extra runs for a Source whose attempt the host cut short: the app quit,
/// crashed, lost power or the Mac slept mid-call.
const HOST_INTERRUPTION_RETRIES: u8 = 2;
/// Attempts that sent no paid call before the job ends without quarantine.
const UNSENT_RETRIES: u8 = 3;

/// One attempt's wall-clock allowance: every model wave at its full limit, and
/// never less than the local operation budget.
pub fn attempt_budget_ms(timeout_ms: u64) -> u64 {
    LOCAL_OPERATION_BUDGET_MS.max(
        timeout_ms
            .saturating_add(15_000)
            .saturating_mul(EXTRACTION_WAVES),
    )
}
pub const JOB_FAILED_BEFORE: &str = "failed_before_mutation";
pub const JOB_RECONCILE: &str = "reconcile_required";
pub const JOB_RECONCILED: &str = "reconciled_without_retry";
pub const JOB_PAUSED: &str = "paused_budget";

const EPISODE_SPAN: &str = "source_span";
const EPISODE_SEALED: &str = "sealed_snapshot";
const EPISODE_RECOVERY: &str = "world_recovery";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ExtractionBudget {
    pub model_profile: String,
    pub thinking_level: String,
    pub run_options: std::collections::BTreeMap<String, String>,
    pub embedding_profile: String,
    pub max_llm_calls: u32,
    pub max_embedding_calls: u32,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct StoredSourceBinding {
    pub source_id: String,
    pub capture_id: String,
    pub representation_id: String,
    pub span_id: String,
    pub payload_digest: String,
    pub binding_role: String,
    pub attribution: String,
    #[serde(default)]
    pub source_record_id: String,
    #[serde(default)]
    pub source_revision_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct JobDocument {
    pub version: u32,
    pub episode_kind: String,
    pub generation_id: String,
    pub episode_key: String,
    /// Prior chunk of the same source span. Empty for a single-episode span.
    #[serde(default)]
    pub previous_episode_key: Option<String>,
    pub name: String,
    pub body: String,
    pub source_binding: Option<StoredSourceBinding>,
    /// Chat and message that retained this Source. Absent for Map adds, imports, and recovery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub origin_chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub origin_message_id: Option<String>,
    pub artifact_utf8: Option<String>,
    pub logical_digest: Option<String>,
    pub publication_time: Option<TimePoint>,
    pub effective_time: Option<TimePoint>,
    pub reference_time: String,
    /// Provenance shown to extraction.
    pub source_label: String,
    pub extraction: ExtractionBudget,
    pub dedup_key: String,
    pub mutation_sent: bool,
    pub inference_attempted: bool,
    pub usage_uncertain: bool,
    pub lease_expires_at_ms: Option<i64>,
    pub sealed_revision_id: Option<String>,
    #[serde(default)]
    pub attempt_id: Option<String>,
    /// Paid attempts that ended without publishing anything. The first runs
    /// again; the second quarantines the generation.
    #[serde(default)]
    pub interrupted_attempts: u8,
    /// Paid attempts the host cut short (quit, crash, power loss, sleep). These
    /// run again up to [`HOST_INTERRUPTION_RETRIES`] times before the rule above.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub host_interruptions: u8,
    /// Attempts that ended before any paid call went out. They cost nothing and
    /// run again up to [`UNSENT_RETRIES`] times.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unsent_attempts: u8,
}

fn is_zero(value: &u8) -> bool {
    *value == 0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct GraphJobView {
    pub job_id: String,
    pub loupist_id: String,
    pub world_record_id: String,
    pub world_revision_id: String,
    pub status: String,
    pub payload_digest: String,
    pub created_at_ms: i64,
    // Host-only extraction state; polling exposes the receipt, not Source text.
    #[serde(skip_serializing)]
    #[cfg_attr(test, ts(skip))]
    pub document: JobDocument,
    pub host_may_redrive: bool,
}

pub(crate) enum GraphJobClaim {
    Acquired(GraphJobView),
    Existing(GraphJobView),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphProcessingSettings {
    pub enrichment_enabled: bool,
    pub save_learning: bool,
    pub paused: bool,
    pub helper_provider: Option<String>,
    pub helper_model: Option<String>,
    pub embedding_profile: String,
    pub max_llm_calls: u32,
    pub max_embedding_calls: u32,
    pub timeout_ms: u64,
    pub inactive_loupists: Vec<String>,
    pub quarantined_generations: Vec<String>,
    pub helper_model_profile_id: Option<String>,
    /// Only new Loupists created before a suitable model is connected may auto-fill it.
    pub awaiting_default_model: bool,
    pub thinking_level: String,
    pub run_options: std::collections::BTreeMap<String, String>,
    /// What this Loupist researches. Empty leaves extraction unfocused.
    #[serde(default)]
    pub research_focus: String,
    #[serde(default)]
    pub relation_hints: Vec<String>,
    #[serde(default)]
    pub anchors: Vec<WorldAnchor>,
    /// Optional cheaper model for Graphiti's small calls. Empty uses the main model.
    #[serde(default)]
    pub fast_model: Option<String>,
    #[serde(skip)]
    pub extension: super::world_schema::WorldExtension,
    /// Item delivery and removal controls share the existing per-Loupist JSON row.
    #[serde(default)]
    pub memory_controls: super::memory_controls::MemoryControls,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldAnchor {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub identifiers: Vec<String>,
}

impl Default for GraphProcessingSettings {
    fn default() -> Self {
        Self {
            enrichment_enabled: false,
            save_learning: true,
            paused: false,
            helper_provider: None,
            helper_model: None,
            embedding_profile: "local-en-128".to_string(),
            max_llm_calls: 16,
            max_embedding_calls: 64,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            inactive_loupists: Vec::new(),
            quarantined_generations: Vec::new(),
            helper_model_profile_id: None,
            awaiting_default_model: false,
            thinking_level: "off".to_string(),
            run_options: Default::default(),
            research_focus: String::new(),
            relation_hints: Vec::new(),
            anchors: Vec::new(),
            fast_model: None,
            extension: super::world_schema::WorldExtension::default(),
            memory_controls: super::memory_controls::MemoryControls::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AddToWorld {
    pub loupist_id: String,
    pub world_record_id: String,
    pub world_revision_id: String,
    pub source_record_id: String,
    pub source_revision_id: String,
    pub span_id: String,
    pub at_ms: i64,
    pub binding_role: String,
}

#[cfg(any(test, feature = "eval-harness"))]
#[derive(Debug, Clone)]
pub struct SealedWorldIndex {
    pub loupist_id: String,
    pub world_record_id: String,
    pub world_revision_id: String,
    pub at_ms: i64,
}

pub fn load_settings(
    connection: &rusqlite::Connection,
    loupist_id: &str,
) -> KnowledgeResult<GraphProcessingSettings> {
    validate_loupist_id(loupist_id)?;
    let data: Option<String> = connection
        .query_row(
            "SELECT data_json FROM app_settings WHERE id = ?1",
            [format!("world_processing:{loupist_id}")],
            |row| row.get(0),
        )
        .optional()?;
    let mut settings: GraphProcessingSettings = data
        .map(|value| serde_json::from_str(&value))
        .transpose()?
        .unwrap_or_default();
    settings.extension = super::world_schema::live_extension(connection, loupist_id)?;
    settings.validate()?;
    Ok(settings)
}

pub fn store_settings(
    connection: &rusqlite::Connection,
    loupist_id: &str,
    settings: &GraphProcessingSettings,
) -> KnowledgeResult<()> {
    validate_loupist_id(loupist_id)?;
    settings.validate()?;
    connection.execute(
        "INSERT INTO app_settings (id, data_json) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET data_json = excluded.data_json",
        params![format!("world_processing:{loupist_id}"), serde_json::to_string(settings)?],
    )?;
    Ok(())
}

pub(crate) fn validate_saved_settings(connection: &rusqlite::Connection) -> KnowledgeResult<()> {
    let mut statement =
        connection.prepare("SELECT id FROM app_settings WHERE id LIKE 'world_processing:%'")?;
    for row in statement.query_map([], |row| row.get::<_, String>(0))? {
        let key = row?;
        let loupist_id = key
            .strip_prefix("world_processing:")
            .ok_or(KnowledgeError::Invalid("World settings key is invalid"))?;
        load_settings(connection, loupist_id)?;
        let present: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM loupist_profiles WHERE id = ?1)",
            [loupist_id],
            |row| row.get(0),
        )?;
        if !present {
            return Err(KnowledgeError::Invalid(
                "World settings belong to a missing Loupist",
            ));
        }
    }
    Ok(())
}

// A backfill only creates jobs, so its World head stays fixed in this transaction.
// Decode that head and its closed evidence once, instead of once per source span.
struct EnqueueWorld {
    record_id: String,
    revision_id: String,
    confirmed_spans: std::collections::BTreeSet<(String, String)>,
}

impl super::KnowledgeWrite<'_> {
    /// Sources whose relationships are not on the Map yet ([`sources_awaiting_world`]).
    pub fn sources_awaiting_world(&self, loupist_id: &str) -> KnowledgeResult<BTreeSet<String>> {
        sources_awaiting_world(self.tx, loupist_id)
    }

    /// Called when automatic processing is enabled or resumed. Deduplication
    /// preserves already attempted/completed sources and uncertain paid calls.
    pub fn enqueue_existing_sources(
        &self,
        loupist_id: &str,
        settings: &GraphProcessingSettings,
        at_ms: i64,
    ) -> KnowledgeResult<Vec<GraphJobView>> {
        let mut jobs = Vec::new();
        let existing = list_jobs(self.tx, loupist_id)?;
        let mut world = None;
        for record in super::catalog::list_records(self.tx, loupist_id)? {
            if record.kind != super::RecordKind::Source {
                continue;
            }
            let Some(revision_id) = record.revision_id else {
                continue;
            };
            let revision = super::revision::load_revision(self.tx, &revision_id)?;
            let payload: super::SourceRecordPayload = serde_json::from_str(&revision.payload_json)?;
            let source = payload.source;
            for capture in source.captures {
                for representation in capture.representations {
                    for span in representation.spans {
                        if world.as_ref().is_some_and(|world: &EnqueueWorld| {
                            world
                                .confirmed_spans
                                .contains(&(record.record_id.clone(), span.span_id.clone()))
                        }) {
                            continue;
                        }
                        let span_jobs: Vec<_> = existing
                            .iter()
                            .filter(|job| {
                                job.document
                                    .source_binding
                                    .as_ref()
                                    .is_some_and(|binding| binding.span_id == span.span_id)
                            })
                            .collect();
                        if !span_jobs.is_empty()
                            && !span_jobs.iter().any(|job| host_may_redrive(job))
                        {
                            continue;
                        }
                        let retained = super::RetainedSource {
                            record_id: record.record_id.clone(),
                            source_id: source.source_id.clone(),
                            capture_id: capture.capture_id.clone(),
                            representation_id: Some(representation.representation_id.clone()),
                            span_id: Some(span.span_id),
                            selected_digest: Some(span.selected_digest),
                            material_digest: capture.material_digest.clone(),
                            revision_id: revision_id.clone(),
                            revision_digest: revision.revision_digest.clone(),
                        };
                        if let Some(job) = self.enqueue_source_world_with_context(
                            loupist_id, &retained, settings, at_ms, &mut world,
                        )? {
                            jobs.push(job);
                        }
                    }
                }
            }
        }
        Ok(jobs)
    }

    /// Register source extraction in the same transaction that saves its bytes.
    /// A missing helper or unreadable source never rolls back a useful source save.
    pub fn enqueue_source_world(
        &self,
        loupist_id: &str,
        retained: &super::RetainedSource,
        settings: &GraphProcessingSettings,
        at_ms: i64,
    ) -> KnowledgeResult<Option<GraphJobView>> {
        self.enqueue_source_world_with_context(loupist_id, retained, settings, at_ms, &mut None)
    }

    fn enqueue_source_world_with_context(
        &self,
        loupist_id: &str,
        retained: &super::RetainedSource,
        settings: &GraphProcessingSettings,
        at_ms: i64,
        world: &mut Option<EnqueueWorld>,
    ) -> KnowledgeResult<Option<GraphJobView>> {
        use super::RecordKind;
        validate_loupist_id(loupist_id)?;
        validate_time(at_ms)?;
        let source_revision = super::revision::load_revision(self.tx, &retained.revision_id)?;
        let source =
            super::authority::require_record(self.tx, loupist_id, &source_revision.record_id)?;
        if source.kind != RecordKind::Source {
            return Err(KnowledgeError::Invalid(
                "World extraction requires an original source",
            ));
        }
        let Some(span_id) = &retained.span_id else {
            return Ok(None);
        };
        let existing_ids = {
            let mut statement = self.tx.prepare(
                "SELECT job_id FROM knowledge_graph_outbox WHERE loupist_id = ?1 AND json_extract(payload_json, '$.sourceBinding.spanId') = ?2 ORDER BY created_at_ms, job_id",
            )?;
            let ids = statement
                .query_map(params![loupist_id, span_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        };
        if let Some(id) = existing_ids.first() {
            for id in &existing_ids {
                let job = get_job(self.tx, loupist_id, id)?;
                if host_may_redrive(&job)
                    && settings.enrichment_enabled
                    && settings.save_learning
                    && !settings.inactive_loupists.iter().any(|id| id == loupist_id)
                    && settings
                        .helper_model
                        .as_deref()
                        .is_some_and(|model| !model.is_empty())
                    && !settings
                        .quarantined_generations
                        .contains(&job.document.generation_id)
                    && !generation_needs_reconcile(
                        self.tx,
                        loupist_id,
                        &job.world_record_id,
                        &job.document.generation_id,
                    )?
                {
                    let mut document = job.document.clone();
                    document.generation_id =
                        generation_for(self.tx, loupist_id, &job.world_record_id)?;
                    document.unsent_attempts = 0;
                    clear_lease(&mut document);
                    write_status(
                        self.tx,
                        id,
                        &job.status,
                        requeued(settings, &document),
                        &document,
                    )?;
                }
            }
            return get_job(self.tx, loupist_id, id).map(Some);
        }
        if !settings.enrichment_enabled
            || !settings.save_learning
            || settings.inactive_loupists.iter().any(|id| id == loupist_id)
            || settings.helper_model.as_deref().is_none_or(str::is_empty)
        {
            return Ok(None);
        }
        let payload: super::SourceRecordPayload =
            serde_json::from_str(&source_revision.payload_json)?;
        // A shared pack's reference Source enters the World only through its
        // pack's own claims; it is never sent to a model for extraction.
        if payload.source.provider_identity.starts_with("shared_pack:") {
            return Ok(None);
        }
        let text_representation = payload
            .source
            .captures
            .iter()
            .flat_map(|capture| &capture.representations)
            .find(|representation| {
                representation
                    .spans
                    .iter()
                    .any(|span| span.span_id == *span_id)
            })
            .is_some_and(|representation| {
                representation.media_type.starts_with("text/")
                    || representation.media_type == "application/json"
                    || representation.media_type.ends_with("+json")
            });
        // UTF-8 alone does not make a file extracted text: PDF and other file
        // formats can contain ASCII bytes that are not a readable source.
        if !text_representation {
            return Ok(None);
        }
        let material = load_revision_span(
            self.tx,
            self.blobs,
            loupist_id,
            &source.record_id,
            &retained.revision_id,
            span_id,
        )?;
        if !matches!(std::str::from_utf8(&material.bytes), Ok(body) if validate_text(body, 1_000_000, false, "graph source text").is_ok())
        {
            return Ok(None);
        }
        if world.is_none() {
            *world = Some(self.enqueue_world_context(loupist_id, at_ms)?);
        }
        let world = world.as_ref().expect("loaded enqueue World");
        if world
            .confirmed_spans
            .contains(&(source.record_id.clone(), span_id.clone()))
        {
            return Ok(None);
        }
        let generation = generation_for(self.tx, loupist_id, &world.record_id)?;
        if settings.quarantined_generations.contains(&generation)
            || generation_needs_reconcile(self.tx, loupist_id, &world.record_id, &generation)?
        {
            return Ok(None);
        }
        // Pausing suspends execution, not capture of newly saved source work.
        let mut admission = settings.clone();
        admission.paused = false;
        let job = accept_episode(
            self.tx,
            self.blobs,
            &admission,
            AddToWorld {
                loupist_id: loupist_id.into(),
                world_record_id: world.record_id.clone(),
                world_revision_id: world.revision_id.clone(),
                source_record_id: source.record_id,
                source_revision_id: retained.revision_id.clone(),
                span_id: span_id.clone(),
                at_ms,
                binding_role: "extracted_from".into(),
            },
        )?;
        if settings.paused && job.status == JOB_QUEUED {
            self.tx.execute(
                "UPDATE knowledge_graph_outbox SET status = ?1
                 WHERE loupist_id = ?2 AND status = ?3
                   AND json_extract(payload_json, '$.sourceBinding.spanId') = ?4",
                params![JOB_PAUSED, loupist_id, JOB_QUEUED, span_id],
            )?;
            return get_job(self.tx, loupist_id, &job.job_id).map(Some);
        }
        Ok(Some(job))
    }

    fn enqueue_world_context(&self, loupist_id: &str, at_ms: i64) -> KnowledgeResult<EnqueueWorld> {
        use super::{CreateRecord, RecordKind, SealWorld};
        let record_id: Option<String> = self.tx.query_row(
            "SELECT record_id FROM knowledge_records WHERE owner_loupist_id = ?1 AND kind = 'world'",
            [loupist_id], |row| row.get(0),
        ).optional()?;
        let world = match record_id {
            Some(id) => super::catalog::owned_record(self.tx, loupist_id, &id)?,
            None => self.create_record(CreateRecord {
                loupist_id: loupist_id.into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms,
            })?,
        };
        let revision_id = match world.revision_id {
            Some(revision) => revision,
            None => {
                self.seal_world(SealWorld {
                    loupist_id: loupist_id.into(),
                    record_id: world.record_id.clone(),
                    expected_revision_id: None,
                    at_ms,
                    nodes: Vec::new(),
                    claims: Vec::new(),
                    annotations: Vec::new(),
                    dependencies: Vec::new(),
                })?
                .revision_id
            }
        };
        let sealed = super::revision::load_revision(self.tx, &revision_id)?;
        let snapshot: super::WorldSnapshotBody = serde_json::from_str(&sealed.payload_json)?;
        let used: std::collections::BTreeSet<_> = snapshot
            .claims
            .iter()
            .flat_map(|claim| {
                claim
                    .evidence
                    .iter()
                    .map(|binding| binding.span_id.as_str())
            })
            .chain(
                snapshot
                    .annotations
                    .iter()
                    .flat_map(|annotation| annotation.source_span_ids.iter().map(String::as_str)),
            )
            .collect();
        let dependencies: Vec<super::EvidenceRevision> =
            serde_json::from_str(&sealed.dependency_json)?;
        let mut confirmed_spans = std::collections::BTreeSet::new();
        for dependency in dependencies
            .iter()
            .filter(|dependency| dependency.role == super::DependencyRole::WorldSource)
        {
            let closed =
                super::revision::current_source(self.tx, loupist_id, &dependency.record_id)?;
            let payload: super::SourceRecordPayload = serde_json::from_str(&closed.payload_json)?;
            for span in payload
                .source
                .captures
                .iter()
                .flat_map(|capture| &capture.representations)
                .flat_map(|representation| &representation.spans)
            {
                if used.contains(span.span_id.as_str()) {
                    confirmed_spans.insert((dependency.record_id.clone(), span.span_id.clone()));
                }
            }
        }
        Ok(EnqueueWorld {
            record_id: world.record_id,
            revision_id,
            confirmed_spans,
        })
    }
}

/// Chat origin lives on the capture lineage and the source-retained growth event.
/// World processing copies it onto the job so a later publication can stamp the chip.
fn source_chat_origin(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    capture_id: &str,
) -> KnowledgeResult<(Option<String>, Option<String>)> {
    let chat_id = tx
        .query_row(
            "SELECT chat_id FROM knowledge_capture_lineage
             WHERE record_id = ?1 AND capture_id = ?2",
            params![record_id, capture_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .filter(|chat_id| !chat_id.is_empty());
    let Some(chat_id) = chat_id else {
        return Ok((None, None));
    };
    let message_id = tx
        .query_row(
            "SELECT origin_message_id FROM knowledge_growth_events
             WHERE loupist_id = ?1 AND record_id = ?2 AND kind = 'source_retained'
               AND origin_chat_id = ?3
             ORDER BY at_ms DESC, event_id DESC
             LIMIT 1",
            params![loupist_id, record_id, chat_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .filter(|message_id| !message_id.is_empty());
    Ok((Some(chat_id), message_id))
}

pub fn accept_episode(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    settings: &GraphProcessingSettings,
    request: AddToWorld,
) -> KnowledgeResult<GraphJobView> {
    validate_loupist_id(&request.loupist_id)?;
    validate_knowledge_id(&request.world_record_id, "rec")?;
    validate_knowledge_id(&request.world_revision_id, "rev")?;
    validate_knowledge_id(&request.source_record_id, "rec")?;
    validate_knowledge_id(&request.source_revision_id, "rev")?;
    validate_knowledge_id(&request.span_id, "spn")?;
    validate_time(request.at_ms)?;
    ensure_active(settings, &request.loupist_id)?;
    if settings.paused {
        return Err(KnowledgeError::Invalid("graph processing is paused"));
    }
    if !settings.enrichment_enabled
        || settings.helper_provider.as_deref().unwrap_or("").is_empty()
        || settings.helper_model.as_deref().unwrap_or("").is_empty()
    {
        return Err(KnowledgeError::Invalid(
            "paid graph enrichment is not configured",
        ));
    }
    require_world_write(
        tx,
        &request.loupist_id,
        &request.world_record_id,
        &request.world_revision_id,
    )?;
    let material = load_revision_span(
        tx,
        blobs,
        &request.loupist_id,
        &request.source_record_id,
        &request.source_revision_id,
        &request.span_id,
    )?;
    let body = String::from_utf8(material.bytes)
        .map_err(|_| KnowledgeError::Invalid("graph episode body must be UTF-8"))?;
    validate_text(&body, 1_000_000, false, "graph episode body is invalid")?;
    let generation = generation_for(tx, &request.loupist_id, &request.world_record_id)?;
    if settings
        .quarantined_generations
        .iter()
        .any(|id| id == &generation)
        || generation_needs_reconcile(
            tx,
            &request.loupist_id,
            &request.world_record_id,
            &generation,
        )?
    {
        return Err(KnowledgeError::Invalid(
            "generation needs reconciliation before another mutation",
        ));
    }
    let extraction = budget(settings);
    let (episode_name, source_label) = episode_context(
        &material.title,
        material.source_class.as_str(),
        &material.provider_identity,
        material.publication_time.as_ref(),
    );
    let binding = StoredSourceBinding {
        source_id: material.source_id,
        capture_id: material.capture_id,
        representation_id: material.representation_id,
        span_id: material.span_id,
        payload_digest: material.selected_digest,
        binding_role: request.binding_role,
        attribution: material.provider_identity,
        source_record_id: request.source_record_id.clone(),
        source_revision_id: request.source_revision_id.clone(),
    };
    let (origin_chat_id, origin_message_id) = source_chat_origin(
        tx,
        &request.loupist_id,
        &request.source_record_id,
        &binding.capture_id,
    )?;
    let chunks = chunk_episode(&body);
    let mut created = None;
    let mut previous_episode_key = None;
    for (index, chunk) in chunks.iter().enumerate() {
        let mut chunk_binding = binding.clone();
        if chunks.len() > 1 {
            chunk_binding.payload_digest = sha256(chunk.as_bytes());
        }
        let mut identity = json!({
            "loupistId": request.loupist_id,
            "worldRecordId": request.world_record_id,
            "generationId": generation,
            "episodeKind": EPISODE_SPAN,
            "sourceId": chunk_binding.source_id,
            "captureId": chunk_binding.capture_id,
            "spanId": chunk_binding.span_id,
            "sourceRecordId": request.source_record_id,
            "sourceRevisionId": request.source_revision_id,
            "payloadDigest": chunk_binding.payload_digest,
            "modelProfile": extraction.model_profile,
            "embeddingProfile": extraction.embedding_profile,
        });
        if chunks.len() > 1 {
            identity["chunkIndex"] = json!(index);
        }
        let dedup_key = dedup_digest(&identity)?;
        if let Some(existing) = find_dedup(
            tx,
            &request.loupist_id,
            &request.world_record_id,
            &dedup_key,
        )? {
            previous_episode_key = Some(existing.document.episode_key.clone());
            if created.is_none() {
                created = Some(existing);
            }
            continue;
        }
        let episode_key = format!("ep_{}", &dedup_key[..16]);
        let mut chunk_extraction = extraction.clone();
        if chunks.len() > 1 {
            // One Source allowance, carried forward as each episode settles.
            // Later chunks own no model calls until their predecessor finishes.
            chunk_extraction.max_llm_calls = if index == 0 {
                extraction.max_llm_calls
            } else {
                0
            };
            chunk_extraction.max_embedding_calls =
                share_budget(extraction.max_embedding_calls, chunks.len(), index);
        }
        let document = JobDocument {
            version: 1,
            episode_kind: EPISODE_SPAN.to_string(),
            generation_id: generation.clone(),
            episode_key: episode_key.clone(),
            previous_episode_key: previous_episode_key.clone(),
            name: episode_name.clone(),
            body: chunk.clone(),
            source_binding: Some(chunk_binding),
            origin_chat_id: origin_chat_id.clone(),
            origin_message_id: origin_message_id.clone(),
            artifact_utf8: None,
            logical_digest: None,
            publication_time: material.publication_time.clone(),
            effective_time: None,
            reference_time: reference_time_for(
                material.publication_time.as_ref(),
                material.received_at_ms,
            ),
            source_label: source_label.clone(),
            extraction: chunk_extraction,
            dedup_key: dedup_key.clone(),
            mutation_sent: false,
            inference_attempted: false,
            usage_uncertain: false,
            lease_expires_at_ms: None,
            sealed_revision_id: Some(request.world_revision_id.clone()),
            attempt_id: None,
            interrupted_attempts: 0,
            host_interruptions: 0,
            unsent_attempts: 0,
        };
        let job = insert_job(
            tx,
            &request.loupist_id,
            &request.world_record_id,
            &request.world_revision_id,
            &dedup_key,
            request
                .at_ms
                .saturating_add(i64::try_from(index).unwrap_or(i64::MAX)),
            &document,
        )?;
        previous_episode_key = Some(episode_key);
        if created.is_none() {
            created = Some(job);
        }
    }
    created.ok_or(KnowledgeError::Invalid("graph episode was not queued"))
}

#[cfg(any(test, feature = "eval-harness"))]
pub fn accept_sealed_world(
    tx: &Transaction<'_>,
    _blobs: &BlobStore,
    settings: &GraphProcessingSettings,
    request: SealedWorldIndex,
) -> KnowledgeResult<GraphJobView> {
    validate_loupist_id(&request.loupist_id)?;
    validate_knowledge_id(&request.world_record_id, "rec")?;
    validate_knowledge_id(&request.world_revision_id, "rev")?;
    validate_time(request.at_ms)?;
    ensure_active(settings, &request.loupist_id)?;
    if settings.paused {
        return Err(KnowledgeError::Invalid("graph processing is paused"));
    }
    let module =
        super::authority::require_record(tx, &request.loupist_id, &request.world_record_id)?;
    if module.kind != super::model::RecordKind::World {
        return Err(KnowledgeError::Invalid(
            "graph index requires a sealed World",
        ));
    }
    require_world_read(
        tx,
        &request.loupist_id,
        &request.world_record_id,
        &request.world_revision_id,
    )?;
    let stored = super::revision::load_revision(tx, &request.world_revision_id)?;
    let logical_digest = stored.revision_digest;
    let generation_id = mint_id("gen");
    let dedup_key = dedup_digest(&json!({
        "episodeKind": EPISODE_SEALED,
        "loupistId": request.loupist_id,
        "logicalDigest": logical_digest,
        "origin": "sealed_world",
        "worldRecordId": request.world_record_id,
    }))?;
    if let Some(existing) = find_dedup(
        tx,
        &request.loupist_id,
        &request.world_record_id,
        &dedup_key,
    )? {
        return Ok(existing);
    }
    let document = JobDocument {
        version: 1,
        episode_kind: EPISODE_SEALED.to_string(),
        generation_id,
        episode_key: format!("imp_{}", &logical_digest[..16]),
        previous_episode_key: None,
        name: "sealed World".to_string(),
        body: String::new(),
        source_binding: None,
        origin_chat_id: None,
        origin_message_id: None,
        artifact_utf8: None,
        logical_digest: Some(logical_digest.clone()),
        publication_time: None,
        effective_time: None,
        reference_time: reference_time(request.at_ms),
        source_label: default_source_label(),
        extraction: ExtractionBudget {
            model_profile: "none".to_string(),
            thinking_level: "off".into(),
            run_options: Default::default(),
            embedding_profile: settings.embedding_profile.clone(),
            max_llm_calls: 0,
            max_embedding_calls: settings.max_embedding_calls,
            timeout_ms: settings.timeout_ms,
        },
        dedup_key: dedup_key.clone(),
        mutation_sent: false,
        inference_attempted: false,
        usage_uncertain: false,
        lease_expires_at_ms: None,
        sealed_revision_id: Some(request.world_revision_id.clone()),
        attempt_id: None,
        interrupted_attempts: 0,
        host_interruptions: 0,
        unsent_attempts: 0,
    };
    insert_job(
        tx,
        &request.loupist_id,
        &request.world_record_id,
        &request.world_revision_id,
        &dedup_key,
        request.at_ms,
        &document,
    )
}

/// Explicitly rebuild derived state from the confirmed SQLite head. The old
/// generation remains quarantined until this new generation is confirmed.
pub fn recover_world(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    loupist_id: &str,
    at_ms: i64,
) -> KnowledgeResult<GraphJobView> {
    validate_loupist_id(loupist_id)?;
    validate_time(at_ms)?;
    let settings = load_settings(tx, loupist_id)?;
    ensure_active(&settings, loupist_id)?;
    let world = super::catalog::world_record(tx, loupist_id)?.ok_or(KnowledgeError::NotFound)?;
    let revision = world.revision_id.ok_or(KnowledgeError::NotFound)?;
    require_world_write(tx, loupist_id, &world.record_id, &revision)?;
    let jobs = list_jobs(tx, loupist_id)?;
    if let Some(existing) = jobs.iter().find(|job| {
        job.world_record_id == world.record_id
            && job.document.episode_kind == EPISODE_RECOVERY
            && matches!(job.status.as_str(), JOB_QUEUED | JOB_RUNNING | JOB_PAUSED)
    }) {
        // A paused recovery was never dispatched; an explicit action may resume it.
        if existing.status == JOB_PAUSED {
            write_status(
                tx,
                &existing.job_id,
                JOB_PAUSED,
                JOB_QUEUED,
                &existing.document,
            )?;
        }
        return load_job(tx, &existing.job_id);
    }
    if world_processing_busy(tx, &world.record_id, "")? {
        return Err(KnowledgeError::Invalid("World processing is still running"));
    }
    if !jobs
        .iter()
        .any(|job| job.world_record_id == world.record_id && job.status == JOB_RECONCILE)
    {
        return Err(KnowledgeError::Invalid(
            "World processing does not need recovery",
        ));
    }
    insert_world_load(
        tx,
        blobs,
        &settings,
        loupist_id,
        &world.record_id,
        &revision,
        "Recover World processing",
        at_ms,
    )
}

/// Queue the rebuild a quarantined World needs without waiting for the user.
/// The rebuild calls no model and never retries the interrupted call. A
/// rebuild that failed waits for the explicit action, so it cannot loop.
pub(crate) fn recover_world_unattended(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    loupist_id: &str,
    at_ms: i64,
) -> KnowledgeResult<Option<GraphJobView>> {
    if recovery_status(tx, loupist_id)? != Some("reconciliation_required") {
        return Ok(None);
    }
    let last_rebuild: Option<String> = tx
        .query_row(
            "SELECT status FROM knowledge_graph_outbox
             WHERE loupist_id = ?1 AND json_extract(payload_json, '$.episodeKind') = ?2
             ORDER BY created_at_ms DESC, job_id DESC LIMIT 1",
            params![loupist_id, EPISODE_RECOVERY],
            |row| row.get(0),
        )
        .optional()?;
    if matches!(
        last_rebuild.as_deref(),
        Some(JOB_FAILED_BEFORE | JOB_RECONCILE)
    ) {
        return Ok(None);
    }
    match recover_world(tx, blobs, loupist_id, at_ms) {
        Ok(job) => Ok(Some(job)),
        // Running work, an inactive Loupist or no World yet: a later pump looks again.
        Err(
            KnowledgeError::Invalid(_) | KnowledgeError::Unauthorized(_) | KnowledgeError::NotFound,
        ) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Rebuild the derived index after a shared pack's Add or Remove, ahead of
/// queued extraction. A rebuild still waiting for an older head is cancelled
/// first. No model is called; it runs while
/// processing is paused and waits while the Loupist's processing is off.
pub(crate) fn reload_world(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    loupist_id: &str,
    at_ms: i64,
) -> KnowledgeResult<Option<GraphJobView>> {
    validate_loupist_id(loupist_id)?;
    validate_time(at_ms)?;
    let settings = load_settings(tx, loupist_id)?;
    let Some(world) = super::catalog::world_record(tx, loupist_id)? else {
        return Ok(None);
    };
    let Some(revision) = world.revision_id else {
        return Ok(None);
    };
    for job in list_jobs(tx, loupist_id)? {
        if job.world_record_id == world.record_id
            && job.document.episode_kind == EPISODE_RECOVERY
            && matches!(job.status.as_str(), JOB_QUEUED | JOB_PAUSED)
            && job.document.attempt_id.is_none()
            && !job.document.mutation_sent
            && !job.document.inference_attempted
        {
            write_status(tx, &job.job_id, &job.status, JOB_CANCELLED, &job.document)?;
        }
    }
    insert_world_load(
        tx,
        blobs,
        &settings,
        loupist_id,
        &world.record_id,
        &revision,
        "Load the World",
        at_ms,
    )
    .map(Some)
}

/// Queue a local rebuild from the sealed head (recovery and reload).
#[allow(clippy::too_many_arguments)]
fn insert_world_load(
    tx: &Transaction<'_>,
    _blobs: &BlobStore,
    settings: &GraphProcessingSettings,
    loupist_id: &str,
    world_record_id: &str,
    revision: &str,
    name: &str,
    at_ms: i64,
) -> KnowledgeResult<GraphJobView> {
    // Never reuse a possibly partially mutated generation, including a failed
    // previous recovery.
    let generation_id = mint_id("gen");
    let logical_digest = super::revision::load_revision(tx, revision)?.revision_digest;
    let dedup_key = format!("{revision}:{generation_id}");
    let document = JobDocument {
        version: 1,
        episode_kind: EPISODE_RECOVERY.into(),
        generation_id,
        episode_key: format!("imp_{}", &logical_digest[..16]),
        previous_episode_key: None,
        name: name.into(),
        body: String::new(),
        source_binding: None,
        origin_chat_id: None,
        origin_message_id: None,
        artifact_utf8: None,
        logical_digest: Some(logical_digest),
        publication_time: None,
        effective_time: None,
        reference_time: reference_time(at_ms),
        source_label: default_source_label(),
        extraction: ExtractionBudget {
            model_profile: "none".into(),
            thinking_level: "off".into(),
            run_options: Default::default(),
            embedding_profile: "local-en-128".into(),
            max_llm_calls: 0,
            max_embedding_calls: settings.max_embedding_calls,
            timeout_ms: settings.timeout_ms,
        },
        dedup_key: dedup_key.clone(),
        mutation_sent: false,
        inference_attempted: false,
        usage_uncertain: false,
        lease_expires_at_ms: None,
        sealed_revision_id: Some(revision.to_owned()),
        attempt_id: None,
        interrupted_attempts: 0,
        host_interruptions: 0,
        unsent_attempts: 0,
    };
    insert_job(
        tx,
        loupist_id,
        world_record_id,
        revision,
        &dedup_key,
        at_ms,
        &document,
    )
}

pub fn recovery_status(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<Option<&'static str>> {
    validate_loupist_id(loupist_id)?;
    // Status refreshes need only existence, not every retained source body,
    // artifact and historical attempt in the queue.
    let recovering: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM knowledge_graph_outbox
         WHERE loupist_id = ?1 AND status IN ('queued', 'running')
           AND json_extract(payload_json, '$.episodeKind') = 'world_recovery')",
        [loupist_id],
        |row| row.get(0),
    )?;
    if recovering {
        return Ok(Some("recovering"));
    }
    let interrupted: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM knowledge_graph_outbox
         WHERE loupist_id = ?1 AND status = ?2)",
        params![loupist_id, JOB_RECONCILE],
        |row| row.get(0),
    )?;
    Ok(interrupted.then_some("reconciliation_required"))
}

pub(super) fn fail_recovery(tx: &Transaction<'_>, job_id: &str) -> KnowledgeResult<()> {
    let job = load_job(tx, job_id)?;
    if job.document.episode_kind != EPISODE_RECOVERY
        || !matches!(job.status.as_str(), JOB_QUEUED | JOB_RUNNING)
    {
        return Ok(());
    }
    let mut document = job.document;
    let uncertain =
        document.mutation_sent || document.inference_attempted || document.usage_uncertain;
    clear_lease(&mut document);
    let status = if uncertain {
        JOB_RECONCILE
    } else {
        JOB_FAILED_BEFORE
    };
    write_status(tx, job_id, &job.status, status, &document)?;
    if uncertain {
        let mut settings = load_settings(tx, &job.loupist_id)?;
        quarantine(&mut settings, &document.generation_id);
        store_settings(tx, &job.loupist_id, &settings)?;
    }
    Ok(())
}

pub fn get_job(
    tx: &Transaction<'_>,
    loupist_id: &str,
    job_id: &str,
) -> KnowledgeResult<GraphJobView> {
    validate_loupist_id(loupist_id)?;
    let job = load_job(tx, job_id)?;
    if job.loupist_id != loupist_id {
        return Err(KnowledgeError::NotFound);
    }
    Ok(job)
}

/// Display the same user-pause distinction in Chat and the Source reader.
pub(super) fn processing_status(
    tx: &Transaction<'_>,
    loupist_id: &str,
    status: &str,
) -> KnowledgeResult<String> {
    Ok(
        if status == JOB_PAUSED && load_settings(tx, loupist_id)?.paused {
            "paused".into()
        } else {
            status.into()
        },
    )
}

pub fn get_processing_job(
    tx: &Transaction<'_>,
    loupist_id: &str,
    job_id: &str,
) -> KnowledgeResult<GraphJobView> {
    let mut job = get_job(tx, loupist_id, job_id)?;
    job.status = processing_status(tx, loupist_id, &job.status)?;
    Ok(job)
}

/// The next runnable job of each Loupist. Sources a Chat in `hot_chats` saved
/// come before an older backlog; otherwise jobs run in the order they were
/// queued. Either way a chunk still waits for the chunk before it.
pub(super) fn queued_loupist_heads(
    tx: &Transaction<'_>,
    hot_chats: &[String],
) -> KnowledgeResult<Vec<GraphJobView>> {
    // Read predecessor keys once. A correlated scan of full job JSON for each
    // queued chunk makes a large backfill quadratic and blocks other folder reads.
    // A chunk waits for the chunk before it, which is only extraction context:
    // once that chunk can no longer finish, the rest of the Source still runs.
    let mut statement = tx.prepare(
        "WITH completed AS MATERIALIZED (
           SELECT loupist_id, json_extract(payload_json, '$.episodeKey') AS episode_key
           FROM knowledge_graph_outbox WHERE status = 'completed'
         ), pending AS MATERIALIZED (
           SELECT loupist_id, json_extract(payload_json, '$.episodeKey') AS episode_key
           FROM knowledge_graph_outbox
           WHERE status IN ('queued', 'running', 'paused_budget', 'reconcile_required')
         )
         SELECT job_id, loupist_id, world_record_id, world_revision_id, status, payload_json,
                payload_digest, created_at_ms
         FROM knowledge_graph_outbox WHERE job_id IN (
           SELECT job_id FROM (
             SELECT job_id, ROW_NUMBER() OVER (
               PARTITION BY loupist_id
               ORDER BY COALESCE(json_extract(payload_json, '$.originChatId')
                                   IN (SELECT value FROM json_each(?1)), 0) DESC,
                        created_at_ms, job_id
             ) AS position
             FROM knowledge_graph_outbox candidate
             WHERE status = 'queued'
               AND (json_extract(payload_json, '$.episodeKind') = 'world_recovery'
                 OR NOT EXISTS (
                   SELECT 1 FROM knowledge_graph_outbox blocked
                   WHERE blocked.world_record_id = candidate.world_record_id
                     AND (blocked.status = 'reconcile_required'
                       OR (blocked.status IN ('queued', 'running')
                         AND json_extract(blocked.payload_json, '$.episodeKind') = 'world_recovery'))
                 ))
               AND (json_extract(payload_json, '$.previousEpisodeKey') IS NULL
                 OR EXISTS (
                   SELECT 1 FROM completed previous
                   WHERE previous.loupist_id = candidate.loupist_id
                     AND previous.episode_key = json_extract(candidate.payload_json, '$.previousEpisodeKey')
                 )
                 OR NOT EXISTS (
                   SELECT 1 FROM pending previous
                   WHERE previous.loupist_id = candidate.loupist_id
                     AND previous.episode_key = json_extract(candidate.payload_json, '$.previousEpisodeKey')
                 ))
           ) WHERE position = 1
         ) ORDER BY created_at_ms, job_id"
    )?;
    let rows = statement
        .query_map([serde_json::to_string(hot_chats)?], row_to_job)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// World jobs still to run in this database: queued or running.
pub(crate) fn backlog(tx: &Transaction<'_>) -> KnowledgeResult<u64> {
    Ok(tx.query_row(
        "SELECT COUNT(*) FROM knowledge_graph_outbox WHERE status IN (?1, ?2)",
        params![JOB_QUEUED, JOB_RUNNING],
        |row| row.get::<_, i64>(0),
    )? as u64)
}

/// Source records whose relationships are not on the Map yet: a job for them is
/// queued, running or paused.
fn sources_awaiting_world(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<BTreeSet<String>> {
    let mut statement = tx.prepare(
        "SELECT DISTINCT json_extract(payload_json, '$.sourceBinding.sourceRecordId')
         FROM knowledge_graph_outbox
         WHERE loupist_id = ?1 AND status IN (?2, ?3, ?4)
           AND json_extract(payload_json, '$.sourceBinding.sourceRecordId') IS NOT NULL",
    )?;
    let rows = statement.query_map(
        params![loupist_id, JOB_QUEUED, JOB_RUNNING, JOB_PAUSED],
        |row| row.get::<_, String>(0),
    )?;
    rows.collect::<Result<BTreeSet<_>, _>>()
        .map_err(KnowledgeError::from)
}

pub(super) fn latest_source_job(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<Option<GraphJobView>> {
    tx.query_row(
        "SELECT job_id, loupist_id, world_record_id, world_revision_id, status, payload_json, payload_digest, created_at_ms FROM knowledge_graph_outbox WHERE loupist_id = ?1 AND json_extract(payload_json, '$.sourceBinding.sourceRecordId') = ?2 ORDER BY created_at_ms DESC, job_id DESC LIMIT 1",
        params![loupist_id, record_id], row_to_job,
    ).optional().map_err(KnowledgeError::from)
}

/// Whether a job's status is final. A queued, running or paused job may still
/// read its Source; every other status means the World has published what it
/// read, or cannot read more from it (failed, cancelled, quarantined, reconciled).
pub(super) fn final_status(status: &str) -> bool {
    !matches!(status, JOB_QUEUED | JOB_RUNNING | JOB_PAUSED)
}

/// The Sources among `sources` the World is through with: no job of theirs is
/// queued or running, and a job for the Source's current revision has any
/// other status. A job that failed or was quarantined counts, because the
/// World cannot reveal more from it, and so does a paused one: a chunk whose
/// frozen allowance cannot fund an extraction, or a queue the user paused,
/// is not read without the user's action, and the resume that lets it settle
/// rechecks learning then. A Source with no job at all, or whose latest
/// revision has none, is not processed. Method learning reads this before
/// judging a pending page by the claims on the Sources it cites.
pub(super) fn processed_sources(
    tx: &Transaction<'_>,
    loupist_id: &str,
    sources: &BTreeSet<String>,
) -> KnowledgeResult<BTreeSet<String>> {
    let mut statement = tx.prepare(&format!(
        "SELECT source,
                SUM(status IN ('{queued}', '{running}')) AS pending,
                SUM(status NOT IN ('{queued}', '{running}') AND revision = head) AS done
         FROM (SELECT o.status,
                      json_extract(o.payload_json, '$.sourceBinding.sourceRecordId') AS source,
                      json_extract(o.payload_json, '$.sourceBinding.sourceRevisionId') AS revision,
                      h.revision_id AS head
               FROM knowledge_graph_outbox o
               JOIN knowledge_record_heads h
                 ON h.record_id = json_extract(o.payload_json, '$.sourceBinding.sourceRecordId')
               WHERE o.loupist_id = ?1
                 AND json_extract(o.payload_json, '$.sourceBinding.sourceRecordId')
                     IN (SELECT value FROM json_each(?2)))
         GROUP BY source",
        queued = JOB_QUEUED,
        running = JOB_RUNNING,
    ))?;
    let rows = statement.query_map(
        params![loupist_id, serde_json::to_string(sources)?],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        },
    )?;
    let mut processed = BTreeSet::new();
    for row in rows {
        let (source, pending, done) = row?;
        if pending == 0 && done > 0 {
            processed.insert(source);
        }
    }
    Ok(processed)
}

pub fn list_jobs(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<Vec<GraphJobView>> {
    validate_loupist_id(loupist_id)?;
    let mut statement = tx.prepare(
        "SELECT job_id, loupist_id, world_record_id, world_revision_id, status, payload_json,
                payload_digest, created_at_ms
         FROM knowledge_graph_outbox WHERE loupist_id = ?1 ORDER BY created_at_ms, job_id",
    )?;
    let rows = statement.query_map(params![loupist_id], row_to_job)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(KnowledgeError::from)
}

pub fn pause_processing(
    tx: &Transaction<'_>,
    settings: &mut GraphProcessingSettings,
    loupist_id: &str,
) -> KnowledgeResult<u32> {
    validate_loupist_id(loupist_id)?;
    settings.paused = true;
    let mut changed = 0;
    for job in list_jobs_with_status(tx, loupist_id, JOB_QUEUED)? {
        if job.document.episode_kind == EPISODE_RECOVERY {
            continue;
        }
        let mut document = job.document;
        document.lease_expires_at_ms = None;
        if write_status(tx, &job.job_id, JOB_QUEUED, JOB_PAUSED, &document)? {
            changed += 1;
        }
    }
    Ok(changed)
}

pub fn resume_processing(
    tx: &Transaction<'_>,
    settings: &mut GraphProcessingSettings,
    loupist_id: &str,
) -> KnowledgeResult<u32> {
    validate_loupist_id(loupist_id)?;
    let mut changed = 0;
    let jobs = list_jobs_with_status(tx, loupist_id, JOB_PAUSED)?;
    let mut blocked = jobs
        .iter()
        .filter(|job| {
            let document = &job.document;
            document.mutation_sent
                || document.inference_attempted
                || document.usage_uncertain
                || document.attempt_id.is_some()
                || document.lease_expires_at_ms.is_some()
                || (document.episode_kind == EPISODE_SPAN
                    && document.previous_episode_key.is_none()
                    && (document.extraction.max_llm_calls < 2
                        || document.extraction.max_embedding_calls == 0))
        })
        .map(|job| job.job_id.clone())
        .collect::<BTreeSet<_>>();
    // A follower cannot run while its spent predecessor stays parked. Keep
    // the whole stopped tail visible as stopped until the explicit retry.
    loop {
        let blocked_keys = jobs
            .iter()
            .filter(|job| blocked.contains(&job.job_id))
            .map(|job| job.document.episode_key.as_str())
            .collect::<BTreeSet<_>>();
        let descendants = jobs
            .iter()
            .filter(|job| {
                job.document
                    .previous_episode_key
                    .as_ref()
                    .is_some_and(|key| blocked_keys.contains(key.as_str()))
            })
            .map(|job| job.job_id.clone())
            .collect::<BTreeSet<_>>();
        let before = blocked.len();
        blocked.extend(descendants);
        if blocked.len() == before {
            break;
        }
    }
    for job in jobs {
        let document = &job.document;
        // Resume an untouched queue entry, never a previous paid or uncertain attempt.
        if blocked.contains(&job.job_id)
            || document.mutation_sent
            || document.inference_attempted
            || document.usage_uncertain
            || document.attempt_id.is_some()
            || document.lease_expires_at_ms.is_some()
            || settings
                .inactive_loupists
                .iter()
                .any(|loupist| loupist == loupist_id)
            || settings
                .quarantined_generations
                .contains(&document.generation_id)
        {
            continue;
        }
        if write_status(tx, &job.job_id, JOB_PAUSED, JOB_QUEUED, document)? {
            changed += 1;
        }
    }
    settings.paused = false;
    Ok(changed)
}

/// Explicit Source action only: fund one stopped span again. Resume and the
/// scheduler never call this, so each fresh allowance requires another action.
pub fn retry_source_processing(
    tx: &Transaction<'_>,
    settings: &GraphProcessingSettings,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<GraphJobView> {
    validate_loupist_id(loupist_id)?;
    let source = super::authority::require_record(tx, loupist_id, record_id)?;
    if source.kind != super::RecordKind::Source {
        return Err(KnowledgeError::Invalid("retry requires a Source"));
    }
    let jobs = list_jobs(tx, loupist_id)?
        .into_iter()
        .filter(|job| {
            job.document.source_binding.as_ref().is_some_and(|binding| {
                binding.source_record_id == record_id
                    && Some(&binding.source_revision_id) == source.head_revision_id.as_ref()
            })
        })
        .collect::<Vec<_>>();
    // Never refill while this Source is active or needs reconciliation.
    if jobs.iter().any(|job| {
        matches!(job.status.as_str(), JOB_RUNNING | JOB_RECONCILE)
            || (job.status == JOB_QUEUED
                && (job.document.extraction.max_llm_calls >= 2
                    || job.document.previous_episode_key.is_none()))
    }) {
        return Err(KnowledgeError::Invalid(
            "Source processing is active or uncertain",
        ));
    }
    let mut head = jobs
        .iter()
        .find(|job| {
            job.status == JOB_PAUSED
                && job.document.episode_kind == EPISODE_SPAN
                && job
                    .document
                    .previous_episode_key
                    .as_ref()
                    .is_none_or(|key| {
                        jobs.iter().any(|previous| {
                            previous.document.episode_key == *key
                                && previous.status == JOB_COMPLETED
                        })
                    })
        })
        .cloned()
        .ok_or(KnowledgeError::Invalid(
            "Source has no stopped span to retry",
        ))?;
    if !ready_to_run(settings, &head)
        || settings
            .quarantined_generations
            .contains(&head.document.generation_id)
        || generation_needs_reconcile(
            tx,
            loupist_id,
            &head.world_record_id,
            &head.document.generation_id,
        )?
        || settings.max_llm_calls < 2
        || settings.max_embedding_calls == 0
    {
        return Err(KnowledgeError::Invalid(
            "enable Map building with a sufficient allowance before retrying",
        ));
    }
    if head.document.extraction.max_llm_calls >= 2
        && head.document.extraction.max_embedding_calls > 0
    {
        return Err(KnowledgeError::Invalid("Source allowance has not run out"));
    }
    let mut tail = vec![head.clone()];
    while let Some(next) = jobs.iter().find(|job| {
        job.document.previous_episode_key.as_ref()
            == Some(&tail.last().unwrap().document.episode_key)
    }) {
        if tail.iter().any(|job| job.job_id == next.job_id) {
            return Err(KnowledgeError::Invalid("Source episode chain is invalid"));
        }
        tail.push(next.clone());
    }
    // A dispatched/uncertain follower is never silently reset with the head.
    if tail.iter().any(|job| {
        !matches!(job.status.as_str(), JOB_PAUSED | JOB_QUEUED)
            || job.document.mutation_sent
            || job.document.usage_uncertain
            || job.document.attempt_id.is_some()
            || job.document.lease_expires_at_ms.is_some()
            || (job.job_id != head.job_id && job.document.inference_attempted)
    }) {
        return Err(KnowledgeError::Invalid(
            "Source attempt cannot safely be retried",
        ));
    }
    for (index, job) in tail.iter().enumerate() {
        let mut document = job.document.clone();
        document.extraction = budget(settings);
        if index > 0 {
            document.extraction.max_llm_calls = 0;
        }
        document.inference_attempted = false;
        document.interrupted_attempts = 0;
        document.host_interruptions = 0;
        document.unsent_attempts = 0;
        write_status(tx, &job.job_id, &job.status, JOB_QUEUED, &document)?;
    }
    head = load_job(tx, &head.job_id)?;
    Ok(head)
}

/// Called once after acquiring the app-data lock and reclaiming orphaned processes.
/// No prior process can still own these running attempts; wall-clock lease expiry
/// is irrelevant on startup. The app quit, crashed or lost power during each of
/// them, so a Source's attempt runs again within its host allowance
/// ([`host_interrupted`]), then once more (`retry_once`); any other dispatched
/// or uncertain call is never replayed.
pub(crate) fn recover_interrupted(tx: &Transaction<'_>) -> KnowledgeResult<u32> {
    let mut changed = 0;
    for job in list_running(tx)? {
        let mut settings = load_settings(tx, &job.loupist_id)?;
        let mut document = job.document.clone();
        let retried = host_interrupted(&mut document) || retry_once(&mut document);
        let uncertain = !retried
            && (document.mutation_sent || document.inference_attempted || document.usage_uncertain);
        let next = if uncertain {
            quarantine(&mut settings, &document.generation_id);
            JOB_RECONCILE
        } else {
            requeued(&settings, &document)
        };
        clear_lease(&mut document);
        if write_status(tx, &job.job_id, JOB_RUNNING, next, &document)? {
            changed += 1;
        }
        if uncertain {
            store_settings(tx, &job.loupist_id, &settings)?;
        }
    }
    Ok(changed)
}

/// Reclaim abandoned attempts during normal scheduling. A live host worker is
/// excluded even if queueing or sleep took it past its wall-clock lease.
pub(super) fn recover_expired(
    tx: &Transaction<'_>,
    now_ms: i64,
    live_loupists: &std::collections::HashSet<String>,
) -> KnowledgeResult<Vec<String>> {
    let mut reclaimed = Vec::new();
    for job in list_running(tx)? {
        if live_loupists.contains(&job.loupist_id)
            || job
                .document
                .lease_expires_at_ms
                .is_some_and(|expiry| expiry > now_ms)
        {
            continue;
        }
        let mut settings = load_settings(tx, &job.loupist_id)?;
        let mut document = job.document.clone();
        if document.attempt_id.is_none() {
            document.attempt_id = Some(mint_id("atm"));
            write_status(tx, &job.job_id, JOB_RUNNING, JOB_RUNNING, &document)?;
        }
        finish_unpublished(
            tx,
            &mut settings,
            &job.job_id,
            document.attempt_id.as_deref().unwrap_or_default(),
            Unpublished {
                paid: job.document.inference_attempted,
                uncertain: job.document.usage_uncertain,
                suspended: false,
            },
            now_ms,
        )?;
        store_settings(tx, &job.loupist_id, &settings)?;
        reclaimed.push(job.job_id);
    }
    Ok(reclaimed)
}

/// Only the caller that changes QUEUED to RUNNING owns dispatch authority.
/// Observing an existing attempt never grants permission to invoke its worker.
pub(super) fn claim_running(
    tx: &Transaction<'_>,
    settings: &GraphProcessingSettings,
    job_id: &str,
    now_ms: i64,
) -> KnowledgeResult<GraphJobClaim> {
    let mut job = load_job(tx, job_id)?;
    if job.status != JOB_QUEUED {
        return Ok(GraphJobClaim::Existing(job));
    }
    if !ready_to_run(settings, &job) {
        return Ok(GraphJobClaim::Existing(job));
    }
    if world_processing_busy(tx, &job.world_record_id, &job.job_id)? {
        return Ok(GraphJobClaim::Existing(job));
    }
    if let Some(previous_key) = &job.document.previous_episode_key {
        let previous = tx.query_row(
            "SELECT job_id, loupist_id, world_record_id, world_revision_id, status, payload_json,
                    payload_digest, created_at_ms
             FROM knowledge_graph_outbox WHERE loupist_id = ?1
               AND json_extract(payload_json, '$.episodeKey') = ?2",
            params![job.loupist_id, previous_key], row_to_job,
        ).optional()?;
        if let Some(mut previous) = previous {
            if !final_status(&previous.status) {
                return Ok(GraphJobClaim::Existing(job));
            }
            let remaining = previous.document.extraction.max_llm_calls;
            // Old queued documents keep their positive per-chunk allowances.
            // New followers start at zero; an old zero-funded tail can only
            // follow a shard with fewer than two calls, which cannot run.
            if job.document.extraction.max_llm_calls == 0
                && remaining >= 2
                && !job.document.mutation_sent
                && !job.document.inference_attempted
                && !job.document.usage_uncertain
                && job.document.attempt_id.is_none()
                && job.document.lease_expires_at_ms.is_none()
                && job.document.interrupted_attempts == 0
                && job.document.host_interruptions == 0
                && job.document.unsent_attempts == 0
            {
                job.document.extraction.max_llm_calls = job
                    .document
                    .extraction
                    .max_llm_calls
                    .saturating_add(remaining);
                previous.document.extraction.max_llm_calls = 0;
                write_status(
                    tx,
                    &previous.job_id,
                    &previous.status,
                    &previous.status,
                    &previous.document,
                )?;
                write_status(tx, job_id, JOB_QUEUED, JOB_QUEUED, &job.document)?;
            }
        }
    }
    // Even the minimum extraction needs entities and relationships. Park the
    // exhausted tail together so method learning can settle its Source.
    if job.document.episode_kind == EPISODE_SPAN
        && (job.document.extraction.max_llm_calls < 2
            || job.document.extraction.max_embedding_calls == 0)
    {
        if !job.document.mutation_sent
            && !job.document.inference_attempted
            && !job.document.usage_uncertain
            && job.document.attempt_id.is_none()
            && job.document.lease_expires_at_ms.is_none()
        {
            write_status(tx, job_id, JOB_QUEUED, JOB_PAUSED, &job.document)?;
            pause_unfunded_followers(tx, &job)?;
            source_job_settled(tx, &job, now_ms)?;
        }
        return load_job(tx, job_id).map(GraphJobClaim::Existing);
    }
    if job.document.episode_kind == EPISODE_SPAN
        && (settings
            .quarantined_generations
            .contains(&job.document.generation_id)
            || generation_needs_reconcile(
                tx,
                &job.loupist_id,
                &job.world_record_id,
                &job.document.generation_id,
            )?)
    {
        return Ok(GraphJobClaim::Existing(job));
    }
    // Several sources can be committed before the first graph job finishes.
    // Bind an unattempted extraction to the latest confirmed snapshot only at
    // dispatch, preserving the accumulated graph from preceding source jobs.
    if job.document.episode_kind == EPISODE_SPAN
        && !job.document.mutation_sent
        && !job.document.inference_attempted
        && !job.document.usage_uncertain
    {
        if let Ok(world) =
            super::authority::require_record(tx, &job.loupist_id, &job.world_record_id)
        {
            if let Some(head) = world.head_revision_id.clone() {
                if head != job.world_revision_id
                    && super::authority::require_edit(&world, &job.loupist_id).is_ok()
                {
                    job.world_revision_id = head.clone();
                    job.document.sealed_revision_id = Some(head.clone());
                    tx.execute("UPDATE knowledge_graph_outbox SET world_revision_id = ?1, payload_json = ?2 WHERE job_id = ?3 AND status = ?4",
                        params![head, serde_json::to_string(&job.document)?, job_id, JOB_QUEUED])?;
                }
            }
        }
    }
    let writable = require_world_write(
        tx,
        &job.loupist_id,
        &job.world_record_id,
        &job.world_revision_id,
    )
    .is_ok();
    let readable_snapshot = job.document.episode_kind == EPISODE_SEALED
        && require_world_read(
            tx,
            &job.loupist_id,
            &job.world_record_id,
            &job.world_revision_id,
        )
        .is_ok();
    if !writable && !readable_snapshot {
        let mut document = job.document.clone();
        clear_lease(&mut document);
        if write_status(tx, job_id, JOB_QUEUED, JOB_FAILED_BEFORE, &document)? {
            source_job_settled(tx, &job, now_ms)?;
        }
        return load_job(tx, job_id).map(GraphJobClaim::Existing);
    }
    let mut document = job.document.clone();
    if document.episode_kind == EPISODE_SPAN
        && !document.inference_attempted
        && !document.mutation_sent
    {
        // The request limit is not an allowance: it follows the saved setting,
        // as the model does. The call count stays as frozen.
        let selected = budget(settings);
        document.extraction.model_profile = selected.model_profile;
        document.extraction.thinking_level = selected.thinking_level;
        document.extraction.run_options = selected.run_options;
        document.extraction.timeout_ms = selected.timeout_ms;
    }
    document.attempt_id = Some(mint_id("atm"));
    document.lease_expires_at_ms =
        Some(now_ms.saturating_add(attempt_budget_ms(document.extraction.timeout_ms) as i64));
    if !write_status(tx, job_id, JOB_QUEUED, JOB_RUNNING, &document)? {
        return load_job(tx, job_id).map(GraphJobClaim::Existing);
    }
    load_job(tx, job_id).map(GraphJobClaim::Acquired)
}

pub fn note_inference_attempted(
    tx: &Transaction<'_>,
    job_id: &str,
    attempt_id: &str,
) -> KnowledgeResult<()> {
    let job = active_attempt(tx, job_id, attempt_id)?;
    let mut document = job.document;
    document.inference_attempted = true;
    if !write_status(tx, job_id, JOB_RUNNING, JOB_RUNNING, &document)? {
        return Err(KnowledgeError::Invalid(
            "graph job attempt is no longer active",
        ));
    }
    Ok(())
}

/// Reserve a model call durably before dispatch. Refused calls return it; a
/// crash or uncertain reply keeps it spent, so retries cannot refill the pool.
pub(super) fn reserve_model_call(
    tx: &Transaction<'_>,
    job_id: &str,
    attempt_id: &str,
) -> KnowledgeResult<()> {
    let job = active_attempt(tx, job_id, attempt_id)?;
    let mut document = job.document;
    document.extraction.max_llm_calls =
        document
            .extraction
            .max_llm_calls
            .checked_sub(1)
            .ok_or(KnowledgeError::Invalid(
                "World model call budget is exhausted",
            ))?;
    document.inference_attempted = true;
    write_status(tx, job_id, JOB_RUNNING, JOB_RUNNING, &document)?;
    Ok(())
}

pub(super) fn refund_model_call(
    tx: &Transaction<'_>,
    job_id: &str,
    attempt_id: &str,
) -> KnowledgeResult<()> {
    let job = active_attempt(tx, job_id, attempt_id)?;
    let mut document = job.document;
    document.extraction.max_llm_calls = document.extraction.max_llm_calls.saturating_add(1);
    write_status(tx, job_id, JOB_RUNNING, JOB_RUNNING, &document)?;
    Ok(())
}

/// A Source extraction job reached a final status: method learning may now
/// judge the pages citing its Source ([`super::learning::world_job_settled`]).
fn source_job_settled(tx: &Transaction<'_>, job: &GraphJobView, at_ms: i64) -> KnowledgeResult<()> {
    if job.document.episode_kind == EPISODE_SPAN {
        super::learning::world_job_settled(tx, &job.loupist_id, at_ms)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn finish_job(
    tx: &Transaction<'_>,
    settings: &mut GraphProcessingSettings,
    job_id: &str,
    attempt_id: &str,
    outcome: &str,
    usage_uncertain: bool,
    validated: bool,
    now_ms: i64,
) -> KnowledgeResult<GraphJobView> {
    let job = active_attempt(tx, job_id, attempt_id)?;
    if super::authority::require_record(tx, &job.loupist_id, &job.world_record_id).is_err() {
        let mut document = job.document.clone();
        clear_lease(&mut document);
        write_status(tx, job_id, JOB_RUNNING, JOB_CANCELLED, &document)?;
        return load_job(tx, job_id);
    }
    let mut document = job.document.clone();
    document.usage_uncertain = document.usage_uncertain || usage_uncertain;
    clear_lease(&mut document);
    let next =
        if document.mutation_sent && (usage_uncertain || outcome == JOB_RECONCILE || !validated) {
            quarantine(settings, &document.generation_id);
            JOB_RECONCILE
        } else if outcome == JOB_FAILED_BEFORE
            && !document.mutation_sent
            && !document.inference_attempted
        {
            JOB_FAILED_BEFORE
        } else if outcome == JOB_COMPLETED && validated && !usage_uncertain {
            JOB_COMPLETED
        } else if retry_once(&mut document) {
            requeued(settings, &document)
        } else if document.mutation_sent || document.inference_attempted || usage_uncertain {
            quarantine(settings, &document.generation_id);
            JOB_RECONCILE
        } else {
            JOB_FAILED_BEFORE
        };
    if !write_status(tx, job_id, JOB_RUNNING, next, &document)? {
        return Err(KnowledgeError::Invalid(
            "graph job attempt is no longer active",
        ));
    }
    if final_status(next) {
        source_job_settled(tx, &job, now_ms)?;
    }
    load_job(tx, job_id)
}

/// How an attempt that published nothing ended, as the host observed it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Unpublished {
    /// A sent call's outcome or usage could not be confirmed.
    pub uncertain: bool,
    /// A paid call was sent (completed or uncertain). Refused calls do not count.
    pub paid: bool,
    /// The Mac slept while the attempt ran.
    pub suspended: bool,
}

/// End an attempt that published nothing. One that never sent a paid call costs
/// nothing and runs again, up to [`UNSENT_RETRIES`] times, without touching the
/// retry rule; after that it fails without quarantining the World. One the Mac's
/// sleep cut short runs again within its host allowance. Anything else follows
/// [`finish_job`].
pub fn finish_unpublished(
    tx: &Transaction<'_>,
    settings: &mut GraphProcessingSettings,
    job_id: &str,
    attempt_id: &str,
    end: Unpublished,
    now_ms: i64,
) -> KnowledgeResult<GraphJobView> {
    let job = active_attempt(tx, job_id, attempt_id)?;
    let free = !end.paid && !end.uncertain && !job.document.mutation_sent;
    if job.document.episode_kind == EPISODE_SPAN && (free || end.suspended) {
        let mut document = job.document.clone();
        let next = if free {
            document.inference_attempted = false;
            if document.unsent_attempts < UNSENT_RETRIES {
                document.unsent_attempts += 1;
                Some(requeued(settings, &document))
            } else {
                Some(JOB_FAILED_BEFORE)
            }
        } else if host_interrupted(&mut document) {
            Some(requeued(settings, &document))
        } else {
            None
        };
        if let Some(next) = next {
            clear_lease(&mut document);
            if !write_status(tx, job_id, JOB_RUNNING, next, &document)? {
                return Err(KnowledgeError::Invalid(
                    "graph job attempt is no longer active",
                ));
            }
            if final_status(next) {
                source_job_settled(tx, &job, now_ms)?;
            }
            return load_job(tx, job_id);
        }
    }
    finish_job(
        tx,
        settings,
        job_id,
        attempt_id,
        JOB_FAILED_BEFORE,
        end.uncertain,
        false,
        now_ms,
    )
}

/// Followers without their own allowance cannot run after an exhausted chunk.
fn pause_unfunded_followers(tx: &Transaction<'_>, job: &GraphJobView) -> KnowledgeResult<()> {
    tx.execute(
        "WITH RECURSIVE tail(episode_key) AS (
           SELECT ?1 UNION
           SELECT json_extract(payload_json, '$.episodeKey')
           FROM knowledge_graph_outbox JOIN tail
             ON json_extract(payload_json, '$.previousEpisodeKey') = tail.episode_key
           WHERE loupist_id = ?2
         )
         UPDATE knowledge_graph_outbox SET status = ?3
         WHERE loupist_id = ?2 AND status = ?4
           AND json_extract(payload_json, '$.episodeKey') IN (SELECT episode_key FROM tail)
           AND (json_extract(payload_json, '$.extraction.maxLlmCalls') < 2
             OR json_extract(payload_json, '$.extraction.maxEmbeddingCalls') = 0)
           AND json_extract(payload_json, '$.mutationSent') = 0
           AND json_extract(payload_json, '$.inferenceAttempted') = 0
           AND json_extract(payload_json, '$.usageUncertain') = 0
           AND json_extract(payload_json, '$.attemptId') IS NULL
           AND json_extract(payload_json, '$.leaseExpiresAtMs') IS NULL",
        params![
            job.document.episode_key,
            job.loupist_id,
            JOB_PAUSED,
            JOB_QUEUED
        ],
    )?;
    Ok(())
}

/// The frozen allowance cannot finish this episode. Repeating the paid calls
/// cannot increase it, and no publication took place, so leave the episode
/// paused without quarantining the rest of the World.
pub(super) fn pause_exhausted_attempt(
    tx: &Transaction<'_>,
    job_id: &str,
    attempt_id: &str,
    now_ms: i64,
) -> KnowledgeResult<GraphJobView> {
    let job = active_attempt(tx, job_id, attempt_id)?;
    let mut document = job.document.clone();
    clear_lease(&mut document);
    write_status(tx, job_id, JOB_RUNNING, JOB_PAUSED, &document)?;
    pause_unfunded_followers(tx, &job)?;
    source_job_settled(tx, &job, now_ms)?;
    load_job(tx, job_id)
}

pub(super) fn ready_to_run(settings: &GraphProcessingSettings, job: &GraphJobView) -> bool {
    if settings.paused && job.document.episode_kind != EPISODE_RECOVERY {
        return false;
    }
    if settings
        .inactive_loupists
        .iter()
        .any(|loupist| loupist == &job.loupist_id)
    {
        return false;
    }
    if matches!(
        job.document.episode_kind.as_str(),
        EPISODE_SEALED | EPISODE_RECOVERY
    ) {
        return true;
    }
    if !settings.enrichment_enabled {
        return false;
    }
    let provider = settings.helper_provider.as_deref().unwrap_or("");
    let model = settings.helper_model.as_deref().unwrap_or("");
    !provider.is_empty() && !model.is_empty()
}

fn active_attempt(
    tx: &Transaction<'_>,
    job_id: &str,
    attempt_id: &str,
) -> KnowledgeResult<GraphJobView> {
    let job = load_job(tx, job_id)?;
    if job.status != JOB_RUNNING || job.document.attempt_id.as_deref() != Some(attempt_id) {
        return Err(KnowledgeError::Invalid(
            "graph job attempt is no longer active",
        ));
    }
    Ok(job)
}

fn require_world_read(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    revision_id: &str,
) -> KnowledgeResult<()> {
    let module = super::authority::require_record(tx, loupist_id, record_id)?;
    if module.kind != super::model::RecordKind::World {
        return Err(KnowledgeError::Invalid("graph jobs require a World record"));
    }
    let revision = super::revision::load_revision(tx, revision_id)?;
    super::authority::revision_in_record(&revision.record_id, &module.record_id)
}

fn require_world_write(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    expected_revision_id: &str,
) -> KnowledgeResult<()> {
    let module = super::authority::require_record(tx, loupist_id, record_id)?;
    if module.kind != super::model::RecordKind::World {
        return Err(KnowledgeError::Invalid("graph jobs require a World record"));
    }
    super::authority::require_edit(&module, loupist_id)?;
    if module.head_revision_id.as_deref() != Some(expected_revision_id) {
        return Err(KnowledgeError::RevisionConflict {
            expected: Some(expected_revision_id.to_string()),
            actual: module.head_revision_id,
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphPublicSettings {
    pub enrichment_enabled: bool,
    pub paused: bool,
    pub helper_model_profile_id: Option<String>,
    pub max_llm_calls: u32,
    pub timeout_ms: u64,
    pub thinking_level: String,
    pub run_options: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub research_focus: String,
    #[serde(default)]
    pub relation_hints: Vec<String>,
    #[serde(default)]
    pub anchors: Vec<WorldAnchor>,
    #[serde(default)]
    pub fast_model: Option<String>,
}

/// The sentence under the World processing status, by code: the interface
/// renders it in its own language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ReadinessNote {
    RecoveryRequired,
    Recovering,
    RuntimeUnavailable,
    AwaitingProvider,
    ChooseModel,
    ModelUnavailable,
    ConnectProvider,
    #[serde(untagged)]
    Provider(crate::providers::inference::UnreadyReason),
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct GraphReadiness {
    pub status: String,
    pub note: Option<ReadinessNote>,
    pub provider_name: Option<String>,
    pub model_name: Option<String>,
    pub auth_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct GraphProcessingView {
    pub settings: GraphPublicSettings,
    pub readiness: GraphReadiness,
}

impl GraphProcessingSettings {
    pub fn public_settings(&self) -> GraphPublicSettings {
        GraphPublicSettings {
            enrichment_enabled: self.enrichment_enabled,
            paused: self.paused,
            helper_model_profile_id: self.helper_model_profile_id.clone(),
            max_llm_calls: self.max_llm_calls,
            timeout_ms: self.timeout_ms,
            thinking_level: self.thinking_level.clone(),
            run_options: self.run_options.clone(),
            research_focus: self.research_focus.clone(),
            relation_hints: self.relation_hints.clone(),
            anchors: self.anchors.clone(),
            fast_model: self.fast_model.clone(),
        }
    }

    pub fn validate(&self) -> KnowledgeResult<()> {
        if self.max_llm_calls == 0
            || self.max_llm_calls > 64
            || self.max_embedding_calls == 0
            || self.max_embedding_calls > 256
            || self.timeout_ms < 1_000
            || self.timeout_ms > MAX_TIMEOUT_MS
            || self.embedding_profile != "local-en-128"
        {
            return Err(KnowledgeError::Invalid(
                "World processing bounds are invalid",
            ));
        }
        if self.helper_provider.is_some() != self.helper_model.is_some() {
            return Err(KnowledgeError::Invalid("World model is incomplete"));
        }
        if self.thinking_level.len() > 32
            || self.run_options.len() > 32
            || self
                .run_options
                .iter()
                .any(|(key, value)| key.len() > 128 || value.len() > 256)
        {
            return Err(KnowledgeError::Invalid("World model controls are invalid"));
        }
        validate_text(
            &self.research_focus,
            1_000,
            true,
            "research focus is invalid",
        )?;
        if self.relation_hints.len() > 32
            || self
                .relation_hints
                .iter()
                .any(|hint| hint.is_empty() || hint.len() > 64)
            || self.anchors.len() > 200
            || self.anchors.iter().any(|anchor| {
                anchor.name.is_empty()
                    || anchor.name.len() > 200
                    || anchor.aliases.len() > 16
                    || anchor.identifiers.len() > 16
                    || anchor
                        .aliases
                        .iter()
                        .chain(anchor.identifiers.iter())
                        .any(|item| item.is_empty() || item.len() > 200)
            })
        {
            return Err(KnowledgeError::Invalid("World seed is invalid"));
        }
        if self
            .fast_model
            .as_deref()
            .is_some_and(|model| model.is_empty() || model.len() > 200)
        {
            return Err(KnowledgeError::Invalid("fast model is invalid"));
        }
        super::world_schema::validate_extension(&self.extension)?;
        Ok(())
    }

    pub fn apply_public(
        &mut self,
        incoming: &GraphPublicSettings,
        model: Option<&crate::providers::catalog::ConfiguredModel>,
    ) {
        self.awaiting_default_model = false;
        self.enrichment_enabled = incoming.enrichment_enabled;
        self.paused = incoming.paused;
        self.helper_model_profile_id = incoming.helper_model_profile_id.clone();
        self.helper_provider = model.map(|model| model.provider.clone());
        self.helper_model = model.map(|model| model.model.clone());
        self.max_llm_calls = incoming.max_llm_calls;
        self.timeout_ms = incoming.timeout_ms;
        self.thinking_level = incoming.thinking_level.clone();
        self.run_options = incoming.run_options.clone();
        self.research_focus = incoming.research_focus.clone();
        self.relation_hints = incoming.relation_hints.clone();
        self.anchors = incoming.anchors.clone();
        self.fast_model = incoming.fast_model.clone();
    }
}

fn budget(settings: &GraphProcessingSettings) -> ExtractionBudget {
    ExtractionBudget {
        model_profile: format!(
            "{}/{}",
            settings.helper_provider.as_deref().unwrap_or("none"),
            settings.helper_model.as_deref().unwrap_or("none")
        ),
        thinking_level: settings.thinking_level.clone(),
        run_options: settings.run_options.clone(),
        embedding_profile: settings.embedding_profile.clone(),
        max_llm_calls: settings.max_llm_calls,
        max_embedding_calls: settings.max_embedding_calls,
        timeout_ms: settings.timeout_ms,
    }
}

fn ensure_active(settings: &GraphProcessingSettings, loupist_id: &str) -> KnowledgeResult<()> {
    if settings.inactive_loupists.iter().any(|id| id == loupist_id) {
        Err(KnowledgeError::Unauthorized("loupist is not active"))
    } else {
        Ok(())
    }
}

/// A Source's paid attempt that published nothing runs once more: the World is
/// unchanged, and the Source's cost stays at two calls at most. Returns false,
/// leaving the document as it was, for any other attempt.
fn retry_once(document: &mut JobDocument) -> bool {
    let paid = document.inference_attempted || document.usage_uncertain;
    if document.episode_kind != EPISODE_SPAN
        || document.mutation_sent
        || !paid
        || document.interrupted_attempts > 0
    {
        return false;
    }
    document.interrupted_attempts += 1;
    document.inference_attempted = false;
    document.usage_uncertain = false;
    true
}

/// A Source's paid attempt the host cut short runs again without spending its
/// one retry, at most [`HOST_INTERRUPTION_RETRIES`] times. Returns false,
/// leaving the document as it was, for any other attempt.
fn host_interrupted(document: &mut JobDocument) -> bool {
    let paid = document.inference_attempted || document.usage_uncertain;
    if document.episode_kind != EPISODE_SPAN
        || document.mutation_sent
        || !paid
        || document.host_interruptions >= HOST_INTERRUPTION_RETRIES
    {
        return false;
    }
    document.host_interruptions += 1;
    document.inference_attempted = false;
    document.usage_uncertain = false;
    true
}

fn requeued(settings: &GraphProcessingSettings, document: &JobDocument) -> &'static str {
    if settings.paused && document.episode_kind != EPISODE_RECOVERY {
        JOB_PAUSED
    } else {
        JOB_QUEUED
    }
}

fn quarantine(settings: &mut GraphProcessingSettings, generation_id: &str) {
    if !settings
        .quarantined_generations
        .iter()
        .any(|id| id == generation_id)
    {
        settings
            .quarantined_generations
            .push(generation_id.to_string());
    }
}

fn clear_lease(document: &mut JobDocument) {
    document.lease_expires_at_ms = None;
    document.attempt_id = None;
}

pub(super) const EPISODE_CHUNK_CHARS: usize = 7_000;

/// Split a long source at paragraph boundaries so one Graphiti episode stays
/// inside the extraction budget. A short source stays one episode, which keeps
/// its existing dedup key.
fn chunk_episode(body: &str) -> Vec<String> {
    if body.chars().count() <= EPISODE_CHUNK_CHARS {
        return vec![body.to_string()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_chars = 0;
    for paragraph in body.split_inclusive("\n\n") {
        let paragraph_chars = paragraph.chars().count();
        if paragraph_chars > EPISODE_CHUNK_CHARS {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            let mut piece = String::new();
            let mut piece_chars = 0;
            for ch in paragraph.chars() {
                piece.push(ch);
                piece_chars += 1;
                if piece_chars >= EPISODE_CHUNK_CHARS {
                    chunks.push(std::mem::take(&mut piece));
                    piece_chars = 0;
                }
            }
            current = piece;
            current_chars = piece_chars;
            continue;
        }
        if !current.is_empty() && current_chars + paragraph_chars > EPISODE_CHUNK_CHARS {
            chunks.push(std::mem::take(&mut current));
            current_chars = 0;
        }
        current.push_str(paragraph);
        current_chars += paragraph_chars;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn default_source_label() -> String {
    "retained source".to_string()
}

fn reference_time(at_ms: i64) -> String {
    let seconds = at_ms.div_euclid(1000);
    let millis = at_ms.rem_euclid(1000) as u32;
    format!(
        "{}",
        chrono::DateTime::from_timestamp(seconds, millis * 1_000_000)
            .unwrap_or(chrono::DateTime::UNIX_EPOCH)
            .format("%Y-%m-%dT%H:%M:%S+00:00")
    )
}

/// Graphiti interprets relative dates from `reference_time`. Prefer the source
/// publication instant. A year or month starts on its first day; the original
/// precision stays on `publication_time`.
pub(super) fn reference_time_for(publication: Option<&TimePoint>, at_ms: i64) -> String {
    publication
        .and_then(publication_reference)
        .unwrap_or_else(|| reference_time(at_ms))
}

fn publication_reference(time: &TimePoint) -> Option<String> {
    let value = time
        .value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let stamped = match time.precision {
        super::model::TimePrecision::Instant => value.to_string(),
        super::model::TimePrecision::Date => format!("{value}T00:00:00+00:00"),
        super::model::TimePrecision::Month => format!("{value}-01T00:00:00+00:00"),
        super::model::TimePrecision::Year => format!("{value}-01-01T00:00:00+00:00"),
        super::model::TimePrecision::Unknown => return None,
    };
    Some(stamped)
}

/// Episode name and source description from the retained Source, not the span id.
pub(super) fn episode_context(
    title: &str,
    source_class: &str,
    attribution: &str,
    publication: Option<&TimePoint>,
) -> (String, String) {
    let title = title.trim();
    let class = source_class.replace('_', " ");
    let attribution = attribution.trim();
    let when = publication
        .and_then(|time| time.value.as_deref())
        .unwrap_or("")
        .trim();
    let mut parts = Vec::new();
    if !title.is_empty() {
        parts.push(title.to_string());
    }
    if !class.is_empty() {
        parts.push(class);
    }
    if !attribution.is_empty() && attribution != title {
        parts.push(attribution.to_string());
    }
    if !when.is_empty() {
        parts.push(when.to_string());
    }
    let label = clip(&parts.join(" · "), 200);
    let label = if label.is_empty() {
        default_source_label()
    } else {
        label
    };
    let name = clip(if title.is_empty() { attribution } else { title }, 180);
    let name = if name.is_empty() { label.clone() } else { name };
    (name, label)
}

fn dedup_digest(value: &Value) -> KnowledgeResult<String> {
    digest_json(value)
}

fn generation_for(
    tx: &Transaction<'_>,
    loupist_id: &str,
    world_record_id: &str,
) -> KnowledgeResult<String> {
    let recovered: Option<String> = tx.query_row(
        "SELECT json_extract(payload_json, '$.generationId') FROM knowledge_graph_outbox WHERE loupist_id = ?1 AND world_record_id = ?2 AND status = 'completed' AND json_extract(payload_json, '$.episodeKind') = 'world_recovery' ORDER BY rowid DESC LIMIT 1",
        params![loupist_id, world_record_id], |row| row.get(0),
    ).optional()?;
    if let Some(generation) = recovered {
        return Ok(generation);
    }
    let generation: Option<String> = tx.query_row(
        "SELECT json_extract(payload_json, '$.generationId') FROM knowledge_graph_outbox WHERE loupist_id = ?1 AND world_record_id = ?2 ORDER BY created_at_ms, job_id LIMIT 1",
        params![loupist_id, world_record_id], |row| row.get(0),
    ).optional()?;
    Ok(generation.unwrap_or_else(|| mint_id("gen")))
}

fn find_dedup(
    tx: &Transaction<'_>,
    loupist_id: &str,
    world_record_id: &str,
    dedup_key: &str,
) -> KnowledgeResult<Option<GraphJobView>> {
    let mut statement = tx.prepare(
        "SELECT job_id, loupist_id, world_record_id, world_revision_id, status, payload_json,
                payload_digest, created_at_ms
         FROM knowledge_graph_outbox
         WHERE loupist_id = ?1 AND world_record_id = ?2 AND payload_digest = ?3
         ORDER BY created_at_ms DESC LIMIT 1",
    )?;
    let job = statement
        .query_row(params![loupist_id, world_record_id, dedup_key], row_to_job)
        .optional()?;
    Ok(job.filter(|job| {
        matches!(
            job.status.as_str(),
            JOB_QUEUED | JOB_RUNNING | JOB_PAUSED | JOB_COMPLETED | JOB_RECONCILE | JOB_RECONCILED
        )
    }))
}

fn generation_needs_reconcile(
    tx: &Transaction<'_>,
    loupist_id: &str,
    world_record_id: &str,
    _generation_id: &str,
) -> KnowledgeResult<bool> {
    Ok(tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_graph_outbox WHERE loupist_id = ?1 AND world_record_id = ?2 AND status = ?3)", params![loupist_id, world_record_id, JOB_RECONCILE], |row| row.get(0))?)
}

fn world_processing_busy(
    tx: &Transaction<'_>,
    world_record_id: &str,
    except_job: &str,
) -> KnowledgeResult<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM knowledge_graph_outbox WHERE world_record_id = ?1 AND job_id != ?2 AND status = ?3)",
        params![world_record_id, except_job, JOB_RUNNING], |row| row.get(0),
    )?)
}

fn insert_job(
    tx: &Transaction<'_>,
    loupist_id: &str,
    world_record_id: &str,
    world_revision_id: &str,
    dedup_key: &str,
    at_ms: i64,
    document: &JobDocument,
) -> KnowledgeResult<GraphJobView> {
    let job_id = mint_id("job");
    let payload = serde_json::to_string(document)?;
    tx.execute(
        "INSERT INTO knowledge_graph_outbox (
            job_id, loupist_id, world_record_id, world_revision_id, status, payload_json,
            payload_digest, created_at_ms
         ) VALUES (?1, ?2, ?3, ?4, 'queued', ?5, ?6, ?7)",
        params![
            job_id,
            loupist_id,
            world_record_id,
            world_revision_id,
            payload,
            dedup_key,
            at_ms
        ],
    )?;
    load_job(tx, &job_id)
}

fn write_status(
    tx: &Transaction<'_>,
    job_id: &str,
    expected: &str,
    status: &str,
    document: &JobDocument,
) -> KnowledgeResult<bool> {
    let payload = serde_json::to_string(document)?;
    let changed = tx.execute(
        "UPDATE knowledge_graph_outbox SET status = ?1, payload_json = ?2
         WHERE job_id = ?3 AND status = ?4",
        params![status, payload, job_id, expected],
    )?;
    Ok(changed == 1)
}

fn list_jobs_with_status(
    tx: &Transaction<'_>,
    loupist_id: &str,
    status: &str,
) -> KnowledgeResult<Vec<GraphJobView>> {
    let mut statement = tx.prepare(
        "SELECT job_id, loupist_id, world_record_id, world_revision_id, status, payload_json,
                payload_digest, created_at_ms
         FROM knowledge_graph_outbox WHERE loupist_id = ?1 AND status = ?2
         ORDER BY created_at_ms, job_id",
    )?;
    let rows = statement.query_map(params![loupist_id, status], row_to_job)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(KnowledgeError::from)
}

fn list_running(tx: &Transaction<'_>) -> KnowledgeResult<Vec<GraphJobView>> {
    let mut statement = tx.prepare(
        "SELECT job_id, loupist_id, world_record_id, world_revision_id, status, payload_json,
                payload_digest, created_at_ms
         FROM knowledge_graph_outbox WHERE status = ?1 ORDER BY created_at_ms, job_id",
    )?;
    let rows = statement.query_map([JOB_RUNNING], row_to_job)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(KnowledgeError::from)
}

fn load_job(tx: &Transaction<'_>, job_id: &str) -> KnowledgeResult<GraphJobView> {
    tx.query_row(
        "SELECT job_id, loupist_id, world_record_id, world_revision_id, status, payload_json,
                payload_digest, created_at_ms
         FROM knowledge_graph_outbox WHERE job_id = ?1",
        params![job_id],
        row_to_job,
    )
    .optional()?
    .ok_or(KnowledgeError::NotFound)
}

fn row_to_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<GraphJobView> {
    let payload: String = row.get(5)?;
    let document: JobDocument = serde_json::from_str(&payload).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let mut job = GraphJobView {
        job_id: row.get(0)?,
        loupist_id: row.get(1)?,
        world_record_id: row.get(2)?,
        world_revision_id: row.get(3)?,
        status: row.get(4)?,
        payload_digest: row.get(6)?,
        created_at_ms: row.get(7)?,
        host_may_redrive: false,
        document,
    };
    job.host_may_redrive = host_may_redrive(&job);
    Ok(job)
}

pub fn host_may_redrive(job: &GraphJobView) -> bool {
    matches!(job.status.as_str(), JOB_FAILED_BEFORE | JOB_CANCELLED)
        && !job.document.mutation_sent
        && !job.document.inference_attempted
        && !job.document.usage_uncertain
}

// Publication binds attempt identity, exact artifact bytes and completion in one transaction.
#[allow(clippy::too_many_arguments)]
pub fn publish_and_complete(
    write: &super::KnowledgeWrite<'_>,
    settings: &mut GraphProcessingSettings,
    job_id: &str,
    attempt_id: &str,
    artifact_utf8: &str,
    logical_digest: &str,
    at_ms: i64,
    usage_uncertain: bool,
    followed: &[crate::store::user_context::MapContextItem],
) -> KnowledgeResult<GraphJobView> {
    use super::model::{SealWorld, WorldSnapshotBody};
    if sha256(artifact_utf8.as_bytes()) != logical_digest {
        return Err(KnowledgeError::DigestMismatch);
    }
    let job = active_attempt(write.tx, job_id, attempt_id)?;
    if job.document.episode_kind != EPISODE_SPAN {
        return Err(KnowledgeError::Invalid(
            "publication requires source extraction",
        ));
    }
    let admitted = require_world_write(
        write.tx,
        &job.loupist_id,
        &job.world_record_id,
        &job.world_revision_id,
    );
    if admitted.is_err() {
        return settle_without_head(
            write.tx,
            settings,
            &job,
            artifact_utf8,
            logical_digest,
            usage_uncertain,
            at_ms,
        );
    }
    if usage_uncertain {
        return settle_without_head(
            write.tx,
            settings,
            &job,
            artifact_utf8,
            logical_digest,
            true,
            at_ms,
        );
    }
    let current = super::revision::load_revision(write.tx, &job.world_revision_id)?;
    let previous: WorldSnapshotBody = serde_json::from_str(&current.payload_json)?;
    let (nodes, claims, mut annotations) = project_worker_artifact(artifact_utf8)?;
    carry_forward_annotations(&previous, &mut annotations);
    let mut spans = evidence_spans(&claims, &annotations);
    if let Some(binding) = &job.document.source_binding {
        spans.push(binding.span_id.clone());
    }
    let previous_dependencies: Vec<super::model::EvidenceRevision> =
        serde_json::from_str(&current.dependency_json)?;
    let dependencies = source_closure(
        write.tx,
        &job.loupist_id,
        &job,
        &previous_dependencies,
        &spans,
    )?;
    let sealed = super::world::seal_world_with_actor(
        write.tx,
        SealWorld {
            loupist_id: job.loupist_id.clone(),
            record_id: job.world_record_id.clone(),
            expected_revision_id: Some(job.world_revision_id.clone()),
            at_ms,
            nodes,
            claims,
            annotations,
            dependencies,
        },
        "processing",
        job.document.origin_chat_id.as_deref(),
        job.document.origin_message_id.as_deref(),
        followed,
    )?;
    let mut document = job.document.clone();
    finish_artifact(&mut document, logical_digest);
    document.sealed_revision_id = Some(sealed.revision_id);
    document.usage_uncertain = false;
    clear_lease(&mut document);
    if !write_status(write.tx, job_id, JOB_RUNNING, JOB_COMPLETED, &document)? {
        return Err(KnowledgeError::Invalid(
            "graph job attempt is no longer active",
        ));
    }
    // The seal rechecked learning while this job still ran; now its own
    // Source is processed.
    source_job_settled(write.tx, &job, at_ms)?;
    load_job(write.tx, job_id)
}

pub(super) fn complete_reindex(
    write: &super::KnowledgeWrite<'_>,
    settings: &mut GraphProcessingSettings,
    job: &GraphJobView,
    attempt_id: &str,
    at_ms: i64,
) -> KnowledgeResult<GraphJobView> {
    let job = active_attempt(write.tx, &job.job_id, attempt_id)?;
    if job.document.episode_kind == EPISODE_SEALED {
        require_world_read(
            write.tx,
            &job.loupist_id,
            &job.world_record_id,
            &job.world_revision_id,
        )?;
    } else if job.document.episode_kind == EPISODE_RECOVERY {
        require_world_write(
            write.tx,
            &job.loupist_id,
            &job.world_record_id,
            &job.world_revision_id,
        )?;
    } else {
        return Err(KnowledgeError::Invalid(
            "local reindex requires a sealed World",
        ));
    }
    let digest = super::revision::load_revision(write.tx, &job.world_revision_id)?.revision_digest;
    let completed = finish_indexed_head(write, &job, &digest)?;
    if job.document.episode_kind == EPISODE_SEALED {
        return Ok(completed);
    }
    let mut current = load_settings(write.tx, &job.loupist_id)?;
    let mut retired_generations = Vec::new();
    for previous in list_jobs(write.tx, &job.loupist_id)? {
        if previous.world_record_id != job.world_record_id || previous.job_id == job.job_id {
            continue;
        }
        if previous.status == JOB_RECONCILE {
            // Keep the original usage/mutation uncertainty visible in history.
            // This terminal state means the derived graph was replaced, not
            // that the interrupted provider call was refunded or retried.
            let mut document = previous.document.clone();
            clear_lease(&mut document);
            write_status(
                write.tx,
                &previous.job_id,
                JOB_RECONCILE,
                JOB_RECONCILED,
                &document,
            )?;
            retired_generations.push(document.generation_id);
        } else if previous.document.episode_kind == EPISODE_SPAN
            && matches!(previous.status.as_str(), JOB_QUEUED | JOB_PAUSED)
            && !previous.document.mutation_sent
            && !previous.document.inference_attempted
            && !previous.document.usage_uncertain
        {
            let mut document = previous.document.clone();
            document.generation_id = job.document.generation_id.clone();
            document.sealed_revision_id = Some(job.world_revision_id.clone());
            clear_lease(&mut document);
            let status = if current.paused {
                JOB_PAUSED
            } else {
                JOB_QUEUED
            };
            write_status(
                write.tx,
                &previous.job_id,
                &previous.status,
                status,
                &document,
            )?;
            write.tx.execute(
                "UPDATE knowledge_graph_outbox SET world_revision_id = ?1 WHERE job_id = ?2",
                params![job.world_revision_id, previous.job_id],
            )?;
        }
    }
    current
        .quarantined_generations
        .retain(|generation| !retired_generations.contains(generation));
    store_settings(write.tx, &job.loupist_id, &current)?;
    settings.quarantined_generations = current.quarantined_generations.clone();
    // Sources saved while quarantined can now enter the queue. Original
    // attempted spans still deduplicate against their preserved history.
    write.enqueue_existing_sources(&job.loupist_id, &current, at_ms)?;
    Ok(completed)
}

fn finish_indexed_head(
    write: &super::KnowledgeWrite<'_>,
    job: &GraphJobView,
    logical_digest: &str,
) -> KnowledgeResult<GraphJobView> {
    let mut document = job.document.clone();
    finish_artifact(&mut document, logical_digest);
    document.sealed_revision_id = Some(job.world_revision_id.clone());
    document.usage_uncertain = false;
    clear_lease(&mut document);
    if !write_status(write.tx, &job.job_id, JOB_RUNNING, JOB_COMPLETED, &document)? {
        return Err(KnowledgeError::Invalid(
            "graph job attempt is no longer active",
        ));
    }
    load_job(write.tx, &job.job_id)
}

fn finish_artifact(document: &mut JobDocument, logical_digest: &str) {
    // The canonical World and retained Source spans rebuild the derived index.
    // A settled job needs only its digest and usage receipt, not another graph copy.
    document.artifact_utf8 = None;
    document.logical_digest = Some(logical_digest.to_string());
}

fn settle_without_head(
    tx: &Transaction<'_>,
    settings: &mut GraphProcessingSettings,
    job: &GraphJobView,
    artifact_utf8: &str,
    logical_digest: &str,
    usage_uncertain: bool,
    at_ms: i64,
) -> KnowledgeResult<GraphJobView> {
    let mut document = job.document.clone();
    document.artifact_utf8 = Some(artifact_utf8.to_string());
    document.logical_digest = Some(logical_digest.to_string());
    document.usage_uncertain = document.usage_uncertain || usage_uncertain;
    clear_lease(&mut document);
    quarantine(settings, &document.generation_id);
    if !write_status(tx, &job.job_id, JOB_RUNNING, JOB_RECONCILE, &document)? {
        return Err(KnowledgeError::Invalid(
            "graph job attempt is no longer active",
        ));
    }
    source_job_settled(tx, job, at_ms)?;
    load_job(tx, &job.job_id)
}

/// Topic notes are written by the host, not the derived index; keep the ones
/// a new projection does not restate.
fn carry_forward_annotations(
    previous: &super::model::WorldSnapshotBody,
    annotations: &mut Vec<super::model::TopicAnnotation>,
) {
    for annotation in &previous.annotations {
        if !annotations
            .iter()
            .any(|item| item.topic_id == annotation.topic_id)
        {
            annotations.push(annotation.clone());
        }
    }
}

fn evidence_spans(
    claims: &[super::model::GraphClaim],
    annotations: &[super::model::TopicAnnotation],
) -> Vec<String> {
    let mut spans = Vec::new();
    for claim in claims {
        for evidence in &claim.evidence {
            spans.push(evidence.span_id.clone());
        }
    }
    for annotation in annotations {
        spans.extend(annotation.source_span_ids.clone());
    }
    spans.sort();
    spans.dedup();
    spans
}

fn share_budget(total: u32, count: usize, index: usize) -> u32 {
    let count = u32::try_from(count).unwrap_or(u32::MAX).max(1);
    let index = u32::try_from(index).unwrap_or(u32::MAX);
    let base = total / count;
    let extra = total % count;
    base + u32::from(index < extra)
}

fn bound_source_catalog(
    tx: &Transaction<'_>,
    loupist_id: &str,
    binding: &StoredSourceBinding,
) -> KnowledgeResult<(String, String, super::model::SourceRecordPayload)> {
    use super::model::{RecordKind, SourceRecordPayload};
    let stored = super::revision::current_source(tx, loupist_id, &binding.source_record_id)?;
    super::authority::revision_in_record(&stored.record_id, &binding.source_record_id)?;
    if stored.kind != RecordKind::Source {
        return Err(KnowledgeError::Invalid("record is not a Source"));
    }
    super::authority::require_record(tx, loupist_id, &binding.source_record_id)?;
    let payload: SourceRecordPayload = serde_json::from_str(&stored.payload_json)?;
    Ok((
        binding.source_record_id.clone(),
        stored.revision_digest,
        payload,
    ))
}

fn source_head_for_span(
    tx: &Transaction<'_>,
    loupist_id: &str,
    span_id: &str,
) -> KnowledgeResult<(String, String)> {
    use super::model::{RecordKind, SourceRecordPayload};
    let span = super::source::require_any_visible_span(tx, loupist_id, span_id)
        .map_err(|_| KnowledgeError::Invalid("source span has no sealed revision"))?;
    let module = super::authority::require_record(tx, loupist_id, &span.record_id)?;
    if module.kind != RecordKind::Source {
        return Err(KnowledgeError::Invalid("record is not a Source"));
    }
    let Some(head) = module.head_revision_id else {
        return Err(KnowledgeError::Invalid(
            "source span has no sealed revision",
        ));
    };
    let stored = super::revision::load_revision(tx, &head)?;
    let payload: SourceRecordPayload = serde_json::from_str(&stored.payload_json)?;
    if !payload_has_span(&payload, span_id) {
        return Err(KnowledgeError::Invalid(
            "source span has no sealed revision",
        ));
    }
    Ok((span.record_id, stored.revision_digest))
}

fn source_closure(
    tx: &Transaction<'_>,
    loupist_id: &str,
    job: &GraphJobView,
    previous: &[super::model::EvidenceRevision],
    span_ids: &[String],
) -> KnowledgeResult<Vec<super::model::EvidenceRevision>> {
    use std::collections::{BTreeMap, BTreeSet};

    use super::model::{DependencyRole, EvidenceRevision, SourceRecordPayload};
    let binding = job.document.source_binding.as_ref();
    if job.document.episode_kind == EPISODE_SPAN
        && binding.is_none_or(|item| {
            item.source_record_id.is_empty() || item.source_revision_id.is_empty()
        })
    {
        return Err(KnowledgeError::Invalid(
            "graph episode source revision is required",
        ));
    }
    // A later source revision still carries earlier spans. Cite that revision
    // for every span it contains, instead of also keeping the previous digest.
    let bound = match binding {
        Some(binding) if !binding.source_revision_id.is_empty() => {
            Some(bound_source_catalog(tx, loupist_id, binding)?)
        }
        _ => None,
    };
    let mut previous_spans: Option<BTreeMap<String, BTreeSet<(String, String)>>> = None;
    let mut dependencies = Vec::new();
    for span_id in span_ids {
        let carried = bound.as_ref().and_then(|(record_id, digest, payload)| {
            payload_has_span(payload, span_id).then(|| (record_id.clone(), digest.clone()))
        });
        let (record_id, revision_digest) = if let Some(pair) = carried {
            pair
        } else if binding.is_some_and(|item| item.span_id == *span_id) {
            return Err(KnowledgeError::NotFound);
        } else {
            // Resolve each sealed Source once. Re-reading every Source for
            // every evidence span holds the publication transaction for minutes
            // in an established World, blocking all other workspace operations.
            if previous_spans.is_none() {
                let mut index: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
                let mut loaded = BTreeSet::new();
                for dependency in previous
                    .iter()
                    .filter(|dependency| dependency.role == DependencyRole::WorldSource)
                {
                    let key = (
                        dependency.record_id.clone(),
                        dependency.revision_digest.clone(),
                    );
                    if !loaded.insert(key.clone()) {
                        continue;
                    }
                    let stored = super::revision::current_source(tx, loupist_id, &key.0)?;
                    let key = (stored.record_id.clone(), stored.revision_digest.clone());
                    let payload: SourceRecordPayload = serde_json::from_str(&stored.payload_json)?;
                    for capture in payload.source.captures {
                        for representation in capture.representations {
                            for span in representation.spans {
                                index.entry(span.span_id).or_default().insert(key.clone());
                            }
                        }
                    }
                }
                previous_spans = Some(index);
            }
            let found = previous_spans.as_ref().and_then(|index| index.get(span_id));
            let count = found.map_or(0, BTreeSet::len);
            if count == 0
                && matches!(
                    job.document.episode_kind.as_str(),
                    EPISODE_SEALED | EPISODE_RECOVERY
                )
            {
                source_head_for_span(tx, loupist_id, span_id)?
            } else if count == 0 {
                return Err(KnowledgeError::Invalid(
                    "source span has no sealed revision",
                ));
            } else if count != 1 {
                return Err(KnowledgeError::ConflictingDependencyRevision);
            } else {
                found
                    .and_then(|matches| matches.first())
                    .cloned()
                    .ok_or(KnowledgeError::NotFound)?
            }
        };
        if let Some(existing) = dependencies
            .iter()
            .find(|dependency: &&EvidenceRevision| dependency.record_id == record_id)
        {
            if existing.revision_digest != revision_digest {
                return Err(KnowledgeError::ConflictingDependencyRevision);
            }
            continue;
        }
        dependencies.push(EvidenceRevision {
            record_id,
            revision_digest,
            role: DependencyRole::WorldSource,
        });
    }
    Ok(dependencies)
}

fn payload_has_span(payload: &super::model::SourceRecordPayload, span_id: &str) -> bool {
    payload.source.captures.iter().any(|capture| {
        capture.representations.iter().any(|representation| {
            representation
                .spans
                .iter()
                .any(|span| span.span_id == span_id)
        })
    })
}

fn clip(value: &str, max: usize) -> String {
    let mut end = max.min(value.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

pub fn binding_matches(document: &JobDocument, delivered_span_ids: &[String]) -> bool {
    let Some(binding) = &document.source_binding else {
        return matches!(
            document.episode_kind.as_str(),
            EPISODE_SEALED | EPISODE_RECOVERY
        );
    };
    delivered_span_ids
        .iter()
        .any(|span| span == &binding.span_id)
}

/// Test conveniences over the app's job transactions, shared by graph tests
/// across the crate.
#[cfg(test)]
impl super::KnowledgeStore {
    pub fn graph_accept_episode(
        &self,
        settings: &GraphProcessingSettings,
        request: AddToWorld,
    ) -> KnowledgeResult<GraphJobView> {
        self.transact(|write| accept_episode(write.tx, write.blobs, settings, request))
    }

    pub fn graph_job(&self, loupist_id: &str, job_id: &str) -> KnowledgeResult<GraphJobView> {
        self.read(|write| get_job(write.tx, loupist_id, job_id))
    }

    /// The jobs the worker loop would dispatch next, one per Loupist.
    pub fn graph_queue_heads(&self) -> KnowledgeResult<Vec<String>> {
        self.read(|write| {
            Ok(queued_loupist_heads(write.tx, &[])?
                .into_iter()
                .map(|job| job.job_id)
                .collect())
        })
    }

    /// The app's claim; a job that cannot start comes back unchanged.
    pub fn graph_mark_running(
        &self,
        settings: &GraphProcessingSettings,
        job_id: &str,
        now_ms: i64,
    ) -> KnowledgeResult<GraphJobView> {
        match self.graph_claim_running(settings, job_id, now_ms)? {
            GraphJobClaim::Acquired(job) | GraphJobClaim::Existing(job) => Ok(job),
        }
    }
}

#[cfg(test)]
mod tests;
