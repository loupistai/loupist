use super::*;
use crate::scheduled_tasks::types::RunStatus;
#[test]
fn summary_is_first_prose_paragraph_without_markdown() {
    assert_eq!(
        summary(
            "# Heading\n\n**Revenue** grew [today](https://example.org).\nNext line.\n\nSecond."
        ),
        "Revenue grew today. Next line."
    );
    assert_eq!(summary(&"한".repeat(400)).chars().count(), 280);
    // A paragraph that is only a bold label, such as a dateline, is a heading
    // in disguise; the summary starts at the prose after it.
    assert_eq!(
        summary("# Title\n**조사 기준일: 2026년 11월 6일**\n\n**No direct contract** was found."),
        "No direct contract was found."
    );
    assert_eq!(summary("**Only a label**"), "Only a label");
}
#[test]
fn summary_skips_tables_and_loose_lists_and_preserves_first_prose() {
    assert_eq!(
        summary("# Heading\n\n| Quarter | Sales |\n| --- | --- |\n| Q3 | 42 |\n\n- List item\n\n  Another list paragraph\n\n첫 *문장*입니다.\n\nLater."),
        "첫 문장입니다."
    );
    assert_eq!(
        summary("| Quarter | Sales |\n| --- | --- |\n| Q3 | 42 |"),
        ""
    );
    assert_eq!(
        summary(
            "First paragraph.

Later."
        ),
        "First paragraph."
    );
}

#[test]
fn journal_hook_is_empty_and_unattended_native_tools_are_denied() {
    for name in [
        "read",
        "write",
        "edit",
        "find",
        "grep",
        "ls",
        "schedule_create",
        "principle_save",
        "loupist_setup",
        "ask_user",
        "interface_labels",
        "capability_list",
        "skills_list",
        "skills_read",
    ] {
        assert!(!policy::allowed_name(name), "{name}");
    }
    for name in [
        "world_read",
        "source_save",
        "web_search",
        "lens_file",
        "run_python",
        "capability_load",
    ] {
        assert!(policy::allowed_name(name), "{name}");
    }
}

fn state_fixture() -> (tempfile::TempDir, AppState, String) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(workspace.join(".loupist")).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let bound = crate::fs::loupist_root::BoundLoupistRoot::open_unbound(workspace.clone()).unwrap();
    let state = AppState::for_test(root.path().join("app"));
    let loupist = crate::domain::LoupistProfile {
        workspace_root: workspace.to_string_lossy().into_owned(),
        root_filesystem_identity: bound.identity(),
        ..crate::test_fixtures::loupist_profile("loupist-brief")
    };
    state.store.create_loupist(&loupist).unwrap();
    crate::knowledge::KnowledgeStore::for_loupist(state.store.clone(), &loupist.id).unwrap();
    (root, state, loupist.id)
}

fn connect_fixture_accounts(state: &AppState, providers: &[&str]) {
    let auth = state.artifacts.app_data_dir.join("pi/auth.json");
    crate::fs::artifact_trust::ensure_private_regular_file(&auth).unwrap();
    let accounts: serde_json::Map<_, _> = providers
        .iter()
        .map(|provider| {
            (
                provider.to_string(),
                serde_json::json!({"type":"api_key","key":"fixture"}),
            )
        })
        .collect();
    std::fs::write(auth, serde_json::to_vec(&accounts).unwrap()).unwrap();
}

#[test]
fn settings_opt_in_is_single_task_and_pause_preserves_its_chat_definition() {
    let (_root, state, loupist) = state_fixture();
    let initial = commands::settings_get(&state, &loupist).unwrap();
    assert!(!initial.enabled);
    assert!(initial.task.is_none());
    let enabled = commands::settings_set(
        &state,
        commands::SettingsRequest {
            loupist_id: loupist.clone(),
            enabled: true,
            schedule: None,
            edition: None,
        },
    )
    .unwrap();
    let task = enabled.task.unwrap();
    assert!(task.use_memory && task.update_memory);
    assert_eq!(task.kind, crate::scheduled_tasks::types::TaskKind::Briefing);
    let again = commands::settings_set(
        &state,
        commands::SettingsRequest {
            loupist_id: loupist.clone(),
            enabled: true,
            schedule: None,
            edition: None,
        },
    )
    .unwrap();
    assert_eq!(again.task.unwrap().id, task.id);
    let paused = commands::settings_set(
        &state,
        commands::SettingsRequest {
            loupist_id: loupist.clone(),
            enabled: false,
            schedule: None,
            edition: None,
        },
    )
    .unwrap();
    assert!(!paused.enabled);
    assert_eq!(paused.task.unwrap().id, task.id);
    assert_eq!(state.store.scheduled_tasks_list(&loupist).unwrap().len(), 1);
}

#[test]
fn a_schedule_saved_while_off_is_kept_without_turning_briefings_on() {
    let (_root, state, loupist) = state_fixture();
    let schedule = crate::scheduled_tasks::types::Schedule::Weekly {
        days: vec![0, 6],
        hour: 9,
        minute: 30,
        timezone: "Asia/Seoul".into(),
    };
    let saved = commands::settings_set(
        &state,
        commands::SettingsRequest {
            loupist_id: loupist.clone(),
            enabled: false,
            schedule: Some(schedule.clone()),
            edition: None,
        },
    )
    .unwrap();
    assert!(!saved.enabled);
    assert_eq!(saved.schedule, schedule);
    let reread = commands::settings_get(&state, &loupist).unwrap();
    assert!(!reread.enabled);
    assert_eq!(reread.schedule, schedule);
    assert_eq!(
        reread.task.unwrap().status,
        crate::scheduled_tasks::types::TaskStatus::Paused
    );
}

#[test]
fn tray_actions_focus_select_quit_and_residency_controls_close() {
    use std::cell::RefCell;
    struct Fake(RefCell<Vec<String>>);
    impl desktop::ResidentEffects for Fake {
        fn focus(&self) {
            self.0.borrow_mut().push("focus".into());
        }
        fn select(&self, target: desktop::InboxOpen) {
            self.0
                .borrow_mut()
                .push(format!("select:{}", target.loupist_id));
        }
        fn quit(&self) {
            self.0.borrow_mut().push("quit".into());
        }
        fn hide(&self) {
            self.0.borrow_mut().push("hide".into());
        }
    }
    let fake = Fake(RefCell::new(Vec::new()));
    desktop::tray_action(&fake, "briefing-open");
    desktop::tray_action(&fake, "briefing-loupist:loupist-a");
    desktop::tray_action(&fake, "briefing-quit");
    assert!(!desktop::close_window(&fake, false));
    assert!(desktop::close_window(&fake, true));
    assert_eq!(
        fake.0.borrow().as_slice(),
        ["focus", "focus", "select:loupist-a", "quit", "hide"]
    );
    assert!(!desktop::ResidentSettings::default().keep_running);
}

#[test]
fn opt_in_keeps_an_unset_model_so_the_briefing_can_follow_the_catalog() {
    let (_root, state, loupist) = state_fixture();
    let model = |provider: &str, name: &str| crate::providers::catalog::ConfiguredModel {
        id: format!("{provider}/{name}"),
        name: name.into(),
        provider: provider.into(),
        model: name.into(),
        input: vec!["text".into()],
        reasoning: true,
        context_window: 128_000,
        max_tokens: 32_000,
        thinking_levels: vec!["low".into(), "medium".into(), "high".into()],
        thinking_level_map: std::collections::BTreeMap::new(),
        run_controls: vec![],
    };
    state
        .set_model_catalog(crate::providers::catalog::ModelCatalog {
            models: vec![
                model("anthropic", "claude-fixture"),
                model("openai-codex", "gpt-fixture"),
            ],
        })
        .unwrap();
    connect_fixture_accounts(&state, &["anthropic", "openai-codex"]);
    let task = commands::settings_set(
        &state,
        commands::SettingsRequest {
            loupist_id: loupist.clone(),
            enabled: true,
            schedule: None,
            edition: None,
        },
    )
    .unwrap()
    .task
    .unwrap();
    assert!(task.model_profile_id.is_empty());
    let first = task.model_selection.unwrap().model.unwrap().id;
    state.set_model_visible(&first, false).unwrap();
    let current = commands::settings_get(&state, &loupist)
        .unwrap()
        .task
        .unwrap();
    assert_eq!(current.id, task.id);
    assert_eq!(current.revision, task.revision);
    assert!(current.model_profile_id.is_empty());
    let selection = current.model_selection.unwrap();
    assert!(selection.is_default);
    assert_ne!(selection.model.unwrap().id, first);
}

#[test]
fn briefing_settings_take_an_exact_schedule_while_saved_schedules_read_leniently() {
    use crate::scheduled_tasks::types::Schedule;
    use serde_json::json;
    let samples = [
        Schedule::Once { at_ms: 1 },
        Schedule::Interval { every_minutes: 5 },
        Schedule::Daily {
            hour: 9,
            minute: 0,
            timezone: "UTC".into(),
        },
        Schedule::Weekly {
            days: vec![1],
            hour: 9,
            minute: 0,
            timezone: "UTC".into(),
        },
    ];
    for schedule in samples {
        // A new kind needs an exact twin in `commands::ExactSchedule`, then a sample here.
        match &schedule {
            Schedule::Once { .. }
            | Schedule::Interval { .. }
            | Schedule::Daily { .. }
            | Schedule::Weekly { .. } => {}
        }
        let request = json!({ "loupistId": "loupist-a", "enabled": true, "schedule": schedule });
        let parsed: commands::SettingsRequest = serde_json::from_value(request.clone()).unwrap();
        assert_eq!(parsed.schedule.as_ref(), Some(&schedule));
        let mut extra = request;
        extra["schedule"]["jitter_minutes"] = json!(5);
        assert!(serde_json::from_value::<commands::SettingsRequest>(extra).is_err());
        let mut saved = serde_json::to_value(&schedule).unwrap();
        saved["jitter_minutes"] = json!(5);
        assert_eq!(serde_json::from_value::<Schedule>(saved).unwrap(), schedule);
    }
    let off: commands::SettingsRequest =
        serde_json::from_value(json!({ "loupistId": "loupist-a", "enabled": false })).unwrap();
    assert!(off.schedule.is_none());
}

#[test]
fn resuming_a_briefing_or_custom_task_enables_the_automatic_resident_setting() {
    for custom in [false, true] {
        let (fixture, mut state, loupist) = state_fixture();
        let path = fixture.path().to_path_buf();
        connect_fixture_accounts(&state, &["openai"]);
        // Status admission checks the runtime configuration without starting Pi.
        std::sync::Arc::make_mut(&mut state.artifacts).pi = Some(crate::app::PiArtifacts {
            executable: path.clone(),
            runtime_dir: path.clone(),
            extension: path.clone(),
            provider_extension: path.clone(),
            system_prompt: path,
            provider: String::new(),
            model: String::new(),
            thinking_level: String::new(),
            run_options: Default::default(),
            provider_credential: None,
        });
        let root = &state.artifacts.app_data_dir;
        let task = commands::settings_set(
            &state,
            commands::SettingsRequest {
                loupist_id: loupist.clone(),
                enabled: false,
                schedule: Some(commands::settings_get(&state, &loupist).unwrap().schedule),
                edition: None,
            },
        )
        .unwrap()
        .task
        .unwrap();
        let task = if custom {
            crate::scheduled_tasks::service::create(&state, &loupist, task.draft(), None).unwrap()
        } else {
            task
        };
        assert!(!desktop::read_settings(root).unwrap().keep_running);
        crate::scheduled_tasks::service::set_status(
            &state,
            &loupist,
            &task.id,
            task.revision,
            crate::scheduled_tasks::types::TaskStatus::Active,
        )
        .unwrap();
        assert!(desktop::read_settings(root).unwrap().keep_running);
    }
}

#[test]
fn resident_choice_survives_briefing_enable_and_disable() {
    for choice in [None, Some(false), Some(true)] {
        let (_root, state, loupist) = state_fixture();
        let root = &state.artifacts.app_data_dir;
        let initial = desktop::read_settings(root).unwrap();
        assert!(!initial.keep_running);
        assert_eq!(initial.keep_running_choice, None);
        if let Some(choice) = choice {
            super::login_item::apply(&state, super::login_item::Action::MenuBar(choice)).unwrap();
        }
        for enabled in [true, false, true, false] {
            commands::settings_set(
                &state,
                commands::SettingsRequest {
                    loupist_id: loupist.clone(),
                    enabled,
                    schedule: None,
                    edition: None,
                },
            )
            .unwrap();
            let settings = desktop::read_settings(root).unwrap();
            assert_eq!(settings.keep_running, choice.unwrap_or(true));
            assert_eq!(settings.keep_running_choice, choice);
            let response = serde_json::to_value(&settings).unwrap();
            assert_eq!(
                response["keepRunningChoice"],
                serde_json::to_value(choice).unwrap()
            );
        }
    }
}

#[test]
fn old_resident_values_load_and_remain_the_users_choice() {
    for keep_running in [false, true] {
        let (_root, state, _loupist) = state_fixture();
        let root = &state.artifacts.app_data_dir;
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(
            root.join("briefings-settings.json"),
            serde_json::to_vec(&serde_json::json!({
                "keepRunning": keep_running, "notificationPermissionAsked": false,
            }))
            .unwrap(),
        )
        .unwrap();
        let loaded = desktop::read_settings(root).unwrap();
        assert_eq!(loaded.keep_running, keep_running);
        assert_eq!(loaded.keep_running_choice, Some(keep_running));
        desktop::enable(&state).unwrap();
        let reopened = desktop::read_settings(root).unwrap();
        assert_eq!(reopened.keep_running, keep_running);
        assert_eq!(reopened.keep_running_choice, Some(keep_running));
    }
}

#[test]
fn a_scheduled_briefing_reads_only_its_own_loupists_follows() {
    let (_root, state, loupist) = state_fixture();
    state
        .store
        .create_loupist(&crate::test_fixtures::loupist_profile("loupist-other"))
        .unwrap();
    state
        .store
        .seed_followed_listing(&loupist, "HANUL", "KRX", Some("Hanul Precision"))
        .unwrap();
    state
        .store
        .seed_followed_listing(
            "loupist-other",
            "MORROW",
            "NASDAQ",
            Some("Morrow Components"),
        )
        .unwrap();
    let (prompt, context) = build_prompt(
        &state,
        &loupist,
        1_800_000_000_000,
        crate::scheduled_tasks::types::BriefingEdition::Morning,
    )
    .unwrap();
    assert_eq!(context.symbols, ["Hanul Precision"]);
    assert!(prompt.contains("Hanul Precision"));
    assert!(!prompt.contains("Morrow"));
}

#[test]
fn a_briefing_dates_this_run_and_the_last_one_at_the_users_offset() {
    let (_root, state, loupist) = state_fixture();
    let (previous, now) = (1_799_000_000_000, 1_800_000_000_000);
    let task = commands::settings_set(
        &state,
        commands::SettingsRequest {
            loupist_id: loupist.clone(),
            enabled: true,
            schedule: None,
            edition: None,
        },
    )
    .unwrap()
    .task
    .unwrap();
    finish_run(
        &state,
        &loupist,
        &task.id,
        previous,
        previous + 1,
        RunStatus::Succeeded,
    );
    let (prompt, _) = build_prompt(
        &state,
        &loupist,
        now,
        crate::scheduled_tasks::types::BriefingEdition::Morning,
    )
    .unwrap();
    let data: serde_json::Value =
        serde_json::from_str(prompt.strip_prefix(PROMPT).unwrap().trim()).unwrap();
    let local = |at_ms| {
        chrono::DateTime::from_timestamp_millis(at_ms)
            .unwrap()
            .with_timezone(&chrono::Local)
            .to_rfc3339()
    };
    assert_eq!(data["date"], local(now));
    assert_eq!(data["previousBriefingAt"], local(previous));
    assert_eq!(data["timeSincePreviousBriefingMs"], now - previous);
    assert!(data.get("previousBriefingAtMs").is_none());
    assert_eq!(data["edition"], "morning");
}

fn finish_run(
    state: &AppState,
    loupist: &str,
    task_id: &str,
    started: i64,
    finished: i64,
    status: RunStatus,
) -> crate::scheduled_tasks::types::Run {
    let (claim, ()) = state
        .store
        .scheduled_task_claim_with(loupist, task_id, started, true, |_| Ok(()))
        .unwrap()
        .unwrap();
    state
        .store
        .scheduled_task_finish(loupist, &claim.run.run_id, status, None, finished)
        .unwrap()
}

#[test]
fn evening_edition_includes_only_background_runs_since_the_last_successful_briefing() {
    use crate::scheduled_tasks::types::*;
    let (_root, state, loupist) = state_fixture();
    let morning = commands::settings_set(
        &state,
        commands::SettingsRequest {
            loupist_id: loupist.clone(),
            enabled: true,
            schedule: None,
            edition: Some(BriefingEdition::Morning),
        },
    )
    .unwrap();
    let evening = commands::settings_set(
        &state,
        commands::SettingsRequest {
            loupist_id: loupist.clone(),
            enabled: true,
            schedule: None,
            edition: Some(BriefingEdition::Evening),
        },
    )
    .unwrap();
    let morning_task = morning.task.unwrap();
    let evening_task = evening.task.unwrap();
    assert_ne!(morning_task.id, evening_task.id);
    assert_eq!(morning_task.edition(), BriefingEdition::Morning);
    assert_eq!(evening_task.edition(), BriefingEdition::Evening);
    assert!(matches!(
        evening_task.schedule,
        Schedule::Weekly { hour: 18, .. }
    ));
    let still = commands::settings_for(&state, &loupist, BriefingEdition::Morning).unwrap();
    assert_eq!(still.task.unwrap().id, morning_task.id);
    let watch = state
        .store
        .scheduled_task_create(
            &loupist,
            TaskDraft {
                title: "Filings".into(),
                prompt: "Check filings.".into(),
                schedule: Schedule::Daily {
                    hour: 9,
                    minute: 0,
                    timezone: "UTC".into(),
                },
                target: TaskTarget::NewChat,
                model_profile_id: String::new(),
                thinking_level: String::new(),
                run_options: Default::default(),
                use_memory: true,
                update_memory: true,
                status: TaskStatus::Active,
            },
            1_000,
        )
        .unwrap();
    state
        .store
        .scheduled_task_set_delivery(
            &loupist,
            &watch.id,
            watch.revision,
            Some(DeliveryTarget::Briefing),
            None,
            1_100,
        )
        .unwrap();
    let before_morning = finish_run(
        &state,
        &loupist,
        &watch.id,
        1_200,
        1_300,
        RunStatus::Succeeded,
    );
    assert!(state
        .store
        .inbox_runs(&loupist, None, 10)
        .unwrap()
        .is_empty());
    let (_, morning) = build_prompt(&state, &loupist, 2_000, BriefingEdition::Morning).unwrap();
    assert_eq!(morning.background_runs[0].run_id, before_morning.run_id);
    finish_run(
        &state,
        &loupist,
        &morning_task.id,
        2_000,
        3_000,
        RunStatus::Succeeded,
    );
    let during_morning = finish_run(
        &state,
        &loupist,
        &watch.id,
        2_100,
        2_200,
        RunStatus::Succeeded,
    );
    finish_run(
        &state,
        &loupist,
        &evening_task.id,
        4_000,
        4_100,
        RunStatus::Failed,
    );

    let (_, context) = build_prompt(&state, &loupist, 5_000, BriefingEdition::Evening).unwrap();
    assert_eq!(context.edition, BriefingEdition::Evening);
    assert_eq!(context.background_runs.len(), 1);
    assert_eq!(context.background_runs[0].task, "Filings");
    assert_eq!(context.background_runs[0].task_id, watch.id);
    assert_eq!(context.background_runs[0].run_id, during_morning.run_id);
}

#[test]
fn research_titles_and_delivered_summaries_mark_omitted_text() {
    assert_eq!(
        crate::commands::fallback_chat_title(
            "Use only the retained QA Field Notes source about Deere"
        ),
        "Use only the retained QA Field Notes…"
    );
    let first = "research ".repeat(30);
    assert_eq!(
        summary(&format!("{first}income statement")),
        format!("{}income…", first)
    );
}
