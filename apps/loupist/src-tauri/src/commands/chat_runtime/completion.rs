//! Completed-turn commit: one host transaction publishes the assistant reply
//! with its staged Memory, receipts, citations and artifacts, then emits the
//! completion, or recovers it when the commit outcome is ambiguous.
use super::*;

/// Borrowed view of a completed turn for its canonical commit.
pub(super) struct CompletedTurn<'a> {
    pub reply: &'a TerminalReply<'a>,
    pub task_loupist_id: &'a str,
    pub execution_model: &'a ExecutionModelLock,
    pub capture: &'a ToolCapture,
    pub turn_resources: &'a mut ChatTurnResources,
    pub memory_finalization_writer: &'a mut Option<RunRegistration>,
    pub progress: &'a mut ChatProgressProjection,
    pub failure: &'a mut Option<String>,
    pub content: String,
    pub sealed_cache: Option<PiSessionCache>,
    pub task_pins: &'a [crate::commands::ChatKnowledgePin],
    pub task_update_memory: bool,
    pub should_generate_title: bool,
    pub fallback_title: &'a String,
    pub chat_started_at: Instant,
    pub setup_ms: u64,
    pub first_text_ms: Option<u64>,
    pub generation_ms: Option<u64>,
    pub resumed: bool,
    pub token_usage: AssistantTokenUsage,
}

/// `Break` means a failed reply was already persisted and emitted, so the
/// caller must not record a second terminal outcome.
pub(super) async fn commit_completed_turn(turn: CompletedTurn<'_>) -> ControlFlow<()> {
    let CompletedTurn {
        reply,
        task_loupist_id,
        execution_model,
        capture,
        turn_resources,
        memory_finalization_writer,
        progress,
        failure,
        mut content,
        sealed_cache,
        task_pins,
        task_update_memory,
        should_generate_title,
        fallback_title,
        chat_started_at,
        setup_ms,
        first_text_ms,
        generation_ms,
        resumed,
        token_usage,
    } = turn;
    let TerminalReply {
        state: app_state,
        on_event,
        run_id: task_run_id,
        thread_id,
        message_id: task_message_id,
        memory_revision,
        harness,
    } = *reply;
    // A custom result with Don't save enabled suppresses every staged
    // Memory mutation, including writes staged before that result arrived.
    let task_update_memory = task_update_memory && !capture.turn_taint.memory_blocked();
    let all_memories = capture
        .memories
        .lock()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let mut lens_deliveries = capture.context_lens_receipts.lock().await.clone();
    lens_deliveries.extend(
        capture
            .staged_reads
            .lock()
            .await
            .iter()
            .filter_map(|read| read.lens_receipt("exact_read")),
    );
    let lens_receipts = lens_deliveries
        .iter()
        .map(|receipt| (receipt.record_id.clone(), receipt.clone()))
        .collect::<std::collections::BTreeMap<_, _>>()
        .into_values()
        .take(256)
        .collect::<Vec<_>>();
    let host_composed = content.trim().is_empty();
    let has_staged_memory = task_update_memory
        && (!capture.staged_principles.lock().await.is_empty()
            || !capture.staged_sources.lock().await.is_empty()
            || !capture.staged_filings.lock().await.is_empty()
            || capture.staged_setup.lock().await.is_some()
            || capture.staged_research.lock().await.is_some());
    if host_composed && !has_staged_memory {
        *failure = Some("Pi settled without an assistant message".into());
    } else {
        if host_composed {
            // Provisional transaction-only text, replaced by committed receipts below.
            content = "Finalizing saved Memory.".into();
        }
        let message_id = task_message_id.to_owned();
        let memory_trace = match durable_memory_trace(all_memories) {
            Ok(trace) => trace,
            Err(_) => {
                on_event.set_failure_kind(ChatFailureKind::StoreFailed);
                progress.fail_plan();
                let _ = reply.failed(
                    execution_model.clone(),
                    finish_chat_progress(on_event, task_run_id, progress, now_ms(), false),
                );
                return ControlFlow::Break(());
            }
        };
        let title = should_generate_title.then(|| fallback_title.clone());
        let created_at_ms = now_ms();
        let progress_snapshot =
            finish_chat_progress(on_event, task_run_id, progress, created_at_ms, false);
        let evidence_refs = capture.evidence_refs.lock().await.clone();
        // Rust checks each number link against the evidence it names; the
        // answer's text cannot set a status (finance-show-the-math.md §7.2).
        let number_checks = if host_composed {
            Vec::new()
        } else {
            capture.number_checks(&content, &evidence_refs).await
        };
        // A paragraph that only links a grid result shows that grid as a card.
        let sensitivity_cards = if host_composed {
            Vec::new()
        } else {
            capture.sensitivity_cards(&content, &evidence_refs).await
        };
        let as_of_report = capture.as_of_report(&on_event.tool_trace.snapshot());
        let artifact_commits = capture.artifacts.lock().await.clone();
        let artifact_refs = artifact_commits
            .iter()
            .map(|commit| commit.revision.artifact_ref(commit.artifact.title.clone()))
            .collect::<Vec<_>>();
        // Reserve the FIFO writer while the Chat reader is still
        // active. The handoff then releases this turn's reader and
        // waits for older readers without allowing a new reader to
        // barge between Chat completion and canonical finalization.
        on_event.set_failure_kind(ChatFailureKind::StoreFailed);
        let (_write_id, pending_writer) =
            match memory_updates::reserve_chat_memory_finalization(app_state, task_loupist_id) {
                Ok(reservation) => reservation,
                Err(error) => {
                    on_event.set_failure_kind(classify_turn_failure(
                        &error,
                        ChatFailureKind::StoreFailed,
                    ));
                    let _ = reply.failed(execution_model.clone(), progress_snapshot.clone());
                    return ControlFlow::Break(());
                }
            };
        let (memory_writer, ready) = turn_resources.handoff_to_memory_write(pending_writer).await;
        *memory_finalization_writer = Some(memory_writer);
        if let Err(error) = ready {
            on_event.set_failure_kind(classify_turn_failure(&error, ChatFailureKind::StoreFailed));
            let _ = reply.failed(execution_model.clone(), progress_snapshot.clone());
            return ControlFlow::Break(());
        }
        // The broker has settled; snapshot staged Memory with async locks
        // before entering the synchronous storage transaction.
        let saved_sources = if task_update_memory {
            capture.staged_sources.lock().await.clone()
        } else {
            Vec::new()
        };
        let mut saved_reads = if task_update_memory {
            capture.staged_reads.lock().await.clone()
        } else {
            Vec::new()
        };
        if task_update_memory {
            saved_reads.extend(capture.context_reads.lock().await.clone());
        }
        let saved_research = if task_update_memory {
            capture.staged_research.lock().await.clone()
        } else {
            None
        };
        let saved_principles = capture.staged_principles.lock().await.clone();
        let staged_handoff = capture.staged_handoff.lock().await.clone();
        let citation_reads = capture.staged_reads.lock().await.clone();
        // Asset pages change only with Memory saving on and never under an as-of date.
        let asset_work = task_update_memory && !capture.as_of_turn();
        let saved_theses = if asset_work {
            capture.staged_theses.lock().await.clone()
        } else {
            Vec::new()
        };
        let saved_earnings = if asset_work {
            capture.staged_earnings.lock().await.clone()
        } else {
            Vec::new()
        };
        let saved_vocabulary = if task_update_memory {
            capture.staged_vocabulary.lock().await.clone()
        } else {
            Vec::new()
        };
        let saved_filings = if task_update_memory {
            capture.staged_filings.lock().await.clone()
        } else {
            Vec::new()
        };
        let saved_setup = if task_update_memory {
            capture.staged_setup.lock().await.clone()
        } else {
            None
        };
        let watchlist_changes = saved_setup
            .as_ref()
            .map(|setup| {
                crate::commands::tool_executor::apply_setup_watchlist(
                    app_state,
                    task_loupist_id,
                    setup,
                )
            })
            .unwrap_or_default();
        let unattended = capture.unattended_run_id.is_some();
        let turn_observations = match asset_work
            .then(|| crate::commands::assets::view_context(app_state).ok())
            .flatten()
        {
            Some(context) => {
                capture
                    .turn_observations(&crate::commands::assets::followed(&context))
                    .await
            }
            None => Vec::new(),
        };
        let mut turn_assets = crate::commands::assets::TurnAssets::default();
        let mut completion_performance = None;
        let finalized = memory_updates::apply_chat_memory_update_with_registered_finalize(
            app_state,
            task_loupist_id,
            memory_finalization_writer
                .as_ref()
                .expect("acquired Memory writer"),
            || {
                let expected = app_state
                    .store
                    .get_chat(thread_id)
                    .map_err(map_store)?
                    .ok_or_else(|| CommandError::not_found("chat thread"))?;
                let mut chat = expected.clone();
                let mut completed_message = ChatMessage {
                    tool_refusals: on_event.tool_refusals(),
                    performance: None,
                    feedback: None,
                    tool_trace: Some(on_event.tool_trace.snapshot()),
                    schedule_receipts: on_event.schedule_receipts(),
                    failure_kind: None,
                    host_composed,
                    steer: false,
                    retry_after_seconds: None,
                    delivery: None,
                    replaced_model: on_event.replaced_model(),
                    as_of: capture.turn_as_of.clone(),
                    number_checks: number_checks.clone(),
                    sensitivity_cards: sensitivity_cards.clone(),
                    as_of_report: as_of_report.clone(),
                    alert: None,
                    watch: None,
                    earnings: None,
                    vocabulary: None,
                    handoff: None,
                    link: None,
                    id: message_id.clone(),
                    role: ChatRole::Assistant,
                    status: ChatMessageStatus::Complete,
                    content: content.clone(),
                    created_at_ms,
                    lens_receipts: lens_receipts.clone(),
                    memory_refs: memory_trace.refs.clone(),
                    observed_exact_count: memory_trace.observed_exact_count,
                    refs_truncated: memory_trace.refs_truncated,
                    refs_digest: memory_trace.refs_digest.clone(),
                    memory_update: None,
                    memory_revision: memory_revision.clone(),
                    execution_model: Some(execution_model.clone()),
                    agent_harness: Some(harness.clone()),
                    attachments: Vec::new(),
                    artifact_refs,
                    evidence_refs,
                    progress: progress_snapshot.clone(),
                };
                chat.messages.push(completed_message.clone());
                let title_applied = match &title {
                    Some(title) if chat.title.is_empty() => {
                        chat.title = title.clone();
                        true
                    }
                    _ => false,
                };
                // A host receipt is absent from Pi's JSONL; rebuild canonical history next turn.
                chat.pi_session_cache = if host_composed {
                    None
                } else {
                    sealed_cache.clone()
                };
                chat.updated_at_ms = created_at_ms.max(chat.updated_at_ms + 1).max(
                    artifact_commits
                        .iter()
                        .map(|commit| commit.artifact.updated_at_ms)
                        .max()
                        .unwrap_or_default(),
                );
                let blobs = crate::commands::knowledge::knowledge_blobs_for_loupist(
                    app_state,
                    task_loupist_id,
                )?;
                let persist = |transaction: &rusqlite::Transaction<'_>| {
                    let saved = crate::commands::knowledge::persist_saved_turn_sources(
                        transaction,
                        &blobs,
                        task_loupist_id,
                        thread_id,
                        &message_id,
                        created_at_ms,
                        task_pins,
                        task_update_memory,
                        &saved_sources,
                        &saved_reads,
                        saved_research.as_ref(),
                    )
                    .map_err(|error| {
                        on_event.set_failure_kind(classify_turn_failure(
                            &error,
                            ChatFailureKind::MemoryCommitFailed,
                        ));
                        crate::store::StoreError::Invalid(error.to_string())
                    })?;
                    let mut changes = saved.changes;
                    if task_update_memory {
                        changes.extend(
                            crate::commands::knowledge::principles::persist_principles(
                                transaction,
                                task_loupist_id,
                                &saved_principles,
                                created_at_ms,
                            )
                            .map_err(|error| {
                                on_event.set_failure_kind(classify_turn_failure(
                                    &error,
                                    ChatFailureKind::MemoryCommitFailed,
                                ));
                                crate::store::StoreError::Invalid(error.to_string())
                            })?,
                        );
                        changes.extend(
                            crate::commands::tool_executor::commit_filings(
                                transaction,
                                &blobs,
                                task_loupist_id,
                                &crate::knowledge::lens_page::FilingOrigin::Turn {
                                    chat_id: thread_id.to_owned(),
                                    message_id: message_id.clone(),
                                    user_message_id: capture.principle_user_message_id.clone(),
                                    unattended,
                                },
                                created_at_ms,
                                &saved_filings,
                                &saved.retained,
                            )
                            .map_err(|error| {
                                on_event.set_failure_kind(ChatFailureKind::MemoryCommitFailed);
                                crate::store::StoreError::Invalid(error.to_string())
                            })?,
                        );
                    }
                    crate::commands::knowledge::memory_usage::persist_deliveries(
                        transaction,
                        &blobs,
                        task_loupist_id,
                        thread_id,
                        &message_id,
                        &lens_deliveries,
                        created_at_ms,
                    )
                    .map_err(|e| crate::store::StoreError::Invalid(e.to_string()))?;
                    crate::commands::knowledge::principles::persist_citations(
                        transaction,
                        &blobs,
                        task_loupist_id,
                        &message_id,
                        &content,
                        &citation_reads,
                        created_at_ms,
                    )
                    .map_err(|e| crate::store::StoreError::Invalid(e.to_string()))?;
                    turn_assets = crate::commands::assets::commit_turn(
                        transaction,
                        task_loupist_id,
                        thread_id,
                        &message_id,
                        created_at_ms,
                        &saved_theses,
                        &turn_observations,
                    )
                    .map_err(|e| crate::store::StoreError::Invalid(e.to_string()))?;
                    crate::commands::tool_executor::commit_earnings(
                        transaction,
                        task_loupist_id,
                        &message_id,
                        created_at_ms,
                        &saved_earnings,
                        unattended,
                    )
                    .map_err(|e| crate::store::StoreError::Invalid(e.to_string()))?;
                    let mut skipped_kinds = crate::commands::tool_executor::commit_vocabulary(
                        transaction,
                        task_loupist_id,
                        thread_id,
                        created_at_ms,
                        &saved_vocabulary,
                    )
                    .map_err(|e| crate::store::StoreError::Invalid(e.to_string()))?;
                    let mut setup = watchlist_changes.clone();
                    if let Some(staged) = &saved_setup {
                        let (changed, skipped) = crate::commands::tool_executor::commit_setup(
                            transaction,
                            &blobs,
                            task_loupist_id,
                            thread_id,
                            created_at_ms,
                            staged,
                        )?;
                        setup.extend(changed);
                        skipped_kinds.extend(skipped);
                    }
                    if let Some(staged) = &staged_handoff {
                        crate::chat::handoff::commit(
                            transaction,
                            task_loupist_id,
                            staged,
                            created_at_ms,
                        )?;
                    }
                    if host_composed && changes.is_empty() && setup.is_empty() {
                        on_event.set_failure_kind(ChatFailureKind::EmptyReply);
                        return Err(crate::store::StoreError::Invalid(
                            "No committed receipts for an empty answer".into(),
                        ));
                    }
                    // Include staged Memory writes in total time, then
                    // freeze one receipt for persistence, live delivery,
                    // and recovery. Final report serialization/CAS and
                    // transaction COMMIT occur after this boundary.
                    let performance = ChatPerformanceDto {
                        setup_ms,
                        first_text_ms,
                        generation_ms: generation_ms.unwrap_or_else(|| {
                            elapsed_millis(chat_started_at).saturating_sub(setup_ms)
                        }),
                        total_ms: elapsed_millis(chat_started_at),
                        session_cache: if resumed {
                            ChatSessionCacheStatusDto::Warm
                        } else {
                            ChatSessionCacheStatusDto::Cold
                        },
                        input_tokens: token_usage.input,
                        output_tokens: token_usage.output,
                        cache_read_tokens: token_usage.cache_read,
                        cache_write_tokens: token_usage.cache_write,
                    };
                    completion_performance = Some(performance.clone());
                    completed_message = crate::commands::knowledge::write_chat_learning_report(
                        transaction,
                        &chat,
                        &message_id,
                        task_update_memory,
                        changes,
                        skipped_kinds,
                        setup,
                        Some(performance),
                        turn_assets.trace_notes(elapsed_millis(chat_started_at)),
                    )?;
                    content = completed_message.content.clone();
                    Ok(())
                };
                if artifact_commits.is_empty() {
                    app_state
                        .store
                        .replace_chat_with(&expected, &chat, persist)
                        .map_err(map_store)?;
                } else {
                    app_state
                        .store
                        .save_chat_with_artifacts_and(&expected, &chat, &artifact_commits, persist)
                        .map_err(map_store)?;
                }
                let performance = completed_message.performance.clone().ok_or_else(|| {
                    CommandError::internal("completed Chat performance is missing")
                })?;
                Ok((title_applied, completed_message, performance))
            },
        );
        if saved_setup.is_some()
            && app_state
                .store
                .resync_folder_registration(task_loupist_id)
                .is_err()
        {
            eprintln!("The Loupist list could not pick up the new focus yet");
        }
        match finalized {
            Ok((title_applied, completed_message, performance)) => {
                app_state.graph.wake();
                if turn_assets.crossed {
                    // The Thread announces a crossed line once; a busy Thread gets it later.
                    let _ = crate::chat::home::deliver_finished_runs(app_state, task_loupist_id);
                }
                emit_completed_chat(
                    on_event,
                    task_run_id,
                    &completed_message,
                    if title_applied { title.clone() } else { None },
                    execution_model,
                    harness,
                    performance,
                );
                if asset_work {
                    // Filing sections are compared after the commit with no lock
                    // held; a comparison that cannot run leaves "not compared yet".
                    if let Err(error) = crate::commands::assets::align_turn_filings(
                        app_state,
                        task_loupist_id,
                        &message_id,
                        created_at_ms,
                    ) {
                        eprintln!("Filing sections were not compared: {}", error.code);
                    }
                }
            }
            Err(error) => {
                let recovered = completion_performance.as_ref().and_then(|performance| {
                    recovered_canonical_completion(
                        app_state.store.as_ref(),
                        thread_id,
                        task_message_id,
                        CanonicalCompletionExpectation {
                            content: &content,
                            memory_revision,
                            execution_model,
                            agent_harness: harness,
                            performance,
                            title: title.as_deref(),
                        },
                    )
                    .map(|(message, title)| (message, title, performance.clone()))
                });
                if let Some((completed_message, recovered_title, performance)) = recovered {
                    emit_completed_chat(
                        on_event,
                        task_run_id,
                        &completed_message,
                        recovered_title,
                        execution_model,
                        harness,
                        performance,
                    );
                } else {
                    if error.code == "conflict" {
                        on_event.set_failure_kind(ChatFailureKind::CancelledByConflict);
                    }
                    *failure = Some(error.message);
                }
            }
        }
    }
    ControlFlow::Continue(())
}
