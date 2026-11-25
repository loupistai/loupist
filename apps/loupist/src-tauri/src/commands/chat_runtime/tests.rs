use super::*;
use serde_json::json;
use tauri::Manager;
use FirstProviderBodyWatchdogSelectMode::{Fair, Timeout};

mod admission;

#[test]
fn price_setup_host_context_starts_the_extension_with_bundled_capabilities() {
    let root = tempfile::tempdir().unwrap();
    let workspace = agent_harness::workspace::workspace_snapshot(root.path(), root.path()).unwrap();
    let capabilities = [
        "community.web-research",
        "defillama.tvl",
        "openbb.platform",
        "openfigi.lookup",
        "world-bank.indicators",
    ]
    .map(String::from);
    let mut request = crate::commands::tests::chat_behavior::send_request(
        "price-setup-fixture",
        "loupist-a",
        "price-setup-thread",
        "Find a price.",
    );
    request.use_memory = true;
    request.update_memory = true;
    let context = setup::chat_host_context(
        &request,
        &request.prompt,
        crate::chat::surface::ChatSurface::PriceSetup,
        &[],
        &capabilities,
        workspace,
    )
    .unwrap();
    let context_path = root.path().join("host-context.json");
    std::fs::write(&context_path, serde_json::to_vec(&context).unwrap()).unwrap();
    let output = std::process::Command::new("node")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/commands/tests/fixtures/host_context_probe.mjs"),
        )
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("LOUPIST_HOST_CONTEXT_FILE", &context_path)
        .env("LOUPIST_MODEL_RUN_OPTIONS", "{}")
        .output()
        .expect("Node is required for the extension startup probe");
    assert!(
        output.status.success(),
        "extension startup failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !context_path.exists(),
        "extension did not load the host context"
    );
}

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

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn fx_admitted_envelope_and_tool_keep_the_same_currency_across_a_preference_write() {
    use crate::{
        broker::{ToolExecutor, ToolMethod},
        commands::tests::{
            chat,
            chat_behavior::{install_chat_rpc_fixture, send_request},
            initialized_workspace, profile, seed_profile,
        },
    };
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    initialized_workspace(&workspace, "frozen-appearance");
    let mut state = AppState::for_test(temporary.path().join("app"));
    install_chat_rpc_fixture(
        &mut state,
        temporary.path(),
        include_str!("../tests/fixtures/model_gone_pi.py"),
    );
    let profile = profile("loupist-a", &workspace, 1);
    seed_profile(state.store.as_ref(), &profile);
    for id in ["chat-a", "chat-b"] {
        state.store.create_chat(&chat(id, &profile.id, 1)).unwrap();
    }
    state
        .fx_reference
        .resolve(chrono::Utc::now(), true, || async {
            Ok(json!({"date":"2099-11-17","KRW":1470.23,"USD":1.1014,"JPY":172.1}))
        })
        .await
        .unwrap();
    let set_currency = |currency| {
        std::fs::write(
            state.artifacts.app_data_dir.join("config.toml"),
            format!("[appearance]\ndisplay_currency = '{currency}'\nlanguage = 'en'\n"),
        )
        .unwrap();
    };
    let admit = |run, thread| {
        let mut request = send_request(run, &profile.id, thread, "Question");
        request.use_memory = false;
        request.update_memory = false;
        match setup::admit_chat_turn(request, ChatEventSink::new(|_| Ok(())), &state, None).unwrap()
        {
            ControlFlow::Continue(turn) => turn,
            ControlFlow::Break(_) => panic!("turn must be admitted"),
        }
    };
    set_currency("KRW");
    let previous = admit("fx-admit-a", "chat-a");
    let execute = |policy: crate::broker::ToolPolicy| {
        let executor = crate::commands::tool_executor::AppToolExecutor::for_host(
            &state,
            &profile.id,
            ["openbb.platform".into()].into(),
        );
        async move {
            executor
                .execute(&policy, ToolMethod::FxRate, json!({"from":"USD"}))
                .await
                .unwrap()
        }
    };
    assert_eq!(execute(previous.policy.clone()).await["to"], "KRW");
    set_currency("JPY");
    assert_eq!(execute(previous.policy.clone()).await["to"], "KRW");
    let next = admit("fx-admit-b", "chat-b");
    assert_eq!(execute(next.policy.clone()).await["to"], "JPY");
    assert_eq!(previous.runtime_policy_sha256, next.runtime_policy_sha256);

    let mut digests = Vec::new();
    for (turn, expected) in [(previous, "KRW"), (next, "JPY")] {
        let envelope = launch::build_turn_envelope(
            turn.envelope,
            &state,
            &turn.profile,
            false,
            &turn.surface,
            &ToolCapture::default(),
        )
        .await
        .unwrap();
        let rendered: Value =
            serde_json::from_str(&agent_harness::render_turn_context(&envelope)).unwrap();
        assert_eq!(rendered["interface"]["display_currency"], expected);
        digests.push(agent_harness::envelope_context_digest(&envelope));
    }
    assert_ne!(digests[0], digests[1]);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn a_new_loupists_focus_reaches_the_turn_from_its_profile_without_a_lens_page() {
    use crate::commands::tests::{
        chat,
        chat_behavior::{install_chat_rpc_fixture, send_request},
    };
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("research");
    std::fs::create_dir(&workspace).unwrap();
    let mut state = AppState::for_test(temporary.path().join("app"));
    install_chat_rpc_fixture(
        &mut state,
        temporary.path(),
        include_str!("../tests/fixtures/model_gone_pi.py"),
    );
    let focus = "Korean memory makers and HBM capacity";
    let created = crate::commands::loupist::create_folder_loupist_with_setup(
        &state,
        &workspace,
        Some(focus.into()),
        None,
    )
    .await
    .unwrap();
    let perspective =
        crate::knowledge::KnowledgeStore::for_loupist(state.store.clone(), &created.id)
            .unwrap()
            .read(|write| write.active_perspective(&created.id))
            .unwrap();
    assert_eq!(
        perspective, None,
        "a new Loupist's focus files no Lens page"
    );
    // The test store does not write the folder; a turn reads the folder's settings.
    std::fs::create_dir_all(workspace.join(".loupist")).unwrap();
    state
        .store
        .create_chat(&chat("focus-chat", &created.id, 1))
        .unwrap();
    for memory in [false, true] {
        let mut request = send_request(
            &format!("focus-run-{memory}"),
            &created.id,
            "focus-chat",
            "Question",
        );
        request.use_memory = memory;
        request.update_memory = false;
        let ControlFlow::Continue(turn) =
            setup::admit_chat_turn(request, ChatEventSink::new(|_| Ok(())), &state, None).unwrap()
        else {
            panic!("turn must be admitted");
        };
        let envelope = launch::build_turn_envelope(
            turn.envelope,
            &state,
            &turn.profile,
            memory,
            &turn.surface,
            &ToolCapture::default(),
        )
        .await
        .unwrap();
        let rendered: Value =
            serde_json::from_str(&agent_harness::render_turn_context(&envelope)).unwrap();
        assert_eq!(rendered[agent_harness::LOUPIST_FOCUS_BLOCK], focus);
        assert!(rendered.get("Standing perspective").is_none());
    }
}

/// The model writes its answer and its progress notes in the language of the
/// question. The turn names the question's script, next to the question, and
/// not the Loupist's: an English question in a Loupist whose focus is Korean
/// (the v0.1.6 acceptance, whose notes came back Korean), and the reverse.
#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn a_turn_names_the_script_of_its_question_not_of_its_loupist() {
    use crate::agent_harness::message_script::CURRENT_MESSAGE_SCRIPT;
    use crate::commands::tests::{
        chat,
        chat_behavior::{install_chat_rpc_fixture, send_request},
    };
    for (index, (focus, question, script)) in [
        (
            "미국 대형 기술 기업",
            "When does Adriana Kugler's term on the Fed Board end, and how did she get onto the Board?",
            "Latn",
        ),
        (
            "Large US technology companies",
            "애드리아나 쿠글러의 연준 이사 임기는 언제 끝나고, 어떻게 이사가 됐어?",
            "Hang",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("research");
        std::fs::create_dir(&workspace).unwrap();
        let mut state = AppState::for_test(temporary.path().join("app"));
        install_chat_rpc_fixture(
            &mut state,
            temporary.path(),
            include_str!("../tests/fixtures/model_gone_pi.py"),
        );
        let created = crate::commands::loupist::create_folder_loupist_with_setup(
            &state,
            &workspace,
            Some(focus.into()),
            None,
        )
        .await
        .unwrap();
        std::fs::create_dir_all(workspace.join(".loupist")).unwrap();
        state
            .store
            .create_chat(&chat("language-chat", &created.id, 1))
            .unwrap();
        let request = send_request(
            &format!("language-run-{index}"),
            &created.id,
            "language-chat",
            question,
        );
        let ControlFlow::Continue(turn) =
            setup::admit_chat_turn(request, ChatEventSink::new(|_| Ok(())), &state, None).unwrap()
        else {
            panic!("turn must be admitted");
        };
        let envelope = launch::build_turn_envelope(
            turn.envelope,
            &state,
            &turn.profile,
            false,
            &turn.surface,
            &ToolCapture::default(),
        )
        .await
        .unwrap();
        let rendered = agent_harness::render_turn_context(&envelope);
        let value: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(value[agent_harness::LOUPIST_FOCUS_BLOCK], focus);
        assert_eq!(value[CURRENT_MESSAGE_SCRIPT], script, "{rendered}");
        // The block nearest the message: only the clock comes after it.
        assert!(
            rendered.contains(&format!(
                "\"{CURRENT_MESSAGE_SCRIPT}\":\"{script}\",\"live_time\":"
            )),
            "{rendered}"
        );
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn interface_labels_follow_turn_origin_and_freeze_the_admitted_appearance() {
    use crate::{
        broker::{ToolExecutor, ToolMethod},
        chat::{
            handoff::{
                claim_fixture, commit_standalone, HandoffRequest, HandoffRun, StagedHandoff,
            },
            surface::ChatSurface,
        },
        commands::tests::{
            chat_behavior::{install_chat_rpc_fixture, send_request},
            initialized_workspace, profile, seed_profile,
        },
    };

    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    initialized_workspace(&workspace, "interface-labels");
    let mut state = AppState::for_test(temp.path().join("app"));
    install_chat_rpc_fixture(
        &mut state,
        temp.path(),
        include_str!("../tests/fixtures/model_gone_pi.py"),
    );
    let profile = profile("loupist-interface", &workspace, 1);
    seed_profile(state.store.as_ref(), &profile);
    let home = crate::chat::home::ensure_home_thread(&state, &profile.id).unwrap();
    let handoff = commit_standalone(
        &state,
        &profile.id,
        &StagedHandoff {
            request: HandoffRequest::parse(&json!({
                "title": "Alder Rail",
                "question": "Write the answer in French.",
                "requestKey": "interface-labels"
            }))
            .unwrap(),
            run: HandoffRun {
                model_profile_id: "fixture".into(),
                thinking_level: "medium".into(),
                run_options: BTreeMap::new(),
                use_memory: true,
                update_memory: false,
            },
        },
    )
    .unwrap();

    let set_appearance = |lang| {
        std::fs::write(
            state.artifacts.app_data_dir.join("config.toml"),
            format!("[appearance]\nlanguage = '{lang}'\n"),
        )
        .unwrap();
    };
    for (index, (thread, expected, memory)) in [
        (&home.id, ChatSurface::Bot, false),
        (&home.id, ChatSurface::Scheduled, true),
        (
            &handoff.chat_id,
            ChatSurface::Delegated {
                handoff_id: handoff.id.clone(),
            },
            true,
        ),
        (&handoff.chat_id, ChatSurface::Session, true),
    ]
    .into_iter()
    .enumerate()
    {
        set_appearance(if index == 3 { "en" } else { "ko" });
        let run = format!("labels-{index}");
        let (admission, prompt) = match &expected {
            ChatSurface::Delegated { .. } => {
                let (admission, prompt) = claim_fixture(&state, &handoff, &run);
                (Some(admission), prompt)
            }
            ChatSurface::Scheduled => (
                Some(ChatAdmission::new(&state, &run, &profile.id, thread).unwrap()),
                "Write the answer in French.".into(),
            ),
            _ => (None, "Where is World?".into()),
        };
        let mut request = send_request(&run, &profile.id, thread, &prompt);
        request.use_memory = memory;
        request.update_memory = false;
        let admitted =
            setup::admit_chat_turn(request, ChatEventSink::new(|_| Ok(())), &state, admission)
                .unwrap();
        let ControlFlow::Continue(admitted) = admitted else {
            panic!("fixture turn was not admitted");
        };
        assert_eq!(admitted.surface, expected);
        let host: Value = serde_json::from_str(&admitted.host_context).unwrap();
        let tools = host["agent_runtime"]["core_tool_names"].as_array().unwrap();
        assert_eq!(
            tools.contains(&json!("interface_labels")),
            expected.interactive()
        );
        assert!(tools.contains(&json!("chat_history")));
        let policy = admitted.policy.clone();
        set_appearance(if index == 3 { "ko" } else { "en" });
        let launched = launch::launch_chat_turn(admitted, &state).await.unwrap();
        let ControlFlow::Continue(mut launched) = launched else {
            panic!("fixture turn did not launch");
        };
        let seed = launched.gone_retry.as_ref().unwrap();
        let envelope: Value = serde_json::from_str(&seed.turn_envelope).unwrap();
        if expected.interactive() {
            assert!(envelope.get("interface").is_none());
        } else {
            assert_eq!(envelope["interface"]["language"], "ko");
            assert_eq!(envelope["interface"]["names"]["world"], "월드");
            assert!(seed.current_prompt.contains("Write the answer in French."));
        }

        let executor = crate::commands::tool_executor::AppToolExecutor {
            state: state.clone(),
            capture: launched.capture.clone(),
            loupist_id: profile.id.clone(),
            capability_ids: Default::default(),
        };
        // Local labels stay available after an acknowledged third-party read.
        launched.capture.turn_taint.mark_untrusted_text();
        let result = executor
            .execute(
                &policy,
                ToolMethod::InterfaceLabels,
                json!({"keys": ["world"]}),
            )
            .await;
        match expected {
            ChatSurface::Bot => assert_eq!(result.unwrap(), json!({"world": "월드"})),
            ChatSurface::Session => assert_eq!(result.unwrap(), json!({"world": "World"})),
            _ => assert!(result.is_err()),
        }
        let streamed = stream::stream_chat_turn(&mut launched).await;
        finalize::finalize_chat_turn(launched, streamed).await;
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn a_stopped_reply_with_a_settle_failure_is_persisted_and_emitted_once() {
    use crate::commands::tests::{
        chat,
        chat_behavior::{install_chat_rpc_fixture, send_request},
        initialized_workspace, profile, seed_profile,
    };
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    initialized_workspace(&workspace, "abort-settle-failure");
    let mut state = AppState::for_test(temporary.path().join("app"));
    install_chat_rpc_fixture(
        &mut state,
        temporary.path(),
        include_str!("../tests/fixtures/model_gone_pi.py"),
    );
    let profile = profile("loupist-a", &workspace, 1);
    seed_profile(state.store.as_ref(), &profile);
    state
        .store
        .create_chat(&chat("chat-a", &profile.id, 1))
        .unwrap();
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = events.clone();
    let sink = ChatEventSink::new(move |event| {
        seen.lock().unwrap().push(event);
        Ok(())
    });
    let admitted = match setup::admit_chat_turn(
        send_request("abort-settle", &profile.id, "chat-a", "Question"),
        sink,
        &state,
        None,
    )
    .unwrap()
    {
        ControlFlow::Continue(turn) => turn,
        ControlFlow::Break(_) => panic!("turn must be admitted"),
    };
    let launched = match launch::launch_chat_turn(admitted, &state).await.unwrap() {
        ControlFlow::Continue(turn) => turn,
        ControlFlow::Break(_) => panic!("fixture must launch"),
    };
    state
        .cancel_run("abort-settle", RunKind::Chat)
        .await
        .unwrap();
    finalize::finalize_chat_turn(
        launched,
        stream::StreamedChatTurn {
            content: "Partial answer".into(),
            completed: false,
            aborted: true,
            terminal_completion_claimed: false,
            failure: Some("settle failed".into()),
            pending_controls: HashMap::new(),
            settled_controls: BTreeMap::new(),
            first_text_ms: None,
            answer_text_ms: None,
            generation_ms: None,
            token_usage: AssistantTokenUsage::default(),
            progress: ChatProgressProjection::new(now_ms()),
            commentary: CommentaryCoalescer::new(),
            envelope_compacted: false,
        },
    )
    .await;
    let chat = state.store.get_chat("chat-a").unwrap().unwrap();
    assert_eq!(chat.messages.len(), 2);
    assert_eq!(chat.messages[1].status, ChatMessageStatus::Aborted);
    assert_eq!(chat.messages[1].content, "Partial answer");
    let events = events.lock().unwrap();
    let terminal: Vec<_> = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                ChatStreamEvent::Aborted { .. } | ChatStreamEvent::Error { .. }
            )
        })
        .collect();
    assert!(matches!(
        terminal.as_slice(),
        [ChatStreamEvent::Aborted { .. }]
    ));
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
                    Instant::now() + CHAT_TOOL_SILENCE_TIMEOUT
                );
            }
            tokio::time::sleep_until(watchdog.deadline()).await;
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
        assert_eq!(watchdog.deadline(), Instant::now() + remaining);
        tokio::time::sleep_until(watchdog.deadline()).await;
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
        assert_eq!(watchdog.deadline(), Instant::now() + resumed);

        // An unended compaction stays bounded even while Pi keeps emitting events.
        observe(
            &mut watchdog,
            Instant::now(),
            json!({"type": "compaction_start"}),
        );
        let cap = watchdog.deadline();
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
    assert_eq!(watchdog.deadline(), start + CHAT_TOOL_SILENCE_TIMEOUT);
    assert_eq!(select(&watchdog, later), Fair);
    observe(
        &mut watchdog,
        later,
        json!({"type": "tool_execution_end", "toolCallId": "python-call"}),
    );
    assert_eq!(watchdog.deadline(), later + CHAT_TOOL_SILENCE_TIMEOUT);
    observe(
        &mut watchdog,
        later,
        json!({"type": "tool_execution_end", "toolCallId": "world"}),
    );
    assert_eq!(watchdog.deadline(), later + bound);
    observe(&mut watchdog, later, json!({"type": "compaction_start"}));
    assert_eq!(watchdog.deadline(), later + CHAT_COMPACTION_TOTAL_TIMEOUT);
    observe(
        &mut watchdog,
        later + bound * 10,
        json!({"type": "compaction_end", "result": {}}),
    );
    assert_eq!(watchdog.deadline(), later + bound * 11);
    assert_eq!(select(&watchdog, later + bound * 11), Timeout);
}

#[test]
fn pi_progress_and_compaction_failure_follow_pi_event_fields() {
    use tokio::sync::broadcast::error::RecvError;
    assert_eq!(
        pi_event_stream_failure(RecvError::Closed),
        "Pi event stream was interrupted"
    );
    assert_eq!(
        pi_event_stream_failure(RecvError::Lagged(2)),
        "Pi event stream fell behind and was stopped safely"
    );
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
            "memory_busy",
            "Waiting for Memory",
            ChatFailureKind::Other,
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

#[tokio::test(start_paused = true)]
async fn a_settled_reply_keeps_its_answer_when_an_older_reader_does_not_drain() {
    assert_pending_reply_is_kept(Some(crate::domain::MemorySaveSkipReason::Deadline), true).await;
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn interactive_answers_complete_directly_while_a_research_reader_stays_active() {
    use crate::commands::{
        knowledge::{file_test_perspective, memory_usage},
        tests::{
            chat,
            chat_behavior::{install_chat_rpc_fixture, send_request},
            initialized_workspace, profile, seed_profile,
        },
    };
    use crate::domain::MemoryUpdateStatus;

    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    initialized_workspace(&workspace, "chat-direct-completion");
    let mut state = AppState::for_folder_test(root.path().join("app"));
    install_chat_rpc_fixture(
        &mut state,
        root.path(),
        include_str!("../tests/fixtures/answer_checks_pi.py"),
    );
    let profile = profile("direct-completion", &workspace, 1);
    seed_profile(state.store.as_ref(), &profile);
    let lens = file_test_perspective(&state, &profile.id, "Check units before comparing.");
    let bot = crate::chat::home::ensure_home_thread(&state, &profile.id).unwrap();
    let session = chat("direct-session", &profile.id, 1);
    state.store.create_chat(&session).unwrap();
    let (controls, _) = chat_control_channel();
    let _older = state
        .register_chat_run(
            "older-research".into(),
            profile.id.clone(),
            "older-chat".into(),
            controls,
        )
        .unwrap();

    for (run_id, thread_id, update_memory) in [
        ("direct-bot-1", &bot.id, true),
        ("direct-bot-2", &bot.id, true),
        ("direct-session-1", &session.id, false),
    ] {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = events.clone();
        let sink = ChatEventSink::new(move |event| {
            recorded.lock().unwrap().push(event);
            Ok(())
        });
        let mut request = send_request(run_id, &profile.id, thread_id, "What was the margin?");
        request.use_memory = true;
        request.update_memory = update_memory;
        send_chat(request, sink, &state, None).await.unwrap();
        timeout(Duration::from_secs(10), async {
            while state.run_coordinator.active_count() != 1 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the interactive turn must finish while the older reader is held");

        let events = events.lock().unwrap();
        assert!(!events
            .iter()
            .any(|event| matches!(event, ChatStreamEvent::Saving { .. })));
        assert!(matches!(events.last(), Some(ChatStreamEvent::Completed {
            memory_save_skip: None, save_activity: None, number_checks, ..
        }) if number_checks.len() == 4));
        let stored = state.store.get_chat(thread_id).unwrap().unwrap();
        let answer = stored.messages.last().unwrap();
        assert_eq!(answer.status, ChatMessageStatus::Complete);
        // The fixture's answer also holds a miscopied link (rebound) and one naming no read.
        assert_eq!(answer.number_checks.len(), 4);
        assert_eq!(answer.evidence_refs.len(), 1);
        assert!(answer.progress.as_ref().unwrap().save_activity.is_none());
        assert!(answer
            .lens_receipts
            .iter()
            .any(|receipt| receipt.record_id == lens));
        assert_eq!(
            answer
                .memory_update
                .as_ref()
                .map(|receipt| (receipt.status, receipt.skip_reason)),
            update_memory.then_some((MemoryUpdateStatus::NoChange, None)),
        );
        if thread_id == &bot.id {
            let requests = stored
                .messages
                .iter()
                .filter(|message| message.role == ChatRole::User);
            assert!(requests
                .map(|request| &request.id)
                .eq(stored.bot_memory.unreviewed.iter()));
        }
    }
    let usage = memory_usage::usage(&state, &profile.id).unwrap();
    assert!(usage
        .iter()
        .any(|entry| entry.record_id == lens && entry.used_count == 3));
    let diagnostics_dir = workspace.join(".loupist/logs");
    let diagnostics = std::fs::read_dir(diagnostics_dir)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            (path.extension().and_then(|extension| extension.to_str()) == Some("jsonl"))
                .then(|| std::fs::read_to_string(path).unwrap())
        })
        .collect::<Vec<_>>();
    let records = diagnostics
        .iter()
        .flat_map(|file| file.lines())
        .map(|line| serde_json::from_str::<Value>(line).unwrap());
    assert_eq!(
        records
            .filter(|record| record["chatSavePath"] == "direct")
            .count(),
        3
    );
}

#[tokio::test(start_paused = true)]
async fn stop_while_saving_skips_memory_and_keeps_the_settled_reply() {
    assert_pending_reply_is_kept(Some(crate::domain::MemorySaveSkipReason::User), true).await;
}

#[tokio::test(start_paused = true)]
async fn a_memory_reset_while_waiting_leaves_staged_changes_behind() {
    assert_pending_reply_is_kept(
        Some(crate::domain::MemorySaveSkipReason::PolicyChanged),
        true,
    )
    .await;
}

#[tokio::test(start_paused = true)]
async fn a_failed_commit_names_the_memory_it_rolled_back() {
    assert_pending_reply_is_kept(Some(crate::domain::MemorySaveSkipReason::Failed), true).await;
}

#[tokio::test(start_paused = true)]
async fn quitting_after_the_answer_checkpoint_keeps_the_discarded_item_titles() {
    assert_pending_reply_is_kept(Some(crate::domain::MemorySaveSkipReason::Interrupted), true)
        .await;
}

#[tokio::test(start_paused = true)]
async fn skipping_a_save_without_staged_writes_omits_the_item_list() {
    assert_pending_reply_is_kept(Some(crate::domain::MemorySaveSkipReason::User), false).await;
}

#[tokio::test(start_paused = true)]
async fn saving_memory_clears_the_checkpoint_discarded_item_list() {
    assert_pending_reply_is_kept(None, true).await;
}

async fn assert_pending_reply_is_kept(
    expected_skip: Option<crate::domain::MemorySaveSkipReason>,
    stage_memory: bool,
) {
    use crate::chat::{
        artifacts::{
            ArtifactCommit, ChatArtifact, ChatArtifactKind, ChatArtifactPayload,
            ChatArtifactRevision,
        },
        chart::{ChartColumn, ChartColumnKind, ChartDataset, ChartDatasetLineage, ChartDocument},
    };
    use crate::domain::{DiscardedMemoryItem, DiscardedMemoryKind, MemorySaveSkipReason};
    let root = tempfile::tempdir().unwrap();
    let state = AppState::for_test(root.path().join("app"));
    let earlier =
        crate::commands::tests::profile("loupist-finalization", &root.path().join("workspace"), 1);
    crate::commands::tests::seed_profile(state.store.as_ref(), &earlier);
    let chat = crate::commands::tests::chat("chat-finalization", &earlier.id, 1);
    state.store.create_chat(&chat).unwrap();
    let (control, _) = chat_control_channel();
    let older = state
        .register_chat_run(
            "older".into(),
            earlier.id.clone(),
            "older-chat".into(),
            control,
        )
        .unwrap();
    let (control, _) = chat_control_channel();
    let settled = state
        .register_chat_run(
            "settled".into(),
            earlier.id.clone(),
            chat.id.clone(),
            control,
        )
        .unwrap();
    assert!(state
        .claim_run_completion("settled", RunKind::Chat)
        .unwrap());
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = events.clone();
    let pending = Arc::new(tokio::sync::Notify::new());
    let queued = pending.clone();
    let sink = ChatEventSink::new(move |event| {
        if matches!(
            event,
            ChatStreamEvent::Saving {
                waiting_for_other_chat: true,
                ..
            }
        ) {
            queued.notify_one();
        }
        observed.lock().unwrap().push(event);
        Ok(())
    });
    let reply = TerminalReply {
        state: &state,
        on_event: &sink,
        run_id: "settled",
        loupist_id: &earlier.id,
        thread_id: &chat.id,
        message_id: "settled-reply",
        memory_revision: &None,
        harness: &crate::agent_harness::snapshot("chat", &[], &[]).unwrap(),
    };
    let mut progress = ChatProgressProjection::new(now_ms());
    let mut failure = None;
    let model = execution_model();
    let capture = ToolCapture::default();
    let staged_id = format!("rec_{}", uuid::Uuid::new_v4().simple());
    let expected_items = stage_memory.then(|| {
        vec![
            DiscardedMemoryItem {
                kind: DiscardedMemoryKind::Note,
                title: "Short overlap check".into(),
            },
            DiscardedMemoryItem {
                kind: DiscardedMemoryKind::Principle,
                title: "Check cash flow in both years".into(),
            },
        ]
    });
    if stage_memory {
        *capture.staged_research.lock().await =
            Some(crate::commands::tool_executor::StagedResearch {
                thesis_pair: None,
                observations: String::new(),
                interpretation: String::new(),
                assumptions: String::new(),
                evaluation: String::new(),
                journal: None,
                working_note: Some(crate::knowledge::WorkingNoteDraft {
                    title: "Short overlap check".into(),
                    markdown: "The body is never copied into the skip receipt.".into(),
                    change_summary: "An overlap check".into(),
                    expected_research_id: None,
                }),
                source_reads: vec![],
                world_reads: vec![],
                lens_reads: vec![],
                previous_note_ids: vec![],
            });
        capture.staged_principles.lock().await.push(
            crate::commands::tool_executor::StagedPrinciple {
                record_id: staged_id.clone(),
                expected_revision_id: (expected_skip == Some(MemorySaveSkipReason::Failed))
                    .then(|| format!("rev_{}", uuid::Uuid::new_v4().simple())),
                input: crate::knowledge::PrincipleInput {
                    statement: "Check cash flow in both years".into(),
                    why: None,
                    example: None,
                    origin: "settings".into(),
                    origin_chat_id: None,
                    origin_message_id: None,
                },
            },
        );
    }
    let data = ChartDataset::new(
        "comparison-dataset".into(),
        vec![
            ChartColumn {
                id: "year".into(),
                label: "Year".into(),
                kind: ChartColumnKind::String,
            },
            ChartColumn {
                id: "services".into(),
                label: "Services".into(),
                kind: ChartColumnKind::Number,
            },
            ChartColumn {
                id: "products".into(),
                label: "Products".into(),
                kind: ChartColumnKind::Number,
            },
        ],
        vec![
            vec![json!("2022"), json!(78_129), json!(316_199)],
            vec![json!("2024"), json!(96_169), json!(294_866)],
        ],
        ChartDatasetLineage::AgentAuthored {
            upstream_receipts: vec![],
        },
    )
    .unwrap();
    let chart: ChartDocument = serde_json::from_value(json!({
        "dataset_id": data.id, "dataset_digest": data.digest,
        "view": {"kind":"bar", "x":"year", "y":["services","products"], "stack":false, "semantic_types":{}},
        "studies": [], "drawings": []
    })).unwrap();
    let artifact_at = now_ms();
    capture.artifacts.lock().await.push(ArtifactCommit {
        artifact: ChatArtifact {
            id: "comparison-chart".into(),
            chat_session_id: chat.id.clone(),
            kind: ChatArtifactKind::Chart,
            title: "Annual comparison".into(),
            current_revision: 1,
            created_at_ms: artifact_at,
            updated_at_ms: artifact_at,
        },
        revision: ChatArtifactRevision::new(
            "comparison-chart".into(),
            1,
            ChatArtifactPayload::Chart {
                schema: "loupist-chart/1".into(),
                chart: Box::new(chart),
            },
            "settled-reply".into(),
            artifact_at,
        )
        .unwrap(),
        datasets: vec![data],
    });
    let title = String::new();
    let mut turn = Box::pin(tokio::time::timeout(
        Duration::from_secs(660),
        completion::commit_completed_turn(completion::CompletedTurn {
            reply: &reply,
            task_loupist_id: &earlier.id,
            execution_model: &model,
            capture: &capture,
            runtime_settled: true,
            save_started_at_ms: now_ms(),
            progress: &mut progress,
            failure: &mut failure,
            content: "The chart compares both years.".into(),
            sealed_cache: None,
            task_pins: &[],
            task_update_memory: true,
            should_generate_title: false,
            fallback_title: &title,
            chat_started_at: Instant::now(),
            setup_ms: 0,
            first_text_ms: None,
            answer_text_ms: None,
            generation_ms: None,
            resumed: false,
            token_usage: AssistantTokenUsage::default(),
        }),
    ));
    tokio::select! {
        _ = pending.notified() => {}
        _ = &mut turn => panic!("save finished before the older reader was drained"),
    }
    let checkpoint = state.store.get_chat(&chat.id).unwrap().unwrap();
    let kept = checkpoint.messages.last().unwrap();
    assert_eq!(
        kept.memory_update.as_ref().unwrap().skip_reason,
        Some(MemorySaveSkipReason::Interrupted)
    );
    assert_eq!(
        kept.memory_update.as_ref().unwrap().discarded_memory_items,
        expected_items
    );
    let pending_activity = kept
        .progress
        .as_ref()
        .unwrap()
        .save_activity
        .as_ref()
        .unwrap();
    assert_eq!(
        pending_activity.blocking_chat_id.as_deref(),
        Some("older-chat")
    );
    assert_eq!(pending_activity.finished_at_ms, None);
    assert_eq!(state.store.list_chat_artifacts(&chat.id).unwrap().len(), 1);
    let (control, _) = chat_control_channel();
    let later = state
        .register_chat_run(
            "later".into(),
            earlier.id.clone(),
            "later-chat".into(),
            control,
        )
        .unwrap();
    drop(later);
    let mut older = Some(older);
    match expected_skip {
        Some(MemorySaveSkipReason::User) => {
            state.cancel_run("settled", RunKind::Chat).await.unwrap();
        }
        Some(MemorySaveSkipReason::PolicyChanged) => {
            use crate::knowledge::memory_reset;
            let blobs =
                crate::commands::knowledge::knowledge_blobs_for_loupist(&state, &earlier.id)
                    .unwrap();
            let mut connection = state.store.lock_loupist(&earlier.id).unwrap();
            let tx = connection.transaction().unwrap();
            let plan = memory_reset::preview(&tx, &earlier.id).unwrap();
            memory_reset::reset(&tx, &blobs, &earlier.id, &plan, now_ms()).unwrap();
            tx.commit().unwrap();
            drop(older.take());
        }
        Some(MemorySaveSkipReason::Failed) | None => drop(older.take()),
        Some(MemorySaveSkipReason::Deadline | MemorySaveSkipReason::Interrupted) => {}
    }
    if expected_skip == Some(MemorySaveSkipReason::Interrupted) {
        drop(turn);
    } else {
        assert!(
            turn.await.is_ok(),
            "finalization is waiting on an older reader"
        );
    }
    let persisted = state.store.get_chat(&chat.id).unwrap().unwrap();
    let answer = persisted.messages.last().unwrap();
    assert_eq!(answer.content, "The chart compares both years.");
    let expected_items = expected_items.filter(|_| expected_skip.is_some());
    {
        let mut connection = state.store.lock_loupist(&earlier.id).unwrap();
        let tx = connection.transaction().unwrap();
        assert_eq!(
            crate::knowledge::principles::principle_read(&tx, &earlier.id, &staged_id).is_err(),
            expected_skip.is_some()
        );
    }
    assert_eq!(
        answer.memory_update.as_ref().unwrap().skip_reason,
        expected_skip,
    );
    assert_eq!(
        answer
            .memory_update
            .as_ref()
            .unwrap()
            .discarded_memory_items,
        expected_items
    );
    let dto = crate::commands::types::chat_dto(&persisted).unwrap();
    assert_eq!(
        dto.messages
            .last()
            .unwrap()
            .memory_update
            .as_ref()
            .unwrap()
            .discarded_memory_items,
        expected_items
    );
    let activity = answer
        .progress
        .as_ref()
        .unwrap()
        .save_activity
        .as_ref()
        .unwrap();
    assert_eq!(
        activity.finished_at_ms.is_some(),
        expected_skip != Some(MemorySaveSkipReason::Interrupted)
    );
    if expected_skip == Some(MemorySaveSkipReason::Deadline) {
        assert_eq!(activity.waited_ms, 600_000);
    }
    assert_eq!(activity.blocking_chat_id.as_deref(), Some("older-chat"));
    assert_eq!(events.lock().unwrap().iter().any(|event| matches!(
        event, ChatStreamEvent::Completed { memory_save_skip: reason, save_activity: Some(live), discarded_memory_items, .. } if live == activity && *reason == expected_skip && discarded_memory_items == &expected_items
    )), expected_skip != Some(MemorySaveSkipReason::Interrupted));
    drop(settled);
    assert_eq!(
        state.run_coordinator.active_count(),
        usize::from(older.is_some())
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
        user_provided: false,
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
        watch: Default::default(),
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
        trigger: None,
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
        let mut chat = crate::commands::tests::chat(id, "loupist", 1);
        let mut request = crate::test_fixtures::chat_message("request", ChatRole::User, "Question");
        request.as_of = Some("2026-03-03".into());
        let mut steer = crate::test_fixtures::chat_message("steer", ChatRole::User, "Keep going");
        steer.steer = true;
        chat.messages = vec![request, steer];
        store.create_chat(&chat).unwrap();
        // A closed renderer must not discard what the model already acknowledged.
        let sink = ChatEventSink::new(|_| Err(()));
        sink.set_replaced_model("Gone model");
        sink.set_failure_kind(ChatFailureKind::MemoryCommitFailed);
        sink.tool_refusals.lock().unwrap().push(refusal.clone());
        for _ in 0..2 {
            let _ = sink.send(ChatStreamEvent::Evidence {
                run_id: "run-terminal".into(),
                evidence: Box::new(evidence.clone().into()),
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
        let chat = store.get_chat(id).unwrap().unwrap();
        let message = chat.messages.last().unwrap();
        assert_eq!(message.status, status);
        assert_eq!(message.failure_kind, failure_kind);
        assert_eq!(message.evidence_refs, vec![evidence.clone()]);
        assert_eq!(message.schedule_receipts, vec![receipt.clone()]);
        assert_eq!(message.tool_refusals, vec![refusal.clone()]);
        assert_eq!(message.replaced_model.as_deref(), Some("Gone model"));
        assert_eq!(message.as_of.as_deref(), Some("2026-03-03"));
        assert!(message.memory_update.is_none());
        let trace = message.tool_trace.as_ref().unwrap();
        assert_eq!(trace.entries[0].name, "capability_load");
        assert_eq!(trace.entries[1].outcome, ToolTraceOutcome::Error);
    }
}

#[test]
fn scheduled_provider_failures_pause_from_the_real_terminal_sink_kind() {
    use crate::scheduled_tasks::types::{RunStatus, Schedule, TaskDraft, TaskStatus, TaskTarget};
    for (kind, paused) in [
        (ChatFailureKind::NeedsReconnect, true),
        (ChatFailureKind::ModelGone, true),
        (ChatFailureKind::RateLimited, false),
        (ChatFailureKind::Offline, false),
        (ChatFailureKind::ProviderUnavailable, false),
        (ChatFailureKind::ProviderError, false),
    ] {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let profile = crate::commands::tests::profile("scheduled-kind", workspace.path(), 1);
        crate::commands::tests::seed_profile(&state.store, &profile);
        let task = state
            .store
            .scheduled_task_create(
                &profile.id,
                TaskDraft {
                    title: "Provider test".into(),
                    prompt: "Read the local report.".into(),
                    schedule: Schedule::Interval { every_minutes: 5 },
                    target: TaskTarget::NewChat,
                    model_profile_id: "fixture".into(),
                    thinking_level: "off".into(),
                    run_options: Default::default(),
                    use_memory: false,
                    update_memory: false,
                    status: TaskStatus::Active,
                },
                1_000,
            )
            .unwrap();
        let (claim, ()) = state
            .store
            .scheduled_task_claim_with(&profile.id, &task.id, 2_000, true, |_| Ok(()))
            .unwrap()
            .unwrap();
        let scheduled = state.scheduled.chat_event_sink(&state, &claim.run);
        let sink = ChatEventSink::new(move |event| {
            if let ChatStreamEvent::Error {
                message,
                failure_kind,
                ..
            } = &event
            {
                assert_eq!(message, FAILED_CHAT_CONTENT);
                assert_eq!(*failure_kind, kind);
            }
            scheduled.send(event)
        });
        sink.set_failure_kind(kind);
        persist_and_emit_failed_chat(
            &state.store,
            &sink,
            &claim.run.run_id,
            claim.run.thread_id.as_deref().unwrap(),
            "provider-failed",
            None,
            execution_model(),
            crate::agent_harness::snapshot("chat", &[], &[]).unwrap(),
            None,
        )
        .unwrap();
        let run = state
            .store
            .scheduled_task_run_get(&profile.id, &claim.run.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(run.status, RunStatus::Failed);
        let saved = state
            .store
            .scheduled_task_get(&profile.id, &task.id)
            .unwrap()
            .unwrap();
        assert_eq!(saved.status == TaskStatus::Paused, paused, "{kind:?}");
    }
}

#[test]
fn scheduled_abort_sink_distinguishes_a_user_stop_from_app_quit_without_counting_either() {
    use crate::scheduled_tasks::types::{RunStatus, Schedule, TaskDraft, TaskStatus, TaskTarget};
    for quitting in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let profile = crate::commands::tests::profile("scheduled-stop", workspace.path(), 1);
        crate::commands::tests::seed_profile(&state.store, &profile);
        let task = state
            .store
            .scheduled_task_create(
                &profile.id,
                TaskDraft {
                    title: "Stop test".into(),
                    prompt: "Read the local report.".into(),
                    schedule: Schedule::Interval { every_minutes: 5 },
                    target: TaskTarget::NewChat,
                    model_profile_id: "fixture".into(),
                    thinking_level: "off".into(),
                    run_options: Default::default(),
                    use_memory: false,
                    update_memory: false,
                    status: TaskStatus::Active,
                },
                1_000,
            )
            .unwrap();
        for at in [2_000, 3_000, 4_000] {
            let (claim, ()) = state
                .store
                .scheduled_task_claim_with(&profile.id, &task.id, at, true, |_| Ok(()))
                .unwrap()
                .unwrap();
            if quitting {
                state.scheduled.stop();
            }
            state
                .scheduled
                .chat_event_sink(&state, &claim.run)
                .send(ChatStreamEvent::Aborted {
                    run_id: claim.run.run_id.clone(),
                })
                .unwrap();
            let saved = state
                .store
                .scheduled_task_get(&profile.id, &task.id)
                .unwrap()
                .unwrap();
            assert_eq!(saved.status, TaskStatus::Active);
            let run = state
                .store
                .scheduled_task_run_get(&profile.id, &claim.run.run_id)
                .unwrap()
                .unwrap();
            assert_eq!(run.status, RunStatus::Interrupted);
            assert_eq!(
                run.error.as_deref(),
                Some(if quitting {
                    crate::scheduled_tasks::APP_QUIT
                } else {
                    crate::scheduled_tasks::USER_STOPPED
                })
            );
        }
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn replacing_an_acknowledged_setup_keeps_the_reply_and_skips_its_orphaned_journal() {
    use crate::{
        broker::{ToolExecutor as _, ToolMethod, ToolPolicy},
        commands::{
            tests::{
                chat_behavior::{install_chat_rpc_fixture, send_request},
                initialized_workspace, profile, seed_profile,
            },
            tool_executor::AppToolExecutor,
        },
    };
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    initialized_workspace(&workspace, "orphaned-thesis");
    let mut state = AppState::for_test(root.path().join("app"));
    install_chat_rpc_fixture(
        &mut state,
        root.path(),
        include_str!("fixtures/message_snapshot_pi.py"),
    );
    let profile = profile("orphaned-thesis", &workspace, 1);
    seed_profile(&state.store, &profile);
    let chat = crate::chat::home::ensure_home_thread(&state, &profile.id).unwrap();
    let mut request = send_request("orphan", &profile.id, &chat.id, "uncalled snapshot");
    request.use_memory = true;
    request.update_memory = true;
    let admitted = match setup::admit_chat_turn(
        request,
        ChatEventSink::new(|_| Ok(())),
        &state,
        None,
    )
    .unwrap()
    {
        ControlFlow::Continue(turn) => turn,
        ControlFlow::Break(_) => panic!("turn was not admitted"),
    };
    let mut launched = match launch::launch_chat_turn(admitted, &state).await.unwrap() {
        ControlFlow::Continue(turn) => turn,
        ControlFlow::Break(_) => panic!("fixture did not launch"),
    };
    let executor = AppToolExecutor {
        state: state.clone(),
        capture: launched.capture.clone(),
        loupist_id: profile.id.clone(),
        capability_ids: BTreeSet::new(),
    };
    let policy = ToolPolicy {
        loupist_id: profile.id.clone(),
        session_id: chat.id.clone(),
        use_memory: true,
        propose_memory_updates: true,
        memory_proposal_budget: 4,
        as_of: None,
        display_currency: None,
    };
    // The broker has its own one-setup limit. Exercise replacement at the
    // host acknowledgement boundary, then run the real reply commit.
    for (method, params) in [
        (
            ToolMethod::LoupistSetup,
            json!({"follow":[{"name":"NEWT"}]}),
        ),
        (
            ToolMethod::JournalSave,
            json!({"judgment":"Keep the capacity thesis", "thesis":{"asset":"NEWT","role":"reason"}}),
        ),
        (
            ToolMethod::PrincipleSave,
            json!({"statement":"Check cash flow before profit"}),
        ),
        (
            ToolMethod::LoupistSetup,
            json!({"follow":[{"name":"STAY"}]}),
        ),
    ] {
        executor.execute(&policy, method, params).await.unwrap();
    }
    std::fs::write(root.path().join("snapshot-read-ready"), "ready").unwrap();
    let streamed = stream::stream_chat_turn(&mut launched).await;
    finalize::finalize_chat_turn(launched, streamed).await;

    let chat = state.store.get_chat(&chat.id).unwrap().unwrap();
    let reply = chat.messages.last().unwrap();
    assert_eq!(reply.status, ChatMessageStatus::Complete, "{reply:?}");
    assert_eq!(reply.content, "No snapshot figures.");
    let receipt = reply.memory_update.as_ref().unwrap();
    assert_eq!(receipt.skipped_kinds.len(), 1);
    assert_eq!(
        receipt.skipped_kinds[0].name,
        crate::domain::SKIPPED_JOURNAL_SAVE
    );
    assert_eq!(receipt.setup.len(), 1);
    assert!(receipt
        .changes
        .iter()
        .any(|change| change.kind == "principle"));
    let follows = state.store.assets_list(&profile.id, true).unwrap();
    assert_eq!(follows.len(), 1);
    assert_eq!(follows[0].name, "STAY");
    crate::commands::assets::with_assets(&state, &profile.id, false, |tx| {
        assert!(crate::knowledge::journal::list(tx, &profile.id)?.is_empty());
        assert!(crate::knowledge::assets::list_pages(tx, &profile.id)?
            .iter()
            .all(|page| page.asset_id == follows[0].asset_id));
        Ok(())
    })
    .unwrap();
    let db = state.store.lock().unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM assets", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn saved_key_permission_is_in_run_context_and_scheduled_delivery_without_a_secret_read() {
    use crate::{
        commands::tests::{profile, seed_profile},
        finance_credentials::{self, test_store},
        scheduled_tasks::types::{
            Schedule, TaskDraft, TaskStatus, TaskTarget, SAVED_KEY_PERMISSION_SKIP,
        },
    };
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    let workspace = temporary.path().join("workspace");
    seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
    let entry = crate::finance_data::openfigi::SOURCE_ID;
    let environment = &state.credential_environment;
    finance_credentials::save_lookup_key(environment, "test-legacy-key").unwrap();
    test_store::restart(environment, entry);
    state
        .store
        .set_connector_credential_status(entry, r#""PresentUnread""#)
        .unwrap();
    let reads = test_store::secret_reads(environment, entry);

    let authority = capture_chat_connector_authority(&state, "loupist-a").unwrap();
    assert!(!authority.capability_ids.iter().any(|id| id == entry));
    assert_eq!(
        authority.saved_keys_needing_permission,
        std::collections::BTreeSet::from([entry.to_owned()])
    );
    let mut runtime =
        agent_harness::AgentRuntimeProfile::new("chat", false, false, &authority.capability_ids)
            .unwrap();
    runtime
        .mark_saved_keys_needing_permission(&authority.saved_keys_needing_permission)
        .unwrap();
    let connection = runtime
        .unavailable_connections
        .iter()
        .find(|item| item.id == entry)
        .unwrap();
    assert_eq!(
        serde_json::to_value(connection).unwrap()["reason"],
        "its saved key needs the user's permission in macOS"
    );

    let task = state
        .store
        .scheduled_task_create(
            "loupist-a",
            TaskDraft {
                title: "Daily research".into(),
                prompt: "Summarize company updates.".into(),
                schedule: Schedule::Interval { every_minutes: 5 },
                target: TaskTarget::NewChat,
                model_profile_id: "fixture/model".into(),
                thinking_level: "off".into(),
                run_options: Default::default(),
                use_memory: false,
                update_memory: false,
                status: TaskStatus::Active,
            },
            100,
        )
        .unwrap();
    let run = state
        .store
        .scheduled_task_claim_with("loupist-a", &task.id, 200, true, |_| Ok(()))
        .unwrap()
        .unwrap()
        .0
        .run;
    let sink = ChatEventSink::new(|_| Ok(()));
    setup::record_saved_key_skips(&state, "loupist-a", Some(&run.run_id), &runtime, &sink).unwrap();
    let run = state
        .store
        .scheduled_task_run_get("loupist-a", &run.run_id)
        .unwrap()
        .unwrap();
    assert_eq!(run.skipped_tools.len(), 1);
    assert_eq!(run.skipped_tools[0].reason, SAVED_KEY_PERMISSION_SKIP);
    assert_eq!(run.skipped_tools[0].name, connection.name);
    let delivery = crate::chat::home::delivery_message(&state, &run)
        .unwrap()
        .delivery
        .unwrap();
    assert_eq!(
        delivery.skipped_saved_keys,
        Some(vec![connection.name.clone()])
    );

    setup::record_saved_key_skips(&state, "loupist-a", None, &runtime, &sink).unwrap();
    assert_eq!(
        sink.tool_refusals()[0].reason,
        crate::domain::ChatToolRefusalReason::SavedKeyPermission
    );
    assert_eq!(test_store::secret_reads(environment, entry), reads);
}

mod message_saves;
mod pending_memory;
mod price_reads;
