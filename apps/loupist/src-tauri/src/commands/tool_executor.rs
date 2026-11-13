mod approval;
mod artifacts;
mod as_of_turn;
pub(super) mod assets;
mod bot_memory;
mod earnings;
#[cfg(test)]
mod earnings_tests;
mod question;
pub(crate) use earnings::commit_turn as commit_earnings;
mod arguments;
#[cfg(test)]
mod assets_tests;
mod authority;
mod calculation;
#[cfg(test)]
mod calculation_tests;
mod lens_file;
pub(crate) use lens_file::commit_turn as commit_filings;
pub(super) use lens_file::StagedFiling;
mod principles;
pub(super) use principles::StagedPrinciple;
mod chat_history;
mod conversation;
mod dataset_export;
mod dataset_import;
#[cfg(test)]
mod dataset_tests;
mod datasets;
mod errors;
mod evidence;
use errors::{command_error, command_message, knowledge_error, store_error};
mod finance;
mod grid;
#[cfg(test)]
mod grid_tests;
mod market;
mod mcp_host;
mod memory_reads;
mod memory_saves;
mod number_checks;
mod research;
mod returning;
pub(super) mod schedules;
pub(in crate::commands) mod sensitivity;
mod setup;
pub(super) use setup::StagedSetup;
mod connection_setup;
pub(crate) use setup::{apply_watchlist as apply_setup_watchlist, commit_turn as commit_setup};
mod workspace;
pub(in crate::commands) use workspace::NativeGate;
mod world_schema;
#[cfg(test)]
mod world_schema_tests;
pub(crate) use world_schema::commit_turn as commit_vocabulary;
pub(super) use world_schema::StagedVocabulary;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::PathBuf,
    sync::Arc,
};

use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::{
    app::AppState,
    broker::{
        tool_class::{PackText, Retention},
        BrokerError, ToolExecutor, ToolMethod, ToolPolicy,
    },
    chat::artifacts::{
        ArtifactCommit, ChatArtifact, ChatArtifactKind, ChatArtifactPayload, ChatArtifactRevision,
    },
    domain::{MemoryAccess, MemoryRefSnapshot},
    mcp::pool::McpProcessPool,
    util::sha256,
};

use super::{new_id, now_ms};

use market::*;

const DEFAULT_CHART_QUERY_ROWS: usize = 50;
const MAX_CHART_QUERY_ROWS: usize = 200;
const MAX_CHART_QUERY_BYTES: usize = 32 * 1024;
const MAX_RUN_RESULTS: usize = crate::domain::MAX_CHAT_EVIDENCE_REFS;
const MAX_RUN_RESULT_BYTES: usize = 32 * 1024 * 1024;
const MAX_RUN_RESULT_REGISTRY_BYTES: usize = 32 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChartQueryRequest {
    artifact_id: String,
    revision: u32,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_chart_query_rows")]
    limit: usize,
    #[serde(default)]
    columns: Option<Vec<String>>,
}

fn default_chart_query_rows() -> usize {
    DEFAULT_CHART_QUERY_ROWS
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ChartPublishMode {
    Create,
    Revise,
}

struct ChartPublishRequest {
    mode: ChartPublishMode,
    title: String,
    dataset: Option<crate::chat::chart::ChartDatasetInput>,
    view: Option<crate::chat::chart::ChartView>,
    studies: Option<Vec<crate::chat::chart::ChartStudy>>,
    custom_series: Option<Vec<crate::chat::chart::ChartCustomSeries>>,
    drawings: Option<Vec<crate::chat::chart::ChartDrawing>>,
    note: Option<String>,
    artifact_id: Option<String>,
    edit_token: Option<String>,
}

type EvidenceSink = Arc<dyn Fn(crate::domain::ChatEvidenceRef, Option<String>) + Send + Sync>;
/// Receives (tool call id, World node ids) when a call's World read is acknowledged.
type EntitySink = Arc<dyn Fn(String, Vec<String>) + Send + Sync>;
/// Receives (tool call id, approval card) when a call starts or stops waiting for the user.
pub(super) type ApprovalSink =
    Arc<dyn Fn(Option<String>, crate::chat::approvals::ChatApproval) + Send + Sync>;
/// Receives (tool call id, question card) when an `ask_user` call starts or stops waiting.
pub(super) type QuestionSink =
    Arc<dyn Fn(Option<String>, crate::chat::questions::ChatQuestion) + Send + Sync>;

#[derive(Default)]
pub(super) struct ToolCapture {
    #[cfg(feature = "eval-harness")]
    pub(in crate::commands) eval_native_read:
        Option<Arc<super::chat_runtime::EvalNativeReadCallback>>,
    pub(super) tool_refusals: Arc<std::sync::Mutex<Vec<crate::domain::ChatToolRefusal>>>,
    pub(super) turn_taint: crate::custom_connections::taint::TurnTaint,
    pub(crate) unattended_run_id: Option<String>,
    pub(super) tool_trace: Arc<crate::broker::ToolTraceCapture>,
    pub(crate) surface: Option<crate::chat::surface::ChatSurface>,
    pub(super) schedule_context: Option<schedules::ScheduleContext>,
    pub(in crate::commands) staged_handoff: Mutex<Option<crate::chat::handoff::StagedHandoff>>,
    pub(super) schedule_sink:
        Option<Arc<dyn Fn(crate::scheduled_tasks::types::ScheduleReceipt) + Send + Sync>>,
    pub(super) memories: Mutex<HashMap<String, MemoryRefSnapshot>>,
    pub(super) web_sources: Mutex<HashMap<String, crate::web::WebSource>>,
    web_fetch_snapshots: Mutex<HashMap<String, RunWebFetchSnapshot>>,
    pub(crate) run_results: Mutex<RunResultRegistry>,
    pub(super) evidence_refs: Mutex<Vec<crate::domain::ChatEvidenceRef>>,
    pub(super) evidence_sink: Option<EvidenceSink>,
    pub(super) entity_sink: Option<EntitySink>,
    /// Set for interactive Chats: unapproved read-only custom tools wait for the user here.
    pub(super) approval_sink: Option<ApprovalSink>,
    /// Set for interactive Chats: `ask_user` shows its question card here.
    pub(super) question_sink: Option<QuestionSink>,
    /// Cards this reply has shown; it holds at most `MAX_PROGRESS_APPROVALS`.
    pub(super) approvals_shown: std::sync::atomic::AtomicUsize,
    /// The native policy's turn state (`agent_harness::native_policy`).
    pub(in crate::commands) native: workspace::NativeGate,
    pub(super) provider_pace: Mutex<finance::ProviderPace>,
    pub(super) staged_sources: Mutex<Vec<StagedSource>>,
    pub(super) staged_reads: Mutex<Vec<StagedKnowledgeRead>>,
    note_reads: Mutex<BTreeSet<String>>,
    /// Standing Lens body admitted with the turn envelope. This is not a tool ACK.
    pub(super) context_reads: Mutex<Vec<StagedKnowledgeRead>>,
    pub(super) staged_research: Mutex<Option<StagedResearch>>,
    /// One Journal save or review per turn, reserved until delivery is discarded.
    journal_delivery: Mutex<Option<String>>,
    pub(super) staged_principles: Mutex<Vec<StagedPrinciple>>,
    /// Checked thesis updates, committed with the reply.
    pub(in crate::commands) staged_theses: Mutex<Vec<crate::knowledge::assets::ThesisUpdate>>,
    /// Checked results saves, committed with the reply.
    pub(in crate::commands) staged_earnings: Mutex<Vec<earnings::EarningsSave>>,
    pub(in crate::commands) staged_vocabulary: Mutex<Vec<StagedVocabulary>>,
    pub(in crate::commands) staged_filings: Mutex<Vec<StagedFiling>>,
    /// The reply's one `loupist_setup` call.
    pub(in crate::commands) staged_setup: Mutex<Option<StagedSetup>>,
    /// `bot_memory` calls reserved under this one lock when checked; only
    /// delivered ones apply when the Bot reply commits.
    pub(in crate::commands) staged_bot_memory: Mutex<bot_memory::BotMemoryReservations>,
    /// Messages this Bot turn was offered for review (`chat::bot_memory`).
    pub(in crate::commands) bot_memory_review: Vec<String>,
    /// Pages and ops each `lens_file` delivery reserved from the turn's
    /// budget; an undelivered call frees its share.
    lens_file_budget: Mutex<BTreeMap<String, (usize, usize)>>,
    /// Assets with a thesis update in this turn (staged or in flight), by
    /// delivery: parallel calls cannot both move one asset's read.
    pub(super) thesis_assets: Mutex<BTreeMap<String, String>>,
    /// Set when any read ran under an as-of cutoff; such turns never touch asset pages.
    pub(super) as_of_turn: std::sync::atomic::AtomicBool,
    /// The user's as-of date for this turn (`YYYY-MM-DD`), set by the host.
    pub(in crate::commands) turn_as_of: Option<String>,
    /// Saved Memory the cutoff left out, from context assembly and tool reads.
    pub(in crate::commands) as_of_left_out:
        std::sync::Mutex<crate::commands::knowledge::as_of::LeftOut>,
    pub(super) context_lens_receipts: Mutex<Vec<crate::domain::LensReceipt>>,
    pub(super) principle_context: Mutex<BTreeMap<String, String>>,
    pub(super) artifacts: Mutex<Vec<ArtifactCommit>>,
    artifact_reads: Mutex<BTreeSet<(String, u32)>>,
    source_message_id: Option<String>,
    pub(super) principle_user_message_id: Option<String>,
    /// Original Chat binding captured by the host, never selected by tool input.
    pub(super) bound_chat_id: Option<String>,
    pending_deliveries: Mutex<HashMap<String, PendingToolCapture>>,
    pub(super) web_search_policy: crate::web::WebSearchPolicy,
    /// The provider answering this turn, for model-native search. A Chat
    /// turn's Auto retry after a gone model may change it.
    chat_provider: std::sync::Mutex<String>,
    pub(super) search_cancel: crate::web::SearchCancel,
    mcp_sessions: Mutex<mcp_host::McpTurnSessions>,
    pub(super) mcp_scratch_root: Option<PathBuf>,
    pub(super) mcp_pool: Option<McpProcessPool>,
}

#[derive(Default)]
struct PendingToolCapture {
    memories: HashMap<String, MemoryRefSnapshot>,
    web_sources: HashMap<String, crate::web::WebSource>,
    web_fetch_snapshots: HashMap<String, RunWebFetchSnapshot>,
    run_results: BTreeMap<String, CapturedRunResult>,
    /// Run results that host reads resolve but the answer never lists as
    /// Sources (a market data read that returned no rows).
    unlisted_results: BTreeSet<String>,
    staged_sources: Vec<StagedSource>,
    staged_reads: Vec<StagedKnowledgeRead>,
    note_reads: BTreeSet<String>,
    research: Option<StagedResearch>,
    principles: Vec<StagedPrinciple>,
    theses: Vec<crate::knowledge::assets::ThesisUpdate>,
    earnings: Vec<earnings::EarningsSave>,
    vocabulary: Vec<StagedVocabulary>,
    filings: Vec<StagedFiling>,
    setup: Option<StagedSetup>,
    artifacts: Vec<ArtifactCommit>,
    artifact_reads: BTreeSet<(String, u32)>,
    handoff: Option<crate::chat::handoff::StagedHandoff>,
    call_id: Option<String>,
}

struct CapturedRunResult {
    result: RunResult,
    bytes: usize,
}

pub(crate) use crate::broker::tool_class::RunResultOrigin;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RunResultProducer {
    pub(crate) origin: RunResultOrigin,
    pub(crate) runtime_id: String,
    pub(crate) tool_name: String,
    pub(crate) provider: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RunResult {
    pub(crate) result_ref: String,
    pub(crate) producer: RunResultProducer,
    pub(crate) origin: Option<String>,
    pub(crate) request_digest: String,
    pub(crate) response_digest: String,
    pub(crate) retrieved_at: String,
    pub(crate) payload: Value,
    pub(crate) model_delivery: Option<crate::domain::ChatEvidenceDelivery>,
    pub(crate) warnings: Vec<String>,
    pub(crate) upstream_result_refs: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RunResultRegistry {
    results: BTreeMap<String, RunResult>,
    bytes: usize,
}

impl RunResultRegistry {
    pub(crate) fn get(&self, result_ref: &str) -> Option<&RunResult> {
        self.results.get(result_ref)
    }

    #[cfg(test)]
    fn insert(&mut self, result: RunResult) -> Result<(), BrokerError> {
        let payload_bytes = serde_json::to_vec(&result.payload)
            .map_err(|_| BrokerError::Execution("tool result could not be captured".into()))?
            .len();
        let bytes = run_result_bytes(&result, payload_bytes);
        self.insert_captured(CapturedRunResult { result, bytes })
    }

    fn insert_captured(&mut self, captured: CapturedRunResult) -> Result<(), BrokerError> {
        let CapturedRunResult { result, bytes } = captured;
        if bytes > MAX_RUN_RESULT_BYTES
            || self.results.len() >= MAX_RUN_RESULTS
            || self
                .bytes
                .checked_add(bytes)
                .is_none_or(|total| total > MAX_RUN_RESULT_REGISTRY_BYTES)
            || self.results.contains_key(&result.result_ref)
        {
            return Err(BrokerError::Execution(
                "run result registry capacity was exceeded".into(),
            ));
        }
        self.bytes += bytes;
        self.results.insert(result.result_ref.clone(), result);
        Ok(())
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &RunResult> {
        self.results.values()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StagedSource {
    pub(super) custom_origin: Option<crate::custom_connections::taint::CustomOrigin>,
    pub(super) source_class: crate::knowledge::SourceClass,
    pub(super) authorship: crate::knowledge::Authorship,
    pub(super) title: String,
    pub(super) bytes: Vec<u8>,
    pub(super) media_type: String,
    pub(super) origin: String,
    pub(super) coverage: String,
    pub(super) chat_id: String,
    pub(super) request_ref: String,
    pub(super) result_ref: String,
    pub(super) publication_time: Option<crate::knowledge::TimePoint>,
    pub(super) provider_identity: String,
    pub(super) document_id: String,
    pub(super) material_role: String,
}

#[derive(Clone, Debug)]
pub(super) enum AckedPayload {
    Source {
        revision_digest: String,
        capture_id: String,
        representation_id: String,
        bytes: Vec<u8>,
        media_type: String,
        selected_digest: String,
        coverage: String,
        attribution: String,
        publication_time: Option<crate::knowledge::TimePoint>,
    },
    World {
        revision_digest: String,
        view_json: String,
        view_digest: String,
    },
    Lens {
        revision_digest: String,
        document_json: String,
        body_digest: String,
        status: String,
    },
    Journal {
        journal_id: String,
        delivered_text: String,
        digest: String,
        coverage: String,
    },
}

#[derive(Clone, Debug)]
pub(super) struct StagedKnowledgeRead {
    /// Host reference returned to the model. Citations name this, not a module id.
    pub(super) read_ref: String,
    pub(super) scope: String,
    pub(super) record_id: String,
    pub(super) span_id: Option<String>,
    pub(super) revision_id: String,
    pub(super) object_id: String,
    pub(super) title: String,
    pub(super) payload: AckedPayload,
}

impl StagedKnowledgeRead {
    pub(in crate::commands) fn lens_receipt(
        &self,
        delivery: &str,
    ) -> Option<crate::domain::LensReceipt> {
        let AckedPayload::Lens {
            document_json,
            revision_digest,
            ..
        } = &self.payload
        else {
            return None;
        };
        let document: serde_json::Value = serde_json::from_str(document_json).ok()?;
        Some(crate::domain::LensReceipt {
            record_id: self.record_id.clone(),
            revision_id: self.revision_id.clone(),
            revision_digest: revision_digest.clone(),
            kind: document["lensKind"].as_str()?.into(),
            title: document["receiptTitle"]
                .as_str()
                .unwrap_or(&self.title)
                .into(),
            delivery: delivery.into(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StagedEarlierJournal {
    pub(super) read_ref: String,
    pub(super) relation: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct StagedResearchJournal {
    /// Host clock captured at tool time and retained as the Journal timestamp.
    pub(super) at_ms: i64,
    pub(super) check: Option<crate::knowledge::JournalCheck>,
    /// A reason or line for an asset page; the page is created at commit if new.
    pub(super) thesis: Option<(
        crate::knowledge::assets::JournalThesis,
        crate::knowledge::assets::AssetIdentity,
    )>,
    pub(super) outcome: Option<crate::knowledge::JournalOutcomeDraft>,
    pub(super) judgment: String,
    pub(super) reasoning: String,
    pub(super) earlier: Vec<StagedEarlierJournal>,
    pub(super) source_reads: Vec<String>,
    pub(super) world_reads: Vec<String>,
    pub(super) lens_reads: Vec<String>,
    pub(super) entities: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct StagedResearch {
    pub(super) observations: String,
    pub(super) interpretation: String,
    pub(super) assumptions: String,
    pub(super) evaluation: String,
    pub(super) journal: Option<StagedResearchJournal>,
    pub(super) working_note: Option<crate::knowledge::WorkingNoteDraft>,
    pub(super) source_reads: Vec<String>,
    pub(super) world_reads: Vec<String>,
    pub(super) lens_reads: Vec<String>,
    pub(super) previous_note_ids: Vec<String>,
}

#[derive(Clone, Debug)]
struct RunWebFetchSnapshot {
    fetched: crate::web::WebFetchSnapshot,
    issued_offsets: BTreeSet<usize>,
}

impl ToolCapture {
    /// Initial host-authored context can contain the same retained text as a tool read.
    pub(in crate::commands) fn admit_untrusted_context(&self, untrusted: bool) {
        if untrusted {
            self.turn_taint.mark_untrusted_text();
            self.tool_trace.mark_untrusted_text();
        }
    }

    pub(super) fn for_chat(source_message_id: String) -> Self {
        Self {
            source_message_id: Some(source_message_id),
            ..Self::default()
        }
    }

    pub(super) fn set_chat_provider(&self, provider: &str) {
        provider.clone_into(
            &mut self
                .chat_provider
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
    }

    pub(super) fn chat_provider(&self) -> Option<String> {
        Some(
            self.chat_provider
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
        )
        .filter(|provider| !provider.is_empty())
    }

    pub(crate) async fn run_result(&self, result_ref: &str) -> Option<RunResult> {
        self.run_results.lock().await.get(result_ref).cloned()
    }

    pub(super) async fn stage_run_result_with_delivery(
        &self,
        delivery_id: &str,
        producer: RunResultProducer,
        request: &Value,
        payload: Value,
        upstream_result_refs: Vec<String>,
        model_payload: Option<&Value>,
    ) -> Result<String, BrokerError> {
        let request_bytes = serde_json::to_vec(request).map_err(|_| BrokerError::Malformed)?;
        let response_bytes = serde_json::to_vec(&payload)
            .map_err(|_| BrokerError::Execution("tool result could not be captured".into()))?;
        if response_bytes.len() > MAX_RUN_RESULT_BYTES {
            return Err(BrokerError::Execution(
                "tool result exceeded the run result capture limit".into(),
            ));
        }
        let mut unique_upstream = BTreeSet::new();
        for result_ref in &upstream_result_refs {
            if result_ref.is_empty()
                || result_ref.len() > 128
                || !unique_upstream.insert(result_ref.as_str())
                || self.run_result(result_ref).await.is_none()
            {
                return Err(BrokerError::Execution(
                    "upstream_result_refs must name unique delivered results from this turn".into(),
                ));
            }
        }
        let result_ref = format!("result:{}", uuid::Uuid::new_v4().simple());
        let warnings = result_warnings(&payload);
        let origin = if producer.origin == RunResultOrigin::WorkspaceFile {
            payload
                .get("path")
                .and_then(Value::as_str)
                .map(|path| format!("workspace:{path}"))
        } else {
            result_origin(request)
        };
        let result = RunResult {
            result_ref: result_ref.clone(),
            producer,
            origin,
            request_digest: sha256(&request_bytes),
            response_digest: sha256(&response_bytes),
            retrieved_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            payload,
            model_delivery: model_payload.map(evidence::model_delivery),
            warnings,
            upstream_result_refs,
        };
        let result_bytes = run_result_bytes(&result, response_bytes.len());
        if result_bytes > MAX_RUN_RESULT_BYTES {
            return Err(BrokerError::Execution(
                "tool result exceeded the run result capture limit".into(),
            ));
        }
        let committed = self.run_results.lock().await;
        let mut pending = self.pending_deliveries.lock().await;
        let pending_count = pending
            .values()
            .map(|capture| capture.run_results.len())
            .sum::<usize>();
        let pending_bytes = pending
            .values()
            .flat_map(|capture| capture.run_results.values())
            .try_fold(0_usize, |total, captured| total.checked_add(captured.bytes))
            .ok_or_else(|| {
                BrokerError::Execution("run result registry capacity was exceeded".into())
            })?;
        if committed.results.len() + pending_count >= MAX_RUN_RESULTS
            || committed
                .bytes
                .checked_add(pending_bytes)
                .and_then(|bytes| bytes.checked_add(result_bytes))
                .is_none_or(|bytes| bytes > MAX_RUN_RESULT_REGISTRY_BYTES)
        {
            return Err(BrokerError::Execution(
                "run result registry capacity was exceeded".into(),
            ));
        }
        pending
            .entry(delivery_id.to_owned())
            .or_default()
            .run_results
            .insert(
                result_ref.clone(),
                CapturedRunResult {
                    result,
                    bytes: result_bytes,
                },
            );
        drop(pending);
        drop(committed);
        Ok(result_ref)
    }

    pub(super) async fn note_call(&self, delivery_id: &str, call_id: &str) {
        let mut pending = self.pending_deliveries.lock().await;
        pending.entry(delivery_id.to_owned()).or_default().call_id = Some(call_id.to_owned());
    }

    async fn stage<F>(&self, delivery_id: &str, update: F)
    where
        F: FnOnce(&mut PendingToolCapture),
    {
        let mut pending = self.pending_deliveries.lock().await;
        update(pending.entry(delivery_id.to_owned()).or_default());
    }

    async fn discard_delivery(&self, delivery_id: &str) {
        self.pending_deliveries.lock().await.remove(delivery_id);
        let mut journal = self.journal_delivery.lock().await;
        if journal.as_deref() == Some(delivery_id) {
            *journal = None;
        }
        drop(journal);
        // A thesis update that was never delivered frees its asset again.
        self.thesis_assets
            .lock()
            .await
            .retain(|_, delivery| delivery != delivery_id);
        self.lens_file_budget.lock().await.remove(delivery_id);
        self.staged_bot_memory.lock().await.discard(delivery_id);
    }

    async fn commit_delivery(&self, delivery_id: &str) {
        self.staged_bot_memory.lock().await.deliver(delivery_id);
        // Keep the reservation transfer atomic with staging: both paths acquire
        // the registry before pending deliveries, so no concurrent call can
        // reserve the slot between removal and committed insertion.
        let mut registry = self.run_results.lock().await;
        let mut pending_deliveries = self.pending_deliveries.lock().await;
        let Some(mut pending) = pending_deliveries.remove(delivery_id) else {
            return;
        };
        let call_id = pending.call_id.clone();
        let mut delivered_evidence = Vec::new();
        for (result_ref, captured) in std::mem::take(&mut pending.run_results) {
            if !pending.unlisted_results.contains(&result_ref) {
                delivered_evidence.push(evidence::project(&captured.result));
            }
            registry
                .insert_captured(captured)
                .expect("staged run result budget was checked before delivery");
        }
        drop(pending_deliveries);
        drop(registry);
        if let (Some(sink), Some(call_id)) = (&self.entity_sink, call_id.as_deref()) {
            let ids = delivered_world_node_ids(&pending.staged_reads);
            if !ids.is_empty() {
                sink(call_id.to_owned(), ids);
            }
        }
        self.publish_evidence(delivered_evidence, call_id).await;

        if !pending.memories.is_empty() {
            let mut memories = self.memories.lock().await;
            for (record_id, memory) in pending.memories {
                let existing_rank = memories
                    .get(&record_id)
                    .map(memory_authority_rank)
                    .unwrap_or_default();
                if existing_rank <= memory_authority_rank(&memory) {
                    memories.insert(record_id, memory);
                }
            }
        }
        if !pending.web_sources.is_empty() {
            self.web_sources.lock().await.extend(pending.web_sources);
        }
        if !pending.web_fetch_snapshots.is_empty() {
            let mut snapshots = self.web_fetch_snapshots.lock().await;
            for (source_id, snapshot) in pending.web_fetch_snapshots {
                if let Some(existing) = snapshots.get_mut(&source_id).filter(|existing| {
                    snapshot.fetched.page(0).is_ok_and(|page| {
                        existing
                            .fetched
                            .matches_source(&source_id, &page.content_sha256)
                    })
                }) {
                    existing.issued_offsets.extend(snapshot.issued_offsets);
                } else {
                    snapshots.insert(source_id, snapshot);
                }
            }
        }
        if !pending.staged_sources.is_empty() {
            self.staged_sources
                .lock()
                .await
                .extend(pending.staged_sources);
        }
        if !pending.staged_reads.is_empty() {
            self.staged_reads.lock().await.extend(pending.staged_reads);
        }
        self.note_reads.lock().await.extend(pending.note_reads);
        if let Some(mut research) = pending.research {
            let mut staged = self.staged_research.lock().await;
            if let Some(existing) = staged.as_mut() {
                if research.journal.is_some() {
                    existing.journal = research.journal.take();
                }
                if research.working_note.is_some() {
                    existing.working_note = research.working_note;
                    existing.source_reads = research.source_reads;
                    existing.world_reads = research.world_reads;
                    existing.lens_reads = research.lens_reads;
                    existing.previous_note_ids = research.previous_note_ids;
                }
            } else {
                *staged = Some(research);
            }
        }
        if !pending.theses.is_empty() {
            self.staged_theses
                .lock()
                .await
                .extend(std::mem::take(&mut pending.theses));
        }
        if !pending.earnings.is_empty() {
            self.staged_earnings
                .lock()
                .await
                .extend(std::mem::take(&mut pending.earnings));
        }
        if !pending.vocabulary.is_empty() {
            self.staged_vocabulary
                .lock()
                .await
                .extend(std::mem::take(&mut pending.vocabulary));
        }
        if !pending.filings.is_empty() {
            self.staged_filings
                .lock()
                .await
                .extend(std::mem::take(&mut pending.filings));
        }
        if let Some(setup) = pending.setup.take() {
            *self.staged_setup.lock().await = Some(setup);
        }
        for principle in pending.principles {
            let mut staged = self.staged_principles.lock().await;
            staged.retain(|p| p.record_id != principle.record_id);
            staged.push(principle);
        }
        if !pending.artifacts.is_empty() {
            self.artifacts
                .lock()
                .await
                .extend(std::mem::take(&mut pending.artifacts));
        }
        self.artifact_reads
            .lock()
            .await
            .extend(pending.artifact_reads);
        if let Some(handoff) = pending.handoff {
            *self.staged_handoff.lock().await = Some(handoff);
        }
    }

    async fn stage_research_open(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        let refusal = |where_: &str| {
            BrokerError::Execution(format!(
                "research_open is unavailable in a {where_} turn: only the Bot opens a research session"
            ))
        };
        match &self.surface {
            Some(surface) if surface.is_bot() => {}
            Some(surface) => return Err(refusal(surface.name())),
            None => return Err(refusal("host")),
        }
        if policy.as_of.is_some() {
            return Err(BrokerError::Execution(
                "A research session needs a present-day request. Leave the historical cutoff before handing off.".into(),
            ));
        }
        let request = crate::chat::handoff::HandoffRequest::parse(&params)
            .map_err(|(field, expected)| BrokerError::InvalidArguments { field, expected })?;
        if self.staged_handoff.lock().await.is_some()
            || self
                .pending_deliveries
                .lock()
                .await
                .values()
                .any(|pending| pending.handoff.is_some())
        {
            return Err(BrokerError::Execution(
                "This reply already opens a research session: one research session per reply."
                    .into(),
            ));
        }
        let context = self
            .schedule_context
            .as_ref()
            .ok_or_else(|| BrokerError::invalid("research_open", "This reply has no research-session context. Continue in this Chat and tell the user no session was opened."))?;
        let staged = crate::chat::handoff::StagedHandoff {
            request,
            run: crate::chat::handoff::HandoffRun {
                model_profile_id: context.model_profile_id.clone(),
                thinking_level: context.thinking_level.clone(),
                run_options: context.run_options.clone(),
                use_memory: context.use_memory,
                update_memory: context.update_memory,
            },
        };
        let result = serde_json::json!({
            "staged": true,
            "title": staged.request.title,
            "requestKey": staged.request.request_key,
            "notice": "The session opens when this reply finishes and its result arrives here. Tell the user the session title; do not repeat the research in this reply."
        });
        self.stage(delivery_id, |pending| pending.handoff = Some(staged))
            .await;
        Ok(result)
    }
}

fn memory_authority_rank(memory: &MemoryRefSnapshot) -> u8 {
    match (memory.access, memory.section.as_ref()) {
        (MemoryAccess::SearchDiscovered, _) => 0,
        (MemoryAccess::ExactRead, Some(_)) => 1,
        (MemoryAccess::ExactRead, None) => 2,
    }
}

fn run_result_bytes(result: &RunResult, payload_bytes: usize) -> usize {
    payload_bytes
        + result.model_delivery.as_ref().map_or(0, |delivery| {
            delivery.excerpt.len() + delivery.content_sha256.len() + 64
        })
        + result.result_ref.len()
        + result.producer.runtime_id.len()
        + result.producer.tool_name.len()
        + result.producer.provider.as_deref().map_or(0, str::len)
        + result.origin.as_deref().map_or(0, str::len)
        + result.request_digest.len()
        + result.response_digest.len()
        + result.retrieved_at.len()
        + result.warnings.iter().map(String::len).sum::<usize>()
        + result
            .upstream_result_refs
            .iter()
            .map(String::len)
            .sum::<usize>()
}

fn result_origin(request: &Value) -> Option<String> {
    const KEYS: [&str; 5] = ["url", "symbol", "ticker", "query", "uri"];
    for key in KEYS {
        if let Some(value) = request
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= 2_048 && !value.contains('\0'))
        {
            return Some(value.to_owned());
        }
    }
    request.get("arguments").and_then(result_origin)
}

fn result_warnings(payload: &Value) -> Vec<String> {
    let mut warnings = Vec::new();
    for pointer in [
        "/warnings",
        "/quality_warnings",
        "/structuredContent/warnings",
    ] {
        warnings.extend(
            payload
                .pointer(pointer)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(bounded_warning_text),
        );
    }
    if let Some(warning) = payload
        .get("quality")
        .and_then(|quality| quality.get("warning"))
        .and_then(Value::as_str)
        .filter(|warning| !warning.is_empty() && warning.len() <= 2_048)
    {
        warnings.push(warning.to_owned());
    }
    warnings
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(16)
        .collect()
}

fn bounded_warning_text(value: &Value) -> Option<String> {
    let text = match value {
        Value::String(message) => message.clone(),
        Value::Object(object) => {
            let message = object.get("message")?.as_str()?;
            match object.get("category").and_then(Value::as_str) {
                Some(category) if !category.is_empty() => format!("{category}: {message}"),
                _ => message.to_owned(),
            }
        }
        _ => return None,
    };
    (!text.is_empty() && text.len() <= 2_048 && !text.contains('\0')).then_some(text)
}

tokio::task_local! {
    static TOOL_CALL_ID: String;
}

pub(super) fn current_tool_call_id() -> Option<String> {
    TOOL_CALL_ID.try_with(|call_id| call_id.clone()).ok()
}

#[derive(Clone)]
pub(super) struct AppToolExecutor {
    pub(super) state: AppState,
    pub(super) capture: Arc<ToolCapture>,
    pub(super) loupist_id: String,
    pub(super) capability_ids: BTreeSet<String>,
}

#[async_trait]
impl ToolExecutor for AppToolExecutor {
    fn enforce_turn_policy(&self, method: ToolMethod, params: &Value) -> Result<(), BrokerError> {
        let result = self.capture.turn_taint.enforce(method, params);
        let outbound_paused = match &result {
            Err(BrokerError::WithNext { source, .. }) => {
                matches!(source.as_ref(), BrokerError::OutboundPaused)
            }
            result => matches!(result, Err(BrokerError::OutboundPaused)),
        };
        if outbound_paused {
            if let Some(origin) = self.capture.turn_taint.origin() {
                self.capture
                    .record_tool_refusal(crate::domain::ChatToolRefusal {
                        connection_id: origin.connection_id,
                        connection_name: origin.name,
                        tool_name: serde_json::to_value(method)
                            .ok()
                            .and_then(|v| v.as_str().map(str::to_owned))
                            .unwrap_or_default(),
                        reason: crate::domain::ChatToolRefusalReason::Taint,
                    });
            }
        }
        result
    }

    fn tool_trace(&self) -> Option<Arc<crate::broker::ToolTraceCapture>> {
        Some(self.capture.tool_trace.clone())
    }

    async fn execute(
        &self,
        policy: &ToolPolicy,
        method: ToolMethod,
        params: Value,
    ) -> Result<Value, BrokerError> {
        let delivery_id = new_id("direct-delivery");
        let result = self
            .execute_staged(policy, method, params, &delivery_id)
            .await;
        if let Err(error) = result {
            self.capture.discard_delivery(&delivery_id).await;
            return Err(error);
        }
        self.commit_delivery(policy, &delivery_id).await?;
        result
    }

    async fn execute_for_delivery(
        &self,
        policy: &ToolPolicy,
        method: ToolMethod,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        self.execute_staged(policy, method, params, delivery_id)
            .await
    }

    async fn execute_attributed(
        &self,
        policy: &ToolPolicy,
        method: ToolMethod,
        params: Value,
        delivery_id: &str,
        call_id: Option<String>,
    ) -> Result<Value, BrokerError> {
        match call_id {
            Some(call_id) => {
                self.capture.note_call(delivery_id, &call_id).await;
                TOOL_CALL_ID
                    .scope(
                        call_id,
                        self.execute_for_delivery(policy, method, params, delivery_id),
                    )
                    .await
            }
            None => {
                self.execute_for_delivery(policy, method, params, delivery_id)
                    .await
            }
        }
    }

    async fn commit_delivery(
        &self,
        _policy: &ToolPolicy,
        delivery_id: &str,
    ) -> Result<(), BrokerError> {
        self.capture.commit_delivery(delivery_id).await;
        Ok(())
    }

    async fn discard_delivery(&self, _policy: &ToolPolicy, delivery_id: &str) {
        self.capture.discard_delivery(delivery_id).await;
    }
}

impl AppToolExecutor {
    /// File and local dataset/index work must not occupy an async worker.
    async fn blocking_tool<F>(&self, operation: F) -> Result<Value, BrokerError>
    where
        F: FnOnce(Self) -> Result<Value, BrokerError> + Send + 'static,
    {
        let executor = self.clone();
        tokio::task::spawn_blocking(move || operation(executor))
            .await
            .map_err(|_| BrokerError::Execution("local tool execution failed".into()))?
    }

    async fn execute_staged(
        &self,
        policy: &ToolPolicy,
        method: ToolMethod,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        self.state
            .ensure_loupist_available(&self.loupist_id)
            .map_err(|_| BrokerError::Execution("Loupist is unavailable".into()))?;
        self.ensure_scope(policy)?;
        self.enforce_turn_policy(method, &params)?;
        if self.capture.unattended_run_id.is_some() {
            crate::briefings::policy::enforce(
                self.state.store.as_ref(),
                &self.loupist_id,
                self.capture.scheduled_run_id(),
                method,
                &params,
            )?;
        }
        refuse_sensitive_memory(&self.capture, method, &params)?;
        if policy.as_of.is_some() {
            self.capture
                .as_of_turn
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let mut params = params;
        let retention = method.producer().map(|producer| producer.retention);
        let max_age = if retention == Some(Retention::DatasetMaxAge) {
            take_max_age(&mut params)?
        } else {
            None
        };
        let request = params.clone();
        let persistent_data = retention.is_some_and(Retention::keeps_dataset);
        let request_key = if persistent_data {
            Some(self.persistent_request_key(policy, method, &request).await)
        } else {
            None
        };
        // An explicit max_age reuses this Loupist's identical, recent result
        // instead of calling the provider again; older revisions are refetched.
        let reused = match (max_age, request_key.as_deref()) {
            (Some(max_age), Some(key)) => self
                .state
                .store
                .dataset_fresh(&self.loupist_id, key, max_age)
                .map_err(command_error)?,
            _ => None,
        };
        let mut reused_payload = reused.as_ref().map(|(_, payload, _)| payload.clone());
        let mut result = match method {
            // A fresh-enough saved result stands in for the provider call.
            _ if reused_payload.is_some() => Ok(reused_payload.take().unwrap_or_default()),
            ToolMethod::NativeAuthorize => self.native_authorize(&params).await,
            ToolMethod::ScheduleList
            | ToolMethod::ScheduleRead
            | ToolMethod::ScheduleCreate
            | ToolMethod::ScheduleHandoff
            | ToolMethod::ScheduleWatch
            | ToolMethod::ScheduleBriefing
            | ToolMethod::ScheduleUpdate
            | ToolMethod::ScheduleSetStatus
            | ToolMethod::ScheduleDelete
            | ToolMethod::ScheduleRunNow
            | ToolMethod::ScheduleStop => self.schedule_tool(policy, method, params).await,
            ToolMethod::ResearchOpen => {
                self.capture
                    .stage_research_open(policy, params, delivery_id)
                    .await
            }
            ToolMethod::BotMemory => self.stage_bot_memory(policy, params, delivery_id).await,
            ToolMethod::ChatHistory => self.chat_history(policy, params),
            ToolMethod::DatasetExport
            | ToolMethod::DatasetDescribe
            | ToolMethod::DatasetQuery
            | ToolMethod::DatasetImport
            | ToolMethod::LensSearch => {
                let policy = policy.clone();
                self.blocking_tool(move |executor| match method {
                    ToolMethod::DatasetExport => executor.dataset_export(&policy, params),
                    ToolMethod::DatasetImport => executor.dataset_import(params),
                    ToolMethod::LensSearch => executor.lens_search(&policy, params),
                    _ => executor.dataset_tool(&policy, method, params),
                })
                .await
            }
            ToolMethod::Calculate => self.calculate(policy, params).await,
            ToolMethod::WorldbankSeries => self.worldbank_series(policy, params).await,
            ToolMethod::DefillamaRead => self.defillama_read(policy, params).await,
            ToolMethod::KrxDailyQuote => self.krx_daily_quote(policy, params).await,
            ToolMethod::KisOperationSearch => self.kis_operation_search(params).await,
            ToolMethod::KisRead => self.kis_read(policy, params).await,
            ToolMethod::DartFinancials => self.dart_financials(policy, params).await,
            ToolMethod::DartFilingSearch => self.dart_filing_search(policy, params).await,
            ToolMethod::DartFilingRead => self.dart_filing_read(policy, params).await,
            ToolMethod::DartCompanyLookup => self.dart_company_lookup(params).await,
            ToolMethod::DartMajorReport => self.dart_major_report(policy, params).await,
            ToolMethod::DartOwnership => self.dart_ownership(policy, params).await,
            ToolMethod::DartDividends => self.dart_dividends(policy, params).await,
            ToolMethod::McpConnect => self.mcp_connect(params).await,
            ToolMethod::McpCall => self.mcp_call(policy, params, delivery_id).await,
            ToolMethod::RunResultsList => self.run_results_list(params).await,
            ToolMethod::PlanUpdate => plan_update(&params),
            ToolMethod::WebSearch => self.web_search(policy, params, delivery_id).await,
            ToolMethod::WebFetch => self.web_fetch(policy, params, delivery_id).await,
            ToolMethod::SourceSave => self.stage_source_save(policy, params, delivery_id).await,
            ToolMethod::WorldSearch => self.world_search(policy, params).await,
            ToolMethod::WorldRead => self.world_read(policy, params, delivery_id).await,
            ToolMethod::LensRead => self.lens_read(policy, params, delivery_id).await,
            ToolMethod::WorldJob => self.world_job(policy, params).await,
            ToolMethod::ConversationMemory => {
                self.conversation_memory(policy, params, delivery_id).await
            }
            ToolMethod::AssetPageRead => self.asset_page_read(policy, params, delivery_id).await,
            ToolMethod::ThesisUpdate => self.stage_thesis_update(policy, params, delivery_id).await,
            ToolMethod::EarningsSnapshotSave => {
                self.stage_earnings_save(policy, params, delivery_id).await
            }
            ToolMethod::JournalSave => self.stage_journal(policy, params, delivery_id, false).await,
            ToolMethod::JournalReview => {
                self.stage_journal(policy, params, delivery_id, true).await
            }
            ToolMethod::NoteSave => self.stage_research(policy, params, delivery_id).await,
            ToolMethod::PrincipleSave => {
                self.stage_principle_save(policy, params, delivery_id).await
            }
            ToolMethod::WorldSchemaUpdate => {
                self.stage_world_schema_update(policy, params, delivery_id)
                    .await
            }
            ToolMethod::LoupistSetup => self.stage_loupist_setup(policy, params, delivery_id).await,
            ToolMethod::ConnectionSetup => self.connection_setup(params),
            ToolMethod::AskUser => self.ask_user_question(params).await,
            ToolMethod::LensFile => self.stage_lens_file(policy, params, delivery_id).await,
            ToolMethod::ArtifactList => self.artifact_list(policy, params),
            ToolMethod::ArtifactRead => self.artifact_read(policy, params, delivery_id).await,
            ToolMethod::ArtifactPublish => self.artifact_publish(policy, params, delivery_id).await,
            ToolMethod::ChartQuery => self.chart_query(policy, params).await,
            ToolMethod::ChartPublish => self.chart_publish(policy, params, delivery_id).await,
            ToolMethod::GridPublish => self.grid_publish(policy, params, delivery_id).await,
            ToolMethod::WorkspaceCapture => self.workspace_capture(params),
            ToolMethod::PythonRun => self.python_run(params).await,
        }?;
        // Holdings redaction over every tool result (D9): an account read
        // keeps no quantity, amount or price paid in any copy below, a reused
        // dataset included, and an unreviewed account read is refused.
        if method == ToolMethod::KisRead && crate::privacy::holdings::is_account_result(&result) {
            match crate::privacy::holdings::operation_id(&result)
                .and_then(|id| self.state.finance_data.kis_account_review(id))
            {
                Some(crate::privacy::holdings::AccountReview::Account(fields)) => {
                    crate::privacy::holdings::redact_fields(&mut result, &fields);
                }
                Some(crate::privacy::holdings::AccountReview::Market) => {}
                _ => {
                    return Err(BrokerError::Execution(
                        crate::privacy::holdings::UNREVIEWED.into(),
                    ))
                }
            }
        }
        // Raise the tier before any third-party bytes leave the host, so a
        // parallel or later call in this turn is checked against it.
        if crate::custom_connections::taint::carries_untrusted_text(
            method,
            &request,
            &self.state.artifacts.mcp_runtimes,
        ) {
            self.capture.turn_taint.mark_untrusted_text();
        }
        // Third-party text coming back from a saved Source or a retained
        // dataset raises it too, as when it first arrived.
        let returning = self.returning_untrusted(method, &result)?;
        if returning.is_some() {
            self.capture.turn_taint.mark_untrusted_text();
        }
        // What web_fetch may open once third-party text has arrived: the
        // locators the host delivered, never the user's own Memory text.
        self.capture
            .turn_taint
            .note_delivered_locators(method, &result);
        returning::protect_private_text(&self.capture.turn_taint, method, &result);
        let mut upstream = take_resolved_upstream(&mut result).unwrap_or_default();
        if matches!(
            method,
            ToolMethod::DatasetDescribe | ToolMethod::DatasetQuery
        ) {
            // These are new observations of a pinned revision, not the original
            // provider response. Keep the exact delivered slice citable.
            let reference = result.get("ref").cloned().ok_or(BrokerError::Malformed)?;
            let registry = self.capture.run_results.lock().await;
            upstream = registry
                .results
                .values()
                .filter(|entry| {
                    entry.payload.get("dataset_ref") == Some(&reference)
                        && !matches!(
                            entry.producer.tool_name.as_str(),
                            "dataset_describe" | "dataset_query"
                        )
                })
                .map(|entry| entry.result_ref.clone())
                .collect();
            result["dataset_ref"] = reference;
            result["title"] = json!(if method == ToolMethod::DatasetQuery {
                "Dataset slice"
            } else {
                "Dataset summary"
            });
            result["selection"] = if method == ToolMethod::DatasetQuery {
                json!({"pointer":request.get("pointer"), "offset":request.get("offset").cloned().unwrap_or(json!(0)), "limit":request.get("limit").cloned().unwrap_or(json!(100)), "columns":request.get("columns").cloned().unwrap_or(json!([]))})
            } else {
                json!({"operation":"bounded_schema_and_sample"})
            };
            result["coverage"] = json!(if method == ToolMethod::DatasetQuery {
                "selected dataset slice"
            } else {
                "bounded dataset schema and sample"
            });
        }
        let compact = persistent_data && result.to_string().len() > 16 * 1024;
        let mut dataset_ref = None;
        if let Some(request_key) = request_key {
            let (reference, fetched_at, reused) = match reused {
                Some((reference, _, metadata)) => (
                    reference,
                    metadata.get("fetched_at").cloned().unwrap_or(Value::Null),
                    true,
                ),
                None => {
                    let (reference, fetched_at) = self.retain_dataset(
                        &request_key, &result,
                        json!({"producer": format!("{method:?}"), "request": request, "as_of": effective_as_of(policy, &request)?.map(|cutoff| cutoff.to_string()), "upstream_result_refs": upstream, "coverage": {"retained":"complete provider result", "model": if compact {"bounded sample"} else {"complete"}},
                            "trust": crate::custom_connections::taint::retained_trust(method, &request, &self.state.artifacts.mcp_runtimes)}),
                    )?;
                    (reference, json!(fetched_at), false)
                }
            };
            if let Some(object) = result.as_object_mut() {
                object.insert("dataset_ref".into(), json!(reference));
                object.insert("fetched_at".into(), fetched_at);
                if reused {
                    object.insert("reused".into(), json!(true));
                }
                object.insert("model_delivery".into(), json!({"coverage":if compact {"bounded_sample"}else{"complete"}, "retained":"complete", "compute_inputs":"pinned dataset revision, optional pointer selection"}));
            }
            dataset_ref = Some(reference);
        }
        let mut model_payload = if compact {
            if let Some(reference) = dataset_ref {
                let description = self
                    .state
                    .store
                    .dataset_describe(&self.loupist_id, &reference.dataset_id, reference.revision)
                    .map_err(command_error)?;
                Some(
                    json!({"dataset_ref":reference, "fetched_at":result.get("fetched_at"), "data_summary":description, "receipt":result.get("receipt"), "quality":result.get("quality"), "warnings":result.get("warnings"), "model_delivery":result.get("model_delivery")}),
                )
            } else {
                None
            }
        } else {
            None
        };
        if let Some(producer) = run_result_producer(method, &request, &result) {
            let result_ref = self
                .capture
                .stage_run_result_with_delivery(
                    delivery_id,
                    producer,
                    &request,
                    result.clone(),
                    upstream,
                    model_payload.as_ref(),
                )
                .await?;
            attach_result_ref(&mut result, &result_ref);
            if let Some(delivered) = &mut model_payload {
                attach_result_ref(delivered, &result_ref);
            }
        }
        // Full datasets stay in the host; large model-facing views are bounded.
        let mut delivered = model_payload.unwrap_or(result);
        #[cfg(debug_assertions)]
        crate::privacy::holdings::debug_scan(
            &self.state.store,
            "a tool result",
            &delivered.to_string(),
        );
        crate::custom_connections::taint::delimit_untrusted_bodies(
            method,
            &request,
            &self.state.artifacts.mcp_runtimes,
            &mut delivered,
        );
        if let Some(returning) = &returning {
            crate::custom_connections::taint::delimit_returning(method, returning, &mut delivered);
        }
        if method.safety().pack_text == PackText::Delimited {
            // Text from shared packs arrives delimited and pauses commands for this turn.
            crate::sharing::delivery::deliver(
                &self.state,
                &self.loupist_id,
                &mut delivered,
                &self.capture.turn_taint,
            );
        }
        if self.capture.turn_taint.has_untrusted_content() {
            self.capture.tool_trace.mark_untrusted_text();
        }
        Ok(delivered)
    }
}

impl AppToolExecutor {
    /// The reuse and dataset key of a persistent finance read. A KIS read also
    /// names the credential revision, account and environment that serve it,
    /// as a digest and never a secret, so `max_age` cannot hand one account's
    /// balances to a read made after switching accounts.
    pub(super) async fn persistent_request_key(
        &self,
        policy: &ToolPolicy,
        method: ToolMethod,
        request: &Value,
    ) -> String {
        let mut key = json!({"method":format!("{method:?}"),"request":request,"as_of":policy.as_of,"capabilities":self.capability_ids,"schema":"finance-dataset/1"});
        if method == ToolMethod::KisRead {
            key["provider_identity"] = json!(self.kis_reuse_identity().await);
        }
        sha256(key.to_string().as_bytes())
    }
}

pub(super) const RESOLVED_UPSTREAM_FIELD: &str = "_resolved_upstream_result_refs";

fn take_resolved_upstream(result: &mut Value) -> Option<Vec<String>> {
    let object = result.as_object_mut()?;
    let value = object.remove(RESOLVED_UPSTREAM_FIELD)?;
    let mut unique = BTreeSet::new();
    let mut refs = Vec::new();
    for item in value.as_array().into_iter().flatten() {
        let Some(text) = item.as_str() else {
            continue;
        };
        if text.is_empty() || text.len() > 128 || !unique.insert(text) {
            continue;
        }
        refs.push(text.to_owned());
    }
    Some(refs)
}

fn run_result_producer(
    method: ToolMethod,
    request: &Value,
    result: &Value,
) -> Option<RunResultProducer> {
    let native = method.producer()?;
    let provider = match method {
        ToolMethod::WebSearch => result
            .get("selected_provider")
            .and_then(Value::as_str)
            .map(str::to_owned),
        ToolMethod::WebFetch => result
            .get("representation_provider")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                result
                    .get("final_url")
                    .or_else(|| result.get("url"))
                    .and_then(Value::as_str)
                    .and_then(|url| reqwest::Url::parse(url).ok())
                    .and_then(|url| url.host_str().map(str::to_owned))
            }),
        _ => result
            .get("source_id")
            .or_else(|| result.get("provider"))
            .or_else(|| request.get("provider"))
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
    .filter(|value| !value.is_empty() && value.len() <= 256);
    Some(RunResultProducer {
        origin: native.origin,
        runtime_id: native.runtime.into(),
        tool_name: method.tool_name(),
        provider,
    })
}

fn attach_result_ref(result: &mut Value, result_ref: &str) {
    if let Some(object) = result.as_object_mut() {
        object.insert("result_ref".into(), Value::String(result_ref.to_owned()));
        object.insert(
            "citation_url".into(),
            json!(evidence::citation_url(result_ref)),
        );
    } else {
        let payload = std::mem::take(result);
        *result = json!({ "result_ref": result_ref, "citation_url": evidence::citation_url(result_ref), "data": payload });
    }
}

/// World node ids an acknowledged World read delivered, in delivery order, bounded for progress.
fn delivered_world_node_ids(reads: &[StagedKnowledgeRead]) -> Vec<String> {
    let mut ids = Vec::new();
    for read in reads {
        let AckedPayload::World { view_json, .. } = &read.payload else {
            continue;
        };
        let Ok(view) = serde_json::from_str::<serde_json::Value>(view_json) else {
            continue;
        };
        for node in view
            .get("nodes")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            if ids.len() >= crate::chat::progress::MAX_PROGRESS_ENTITY_IDS {
                return ids;
            }
            if let Some(id) = node.get("id").and_then(serde_json::Value::as_str) {
                if !ids.iter().any(|existing: &String| existing == id) {
                    ids.push(id.to_owned());
                }
            }
        }
    }
    ids
}

#[cfg(test)]
mod tests;

impl ToolCapture {
    /// The scheduled run whose row records refused tools; a delegated hand-off
    /// run is unattended too but has no such row.
    fn scheduled_run_id(&self) -> Option<&str> {
        match self.surface {
            Some(crate::chat::surface::ChatSurface::Scheduled) => self.unattended_run_id.as_deref(),
            _ => None,
        }
    }

    fn record_tool_refusal(&self, refusal: crate::domain::ChatToolRefusal) {
        let mut entries = self.tool_refusals.lock().unwrap_or_else(|e| e.into_inner());
        if entries.len() < 64 && !entries.contains(&refusal) {
            entries.push(refusal);
        }
    }
}

fn any_credential_text(value: &Value) -> bool {
    match value {
        Value::String(text) => crate::util::credential_text::contains_credential_text(text),
        Value::Array(items) => items.iter().any(any_credential_text),
        Value::Object(map) => map.values().any(any_credential_text),
        _ => false,
    }
}

/// Memory never keeps text the Loupist wrote that looks like a credential or
/// secret. The write is refused, the reply goes on, and the refusal is kept on
/// the message so the Chat can say once that something was not saved.
/// Retained Sources are excluded: a fetched page may quote an example key.
fn refuse_sensitive_memory(
    capture: &ToolCapture,
    method: ToolMethod,
    params: &Value,
) -> Result<(), BrokerError> {
    if !method.safety().memory.screened_for_credentials() || !any_credential_text(params) {
        return Ok(());
    }
    capture.record_tool_refusal(crate::domain::ChatToolRefusal {
        connection_id: "memory".into(),
        connection_name: "Memory".into(),
        tool_name: method.tool_name(),
        reason: crate::domain::ChatToolRefusalReason::SensitiveMemory,
    });
    Err(BrokerError::Execution(
        "Not saved: this looks like a credential, token or other secret, and Memory never keeps those. Continue without saving it; do not retry with the same text.".into(),
    ))
}

/// The plan itself is projected from the call into chat progress; the tool only
/// checks the arguments and acknowledges them.
fn plan_update(params: &Value) -> Result<Value, BrokerError> {
    let items = crate::chat::progress::parse_plan_update(params)
        .map_err(|(field, expected)| BrokerError::InvalidArguments { field, expected })?;
    Ok(serde_json::json!({ "status": "plan_updated", "items": items.len() }))
}
