//! Stream phase: the turn's event loop. It multiplexes Pi events, Stop,
//! steering, approval cards, evidence/entity links, the commentary flush and
//! the provider watchdog until the turn completes, fails or is stopped.
use super::launch::LaunchedChatTurn;
use super::*;

/// Stream → finalize: what the event loop observed about the reply.
pub(super) struct StreamedChatTurn {
    pub content: String,
    pub completed: bool,
    pub aborted: bool,
    pub terminal_completion_claimed: bool,
    pub failure: Option<String>,
    pub pending_controls: HashMap<u64, (u64, ChatControlRequest)>,
    pub settled_controls: BTreeMap<u64, (ChatControlRequest, bool)>,
    pub first_text_ms: Option<u64>,
    pub generation_ms: Option<u64>,
    pub token_usage: AssistantTokenUsage,
    pub progress: ChatProgressProjection,
    pub commentary: CommentaryCoalescer,
    pub envelope_compacted: bool,
}

/// Runs until Pi settles the turn, Stop wins, the watchdog fires or the
/// event stream fails. Pi and its channels stay owned by `turn`.
pub(super) async fn stream_chat_turn(turn: &mut LaunchedChatTurn) -> StreamedChatTurn {
    let LaunchedChatTurn {
        progress_started_at_ms,
        provider_wait_started_at,
        provider_idle_timeout,
        ref mut result_links,
        ref on_event,
        run_id: ref task_run_id,
        ref mut entity_links,
        ref mut approval_rx,
        ref mut cancel,
        ref mut pi,
        ref mut chat_controls,
        ref mut pi_events,
        ref mut activity_recorder,
        ref capture,
        ref app_state,
        loupist_id: ref profile_id,
        ref thread_id,
        prompt_request_id,
        chat_started_at,
        setup_ms,
        resumed,
        ref mut carried_progress,
        ..
    } = *turn;
    let mut envelope_compacted = false;
    let mut content = String::new();
    let mut pending_tool_calls = std::collections::BTreeSet::new();
    let mut successful_tool_calls = 0usize;
    let mut assistant_capture = AssistantTurnCapture::default();
    let mut completed = false;
    let mut aborted = false;
    let mut terminal_completion_claimed = false;
    let mut failure: Option<String> = None;
    let mut pending_controls = HashMap::new();
    let mut settled_controls = BTreeMap::new();
    let mut next_control_sequence = 0_u64;
    let mut next_control_to_persist = 0_u64;
    let mut first_text_ms: Option<u64> = None;
    let mut generation_ms: Option<u64> = None;
    let mut token_usage = AssistantTokenUsage::default();
    let mut progress = carried_progress.take().unwrap_or_else(|| {
        let mut progress = ChatProgressProjection::new(progress_started_at_ms);
        progress.set_asset_names(asset_names(app_state, profile_id));
        progress
    });
    let mut approvals_open = true;
    let mut provider_wait_watchdog =
        ProviderWaitWatchdog::new(provider_wait_started_at, provider_idle_timeout);
    let provider_wait_timer = tokio::time::sleep_until(
        provider_wait_watchdog
            .deadline()
            .expect("new first-provider-body watchdog has a deadline"),
    );
    tokio::pin!(provider_wait_timer);
    let mut watchdog_expiry_event_deferrals = 0_usize;
    let mut announced_phase: Option<&'static str> = None;
    let mut commentary = CommentaryCoalescer::new();
    // Polled only after a narration window is armed. The deadline is reset
    // to that window; this initial sleep is not a flush.
    let commentary_flush = tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60));
    tokio::pin!(commentary_flush);
    loop {
        while let Ok((call_id, evidence_id)) = result_links.try_recv() {
            progress.attach_result(&call_id, &evidence_id);
            publish_progress(on_event, task_run_id, &mut progress, &mut commentary);
        }
        while let Ok((call_id, ids)) = entity_links.try_recv() {
            progress.attach_entities(&call_id, &ids);
            publish_progress(on_event, task_run_id, &mut progress, &mut commentary);
        }
        while let Ok((call_id, approval)) = approval_rx.try_recv() {
            progress.upsert_approval(call_id.as_deref(), approval);
            publish_progress(on_event, task_run_id, &mut progress, &mut commentary);
        }
        // A window that has already elapsed is published before the next
        // Pi event. Fair select would otherwise keep a busy event stream
        // ahead of the flush timer. Idle runs still wake from that timer.
        if commentary.is_due(Instant::now()) {
            publish_progress(on_event, task_run_id, &mut progress, &mut commentary);
            continue;
        }
        let select_mode =
            provider_wait_watchdog.select_mode(Instant::now(), watchdog_expiry_event_deferrals);
        if select_mode == FirstProviderBodyWatchdogSelectMode::Timeout {
            if *cancel.borrow() || cancel.has_changed().is_err() {
                aborted = true;
                let _ = pi.abort().await;
            } else {
                failure = Some(provider_wait_watchdog.timeout_failure().into());
            }
            break;
        }
        let input = match select_mode {
            FirstProviderBodyWatchdogSelectMode::Fair => {
                // Keep Tokio's default fair branch polling while streaming:
                // a busy Pi event stream must not starve a queued steer.
                tokio::select! {
                    _ = &mut commentary_flush, if commentary.is_pending() => ChatLoopInput::CommentaryFlush,
                    _ = &mut provider_wait_timer, if provider_wait_watchdog.deadline().is_some() => ChatLoopInput::WatchdogElapsed,
                    changed = cancel.changed() => ChatLoopInput::CancelChanged(changed),
                    control = chat_controls.recv() => ChatLoopInput::Control(control),
                    approval = approval_rx.recv(), if approvals_open => ChatLoopInput::Approval(approval),
                    event = pi_events.recv() => ChatLoopInput::PiEvent(event),
                }
            }
            FirstProviderBodyWatchdogSelectMode::PrioritizeQueuedPiEvent => {
                receive_expired_watchdog_input(
                    provider_wait_timer.as_mut(),
                    cancel,
                    chat_controls,
                    pi_events,
                    &mut watchdog_expiry_event_deferrals,
                )
                .await
            }
            FirstProviderBodyWatchdogSelectMode::Timeout => {
                unreachable!("handled before select")
            }
        };
        let input = match input {
            ChatLoopInput::WatchdogElapsed => {
                let Some(event) = take_queued_pi_event_after_watchdog_expiry(
                    pi_events,
                    &mut watchdog_expiry_event_deferrals,
                ) else {
                    failure = Some(provider_wait_watchdog.timeout_failure().into());
                    break;
                };
                ChatLoopInput::PiEvent(event)
            }
            input => input,
        };
        match input {
            ChatLoopInput::CommentaryFlush => {
                publish_progress(on_event, task_run_id, &mut progress, &mut commentary);
                continue;
            }
            ChatLoopInput::Approval(Some((call_id, approval))) => {
                progress.upsert_approval(call_id.as_deref(), approval);
                publish_progress(on_event, task_run_id, &mut progress, &mut commentary);
                continue;
            }
            ChatLoopInput::Approval(None) => {
                approvals_open = false;
                continue;
            }
            ChatLoopInput::CancelChanged(changed) => {
                if changed.is_err() || *cancel.borrow() {
                    aborted = true;
                    let _ = pi.abort().await;
                    break;
                }
                continue;
            }
            ChatLoopInput::Control(control) => {
                let Some(control) = control else {
                    continue;
                };
                let prompt = control.prompt.clone();
                match pi.steer(&prompt).await {
                    Ok(request_id) => {
                        let sequence = next_control_sequence;
                        next_control_sequence += 1;
                        pending_controls.insert(request_id, (sequence, control));
                    }
                    Err(_) => control.complete(Err(ChatControlError::Rejected(
                        "Pi did not accept the queued instruction".into(),
                    ))),
                }
                continue;
            }
            ChatLoopInput::PiEvent(event) => {
                let now = Instant::now();
                provider_wait_watchdog.observe(&event, now);
                if let Some(deadline) = provider_wait_watchdog.deadline() {
                    provider_wait_timer.as_mut().reset(deadline);
                    if deadline > now {
                        watchdog_expiry_event_deferrals = 0;
                    }
                } else {
                    watchdog_expiry_event_deferrals = 0;
                }
                match event {
                    Ok(PiEvent::Rpc { payload }) => {
                        if let Some((_, record)) = on_event
                            .run_log
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .as_mut()
                        {
                            record.observe_pi_tool(&payload);
                        }
                        #[cfg(feature = "eval-harness")]
                        if let Some(capture) = &on_event.eval_pi_event {
                            capture(&payload);
                        }
                        let step = pi_dispatch::apply_pi_rpc(
                            pi_dispatch::PiRpcTurn {
                                content: &mut content,
                                pending_tool_calls: &mut pending_tool_calls,
                                successful_tool_calls: &mut successful_tool_calls,
                                assistant_capture: &mut assistant_capture,
                                completed: &mut completed,
                                aborted: &mut aborted,
                                terminal_completion_claimed: &mut terminal_completion_claimed,
                                failure: &mut failure,
                                pending_controls: &mut pending_controls,
                                settled_controls: &mut settled_controls,
                                next_control_to_persist: &mut next_control_to_persist,
                                first_text_ms: &mut first_text_ms,
                                generation_ms: &mut generation_ms,
                                token_usage: &mut token_usage,
                                progress: &mut progress,
                                commentary: &mut commentary,
                                announced_phase: &mut announced_phase,
                                envelope_compacted: &mut envelope_compacted,
                                on_event,
                                task_run_id,
                                activity_recorder,
                                capture: capture.as_ref(),
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
                            },
                            &payload,
                        )
                        .await;
                        if matches!(step, pi_dispatch::PiRpcStep::Break) {
                            break;
                        }
                        if matches!(
                            step,
                            pi_dispatch::PiRpcStep::Continue {
                                arm_commentary: true
                            }
                        ) {
                            commentary_flush
                                .as_mut()
                                .reset(commentary.deadline().expect("commentary window armed"));
                        }
                    }
                    Ok(PiEvent::ProtocolError { message }) => {
                        failure = Some(message);
                        break;
                    }
                    Ok(PiEvent::Exited) => {
                        failure = Some(match pi.try_exit_code() {
                            Some(code) => {
                                #[cfg(feature = "e2e")]
                                eprintln!(
                                    "Loupist E2E Chat diagnostic: phase=process_exit exit={code}"
                                );
                                format!("Pi stopped before the turn settled (exit code {code})")
                            }
                            None => "Pi stopped before the turn settled".into(),
                        });
                        break;
                    }
                    Err(error) => {
                        if let Some(message) = pi_event_stream_failure(error) {
                            failure = Some(message.into());
                            break;
                        }
                    }
                }
            }
            ChatLoopInput::WatchdogElapsed => unreachable!("normalized before dispatch"),
        }
    }
    StreamedChatTurn {
        content,
        completed,
        aborted,
        terminal_completion_claimed,
        failure,
        pending_controls,
        settled_controls,
        first_text_ms,
        generation_ms,
        token_usage,
        progress,
        commentary,
        envelope_compacted,
    }
}

/// The Loupist's asset pages by symbol, for step labels; a failed read leaves
/// the steps showing their symbols.
fn asset_names(state: &AppState, loupist_id: &str) -> crate::chat::progress::AssetNames {
    crate::commands::assets::with_assets(state, loupist_id, false, |tx| {
        crate::knowledge::assets::list_pages(tx, loupist_id)
    })
    .map(|pages| {
        crate::chat::progress::AssetNames::new(
            pages
                .into_iter()
                .map(|page| (page.symbol, page.market, page.name)),
        )
    })
    .unwrap_or_default()
}
