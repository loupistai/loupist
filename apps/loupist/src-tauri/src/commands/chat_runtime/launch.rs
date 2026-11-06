//! Launch phase: start the tool broker, build the turn envelope, launch or
//! resume Pi and send the prompt. Every exit before Pi accepts the prompt
//! stops what already started and records the reply as failed or stopped.
use super::setup::{AdmittedChatTurn, TurnEnvelopeInputs};
use super::*;
use crate::{broker::ToolBrokerHandle, chat::activity::ActivityRecorder, domain::LoupistProfile};

pub(super) type ApprovalReceiver =
    tokio::sync::mpsc::UnboundedReceiver<(Option<String>, crate::chat::approvals::ChatApproval)>;

/// Launch → stream → finalize: the live turn that the spawned task owns.
///
/// Fields follow the order in which the turn task first used each value.
/// Finalize binds them in reverse, so once the reply is terminal they drop
/// in this order: the sink and the Stop/steer channels before the tool
/// capture, and the run lease (turn resources) after it.
pub(super) struct LaunchedChatTurn {
    pub progress_started_at_ms: i64,
    pub provider_wait_started_at: Instant,
    pub provider_idle_timeout: Duration,
    pub result_links: std::sync::mpsc::Receiver<(String, String)>,
    pub on_event: ChatEventSink,
    pub run_id: String,
    pub entity_links: std::sync::mpsc::Receiver<(String, Vec<String>)>,
    pub approval_rx: ApprovalReceiver,
    pub cancel: tokio::sync::watch::Receiver<bool>,
    pub pi: PiProcess,
    pub chat_controls: ChatControlReceiver,
    pub pi_events: tokio::sync::broadcast::Receiver<PiEvent>,
    pub activity_recorder: ActivityRecorder,
    pub capture: Arc<ToolCapture>,
    pub app_state: AppState,
    pub loupist_id: String,
    pub thread_id: String,
    pub prompt_request_id: u64,
    pub chat_started_at: Instant,
    pub setup_ms: u64,
    pub resumed: bool,
    pub turn_resources: ChatTurnResources,
    pub cache_seal: PiCacheSeal,
    pub harness: AgentHarnessSnapshot,
    pub use_memory: bool,
    pub update_memory: bool,
    pub execution_model: ExecutionModelLock,
    pub broker: ToolBrokerHandle,
    pub message_id: String,
    pub memory_revision: Option<String>,
    pub should_generate_title: bool,
    pub fallback_title: String,
    pub pins: Vec<crate::commands::ChatKnowledgePin>,
    /// What one Auto retry needs after a 404 `model_not_found`; spent once.
    pub gone_retry: Option<Box<super::model_gone::GoneRetrySeed>>,
    /// The first attempt's progress when the turn runs again on Auto.
    pub carried_progress: Option<ChatProgressProjection>,
}

/// What this turn's Pi JSONL must still match before a later turn resumes it.
pub(super) struct PiCacheSeal {
    pub connector_authority_cacheable: bool,
    pub connector_authority_sha256: String,
    pub runtime_policy_sha256: String,
    pub runtime_surface_sha256: String,
    pub derived_session_id: String,
    pub sent_envelope_digest: String,
}

/// `Break` means the reply already ended (failed, or stopped first) and was
/// recorded; the user turn stays durable either way.
pub(super) async fn launch_chat_turn(
    turn: AdmittedChatTurn,
    state: &AppState,
) -> Result<ControlFlow<RunStarted, LaunchedChatTurn>, CommandError> {
    // Bound in setup's creation order: an early exit drops the turn resources
    // first and the sink last, as when setup and launch were one function.
    let AdmittedChatTurn {
        #[cfg(feature = "eval-harness")]
        pi_agent_data_dir,
        request,
        on_event,
        surface,
        untrusted_context,
        chat_started_at,
        run_id,
        chat_controls,
        mut cancel,
        progress_started_at_ms,
        prompt,
        prepared_attachments,
        pi_artifacts,
        configured_model,
        profile,
        pi_execution,
        skill_files,
        connector_authority,
        web_search_policy,
        bound_workspace: _bound_workspace,
        runtime_policy_sha256,
        runtime_surface_sha256,
        policy,
        instruction_roots,
        host_context,
        harness,
        chat_attachments,
        chat,
        had_prior_messages,
        envelope,
        activity_recorder,
        memory_revision,
        mut turn_resources,
        user_message_id,
        current_prompt,
        current_image_bytes,
        current_pi_images,
        intended_cache,
        should_generate_title,
        assistant_message_id,
    } = turn;
    let reply = TerminalReply {
        state,
        on_event: &on_event,
        run_id: &run_id,
        thread_id: &chat.id,
        message_id: &assistant_message_id,
        memory_revision: &memory_revision,
        harness: &harness,
    };
    let broker_socket =
        tool_broker_endpoint(state.artifacts.broker_dir.join(format!("{run_id}.sock")));
    let result_links = std::sync::mpsc::channel::<(String, String)>();
    let entity_links = std::sync::mpsc::channel::<(String, Vec<String>)>();
    // Approval cards wake the run loop even while a tool waits and no Pi event arrives.
    let (approval_tx, approval_rx) = tokio::sync::mpsc::unbounded_channel::<(
        Option<String>,
        crate::chat::approvals::ChatApproval,
    )>();
    let capture = {
        let mut capture = ToolCapture::for_chat(assistant_message_id.clone());
        capture.unattended_run_id = (!surface.interactive()).then(|| run_id.clone());
        capture.surface = Some(surface.clone());
        capture.tool_trace = on_event.tool_trace.clone();
        capture.admit_untrusted_context(untrusted_context);
        capture.turn_taint.protect_instructions(instruction_roots);
        // Sites the user names stay readable after third-party text arrives.
        // A host briefing prompt includes observation text, not a user message;
        // its embedded URLs must not be admitted as sites the user typed.
        if !untrusted_context {
            capture.turn_taint.note_user_text(&request.prompt);
        }
        capture.tool_refusals = on_event.tool_refusals.clone();
        capture.bound_chat_id = Some(chat.id.clone());
        capture.turn_as_of = request.as_of.clone();
        capture.principle_user_message_id = Some(user_message_id.clone());
        capture.schedule_context =
            Some(crate::commands::tool_executor::schedules::ScheduleContext {
                model_profile_id: request.model_profile_id.clone(),
                thinking_level: request.thinking_level.clone(),
                run_options: request.run_options.clone(),
                use_memory: request.use_memory,
                update_memory: request.update_memory,
            });
        let schedule_sink = on_event.clone();
        let schedule_run_id = run_id.clone();
        capture.schedule_sink = Some(Arc::new(move |receipt| {
            let _ = schedule_sink.send(ChatStreamEvent::Schedule {
                run_id: schedule_run_id.clone(),
                receipt,
            });
        }));
        let evidence_event_sink = on_event.clone();
        let evidence_run_id = run_id.clone();
        let result_link_tx = result_links.0.clone();
        capture.evidence_sink = Some(Arc::new(move |evidence, call_id| {
            if let Some(call_id) = call_id {
                let _ = result_link_tx.send((call_id, evidence.id.clone()));
            }
            let _ = evidence_event_sink.send(ChatStreamEvent::Evidence {
                run_id: evidence_run_id.clone(),
                evidence: Box::new(evidence),
            });
        }));
        let entity_link_tx = entity_links.0.clone();
        capture.entity_sink = Some(Arc::new(move |call_id, ids| {
            let _ = entity_link_tx.send((call_id, ids));
        }));
        capture.approval_sink = Some(Arc::new(move |call_id, approval| {
            let _ = approval_tx.send((call_id, approval));
        }));
        capture.mcp_scratch_root = Some(turn_resources.run_scratch_path().join("mcp"));
        capture.mcp_pool = Some(state.mcp_pool.clone());
        capture.web_search_policy = web_search_policy;
        capture.set_chat_provider(&configured_model.provider);
        capture.search_cancel = crate::web::SearchCancel::from_watch(cancel.clone());
        Arc::new(capture)
    };
    let app_state = state.clone();
    let executor = Arc::new(AppToolExecutor {
        state: app_state.clone(),
        capture: capture.clone(),
        loupist_id: profile.id.clone(),
        capability_ids: harness.capability_ids.iter().cloned().collect(),
    });
    let broker = match start_tool_broker(broker_socket.clone(), policy, executor).await {
        Ok(broker) => broker,
        Err(_) => {
            on_event.set_failure_kind(ChatFailureKind::ProtocolError);
            return Ok(ControlFlow::Break(
                reply.failed_with_stop_precedence(pi_execution.model_lock(), None),
            ));
        }
    };
    if *cancel.borrow() {
        let _ = broker.shutdown().await;
        return Ok(ControlFlow::Break(
            reply.failed_with_stop_precedence(pi_execution.model_lock(), None),
        ));
    }
    let turn_envelope = match build_turn_envelope(
        envelope,
        state,
        &profile,
        request.use_memory,
        &surface,
        &capture,
    )
    .await
    {
        Ok(envelope) => envelope,
        Err(error) => {
            on_event.set_failure_kind(classify_turn_failure(&error, ChatFailureKind::StoreFailed));
            let _ = broker.shutdown().await;
            return Ok(ControlFlow::Break(
                reply.failed_with_stop_precedence(pi_execution.model_lock(), None),
            ));
        }
    };
    let mut pi_config = chat_pi_launch_config(
        &pi_artifacts,
        &state.artifacts.app_data_dir,
        &profile.id,
        &run_id,
        broker_socket,
        broker.token().to_owned(),
        turn_resources.pi_session(),
    );
    pi_config.pi_settings = pi_execution.pi_settings.clone();
    #[cfg(feature = "eval-harness")]
    {
        pi_config.agent_data_dir = pi_agent_data_dir;
    }
    #[cfg(any(test, feature = "e2e"))]
    let pi_config = match crate::app::with_live_pi_agent_data_dir_override(pi_config) {
        Ok(config) => config,
        Err(_) => {
            on_event.set_failure_kind(ChatFailureKind::ProtocolError);
            let _ = broker.shutdown().await;
            return Ok(ControlFlow::Break(
                reply.failed_with_stop_precedence(pi_execution.model_lock(), None),
            ));
        }
    };
    let pi_config = match pi_config
        .with_skills(skill_files)
        .and_then(|config| config.with_host_context(host_context))
    {
        Ok(config) => config,
        Err(error) => {
            on_event.set_failure_kind(classify_turn_failure(
                &CommandError::internal(error.to_string()),
                ChatFailureKind::ProtocolError,
            ));
            let _ = broker.shutdown().await;
            return Ok(ControlFlow::Break(
                reply.failed_with_stop_precedence(pi_execution.model_lock(), None),
            ));
        }
    };
    let previous_envelope_digest = intended_cache.as_ref().and_then(|cache| {
        (!cache.context_envelope_stale && !cache.context_envelope_sha256.is_empty())
            .then(|| cache.context_envelope_sha256.clone())
    });
    let resume = select_chat_pi_resume(
        intended_cache,
        had_prior_messages,
        turn_resources.pi_session().pi_session_id(),
        || uuid::Uuid::new_v4().to_string(),
    );
    let (pi, pi_events, execution_model, session_state, resumed) = match launch_chat_pi_resuming(
        pi_config.clone(),
        &pi_execution,
        turn_resources.pi_session_mut(),
        resume,
        &mut cancel,
    )
    .await
    {
        Ok(launched) => launched,
        Err(_error) => {
            on_event.set_failure_kind(classify_turn_failure(&_error, ChatFailureKind::PiExited));
            #[cfg(feature = "eval-harness")]
            if let Some(capture) = &on_event.eval_pi_event {
                capture(
                    &serde_json::json!({"type":"eval_error","code":_error.code,"message":crate::pi::e2e_failure_category(&_error.message)}),
                );
            }
            #[cfg(feature = "e2e")]
            if _error.code != "chat_cancelled" {
                crate::pi::trace_e2e_failure("launch", &_error.message);
            }
            let _ = broker.shutdown().await;
            return Ok(ControlFlow::Break(
                reply.failed_with_stop_precedence(pi_execution.model_lock(), None),
            ));
        }
    };
    let gone_retry = Box::new(super::model_gone::GoneRetrySeed {
        pi_config: pi_config.clone(),
        turn_envelope: turn_envelope.clone(),
        current_prompt: current_prompt.clone(),
        chat_id: chat.id.clone(),
        history: chat.messages[..chat.messages.len().saturating_sub(1)].to_vec(),
        chat_attachments: chat_attachments.clone(),
        current_image_bytes,
        current_pi_images: current_pi_images.clone(),
    });
    let mut historical_images = Vec::new();
    let (turn_envelope, sent_envelope_digest) = agent_harness::maybe_abbreviate_envelope(
        &turn_envelope,
        previous_envelope_digest.as_deref(),
        resumed,
    );
    let turn_envelope = agent_harness::render_turn_context(&turn_envelope);
    let pi_prompt = match pi_chat_turn_prompt(
        &current_prompt,
        &turn_envelope,
        (!resumed).then_some(ColdChatBootstrap {
            attachments_root: &chat_attachments,
            thread_id: &chat.id,
            history: &chat.messages[..chat.messages.len().saturating_sub(1)],
            current_image_bytes,
            current_image_count: current_pi_images.len(),
            historical_images: &mut historical_images,
        }),
    ) {
        Ok(prompt) => prompt,
        Err(error) => {
            on_event.set_failure_kind(classify_turn_failure(
                &CommandError::internal(error.to_string()),
                ChatFailureKind::ProtocolError,
            ));
            let _ = shutdown_failed_chat_setup(pi, turn_resources.pi_session_mut()).await;
            let _ = broker.shutdown().await;
            return Ok(ControlFlow::Break(
                reply.failed_with_stop_precedence(execution_model, None),
            ));
        }
    };
    let mut pi_images = historical_images;
    pi_images.extend(current_pi_images.iter().cloned());
    if *cancel.borrow() {
        let _ = shutdown_failed_chat_setup(pi, turn_resources.pi_session_mut()).await;
        let _ = broker.shutdown().await;
        return Ok(ControlFlow::Break(
            reply.failed_with_stop_precedence(execution_model, None),
        ));
    }
    let prompt_request_id = match pi.prompt_with_images(&pi_prompt, &pi_images).await {
        Ok(id) => id,
        Err(error) => {
            on_event.set_failure_kind(classify_turn_failure(
                &CommandError::internal(error.to_string()),
                ChatFailureKind::ProtocolError,
            ));
            let _ = shutdown_failed_chat_setup(pi, turn_resources.pi_session_mut()).await;
            let _ = broker.shutdown().await;
            return Ok(ControlFlow::Break(
                reply.failed_with_stop_precedence(execution_model, None),
            ));
        }
    };
    let setup_ms = elapsed_millis(chat_started_at);
    #[cfg(feature = "e2e")]
    eprintln!("Loupist E2E Chat diagnostic: phase=prompt category=sent");
    let provider_wait_started_at = Instant::now();
    let provider_idle_timeout = crate::pi::PI_HTTP_IDLE_TIMEOUT;
    #[cfg(test)]
    let provider_idle_timeout = on_event
        .provider_idle_timeout
        .unwrap_or(provider_idle_timeout);
    if *cancel.borrow() {
        let _ = pi.abort().await;
        let _ = shutdown_failed_chat_setup(pi, turn_resources.pi_session_mut()).await;
        let _ = broker.shutdown().await;
        return Ok(ControlFlow::Break(
            reply.failed_with_stop_precedence(execution_model, None),
        ));
    }
    let task_run_id = run_id.clone();
    let thread_id = chat.id.clone();
    let task_message_id = assistant_message_id.clone();
    let task_loupist_id = profile.id.clone();
    let task_use_memory = request.use_memory;
    let task_update_memory = request.update_memory;
    let task_pins = request.pins.clone();
    let task_connector_authority_sha256 = connector_authority.sha256;
    let task_connector_authority_cacheable = connector_authority.cacheable;
    let task_derived_session_id = session_state.session_id;
    let fallback_title = fallback_chat_title(if prompt.is_empty() {
        &prepared_attachments[0].record.filename
    } else {
        &prompt
    });
    Ok(ControlFlow::Continue(LaunchedChatTurn {
        progress_started_at_ms,
        provider_wait_started_at,
        provider_idle_timeout,
        result_links: result_links.1,
        on_event,
        run_id: task_run_id,
        entity_links: entity_links.1,
        approval_rx,
        cancel,
        pi,
        chat_controls,
        pi_events,
        activity_recorder,
        capture,
        app_state,
        loupist_id: task_loupist_id,
        thread_id,
        prompt_request_id,
        chat_started_at,
        setup_ms,
        resumed,
        turn_resources,
        cache_seal: PiCacheSeal {
            connector_authority_cacheable: task_connector_authority_cacheable,
            connector_authority_sha256: task_connector_authority_sha256,
            runtime_policy_sha256,
            runtime_surface_sha256,
            derived_session_id: task_derived_session_id,
            sent_envelope_digest,
        },
        harness,
        use_memory: task_use_memory,
        update_memory: task_update_memory,
        execution_model,
        broker,
        message_id: task_message_id,
        memory_revision,
        should_generate_title,
        fallback_title,
        pins: task_pins,
        gone_retry: Some(gone_retry),
        carried_progress: None,
    }))
}

/// Renders the per-turn context envelope sent with the prompt: live time, the
/// Memory index and charter, selected references, working notes, the Journal,
/// the selected artifact, principles and the Loupist's voice. The charter
/// delivery and principle context are admitted into `capture` here, before
/// Pi starts. Only the Journal read can fail.
async fn build_turn_envelope(
    inputs: TurnEnvelopeInputs,
    state: &AppState,
    profile: &LoupistProfile,
    use_memory: bool,
    surface: &crate::chat::surface::ChatSurface,
    capture: &ToolCapture,
) -> Result<String, CommandError> {
    let TurnEnvelopeInputs {
        mentioned_connections,
        selected_artifact,
        host_memory,
        followed,
        selected_references,
        working_context,
        left_out,
    } = inputs;
    capture.note_left_out(left_out);
    let (learned_index, charter, charter_read_ref) = if let Some(memory) = &host_memory {
        let learned_index = memory
            .entries
            .iter()
            .map(|entry| agent_harness::LearnedMemoryIndexEntry {
                id: entry.id.clone(),
                title: entry.title.clone(),
                summary: entry.summary.clone(),
                as_of: entry.as_of.clone(),
                kind: match entry.kind.as_str() {
                    "lens" => "Lens".to_string(),
                    "world" => "World".to_string(),
                    other => other.to_string(),
                },
            })
            .collect::<Vec<_>>();
        let charter = memory
            .charter
            .clone()
            .filter(|body| !body.trim().is_empty());
        // Prompt injection is admitted here, before Pi starts. It is not a tool ACK.
        // A later research cite of charter_read_ref records this body, not a later read.
        if let (Some(_), Some(staged)) = (&charter, memory.charter_delivery.clone()) {
            if let Some(receipt) = staged.lens_receipt("context") {
                capture.context_lens_receipts.lock().await.push(receipt);
            }
            capture.context_reads.lock().await.push(staged);
        }
        capture.principle_context.lock().await.extend(
            memory
                .principles
                .iter()
                .map(|p| (p.record_id.clone(), p.revision_id.clone())),
        );
        // The user's rules and perspective must not leave in a URL after
        // third-party text arrives.
        for principle in &memory.principles {
            capture
                .turn_taint
                .protect_private_text(&principle.statement);
        }
        if let Some(charter) = &charter {
            capture.turn_taint.protect_private_text(charter);
        }
        if memory.shared_pauses {
            // A shared item accepted despite a scan finding pauses commands this turn.
            capture.turn_taint.mark_untrusted_text();
            capture.tool_trace.mark_untrusted_text();
        }
        (learned_index, charter, memory.charter_read_ref.clone())
    } else {
        (Vec::new(), None, None)
    };
    let lens_catalog = host_memory
        .as_ref()
        .map(|memory| (memory.lens_shown, memory.lens_total));
    let turn_envelope = agent_harness::turn_envelope_block(
        Utc::now(),
        use_memory,
        &learned_index,
        charter.as_deref(),
        charter_read_ref.as_deref(),
        lens_catalog,
        host_memory
            .as_ref()
            .map(|memory| (memory.world_shown, memory.world_total)),
        &mentioned_connections,
    )
    .unwrap_or_else(|_| {
        serde_json::to_string(&serde_json::json!({
            "live_time": agent_harness::live_time_envelope(Utc::now())
        }))
        .unwrap_or_else(|_| r#"{"live_time":{"not_evidence":true}}"#.into())
    });
    let turn_envelope = agent_harness::attach_chat_surface(&turn_envelope, surface);
    let turn_envelope = match capture.turn_as_of.as_deref() {
        Some(cutoff) => agent_harness::attach_as_of(&turn_envelope, cutoff),
        None => turn_envelope,
    };
    let turn_envelope = agent_harness::attach_relevant_memory(
        &turn_envelope,
        host_memory
            .as_ref()
            .map(|memory| memory.relevant.as_slice())
            .unwrap_or_default(),
    );
    let turn_envelope = if use_memory {
        let cutoff = capture.cutoff();
        let extension = state
            .store
            .lock_loupist(&profile.id)
            .ok()
            .and_then(|connection| {
                crate::knowledge::visible_extension(&connection, &profile.id, &|at| {
                    cutoff.is_none_or(|cutoff| cutoff.recorded(at))
                })
                .ok()
            });
        match extension {
            Some(extension) => agent_harness::attach_world_kinds(
                &turn_envelope,
                &crate::knowledge::world_kinds_block(&extension),
            ),
            None => turn_envelope,
        }
    } else {
        turn_envelope
    };
    let turn_envelope = crate::commands::knowledge::attach_selected_references(
        &turn_envelope,
        selected_references.as_ref(),
    );
    let turn_envelope = crate::commands::knowledge::attach_working_context(
        &turn_envelope,
        working_context.as_ref(),
    );
    let (turn_envelope, journal_left_out) =
        crate::commands::knowledge::working_notes::attach_journal_context_as_of(
            state,
            &profile.id,
            &turn_envelope,
            capture.cutoff(),
        )?;
    capture.note_left_out(journal_left_out);
    let turn_envelope = crate::commands::artifact_context::attach_selected_artifact(
        &turn_envelope,
        selected_artifact,
    );
    let turn_envelope = if let Some(memory) = &host_memory {
        crate::commands::knowledge::principles::attach_context(
            &turn_envelope,
            &memory.principles_block,
            &memory.principles,
        )
    } else {
        turn_envelope
    };
    // Record only the bounded principles actually put in this turn's envelope.
    if let Some(memory) = &host_memory {
        let attached = serde_json::from_str::<serde_json::Value>(&turn_envelope)
            .ok()
            .is_some_and(|value| {
                value["User principles"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
            });
        if attached {
            capture
                .context_lens_receipts
                .lock()
                .await
                .extend(memory.principle_receipts.clone());
        }
    }
    let turn_envelope = agent_harness::attach_loupist_context(
        &turn_envelope,
        &profile.instructions,
        profile.tone,
        followed.as_ref(),
    );
    Ok(turn_envelope)
}
