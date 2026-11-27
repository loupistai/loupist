//! Completed-turn commit: one host transaction publishes the assistant reply
//! with its staged Memory, receipts, citations and artifacts, then emits the
//! completion, or recovers it when the commit outcome is ambiguous.
use super::*;
use crate::{
    broker::tools::{ToolPermission, TOOL_CATALOG},
    chat::{artifacts::ArtifactCommit, progress::ChatSaveActivity},
    domain::{MemorySaveSkipReason, MemoryUpdateResult, MemoryUpdateStatus},
    runs::coordinator::SavingMessage,
    runs::log::{ChatSavePath, CommitStage, MemoryCommitFailure},
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

fn used_memory_tool(trace: &crate::domain::ChatToolTrace) -> bool {
    trace.entries.iter().any(|entry| {
        TOOL_CATALOG
            .tools
            .iter()
            .any(|tool| tool.permission == ToolPermission::MemoryRead && tool.name == entry.name)
    })
}

/// Stores the answer and its artifacts before the save waits. A reply whose
/// save will wait on its own also takes its title here, since its
/// conversation may start a newer turn before the save resolves.
fn checkpoint_answer(
    reply: &TerminalReply<'_>,
    message: &ChatMessage,
    artifacts: &[ArtifactCommit],
    quiet_request: Option<&str>,
    title: Option<&str>,
) -> Result<(i64, bool), CommandError> {
    let store = &reply.state.store;
    let expected = store
        .get_chat_in_loupist(reply.loupist_id, reply.thread_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("chat thread"))?;
    let mut chat = expected.clone();
    let title_applied = title.is_some_and(|title| chat.apply_reply_title(title));
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
    Ok((expected.updated_at_ms, title_applied))
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

/// The receipt's items not kept, with Bot memory when some of the reply's Bot
/// memory changes no longer applied (`settle_reply_after`).
fn with_left_out_bot_memory(
    items: Option<Vec<crate::domain::DiscardedMemoryItem>>,
    left_out: usize,
) -> Option<Vec<crate::domain::DiscardedMemoryItem>> {
    if left_out == 0 {
        return items;
    }
    let mut items = items.unwrap_or_default();
    // Bot memory entries have bodies and no titles, as in a skipped save's list.
    let bot = crate::domain::DiscardedMemoryItem {
        kind: crate::domain::DiscardedMemoryKind::BotMemory,
        title: "Bot".into(),
        reason: None,
    };
    if items.len() < crate::domain::MAX_DISCARDED_MEMORY_ITEMS && !items.contains(&bot) {
        items.push(bot);
    }
    Some(items)
}

/// Pauses before a Memory commit is tried again after the store was busy or
/// failed: at most three attempts, about 34 seconds in all with the save
/// budget of each.
const COMMIT_RETRY_PAUSES: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(3)];

/// Commit stages a failed attempt left out because one of their items was
/// refused, so the rest of the turn saves. Each stage is left out at most once.
#[derive(Default)]
struct LeftOut(Vec<CommitStage>);

impl LeftOut {
    /// Leaves a stage out of the next attempt; false when it cannot be, or
    /// already was. The receipt, setup and a hand-off are never left out.
    fn leave_out(&mut self, stage: CommitStage) -> bool {
        let separable = matches!(
            stage,
            CommitStage::Research
                | CommitStage::Principles
                | CommitStage::Lens
                | CommitStage::Deliveries
                | CommitStage::Citations
                | CommitStage::Assets
                | CommitStage::Vocabulary
                | CommitStage::BotMemory
        );
        if !separable || self.has(stage) {
            return false;
        }
        self.0.push(stage);
        true
    }

    fn has(&self, stage: CommitStage) -> bool {
        self.0.contains(&stage)
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    /// What a left-out stage would have written: nothing.
    fn keep<'a, T>(&self, stage: CommitStage, staged: &'a [T]) -> &'a [T] {
        if self.has(stage) {
            &[]
        } else {
            staged
        }
    }

    /// The receipt's list of what was not kept, with each staged item of a
    /// left-out stage named and its cause.
    fn listed(
        &self,
        items: Option<Vec<crate::domain::DiscardedMemoryItem>>,
        staged: Option<&[crate::domain::DiscardedMemoryItem]>,
    ) -> Option<Vec<crate::domain::DiscardedMemoryItem>> {
        let left = staged
            .unwrap_or_default()
            .iter()
            .filter(|item| stage_of(item.kind).is_some_and(|stage| self.has(stage)))
            .collect::<Vec<_>>();
        if left.is_empty() {
            return items;
        }
        let mut items = items.unwrap_or_default();
        for item in left {
            if items.len() == crate::domain::MAX_DISCARDED_MEMORY_ITEMS {
                break;
            }
            if !items
                .iter()
                .any(|kept| kept.kind == item.kind && kept.title == item.title)
            {
                items.push(crate::domain::DiscardedMemoryItem {
                    reason: Some(MemorySaveSkipReason::Invalid),
                    ..item.clone()
                });
            }
        }
        Some(items)
    }
}

/// The commit stage that writes a staged item of this kind.
fn stage_of(kind: crate::domain::DiscardedMemoryKind) -> Option<CommitStage> {
    use crate::domain::DiscardedMemoryKind as Kind;
    Some(match kind {
        Kind::Note | Kind::JournalEntry | Kind::Source => CommitStage::Research,
        Kind::Principle => CommitStage::Principles,
        Kind::LensSection | Kind::LensPage | Kind::Method => CommitStage::Lens,
        Kind::Thesis | Kind::PriceRead => CommitStage::Assets,
        Kind::WorldVocabulary => CommitStage::Vocabulary,
        Kind::BotMemory => CommitStage::BotMemory,
        Kind::Watchlist | Kind::ResearchFocus | Kind::ResearchHandoff => return None,
    })
}

/// Ends a released reply's stream with its stored answer, title and
/// artifacts. The receipt follows by message once the save resolves.
fn emit_checkpoint(
    reply: &TerminalReply<'_>,
    message: &ChatMessage,
    title: Option<String>,
    model: &ExecutionModelLock,
) {
    let Some(performance) = message.performance.clone() else {
        return;
    };
    let mut shown = message.clone();
    shown.memory_update = None;
    emit_completed_chat(
        reply.on_event,
        reply.run_id,
        &shown,
        title,
        model,
        reply.harness,
        performance,
    );
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
        super::saves::retry_chat_cas(|| {
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
            let applied_title = title
                .filter(|title| chat.apply_reply_title(title))
                .map(str::to_owned);
            store.replace_chat(&expected, &chat).map_err(map_store)?;
            Ok::<_, CommandError>((saved, applied_title))
        })
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
    sink.note_save_failure(stage, save_failure_cause(code));
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

/// What the skipped receipt tells the user about a failed commit step's code.
fn save_failure_cause(code: &str) -> MemorySaveSkipReason {
    match code {
        "store_busy" | "world_busy" => MemorySaveSkipReason::StoreBusy,
        "conflict" | "revision_conflict" | "cursor_stale" | "conflicting_dependency_revision" => {
            MemorySaveSkipReason::Conflict
        }
        "internal"
        | "sqlite"
        | "json"
        | "io"
        | "private_storage"
        | "lock_poisoned"
        | "schema_mismatch"
        | "loupist_storage_unavailable" => MemorySaveSkipReason::Storage,
        // The turn's own state was unavailable to the commit: no cause to name.
        "finalization_dependency_unavailable" => MemorySaveSkipReason::Failed,
        _ => MemorySaveSkipReason::Invalid,
    }
}

/// The run log's class for a failed Chat transaction, read before `map_store`
/// turns every kind but a conflict into `internal`. A conflict is retried or
/// recorded once the retries end.
fn chat_commit_failure_code(error: &crate::store::StoreError) -> Option<&'static str> {
    use crate::store::StoreError;
    Some(match error {
        StoreError::Conflict(_) => return None,
        StoreError::Invalid(_) => "invalid",
        StoreError::ChatSaveDeadline => "store_busy",
        StoreError::Sqlite(error) if crate::commands::knowledge::sqlite_is_busy(error) => {
            "store_busy"
        }
        StoreError::Sqlite(_) => "sqlite",
        StoreError::Json(_) => "json",
        StoreError::PrivateStorage(_) => "private_storage",
        StoreError::LockPoisoned => "lock_poisoned",
        _ => "internal",
    })
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
    let cited = crate::commands::knowledge::principles::answer_lens_links(&content)
        .collect::<std::collections::BTreeSet<_>>();
    // Delivery rows keep every principle the envelope carried. Activity keeps
    // a principle only when this answer cites it or the turn read it.
    let lens_receipts = lens_deliveries
        .iter()
        .filter(|receipt| {
            crate::commands::knowledge::principles::shows_in_activity(
                receipt,
                &cited,
                &citation_reads,
            )
        })
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
        content = crate::commands::tool_executor::number_checks::normalize_evidence_link_aliases(
            &content,
        );
        let evidence_refs = snapshot!(capture.evidence_refs);
        // Rust checks each number link against the evidence it names; the
        // answer's text cannot set a status (docs/analysis/number-checks.md#number-checks).
        // A link it rebinds to this reply's evidence is stored rebound.
        let number_checks = if host_composed {
            Vec::new()
        } else {
            match tokio::time::timeout(
                Duration::from_secs(2),
                capture.number_checks(&content, &evidence_refs),
            )
            .await
            {
                Ok(checked) => {
                    content = checked.content;
                    checked.checks
                }
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
        let tool_trace = on_event.tool_trace.snapshot();
        let interactive = capture.surface.as_ref().is_some_and(|s| s.interactive());
        // Keep Chat authority and the reader lease through the ordinary CAS.
        // These inputs cannot change the Memory another conversation reads.
        let direct_completion = interactive
            && !host_composed
            && !missing_snapshot
            && !capture.turn_taint.memory_blocked()
            && quiet_request.is_none()
            && !has_staged_memory
            && staged_handoff.is_none()
            && bot_memory
                .as_ref()
                .is_none_or(|(edits, _)| edits.is_empty())
            && saved_theses.is_empty()
            && saved_vocabulary.is_empty()
            && staged_price_reads.is_empty()
            && citation_reads.is_empty()
            && !tool_trace.truncated
            && !used_memory_tool(&tool_trace)
            && crate::commands::knowledge::principles::answer_lens_links(&content)
                .next()
                .is_none();
        let mut activity = ChatSaveActivity {
            started_at_ms: save_started_at_ms,
            finished_at_ms: None,
            waited_ms: 0,
            blocking_chat_id: None,
        };
        progress_snapshot.save_activity = (!direct_completion).then(|| activity.clone());
        let progress_snapshot = Some(progress_snapshot);
        let mut completed_message = ChatMessage {
            tool_refusals: on_event.tool_refusals(),
            performance: None,
            feedback: None,
            tool_trace: Some(tool_trace),
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
            offers: capture
                .offers
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
            ask: capture
                .ask
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
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
            memory_update: (!direct_completion).then(|| {
                skipped_memory(
                    MemorySaveSkipReason::Interrupted,
                    capture.discarded_memory_items(&citation_reads),
                )
            }),
            memory_revision: memory_revision.clone(),
            execution_model: Some(execution_model.clone()),
            agent_harness: Some(harness.clone()),
            attachments: Vec::new(),
            artifact_refs,
            evidence_refs,
            progress: progress_snapshot.clone(),
        };
        if !direct_completion && (!host_composed || interactive) {
            let _ = on_event.send(ChatStreamEvent::Saving {
                run_id: task_run_id.to_owned(),
                waiting_for_other_chat: false,
                blocking_chat_id: None,
                final_text: (!host_composed).then(|| content.clone()),
                number_checks: (!host_composed).then_some(number_checks),
                save_activity: Some(activity.clone()),
            });
        }
        // An interactive reply with an answer to show gives its conversation
        // back once that answer is stored; its save then waits on its own and
        // reports by message (docs/runtime/chat-turns.md#saves-that-wait-on-their-own).
        let release = interactive && !host_composed && !direct_completion && !missing_snapshot;
        let mut released_title = None;
        // Persist the answer and its artifacts before waiting for Memory. If
        // the app exits before the second transaction, this is the terminal
        // record it recovers, with no claim that the staged work was saved.
        let previous_updated_at = if host_composed || direct_completion {
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
                model_requests: token_usage.model_requests(),
                cost_estimate: token_usage.cost_estimate(),
            });
            let checkpoint_title = title.as_deref().filter(|_| release);
            match crate::store::bounded_chat_save(|| {
                super::saves::retry_chat_cas(|| {
                    checkpoint_answer(
                        reply,
                        &completed_message,
                        &artifact_commits,
                        quiet_request,
                        checkpoint_title,
                    )
                })
            }) {
                Ok((previous, title_applied)) => {
                    if title_applied {
                        released_title = checkpoint_title.map(str::to_owned);
                    }
                    Some(previous)
                }
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
                on_event.save_failure(),
                &[],
            );
            return ControlFlow::Break(());
        }
        let coordinator = &app_state.run_coordinator;
        let wait_started = tokio::time::Instant::now();
        let mut waited = false;
        let queued = if direct_completion {
            Ok(false)
        } else {
            let saving = release.then(|| SavingMessage {
                thread_id: thread_id.to_owned(),
                message_id: message_id.clone(),
                reviewed: bot_memory
                    .as_ref()
                    .map(|(_, reviewed)| reviewed.clone())
                    .unwrap_or_default(),
                leaving: bot_memory
                    .as_ref()
                    .map(|(staged, _)| crate::chat::bot_memory::Leaving::of(staged))
                    .unwrap_or_default(),
            });
            coordinator
                .queue_chat_save(task_run_id, saving)
                .map(|()| release)
        };
        let released = matches!(queued, Ok(true));
        // From here a released save reports through its message, and the
        // reply's stream ends with the stored answer. Its title was applied
        // with the answer, so no later step applies it again.
        let released_sink;
        let released_reply;
        let (reply, on_event, title) = if released {
            released_sink = on_event.released(super::saves::ReleasedSave {
                state: app_state.clone(),
                loupist_id: task_loupist_id.to_owned(),
                thread_id: thread_id.to_owned(),
                message_id: message_id.clone(),
                run_id: task_run_id.to_owned(),
            });
            // The wait is on its message before the stream ends, so the reply
            // never shows as finished without its save.
            let _ = released_sink.send(ChatStreamEvent::Saving {
                run_id: task_run_id.to_owned(),
                waiting_for_other_chat: false,
                blocking_chat_id: None,
                final_text: None,
                number_checks: None,
                save_activity: Some(activity.clone()),
            });
            emit_checkpoint(reply, &completed_message, released_title, execution_model);
            released_reply = TerminalReply {
                on_event: &released_sink,
                ..*reply
            };
            // Results that waited for this conversation can be posted now.
            if let Some(surface) = &capture.surface {
                surface.after_turn(app_state, task_loupist_id);
            }
            (&released_reply, &released_sink, None)
        } else {
            if let Some(title) = released_title {
                let _ = on_event.send(ChatStreamEvent::Title {
                    run_id: task_run_id.to_owned(),
                    title,
                });
            }
            (reply, on_event, title)
        };
        let admission = match queued {
            Ok(false) if direct_completion => Ok(None),
            Ok(_) => {
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
                        // A released save keeps its wait in memory: writing it
                        // here would race the next turn of its conversation.
                        if previous_updated_at.is_some() && !released {
                            let _ = checkpoint_save_activity(reply, &activity);
                        }
                    })
                    .await
            }
            Err(error) => Err(error),
        };
        if let Some((_, record)) = on_event
            .run_log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            record.chat_save_path = Some(if direct_completion {
                ChatSavePath::Direct
            } else if waited {
                ChatSavePath::Waited
            } else {
                ChatSavePath::Queued
            });
        }
        if waited {
            activity.waited_ms = wait_started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        }
        if let Some(progress) = &mut completed_message.progress {
            progress.save_activity = (!direct_completion).then_some(activity);
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
                    on_event.save_failure(),
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
        let prepared = crate::store::bounded_chat_save(|| {
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
            Ok::<_, CommandError>((price_reads, discarded_prices, skipped_journal))
        });
        // Each attempt is one transaction. One that fails on a refused item
        // leaves that item's stage out and saves the rest; a busy or failing
        // store is tried again after a pause. A landed commit never reruns.
        // What a left-out stage did not keep. A setup preset is listed as World
        // vocabulary but lands with setup, which is never left out.
        let staged_preset = saved_setup
            .as_ref()
            .and_then(|setup| setup.preset)
            .map(|preset| preset.as_str());
        let staged_items = capture
            .discarded_memory_items(&citation_reads)
            .map(|items| {
                items
                    .into_iter()
                    .filter(|item| {
                        item.kind != crate::domain::DiscardedMemoryKind::WorldVocabulary
                            || Some(item.title.as_str()) != staged_preset
                    })
                    .collect::<Vec<_>>()
            });
        let base_message = completed_message.clone();
        let base_content = content.clone();
        let base_failure_kind = on_event.failure_kind();
        let mut left_out = LeftOut::default();
        let mut pauses = COMMIT_RETRY_PAUSES.iter();
        let mut retried = 0usize;
        let mut committed = false;
        let finalized: Result<_, CommandError> = match prepared {
            Err(error) => Err(error),
            Ok((price_reads, discarded_prices, skipped_journal)) => loop {
                completed_message = base_message.clone();
                content = base_content.clone();
                on_event.set_failure_kind(base_failure_kind);
                on_event.begin_commit_attempt();
                completion_performance = None;
                let attempt = crate::store::bounded_chat_save(|| {
                    // The save writes its own reply by ID. A newer turn of a released
                    // conversation can win the row meanwhile; read it again and retry.
                    super::saves::retry_chat_cas(|| {
                        let mut bot_memory_left_out = 0;
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
                            let feedback = saved.feedback.take();
                            *saved = completed_message.clone();
                            saved.feedback = feedback;
                        } else if previous_updated_at.is_some() {
                            return Err(CommandError::conflict("finalized Chat turn is missing"));
                        } else {
                            chat.messages.push(completed_message.clone());
                        }
                        // A Bot turn that began before an earlier reply's save changed Bot
                        // memory carries the older entries in its envelope.
                        let bot_memory_moved = capture
                            .bot_memory_basis
                            .as_ref()
                            .is_some_and(|basis| basis.changes != expected.bot_memory.changes);
                        if let Some((staged, reviewed)) = bot_memory
                            .as_ref()
                            .filter(|_| !left_out.has(CommitStage::BotMemory))
                        {
                            let user_message = capture.principle_user_message_id.as_deref();
                            // Checked against memory an earlier save has changed since, a
                            // change can meet an entry that save resolved or forgot, or a
                            // full memory. Only that change is left out; the rest saves.
                            let settled = if bot_memory_moved {
                                crate::chat::bot_memory::settle_reply_after(
                                    &mut chat,
                                    &message_id,
                                    created_at_ms,
                                    staged,
                                    reviewed,
                                    user_message,
                                )
                            } else {
                                crate::chat::bot_memory::settle_reply(
                                    &mut chat,
                                    &message_id,
                                    created_at_ms,
                                    staged,
                                    reviewed,
                                    user_message,
                                )
                                .map(|()| 0)
                            };
                            bot_memory_left_out = settled.map_err(|error| {
                                record_commit_failure(on_event, CommitStage::BotMemory, "invalid");
                                on_event.set_failure_kind(ChatFailureKind::MemoryCommitFailed);
                                CommandError::internal(error)
                            })?;
                        }
                        let title_applied = title
                            .as_ref()
                            .is_some_and(|title| chat.apply_reply_title(title));
                        // A host receipt is absent from Pi's JSONL. Corrected or forgotten
                        // Bot notes can still occur in its old envelopes and summaries.
                        let bot_context_changed = bot_memory.as_ref().is_some_and(|(staged, _)| {
                            staged
                        .iter()
                        .any(crate::chat::bot_memory::StagedBotMemory::invalidates_session_context)
                        });
                        // This turn's Pi cursor describes the transcript up to its reply.
                        // Once a newer turn or post follows, that newer state owns the cache.
                        let latest = chat
                            .messages
                            .last()
                            .is_some_and(|message| message.id == message_id);
                        chat.pi_session_cache =
                            if host_composed || bot_context_changed || bot_memory_moved {
                                None
                            } else if latest {
                                sealed_cache.clone()
                            } else {
                                expected.pi_session_cache.clone()
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
                            let skipped_journal = *skipped_journal.as_ref().map_err(|error| {
                                crate::store::StoreError::Invalid(error.to_string())
                            })?;
                            let research = saved_research.as_ref().filter(|research| {
                                !left_out.has(CommitStage::Research)
                                    && (!skipped_journal
                                        || research.journal.is_some()
                                        || research.working_note.is_some())
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
                                left_out.keep(CommitStage::Research, &saved_sources),
                                &saved_reads,
                                research,
                            )
                            .map_err(|error| {
                                record_commit_failure(
                                    on_event,
                                    CommitStage::Research,
                                    error.code.as_str(),
                                );
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
                                        left_out.keep(CommitStage::Principles, &saved_principles),
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
                                            user_message_id: capture
                                                .principle_user_message_id
                                                .clone(),
                                            unattended,
                                        },
                                        created_at_ms,
                                        left_out.keep(CommitStage::Lens, &saved_filings),
                                        &saved.retained,
                                    )
                                    .map_err(|error| {
                                        record_commit_failure(
                                            on_event,
                                            CommitStage::Lens,
                                            error.code(),
                                        );
                                        on_event
                                            .set_failure_kind(ChatFailureKind::MemoryCommitFailed);
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
                                left_out.keep(CommitStage::Deliveries, &lens_deliveries),
                                created_at_ms,
                            )
                            .map_err(|e| {
                                record_commit_failure(on_event, CommitStage::Deliveries, e.code());
                                crate::store::StoreError::Invalid(e.to_string())
                            })?;
                            if !direct_completion && !left_out.has(CommitStage::Citations) {
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
                                        record_commit_failure(
                                            on_event,
                                            CommitStage::Citations,
                                            e.code(),
                                        );
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
                            }
                            turn_assets = crate::commands::assets::commit_turn(
                                transaction,
                                task_loupist_id,
                                thread_id,
                                &message_id,
                                created_at_ms,
                                left_out.keep(CommitStage::Assets, &saved_theses),
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
                                    price_reads
                                        .iter()
                                        .filter(|_| !left_out.has(CommitStage::Assets))
                                        .map(|(asset, read)| (asset, *read)),
                                )
                                .map_err(|error| {
                                    record_commit_failure(
                                        on_event,
                                        CommitStage::Assets,
                                        error.code(),
                                    );
                                    crate::store::StoreError::Invalid(error.to_string())
                                })?,
                            );
                            let mut skipped_kinds =
                                crate::commands::tool_executor::commit_vocabulary(
                                    transaction,
                                    task_loupist_id,
                                    thread_id,
                                    created_at_ms,
                                    left_out.keep(CommitStage::Vocabulary, &saved_vocabulary),
                                )
                                .map_err(|e| {
                                    record_commit_failure(
                                        on_event,
                                        CommitStage::Vocabulary,
                                        e.code(),
                                    );
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
                                let (changed, skipped) =
                                    crate::commands::tool_executor::commit_setup(
                                        transaction,
                                        task_loupist_id,
                                        thread_id,
                                        created_at_ms,
                                        staged,
                                    )
                                    .map_err(|e| {
                                        let code =
                                            if matches!(e, crate::store::StoreError::Conflict(_)) {
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
                                let source_id = capture
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
                                    let code = if matches!(e, crate::store::StoreError::Conflict(_))
                                    {
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
                                model_requests: token_usage.model_requests(),
                                cost_estimate: token_usage.cost_estimate(),
                            };
                            completion_performance = Some(performance.clone());
                            completed_message =
                                crate::commands::knowledge::write_chat_learning_report(
                                    transaction,
                                    &chat,
                                    &message_id,
                                    task_update_memory,
                                    changes,
                                    skipped_kinds,
                                    left_out.listed(
                                        with_left_out_bot_memory(
                                            discarded_prices.clone(),
                                            bot_memory_left_out,
                                        ),
                                        staged_items.as_deref(),
                                    ),
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
                        let outcome =
                            if artifact_commits.is_empty() || previous_updated_at.is_some() {
                                app_state.store.replace_chat_with(&expected, &chat, persist)
                            } else {
                                app_state.store.save_chat_with_artifacts_and(
                                    &expected,
                                    &chat,
                                    &artifact_commits,
                                    persist,
                                )
                            };
                        outcome.map_err(|error| {
                            if let Some(code) = chat_commit_failure_code(&error) {
                                record_commit_failure(on_event, CommitStage::Chat, code);
                            }
                            map_store(error)
                        })?;
                        committed = true;
                        let performance =
                            completed_message.performance.clone().ok_or_else(|| {
                                CommandError::internal("completed Chat performance is missing")
                            })?;
                        Ok((title_applied, completed_message.clone(), performance))
                    })
                });
                let Err(error) = &attempt else {
                    if retried > 0 || !left_out.is_empty() {
                        on_event.note_saved_after_retries(retried, left_out.len());
                    }
                    break attempt;
                };
                let (stage, cause) = on_event
                    .commit_failure()
                    .unwrap_or((CommitStage::Chat, save_failure_cause(error.code.as_str())));
                // An attempt that froze its receipt failed at the final write or
                // COMMIT, whose outcome the recovery below reads from the store.
                // Running the writes again could keep the turn twice.
                if committed || memory_reset || completion_performance.is_some() {
                    break attempt;
                }
                match cause {
                    MemorySaveSkipReason::Invalid if left_out.leave_out(stage) => {}
                    MemorySaveSkipReason::StoreBusy | MemorySaveSkipReason::Storage => {
                        match pauses.next() {
                            Some(pause) => {
                                retried += 1;
                                tokio::time::sleep(*pause).await;
                            }
                            None => break attempt,
                        }
                    }
                    _ => break attempt,
                }
            },
        };
        if !direct_completion {
            coordinator.finish_chat_save(task_run_id);
        }
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
                            on_event.save_failure()
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
                    if direct_completion {
                        reply.emit_unsaved_response(
                            answer_checkpoint.content,
                            execution_model.clone(),
                            progress_snapshot,
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

#[cfg(test)]
mod commit_failure_tests {
    use super::*;
    use crate::{runs::log::CommitReason, store::StoreError};

    #[test]
    fn a_refused_receipt_is_logged_as_invalid_input_and_a_store_wait_not_as_storage() {
        let reason = |error| {
            chat_commit_failure_code(&error)
                .map(|code| MemoryCommitFailure::from_code(CommitStage::Chat, code).reason)
        };
        assert_eq!(
            reason(StoreError::Invalid(
                "memory update has too many changes".into()
            )),
            Some(CommitReason::InvalidInput)
        );
        assert_eq!(
            reason(StoreError::ChatSaveDeadline),
            Some(CommitReason::Other)
        );
        assert_eq!(
            reason(StoreError::LockPoisoned),
            Some(CommitReason::Storage)
        );
        // A lost compare-and-swap is retried; the final one is logged after.
        assert_eq!(reason(StoreError::Conflict("Chat compare-and-swap")), None);
    }
}
