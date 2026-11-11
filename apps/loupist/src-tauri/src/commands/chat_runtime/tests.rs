use super::*;
use serde_json::json;
use tauri::Manager;
use FirstProviderBodyWatchdogSelectMode::{Fair, Timeout};

fn memory_ref(read_ref: &str, title: &str, access: MemoryAccess) -> MemoryRefSnapshot {
    MemoryRefSnapshot {
        read_ref: read_ref.into(),
        kind: "source".into(),
        title: title.into(),
        excerpt: "Operating margin increased.".into(),
        as_of: None,
        section: None,
        access,
        full_record_digest: None,
        record_id: format!("rec_{}", "a".repeat(32)),
        revision_id: format!("rev_{}", "b".repeat(32)),
        object_id: "filing".into(),
        span_id: None,
        receipt_id: Some("receipt".into()),
    }
}

#[test]
fn durable_memory_trace_retains_the_last_exact_read_in_reference_order() {
    let last = memory_ref("z-read", "Last read", MemoryAccess::ExactRead);
    let first = memory_ref("a-read", "Updated read", MemoryAccess::ExactRead);
    let trace = durable_memory_trace([
        last.clone(),
        memory_ref("a-read", "Earlier read", MemoryAccess::ExactRead),
        memory_ref("m-read", "Discovery", MemoryAccess::SearchDiscovered),
        first.clone(),
        memory_ref("a-read", "Later discovery", MemoryAccess::SearchDiscovered),
    ])
    .unwrap();
    let expected = vec![first, last];
    assert_eq!(trace.refs, expected);
    assert_eq!(trace.observed_exact_count, 2);
    assert!(!trace.refs_truncated);
    assert_eq!(trace.refs_digest, memory_refs_digest(&expected).unwrap());
}

#[test]
fn durable_memory_trace_counts_and_digests_all_exact_reads_before_truncation() {
    for count in [32, 35] {
        let all_refs = (0..count)
            .map(|index| {
                memory_ref(
                    &format!("read-{index:02}"),
                    "Exact read",
                    MemoryAccess::ExactRead,
                )
            })
            .collect::<Vec<_>>();
        let trace = durable_memory_trace(all_refs.iter().rev().cloned()).unwrap();
        assert_eq!(trace.refs, all_refs[..32]);
        assert_eq!(trace.observed_exact_count, count as u64);
        assert_eq!(trace.refs_truncated, count > 32);
        assert_eq!(trace.refs_digest, memory_refs_digest(&all_refs).unwrap());
        if count > 32 {
            assert_ne!(trace.refs_digest, memory_refs_digest(&trace.refs).unwrap());
        }
    }
}

#[test]
fn durable_memory_trace_is_empty_without_exact_reads() {
    for memories in [
        Vec::new(),
        vec![memory_ref(
            "discovered",
            "Discovery",
            MemoryAccess::SearchDiscovered,
        )],
    ] {
        let trace = durable_memory_trace(memories).unwrap();
        assert!(trace.refs.is_empty());
        assert_eq!(trace.observed_exact_count, 0);
        assert!(!trace.refs_truncated);
        assert_eq!(trace.refs_digest, memory_refs_digest(&[]).unwrap());
    }
}

#[test]
fn retry_last_returns_the_original_question_and_date_and_persists_steers() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState::for_test(root.path().join("app"));
    let profile =
        crate::commands::tests::profile("loupist-retry", &root.path().join("workspace"), 1);
    crate::commands::tests::seed_profile(state.store.as_ref(), &profile);
    let mut chat = crate::commands::tests::chat("chat-retry", &profile.id, 1);
    let mut question =
        crate::test_fixtures::chat_message("question", ChatRole::User, "Compare margins");
    question.as_of = Some("2026-10-01".into());
    let mut steer =
        crate::test_fixtures::chat_message("steer", ChatRole::User, "Use annual filings");
    steer.steer = true;
    let mut failed = crate::test_fixtures::chat_message("failed", ChatRole::Assistant, "Stopped.");
    failed.status = crate::domain::ChatMessageStatus::Error;
    chat.messages = vec![question, steer.clone(), failed];
    state.store.create_chat(&chat).unwrap();
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    app.manage(state);
    let receipt = chat_retry_last(
        ChatRetryRequest {
            loupist_id: profile.id,
            thread_id: chat.id.clone(),
        },
        app.state(),
    )
    .unwrap();
    assert_eq!(receipt.prompt, "Compare margins");
    assert_eq!(receipt.as_of.as_deref(), Some("2026-10-01"));
    let state = app.state::<AppState>();
    assert_eq!(
        state.store.get_chat(&chat.id).unwrap().unwrap().messages,
        [steer]
    );
}

fn observe(watchdog: &mut ProviderWaitWatchdog, at: Instant, payload: Value) {
    watchdog.observe(&Ok(PiEvent::Rpc { payload }), at);
}

fn select(watchdog: &ProviderWaitWatchdog, at: Instant) -> FirstProviderBodyWatchdogSelectMode {
    watchdog.select_mode(at, CHAT_WATCHDOG_EXPIRY_EVENT_DEFERRAL_LIMIT)
}

fn assert_provider_timeout(watchdog: &ProviderWaitWatchdog) {
    assert_eq!(select(watchdog, Instant::now()), Timeout);
    assert_eq!(
        classify_turn_failure(
            &CommandError::internal(watchdog.timeout_failure()),
            ChatFailureKind::Other
        ),
        ChatFailureKind::ProviderTimeout
    );
}

fn execution_model() -> ExecutionModelLock {
    ExecutionModelLock {
        profile_id: "openai-codex/gpt-5.6-luna".into(),
        name: "GPT-5.6 Luna".into(),
        provider: "openai-codex".into(),
        model: "gpt-5.6-luna".into(),
        thinking_level: "medium".into(),
        run_options: Default::default(),
    }
}

#[tokio::test(start_paused = true)]
async fn provider_wait_watchdog_bounds_tool_silence_and_keeps_the_first_body_budget() {
    let body = json!({
        "type": "message_update",
        "assistantMessageEvent": {"type": "text_delta", "delta": "Working"}
    });
    for body_first in [false, true] {
        for id in [Some("python-call"), None] {
            let mut watchdog = ProviderWaitWatchdog::new(Instant::now(), Duration::from_secs(3));
            if body_first {
                observe(&mut watchdog, Instant::now(), body.clone());
            }
            observe(
                &mut watchdog,
                Instant::now(),
                json!({"type": "tool_execution_start", "toolCallId": id}),
            );
            // Pi output, even an end missing the active call id, extends tool silence.
            let activity: &[&str] = if id.is_some() {
                &[
                    "tool_execution_update",
                    "message_update",
                    "tool_execution_end",
                ]
            } else {
                &["tool_execution_update", "message_update"]
            };
            for kind in activity {
                tokio::time::advance(CHAT_TOOL_SILENCE_TIMEOUT - Duration::from_secs(1)).await;
                observe(&mut watchdog, Instant::now(), json!({ "type": kind }));
                assert_eq!(select(&watchdog, Instant::now()), Fair);
                assert_eq!(
                    watchdog.deadline(),
                    Some(Instant::now() + CHAT_TOOL_SILENCE_TIMEOUT)
                );
            }
            tokio::time::sleep_until(watchdog.deadline().expect("tool must stay bounded")).await;
            assert_provider_timeout(&watchdog);
        }
    }
    // A pre-body tool pauses, but does not reset, the first-body budget.
    for id in [Some("python-call"), None] {
        let mut watchdog = ProviderWaitWatchdog::new(Instant::now(), Duration::from_secs(3));
        tokio::time::advance(Duration::from_secs(10)).await;
        observe(
            &mut watchdog,
            Instant::now(),
            json!({"type": "tool_execution_start", "toolCallId": id}),
        );
        tokio::time::advance(Duration::from_secs(1200)).await;
        observe(
            &mut watchdog,
            Instant::now(),
            json!({"type": "tool_execution_end", "toolCallId": id}),
        );
        let remaining = CHAT_FIRST_PROVIDER_BODY_PROGRESS_TIMEOUT - Duration::from_secs(10);
        assert_eq!(watchdog.deadline(), Some(Instant::now() + remaining));
        tokio::time::sleep_until(watchdog.deadline().unwrap()).await;
        assert_provider_timeout(&watchdog);
    }
}

#[tokio::test(start_paused = true)]
async fn provider_wait_watchdog_bounds_compaction_and_rearms_after_tools() {
    let bound = Duration::from_secs(3);
    for body_first in [false, true] {
        let start = Instant::now();
        let mut watchdog = ProviderWaitWatchdog::new(start, bound);
        if body_first {
            observe(
                &mut watchdog,
                start,
                json!({
                    "type": "message_update",
                    "assistantMessageEvent": {"type": "text_delta", "delta": "Working"}
                }),
            );
        }
        observe(&mut watchdog, start, json!({"type": "compaction_start"}));
        let original_cap = watchdog.deadline();
        tokio::time::advance(Duration::from_secs(30)).await;
        observe(
            &mut watchdog,
            Instant::now(),
            json!({"type": "compaction_start"}),
        );
        observe(
            &mut watchdog,
            Instant::now(),
            json!({"type": "compaction_end"}),
        );
        assert_eq!(
            watchdog.deadline(),
            original_cap,
            "nested ends wait for the last"
        );
        tokio::time::advance(Duration::from_secs(90)).await;
        assert_eq!(select(&watchdog, Instant::now()), Fair);
        observe(
            &mut watchdog,
            Instant::now(),
            json!({"type": "compaction_end"}),
        );
        let resumed = if body_first {
            bound
        } else {
            CHAT_FIRST_PROVIDER_BODY_PROGRESS_TIMEOUT
        };
        assert_eq!(watchdog.deadline(), Some(Instant::now() + resumed));

        // An unended compaction stays bounded even while Pi keeps emitting events.
        observe(
            &mut watchdog,
            Instant::now(),
            json!({"type": "compaction_start"}),
        );
        let cap = watchdog.deadline().unwrap();
        observe(
            &mut watchdog,
            Instant::now(),
            json!({"type": "compaction_start"}),
        );
        tokio::time::sleep_until(cap).await;
        observe(&mut watchdog, Instant::now(), json!({"type": "turn_start"}));
        assert_provider_timeout(&watchdog);
    }

    // Overlapping tools and compaction exempt the idle wait, which resumes afterwards.
    let start = Instant::now();
    let mut watchdog = ProviderWaitWatchdog::new(start, bound);
    observe(
        &mut watchdog,
        start,
        json!({"type": "message_update", "assistantMessageEvent": {"type": "toolcall_start"}}),
    );
    for id in ["python-call", "world"] {
        observe(
            &mut watchdog,
            start,
            json!({"type": "tool_execution_start", "toolCallId": id}),
        );
    }
    let later = start + crate::pi::PI_HTTP_IDLE_TIMEOUT * 4;
    assert_eq!(watchdog.deadline(), Some(start + CHAT_TOOL_SILENCE_TIMEOUT));
    assert_eq!(select(&watchdog, later), Fair);
    observe(
        &mut watchdog,
        later,
        json!({"type": "tool_execution_end", "toolCallId": "python-call"}),
    );
    assert_eq!(watchdog.deadline(), Some(later + CHAT_TOOL_SILENCE_TIMEOUT));
    observe(
        &mut watchdog,
        later,
        json!({"type": "tool_execution_end", "toolCallId": "world"}),
    );
    assert_eq!(watchdog.deadline(), Some(later + bound));
    observe(&mut watchdog, later, json!({"type": "compaction_start"}));
    assert_eq!(
        watchdog.deadline(),
        Some(later + CHAT_COMPACTION_TOTAL_TIMEOUT)
    );
    observe(
        &mut watchdog,
        later + bound * 10,
        json!({"type": "compaction_end", "result": {}}),
    );
    assert_eq!(watchdog.deadline(), Some(later + bound * 11));
    assert_eq!(select(&watchdog, later + bound * 11), Timeout);
}

#[test]
fn pi_progress_and_compaction_failure_follow_pi_event_fields() {
    use tokio::sync::broadcast::error::RecvError;
    let update =
        |kind: &str| json!({"type": "message_update", "assistantMessageEvent": {"type": kind}});
    let body =
        |event: Result<PiEvent, RecvError>| pi_event_indicates_first_provider_body_progress(&event);
    for kind in [
        "thinking_start",
        "thinking_delta",
        "thinking_end",
        "text_start",
        "text_delta",
        "text_end",
        "toolcall_start",
        "toolcall_delta",
        "toolcall_end",
    ] {
        assert!(
            body(Ok(PiEvent::Rpc {
                payload: update(kind)
            })),
            "{kind}"
        );
    }
    assert!(body(Ok(PiEvent::Rpc {
        payload: json!({"type": "message_end", "message": {"role": "assistant"}}),
    })));
    // Lifecycle events only prove that Pi accepted the turn.
    for payload in [
        json!({"type": "response"}),
        json!({"type": "agent_start"}),
        json!({"type": "message_start", "message": {"role": "assistant"}}),
        json!({"type": "message_end", "message": {"role": "user"}}),
        json!({"type": "message_end", "message": {"role": "tool"}}),
        json!({"type": "message_update"}),
        update("message_start"),
        update("tool_result"),
        json!({"type": "tool_execution_start"}),
        json!({"type": "compaction_start"}),
        json!({"type": "auto_retry_start"}),
    ] {
        assert!(
            !body(Ok(PiEvent::Rpc {
                payload: payload.clone()
            })),
            "{payload}"
        );
    }
    assert!(!body(Err(RecvError::Lagged(1))));
    assert!(!body(Err(RecvError::Closed)));
    assert!(!body(Ok(PiEvent::Exited)));

    // A cancelled summary differs from a completed summary or an actual error.
    for (payload, outcome) in [
        (
            json!({"result": {"summary": "Kept"}, "aborted": false, "willRetry": true}),
            CompactionOutcome::Succeeded,
        ),
        (
            json!({"result": null, "aborted": false, "errorMessage": "quota exceeded"}),
            CompactionOutcome::Failed,
        ),
        (
            json!({"result": null, "aborted": true}),
            CompactionOutcome::Cancelled,
        ),
        (
            json!({"aborted": true, "errorMessage": "failed"}),
            CompactionOutcome::Failed,
        ),
        (
            json!({"aborted": true, "result": {}}),
            CompactionOutcome::Failed,
        ),
        (
            json!({"success": true, "result": null, "aborted": false}),
            CompactionOutcome::Failed,
        ),
    ] {
        assert_eq!(compaction_outcome(&payload), outcome, "{payload}");
    }
}

#[test]
fn classify_turn_failure_keeps_host_categories_and_precedence() {
    for (code, reason, fallback, expected) in [
        (
            "conflict",
            "thread changed",
            ChatFailureKind::MemoryCommitFailed,
            ChatFailureKind::CancelledByConflict,
        ),
        (
            "internal",
            "Pi provider turn failed",
            ChatFailureKind::StoreFailed,
            ChatFailureKind::StoreFailed,
        ),
        (
            "internal",
            "Pi settled without an assistant message",
            ChatFailureKind::Other,
            ChatFailureKind::EmptyReply,
        ),
        // Typed provider failures (X11) travel as fixed host messages, never provider text.
        (
            "internal",
            crate::pi::provider_failure::RATE_LIMITED,
            ChatFailureKind::PiExited,
            ChatFailureKind::RateLimited,
        ),
        (
            "internal",
            crate::pi::provider_failure::NEEDS_RECONNECT,
            ChatFailureKind::PiExited,
            ChatFailureKind::NeedsReconnect,
        ),
        (
            "internal",
            crate::pi::provider_failure::PLAN_LIMIT,
            ChatFailureKind::PiExited,
            ChatFailureKind::PlanLimit,
        ),
        (
            "internal",
            crate::pi::provider_failure::PLAN_NOT_ELIGIBLE,
            ChatFailureKind::PiExited,
            ChatFailureKind::PlanNotEligible,
        ),
        (
            "internal",
            crate::pi::provider_failure::OUT_OF_CREDIT,
            ChatFailureKind::PiExited,
            ChatFailureKind::OutOfCredit,
        ),
        (
            "internal",
            crate::pi::provider_failure::MODEL_GONE,
            ChatFailureKind::PiExited,
            ChatFailureKind::ModelGone,
        ),
        (
            "internal",
            crate::pi::provider_failure::NETWORK,
            ChatFailureKind::PiExited,
            ChatFailureKind::Offline,
        ),
        (
            "internal",
            crate::pi::provider_failure::UNAVAILABLE,
            ChatFailureKind::PiExited,
            ChatFailureKind::ProviderUnavailable,
        ),
        (
            "internal",
            crate::pi::provider_failure::CONTEXT_TOO_LONG,
            ChatFailureKind::PiExited,
            ChatFailureKind::ContextTooLong,
        ),
        (
            "internal",
            crate::pi::provider_failure::OTHER,
            ChatFailureKind::PiExited,
            ChatFailureKind::ProviderError,
        ),
        (
            "pi_unavailable",
            "Pi session initialization timed out",
            ChatFailureKind::PiExited,
            ChatFailureKind::ProviderTimeout,
        ),
        (
            "internal",
            "Pi stopped before the turn settled: exit",
            ChatFailureKind::Other,
            ChatFailureKind::PiExited,
        ),
        (
            "internal",
            "Pi event stream was interrupted",
            ChatFailureKind::Other,
            ChatFailureKind::ProtocolError,
        ),
        (
            "internal",
            "unknown host failure",
            ChatFailureKind::Other,
            ChatFailureKind::Other,
        ),
    ] {
        assert_eq!(
            classify_turn_failure(&CommandError::new(code, reason), fallback),
            expected,
            "{reason}"
        );
    }
}

#[tokio::test]
async fn failed_writer_handoff_stays_active_until_the_failed_assistant_is_persisted() {
    use crate::{
        chat::execution_session::ChatExecutionSession, chat::turn::ChatTurnResources,
        runs::scratch::RunScratch,
    };
    let root = tempfile::tempdir().unwrap();
    let state = AppState::for_test(root.path().join("app"));
    crate::commands::tests::seed_profile(
        state.store.as_ref(),
        &crate::commands::tests::profile("loupist-handoff", &root.path().join("workspace"), 1),
    );
    let chat = crate::commands::tests::chat("chat-handoff", "loupist-handoff", 1);
    state.store.create_chat(&chat).unwrap();
    let (control, _) = chat_control_channel();
    let reader = state
        .register_chat_run(
            "run-handoff".into(),
            chat.loupist_id.clone(),
            chat.id.clone(),
            control,
        )
        .unwrap();
    let deletion = state.artifacts.deletion_root.clone();
    let scratch = RunScratch::create(deletion.clone(), &chat.loupist_id, "run-handoff").unwrap();
    let session = ChatExecutionSession::prepare(deletion, &chat).unwrap();
    let mut resources = ChatTurnResources::new(reader.lease, scratch, session);
    let (_, mut pending) =
        memory_updates::reserve_chat_memory_finalization(&state, &chat.loupist_id).unwrap();
    pending.wait_error = Some(CommandError::internal("queue unavailable"));
    let (writer, ready) = resources.handoff_to_memory_write(pending).await;
    assert!(ready.is_err());
    assert_eq!(state.run_coordinator.active_count(), 1);
    assert!(state
        .store
        .get_chat(&chat.id)
        .unwrap()
        .unwrap()
        .messages
        .is_empty());
    let sink = ChatEventSink::new(|_| Ok(()));
    sink.set_failure_kind(ChatFailureKind::StoreFailed);
    persist_and_emit_failed_chat(
        state.store.as_ref(),
        &sink,
        "run-handoff",
        &chat.id,
        "failed-handoff",
        None,
        execution_model(),
        crate::agent_harness::snapshot("chat", &[], &[]).unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(state.run_coordinator.active_count(), 1);
    drop(writer);
    state.run_coordinator.drain().await.unwrap();
    let persisted = state.store.get_chat(&chat.id).unwrap().unwrap();
    assert_eq!(persisted.messages[0].status, ChatMessageStatus::Error);
    assert_eq!(
        persisted.messages[0].failure_kind,
        Some(ChatFailureKind::StoreFailed)
    );
}

#[test]
fn terminal_replies_keep_acknowledged_records_across_reopen_without_a_renderer() {
    use crate::{
        domain::{ChatEvidenceKind, ChatEvidenceRef, ToolTraceOutcome},
        scheduled_tasks::types::{
            Schedule, ScheduleAction, ScheduleReceipt, TaskStatus, TaskTarget,
        },
        store::SqliteStore,
    };
    let temporary = tempfile::tempdir().unwrap();
    let database = temporary.path().join("test.sqlite");
    let store = SqliteStore::open(&database).unwrap();
    crate::commands::tests::seed_profile(
        &store,
        &crate::commands::tests::profile("loupist", &temporary.path().join("work"), 1),
    );
    let evidence = ChatEvidenceRef {
        id: format!("ev_{}", "a".repeat(32)),
        title: "Issuer filing".into(),
        kind: ChatEvidenceKind::Original,
        url: Some("https://issuer.example/filing.pdf".into()),
        provider: Some("issuer.example".into()),
        tool_name: "web_fetch".into(),
        calc_operation: None,
        read_ref: None,
        retrieved_at: "2026-10-11T00:00:00Z".into(),
        published_at: None,
        as_of: None,
        coverage: None,
        excerpt: "영업이익: 12.166조 원".into(),
        excerpt_match: None,
        excerpt_truncated: false,
        content_sha256: "b".repeat(64),
        warnings: vec![],
        dataset_ref: None,
        model_delivery: None,
        upstream_evidence_ids: vec![],
    };
    let receipt = ScheduleReceipt::Task(crate::scheduled_tasks::types::TaskReceipt {
        task_id: "schedule-survives-stop".into(),
        title: "Daily research".into(),
        revision: 1,
        action: ScheduleAction::Created,
        status: TaskStatus::Paused,
        schedule: Schedule::Interval { every_minutes: 60 },
        target: TaskTarget::Dedicated,
        next_run_at_ms: None,
        use_memory: false,
        update_memory: false,
        awaiting_confirmation: false,
    });
    let refusal = crate::domain::ChatToolRefusal {
        connection_id: "custom-test".into(),
        connection_name: "Company research".into(),
        tool_name: "read_filings".into(),
        reason: crate::domain::ChatToolRefusalReason::NeedsApproval,
    };
    let cases = [
        (
            "chat-aborted",
            ChatTerminalStatus::Aborted,
            ChatMessageStatus::Aborted,
            None,
        ),
        (
            "chat-error",
            ChatTerminalStatus::Error,
            ChatMessageStatus::Error,
            Some(ChatFailureKind::MemoryCommitFailed),
        ),
    ];
    for (id, terminal, _, _) in cases {
        store
            .create_chat(&crate::commands::tests::chat(id, "loupist", 1))
            .unwrap();
        // A closed renderer must not discard what the model already acknowledged.
        let sink = ChatEventSink::new(|_| Err(()));
        sink.set_failure_kind(ChatFailureKind::MemoryCommitFailed);
        sink.tool_refusals.lock().unwrap().push(refusal.clone());
        for _ in 0..2 {
            let _ = sink.send(ChatStreamEvent::Evidence {
                run_id: "run-terminal".into(),
                evidence: Box::new(evidence.clone()),
            });
            let _ = sink.send(ChatStreamEvent::Schedule {
                run_id: "run-terminal".into(),
                receipt: receipt.clone(),
            });
        }
        sink.tool_trace
            .observe_start("load", "capability_load", &json!({"id": "opendart"}));
        sink.tool_trace
            .observe_end("load", false, &json!({"loaded": true}));
        sink.tool_trace
            .observe_start("failed", "grep", &json!({"pattern": "ls"}));
        sink.tool_trace.observe_end("failed", true, &Value::Null);
        persist_and_emit_terminal_chat(
            &store,
            &sink,
            "run-terminal",
            id,
            &format!("assistant-{id}"),
            "Partial answer".into(),
            empty_memory_trace().unwrap(),
            None,
            execution_model(),
            crate::agent_harness::snapshot("chat", &[], &[]).unwrap(),
            None,
            terminal,
        )
        .unwrap();
    }
    drop(store);
    let store = SqliteStore::open(&database).unwrap();
    for (id, _, status, failure_kind) in cases {
        let message = &store.get_chat(id).unwrap().unwrap().messages[0];
        assert_eq!(message.status, status);
        assert_eq!(message.failure_kind, failure_kind);
        assert_eq!(message.evidence_refs, vec![evidence.clone()]);
        assert_eq!(message.schedule_receipts, vec![receipt.clone()]);
        assert_eq!(message.tool_refusals, vec![refusal.clone()]);
        assert!(message.memory_update.is_none());
        let trace = message.tool_trace.as_ref().unwrap();
        assert_eq!(trace.entries[0].name, "capability_load");
        assert_eq!(trace.entries[1].outcome, ToolTraceOutcome::Error);
    }
}
