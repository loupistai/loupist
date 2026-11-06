use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::*;
use crate::{
    broker::{BrokerError, ToolExecutor as _, ToolMethod, ToolPolicy},
    chat::surface::ChatSurface,
    commands::tool_executor::{AppToolExecutor, ToolCapture},
    domain::LoupistTemplate,
};

fn executor(
    temporary: &tempfile::TempDir,
    surface: Option<ChatSurface>,
    unattended: bool,
) -> AppToolExecutor {
    let state = AppState::for_test(temporary.path().join("app"));
    let workspace = temporary.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
    state
        .store
        .create_chat(&chat("home-loupist-a", "loupist-a", 1))
        .unwrap();
    let mut capture = ToolCapture::for_chat("message-a".into());
    capture.surface = surface;
    capture.unattended_run_id = unattended.then(|| "run-a".into());
    AppToolExecutor {
        state,
        capture: Arc::new(capture),
        loupist_id: "loupist-a".into(),
        capability_ids: BTreeSet::new(),
    }
}

fn policy(save: bool) -> ToolPolicy {
    ToolPolicy {
        loupist_id: "loupist-a".into(),
        session_id: "home-loupist-a".into(),
        use_memory: save,
        propose_memory_updates: save,
        memory_proposal_budget: 1,
        as_of: None,
    }
}

fn setup() -> Value {
    json!({
        "follow": [
            {"symbol": "NVDA", "market": "NASDAQ", "name": "Nvidia"},
            {"symbol": "000660", "name": "SK hynix"}
        ],
        "focus": "Semiconductor supply chain",
        "preset": "semiconductors"
    })
}

#[tokio::test]
async fn an_interactive_turn_stages_the_setup_with_unknown_markets_left_unknown() {
    for surface in [ChatSurface::Bot, ChatSurface::Session] {
        let temporary = tempfile::tempdir().unwrap();
        let executor = executor(&temporary, Some(surface), false);
        let result = executor
            .execute(&policy(true), ToolMethod::LoupistSetup, setup())
            .await
            .unwrap();
        assert_eq!(result["status"], "staged");
        let staged = executor.capture.staged_setup.lock().await.clone().unwrap();
        assert_eq!(staged.follow[0].market.as_deref(), Some("NASDAQ"));
        assert_eq!(staged.follow[1].symbol, "000660");
        assert_eq!(staged.follow[1].market, None, "never invented");
        assert_eq!(staged.preset, Some(LoupistTemplate::Semiconductors));
        assert!(
            executor.state.store.watchlist_list().unwrap().is_empty(),
            "nothing applies before the reply commits"
        );
    }
}

#[tokio::test]
async fn scheduled_delegated_and_host_runs_are_refused() {
    for (surface, unattended, said) in [
        (Some(ChatSurface::Scheduled), false, "a scheduled turn"),
        (
            Some(ChatSurface::Delegated {
                handoff_id: "handoff-a".into(),
            }),
            false,
            "a delegated turn",
        ),
        (None, false, "a host turn"),
        (
            Some(ChatSurface::Delegated {
                handoff_id: "handoff-a".into(),
            }),
            true,
            crate::briefings::policy::REFUSAL,
        ),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let executor = executor(&temporary, surface, unattended);
        let error = executor
            .execute(&policy(true), ToolMethod::LoupistSetup, setup())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(said), "{error}");
        assert!(executor.capture.staged_setup.lock().await.is_none());
    }
}

#[tokio::test]
async fn invalid_arguments_and_saving_off_are_refused_by_field() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary, Some(ChatSurface::Bot), false);
    let eleven: Vec<Value> = (0..11)
        .map(|index| json!({"symbol": format!("T{index}")}))
        .collect();
    for (arguments, field) in [
        (json!({"preset": "moon"}), "preset"),
        (json!({"focus": "x".repeat(281)}), "focus"),
        (json!({"focus": "   "}), "focus"),
        (json!({"follow": eleven}), "follow"),
        (json!({"follow": [{"symbol": "NV DA"}]}), "follow[0].symbol"),
        (
            json!({"follow": [{"symbol": "AMD"}, {"symbol": "NVDA", "ticker": "NVDA"}]}),
            "follow[1]",
        ),
        (
            json!({"unfollow": [{"symbol": "NVDA", "name": "Nvidia"}]}),
            "unfollow[0]",
        ),
        (
            json!({"follow": [{"symbol": "NVDA", "market": "NAS DAQ"}]}),
            "follow[0].market",
        ),
        (
            json!({"follow": [{"symbol": "NVDA"}], "unfollow": [{"symbol": "nvda"}]}),
            "unfollow[0].symbol",
        ),
        (json!({}), "loupist_setup"),
        (json!({"focus": "Chips", "theme": "dark"}), "loupist_setup"),
    ] {
        let error = executor
            .execute(&policy(true), ToolMethod::LoupistSetup, arguments.clone())
            .await
            .unwrap_err();
        assert!(
            matches!(&error, BrokerError::InvalidArguments { field: refused, .. } if refused == field),
            "{arguments}: {error}"
        );
    }
    assert!(matches!(
        executor
            .execute(&policy(false), ToolMethod::LoupistSetup, setup())
            .await,
        Err(BrokerError::MemoryDisabled)
    ));
    assert!(executor.capture.staged_setup.lock().await.is_none());
}

#[test]
fn each_preset_default_focus_is_the_new_loupist_dialog_copy() {
    let onboarding: Value = serde_json::from_str(include_str!(
        "../../../../src/i18n/locales/en/onboarding.json"
    ))
    .unwrap();
    for template in LoupistTemplate::ALL {
        let copy = &onboarding["wizard"]["templates"][template.as_str()]["focus"];
        match template.default_focus() {
            Some(focus) => assert_eq!(copy, focus, "{}", template.as_str()),
            None => assert_eq!(template, LoupistTemplate::Custom),
        }
    }
}

#[test]
fn the_tool_offers_every_template_as_a_preset() {
    let catalog: Value =
        serde_json::from_str(include_str!("../../../../agent/tool-catalog.json")).unwrap();
    let tool = catalog["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "loupist_setup")
        .unwrap();
    assert_eq!(
        tool["parameters"]["properties"]["preset"]["enum"],
        json!(LoupistTemplate::ALL.map(LoupistTemplate::as_str))
    );
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
mod turns {
    use super::*;
    use crate::{
        chat::home::{ensure_home_thread, home_thread_id},
        commands::{
            chat_runtime::{send_chat, ChatEventSink},
            tests::chat_behavior::{drain, install_chat_rpc_fixture, send_request},
        },
        domain::SetupChange,
    };

    struct Fixture {
        temporary: tempfile::TempDir,
        state: AppState,
        loupist_id: String,
    }

    fn fixture() -> Fixture {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        initialized_workspace(&workspace, "loupist-setup");
        let mut state = AppState::for_test(temporary.path().join("app"));
        install_chat_rpc_fixture(
            &mut state,
            temporary.path(),
            include_str!("fixtures/loupist_setup_pi.py"),
        );
        let profile = profile("loupist-setup", &workspace, 1);
        seed_profile(state.store.as_ref(), &profile);
        ensure_home_thread(&state, &profile.id).unwrap();
        Fixture {
            temporary,
            state,
            loupist_id: profile.id,
        }
    }

    impl Fixture {
        fn calls(&self, arguments: Value) {
            fs::write(
                self.temporary.path().join("setup-args.json"),
                arguments.to_string(),
            )
            .unwrap();
            let _ = fs::remove_file(self.temporary.path().join("setup-calls.jsonl"));
        }

        fn results(&self) -> Vec<Value> {
            fs::read_to_string(self.temporary.path().join("setup-calls.jsonl"))
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap()["result"].clone())
                .collect()
        }

        async fn bot_turn(&self, run_id: &str) {
            let request = ChatSendRequest {
                use_memory: true,
                update_memory: true,
                ..send_request(
                    run_id,
                    &self.loupist_id,
                    &home_thread_id(&self.loupist_id),
                    "I research the semiconductor supply chain; follow Nvidia and SK hynix.",
                )
            };
            send_chat(request, ChatEventSink::new(|_| Ok(())), &self.state, None)
                .await
                .unwrap();
        }

        fn reply(&self) -> ChatMessage {
            self.state
                .store
                .get_chat(&home_thread_id(&self.loupist_id))
                .unwrap()
                .unwrap()
                .messages
                .into_iter()
                .rfind(|message| {
                    message.role == ChatRole::Assistant && message.vocabulary.is_none()
                })
                .unwrap()
        }

        fn watchlist(&self) -> BTreeSet<(String, String, Option<String>)> {
            self.state
                .store
                .watchlist_list()
                .unwrap()
                .into_iter()
                .map(|entry| (entry.symbol, entry.market, entry.display_name))
                .collect()
        }

        fn focus(&self) -> (String, String) {
            let profile = self
                .state
                .store
                .get_loupist(&self.loupist_id)
                .unwrap()
                .unwrap();
            let connection = self.state.store.lock_loupist(&self.loupist_id).unwrap();
            let settings = crate::knowledge::load_settings(&connection, &self.loupist_id).unwrap();
            (profile.focus, settings.research_focus)
        }

        /// The perspective page's `focus` section, when the page exists.
        fn page_focus(&self) -> Option<String> {
            let knowledge = crate::knowledge::KnowledgeStore::for_loupist(
                self.state.store.clone(),
                &self.loupist_id,
            )
            .unwrap();
            knowledge
                .read(|write| {
                    let Some(record_id) = write.active_perspective(&self.loupist_id)? else {
                        return Ok(None);
                    };
                    Ok(write
                        .read_page(&self.loupist_id, &record_id)?
                        .body
                        .sections()
                        .iter()
                        .find(|section| section.key.as_str() == "focus")
                        .map(|section| section.markdown.clone()))
                })
                .unwrap()
        }

        fn preset_changes(&self) -> Vec<(String, Option<String>)> {
            let connection = self.state.store.lock_loupist(&self.loupist_id).unwrap();
            let mut query = connection
                .prepare(
                    "SELECT producer, origin_chat_id FROM knowledge_schema_changes ORDER BY rowid",
                )
                .unwrap();
            query
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        }
    }

    #[tokio::test]
    async fn a_bot_turn_follows_sets_the_focus_and_applies_the_preset_kinds_once() {
        let fixture = fixture();
        fixture.calls(json!([setup(), {"focus": "A second call"}]));
        fixture.bot_turn("setup-1").await;
        drain(&fixture.state).await;

        let results = fixture.results();
        assert_eq!(results[0]["ok"], true, "{results:?}");
        assert_eq!(results[1]["ok"], false, "one call per reply: {results:?}");

        let reply = fixture.reply();
        assert_eq!(reply.status, ChatMessageStatus::Complete);
        assert_eq!(
            fixture.watchlist(),
            BTreeSet::from([
                ("NVDA".into(), "NASDAQ".into(), Some("Nvidia".into())),
                ("000660".into(), "UNKNOWN".into(), Some("SK hynix".into())),
            ])
        );
        let focus = "Semiconductor supply chain".to_owned();
        assert_eq!(fixture.focus(), (focus.clone(), focus.clone()));
        assert_eq!(
            fixture.page_focus(),
            Some(focus.clone()),
            "the Lens page shows the same focus"
        );
        let home = home_thread_id(&fixture.loupist_id);
        assert_eq!(
            fixture.preset_changes(),
            [("preset".to_owned(), Some(home.clone()))]
        );

        let update = reply.memory_update.unwrap();
        assert_eq!(update.status, crate::domain::MemoryUpdateStatus::Applied);
        let receipt: Vec<Value> = update
            .setup
            .iter()
            .map(|change| serde_json::to_value(change).unwrap())
            .collect();
        assert_eq!(
            receipt[..3],
            [
                json!({"change": "followed", "symbol": "NVDA", "market": "NASDAQ", "name": "Nvidia"}),
                json!({"change": "followed", "symbol": "000660", "name": "SK hynix"}),
                json!({"change": "focus", "focus": "Semiconductor supply chain"}),
            ]
        );
        let SetupChange::Tracks { kinds } = &update.setup[3] else {
            panic!("the map's new kinds are on the receipt: {receipt:?}");
        };
        assert!(!kinds.is_empty() && kinds.iter().all(|kind| kind.label_ko.is_some()));
        assert_eq!(update.setup.len(), 4);
        let bot = fixture.state.store.get_chat(&home).unwrap().unwrap();
        assert!(
            bot.messages
                .iter()
                .all(|message| message.vocabulary.is_none()),
            "the receipt says it; the Bot posts no second card"
        );

        fixture.calls(json!([{"preset": "semiconductors"}]));
        fixture.bot_turn("setup-2").await;
        drain(&fixture.state).await;
        assert_eq!(fixture.results()[0]["ok"], true);
        let again = fixture.reply();
        assert_ne!(again.id, reply.id);
        let update = again.memory_update.unwrap();
        assert!(update.setup.is_empty(), "{:?}", update.setup);
        assert_eq!(
            fixture.preset_changes().len(),
            1,
            "a repeated preset adds nothing"
        );
        assert_eq!(fixture.focus(), (focus.clone(), focus), "a set focus stays");
    }

    #[tokio::test]
    async fn a_stopped_turn_changes_nothing() {
        let fixture = fixture();
        fixture.calls(json!([setup()]));
        fs::write(fixture.temporary.path().join("wait-for-stop"), "").unwrap();
        fixture.bot_turn("setup-stopped").await;
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            while fixture.results().is_empty() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the setup call never reached the broker");
        fixture
            .state
            .cancel_run("setup-stopped", RunKind::Chat)
            .await
            .unwrap();
        drain(&fixture.state).await;

        assert_eq!(fixture.results()[0]["ok"], true);
        assert_eq!(fixture.reply().status, ChatMessageStatus::Aborted);
        assert!(fixture.watchlist().is_empty());
        assert_eq!(fixture.focus(), (String::new(), String::new()));
        assert_eq!(fixture.page_focus(), None);
        assert!(fixture.preset_changes().is_empty());
    }
}
