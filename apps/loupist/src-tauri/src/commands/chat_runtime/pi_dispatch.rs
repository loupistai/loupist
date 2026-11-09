//! Pi RPC handling for one Chat turn. The chat loop keeps select, cancellation,
//! and the provider watchdog; this module applies one provider payload.
use super::progress::{emit_chat_phase, publish_settled_answer};
use super::*;

pub(super) enum PiRpcStep {
    Continue { arm_commentary: bool },
    Break,
}

pub(super) struct PiRpcTurn<'a> {
    pub content: &'a mut String,
    pub pending_tool_calls: &'a mut std::collections::BTreeSet<String>,
    pub successful_tool_calls: &'a mut usize,
    pub assistant_capture: &'a mut AssistantTurnCapture,
    pub completed: &'a mut bool,
    pub aborted: &'a mut bool,
    pub terminal_completion_claimed: &'a mut bool,
    pub failure: &'a mut Option<String>,
    pub pending_controls: &'a mut HashMap<u64, (u64, ChatControlRequest)>,
    pub settled_controls: &'a mut BTreeMap<u64, (ChatControlRequest, bool)>,
    pub next_control_to_persist: &'a mut u64,
    pub first_text_ms: &'a mut Option<u64>,
    pub generation_ms: &'a mut Option<u64>,
    pub token_usage: &'a mut AssistantTokenUsage,
    pub progress: &'a mut ChatProgressProjection,
    pub commentary: &'a mut CommentaryCoalescer,
    pub announced_phase: &'a mut Option<&'static str>,
    pub envelope_compacted: &'a mut bool,
    pub on_event: &'a ChatEventSink,
    pub task_run_id: &'a str,
    pub activity_recorder: &'a mut crate::chat::activity::ActivityRecorder,
    pub capture: &'a ToolCapture,
    pub pi: &'a mut PiProcess,
    pub pi_events: &'a mut tokio::sync::broadcast::Receiver<PiEvent>,
    pub chat_controls: &'a mut ChatControlReceiver,
    pub app_state: &'a AppState,
    pub profile_id: &'a str,
    pub thread_id: &'a str,
    pub prompt_request_id: u64,
    pub chat_started_at: Instant,
    pub setup_ms: u64,
    pub resumed: bool,
}

pub(super) async fn apply_pi_rpc(turn: PiRpcTurn<'_>, payload: &Value) -> PiRpcStep {
    let PiRpcTurn {
        content,
        pending_tool_calls,
        successful_tool_calls,
        assistant_capture,
        completed,
        aborted,
        terminal_completion_claimed,
        failure,
        pending_controls,
        settled_controls,
        next_control_to_persist,
        first_text_ms,
        generation_ms,
        token_usage,
        progress,
        commentary,
        announced_phase,
        envelope_compacted,
        on_event,
        task_run_id,
        activity_recorder,
        capture,
        pi,
        pi_events,
        chat_controls,
        app_state,
        profile_id,
        thread_id,
        prompt_request_id,
        chat_started_at,
        setup_ms,
        resumed,
    } = turn;
    let mut arm_commentary = false;
    match payload.get("type").and_then(Value::as_str) {
        Some("message_start") => {
            if let Err(message) = assistant_capture.observe_message_start(payload) {
                *failure = Some(message.into());
                let _ = pi.abort().await;
                return PiRpcStep::Break;
            }
            if payload
                .get("message")
                .and_then(|message| message.get("role"))
                .and_then(Value::as_str)
                == Some("assistant")
            {
                progress.start_assistant_turn();
            }
        }
        Some("loupist_retry_hints") => {
            // The extension's allowlisted timing headers of the latest provider response.
            assistant_capture.observe_retry_headers(payload.get("headers").unwrap_or(&Value::Null));
        }
        Some("tool_execution_start") => {
            emit_chat_phase(on_event, task_run_id, announced_phase, "tool");
            if let (Some(tool_call_id), Some(tool_name)) = (
                payload.get("toolCallId").and_then(Value::as_str),
                payload.get("toolName").and_then(Value::as_str),
            ) {
                let args = payload.get("args").unwrap_or(&Value::Null);
                on_event
                    .tool_trace
                    .observe_start(tool_call_id, tool_name, args);
                let web_source = if tool_name == "web_fetch" {
                    if let Some(source_id) = args.get("source_id").and_then(Value::as_str) {
                        capture.web_sources.lock().await.get(source_id).cloned()
                    } else if let Some(url) = args.get("url").and_then(Value::as_str) {
                        crate::web::source_from_url(url).ok()
                    } else {
                        None
                    }
                } else {
                    None
                };
                progress.start_tool(tool_call_id, tool_name, args, web_source.as_ref(), now_ms());
                match activity_recorder.start(tool_call_id, tool_name, args, now_ms()) {
                    Ok(Some(reference)) => progress.attach_activity(tool_call_id, reference),
                    Ok(None) => {}
                    Err(error) => {
                        *failure = Some(error.message);
                        let _ = pi.abort().await;
                        return PiRpcStep::Break;
                    }
                }
                publish_progress(on_event, task_run_id, progress, commentary);
            }
        }
        Some("tool_execution_update") => {
            if let Some(tool_call_id) = payload.get("toolCallId").and_then(Value::as_str) {
                match activity_recorder.update(
                    tool_call_id,
                    payload.get("partialResult").unwrap_or(&Value::Null),
                    now_ms(),
                ) {
                    Ok(flushed) => {
                        if flushed {
                            progress.note_output(tool_call_id);
                            publish_progress(on_event, task_run_id, progress, commentary);
                        }
                    }
                    Err(error) => {
                        *failure = Some(error.message);
                        let _ = pi.abort().await;
                        return PiRpcStep::Break;
                    }
                }
            }
        }
        Some("tool_execution_end") => {
            if let Some(tool_call_id) = payload.get("toolCallId").and_then(Value::as_str) {
                let result = payload.get("result").unwrap_or(&Value::Null);
                let failed = payload
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    || progress.attach_python_result(tool_call_id, result);
                pending_tool_calls.remove(tool_call_id);
                if !failed {
                    *successful_tool_calls += 1;
                }
                on_event.tool_trace.observe_end(
                    tool_call_id,
                    failed,
                    payload.get("result").unwrap_or(&Value::Null),
                );
                match activity_recorder.finish(
                    tool_call_id,
                    payload.get("result").unwrap_or(&Value::Null),
                    failed,
                    now_ms(),
                ) {
                    Ok(flushed) => {
                        if flushed {
                            progress.note_output(tool_call_id);
                        }
                    }
                    Err(error) => {
                        *failure = Some(error.message);
                        let _ = pi.abort().await;
                        return PiRpcStep::Break;
                    }
                }
                progress.finish_tool(
                    tool_call_id,
                    failed,
                    now_ms(),
                    summarize_tool_result(result),
                );
                if failed && crate::chat::progress::as_of_refusal(result) {
                    progress.skip_tool(tool_call_id);
                }
                // The broker's own record says which rule refused the call; the
                // model-facing text is never parsed for it.
                if failed {
                    use crate::chat::progress::ChatStepRefusal;
                    let refusal = [
                        (
                            "untrusted_text_paused",
                            ChatStepRefusal::UntrustedTextPaused,
                        ),
                        ("protected_path", ChatStepRefusal::ProtectedPath),
                        ("outside_workspace", ChatStepRefusal::OutsideWorkspace),
                    ]
                    .into_iter()
                    .find(|(code, _)| on_event.tool_trace.refused(tool_call_id, code));
                    if let Some((_, refusal)) = refusal {
                        progress.refuse_tool(tool_call_id, refusal);
                    }
                }
                publish_progress(on_event, task_run_id, progress, commentary);
            }
        }
        Some("compaction_start") => {
            *envelope_compacted = true;
            progress.start_system(
                "compaction",
                ChatProgressOperation::Compact,
                "Summarized earlier messages",
                None,
                now_ms(),
            );
            publish_progress(on_event, task_run_id, progress, commentary);
        }
        Some("compaction_end") => {
            match compaction_outcome(payload) {
                CompactionOutcome::Cancelled => progress.discard_system("compaction"),
                CompactionOutcome::Succeeded => {
                    progress.finish_system("compaction", false, now_ms())
                }
                CompactionOutcome::Failed => progress.finish_system("compaction", true, now_ms()),
            }
            publish_progress(on_event, task_run_id, progress, commentary);
        }
        Some("auto_retry_start") => {
            let target = match (
                payload.get("attempt").and_then(Value::as_u64),
                payload.get("maxAttempts").and_then(Value::as_u64),
            ) {
                (Some(attempt), Some(max)) => Some(format!("attempt {attempt} of {max}")),
                (Some(attempt), None) => Some(format!("attempt {attempt}")),
                _ => None,
            };
            progress.start_system(
                "retry",
                ChatProgressOperation::Retry,
                "Retrying model request",
                target,
                now_ms(),
            );
            publish_progress(on_event, task_run_id, progress, commentary);
        }
        Some("auto_retry_end") => {
            progress.finish_system(
                "retry",
                payload.get("success").and_then(Value::as_bool) == Some(false),
                now_ms(),
            );
            publish_progress(on_event, task_run_id, progress, commentary);
        }
        Some("message_update") => {
            let assistant_event = payload.get("assistantMessageEvent");
            match assistant_event
                .and_then(|event| event.get("type"))
                .and_then(Value::as_str)
            {
                Some("thinking_delta") => {
                    emit_chat_phase(on_event, task_run_id, announced_phase, "thinking");
                }
                Some("thinking_end") => {}
                Some("text_delta") => {
                    emit_chat_phase(on_event, task_run_id, announced_phase, "writing");
                    if let Some(delta) = assistant_event
                        .and_then(|event| event.get("delta"))
                        .and_then(Value::as_str)
                    {
                        first_text_ms.get_or_insert_with(|| elapsed_millis(chat_started_at));
                        if let Err(message) =
                            assistant_capture.observe_text_delta(delta, MAX_CHAT_OUTPUT_BYTES)
                        {
                            *failure = Some(message.into());
                            let _ = pi.abort().await;
                            return PiRpcStep::Break;
                        }
                        progress.append_commentary(delta);
                        if commentary.note_delta(Instant::now()) {
                            arm_commentary = true;
                        }
                    }
                }
                _ => {}
            }
        }
        Some("message_end") => {
            if let Some(blocks) = payload
                .pointer("/message/content")
                .and_then(Value::as_array)
            {
                for block in blocks {
                    if block["type"] == "toolCall" {
                        if let Some(id) = block["id"].as_str() {
                            pending_tool_calls.insert(id.to_owned());
                        }
                    }
                }
            }
            token_usage.observe_message_end(payload);
            let ended = assistant_capture.observe_message_end(payload, MAX_CHAT_OUTPUT_BYTES);
            // A busy answer counts down from the provider's own hint; any later
            // message replaces it.
            on_event.set_retry_after_seconds(assistant_capture.retry_after_seconds());
            match ended {
                Ok(Some(AssistantTurnEnd::Stop(authoritative))) => {
                    let streamed = if content.is_empty() {
                        authoritative.clone()
                    } else {
                        format!("\n\n{authoritative}")
                    };
                    content.push_str(&streamed);
                    emit_chat_phase(on_event, task_run_id, announced_phase, "writing");
                    // Show the settled answer before removing its live copy,
                    // so the paragraph does not disappear between events.
                    publish_settled_answer(on_event, task_run_id, progress, commentary, &streamed);
                }
                Ok(Some(AssistantTurnEnd::ToolUse(_))) => {
                    progress.finish_assistant_turn(false);
                    publish_progress(on_event, task_run_id, progress, commentary);
                }
                Ok(Some(
                    AssistantTurnEnd::Length | AssistantTurnEnd::Error | AssistantTurnEnd::Aborted,
                )) => {
                    progress.finish_assistant_turn(false);
                    publish_progress(on_event, task_run_id, progress, commentary);
                }
                Ok(None) => {}
                Err(message) => {
                    *failure = Some(message.into());
                    let _ = pi.abort().await;
                    return PiRpcStep::Break;
                }
            }
        }
        Some("agent_settled") => {
            // Pi can settle after the tool result without a final text message.
            // Only finished tool calls qualify; provider/protocol failures do not.
            let settled = assistant_capture.finish_settled().or_else(|reason| {
                if reason == "Pi settled with an incomplete tool request"
                    && pending_tool_calls.is_empty()
                    && *successful_tool_calls > 0
                {
                    Ok("")
                } else {
                    Err(reason)
                }
            });
            match settled {
                Ok(authoritative) => {
                    match app_state.claim_run_completion(task_run_id, RunKind::Chat) {
                        Ok(true) => {
                            *terminal_completion_claimed = true;
                            // The terminal run cutoff prevents a steer from
                            // being accepted after this point. Every steer
                            // already sent to Pi must still receive its RPC
                            // acknowledgement before this answer can become
                            // canonical; Pi may emit that acknowledgement
                            // after `agent_settled`.
                            reject_queued_chat_controls(chat_controls);
                            match settle_chat_controls_before_completion(
                                pi_events,
                                pending_controls,
                                settled_controls,
                                next_control_to_persist,
                                app_state.store.as_ref(),
                                profile_id,
                                thread_id,
                            )
                            .await
                            {
                                Ok(()) => {
                                    if content.is_empty() {
                                        *content = authoritative.to_owned();
                                    }
                                    *completed = true;
                                    let elapsed = elapsed_millis(chat_started_at);
                                    let generation_elapsed = elapsed.saturating_sub(setup_ms);
                                    *generation_ms = Some(generation_elapsed);
                                    let _ = on_event.send(ChatStreamEvent::GenerationFinished {
                                        run_id: task_run_id.to_owned(),
                                        performance: ChatPerformanceDto {
                                            setup_ms,
                                            first_text_ms: *first_text_ms,
                                            generation_ms: generation_elapsed,
                                            total_ms: elapsed,
                                            session_cache: if resumed {
                                                ChatSessionCacheStatusDto::Warm
                                            } else {
                                                ChatSessionCacheStatusDto::Cold
                                            },
                                            input_tokens: token_usage.input,
                                            output_tokens: token_usage.output,
                                            cache_read_tokens: token_usage.cache_read,
                                            cache_write_tokens: token_usage.cache_write,
                                        },
                                    });
                                }
                                Err(error) => *failure = Some(error.message),
                            }
                            reject_queued_chat_controls(chat_controls);
                        }
                        Ok(false) => *aborted = true,
                        Err(error) => *failure = Some(error.message),
                    }
                }
                Err(message) => *failure = Some(message.into()),
            }
            return PiRpcStep::Break;
        }
        Some("response") => {
            if let Some(request_id) = payload.get("id").and_then(Value::as_u64) {
                if record_chat_control_response(
                    pending_controls,
                    settled_controls,
                    request_id,
                    payload.get("success").and_then(Value::as_bool) == Some(true),
                ) {
                    if let Err(error) = persist_contiguous_chat_controls(
                        app_state.store.as_ref(),
                        profile_id,
                        thread_id,
                        settled_controls,
                        next_control_to_persist,
                    ) {
                        *failure = Some(error.message);
                        let _ = pi.abort().await;
                        return PiRpcStep::Break;
                    }
                } else if request_id == prompt_request_id
                    && payload.get("success").and_then(Value::as_bool) == Some(false)
                {
                    // A provider-controlled error may echo request or
                    // authentication material. Keep it out of renderer
                    // events and durable Chat state.
                    *failure = Some("Pi rejected the prompt".into());
                    return PiRpcStep::Break;
                }
            }
        }
        Some("extension_error") => {
            *failure = Some("Pi extension failed".into());
            return PiRpcStep::Break;
        }
        _ => {}
    }

    PiRpcStep::Continue { arm_commentary }
}
