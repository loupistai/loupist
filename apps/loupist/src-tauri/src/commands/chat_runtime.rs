//! One Chat turn, in phases: `setup` admits the run and appends the user
//! turn, `launch` starts the broker and Pi, `stream` runs the event loop and
//! `finalize` (with `completion`) settles Pi and records the terminal reply.
mod completion;
mod controls;
mod finalize;
mod launch;
mod model_gone;
mod pi_dispatch;
mod pi_launch;
mod progress;
mod setup;
mod stream;
mod terminal;
mod watchdog;

pub(super) use controls::*;
pub(super) use pi_launch::*;
pub(super) use terminal::*;
use watchdog::*;

use progress::{
    emit_chat_progress_snapshot, finish_chat_progress, publish_progress, CommentaryCoalescer,
};

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    ops::ControlFlow,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::Utc;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use tauri::{ipc::Channel, State};

use crate::{
    agent_harness::{self, AgentHarnessSnapshot},
    app::{AppState, CommandError},
    broker::{start_tool_broker, tool_broker_endpoint},
    chat::control::{
        chat_control_channel, AcceptedChatControl, ChatControlError, ChatControlReceiver,
        ChatControlRequest,
    },
    chat::execution_session::ChatExecutionSession,
    chat::progress::{
        summarize_tool_result, ChatProgress, ChatProgressOperation, ChatProgressProjection,
    },
    chat::turn::ChatTurnResources,
    domain::{
        memory_refs_digest, ChatMessage, ChatMessageStatus, ChatRole, MemoryAccess, MemoryPolicy,
        MemoryRefSnapshot, PiSessionCache, PiSessionCacheScope, MAX_MEMORY_REFS,
    },
    pi::execution::{
        configure_pi_session_and_read_entries, read_pi_entries, PiEntriesState, PiExecutionConfig,
        PiSessionFileRequirement, PiSessionState,
    },
    pi::response::{AssistantTurnCapture, AssistantTurnEnd},
    pi::{PiError, PiEvent, PiImageContent, PiLaunchConfig, PiProcess},
    providers::catalog::ExecutionModelLock,
    runs::coordinator::{RunKind, RunRegistration, RunTarget},
};
use tokio::time::{timeout, Instant};

use super::{
    attachments::{
        attachment_prompt, persist_chat_attachments, pi_chat_turn_prompt, prepare_chat_attachments,
        ColdChatBootstrap,
    },
    capture_chat_connector_authority, fallback_chat_title, iso_time, map_internal, map_store,
    memory_updates, new_id, now_ms, require_text,
    tool_executor::{AppToolExecutor, ToolCapture},
    types::{
        memory_ref_dto, ChatControlModeDto, ChatControlReceiptDto, ChatControlRequestDto,
        ChatPerformanceDto, ChatRunPhase, ChatSendRequest, ChatSessionCacheStatusDto,
        ChatStreamEvent, RunStarted,
    },
    MAX_CHAT_OUTPUT_BYTES, MAX_PROMPT_BYTES,
};

use crate::domain::ChatFailureKind;

type ChatEventCallback = dyn Fn(ChatStreamEvent) -> Result<(), ()> + Send + Sync;
type ChatRunLogCapture =
    Arc<std::sync::Mutex<Option<(Arc<crate::store::SqliteStore>, crate::runs::log::RunLog)>>>;
type ChatBaselineCallback = dyn Fn(&crate::domain::ChatSession) + Send + Sync;
#[cfg(feature = "eval-harness")]
type EvalPiEventCallback = dyn Fn(&Value) + Send + Sync;

#[derive(Clone)]
pub(crate) struct ChatEventSink {
    #[cfg(test)]
    pub(crate) provider_idle_timeout: Option<Duration>,
    #[cfg(feature = "eval-harness")]
    pub(crate) eval_pi_event: Option<Arc<EvalPiEventCallback>>,
    send: Arc<ChatEventCallback>,
    baseline: Option<Arc<ChatBaselineCallback>>,
    evidence: Arc<std::sync::Mutex<Vec<crate::domain::ChatEvidenceRef>>>,
    tool_refusals: Arc<std::sync::Mutex<Vec<crate::domain::ChatToolRefusal>>>,
    schedules: Arc<std::sync::Mutex<Vec<crate::scheduled_tasks::types::ScheduleReceipt>>>,
    tool_trace: Arc<crate::broker::ToolTraceCapture>,
    failure_kind: Arc<std::sync::Mutex<ChatFailureKind>>,
    retry_after_seconds: Arc<std::sync::Mutex<Option<u32>>>,
    run_log: ChatRunLogCapture,
    /// Provider status (W17): each terminal outcome on the run's credential.
    provider_outcome: Option<Arc<ProviderOutcomeCallback>>,
    replaced_model: Arc<std::sync::Mutex<Option<String>>>,
}

type ProviderOutcomeCallback =
    dyn Fn(&crate::providers::catalog::ExecutionModelLock, Option<ChatFailureKind>) + Send + Sync;

impl ChatEventSink {
    pub(crate) fn new(
        send: impl Fn(ChatStreamEvent) -> Result<(), ()> + Send + Sync + 'static,
    ) -> Self {
        Self {
            #[cfg(test)]
            provider_idle_timeout: None,
            #[cfg(feature = "eval-harness")]
            eval_pi_event: None,
            send: Arc::new(send),
            baseline: None,
            evidence: Arc::default(),
            schedules: Arc::default(),
            tool_refusals: Arc::default(),
            tool_trace: Arc::default(),
            failure_kind: Arc::default(),
            retry_after_seconds: Arc::default(),
            run_log: Arc::default(),
            provider_outcome: None,
            replaced_model: Arc::default(),
        }
    }

    pub(crate) fn with_provider_outcome(
        mut self,
        callback: impl Fn(&crate::providers::catalog::ExecutionModelLock, Option<ChatFailureKind>)
            + Send
            + Sync
            + 'static,
    ) -> Self {
        self.provider_outcome = Some(Arc::new(callback));
        self
    }

    pub(crate) fn with_baseline(
        mut self,
        callback: impl Fn(&crate::domain::ChatSession) + Send + Sync + 'static,
    ) -> Self {
        self.baseline = Some(Arc::new(callback));
        self
    }

    pub(crate) fn send(&self, event: ChatStreamEvent) -> Result<(), ()> {
        if let ChatStreamEvent::Schedule { receipt, .. } = &event {
            let mut receipts = self
                .schedules
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(previous) = receipts
                .iter_mut()
                .find(|previous| previous.id() == receipt.id())
            {
                *previous = receipt.clone();
            } else if receipts.len() < crate::scheduled_tasks::types::MAX_TASKS_PER_LOUPIST {
                receipts.push(receipt.clone());
            }
        }
        if let ChatStreamEvent::Evidence { evidence, .. } = &event {
            let mut snapshot = self
                .evidence
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(prior) = snapshot.iter_mut().find(|prior| prior.id == evidence.id) {
                *prior = evidence.as_ref().clone();
            } else if snapshot.len() < crate::domain::MAX_CHAT_EVIDENCE_REFS {
                snapshot.push(evidence.as_ref().clone());
            }
        }
        if let Some(outcome) = &self.provider_outcome {
            match &event {
                ChatStreamEvent::Completed {
                    execution_model, ..
                } => outcome(execution_model, None),
                ChatStreamEvent::Error {
                    execution_model,
                    failure_kind,
                    ..
                } => outcome(execution_model, Some(*failure_kind)),
                _ => {}
            }
        }
        {
            let mut pending = self.run_log.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((_, record)) = pending.as_mut() {
                match &event {
                    ChatStreamEvent::MemoryUpdate { result, .. } => record.memory(result),
                    ChatStreamEvent::Completed {
                        execution_model,
                        performance,
                        ..
                    } => {
                        record.model(execution_model);
                        record.usage = Some(crate::runs::log::Usage {
                            input: performance
                                .input_tokens
                                .saturating_add(performance.cache_write_tokens),
                            cached_input: performance.cache_read_tokens,
                            output: performance.output_tokens,
                        });
                        record.finish("ok", None);
                    }
                    ChatStreamEvent::Error { failure_kind, .. } => {
                        record.finish("failed", Some(&crate::runs::log::enum_name(failure_kind)))
                    }
                    ChatStreamEvent::Aborted { .. } => record.finish("cancelled", None),
                    _ => {}
                }
            }
            if matches!(
                &event,
                ChatStreamEvent::Completed { .. }
                    | ChatStreamEvent::Error { .. }
                    | ChatStreamEvent::Aborted { .. }
            ) {
                if let Some((store, mut record)) = pending.take() {
                    record.refusals(&self.tool_refusals());
                    record.trace(&self.tool_trace.snapshot());
                    record.record(store.as_ref());
                }
            }
        }
        (self.send)(event)
    }

    fn set_retry_after_seconds(&self, seconds: Option<u32>) {
        *self
            .retry_after_seconds
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = seconds;
    }

    /// The provider's bounded wait, only for an answer that failed as busy.
    fn retry_after_seconds(&self) -> Option<u32> {
        if self.failure_kind() != ChatFailureKind::RateLimited {
            return None;
        }
        *self
            .retry_after_seconds
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn set_failure_kind(&self, kind: ChatFailureKind) {
        *self
            .failure_kind
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = kind;
    }

    fn failure_kind(&self) -> ChatFailureKind {
        *self
            .failure_kind
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn tool_refusals(&self) -> Vec<crate::domain::ChatToolRefusal> {
        self.tool_refusals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// The model this turn's one Auto retry replaced (W17.5).
    fn set_replaced_model(&self, name: &str) {
        *self
            .replaced_model
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(name.chars().take(80).collect());
    }

    fn replaced_model(&self) -> Option<String> {
        self.replaced_model
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn schedule_receipts(&self) -> Vec<crate::scheduled_tasks::types::ScheduleReceipt> {
        self.schedules
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn evidence_refs(&self) -> Vec<crate::domain::ChatEvidenceRef> {
        self.evidence
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn admitted(&self, chat: &crate::domain::ChatSession) {
        if let Some(callback) = &self.baseline {
            callback(chat);
        }
    }
}

impl From<Channel<ChatStreamEvent>> for ChatEventSink {
    fn from(channel: Channel<ChatStreamEvent>) -> Self {
        Self::new(move |event| channel.send(event).map_err(|_| ()))
    }
}

fn pi_event_stream_failure(error: tokio::sync::broadcast::error::RecvError) -> &'static str {
    match error {
        tokio::sync::broadcast::error::RecvError::Lagged(_) => {
            "Pi event stream fell behind and was stopped safely"
        }
        tokio::sync::broadcast::error::RecvError::Closed => "Pi event stream was interrupted",
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct AssistantTokenUsage {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}

impl AssistantTokenUsage {
    fn observe_message_end(&mut self, payload: &Value) {
        let Some(usage) = payload
            .get("message")
            .and_then(|message| message.get("usage"))
        else {
            return;
        };
        self.input = self
            .input
            .saturating_add(usage.get("input").and_then(Value::as_u64).unwrap_or(0));
        self.output = self
            .output
            .saturating_add(usage.get("output").and_then(Value::as_u64).unwrap_or(0));
        self.cache_read = self
            .cache_read
            .saturating_add(usage.get("cacheRead").and_then(Value::as_u64).unwrap_or(0));
        self.cache_write = self
            .cache_write
            .saturating_add(usage.get("cacheWrite").and_then(Value::as_u64).unwrap_or(0));
    }
}

fn elapsed_millis(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn chat_steer(
    request: ChatControlRequestDto,
    state: State<'_, AppState>,
) -> Result<ChatControlReceiptDto, CommandError> {
    let loupist_id = require_text(&request.loupist_id, "Loupist", 512)?;
    let thread_id = require_text(&request.thread_id, "Chat thread", 512)?;
    let prompt = require_text(&request.prompt, "prompt", MAX_PROMPT_BYTES)?;
    let accepted = state
        .submit_chat_control(&loupist_id, &thread_id, prompt.clone())
        .await?;
    Ok(ChatControlReceiptDto {
        message_id: accepted.message_id,
        prompt,
        created_at: iso_time(accepted.created_at_ms)?,
        mode: ChatControlModeDto::Steer,
    })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn chat_send(
    request: ChatSendRequest,
    on_event: Channel<ChatStreamEvent>,
    state: State<'_, AppState>,
) -> Result<RunStarted, CommandError> {
    send_chat(request, ChatEventSink::from(on_event), state.inner(), None).await
}

/// A registered Chat owns both its cancellation identity and model capacity.
/// Scheduled claims obtain this while their SQLite transaction is still open,
/// so a busy target rolls back the occurrence instead of consuming it.
pub(crate) struct ChatAdmission {
    run_id: String,
    loupist_id: String,
    thread_id: String,
    registration: RunRegistration,
    controls: ChatControlReceiver,
    pub(crate) untrusted_text: bool,
}

impl ChatAdmission {
    pub(crate) fn new(
        state: &AppState,
        run_id: &str,
        loupist_id: &str,
        thread_id: &str,
    ) -> Result<Self, CommandError> {
        let run_id = validated_chat_run_id(run_id)?;
        require_text(loupist_id, "Loupist", 512)?;
        require_text(thread_id, "Chat thread", 512)?;
        let (control, controls) = chat_control_channel();
        let registration = state.register_chat_run(
            run_id.clone(),
            loupist_id.to_owned(),
            thread_id.to_owned(),
            control,
        )?;
        Ok(Self {
            run_id,
            loupist_id: loupist_id.to_owned(),
            thread_id: thread_id.to_owned(),
            registration,
            controls,
            untrusted_text: false,
        })
    }
}

/// One execution path for interactive and scheduled Chat turns. The sink owns
/// presentation delivery; admission, persistence and tool authority stay here.
pub(crate) async fn send_chat(
    request: ChatSendRequest,
    on_event: ChatEventSink,
    state: &AppState,
    admission: Option<ChatAdmission>,
) -> Result<RunStarted, CommandError> {
    let on_event = on_event.with_provider_outcome({
        let state = state.clone();
        move |model, failure| crate::providers::status::record_chat_outcome(&state, model, failure)
    });
    // Sources this turn saves reach the Map before an older backlog.
    state
        .graph
        .note_attention(&request.loupist_id, &request.thread_id);
    let mut record =
        crate::runs::log::RunLog::new("chat", &request.loupist_id, &request.run_id, now_ms());
    record.chat_id = Some(request.thread_id.clone());
    *on_event.run_log.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((state.store.clone(), record));
    let result = send_chat_inner(request, on_event.clone(), state, admission).await;
    if let Err(error) = &result {
        if let Some((store, mut record)) = on_event
            .run_log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            record.finish(
                "failed",
                Some(&crate::runs::log::enum_name(&classify_turn_failure(
                    error,
                    ChatFailureKind::Other,
                ))),
            );
            record.notes.push("Chat admission or setup failed.".into());
            record.record(store.as_ref());
        }
    }
    result
}

/// Runs one Chat turn through its phases. Setup and launch run inline, so a
/// failure there settles before this command returns; stream and finalize run
/// on a spawned task that owns the live turn until its reply is terminal.
async fn send_chat_inner(
    request: ChatSendRequest,
    on_event: ChatEventSink,
    state: &AppState,
    admission: Option<ChatAdmission>,
) -> Result<RunStarted, CommandError> {
    let admitted = match setup::admit_chat_turn(request, on_event, state, admission)? {
        ControlFlow::Continue(admitted) => admitted,
        ControlFlow::Break(settled) => return Ok(settled),
    };
    let mut launched = match launch::launch_chat_turn(admitted, state).await? {
        ControlFlow::Continue(launched) => launched,
        ControlFlow::Break(settled) => return Ok(settled),
    };
    let run_id = launched.run_id.clone();
    tauri::async_runtime::spawn(async move {
        let mut streamed = stream::stream_chat_turn(&mut launched).await;
        if model_gone::retryable(&launched, &streamed) {
            launched = match model_gone::relaunch_on_auto(launched).await {
                model_gone::Relaunch::Retried(relaunched) => {
                    let mut relaunched = *relaunched;
                    // The renderer's progress continues in sequence.
                    relaunched.carried_progress = Some(streamed.progress);
                    streamed = stream::stream_chat_turn(&mut relaunched).await;
                    relaunched
                }
                model_gone::Relaunch::Unchanged(unchanged) => *unchanged,
                model_gone::Relaunch::Settled => return,
            };
        }
        let surface = launched.capture.surface.clone();
        let (state, loupist_id) = (launched.app_state.clone(), launched.loupist_id.clone());
        finalize::finalize_chat_turn(launched, streamed).await;
        if let Some(surface) = surface {
            surface.after_turn(&state, &loupist_id);
        }
    });
    Ok(RunStarted { run_id })
}

#[cfg(test)]
mod tests;

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ChatRetryRequest {
    loupist_id: String,
    thread_id: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChatRetryReceipt {
    prompt: String,
    /// The as-of date the request was sent with; Retry repeats it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    as_of: Option<String>,
}

/// Drops the latest failed or stopped reply and its original user request,
/// preserving accepted steers, then clears the derived Pi cache. The caller
/// sends the returned prompt again. Workspace files already written by that
/// turn stay. Pins are not stored on the message.
#[tauri::command(rename_all = "snake_case")]
pub fn chat_retry_last(
    request: ChatRetryRequest,
    state: State<'_, AppState>,
) -> Result<ChatRetryReceipt, CommandError> {
    require_text(&request.loupist_id, "Loupist", 512)?;
    require_text(&request.thread_id, "Chat thread", 512)?;
    state.ensure_loupist_available(&request.loupist_id)?;
    // Hold the thread until the transcript is replaced. A busy check that
    // releases the lock first lets a new run start before the deletion lands.
    let _mutation = state.register_run(
        new_id("chat-mutation"),
        request.loupist_id.clone(),
        RunKind::ChatMutation,
        RunTarget::ChatThread(request.thread_id.clone()),
    )?;
    let chat = state
        .store
        .get_chat(&request.thread_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("chat thread"))?;
    if chat.loupist_id != request.loupist_id {
        return Err(CommandError::conflict(
            "chat thread does not belong to the selected Loupist",
        ));
    }
    let (prompt, next) = crate::domain::prepare_chat_retry(&chat, now_ms()).map_err(|error| {
        CommandError::invalid(match error {
            crate::domain::DomainError::Invalid(message) => message,
        })
    })?;
    let as_of = chat
        .messages
        .iter()
        .rev()
        .find(|message| message.role == ChatRole::User && !message.steer)
        .and_then(|message| message.as_of.clone());
    state.store.replace_chat(&chat, &next).map_err(map_store)?;
    Ok(ChatRetryReceipt { prompt, as_of })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn chat_abort(run_id: String, state: State<'_, AppState>) -> Result<(), CommandError> {
    validated_chat_run_id(&run_id)?;
    state.cancel_run(&run_id, RunKind::Chat).await
}

fn validated_chat_run_id(value: &str) -> Result<String, CommandError> {
    crate::runs::id::validate_run_id(value, "Chat")
}

/// Classify only host-owned diagnostics; the persisted value is an enum allowlist.
fn classify_turn_failure(error: &CommandError, fallback: ChatFailureKind) -> ChatFailureKind {
    if error.code == "conflict" {
        return ChatFailureKind::CancelledByConflict;
    }
    // Store/Memory boundaries classify their own errors without interpreting private details.
    if matches!(
        fallback,
        ChatFailureKind::StoreFailed | ChatFailureKind::MemoryCommitFailed
    ) {
        return fallback;
    }
    match error.message.as_str() {
        "Pi settled without an assistant message"
        | "Pi settled without authoritative assistant text"
        | "Pi settled with an incomplete tool request" => ChatFailureKind::EmptyReply,
        crate::pi::provider_failure::RATE_LIMITED => ChatFailureKind::RateLimited,
        crate::pi::provider_failure::NEEDS_RECONNECT => ChatFailureKind::NeedsReconnect,
        crate::pi::provider_failure::PLAN_LIMIT => ChatFailureKind::PlanLimit,
        crate::pi::provider_failure::PLAN_NOT_ELIGIBLE => ChatFailureKind::PlanNotEligible,
        crate::pi::provider_failure::OUT_OF_CREDIT => ChatFailureKind::OutOfCredit,
        crate::pi::provider_failure::MODEL_GONE => ChatFailureKind::ModelGone,
        crate::pi::provider_failure::NETWORK => ChatFailureKind::Offline,
        crate::pi::provider_failure::UNAVAILABLE => ChatFailureKind::ProviderUnavailable,
        crate::pi::provider_failure::CONTEXT_TOO_LONG => ChatFailureKind::ContextTooLong,
        crate::pi::provider_failure::OTHER | "Pi rejected the prompt" => {
            ChatFailureKind::ProviderError
        }
        "Pi assistant response reached its length limit"
        | "Pi assistant message exceeded its output limit" => ChatFailureKind::OutputLimit,
        reason if reason.starts_with("Pi stopped before the turn settled") => {
            ChatFailureKind::PiExited
        }
        "Pi did not emit provider body progress"
        | "Pi did not finish pre-body compaction"
        | "Pi stopped responding while waiting for the model"
        | CHAT_TOOL_SILENCE_FAILURE
        | CHAT_COMPACTION_TIMEOUT_FAILURE
        | "Pi session initialization timed out"
        | "Pi RPC write timed out" => ChatFailureKind::ProviderTimeout,
        reason
            if reason.starts_with("Pi runtime directory is missing")
                || reason.starts_with("Pi I/O failed")
                || reason == "Pi stopped during session initialization" =>
        {
            ChatFailureKind::PiExited
        }
        reason if reason.starts_with("Pi ") => ChatFailureKind::ProtocolError,
        _ => fallback,
    }
}
