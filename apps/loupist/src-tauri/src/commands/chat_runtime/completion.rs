//! Completed-turn commit: one host transaction publishes the assistant reply
//! with its staged Memory, receipts, citations and artifacts, then emits the
//! completion, or recovers it when the commit outcome is ambiguous.
use super::*;
use crate::{
    chat::{artifacts::ArtifactCommit, progress::ChatSaveActivity},
    domain::{MemorySaveSkipReason, MemoryUpdateResult, MemoryUpdateStatus},
    runs::log::{CommitStage, MemoryCommitFailure},
};

fn skipped_memory(
    reason: MemorySaveSkipReason,
    discarded_memory_items: Option<Vec<crate::domain::DiscardedMemoryItem>>,
) -> MemoryUpdateResult {
    MemoryUpdateResult {
        discarded_memory_items,
        status: MemoryUpdateStatus::NoChange,
        skip_reason: Some(reason),
        commit_id: None,
        changes: Vec::new(),
        skipped_kinds: Vec::new(),
        setup: Vec::new(),
    }
}

fn checkpoint_answer(
    reply: &TerminalReply<'_>,
    message: &ChatMessage,
    artifacts: &[ArtifactCommit],
    quiet_request: Option<&str>,
) -> Result<i64, CommandError> {
    let store = &reply.state.store;
    let expected = store
        .get_chat_in_loupist(reply.loupist_id, reply.thread_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("chat thread"))?;
    let mut chat = expected.clone();
    if quiet_request.is_none() || !artifacts.is_empty() {
        chat.updated_at_ms = message.created_at_ms.max(expected.updated_at_ms + 1).max(
            artifacts
                .iter()
                .map(|commit| commit.artifact.updated_at_ms)
                .max()
                .unwrap_or_default(),
        );
    }
    chat.messages.push(message.clone());
    chat.pi_session_cache = None;
    let mark_quiet = |tx: &rusqlite::Transaction<'_>| {
        if let Some(run_id) = quiet_request {
            let _ = crate::scheduled_tasks::store::mark_quiet(tx, reply.loupist_id, run_id);
        }
        Ok(())
    };
    if artifacts.is_empty() {
        store
            .replace_chat_with(&expected, &chat, mark_quiet)
            .map_err(map_store)?;
    } else {
        store
            .save_chat_with_artifacts_and(&expected, &chat, artifacts, mark_quiet)
            .map_err(map_store)?;
    }
    Ok(expected.updated_at_ms)
}

fn checkpoint_save_activity(
    reply: &TerminalReply<'_>,
    activity: &ChatSaveActivity,
) -> Result<(), CommandError> {
    crate::store::bounded_chat_save(|| {
        let store = &reply.state.store;
        let expected = store
            .get_chat_in_loupist(reply.loupist_id, reply.thread_id)
            .map_err(map_store)?
            .ok_or_else(|| CommandError::not_found("chat thread"))?;
        let mut chat = expected.clone();
        let progress = chat
            .messages
            .iter_mut()
            .find(|message| message.id == reply.message_id)
            .and_then(|message| message.progress.as_mut())
            .ok_or_else(|| CommandError::not_found("answer progress"))?;
        progress.save_activity = Some(activity.clone());
        store.replace_chat(&expected, &chat).map_err(map_store)
    })
}

fn finish_skipped_save(
    reply: &TerminalReply<'_>,
    message: &ChatMessage,
    model: &ExecutionModelLock,
    title: Option<&str>,
    reason: MemorySaveSkipReason,
    applied_watchlist: &[(crate::domain::SetupChange, String)],
) {
    let Some(performance) = &message.performance else {
        reply
            .on_event
            .set_failure_kind(ChatFailureKind::MemoryCommitFailed);
        let _ = reply.failed(model.clone(), message.progress.clone());
        return;
    };
    let store = &reply.state.store;
    let persisted = crate::store::bounded_chat_save(|| {
        let expected = store
            .get_chat_in_loupist(reply.loupist_id, reply.thread_id)
            .map_err(map_store)?
            .ok_or_else(|| CommandError::not_found("chat thread"))?;
        let mut chat = expected.clone();
        let saved = chat
            .messages
            .iter_mut()
            .find(|saved| saved.id == reply.message_id)
            .ok_or_else(|| CommandError::not_found("saved answer"))?;
        let mut discarded = message
            .memory_update
            .as_ref()
            .and_then(|update| update.discarded_memory_items.clone());
        if let Some(items) = &mut discarded {
            // Watchlist changes use the catalog transaction and can survive
            // a later failure in the Memory transaction.
            items.retain(|item| {
                item.kind != crate::domain::DiscardedMemoryKind::Watchlist
                    || !applied_watchlist
                        .iter()
                        .any(|(_, title)| *title == item.title)
            });
        }
        saved.memory_update = Some(skipped_memory(
            reason,
            discarded.filter(|items| !items.is_empty()),
        ));
        saved.progress.clone_from(&message.progress);
        if let Some(activity) = saved
            .progress
            .as_mut()
            .and_then(|progress| progress.save_activity.as_mut())
        {
            activity.finished_at_ms = Some(now_ms().max(activity.started_at_ms));
        }
        let saved = saved.clone();
        let applied_title = title.filter(|_| chat.title.is_empty()).map(str::to_owned);
        if let Some(title) = &applied_title {
            chat.title.clone_from(title);
        }
        store.replace_chat(&expected, &chat).map_err(map_store)?;
        Ok::<_, CommandError>((saved, applied_title))
    });
    match persisted {
        Ok((saved, applied_title)) => emit_completed_chat(
            reply.on_event,
            reply.run_id,
            &saved,
            applied_title,
            model,
            reply.harness,
            performance.clone(),
        ),
        Err(error) => {
            record_commit_failure(reply.on_event, CommitStage::Chat, error.code.as_str());
            // The checkpoint already holds the answer and artifacts. Emit
            // that durable outcome if the final skip receipt cannot replace it.
            emit_completed_chat(
                reply.on_event,
                reply.run_id,
                message,
                None,
                model,
                reply.harness,
                performance.clone(),
            );
        }
    }
}

/// Borrowed view of a completed turn for its canonical commit.
pub(super) struct CompletedTurn<'a> {
    pub reply: &'a TerminalReply<'a>,
    pub task_loupist_id: &'a str,
    pub execution_model: &'a ExecutionModelLock,
    pub capture: &'a ToolCapture,
    pub runtime_settled: bool,
    pub save_started_at_ms: i64,
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
    pub answer_text_ms: Option<u64>,
    pub generation_ms: Option<u64>,
    pub resumed: bool,
    pub token_usage: AssistantTokenUsage,
}

fn record_commit_failure(sink: &ChatEventSink, stage: CommitStage, code: &str) {
    if let Some((_, record)) = sink
        .run_log
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
    {
        // Preserve the first failing phase when the enclosing Chat transaction
        // reports the same error again after rollback.
        record
            .memory_commit_failure
            .get_or_insert_with(|| MemoryCommitFailure::from_code(stage, code));
    }
}

/// `Break` means a failed reply was already persisted and emitted, so the
/// caller must not record a second terminal outcome.
pub(super) async fn commit_completed_turn(turn: CompletedTurn<'_>) -> ControlFlow<()> {
    let CompletedTurn {
        reply,
        task_loupist_id,
        execution_model,
        capture,
        runtime_settled,
        save_started_at_ms,
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
        answer_text_ms,
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
        loupist_id: _,
    } = *reply;
    // A custom result with Don't save enabled suppresses every staged
    // Memory mutation, including writes staged before that result arrived.
    let task_update_memory = task_update_memory && !capture.turn_taint.memory_blocked();
    let mut missing_snapshot = !runtime_settled;
    // Once the broker has drained, there should be no task holding capture
    // state. A broken cleanup must not keep a settled reply waiting here.
    macro_rules! snapshot {
        ($mutex:expr, $guard:ident => $read:expr) => {
            match $mutex.try_lock() {
                Ok($guard) => $read,
                Err(_) => {
                    missing_snapshot = true;
                    Default::default()
                }
            }
        };
        ($mutex:expr) => {
            snapshot!($mutex, guard => guard.clone())
        };
    }
    let all_memories =
        snapshot!(capture.memories, guard => guard.values().cloned().collect::<Vec<_>>());
    let mut lens_deliveries = snapshot!(capture.context_lens_receipts);
    let citation_reads = snapshot!(capture.staged_reads);
    lens_deliveries.extend(
        citation_reads
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
    let quiet_request = match capture.scheduled_run_id_for_commit() {
        Some(run_id) => {
            match tokio::time::timeout(Duration::from_secs(2), capture.has_data_for_quiet()).await {
                Ok(true) => Some(run_id),
                Ok(false) => None,
                Err(_) => {
                    missing_snapshot = true;
                    None
                }
            }
        }
        None => None,
    };
    if content.trim().is_empty() && quiet_request.is_some() {
        content = crate::scheduled_tasks::QUIET_NO_REPORT.into();
    }
    let host_composed = content.trim().is_empty();
    let saved_principles = snapshot!(capture.staged_principles);
    let saved_sources = if task_update_memory {
        snapshot!(capture.staged_sources)
    } else {
        Vec::new()
    };
    let saved_filings = if task_update_memory {
        snapshot!(capture.staged_filings)
    } else {
        Vec::new()
    };
    let saved_setup = if task_update_memory {
        snapshot!(capture.staged_setup)
    } else {
        None
    };
    let mut saved_research = if task_update_memory {
        snapshot!(capture.staged_research)
    } else {
        None
    };
    let has_staged_memory = task_update_memory
        && (!saved_principles.is_empty()
            || !saved_sources.is_empty()
            || !saved_filings.is_empty()
            || saved_setup.is_some()
            || saved_research.is_some());
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
            Err(error) => {
                record_commit_failure(on_event, CommitStage::Chat, error.code.as_str());
                missing_snapshot = true;
                match empty_memory_trace() {
                    Ok(trace) => trace,
                    Err(_) => {
                        reply.emit_unsaved_response(
                            content,
                            execution_model.clone(),
                            finish_chat_progress(on_event, task_run_id, progress, now_ms(), false),
                        );
                        return ControlFlow::Break(());
                    }
                }
            }
        };
        let title = should_generate_title.then(|| fallback_title.clone());
        let created_at_ms = now_ms();
        let mut progress_snapshot =
            finish_chat_progress(on_event, task_run_id, progress, created_at_ms, false)
                .unwrap_or_else(|| progress.snapshot());
        let mut activity = ChatSaveActivity {
            started_at_ms: save_started_at_ms,
            finished_at_ms: None,
            waited_ms: 0,
            blocking_chat_id: None,
        };
        progress_snapshot.save_activity = Some(activity.clone());
        let progress_snapshot = Some(progress_snapshot);
        content = crate::commands::tool_executor::number_checks::normalize_evidence_link_aliases(
            &content,
        );
        let evidence_refs = snapshot!(capture.evidence_refs);
        // Rust checks each number link against the evidence it names; the
        // answer's text cannot set a status (docs/analysis/number-checks.md#number-checks).
        let number_checks = if host_composed {
            Vec::new()
        } else {
            match tokio::time::timeout(
                Duration::from_secs(2),
                capture.number_checks(&content, &evidence_refs),
            )
            .await
            {
                Ok(checks) => checks,
                Err(_) => {
                    missing_snapshot = true;
                    Vec::new()
                }
            }
        };
        let as_of_report = capture.as_of_report(&on_event.tool_trace.snapshot());
        let artifact_commits = snapshot!(capture.artifacts);
        let artifact_refs = artifact_commits
            .iter()
            .map(|commit| commit.revision.artifact_ref(commit.artifact.title.clone()))
            .collect::<Vec<_>>();
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
            as_of_report: as_of_report.clone(),
            alert: None,

            vocabulary: None,
            handoff: None,
            research_session_id: None,
            handoff_note: None,
            handoff_action: None,
            scheduled: None,
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
            memory_update: Some(skipped_memory(
                MemorySaveSkipReason::Interrupted,
                capture.discarded_memory_items(&citation_reads),
            )),
            memory_revision: memory_revision.clone(),
            execution_model: Some(execution_model.clone()),
            agent_harness: Some(harness.clone()),
            attachments: Vec::new(),
            artifact_refs,
            evidence_refs,
            progress: progress_snapshot.clone(),
        };
        if !host_composed {
            let _ = on_event.send(ChatStreamEvent::Saving {
                run_id: task_run_id.to_owned(),
                waiting_for_other_chat: false,
                blocking_chat_id: None,
                final_text: Some(content.clone()),
                number_checks: Some(number_checks),
                save_activity: Some(activity.clone()),
            });
        }
        // Persist the answer and its artifacts before waiting for Memory. If
        // the app exits before the second transaction, this is the terminal
        // record it recovers, with no claim that the staged work was saved.
        let previous_updated_at = if host_composed {
            None
        } else {
            completed_message.performance = Some(ChatPerformanceDto {
                setup_ms,
                first_text_ms,
                answer_text_ms,
                generation_ms: generation_ms
                    .unwrap_or_else(|| elapsed_millis(chat_started_at).saturating_sub(setup_ms)),
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
            });
            match crate::store::bounded_chat_save(|| {
                checkpoint_answer(reply, &completed_message, &artifact_commits, quiet_request)
            }) {
                Ok(previous) => Some(previous),
                Err(error) => {
                    record_commit_failure(on_event, CommitStage::Chat, error.code.as_str());
                    reply.emit_unsaved_response(
                        content,
                        execution_model.clone(),
                        progress_snapshot,
                    );
                    return ControlFlow::Break(());
                }
            }
        };
        // The broker has settled; snapshot staged Memory with async locks
        // before entering the synchronous storage transaction.
        let mut saved_reads = if task_update_memory {
            citation_reads.clone()
        } else {
            Vec::new()
        };
        if task_update_memory {
            saved_reads.extend(snapshot!(capture.context_reads));
        }
        let staged_handoff = snapshot!(capture.staged_handoff).map(|mut staged| {
            staged
                .run
                .model_profile_id
                .clone_from(&execution_model.profile_id);
            staged
                .run
                .thinking_level
                .clone_from(&execution_model.thinking_level);
            staged
                .run
                .run_options
                .clone_from(&execution_model.run_options);
            staged
        });
        // Only a Bot reply with saving on changes Bot memory; one that read
        // third-party text keeps its review batch for the next reply.
        let bot_memory = if task_update_memory
            && capture
                .surface
                .as_ref()
                .is_some_and(crate::chat::surface::ChatSurface::is_bot)
        {
            let reviewed = if capture.turn_taint.has_untrusted_content() {
                Vec::new()
            } else {
                capture.bot_memory_review.clone()
            };
            Some((
                snapshot!(capture.staged_bot_memory, guard => guard.delivered()),
                reviewed,
            ))
        } else {
            None
        };
        // Asset pages change only with Memory saving on and never under an as-of date.
        let asset_work = task_update_memory && !capture.as_of_turn();
        let saved_theses = if asset_work {
            snapshot!(capture.staged_theses, guard => guard.iter().map(|(_, update)| update.clone()).collect::<Vec<_>>())
        } else {
            Vec::new()
        };
        let saved_vocabulary = if task_update_memory {
            snapshot!(capture.staged_vocabulary)
        } else {
            Vec::new()
        };
        let unattended = capture.unattended_run_id.is_some();
        let staged_price_reads = if asset_work {
            snapshot!(capture.staged_price_reads)
        } else {
            Vec::new()
        };
        if missing_snapshot {
            record_commit_failure(
                on_event,
                CommitStage::Chat,
                "finalization_dependency_unavailable",
            );
            finish_skipped_save(
                reply,
                &completed_message,
                execution_model,
                title.as_deref(),
                MemorySaveSkipReason::Failed,
                &[],
            );
            return ControlFlow::Break(());
        }
        let coordinator = &app_state.run_coordinator;
        let wait_started = tokio::time::Instant::now();
        let mut waited = false;
        let admission = match coordinator.queue_chat_save(task_run_id) {
            Ok(()) => {
                coordinator
                    .wait_for_chat_save(task_run_id, |blocking_chat_id| {
                        waited = true;
                        if let Some(id) = &blocking_chat_id {
                            activity.blocking_chat_id = Some(id.clone());
                        }
                        activity.waited_ms =
                            wait_started.elapsed().as_millis().min(u64::MAX as u128) as u64;
                        let _ = on_event.send(ChatStreamEvent::Saving {
                            run_id: task_run_id.to_owned(),
                            waiting_for_other_chat: true,
                            blocking_chat_id,
                            final_text: None,
                            number_checks: None,
                            save_activity: Some(activity.clone()),
                        });
                        if previous_updated_at.is_some() {
                            let _ = checkpoint_save_activity(reply, &activity);
                        }
                    })
                    .await
            }
            Err(error) => Err(error),
        };
        if waited {
            activity.waited_ms = wait_started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        }
        if let Some(progress) = &mut completed_message.progress {
            progress.save_activity = Some(activity);
        }
        match admission {
            Ok(None) => {}
            Ok(Some(reason)) => {
                finish_skipped_save(
                    reply,
                    &completed_message,
                    execution_model,
                    title.as_deref(),
                    reason,
                    &[],
                );
                return ControlFlow::Break(());
            }
            Err(error) => {
                record_commit_failure(on_event, CommitStage::Chat, error.code.as_str());
                finish_skipped_save(
                    reply,
                    &completed_message,
                    execution_model,
                    title.as_deref(),
                    MemorySaveSkipReason::Failed,
                    &[],
                );
                return ControlFlow::Break(());
            }
        }
        let answer_checkpoint = completed_message.clone();
        let mut turn_assets = crate::commands::assets::TurnAssets::default();
        let mut completion_performance = None;
        let mut memory_reset = false;
        let mut watchlist_changes = Vec::new();
        let started_at_ms = progress.started_at_ms();
        let finalized: Result<_, CommandError> = crate::store::bounded_chat_save(|| {
            if saved_setup.is_some() {
                let connection = app_state
                    .store
                    .lock_loupist(task_loupist_id)
                    .map_err(map_store)?;
                if crate::knowledge::memory_reset::reset_since(
                    &connection,
                    task_loupist_id,
                    started_at_ms,
                )
                .map_err(|error| map_store(error.into()))?
                {
                    memory_reset = true;
                    return Err(CommandError::conflict("Memory was reset during the turn"));
                }
            }
            watchlist_changes = saved_setup
                .as_ref()
                .map(|setup| {
                    crate::commands::tool_executor::apply_setup_watchlist(
                        app_state,
                        task_loupist_id,
                        setup,
                    )
                })
                .unwrap_or_default();
            // A same-turn follow can register this ID. A competing follow
            // under a different ID or an unfollow during the wait can leave
            // it unavailable. Read the catalog after both the wait and setup.
            let mut price_reads = Vec::new();
            let mut discarded_prices = Vec::new();
            for (staged_asset, read) in &staged_price_reads {
                match app_state
                    .store
                    .asset_by_id(task_loupist_id, &staged_asset.asset_id)
                    .map_err(map_store)?
                    .filter(|asset| asset.followed_at_ms.is_some())
                {
                    Some(asset) => price_reads.push((asset, read)),
                    None => discarded_prices.push(staged_asset.name.as_str()),
                }
            }
            let discarded_prices =
                crate::commands::tool_executor::discarded_price_reads(discarded_prices);
            let skipped_journal = saved_research.as_mut().map_or(Ok(false), |research| {
                crate::commands::tool_executor::finish_setup_theses(
                    &app_state.store,
                    task_loupist_id,
                    research,
                )
            });
            app_state.ensure_loupist_available(task_loupist_id)?;
            let expected = app_state
                .store
                .get_chat_in_loupist(task_loupist_id, thread_id)
                .map_err(map_store)?
                .ok_or_else(|| CommandError::not_found("chat thread"))?;
            let mut chat = expected.clone();
            if let Some(saved) = chat
                .messages
                .iter_mut()
                .find(|message| message.id == message_id)
            {
                *saved = completed_message.clone();
            } else {
                chat.messages.push(completed_message.clone());
            }
            if let Some((staged, reviewed)) = &bot_memory {
                crate::chat::bot_memory::settle_reply(
                    &mut chat,
                    &message_id,
                    created_at_ms,
                    staged,
                    reviewed,
                    capture.principle_user_message_id.as_deref(),
                )
                .map_err(|error| {
                    record_commit_failure(on_event, CommitStage::BotMemory, "invalid");
                    on_event.set_failure_kind(ChatFailureKind::MemoryCommitFailed);
                    CommandError::internal(error)
                })?;
            }
            let title_applied = match &title {
                Some(title) if chat.title.is_empty() => {
                    chat.title = title.clone();
                    true
                }
                _ => false,
            };
            // A host receipt is absent from Pi's JSONL. Corrected or forgotten
            // Bot notes can still occur in its old envelopes and summaries.
            let bot_context_changed = bot_memory.as_ref().is_some_and(|(staged, _)| {
                staged
                    .iter()
                    .any(crate::chat::bot_memory::StagedBotMemory::invalidates_session_context)
            });
            chat.pi_session_cache = if host_composed || bot_context_changed {
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
                if crate::knowledge::memory_reset::reset_since(
                    transaction,
                    task_loupist_id,
                    started_at_ms,
                )? {
                    memory_reset = true;
                    return Err(crate::store::StoreError::Conflict(
                        "Memory was reset during the turn",
                    ));
                }
                let skipped_journal = *skipped_journal
                    .as_ref()
                    .map_err(|error| crate::store::StoreError::Invalid(error.to_string()))?;
                let research = saved_research.as_ref().filter(|research| {
                    !skipped_journal
                        || research.journal.is_some()
                        || research.working_note.is_some()
                });
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
                    research,
                )
                .map_err(|error| {
                    record_commit_failure(on_event, CommitStage::Research, error.code.as_str());
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
                            record_commit_failure(
                                on_event,
                                CommitStage::Principles,
                                error.code.as_str(),
                            );
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
                            record_commit_failure(on_event, CommitStage::Lens, error.code());
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
                .map_err(|e| {
                    record_commit_failure(on_event, CommitStage::Deliveries, e.code());
                    crate::store::StoreError::Invalid(e.to_string())
                })?;
                let skipped_applications =
                    crate::commands::knowledge::principles::persist_citations(
                        transaction,
                        &blobs,
                        task_loupist_id,
                        &message_id,
                        &content,
                        crate::commands::knowledge::principles::CitationInputs {
                            reads: &citation_reads,
                            lens_deliveries: &lens_deliveries,
                        },
                        created_at_ms,
                    )
                    .map_err(|e| {
                        record_commit_failure(on_event, CommitStage::Citations, e.code());
                        crate::store::StoreError::Invalid(e.to_string())
                    })?;
                if skipped_applications != 0 {
                    if let Some((_, record)) = on_event
                        .run_log
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .as_mut()
                    {
                        record.notes.push(format!(
                                "Skipped {skipped_applications} Lens application receipts after a revision or pause changed."
                            ));
                    }
                }
                turn_assets = crate::commands::assets::commit_turn(
                    transaction,
                    task_loupist_id,
                    thread_id,
                    &message_id,
                    created_at_ms,
                    &saved_theses,
                )
                .map_err(|e| {
                    record_commit_failure(on_event, CommitStage::Assets, e.code());
                    crate::store::StoreError::Invalid(e.to_string())
                })?;
                changes.extend(
                    crate::commands::tool_executor::commit_price_reads(
                        transaction,
                        task_loupist_id,
                        created_at_ms,
                        price_reads.iter().map(|(asset, read)| (asset, *read)),
                    )
                    .map_err(|error| {
                        record_commit_failure(on_event, CommitStage::Assets, error.code());
                        crate::store::StoreError::Invalid(error.to_string())
                    })?,
                );
                let mut skipped_kinds = crate::commands::tool_executor::commit_vocabulary(
                    transaction,
                    task_loupist_id,
                    thread_id,
                    created_at_ms,
                    &saved_vocabulary,
                )
                .map_err(|e| {
                    record_commit_failure(on_event, CommitStage::Vocabulary, e.code());
                    crate::store::StoreError::Invalid(e.to_string())
                })?;
                if skipped_journal {
                    skipped_kinds.push(crate::domain::MapKind {
                        name: crate::domain::SKIPPED_JOURNAL_SAVE.into(),
                        label: "Journal".into(),
                        label_ko: Some("저널".into()),
                    });
                }
                let mut setup = watchlist_changes
                    .iter()
                    .map(|(change, _)| change.clone())
                    .collect::<Vec<_>>();
                if let Some(staged) = &saved_setup {
                    let (changed, skipped) = crate::commands::tool_executor::commit_setup(
                        transaction,
                        task_loupist_id,
                        thread_id,
                        created_at_ms,
                        staged,
                    )
                    .map_err(|e| {
                        let code = if matches!(e, crate::store::StoreError::Conflict(_)) {
                            "conflict"
                        } else {
                            "internal"
                        };
                        record_commit_failure(on_event, CommitStage::Setup, code);
                        e
                    })?;
                    setup.extend(changed);
                    skipped_kinds.extend(skipped);
                }
                let research_session_id = if let Some(staged) = &staged_handoff {
                    let source_id =
                        capture
                            .principle_user_message_id
                            .as_deref()
                            .ok_or_else(|| {
                                crate::store::StoreError::Invalid(
                                    "The original Bot request is unavailable".into(),
                                )
                            })?;
                    let handoff = crate::chat::handoff::commit_bot(
                        transaction,
                        task_loupist_id,
                        staged,
                        (thread_id, source_id),
                        &capture.turn_taint,
                        created_at_ms,
                    )
                    .map_err(|e| {
                        let code = if matches!(e, crate::store::StoreError::Conflict(_)) {
                            "conflict"
                        } else {
                            "internal"
                        };
                        record_commit_failure(on_event, CommitStage::Handoff, code);
                        e
                    })?;
                    Some(handoff.chat_id)
                } else {
                    None
                };
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
                    answer_text_ms,
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
                    discarded_prices,
                    setup,
                    Some(performance),
                    turn_assets.trace_notes(elapsed_millis(chat_started_at)),
                    quiet_request.map(|run_id| {
                        (
                            run_id,
                            previous_updated_at.unwrap_or(expected.updated_at_ms),
                        )
                    }),
                    research_session_id,
                )?;
                content = completed_message.content.clone();
                Ok(())
            };
            if artifact_commits.is_empty() || previous_updated_at.is_some() {
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
            let performance = completed_message
                .performance
                .clone()
                .ok_or_else(|| CommandError::internal("completed Chat performance is missing"))?;
            Ok((title_applied, completed_message.clone(), performance))
        });
        coordinator.finish_chat_save(task_run_id);
        if saved_setup.is_some()
            && crate::store::bounded_chat_save(|| {
                app_state.store.resync_folder_registration(task_loupist_id)
            })
            .is_err()
        {
            eprintln!("The Loupist list could not pick up the new focus yet");
        }
        match finalized {
            Ok((title_applied, completed_message, performance)) => {
                app_state.graph.wake();
                let saved_price = completed_message
                    .memory_update
                    .as_ref()
                    .is_some_and(|update| {
                        update
                            .changes
                            .iter()
                            .any(|change| change.kind == crate::domain::PRICE_READ_CHANGE_KIND)
                    });
                if saved_price || !saved_theses.is_empty() {
                    crate::commands::assets::changed();
                }
                if turn_assets.crossed {
                    // The Thread announces a crossed line once; a busy Thread gets it later.
                    let _ = crate::store::bounded_chat_save(|| {
                        crate::chat::home::deliver_finished_runs(app_state, task_loupist_id)
                    });
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
            }
            Err(error) => {
                let recovered = completion_performance.as_ref().and_then(|performance| {
                    crate::store::bounded_chat_save(|| {
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
                    })
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
                    record_commit_failure(on_event, CommitStage::Chat, error.code.as_str());
                    if error.code == "conflict" {
                        on_event.set_failure_kind(ChatFailureKind::CancelledByConflict);
                    }
                    if previous_updated_at.is_some() {
                        let reason = if memory_reset {
                            MemorySaveSkipReason::PolicyChanged
                        } else {
                            MemorySaveSkipReason::Failed
                        };
                        finish_skipped_save(
                            reply,
                            &answer_checkpoint,
                            execution_model,
                            title.as_deref(),
                            reason,
                            &watchlist_changes,
                        );
                        return ControlFlow::Break(());
                    }
                    *failure = Some(error.message);
                }
            }
        }
    }
    ControlFlow::Continue(())
}
