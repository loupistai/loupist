//! Terminal Chat outcomes: the durable Memory trace, completed-reply events,
//! and the aborted/failed assistant messages every early exit persists.
use super::*;

#[derive(Clone)]
pub(in crate::commands) struct DurableMemoryTrace {
    pub(in crate::commands) refs: Vec<MemoryRefSnapshot>,
    pub(in crate::commands) observed_exact_count: u64,
    pub(in crate::commands) refs_truncated: bool,
    pub(in crate::commands) refs_digest: String,
}

pub(in crate::commands) fn durable_memory_trace(
    memories: impl IntoIterator<Item = MemoryRefSnapshot>,
) -> Result<DurableMemoryTrace, CommandError> {
    let all = memories
        .into_iter()
        .filter(|memory| memory.access == MemoryAccess::ExactRead)
        .map(|memory| (memory.read_ref.clone(), memory))
        .collect::<BTreeMap<_, _>>();
    let all_refs = all.into_values().collect::<Vec<_>>();
    let observed_exact_count = u64::try_from(all_refs.len())
        .map_err(|_| CommandError::internal("exact Memory read count overflowed"))?;
    let refs_digest = memory_refs_digest(&all_refs).map_err(map_internal)?;

    let refs = all_refs
        .into_iter()
        .take(MAX_MEMORY_REFS)
        .collect::<Vec<_>>();
    Ok(DurableMemoryTrace {
        refs_truncated: observed_exact_count > refs.len() as u64,
        refs,
        observed_exact_count,
        refs_digest,
    })
}

pub(super) fn empty_memory_trace() -> Result<DurableMemoryTrace, CommandError> {
    durable_memory_trace(Vec::new())
}

pub(in crate::commands) fn emit_completed_chat(
    on_event: &ChatEventSink,
    run_id: &str,
    message: &ChatMessage,
    title: Option<String>,
    execution_model: &ExecutionModelLock,
    agent_harness: &AgentHarnessSnapshot,
    performance: ChatPerformanceDto,
) {
    if !message.memory_refs.is_empty() {
        let _ = on_event.send(ChatStreamEvent::Memory {
            run_id: run_id.to_owned(),
            memories: message.memory_refs.iter().map(memory_ref_dto).collect(),
        });
    }
    if let Some(title) = title {
        let _ = on_event.send(ChatStreamEvent::Title {
            run_id: run_id.to_owned(),
            title,
        });
    }
    if let Some(result) = message.memory_update.clone() {
        let _ = on_event.send(ChatStreamEvent::MemoryUpdate {
            run_id: run_id.to_owned(),
            result: Box::new(result),
        });
    }
    for evidence in &message.evidence_refs {
        let _ = on_event.send(ChatStreamEvent::Evidence {
            run_id: run_id.to_owned(),
            evidence: Box::new(evidence.clone()),
        });
    }
    for artifact in &message.artifact_refs {
        let _ = on_event.send(ChatStreamEvent::Artifact {
            run_id: run_id.to_owned(),
            artifact: artifact.clone(),
        });
    }
    let _ = on_event.send(ChatStreamEvent::Completed {
        lens_receipts: message.lens_receipts.clone(),
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
        host_composed: message.host_composed,
        run_id: run_id.to_owned(),
        message_id: message.id.clone(),
        final_text: message.content.clone(),
        created_at: iso_time(message.created_at_ms).unwrap_or_default(),
        execution_model: Box::new(execution_model.clone()),
        agent_harness: Box::new(agent_harness.clone()),
        performance,
        replaced_model: message.replaced_model.clone(),
        as_of: message.as_of.clone(),
        number_checks: message.number_checks.clone(),
        sensitivity_cards: message.sensitivity_cards.clone(),
        as_of_report: message.as_of_report.clone().map(Box::new),
    });
}

pub(in crate::commands) struct CanonicalCompletionExpectation<'a> {
    pub content: &'a str,
    pub memory_revision: &'a Option<String>,
    pub execution_model: &'a ExecutionModelLock,
    pub agent_harness: &'a AgentHarnessSnapshot,
    pub performance: &'a ChatPerformanceDto,
    pub title: Option<&'a str>,
}

pub(in crate::commands) fn recovered_canonical_completion(
    store: &dyn LoupistStore,
    thread_id: &str,
    message_id: &str,
    expected: CanonicalCompletionExpectation<'_>,
) -> Option<(ChatMessage, Option<String>)> {
    let chat = store.get_chat(thread_id).ok()??;
    let message = chat
        .messages
        .iter()
        .find(|message| message.id == message_id)?;
    if message.role != ChatRole::Assistant
        || message.status != ChatMessageStatus::Complete
        || message.content != expected.content
        || &message.memory_revision != expected.memory_revision
        || message.execution_model.as_ref() != Some(expected.execution_model)
        || message.agent_harness.as_ref() != Some(expected.agent_harness)
        || message.performance.as_ref() != Some(expected.performance)
    {
        return None;
    }
    let title = expected
        .title
        .filter(|expected| chat.title.as_str() == *expected)
        .map(|_| chat.title.clone());
    Some((message.clone(), title))
}

pub(in crate::commands) const FAILED_CHAT_CONTENT: &str = "Response could not be completed.";
#[derive(Clone, Copy)]
pub(super) enum ChatTerminalStatus {
    Aborted,
    Error,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn persist_and_emit_terminal_chat(
    store: &dyn LoupistStore,
    on_event: &ChatEventSink,
    run_id: &str,
    thread_id: &str,
    message_id: &str,
    content: String,
    memory_trace: DurableMemoryTrace,
    memory_revision: Option<String>,
    execution_model: ExecutionModelLock,
    agent_harness: AgentHarnessSnapshot,
    progress: Option<ChatProgress>,
    terminal_status: ChatTerminalStatus,
) -> Result<RunStarted, CommandError> {
    let expected = store
        .get_chat(thread_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("chat thread"))?;
    let created_at_ms = now_ms().max(expected.updated_at_ms + 1);
    let mut chat = expected.clone();
    chat.messages.push(ChatMessage {
        tool_refusals: on_event.tool_refusals(),
        performance: None,
        feedback: None,
        tool_trace: Some(on_event.tool_trace.snapshot()),
        schedule_receipts: on_event.schedule_receipts(),
        failure_kind: matches!(terminal_status, ChatTerminalStatus::Error)
            .then(|| on_event.failure_kind()),
        host_composed: false,
        steer: false,
        retry_after_seconds: on_event.retry_after_seconds(),
        delivery: None,
        replaced_model: None,
        as_of: None,
        number_checks: Vec::new(),
        sensitivity_cards: Vec::new(),
        as_of_report: None,
        alert: None,
        watch: None,
        earnings: None,
        vocabulary: None,
        id: message_id.to_owned(),
        role: ChatRole::Assistant,
        status: match terminal_status {
            ChatTerminalStatus::Aborted => ChatMessageStatus::Aborted,
            ChatTerminalStatus::Error => ChatMessageStatus::Error,
        },
        content: content.clone(),
        created_at_ms,
        lens_receipts: Vec::new(),
        memory_refs: memory_trace.refs,
        observed_exact_count: memory_trace.observed_exact_count,
        refs_truncated: memory_trace.refs_truncated,
        refs_digest: memory_trace.refs_digest,
        memory_update: None,
        memory_revision,
        execution_model: Some(execution_model.clone()),
        agent_harness: Some(agent_harness.clone()),
        attachments: Vec::new(),
        artifact_refs: Vec::new(),
        evidence_refs: on_event.evidence_refs(),
        progress: progress.clone(),
    });
    chat.updated_at_ms = created_at_ms;
    store.replace_chat(&expected, &chat).map_err(map_store)?;
    let _ = on_event.send(match terminal_status {
        ChatTerminalStatus::Aborted => ChatStreamEvent::Aborted {
            run_id: run_id.to_owned(),
        },
        ChatTerminalStatus::Error => ChatStreamEvent::Error {
            failure_kind: on_event.failure_kind(),
            retry_after_seconds: on_event.retry_after_seconds(),
            run_id: run_id.to_owned(),
            message: FAILED_CHAT_CONTENT.into(),
            message_id: message_id.to_owned(),
            final_text: content,
            created_at: iso_time(created_at_ms).unwrap_or_default(),
            execution_model: Box::new(execution_model),
            agent_harness: Box::new(agent_harness),
            progress,
        },
    });
    Ok(RunStarted {
        run_id: run_id.to_owned(),
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn persist_and_emit_aborted_chat(
    store: &dyn LoupistStore,
    on_event: &ChatEventSink,
    run_id: &str,
    thread_id: &str,
    message_id: &str,
    content: String,
    memory_trace: DurableMemoryTrace,
    memory_revision: Option<String>,
    execution_model: ExecutionModelLock,
    agent_harness: AgentHarnessSnapshot,
    progress: Option<ChatProgress>,
) -> Result<RunStarted, CommandError> {
    persist_and_emit_terminal_chat(
        store,
        on_event,
        run_id,
        thread_id,
        message_id,
        content,
        memory_trace,
        memory_revision,
        execution_model,
        agent_harness,
        progress,
        ChatTerminalStatus::Aborted,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn persist_and_emit_failed_chat(
    store: &dyn LoupistStore,
    on_event: &ChatEventSink,
    run_id: &str,
    thread_id: &str,
    message_id: &str,
    memory_revision: Option<String>,
    execution_model: ExecutionModelLock,
    agent_harness: AgentHarnessSnapshot,
    progress: Option<ChatProgress>,
) -> Result<RunStarted, CommandError> {
    persist_and_emit_terminal_chat(
        store,
        on_event,
        run_id,
        thread_id,
        message_id,
        FAILED_CHAT_CONTENT.into(),
        empty_memory_trace()?,
        memory_revision,
        execution_model,
        agent_harness,
        progress,
        ChatTerminalStatus::Error,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn persist_or_emit_failed_chat(
    store: &dyn LoupistStore,
    on_event: &ChatEventSink,
    run_id: &str,
    thread_id: &str,
    message_id: &str,
    memory_revision: Option<String>,
    execution_model: ExecutionModelLock,
    agent_harness: AgentHarnessSnapshot,
    progress: Option<ChatProgress>,
) -> RunStarted {
    match persist_and_emit_failed_chat(
        store,
        on_event,
        run_id,
        thread_id,
        message_id,
        memory_revision,
        execution_model.clone(),
        agent_harness.clone(),
        progress.clone(),
    ) {
        Ok(run) => run,
        Err(_) => {
            on_event.set_failure_kind(ChatFailureKind::StoreFailed);
            // The user turn was already durable. Never surface internal store
            // details or leave the renderer waiting if terminal persistence
            // itself is unavailable.
            let _ = on_event.send(ChatStreamEvent::Error {
                failure_kind: on_event.failure_kind(),
                retry_after_seconds: on_event.retry_after_seconds(),
                run_id: run_id.to_owned(),
                message: FAILED_CHAT_CONTENT.into(),
                message_id: message_id.to_owned(),
                final_text: FAILED_CHAT_CONTENT.into(),
                created_at: iso_time(now_ms()).unwrap_or_default(),
                execution_model: Box::new(execution_model),
                agent_harness: Box::new(agent_harness),
                progress,
            });
            RunStarted {
                run_id: run_id.to_owned(),
            }
        }
    }
}

/// Persists an early failure only if terminal completion wins the same
/// coordinator boundary as Stop. A launch/configuration error can arrive
/// while its await is in flight, after the user message is already durable.
/// In that race, a successful Stop must be represented as an aborted
/// assistant message rather than an Error that resurfaces after reload.
#[allow(clippy::too_many_arguments)]
pub(in crate::commands) fn persist_or_emit_failed_chat_with_stop_precedence(
    state: &AppState,
    on_event: &ChatEventSink,
    run_id: &str,
    thread_id: &str,
    message_id: &str,
    memory_revision: Option<String>,
    execution_model: ExecutionModelLock,
    agent_harness: AgentHarnessSnapshot,
    progress: Option<ChatProgress>,
) -> RunStarted {
    if matches!(state.claim_run_completion(run_id, RunKind::Chat), Ok(false)) {
        let Ok(memory_trace) = empty_memory_trace() else {
            let _ = on_event.send(ChatStreamEvent::Aborted {
                run_id: run_id.to_owned(),
            });
            return RunStarted {
                run_id: run_id.to_owned(),
            };
        };
        return match persist_and_emit_aborted_chat(
            state.store.as_ref(),
            on_event,
            run_id,
            thread_id,
            message_id,
            "Response stopped.".into(),
            memory_trace,
            memory_revision,
            execution_model,
            agent_harness,
            progress,
        ) {
            Ok(run) => run,
            Err(_) => {
                let _ = on_event.send(ChatStreamEvent::Aborted {
                    run_id: run_id.to_owned(),
                });
                RunStarted {
                    run_id: run_id.to_owned(),
                }
            }
        };
    }
    persist_or_emit_failed_chat(
        state.store.as_ref(),
        on_event,
        run_id,
        thread_id,
        message_id,
        memory_revision,
        execution_model,
        agent_harness,
        progress,
    )
}

/// The assistant reply that every terminal outcome of one turn is recorded
/// under. Launch and finalize use it so each exit names the same reply.
pub(super) struct TerminalReply<'a> {
    pub state: &'a AppState,
    pub on_event: &'a ChatEventSink,
    pub run_id: &'a str,
    pub thread_id: &'a str,
    pub message_id: &'a str,
    pub memory_revision: &'a Option<String>,
    pub harness: &'a AgentHarnessSnapshot,
}

impl TerminalReply<'_> {
    /// Records a failed reply unless Stop claimed the run first.
    pub fn failed_with_stop_precedence(
        &self,
        execution_model: ExecutionModelLock,
        progress: Option<ChatProgress>,
    ) -> RunStarted {
        persist_or_emit_failed_chat_with_stop_precedence(
            self.state,
            self.on_event,
            self.run_id,
            self.thread_id,
            self.message_id,
            self.memory_revision.clone(),
            execution_model,
            self.harness.clone(),
            progress,
        )
    }

    /// Records a failed reply without consulting Stop.
    pub fn failed(
        &self,
        execution_model: ExecutionModelLock,
        progress: Option<ChatProgress>,
    ) -> RunStarted {
        persist_or_emit_failed_chat(
            self.state.store.as_ref(),
            self.on_event,
            self.run_id,
            self.thread_id,
            self.message_id,
            self.memory_revision.clone(),
            execution_model,
            self.harness.clone(),
            progress,
        )
    }

    /// Records the stopped reply with whatever text and exact reads it has.
    pub fn aborted(
        &self,
        content: String,
        memory_trace: DurableMemoryTrace,
        execution_model: ExecutionModelLock,
        progress: Option<ChatProgress>,
    ) -> Result<RunStarted, CommandError> {
        persist_and_emit_aborted_chat(
            self.state.store.as_ref(),
            self.on_event,
            self.run_id,
            self.thread_id,
            self.message_id,
            content,
            memory_trace,
            self.memory_revision.clone(),
            execution_model,
            self.harness.clone(),
            progress,
        )
    }
}
