use std::collections::BTreeSet;

use serde_json::json;

use super::*;
use crate::{
    broker::{ToolExecutor as _, ToolMethod, ToolPolicy},
    chat::surface::ChatSurface,
    commands::tool_executor::{schedules::ScheduleContext, AppToolExecutor, ToolCapture},
};

fn executor(temporary: &tempfile::TempDir, surface: Option<ChatSurface>) -> AppToolExecutor {
    let state = AppState::for_test(temporary.path().join("app"));
    let workspace = temporary.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
    state
        .store
        .create_chat(&chat("home-loupist-a", "loupist-a", 1))
        .unwrap();
    let mut capture = ToolCapture::for_chat("message-a".into());
    capture.bound_chat_id = Some("home-loupist-a".into());
    capture.surface = surface;
    let model = state.model_catalog().unwrap().resolve("fixture").unwrap();
    capture.schedule_context = Some(ScheduleContext {
        research_model: Some(crate::providers::catalog::ExecutionModelLock::from_model(
            &model,
            "low",
            &Default::default(),
        )),
        model_profile_id: String::new(),
        thinking_level: String::new(),
        run_options: Default::default(),
        use_memory: true,
        update_memory: false,
    });
    AppToolExecutor {
        state,
        capture: Arc::new(capture),
        loupist_id: "loupist-a".into(),
        capability_ids: BTreeSet::new(),
    }
}

fn policy() -> ToolPolicy {
    ToolPolicy {
        loupist_id: "loupist-a".into(),
        session_id: "home-loupist-a".into(),
        use_memory: true,
        propose_memory_updates: false,
        memory_proposal_budget: 0,
        as_of: None,
        display_currency: None,
    }
}

fn arguments() -> serde_json::Value {
    json!({
        "title": "Hynix HBM supply",
        "question": "Who else can supply HBM4 in 2027?",
        "brief": "Check capacity announcements.",
        "pins": [{"lens": "rec_a"}, {"world": "node_b"}],
        "requestKey": "hbm-2027"
    })
}

#[tokio::test]
async fn research_open_is_staged_in_the_bot_and_refused_elsewhere_by_name() {
    let temporary = tempfile::tempdir().unwrap();
    let bot = executor(&temporary, Some(ChatSurface::Bot));
    let directory = crate::commands::loupist_config_directory(&bot.state, "loupist-a").unwrap();
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("config.toml"),
        "[chat]\nthinking_level = \"medium\"\n",
    )
    .unwrap();
    let result = bot
        .execute(&policy(), ToolMethod::ResearchOpen, arguments())
        .await
        .unwrap();
    assert_eq!(result["staged"], true);
    assert_eq!(result["title"], "Hynix HBM supply");
    let staged = bot.capture.staged_handoff.lock().await.clone().unwrap();
    assert_eq!(staged.request.request_key, "hbm-2027");
    assert_eq!(staged.run.model_profile_id, "fixture");
    assert_eq!(
        staged.run.thinking_level, "low",
        "research uses the admitted source level"
    );
    assert!(staged.run.use_memory && !staged.run.update_memory);

    let malformed = bot
        .execute(
            &policy(),
            ToolMethod::ResearchOpen,
            json!({"title": "t", "question": "q", "requestKey": "has space"}),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(malformed, crate::broker::BrokerError::InvalidArguments { ref field, .. } if field == "requestKey"),
        "{malformed}"
    );

    for (surface, reason) in [
        (Some(ChatSurface::Session), "a session turn"),
        (Some(ChatSurface::Scheduled), "a scheduled turn"),
        (
            Some(ChatSurface::Delegated {
                handoff_id: "handoff-a".into(),
            }),
            "a delegated turn",
        ),
        (None, "a host turn"),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let other = executor(&temporary, surface);
        let error = other
            .execute(&policy(), ToolMethod::ResearchOpen, arguments())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(reason), "{error}");
        assert!(other.capture.staged_handoff.lock().await.is_none());
    }
}

#[tokio::test]
async fn a_second_research_open_in_one_turn_is_refused_and_the_first_stays_staged() {
    let temporary = tempfile::tempdir().unwrap();
    let bot = executor(&temporary, Some(ChatSurface::Bot));
    bot.execute(&policy(), ToolMethod::ResearchOpen, arguments())
        .await
        .unwrap();
    let mut second = arguments();
    second["title"] = json!("DRAM pricing");
    second["requestKey"] = json!("dram-2027");
    let error = bot
        .execute(&policy(), ToolMethod::ResearchOpen, second)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("one research session per reply"), "{error}");
    let staged = bot.capture.staged_handoff.lock().await.clone().unwrap();
    assert_eq!(staged.request.request_key, "hbm-2027");
}

#[tokio::test]
async fn research_open_reserves_one_delivery_and_only_an_ack_commits_it() {
    let temporary = tempfile::tempdir().unwrap();
    let bot = executor(&temporary, Some(ChatSurface::Bot));
    let policy = policy();
    let (first, second) = tokio::join!(
        bot.execute_for_delivery(&policy, ToolMethod::ResearchOpen, arguments(), "first"),
        bot.execute_for_delivery(&policy, ToolMethod::ResearchOpen, arguments(), "second"),
    );
    assert_ne!(first.is_ok(), second.is_ok());
    let winner = if first.is_ok() { "first" } else { "second" };
    bot.discard_delivery(&policy, winner).await;
    bot.commit_delivery(&policy, winner).await.unwrap();
    assert!(bot.capture.staged_handoff.lock().await.is_none());
    bot.execute_for_delivery(&policy, ToolMethod::ResearchOpen, arguments(), "retry")
        .await
        .unwrap();
    bot.commit_delivery(&policy, "retry").await.unwrap();
    let accepted = bot.capture.staged_handoff.lock().await.clone().unwrap();
    bot.commit_delivery(&policy, "retry").await.unwrap();
    assert_eq!(
        bot.capture.staged_handoff.lock().await.as_ref(),
        Some(&accepted)
    );
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
mod turns {
    use super::*;
    use crate::chat::{
        handoff::{self, HandoffStatus},
        home::{ensure_home_thread, home_thread_id},
    };
    use crate::commands::{
        chat_runtime::{send_chat, ChatEventSink},
        tests::chat_behavior::{drain, install_chat_rpc_fixture, send_request},
    };

    struct Fixture {
        temporary: tempfile::TempDir,
        state: AppState,
        loupist_id: String,
        completions: std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    }

    fn fixture() -> Fixture {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        initialized_workspace(&workspace, "research-handoff");
        let mut state = AppState::for_test(temporary.path().join("app"));
        install_chat_rpc_fixture(
            &mut state,
            temporary.path(),
            include_str!("fixtures/research_open_pi.py"),
        );
        let profile = profile("loupist-research", &workspace, 1);
        seed_profile(state.store.as_ref(), &profile);
        ensure_home_thread(&state, &profile.id).unwrap();
        Fixture {
            temporary,
            state,
            loupist_id: profile.id,
            completions: Default::default(),
        }
    }

    impl Fixture {
        fn calls(&self) -> Vec<serde_json::Value> {
            fs::read_to_string(self.temporary.path().join("research-open-calls.jsonl"))
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }

        fn chats(&self) -> Vec<ChatSession> {
            self.state
                .store
                .list_chats_for_loupist(&self.loupist_id)
                .unwrap()
        }

        async fn bot_turn(&self, run_id: &str) {
            self.bot_turn_with(run_id, "Who else can supply HBM4?", false)
                .await;
        }

        fn options(&self, options: serde_json::Value) {
            fs::write(
                self.temporary.path().join("handoff-options.json"),
                options.to_string(),
            )
            .unwrap();
        }

        async fn bot_turn_with(&self, run_id: &str, prompt: &str, memory: bool) {
            self.send_bot(run_id, prompt, memory).await;
            drain(&self.state).await;
        }

        async fn send_bot(&self, run_id: &str, prompt: &str, memory: bool) {
            let mut request = send_request(
                run_id,
                &self.loupist_id,
                &home_thread_id(&self.loupist_id),
                prompt,
            );
            request.use_memory = memory;
            request.update_memory = memory;
            request.thinking_level = "high".into();
            let completions = self.completions.clone();
            let sink = ChatEventSink::new(move |event| {
                if matches!(
                    event,
                    crate::commands::types::ChatStreamEvent::Completed { .. }
                ) {
                    completions
                        .lock()
                        .unwrap()
                        .push(serde_json::to_value(event).unwrap());
                }
                Ok(())
            });
            send_chat(request, sink, &self.state, None).await.unwrap();
        }

        async fn settled_cards(&self) -> Vec<ChatMessage> {
            tokio::time::timeout(std::time::Duration::from_secs(20), async {
                loop {
                    let home = ensure_home_thread(&self.state, &self.loupist_id).unwrap();
                    let cards = home
                        .messages
                        .iter()
                        .filter(|message| message.handoff.is_some())
                        .cloned()
                        .collect::<Vec<_>>();
                    if !cards.is_empty()
                        && cards.iter().all(|card| {
                            card.handoff.as_ref().unwrap().status != HandoffStatus::Running
                        })
                    {
                        break cards;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
            })
            .await
            .expect("the Bot never showed the finished hand-off")
        }

        async fn settled_handoffs(&self) -> Vec<handoff::Handoff> {
            tokio::time::timeout(std::time::Duration::from_secs(20), async {
                loop {
                    let rows = handoff::list(&self.state, &self.loupist_id).unwrap();
                    if !rows.is_empty()
                        && rows.iter().all(|row| {
                            matches!(
                                row.status,
                                HandoffStatus::Done
                                    | HandoffStatus::Failed
                                    | HandoffStatus::Stopped
                            )
                        })
                    {
                        break rows;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
            })
            .await
            .expect("the hand-off run did not settle")
        }
    }

    #[tokio::test]
    async fn a_bot_turn_opens_one_session_runs_it_unattended_once_and_reports_into_the_bot() {
        let fixture = fixture();
        let request = "Yes, do that.";
        fixture.bot_turn_with("bot-1", request, false).await;
        let rows = fixture.settled_handoffs().await;
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.status, HandoffStatus::Done, "{:?}", row.error);
        assert_eq!(row.request_key, "hbm-2027");
        assert_eq!(
            serde_json::to_value(row).unwrap()["trust"]["exposure"],
            "clean"
        );
        assert!(row.run_id.is_some() && row.message_id.is_some());

        let home = ensure_home_thread(&fixture.state, &fixture.loupist_id).unwrap();
        let announced = home
            .messages
            .iter()
            .find(|message| message.role == ChatRole::Assistant && !message.host_composed)
            .unwrap();
        assert_eq!(
            serde_json::to_value(announced).unwrap()["researchSessionId"],
            row.chat_id
        );
        let snapshot = crate::commands::types::chat_dto(&home).unwrap();
        assert_eq!(
            snapshot
                .messages
                .iter()
                .find(|m| m.id == announced.id)
                .unwrap()
                .research_session_id
                .as_deref(),
            Some(row.chat_id.as_str())
        );
        assert!(home
            .messages
            .iter()
            .all(|m| m.id == announced.id || m.research_session_id.is_none()));
        assert_eq!(
            fixture.completions.lock().unwrap()[0]["research_session_id"],
            row.chat_id
        );
        let session = fixture.state.store.get_chat(&row.chat_id).unwrap().unwrap();
        assert_eq!(session.loupist_id, home.loupist_id);
        assert!(session
            .messages
            .iter()
            .all(|message| message.research_session_id.is_none()));
        assert_eq!(session.title, "Hynix HBM supply");
        assert_eq!(session.messages[0].role, ChatRole::User);
        assert_eq!(session.messages[0].content, request);
        assert_eq!(
            session.messages[0]
                .handoff_note
                .as_ref()
                .unwrap()
                .question
                .as_deref(),
            Some("Who else can supply HBM4 in 2027?")
        );
        let reply = session.messages.last().unwrap();
        assert_eq!(
            reply.execution_model.as_ref().unwrap().thinking_level,
            "high"
        );
        assert_eq!(reply.status, ChatMessageStatus::Complete);
        assert_eq!(Some(&reply.id), row.message_id.as_ref());

        let calls = fixture.calls();
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert_eq!(calls[0]["result"]["ok"], true);
        assert_eq!(calls[0]["result"]["result"]["staged"], true);
        assert_eq!(calls[1]["result"]["ok"], false);
        assert!(calls[1]["result"]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unattended"));
        assert_eq!(current_body(&calls[1]), request);
        assert_eq!(
            envelope(&calls[1])["handoff"]["bot_note"]["question"],
            "Who else can supply HBM4 in 2027?"
        );

        let cards = fixture.settled_cards().await;
        assert_eq!(cards.len(), 1);
        let card = cards[0].handoff.as_ref().unwrap();
        assert_eq!(card.handoff_id, row.id);
        assert_eq!(card.chat_id, row.chat_id);
        assert_eq!(card.title, "Hynix HBM supply");
        assert_eq!(card.status, HandoffStatus::Done);
        assert!(card.learned.is_none());
        assert_eq!(cards[0].content, "HBM4 supply widens in 2027.");
        assert!(cards[0].host_composed);

        let chats_before = fixture.chats().len();
        fixture.bot_turn("bot-2").await;
        let home = ensure_home_thread(&fixture.state, &fixture.loupist_id).unwrap();
        let second_user = home
            .messages
            .iter()
            .position(|message| {
                message.role == ChatRole::User && message.content == "Who else can supply HBM4?"
            })
            .unwrap();
        let second_reply = home.messages[second_user + 1..]
            .iter()
            .find(|message| message.role == ChatRole::Assistant && !message.host_composed)
            .unwrap();
        assert_eq!(second_reply.status, ChatMessageStatus::Complete);
        assert!(second_reply.content.contains("HBM4"));
        let rows = fixture.settled_handoffs().await;
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .all(|row| row.status == HandoffStatus::Done && row.request_key == "hbm-2027"));
        assert!(rows
            .iter()
            .any(|candidate| candidate.id == row.id && candidate.run_id == row.run_id));
        assert_eq!(fixture.chats().len(), chats_before + 1);
        assert_eq!(fixture.calls().len(), 4);
        fixture.state.close_after_turns_for_restart_test().await;
        let reopened = AppState::for_test(fixture.temporary.path().join("app"));
        let home = ensure_home_thread(&reopened, &fixture.loupist_id).unwrap();
        let restored = home.messages.iter().find(|m| m.id == announced.id).unwrap();
        assert_eq!(
            restored.research_session_id.as_deref(),
            Some(row.chat_id.as_str())
        );
    }

    fn envelope(call: &serde_json::Value) -> serde_json::Value {
        let prompt = call["prompt"].as_str().unwrap();
        let (_, after) = prompt.split_once("<turn_envelope>\n").unwrap();
        let (before, _) = after.split_once("\n</turn_envelope>").unwrap();
        serde_json::from_str(before).unwrap()
    }

    fn current_body(call: &serde_json::Value) -> &str {
        let prompt = call["prompt"].as_str().unwrap();
        let (_, after) = prompt.rsplit_once("<current_user_message>\n").unwrap();
        after.strip_suffix("\n</current_user_message>").unwrap()
    }

    #[tokio::test]
    async fn late_text_restricts_the_first_tool_and_the_original_message_survives_cold_rebuild() {
        let fixture = fixture();
        fixture
            .state
            .store
            .save_loupist_capability(&crate::domain::LoupistCapabilityBinding {
                loupist_id: fixture.loupist_id.clone(),
                entry_id: "community.web-research".into(),
                enabled: true,
                granted_permissions: vec!["execute".into()],
                config: Default::default(),
                updated_at_ms: 1,
            })
            .unwrap();
        let dataset = fixture
            .state
            .store
            .dataset_put(
                &fixture.loupist_id,
                "third-party",
                &json!([{"text":"Publisher data"}]),
                json!({"trust":"third_party_text"}),
            )
            .unwrap();
        let knowledge = crate::knowledge::KnowledgeStore::attach(
            fixture.state.store.clone(),
            crate::commands::knowledge::knowledge_blobs(&fixture.state).unwrap(),
        );
        let lens = knowledge
            .transact(|write| {
                write.file_section(
                    &fixture.loupist_id,
                    crate::knowledge::lens_page::PageKind::Method,
                    "Confirmed date recovery",
                    "criteria",
                    "Use confirmed dates.",
                    &crate::knowledge::test_support::user_origin(),
                    10,
                )
            })
            .unwrap();
        fixture.options(
            json!({"probe":true,"pins":[{"lens":lens.record_id},{"world":"gone-node"}],
            "source":{"method":"dataset.query",
            "params":{"dataset_id":dataset.dataset_id,"revision":dataset.revision}}}),
        );
        let request =
            "Alder announced one capacity change. ".repeat(60) + "Use confirmed dates only.";
        fixture.bot_turn_with("bot-text", &request, true).await;
        let rows = fixture.settled_handoffs().await;
        let row = &rows[0];
        assert_eq!(row.status, HandoffStatus::Done, "{:?}", row.error);
        assert_eq!(
            serde_json::to_value(row).unwrap()["trust"]["exposure"],
            "text"
        );
        let calls = fixture.calls();
        assert_eq!(calls[0]["late_read"]["ok"], true, "{calls:?}");
        let delegated = &calls[1];
        assert_eq!(current_body(delegated), request);
        assert_eq!(delegated["thinking"], "high");
        let delegated_envelope = envelope(delegated);
        assert_eq!(
            delegated_envelope["chat_surface"]["hint"],
            crate::chat::surface::ChatSurface::typed_handoff_hint()
        );
        let hint = delegated_envelope["chat_surface"]["hint"].as_str().unwrap();
        assert!(hint.contains("The message is what the user wrote to Bot, unchanged"));
        assert!(hint.contains("The user can open the session while it runs, and a further user message is their guidance for this run."));
        assert!(hint.contains("interface.language is the language of the app's screens"));
        assert!(delegated_envelope["interface"]["language"].is_string());
        let note = delegated_envelope["handoff"].clone();
        assert!(crate::custom_connections::taint::delimited_body(
            note["bot_note"].as_str().unwrap()
        )
        .unwrap()
        .contains("Include an estimate."));
        assert!(note.get("omitted_pins").is_none());
        let selected = &delegated_envelope["memory_protocol"]["selectedReferences"]["items"][0];
        assert_eq!(selected["tool"], "lens_read");
        assert_eq!(selected["read"]["recordId"], lens.record_id);
        assert_eq!(selected["read"]["revisionId"], lens.revision_id);
        assert!(
            delegated["first_outbound"]
                .to_string()
                .contains(crate::custom_connections::taint::WEB_FETCH_SITE_REFUSAL),
            "{delegated:?}"
        );
        assert!(delegated["instruction_edit"]
            .to_string()
            .contains("unattended"));
        assert!(delegated["skill_edit"].to_string().contains("unattended"));
        assert_eq!(delegated["calculation"]["ok"], true, "{delegated:?}");

        let session = fixture.state.store.get_chat(&row.chat_id).unwrap().unwrap();
        let mut cold = session.clone();
        cold.pi_session_cache = None;
        fixture.state.store.replace_chat(&session, &cold).unwrap();
        send_chat(
            send_request(
                "interactive-later",
                &fixture.loupist_id,
                &session.id,
                "Which date is confirmed?",
            ),
            ChatEventSink::new(|_| Ok(())),
            &fixture.state,
            None,
        )
        .await
        .unwrap();
        drain(&fixture.state).await;
        let calls = fixture.calls();
        let interactive = &calls[2];
        assert_eq!(envelope(interactive)["chat_surface"]["surface"], "session");
        assert!(envelope(interactive).get("interface").is_none());
        assert_eq!(
            interactive["interactive_labels"]["ok"], true,
            "{interactive:?}"
        );
        let prompt = interactive["prompt"].as_str().unwrap();
        let (_, history) = prompt.split_once("<conversation_history_jsonl>\n").unwrap();
        let first: serde_json::Value =
            serde_json::from_str(history.lines().next().unwrap()).unwrap();
        assert_eq!(first["content"], request);
        assert_eq!(first["handoff_note"], note);
    }

    #[tokio::test]
    async fn a_late_custom_read_restores_its_origin_and_sticky_save_setting() {
        use crate::custom_connections::{
            config::{ParsedConnection, TransportConfig},
            fixture::{Response, Server},
            policy::ApprovalChoice,
            service,
        };
        let server = Server::start(|_, request| {
            let body = request.json();
            match body["method"].as_str().unwrap() {
                "initialize" => Response::json(json!({"jsonrpc":"2.0","id":body["id"],"result":{
                    "protocolVersion":crate::mcp::MCP_PROTOCOL_VERSION,
                    "serverInfo":{"name":"fixture"},"capabilities":{"tools":{}}}})),
                "notifications/initialized" => Response::status(202),
                "tools/list" => Response::json(json!({"jsonrpc":"2.0","id":body["id"],"result":{
                    "tools":[{"name":"read_company","description":"Read company data",
                        "inputSchema":{"type":"object"},"annotations":{"readOnlyHint":true}}]}})),
                "tools/call" => Response::json(json!({"jsonrpc":"2.0","id":body["id"],"result":{
                    "content":[{"type":"text","text":"Revenue grew."}]}})),
                _ => Response::status(400),
            }
        })
        .await;
        for dont_save in [false, true] {
            let fixture = fixture();
            let manager = service::for_state(&fixture.state).unwrap();
            let connection = {
                let mut service = manager.lock().await;
                let pending = service
                    .add_pending(ParsedConnection {
                        name: "Company research".into(),
                        transport: TransportConfig::Http {
                            url: format!("{}/mcp", server.url),
                        },
                        secrets: Default::default(),
                    })
                    .unwrap();
                service.confirm(&pending.id).await.unwrap();
                service.set_memory(&pending.id, dont_save).unwrap();
                let connection = service
                    .set_enabled(&pending.id, &fixture.loupist_id, true)
                    .unwrap();
                service
                    .approve(
                        &fixture.loupist_id,
                        &connection.id,
                        "read_company",
                        &connection.tools[0].definition_hash,
                        ApprovalChoice::Always,
                    )
                    .await
                    .unwrap();
                connection
            };
            fixture.options(json!({"probe":true,"pins":[],
                "source":{"method":"mcp.call","params":{
                    "server_id":connection.id,"tool_name":"read_company","arguments":{}}}}));
            fixture
                .bot_turn_with("bot-custom", "Check capacity. Use confirmed dates.", true)
                .await;
            let rows = fixture.settled_handoffs().await;
            let row = &rows[0];
            assert_eq!(row.status, HandoffStatus::Done, "{:?}", row.error);
            let stored = serde_json::to_value(row).unwrap();
            assert_eq!(stored["trust"]["connection"]["connectionId"], connection.id);
            assert_eq!(stored["trust"]["exposure"], "custom");
            assert_eq!(stored["trust"]["dont_save"], dont_save);
            assert_eq!(row.run.update_memory, !dont_save);
            let calls = fixture.calls();
            assert_eq!(calls[0]["late_read"]["ok"], true, "{calls:?}");
            let delegated = &calls[1];
            assert!(
                delegated["first_outbound"]
                    .to_string()
                    .contains(crate::custom_connections::taint::OUTBOUND_REFUSAL),
                "{delegated:?}"
            );
            assert_eq!(delegated["memory_write"]["ok"], !dont_save, "{delegated:?}");
            assert_eq!(delegated["calculation"]["ok"], true, "{delegated:?}");
        }
    }

    #[tokio::test]
    async fn a_running_bot_handoff_posts_nothing_streams_into_its_session_and_takes_steer_and_stop()
    {
        use crate::{
            commands::{
                loupist::thread_dtos,
                types::{ChatStreamEvent, ChatWatchRequest, HandoffRunDto},
            },
            runs::coordinator::RunKind,
        };
        use tokio::{
            sync::mpsc,
            time::{sleep, timeout, Duration},
        };

        let fixture = fixture();
        let state = &fixture.state;
        let loupist = &fixture.loupist_id;
        fixture.options(json!({"slow":true,"pins":[]}));
        fixture
            .send_bot("bot-live", "Who else can supply HBM4?", false)
            .await;
        let bot_cards = || {
            ensure_home_thread(state, loupist)
                .unwrap()
                .messages
                .into_iter()
                .filter_map(|message| message.handoff)
                .collect::<Vec<_>>()
        };
        timeout(Duration::from_secs(15), async {
            // The workspace's session thread carries the run; Bot posts nothing for it.
            let (row, running) = loop {
                crate::chat::home::deliver_finished_runs(state, loupist).unwrap();
                let rows = handoff::list(state, loupist).unwrap();
                let session = rows.last().and_then(|row| {
                    thread_dtos(state, loupist, &fixture.chats())
                        .into_iter()
                        .find(|thread| thread.id == row.chat_id && !thread.messages.is_empty())
                        .and_then(|thread| thread.handoff_run)
                });
                if let (Some(row), Some(running)) = (rows.last(), session) {
                    break (row.clone(), running);
                }
                sleep(Duration::from_millis(20)).await;
            };
            assert_eq!(row.status, HandoffStatus::Running);
            let run_id = row.run_id.as_ref().unwrap();
            assert_eq!(running, HandoffRunDto { handoff_id: row.id.clone(), run_id: run_id.clone(), connection_id: None });
            assert!(thread_dtos(state, loupist, &fixture.chats()).iter()
                .all(|thread| thread.id == row.chat_id || thread.handoff_run.is_none()));
            assert!(bot_cards().is_empty(), "Bot posted the session while it ran");

            let attach = |subscription_id: &'static str| {
                let request = ChatWatchRequest {
                    loupist_id: loupist.clone(), thread_id: row.chat_id.clone(), subscription_id: subscription_id.into(),
                };
                async move {
                    let (sender, receiver) = mpsc::unbounded_channel();
                    let snapshot = state.live_handoffs.attach(request.clone(), move |event| sender.send(event).map_err(|_| ()))
                        .await.unwrap().unwrap();
                    (request, snapshot, receiver)
                }
            };
            let (first, snapshot, mut events) = attach("first").await;
            assert_eq!(&snapshot.run_id, run_id);
            assert_eq!(snapshot.thread.id, row.chat_id);
            assert!(!snapshot.thread.messages.is_empty());
            // The joined thread keeps saying what it runs, as the workspace's does.
            assert_eq!(snapshot.thread.handoff_run.as_ref(), Some(&running));
            // Joined before its first step, the session sees that step as it happens.
            loop {
                match events.recv().await.unwrap() {
                    ChatStreamEvent::Progress { progress, .. } if !progress.items.is_empty() => break,
                    ChatStreamEvent::ProgressPatch { patch, .. } if !patch.upsert_items.is_empty() => break,
                    _ => {}
                }
            }
            let (_, _, mut other_events) = attach("other").await;
            state.live_handoffs.detach(&first);
            while events.recv().await.is_some() {}

            let steer = "Use confirmed delivery dates instead.";
            let accepted = state.submit_chat_control(loupist, &row.chat_id, steer.into()).await.unwrap();
            assert!(fixture.calls().iter().any(|call| call["steer"] == steer));
            let chat = state.store.get_chat(&row.chat_id).unwrap().unwrap();
            assert!(chat.messages.iter().any(|message| message.id == accepted.message_id && message.steer && message.content == steer));

            while !matches!(other_events.recv().await.unwrap(), ChatStreamEvent::ProgressPatch { patch, .. } if !patch.upsert_items.is_empty()) {}
            crate::chat::home::deliver_finished_runs(state, loupist).unwrap();
            assert!(bot_cards().is_empty(), "a delivery pass posted the running session");
            let (_, _, mut reattached) = attach("first").await;
            while !matches!(reattached.recv().await.unwrap(), ChatStreamEvent::Progress { progress, .. } if !progress.items.is_empty()) {}
            state.cancel_run(run_id, RunKind::Chat).await.unwrap();
            for receiver in [&mut other_events, &mut reattached] {
                while !matches!(receiver.recv().await.unwrap(), ChatStreamEvent::Aborted { run_id: stopped, .. } if &stopped == run_id) {}
            }
            // Stopping it from its session posts one stopped item into Bot.
            let cards = fixture.settled_cards().await;
            assert_eq!(cards.len(), 1);
            let stopped = cards[0].handoff.as_ref().unwrap();
            assert_eq!(stopped.handoff_id, running.handoff_id);
            assert_eq!(stopped.status, HandoffStatus::Stopped);
            assert_eq!(stopped.stop_reason, None);
            let settled = handoff::list(state, loupist).unwrap().pop().unwrap();
            assert_eq!(cards[0].created_at_ms, settled.finished_at_ms.unwrap());
            assert!(thread_dtos(state, loupist, &fixture.chats()).iter().all(|thread| thread.handoff_run.is_none()));
            let (sender, _) = mpsc::unbounded_channel();
            assert!(state.live_handoffs.attach(first, move |event| sender.send(event).map_err(|_| ())).await.unwrap().is_none());
        }).await.expect("scripted live hand-off did not settle");
    }

    #[tokio::test]
    async fn a_retained_link_claims_once_with_host_names_and_explicit_settings() {
        use crate::commands::handoff::start_link;
        use crate::knowledge::{CreateRecord, GraphNode, KnowledgeStore, RecordKind, SealWorld};

        let fixture = fixture();
        fixture.options(json!({"probe":true}));
        let state = &fixture.state;
        let id = &fixture.loupist_id;
        fs::write(
            state.artifacts.app_data_dir.join("config.toml"),
            "[appearance]\nlanguage = 'ko'\n",
        )
        .unwrap();
        let mut model = state.model_catalog().unwrap().resolve("fixture").unwrap();
        model
            .run_controls
            .push(crate::providers::catalog::ModelRunControl {
                id: "performance".into(),
                label: "Performance".into(),
                default_choice: "standard".into(),
                choices: ["standard", "fast"]
                    .into_iter()
                    .map(|choice| crate::providers::catalog::ModelRunControlChoice {
                        id: choice.into(),
                        label: choice.into(),
                        description: "Fixture option".into(),
                    })
                    .collect(),
            });
        state
            .replace_provider_models("openai", vec![model])
            .unwrap();
        let world = KnowledgeStore::attach(
            state.store.clone(),
            crate::commands::knowledge::knowledge_blobs(state).unwrap(),
        );
        let revision = world
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: id.clone(),
                    kind: RecordKind::World,
                    title: "Fixture Map".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: id.clone(),
                    record_id: record.record_id,
                    expected_revision_id: None,
                    at_ms: 2,
                    nodes: [("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")]
                        .into_iter()
                        .map(|(id, name)| GraphNode {
                            id: id.into(),
                            name: name.into(),
                            aliases: Vec::new(),
                            description: String::new(),
                            kind: "Company".into(),
                            source_role: "entity".into(),
                            properties: Default::default(),
                        })
                        .collect(),
                    claims: Vec::new(),
                    annotations: Vec::new(),
                    dependencies: Vec::new(),
                })
            })
            .unwrap();
        let home = ensure_home_thread(state, id).unwrap();
        let mut with_link = home.clone();
        let mut post =
            crate::test_fixtures::chat_message("link-post", ChatRole::Assistant, "A new link");
        post.host_composed = true;
        // The command reads the scoped connection rather than the card's labels.
        post.link = Some(crate::domain::HomeLink {
            connection_id: "connection-fixture".into(),
            seed_name: "Old card label".into(),
            far_name: "Gamma".into(),
            node_ids: vec!["a".into(), "b".into(), "c".into()],
            path: vec![],
        });
        with_link.messages.push(post);
        state.store.replace_chat(&home, &with_link).unwrap();
        let hop = |name, node, claim, relation, reversed| {
            json!({
                "name":name, "nodeId":node, "claimId":claim, "relation":relation,
                "fromRole":"supplier", "toRole":"customer", "reversed":reversed
            })
        };
        let path = json!({"seedName":"Alpha","hops":[
            hop("Beta","b","c1","SUPPLIES",false), hop("Gamma","c","c2","USES",true)
        ]});
        state.store.lock_loupist(id).unwrap().execute(
            "INSERT INTO knowledge_connections(connection_id,loupist_id,seed_node_id,far_node_id,
                path_json,score,status,created_at_ms,status_at_ms) VALUES (?1,?2,'a','c',?3,1,'shown',1,1)",
            rusqlite::params!["connection-fixture", id, path.to_string()],
        ).unwrap();

        let options = json!({"loupistId":id,"connectionId":"connection-fixture","modelProfileId":"fixture",
            "thinkingLevel":"low","runOptions":{"performance":"fast"},"useMemory":true,"updateMemory":false});
        let send = |options| start_link(serde_json::from_value(options).unwrap(), state);
        for field in ["title", "question", "pins", "trust", "brief", "requestKey"] {
            let mut malformed = options.clone();
            malformed[field] = json!("untrusted");
            assert!(
                serde_json::from_value::<crate::commands::handoff::ResearchHandoffRequest>(
                    malformed
                )
                .is_err()
            );
        }
        let mut bad_level = options.clone();
        bad_level["thinkingLevel"] = json!("not offered");
        assert_eq!(send(bad_level).unwrap_err().code, "invalid_request");
        let mut unavailable = options.clone();
        unavailable["connectionId"] = json!("gone");
        assert_eq!(send(unavailable).unwrap_err().code, "not_found");
        assert!(handoff::list(state, id).unwrap().is_empty());
        let accepted = send(options.clone()).unwrap();
        assert_eq!(send(options.clone()).unwrap(), accepted);
        let mut conflict = options;
        conflict["updateMemory"] = json!(true);
        assert_eq!(send(conflict).unwrap_err().code, "conflict");
        assert_eq!(fixture.chats().len(), 2);
        let deliver = || crate::chat::home::deliver_finished_runs(state, id).unwrap();
        deliver();
        deliver();
        let rows = fixture.settled_handoffs().await;
        assert_eq!(rows[0].status, HandoffStatus::Done, "{:?}", rows[0].error);
        assert_eq!(rows[0].run.thinking_level, "low");
        assert_eq!(
            rows[0]
                .run
                .run_options
                .get("performance")
                .map(String::as_str),
            Some("fast")
        );
        assert!(rows[0].run.use_memory && !rows[0].run.update_memory);
        let calls = fixture.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["thinking"], "low");
        assert_eq!(
            current_body(&calls[0]),
            "The user chose “Look into it” on this Map link."
        );
        let envelope = envelope(&calls[0]);
        assert_eq!(envelope["interface"]["language"], "ko");
        assert_eq!(
            envelope["chat_surface"],
            ChatSurface::Session.envelope_block()
        );
        assert_eq!(
            envelope["handoff"]["hint"],
            ChatSurface::link_handoff_hint()
        );
        let hint = envelope["handoff"]["hint"].as_str().unwrap();
        assert!(hint.contains("The user can open the session while it runs, and a further user message is their guidance for this run."));
        assert_eq!(calls[0]["interactive_labels"]["ok"], true, "{:?}", calls[0]);
        assert_eq!(calls[0]["question"]["ok"], false, "{:?}", calls[0]);
        let link_tools = calls[0]["tools"].as_array().unwrap();
        assert!(!link_tools.contains(&json!("ask_user")));
        for tool in ["read", "write", "run_python", "interface_labels"] {
            assert!(
                link_tools.contains(&json!(tool)),
                "missing {tool}: {link_tools:?}"
            );
        }
        let link = envelope["handoff"]["map_link"].as_str().unwrap();
        let body = crate::custom_connections::taint::delimited_body(link).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(body).unwrap(),
            json!({
                "seed": "Alpha",
                "far": "Gamma",
                "path": [
                    {"from": "Alpha", "relation": "SUPPLIES", "to": "Beta"},
                    {"from": "Gamma", "relation": "USES", "to": "Beta"},
                ],
            })
        );
        let refs = envelope["memory_protocol"]["selectedReferences"]["items"]
            .as_array()
            .unwrap();
        assert_eq!(refs.len(), 3, "{envelope}");
        for item in refs {
            assert_eq!(item["read"]["revisionId"], revision.revision_id);
        }
        let chat = state.store.get_chat(&rows[0].chat_id).unwrap().unwrap();
        assert_eq!(chat.title, "연결: Alpha ↔ Gamma");
        assert!(chat.messages[0].handoff_action.is_some());
        let projected =
            serde_json::to_value(crate::commands::types::chat_dto(&chat).unwrap()).unwrap();
        let note = &projected["messages"][0]["handoff_note"];
        assert_eq!(
            note["linkPath"],
            serde_json::from_str::<serde_json::Value>(body).unwrap()
        );
        assert!(note.get("brief").is_none() && note.get("question").is_none());
        assert_eq!(
            chat.messages
                .last()
                .unwrap()
                .execution_model
                .as_ref()
                .unwrap()
                .profile_id,
            "fixture"
        );
        assert_eq!(
            chat.messages
                .last()
                .unwrap()
                .execution_model
                .as_ref()
                .unwrap()
                .run_options,
            rows[0].run.run_options
        );
        let bound = state.store.lock_loupist(id).unwrap();
        let view = crate::knowledge::connection_in_loupist(&bound, id, "connection-fixture")
            .unwrap()
            .unwrap();
        assert_eq!(view.status, crate::knowledge::ConnectionStatus::Opened);
        assert!(crate::knowledge::connection_in_loupist(
            &bound,
            "different-loupist",
            "connection-fixture"
        )
        .unwrap()
        .is_none());
        drop(bound);
        let repeat = send(json!({"loupistId":id,"connectionId":"connection-fixture",
            "modelProfileId":"fixture","thinkingLevel":"low","runOptions":{"performance":"fast"},
            "useMemory":false,"updateMemory":false}))
        .unwrap();
        assert_ne!(repeat, accepted);
        assert_eq!(fixture.chats().len(), 3);
    }

    #[tokio::test]
    async fn an_unacknowledged_stage_cannot_commit_with_a_successful_reply() {
        let fixture = fixture();
        fixture.options(json!({"discard":true}));
        fixture.bot_turn("bot-no-ack").await;
        assert_eq!(fixture.calls()[0]["result"]["ok"], true);
        assert!(handoff::list(&fixture.state, &fixture.loupist_id)
            .unwrap()
            .is_empty());
        let home = ensure_home_thread(&fixture.state, &fixture.loupist_id).unwrap();
        assert!(home
            .messages
            .iter()
            .all(|m| m.research_session_id.is_none()));
        assert!(fixture.completions.lock().unwrap()[0]
            .get("research_session_id")
            .is_none());
        assert_eq!(
            home.messages.last().unwrap().status,
            ChatMessageStatus::Complete
        );
    }

    #[tokio::test]
    async fn a_bot_reply_that_does_not_finish_hands_nothing_off() {
        let fixture = fixture();
        fs::write(fixture.temporary.path().join("end-without-answer"), "").unwrap();
        fixture.bot_turn("bot-stopped").await;
        let home = fixture
            .state
            .store
            .get_chat(&home_thread_id(&fixture.loupist_id))
            .unwrap()
            .unwrap();
        assert!(home
            .messages
            .iter()
            .all(|m| m.research_session_id.is_none()));
        assert_ne!(
            home.messages.last().unwrap().status,
            ChatMessageStatus::Complete
        );
        assert_eq!(fixture.calls().len(), 1);
        assert_eq!(fixture.calls()[0]["result"]["ok"], true);
        assert!(handoff::list(&fixture.state, &fixture.loupist_id)
            .unwrap()
            .is_empty());
        assert_eq!(fixture.chats().len(), 1);
    }

    #[tokio::test]
    async fn a_committed_hand_off_left_unsent_is_sent_once_by_the_next_delivery_passes() {
        let fixture = fixture();
        let staged = handoff::StagedHandoff {
            request: handoff::HandoffRequest::parse(&arguments()).unwrap(),
            run: handoff::HandoffRun {
                model_profile_id: "fixture".into(),
                thinking_level: "medium".into(),
                run_options: Default::default(),
                use_memory: false,
                update_memory: false,
            },
        };
        let committed =
            handoff::commit_standalone(&fixture.state, &fixture.loupist_id, &staged).unwrap();
        let again =
            handoff::commit_standalone(&fixture.state, &fixture.loupist_id, &staged).unwrap();
        assert_eq!(committed, again);
        assert_eq!(committed.status, HandoffStatus::Created);
        assert_eq!(fixture.chats().len(), 2);
        let deliver =
            || crate::chat::home::deliver_finished_runs(&fixture.state, &fixture.loupist_id);
        deliver().unwrap();
        deliver().unwrap();
        let rows = fixture.settled_handoffs().await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, HandoffStatus::Done, "{:?}", rows[0].error);
        assert_eq!(fixture.settled_cards().await.len(), 1);
        deliver().unwrap();
        assert_eq!(fixture.calls().len(), 1, "one session run");
        assert_eq!(fixture.chats().len(), 2);
    }
}
