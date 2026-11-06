//! Research hand-off behavior at its boundaries: the tool executor refuses
//! `research_open` outside the Bot; a Bot turn commits one hand-off and one
//! session, the host runs that session unattended once and the Bot shows the
//! result; a repeated key, a failed Bot reply and a crash before launch each
//! converge to one row, one chat and one run.
use std::collections::BTreeSet;

use serde_json::json;

use super::*;
use crate::{
    broker::{ToolExecutor as _, ToolMethod, ToolPolicy},
    chat::{
        handoff::{self, HandoffStatus},
        home::{ensure_home_thread, home_thread_id},
        surface::ChatSurface,
    },
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
    capture.schedule_context = Some(ScheduleContext {
        model_profile_id: "fixture".into(),
        thinking_level: "medium".into(),
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
    let result = bot
        .execute(&policy(), ToolMethod::ResearchOpen, arguments())
        .await
        .unwrap();
    assert_eq!(result["staged"], true);
    assert_eq!(result["title"], "Hynix HBM supply");
    let staged = bot.capture.staged_handoff.lock().await.clone().unwrap();
    assert_eq!(staged.request.request_key, "hbm-2027");
    assert_eq!(staged.run.model_profile_id, "fixture");
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

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
mod turns {
    use super::*;
    use crate::commands::{
        chat_runtime::{send_chat, ChatEventSink},
        tests::chat_behavior::{drain, install_chat_rpc_fixture, send_request},
    };

    struct Fixture {
        temporary: tempfile::TempDir,
        state: AppState,
        loupist_id: String,
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
            send_chat(
                send_request(
                    run_id,
                    &self.loupist_id,
                    &home_thread_id(&self.loupist_id),
                    "Who else can supply HBM4?",
                ),
                ChatEventSink::new(|_| Ok(())),
                &self.state,
                None,
            )
            .await
            .unwrap();
            drain(&self.state).await;
        }

        /// The Bot's cards for hand-offs, once every one shows a terminal status.
        /// The card changes after the run's lease drops, a little after its row.
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
        fixture.bot_turn("bot-1").await;
        let rows = fixture.settled_handoffs().await;
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.status, HandoffStatus::Done, "{:?}", row.error);
        assert_eq!(row.request_key, "hbm-2027");
        assert!(row.run_id.is_some() && row.message_id.is_some());

        let session = fixture.state.store.get_chat(&row.chat_id).unwrap().unwrap();
        assert_eq!(session.title, "Hynix HBM supply");
        assert_eq!(session.messages[0].role, ChatRole::User);
        assert_eq!(
            session.messages[0].content,
            "Who else can supply HBM4 in 2027?\n\nCheck capacity announcements.\n\nRead these first:\n- lens_read rec_a"
        );
        let reply = session.messages.last().unwrap();
        assert_eq!(reply.status, ChatMessageStatus::Complete);
        assert_eq!(Some(&reply.id), row.message_id.as_ref());

        // The Bot staged the hand-off; the delegated session ran under the
        // unattended policy, which refuses research_open before anything else.
        let calls = fixture.calls();
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert_eq!(calls[0]["result"]["ok"], true);
        assert_eq!(calls[0]["result"]["result"]["staged"], true);
        assert_eq!(calls[1]["result"]["ok"], false);
        assert!(calls[1]["result"]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unattended"));
        assert!(calls[1]["prompt"]
            .as_str()
            .unwrap()
            .contains("Who else can supply HBM4 in 2027?"));

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

        // The same request key again creates no second session and no second run.
        let chats_before = fixture.chats().len();
        fixture.bot_turn("bot-2").await;
        let rows = fixture.settled_handoffs().await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].run_id, row.run_id);
        assert_eq!(fixture.chats().len(), chats_before);
        assert_eq!(fixture.calls().len(), 3);
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
        // The bridge and the tool commit through the same function.
        let committed =
            handoff::commit_standalone(&fixture.state, &fixture.loupist_id, &staged).unwrap();
        let again =
            handoff::commit_standalone(&fixture.state, &fixture.loupist_id, &staged).unwrap();
        assert_eq!(committed, again);
        assert_eq!(committed.status, HandoffStatus::Created);
        assert_eq!(fixture.chats().len(), 2);
        // The app stopped before the launch. Opening the Loupist runs the
        // delivery pass, and another pass can follow at once.
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
