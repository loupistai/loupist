//! Finalize phase: stop Pi, the broker and MCP, seal the reusable Pi cache,
//! then persist and emit exactly one terminal reply: stopped, completed (with
//! its Memory transaction, receipts and artifacts) or failed.
use super::completion::{commit_completed_turn, CompletedTurn};
use super::launch::{LaunchedChatTurn, PiCacheSeal};
use super::stream::StreamedChatTurn;
use super::*;
use crate::{broker::ToolBrokerHandle, chat::activity::ActivityRecorder};

pub(super) async fn finalize_chat_turn(turn: LaunchedChatTurn, streamed: StreamedChatTurn) {
    // Bound in reverse of the field order. Locals drop in reverse, so once the
    // reply is terminal the link receivers, sink and Stop/steer channels drop
    // before the tool capture, and the run lease (turn resources) after it.
    let LaunchedChatTurn {
        gone_retry: _,
        carried_progress: _,
        pins: task_pins,
        fallback_title,
        should_generate_title,
        memory_revision,
        message_id: task_message_id,
        broker,
        execution_model,
        update_memory: task_update_memory,
        use_memory: task_use_memory,
        harness,
        cache_seal,
        mut turn_resources,
        resumed,
        setup_ms,
        chat_started_at,
        prompt_request_id: _,
        thread_id,
        loupist_id: task_loupist_id,
        app_state,
        capture,
        mut activity_recorder,
        mut pi_events,
        chat_controls: _chat_controls,
        pi,
        cancel: _cancel,
        card_rx: _card_rx,
        entity_links: _entity_links,
        run_id: task_run_id,
        on_event,
        result_links: _result_links,
        provider_idle_timeout: _,
        provider_wait_started_at: _,
        progress_started_at_ms: _,
    } = turn;
    let StreamedChatTurn {
        content,
        mut completed,
        aborted,
        terminal_completion_claimed,
        mut failure,
        pending_controls,
        settled_controls,
        first_text_ms,
        generation_ms,
        token_usage,
        mut progress,
        mut commentary,
        envelope_compacted,
    } = streamed;
    let sealed_cache = settle_turn_runtime(SettleTurn {
        on_event: &on_event,
        task_run_id: &task_run_id,
        app_state: &app_state,
        task_loupist_id: &task_loupist_id,
        progress: &mut progress,
        commentary: &mut commentary,
        token_usage,
        activity_recorder: &mut activity_recorder,
        aborted,
        completed: &mut completed,
        failure: &mut failure,
        pending_controls,
        settled_controls,
        pi,
        pi_events: &mut pi_events,
        broker,
        capture: capture.as_ref(),
        turn_resources: &mut turn_resources,
        cache_seal,
        harness: &harness,
        execution_model: &execution_model,
        task_use_memory,
        task_update_memory,
        envelope_compacted,
    })
    .await;
    let reply = TerminalReply {
        state: &app_state,
        on_event: &on_event,
        run_id: &task_run_id,
        thread_id: &thread_id,
        message_id: &task_message_id,
        memory_revision: &memory_revision,
        harness: &harness,
    };

    // Keep the writer lease until terminal persistence/emission has finished,
    // including the error path after a rolled-back Memory transaction.
    let mut memory_finalization_writer = None;
    if aborted {
        let progress_snapshot =
            finish_chat_progress(&on_event, &task_run_id, &mut progress, now_ms(), true);
        let memories = capture
            .memories
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let aborted_content = if content.trim().is_empty() {
            "Response stopped.".to_owned()
        } else {
            content.clone()
        };
        let persist_result = durable_memory_trace(memories).and_then(|trace| {
            reply.aborted(
                aborted_content,
                trace,
                execution_model.clone(),
                progress_snapshot,
            )
        });
        match persist_result {
            Ok(_) => return,
            Err(error) => {
                on_event.set_failure_kind(ChatFailureKind::StoreFailed);
                failure = Some(error.message);
            }
        }
    } else if completed {
        let committed = commit_completed_turn(CompletedTurn {
            reply: &reply,
            task_loupist_id: &task_loupist_id,
            execution_model: &execution_model,
            capture: capture.as_ref(),
            turn_resources: &mut turn_resources,
            memory_finalization_writer: &mut memory_finalization_writer,
            progress: &mut progress,
            failure: &mut failure,
            content,
            sealed_cache,
            task_pins: &task_pins,
            task_update_memory,
            should_generate_title,
            fallback_title: &fallback_title,
            chat_started_at,
            setup_ms,
            first_text_ms,
            generation_ms,
            resumed,
            token_usage,
        })
        .await;
        if committed.is_break() {
            return;
        }
    }
    if let Some(reason) = failure.as_deref() {
        if on_event.failure_kind() == ChatFailureKind::Other {
            on_event.set_failure_kind(classify_turn_failure(
                &CommandError::internal(reason),
                ChatFailureKind::Other,
            ));
        }
        progress.fail_plan();
        let progress_snapshot =
            finish_chat_progress(&on_event, &task_run_id, &mut progress, now_ms(), false);
        if terminal_completion_claimed {
            let _ = reply.failed(execution_model, progress_snapshot);
        } else {
            let _ = reply.failed_with_stop_precedence(execution_model, progress_snapshot);
        }
    }
    drop(memory_finalization_writer);
}

/// Borrowed view of a streamed turn while its processes stop.
struct SettleTurn<'a> {
    on_event: &'a ChatEventSink,
    task_run_id: &'a str,
    app_state: &'a AppState,
    task_loupist_id: &'a str,
    progress: &'a mut ChatProgressProjection,
    commentary: &'a mut CommentaryCoalescer,
    token_usage: AssistantTokenUsage,
    activity_recorder: &'a mut ActivityRecorder,
    aborted: bool,
    completed: &'a mut bool,
    failure: &'a mut Option<String>,
    pending_controls: HashMap<u64, (u64, ChatControlRequest)>,
    settled_controls: BTreeMap<u64, (ChatControlRequest, bool)>,
    pi: PiProcess,
    pi_events: &'a mut tokio::sync::broadcast::Receiver<PiEvent>,
    broker: ToolBrokerHandle,
    capture: &'a ToolCapture,
    turn_resources: &'a mut ChatTurnResources,
    cache_seal: PiCacheSeal,
    harness: &'a AgentHarnessSnapshot,
    execution_model: &'a ExecutionModelLock,
    task_use_memory: bool,
    task_update_memory: bool,
    envelope_compacted: bool,
}

/// Publishes pending narration, records usage and unfinished activity, then
/// stops Pi, the broker and MCP. Returns the Pi cache this turn may seal.
async fn settle_turn_runtime(turn: SettleTurn<'_>) -> Option<PiSessionCache> {
    let SettleTurn {
        on_event,
        task_run_id,
        app_state,
        task_loupist_id,
        progress,
        commentary,
        token_usage,
        activity_recorder,
        aborted,
        completed,
        failure,
        pending_controls,
        settled_controls,
        pi,
        pi_events,
        broker,
        capture,
        turn_resources,
        cache_seal,
        harness,
        execution_model,
        task_use_memory,
        task_update_memory,
        envelope_compacted,
    } = turn;
    let PiCacheSeal {
        connector_authority_cacheable: task_connector_authority_cacheable,
        connector_authority_sha256: task_connector_authority_sha256,
        runtime_policy_sha256,
        runtime_surface_sha256,
        derived_session_id: task_derived_session_id,
        sent_envelope_digest,
    } = cache_seal;

    // Abort, timeout, and failure leave the loop without waiting out the
    // narration window. Publish that text before Pi shutdown.
    publish_progress(on_event, task_run_id, progress, commentary);
    if token_usage.input > 0 || token_usage.output > 0 || token_usage.cache_read > 0 {
        if let Some((_, record)) = on_event
            .run_log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            record.usage = Some(crate::runs::log::Usage {
                input: token_usage.input.saturating_add(token_usage.cache_write),
                cached_input: token_usage.cache_read,
                output: token_usage.output,
            });
        }
    }
    match activity_recorder.finish_remaining(aborted, now_ms()) {
        Ok(call_ids) => {
            if !call_ids.is_empty() {
                for call_id in &call_ids {
                    progress.note_output(call_id);
                }
                publish_progress(on_event, task_run_id, progress, commentary);
            }
        }
        Err(error) => {
            *failure = Some(error.message);
            *completed = false;
        }
    }
    #[cfg(feature = "eval-harness")]
    if let (Some(capture), Some(reason)) = (&on_event.eval_pi_event, failure.as_deref()) {
        capture(
            &serde_json::json!({"type":"eval_error","message":crate::pi::e2e_failure_category(reason)}),
        );
    }
    #[cfg(any(debug_assertions, feature = "e2e"))]
    if let Some(reason) = failure.as_deref() {
        crate::pi::trace_e2e_failure("turn", reason);
    }
    reject_pending_chat_controls(pending_controls, settled_controls);
    let cache_entries = if *completed && !aborted {
        read_pi_entries(&pi, pi_events, None).await.ok()
    } else {
        None
    };
    let pi_process_group_id = pi_process_group_id(&pi);
    let pi_stopped = if *completed && !aborted {
        pi.shutdown_settled().await
    } else {
        pi.shutdown(Duration::from_secs(2)).await
    }
    .is_ok();
    if !pi_stopped {
        if let Err(error) =
            record_unconfirmed_pi_stop(turn_resources.pi_session(), pi_process_group_id)
        {
            // Without a durable quarantine we cannot safely finalize this turn.
            // Fail before committing, instead of appending an error after a saved reply.
            on_event.set_failure_kind(ChatFailureKind::StoreFailed);
            *completed = false;
            *failure = Some(error.message);
        }
    }
    // A Pi child owns the JSONL. Only publish a cursor after it has
    // acknowledged a settled shutdown, otherwise a still-running child
    // could append after the cursor is sealed.
    let connector_authority_unchanged = *completed
        && !aborted
        && task_connector_authority_cacheable
        && capture_chat_connector_authority(app_state, task_loupist_id)
            .map(|current| current.cacheable && current.sha256 == task_connector_authority_sha256)
            .unwrap_or(false);
    let sealed_cache = (pi_stopped && connector_authority_unchanged)
        .then(|| {
            cache_entries.and_then(|entries| {
                let cache_scope = PiSessionCacheScope {
                    harness_digest: &harness.digest,
                    runtime_policy_sha256: &runtime_policy_sha256,
                    runtime_surface_sha256: &runtime_surface_sha256,
                    connector_authority_sha256: &task_connector_authority_sha256,
                    memory_access_enabled: task_use_memory,
                    memory_update_enabled: task_update_memory,
                    execution_model,
                };
                cache_from_entries(entries, &cache_scope, &task_derived_session_id).map(
                    |mut cache| {
                        cache.context_envelope_sha256 = sent_envelope_digest.clone();
                        cache.context_envelope_stale = envelope_compacted;
                        cache
                    },
                )
            })
        })
        .flatten();
    let _ = broker.shutdown().await;
    capture.shutdown_mcp().await;
    // Pi owns the JSONL session file. Stop it before wiping an incomplete
    // run so a still-draining child cannot recreate a cache we just removed.
    if (aborted || !*completed) && pi_stopped {
        let _ = turn_resources.pi_session_mut().wipe();
    }
    sealed_cache
}
