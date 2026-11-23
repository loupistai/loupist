use super::*;
use crate::{
    commands::tests::{chat, chat_behavior::send_request, profile, seed_profile},
    finance_credentials::{self, test_store, KeychainReadStatus},
    providers::{
        catalog::{ConfiguredModel, ModelCatalog, ModelRunControl, ModelRunControlChoice},
        status::{self, CredentialKind, Trigger},
    },
};

struct AdmissionFixture {
    root: tempfile::TempDir,
    state: AppState,
    profile: crate::domain::LoupistProfile,
}

impl AdmissionFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut state = AppState::for_folder_test(root.path().join("app"));
        let agent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../agent");
        Arc::make_mut(&mut state.artifacts).pi = Some(crate::app::PiArtifacts {
            executable: root.path().join("unused-pi"),
            runtime_dir: root.path().join("unused-runtime"),
            extension: agent.join("loupist-extension.mjs"),
            provider_extension: agent.join("loupist-provider-extension.mjs"),
            system_prompt: agent.join("SYSTEM.md"),
            provider: String::new(),
            model: String::new(),
            thinking_level: String::new(),
            run_options: BTreeMap::new(),
            provider_credential: None,
        });
        let profile = profile("loupist-admission", &root.path().join("workspace"), 1);
        seed_profile(&state.store, &profile);
        state
            .set_model_catalog(ModelCatalog { models: vec![] })
            .unwrap();
        Self {
            root,
            state,
            profile,
        }
    }

    fn connect(&self, providers: &[(&str, Option<&str>)]) {
        let auth = providers
            .iter()
            .map(|(provider, _)| {
                (
                    provider.to_string(),
                    json!({
                        "type": "oauth", "access": "fixture-access",
                        "refresh": "fixture-refresh", "expires": 4_102_444_800_000u64
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        let path = self.root.path().join("app/pi/auth.json");
        crate::fs::artifact_trust::ensure_private_regular_file(&path).unwrap();
        std::fs::write(&path, serde_json::to_vec(&auth).unwrap()).unwrap();
        for (provider, model) in providers {
            let models = model
                .iter()
                .map(|model| ConfiguredModel {
                    id: format!("{provider}/{model}"),
                    name: model.to_string(),
                    provider: provider.to_string(),
                    model: model.to_string(),
                    input: vec!["text".into()],
                    reasoning: true,
                    context_window: 128_000,
                    max_tokens: 16_000,
                    thinking_levels: vec!["low".into(), "medium".into(), "high".into()],
                    thinking_level_map: BTreeMap::new(),
                    run_controls: vec![ModelRunControl {
                        id: "performance".into(),
                        label: "Performance".into(),
                        default_choice: "standard".into(),
                        choices: vec![ModelRunControlChoice {
                            id: "standard".into(),
                            label: "Standard".into(),
                            description: "Use the standard service tier.".into(),
                        }],
                    }],
                })
                .collect();
            self.state
                .replace_provider_models(provider, models)
                .unwrap();
            status::record(
                &self.state,
                provider,
                Trigger::Connected(CredentialKind::Plan),
            )
            .unwrap();
        }
        crate::commands::knowledge::follow_recommended_world_models(&self.state).unwrap();
    }

    fn request(&self, run_id: &str, thread_id: &str, model_id: &str) -> ChatSendRequest {
        if self.state.store.get_chat(thread_id).unwrap().is_none() {
            self.state
                .store
                .create_chat(&chat(thread_id, &self.profile.id, 1))
                .unwrap();
        }
        let mut request = send_request(run_id, &self.profile.id, thread_id, "Hello");
        request.model_profile_id = model_id.into();
        request.thinking_level.clear();
        request
    }

    fn admit(&self, request: ChatSendRequest) -> setup::AdmittedChatTurn {
        match setup::admit_chat_turn(request, ChatEventSink::new(|_| Ok(())), &self.state, None) {
            Ok(ControlFlow::Continue(turn)) => turn,
            Ok(ControlFlow::Break(_)) => panic!("Chat stopped during setup"),
            Err(error) => panic!("Chat setup failed: {error:?}"),
        }
    }

    fn save_model_only_preference(&self, model: &str) -> PathBuf {
        let project = Path::new(&self.profile.workspace_root).join(".loupist");
        let path = project.join("config.toml");
        let config = format!("[chat]\nmodel_profile_id = '{model}'\n");
        std::fs::write(&path, config).unwrap();
        path
    }

    fn default_selection(&self) -> crate::providers::selection::ResolvedModelSelection {
        use crate::scoped_config::{read_scope, ConfigScope};
        let project = Path::new(&self.profile.workspace_root).join(".loupist");
        let snapshot = read_scope(
            &self.state.artifacts.app_data_dir,
            Some(&project),
            ConfigScope::Project {
                loupist_id: self.profile.id.clone(),
            },
        )
        .unwrap();
        crate::commands::configuration::present(&self.state, snapshot)
            .unwrap()
            .model_selection
            .unwrap()
    }
}

#[test]
fn admission_uses_model_only_preferences_across_disconnect_and_reconnect() {
    use crate::chat::{home::home_thread_id, surface::ChatSurface};
    let grok = ("xai", Some("grok-4.7"));
    let gpt = ("openai", Some("gpt-6.2-sol"));
    for [selected, other] in [[gpt, grok], [grok, gpt]] {
        let fixture = AdmissionFixture::new();
        let selected_id = format!("{}/{}", selected.0, selected.1.unwrap());
        let other_id = format!("{}/{}", other.0, other.1.unwrap());
        let preference = fixture.save_model_only_preference(&selected_id);
        let before = std::fs::read_to_string(&preference).unwrap();
        let verify = |scenario: &str, model: &str, auto: bool| {
            let selection = fixture.default_selection();
            assert_eq!(selection.is_default, auto);
            assert_eq!(selection.model.unwrap().id, model);
            for (thread, surface) in [
                (home_thread_id(&fixture.profile.id), ChatSurface::Bot),
                (format!("research-{scenario}"), ChatSurface::Session),
            ] {
                let mut request = fixture.request(
                    &format!("chat-{scenario}"),
                    &thread,
                    if auto { "" } else { model },
                );
                if !auto {
                    request.thinking_level = selection.thinking_level.clone().unwrap();
                    request.run_options = selection.run_options.clone();
                }
                let turn = fixture.admit(request);
                assert_eq!(turn.surface, surface);
                assert_eq!(turn.configured_model.id, model);
                assert_eq!(
                    turn.pi_execution.run_options,
                    BTreeMap::from([("performance".into(), "standard".into())])
                );
            }
        };
        fixture.connect(&[other, selected]);
        verify("connected", &selected_id, false);
        fixture.connect(&[other]);
        fixture
            .state
            .replace_provider_models(selected.0, vec![])
            .unwrap();
        status::record(
            &fixture.state,
            selected.0,
            Trigger::Disconnected(Default::default()),
        )
        .unwrap();
        verify("disconnected", &other_id, true);
        fixture.connect(&[other, selected]);
        verify("reconnected", &selected_id, false);
        assert_eq!(std::fs::read_to_string(preference).unwrap(), before);
    }
}

#[tokio::test]
async fn admission_with_two_plan_accounts_and_keychain_read_failures() {
    for keychain_status in [None, Some(-25307), Some(-67869), Some(-25308)] {
        let fixture = AdmissionFixture::new();
        let state = &fixture.state;
        if let Some(os_status) = keychain_status {
            let id = "fred.macro";
            test_store::fail_with_security_status(&state.credential_environment, id, os_status);
            assert!(finance_credentials::get_for_state(state, id).is_err());
            assert_eq!(
                finance_credentials::refused_read_status(state, id),
                Some(match os_status {
                    -25307 => KeychainReadStatus::NoDefaultKeychain,
                    -67869 => KeychainReadStatus::DatabaseLocked,
                    _ => KeychainReadStatus::InteractionNotAllowed,
                })
            );
        } else {
            assert!(finance_credentials::get_for_state(state, "fred.macro")
                .unwrap()
                .is_none());
        }
        crate::app::off_main_thread(state, crate::marketplace::reconcile_credential_presence)
            .await
            .unwrap();
        let grok = ("xai", Some("grok-4.7"));
        let chatgpt = ("openai", Some("gpt-6.2-sol"));
        for [first, second] in [[grok, chatgpt], [chatgpt, grok]] {
            let first_id = format!("{}/{}", first.0, first.1.unwrap());
            let second_id = format!("{}/{}", second.0, second.1.unwrap());
            fixture.connect(&[first]);
            state.replace_provider_models(second.0, vec![]).unwrap();
            let mut request = fixture.request("chat-first", first.0, &first_id);
            request.run_options = BTreeMap::from([("performance".into(), "standard".into())]);
            let first_turn = fixture.admit(request);
            fixture.connect(&[first, second]);
            assert!(!*first_turn.cancel.borrow());
            let request = fixture.request("chat-second", second.0, &second_id);
            assert_eq!(fixture.admit(request).configured_model.id, second_id);
            drop(first_turn);
            let followup = fixture.request("chat-followup", first.0, &first_id);
            assert_eq!(fixture.admit(followup).configured_model.id, first_id);
            let auto = fixture.request("chat-auto", "new-auto", "");
            assert!(!fixture.admit(auto).configured_model.id.is_empty());
        }
    }
}

#[test]
fn admission_omits_incompatible_inherited_controls_for_a_project_model() {
    let fixture = AdmissionFixture::new();
    let other = "anthropic/claude-sonnet-4-5";
    fixture.connect(&[
        ("xai", Some("grok-4.7")),
        ("anthropic", Some("claude-sonnet-4-5")),
    ]);
    let mut model = fixture
        .state
        .model_catalog()
        .unwrap()
        .resolve(other)
        .unwrap();
    model.run_controls.clear();
    fixture
        .state
        .replace_provider_models("anthropic", vec![model])
        .unwrap();
    std::fs::write(
        fixture.state.artifacts.app_data_dir.join("config.toml"),
        "[chat]\nmodel_profile_id = 'xai/grok-4.7'\n[chat.run_options]\nperformance = 'standard'\n",
    )
    .unwrap();
    let path = fixture.save_model_only_preference(other);
    let selection = fixture.default_selection();
    assert!(selection.run_options.is_empty());
    let mut request = fixture.request("chat-overridden-default", "research", other);
    request.run_options = selection.run_options;
    assert_eq!(fixture.admit(request).configured_model.id, other);

    // An option explicitly set in this Loupist still reaches validation.
    std::fs::write(
        path,
        format!("[chat]\nmodel_profile_id = '{other}'\n[chat.run_options]\nperformance = 'fast'\n"),
    )
    .unwrap();
    assert_eq!(
        fixture.default_selection().run_options["performance"],
        "fast"
    );
}

#[test]
fn admission_keeps_a_usable_account_when_the_other_account_has_no_models_or_sign_in() {
    let fixture = AdmissionFixture::new();
    let good = ("xai", Some("grok-4.7"));
    fixture.connect(&[("openai", None), good]);
    fixture.save_model_only_preference("openai/gpt-6.2-sol");
    let selection = fixture.default_selection();
    assert!(selection.is_default);
    assert_eq!(selection.model.unwrap().id, "xai/grok-4.7");
    let request = fixture.request("chat-empty-catalog", "research", "");
    assert_eq!(fixture.admit(request).configured_model.id, "xai/grok-4.7");

    fixture.connect(&[("openai", Some("gpt-6.2-sol")), good]);
    let path = fixture.state.pi_agent_data_dir().unwrap().join("auth.json");
    let mut auth: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    // A valid credential document with a single unreadable saved plan entry.
    auth["openai"]["expires"] = Value::Null;
    std::fs::write(&path, serde_json::to_vec(&auth).unwrap()).unwrap();
    // The stored model cannot run, so Auto answers in its place.
    let selection = fixture.default_selection();
    assert_eq!(selection.unavailable_reason, None);
    assert_eq!(
        selection.stored_unavailable,
        Some(crate::providers::selection::ModelUnavailableReason::AccountDisconnected)
    );
    assert!(selection.is_default);
    assert_eq!(selection.model.unwrap().id, "xai/grok-4.7");
    for model in ["", "xai/grok-4.7"] {
        let request = fixture.request("chat-other-sign-in", "research", model);
        assert_eq!(fixture.admit(request).configured_model.id, "xai/grok-4.7");
    }

    // Disconnecting removes the model list while the explicit preference stays.
    fixture
        .state
        .replace_provider_models("openai", vec![])
        .unwrap();
    let selection = fixture.default_selection();
    assert!(selection.is_default);
    assert_eq!(selection.model.unwrap().id, "xai/grok-4.7");
    let request = fixture.request(
        "chat-disconnected-default",
        "research",
        "openai/gpt-6.2-sol",
    );
    assert_eq!(fixture.admit(request).configured_model.id, "xai/grok-4.7");
}

#[tokio::test]
async fn admission_refuses_unsupported_controls_before_acknowledging_the_request() {
    let fixture = AdmissionFixture::new();
    fixture.connect(&[("xai", Some("grok-4.7"))]);
    let mut request = fixture.request("chat-invalid-option", "research", "xai/grok-4.7");
    request.run_options = BTreeMap::from([("performance".into(), "private-invalid-option".into())]);
    let error = send_chat(
        request,
        ChatEventSink::new(|_| panic!("a refused request must have no Chat stream events")),
        &fixture.state,
        None,
    )
    .await
    .expect_err("invalid controls must be refused");
    assert_eq!(error.code, "invalid_request");
    assert!(error.message.contains("Chat defaults"));
    let chat = fixture.state.store.get_chat("research").unwrap().unwrap();
    assert!(chat.messages.is_empty());
    let logs = Path::new(&fixture.profile.workspace_root).join(".loupist/logs");
    let log = std::fs::read_dir(logs)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .unwrap();
    let text = std::fs::read_to_string(log).unwrap();
    let record: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        record["chatStartFailure"],
        json!({"phase":"model","reason":"invalid_request"})
    );
    assert!(record["model"].is_null());
    assert!(!text.contains("private-invalid-option"));
}

mod stored_default_fallback {
    use super::*;
    use crate::providers::selection::ModelUnavailableReason;
    use crate::scheduled_tasks::types::{Schedule, TaskDraft, TaskStatus, TaskTarget};

    const STORED: &str = "openai/gpt-6.2-sol";
    const AUTO: &str = "xai/grok-4.7";

    /// The stored Chat default declares a thinking level and a run option that
    /// the model Auto falls back to does not.
    fn fixture() -> AdmissionFixture {
        let fixture = AdmissionFixture::new();
        connect_both(&fixture);
        let config = format!(
            "[chat]\nmodel_profile_id = '{STORED}'\nthinking_level = 'xhigh'\n[chat.run_options]\nperformance = 'fast'\n"
        );
        let project = Path::new(&fixture.profile.workspace_root).join(".loupist");
        std::fs::write(project.join("config.toml"), config).unwrap();
        fixture
    }

    /// Both accounts connected, as Settings connects them: each lists its models.
    fn connect_both(fixture: &AdmissionFixture) {
        fixture.connect(&[("openai", Some("gpt-6.2-sol")), ("xai", Some("grok-4.7"))]);
        let mut stored = fixture
            .state
            .model_catalog()
            .unwrap()
            .resolve(STORED)
            .unwrap();
        stored.thinking_levels.push("xhigh".into());
        stored.run_controls[0].choices.push(ModelRunControlChoice {
            id: "fast".into(),
            label: "Fast".into(),
            description: "Use the priority service tier.".into(),
        });
        fixture
            .state
            .replace_provider_models("openai", vec![stored])
            .unwrap();
    }

    fn sign_in(fixture: &AdmissionFixture, providers: &[&str]) {
        let auth = providers
            .iter()
            .map(|provider| {
                (
                    provider.to_string(),
                    json!({
                        "type": "oauth", "access": "fixture-access",
                        "refresh": "fixture-refresh", "expires": 4_102_444_800_000u64
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        let path = fixture.root.path().join("app/pi/auth.json");
        std::fs::write(&path, serde_json::to_vec(&auth).unwrap()).unwrap();
    }

    fn stored_task(fixture: &AdmissionFixture) -> crate::scheduled_tasks::types::Task {
        let draft = TaskDraft {
            title: "Daily review".into(),
            prompt: "Read the local report.".into(),
            schedule: Schedule::Interval { every_minutes: 60 },
            target: TaskTarget::NewChat,
            model_profile_id: STORED.into(),
            thinking_level: "xhigh".into(),
            run_options: BTreeMap::from([("performance".into(), "fast".into())]),
            use_memory: false,
            update_memory: false,
            status: TaskStatus::Paused,
        };
        fixture
            .state
            .store
            .scheduled_task_create(&fixture.profile.id, draft, 1)
            .unwrap()
    }

    /// What a Chat turn, a scheduled run and a hand-off run on, each admitted
    /// the way that path admits it, as (model, thinking level, performance).
    fn admitted(fixture: &AdmissionFixture, scenario: &str) -> [(String, String, String); 3] {
        let ran = |request: ChatSendRequest| {
            let turn = fixture.admit(request);
            (
                turn.configured_model.id,
                turn.pi_execution.thinking_level,
                turn.pi_execution.run_options["performance"].clone(),
            )
        };
        // Chat: the renderer sends what the configuration snapshot resolved,
        // Auto with the Chat defaults' controls when the selection is Auto.
        let selection = fixture.default_selection();
        let mut chat = fixture.request(
            &format!("chat-{scenario}"),
            &format!("research-{scenario}"),
            if selection.is_default { "" } else { STORED },
        );
        chat.thinking_level = "xhigh".into();
        chat.run_options = BTreeMap::from([("performance".into(), "fast".into())]);
        let interactive = ran(chat);
        let (model, level, options) =
            crate::scheduled_tasks::run_settings(&fixture.state, &stored_task(fixture));
        let mut scheduled = fixture.request(
            &format!("scheduled-{scenario}"),
            &format!("scheduled-{scenario}"),
            &model,
        );
        (scheduled.thinking_level, scheduled.run_options) = (level, options);
        let scheduled = ran(scheduled);
        let defaults = crate::commands::loupist_configuration(&fixture.state, &fixture.profile.id)
            .unwrap()
            .chat;
        let (model, level, options) = crate::commands::handoff::run_settings(
            &fixture.state,
            &fixture.profile.id,
            &defaults,
            None,
            None,
            None,
        )
        .unwrap();
        let mut handoff = fixture.request(
            &format!("handoff-{scenario}"),
            &format!("handoff-{scenario}"),
            &model,
        );
        (handoff.thinking_level, handoff.run_options) = (level, options);
        [interactive, scheduled, ran(handoff)]
    }

    #[test]
    fn a_stored_default_that_cannot_run_falls_back_to_auto_on_every_path_and_returns() {
        type Cause = (
            &'static str,
            fn(&AdmissionFixture),
            fn(&AdmissionFixture),
            ModelUnavailableReason,
        );
        let causes: [Cause; 4] = [
            (
                // The sign-in ended outside Loupist; its models are still listed.
                "signed-out",
                |fixture| sign_in(fixture, &["xai"]),
                |fixture| sign_in(fixture, &["openai", "xai"]),
                ModelUnavailableReason::AccountDisconnected,
            ),
            (
                "disconnected",
                |fixture| {
                    sign_in(fixture, &["xai"]);
                    status::record(
                        &fixture.state,
                        "openai",
                        Trigger::Disconnected(status::UsedBy::default()),
                    )
                    .unwrap();
                },
                |fixture| {
                    sign_in(fixture, &["openai", "xai"]);
                    status::record(
                        &fixture.state,
                        "openai",
                        Trigger::Connected(CredentialKind::Plan),
                    )
                    .unwrap();
                },
                ModelUnavailableReason::AccountDisconnected,
            ),
            (
                // Settings' Disconnect: the account's models leave the catalog.
                "disconnected-unlisted",
                |fixture| {
                    sign_in(fixture, &["xai"]);
                    status::record(
                        &fixture.state,
                        "openai",
                        Trigger::Disconnected(status::UsedBy::default()),
                    )
                    .unwrap();
                    fixture
                        .state
                        .replace_provider_models("openai", vec![])
                        .unwrap();
                },
                connect_both,
                ModelUnavailableReason::AccountDisconnected,
            ),
            (
                "hidden",
                |fixture| fixture.state.set_model_visible(STORED, false).unwrap(),
                |fixture| fixture.state.set_model_visible(STORED, true).unwrap(),
                ModelUnavailableReason::ModelHidden,
            ),
        ];
        for (cause, lose, restore, reason) in causes {
            let fixture = fixture();
            let stored = |scenario: &str| {
                let selection = fixture.default_selection();
                assert!(!selection.is_default, "{cause}");
                assert_eq!(selection.stored_unavailable, None, "{cause}");
                assert_eq!(selection.stored_model, None, "{cause}");
                assert_eq!(selection.model.unwrap().id, STORED, "{cause}");
                for (path, ran) in ["chat", "scheduled", "hand-off"]
                    .into_iter()
                    .zip(admitted(&fixture, scenario))
                {
                    assert_eq!(
                        ran,
                        (STORED.to_owned(), "xhigh".to_owned(), "fast".to_owned()),
                        "{cause} {path}"
                    );
                }
            };
            stored(&format!("{cause}-before"));
            let config = std::fs::read_to_string(
                Path::new(&fixture.profile.workspace_root).join(".loupist/config.toml"),
            )
            .unwrap();
            lose(&fixture);
            let selection = fixture.default_selection();
            assert_eq!(selection.unavailable_reason, None, "{cause}: Chat can send");
            assert_eq!(selection.stored_unavailable, Some(reason), "{cause}");
            // Chat defaults name the stored model, never the model Auto picked.
            let named = selection.stored_model.as_ref().expect(cause);
            assert_eq!(
                (named.id.as_str(), named.name.as_str()),
                (STORED, "gpt-6.2-sol")
            );
            assert!(selection.is_default, "{cause}");
            assert_eq!(selection.model.unwrap().id, AUTO, "{cause}");
            for (path, (model, level, performance)) in ["chat", "scheduled", "hand-off"]
                .into_iter()
                .zip(admitted(&fixture, &format!("{cause}-lost")))
            {
                assert_eq!(model, AUTO, "{cause} {path}");
                // Controls the fallback model does not declare are never sent.
                assert_ne!(level, "xhigh", "{cause} {path}");
                assert_eq!(performance, "standard", "{cause} {path}");
            }
            // The stored choice is kept, and applies again once it can run.
            assert_eq!(
                std::fs::read_to_string(
                    Path::new(&fixture.profile.workspace_root).join(".loupist/config.toml"),
                )
                .unwrap(),
                config
            );
            restore(&fixture);
            stored(&format!("{cause}-after"));
        }
    }

    #[test]
    fn a_stored_default_its_provider_stopped_offering_is_named_while_auto_answers() {
        let fixture = fixture();
        crate::providers::auto::prune_gone(&fixture.state, STORED);
        let selection = fixture.default_selection();
        assert_eq!(selection.unavailable_reason, None);
        assert_eq!(
            selection.stored_unavailable,
            Some(ModelUnavailableReason::CatalogUnavailable)
        );
        assert_eq!(selection.stored_model.unwrap().name, "gpt-6.2-sol");
        assert_eq!(selection.model.unwrap().id, AUTO);
        // A stored model the catalog never listed has no name to show.
        fixture.save_model_only_preference("openai/gpt-never-listed");
        let selection = fixture.default_selection();
        assert_eq!(
            selection.stored_unavailable,
            Some(ModelUnavailableReason::CatalogUnavailable)
        );
        assert_eq!(selection.stored_model, None);
        // Listed again, the model needs no remembered name.
        connect_both(&fixture);
        assert_eq!(fixture.state.store.unlisted_model(STORED).unwrap(), None);
    }

    #[test]
    fn with_no_account_that_can_run_a_model_chat_still_refuses() {
        let fixture = fixture();
        sign_in(&fixture, &[]);
        let selection = fixture.default_selection();
        assert_eq!(
            selection.unavailable_reason,
            Some(ModelUnavailableReason::NoConnectedAccount)
        );
        assert!(selection.model.is_none());
        let request = fixture.request("chat-no-account", "research-no-account", "");
        let error = match setup::admit_chat_turn(
            request,
            ChatEventSink::new(|_| Ok(())),
            &fixture.state,
            None,
        ) {
            Err(error) => error,
            Ok(_) => panic!("a turn with no AI account was admitted"),
        };
        assert_eq!(
            error.error.code,
            crate::app::CommandErrorCode::ProviderUnavailable
        );
        // A stored model that cannot run gives way to nothing, so it is kept.
        assert!(crate::providers::auto::stored_fallback(
            &fixture.state,
            STORED,
            "xhigh",
            &BTreeMap::new()
        )
        .is_none());
    }

    #[test]
    fn a_hand_off_or_schedule_that_names_a_runnable_model_keeps_it() {
        let fixture = fixture();
        let defaults = crate::commands::loupist_configuration(&fixture.state, &fixture.profile.id)
            .unwrap()
            .chat;
        let named = crate::commands::handoff::run_settings(
            &fixture.state,
            &fixture.profile.id,
            &defaults,
            Some(AUTO),
            Some("low"),
            None,
        )
        .unwrap();
        assert_eq!((named.0.as_str(), named.1.as_str()), (AUTO, "low"));
        let (model, level, _) =
            crate::scheduled_tasks::run_settings(&fixture.state, &stored_task(&fixture));
        assert_eq!((model.as_str(), level.as_str()), (STORED, "xhigh"));
    }
}
