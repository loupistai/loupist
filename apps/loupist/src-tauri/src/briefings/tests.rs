use super::*;
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
        "plan_update",
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

#[test]
fn prompt_uses_only_symbols_and_bounded_relevant_growth_since_previous_run() {
    use crate::store::user_context::{HoldingsPosition, HoldingsSource};
    let (_root, state, loupist) = state_fixture();
    std::fs::write(
        state.artifacts.app_data_dir.join("config.toml"),
        "[appearance]\nlanguage = 'ko'\n",
    )
    .unwrap();
    state
        .store
        .watchlist_add(
            "AAA",
            "NASDAQ",
            Some("Watchlist display text must not become instructions"),
        )
        .unwrap();
    state
        .store
        .save_holdings_snapshot(
            HoldingsSource::Csv,
            vec![HoldingsPosition {
                symbol: "BBB".into(),
                market: "NYSE".into(),
                display_name: None,
                quantity: "123456789".into(),
                cost_basis: Some("998877".into()),
                market_value: Some("11223344".into()),
                currency: "USD".into(),
            }],
        )
        .unwrap();
    {
        let connection = state.store.lock_loupist(&loupist).unwrap();
        for index in 0..80 {
            connection.execute("INSERT INTO knowledge_growth_events(event_id,loupist_id,at_ms,kind,actor,summary) VALUES(?1,?2,?3,?4,'user',?5)",rusqlite::params![format!("event-{index}"),loupist,index+10,if index%2==0 {"claim_added"}else{"source_retained"},format!("Change {index}")]).unwrap();
        }
        connection.execute("INSERT INTO knowledge_growth_events(event_id,loupist_id,at_ms,kind,actor,summary) VALUES('ignored',?1,99,'entity_added','user','Ignored entity')",[&loupist]).unwrap();
        // The user's own "What you follow" Source is not news.
        connection.execute("INSERT INTO knowledge_records(record_id,kind,owner_loupist_id,title,created_at_ms) VALUES('rec_followed','source',?1,'What you follow',95)",[&loupist]).unwrap();
        connection.execute("INSERT INTO knowledge_sources(record_id,source_id,title,source_class,origin_locator,provider_identity,updated_at_ms) VALUES('rec_followed','src_followed','What you follow','user_statement','Your watchlist and holdings',?1,95)",[crate::knowledge::FOLLOWED_PROVIDER]).unwrap();
        connection.execute("INSERT INTO knowledge_growth_events(event_id,loupist_id,at_ms,kind,record_id,actor,summary) VALUES('followed',?1,95,'source_retained','rec_followed','user','What you follow')",[&loupist]).unwrap();
    }
    let (prompt, context) = build_prompt(&state, &loupist, Some(20), 100).unwrap();
    let data: serde_json::Value =
        serde_json::from_str(prompt.strip_prefix(PROMPT).unwrap().trim()).unwrap();
    assert!(data.get("watchlist").is_none());
    assert!(data.get("holdings").is_none());
    assert_eq!(context.symbols, vec!["AAA", "BBB"]);
    assert_eq!(context.new_facts, 40);
    assert_eq!(data["worldGrowth"].as_array().unwrap().len(), 40);
    assert!(data["worldGrowth"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["atMs"].as_i64().unwrap() > 20));
    assert_eq!(data["timeSincePreviousBriefingMs"], 80);
    assert_eq!(data["appLanguage"], "ko");
    for excluded in [
        "123456789",
        "998877",
        "11223344",
        "display text",
        "Ignored entity",
        "What you follow",
    ] {
        assert!(!prompt.contains(excluded));
    }
}

#[test]
fn briefing_prompt_marks_crypto_and_wallet_holdings_without_quantities_or_addresses() {
    let (_root, state, loupist) = state_fixture();
    let preview = state
        .store
        .preview_crypto_bytes(b"asset,balance\nXXBT,0.4271\n")
        .unwrap();
    state
        .store
        .holdings_commit_csv(&preview.preview_id)
        .unwrap();
    let address = "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM";
    let wallet = state.store.wallet_add(address, None).unwrap();
    state
        .store
        .wallet_save_read(
            &wallet.id,
            crate::store::user_context::wallets::WalletRead {
                balances: vec![("SOL".into(), "41.0700001".into())],
                unlisted: 0,
            },
        )
        .unwrap();
    let (prompt, context) = build_prompt(&state, &loupist, None, 100).unwrap();
    let data: serde_json::Value =
        serde_json::from_str(prompt.strip_prefix(PROMPT).unwrap().trim()).unwrap();
    assert!(data.get("holdings").is_none());
    assert_eq!(context.symbols, vec!["BTC", "SOL"]);
    for excluded in ["0.4271", "41.0700001", address, "9WzDXw"] {
        assert!(!prompt.contains(excluded), "{excluded}");
        assert!(
            !serde_json::to_string(&context).unwrap().contains(excluded),
            "{excluded}"
        );
    }
}

#[test]
fn briefing_definition_and_followed_companies_respect_context_bounds() {
    let (_root, state, loupist) = state_fixture();
    let instructions = "한😀".repeat(2_000);
    state
        .store
        .update_loupist_profile(
            &loupist,
            crate::store::LoupistProfileEdit {
                instructions: Some(&instructions),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    for index in (0..75).rev() {
        state
            .store
            .watchlist_add(&format!("F{index:02}"), "NYSE", None)
            .unwrap();
    }
    let (prompt, context) = build_prompt(&state, &loupist, None, 100).unwrap();
    let data: serde_json::Value =
        serde_json::from_str(prompt.strip_prefix(PROMPT).unwrap().trim()).unwrap();
    for key in [
        "Instructions from the user for this Loupist",
        "Loupist tone",
        "followedCompaniesHint",
        "watchlist",
        "holdings",
        "principles",
    ] {
        assert!(data.get(key).is_none(), "{key}");
    }
    assert_eq!(context.symbols.len(), 60);
    assert_eq!(context.symbols.first().unwrap(), "F00");
    assert_eq!(context.symbols.last().unwrap(), "F59");
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
fn notification_effects_include_loupist_status_summary_and_navigation_target() {
    use std::cell::RefCell;
    struct Fake {
        calls: RefCell<Vec<String>>,
    }
    impl desktop::DesktopEffects for Fake {
        fn notify(&self, title: &str, body: &str, target: &desktop::InboxOpen) {
            self.calls.borrow_mut().push(format!(
                "{title}|{body}|{}|{}",
                target.loupist_id,
                target.run_id.as_deref().unwrap_or("")
            ));
        }
        fn changed(&self, loupist: &str) {
            self.calls.borrow_mut().push(format!("changed:{loupist}"));
        }
    }
    let fake = Fake {
        calls: RefCell::new(Vec::new()),
    };
    let run = crate::scheduled_tasks::types::Run {
        briefing_context: None,
        kind: crate::scheduled_tasks::types::TaskKind::Briefing,
        read_at_ms: None,
        summary_items: Vec::new(),
        summary: "First line\nSecond line".into(),
        message_id: Some("message-a".into()),
        skipped_tools: vec![],
        id: "receipt-a".into(),
        task_id: "task-a".into(),
        loupist_id: "loupist-a".into(),
        task_revision: 1,
        task_title: "Briefings".into(),
        thread_id: Some("chat-a".into()),
        run_id: "run-a".into(),
        scheduled_for_ms: 1,
        started_at_ms: 1,
        finished_at_ms: Some(2),
        status: crate::scheduled_tasks::types::RunStatus::Succeeded,
        error: None,
    };
    desktop::deliver(&fake, "Research", &run);
    {
        let calls = fake.calls.borrow();
        assert!(calls[0].starts_with("Research"));
        assert!(calls[0].ends_with("|First line|loupist-a|run-a"));
        assert_eq!(calls[1], "changed:loupist-a");
    }
    use crate::scheduled_tasks::types::{RunStatus, TaskKind};
    for (kind, title) in [
        (TaskKind::Briefing, "briefing"),
        (TaskKind::Custom, "Weekly research"),
        (TaskKind::Earnings, "Results before and after"),
    ] {
        for (status, error, suffix) in [
            (RunStatus::Succeeded, None, "ready"),
            (RunStatus::Failed, None, "couldn't finish"),
            (RunStatus::Interrupted, None, "couldn't finish"),
            (
                RunStatus::Interrupted,
                Some("Interrupted before completion."),
                "couldn't finish",
            ),
            (
                RunStatus::Interrupted,
                Some("Scheduled run was interrupted."),
                "couldn't finish",
            ),
            (
                RunStatus::Interrupted,
                Some("Stopped before completion"),
                "stopped",
            ),
        ] {
            fake.calls.borrow_mut().clear();
            let finished = crate::scheduled_tasks::types::Run {
                kind: kind.clone(),
                task_title: title.into(),
                status,
                error: error.map(str::to_owned),
                ..run.clone()
            };
            desktop::deliver(&fake, "Research", &finished);
            let calls = fake.calls.borrow();
            assert_eq!(
                calls[0],
                format!("Research: {title} {suffix}|First line|loupist-a|run-a")
            );
            assert_eq!(calls[1], "changed:loupist-a");
        }
    }
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
fn briefing_context_is_persisted_on_the_run_and_projected_in_the_inbox() {
    let (_root, state, loupist) = state_fixture();
    state.store.watchlist_add("NVDA", "NASDAQ", None).unwrap();
    let task = commands::settings_set(
        &state,
        commands::SettingsRequest {
            loupist_id: loupist.clone(),
            enabled: true,
            schedule: None,
        },
    )
    .unwrap()
    .task
    .unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    let (claim, ()) = state
        .store
        .scheduled_task_claim_with(&loupist, &task.id, now, true, |_| Ok(()))
        .unwrap()
        .unwrap();
    let (_, context) = build_prompt(&state, &loupist, None, now).unwrap();
    state
        .store
        .scheduled_run_briefing_context(&loupist, &claim.run.run_id, context.clone())
        .unwrap();
    state
        .store
        .scheduled_task_finish(
            &loupist,
            &claim.run.run_id,
            crate::scheduled_tasks::types::RunStatus::Succeeded,
            None,
            now + 1,
        )
        .unwrap();
    let run = state
        .store
        .scheduled_task_runs(&loupist, &task.id)
        .unwrap()
        .remove(0);
    assert_eq!(run.briefing_context, Some(context.clone()));
    let inbox = commands::InboxItem::from(run);
    assert_eq!(inbox.briefing_context, Some(context));
    assert_eq!(inbox.briefing_context.unwrap().symbols, vec!["NVDA"]);
}

#[test]
fn disputed_context_filters_growth_window_and_one_hop_then_caps_at_twenty() {
    let (_root, state, loupist) = state_fixture();
    state.store.watchlist_add("FOLLOW", "NASDAQ", None).unwrap();
    let make_claim = |id: &str, source: &str, target: &str, status: &str| {
        serde_json::json!({
            "id":id,"proposition":"capacity report","attribution":"report","relation":"SUPPLIES",
            "endpoints":[{"nodeId":source,"role":"subject"},{"nodeId":target,"role":"object"}],
            "qualifiers":[],"evidence":[],"status":status
        })
    };
    let mut claims = vec![
        make_claim("link", "n00", "n01", "active"),
        make_claim("neighbor", "n01", "n02", "disputed"),
        make_claim("too-far", "n02", "n03", "disputed"),
        make_claim("old", "n00", "n04", "disputed"),
        make_claim("future", "n00", "n05", "disputed"),
        make_claim("resolved", "n00", "n06", "active"),
    ];
    for index in 7..35 {
        claims.push(make_claim(
            &format!("extra-{index}"),
            "n01",
            &format!("n{index:02}"),
            "disputed",
        ));
    }
    let snapshot = |claims: &[serde_json::Value]| {
        serde_json::json!({
            "annotations":[],
            "nodes":(0..35).map(|index|serde_json::json!({"id":format!("n{index:02}"),"name":format!("Company {index}"),"aliases":if index==0 {vec!["FOLLOW"]}else{vec![]},"description":"","kind":"company","sourceRole":"entity","properties":{}})).collect::<Vec<_>>(), "claims":claims
        })
    };
    {
        let db = state.store.lock_loupist(&loupist).unwrap();
        db.execute(
            "INSERT INTO knowledge_records VALUES ('world','world',?1,'World',1)",
            [&loupist],
        )
        .unwrap();
        db.execute("INSERT INTO knowledge_record_revisions VALUES ('revision','world','digest','{}','[]',1)",[]).unwrap();
        db.execute(
            "INSERT INTO knowledge_record_heads VALUES ('world','revision')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO knowledge_world_snapshots VALUES ('snapshot','world','revision','digest',?1,1)",[snapshot(&claims).to_string()]).unwrap();
        for (id, at, kind) in [
            ("neighbor", 50, "claim_disputed"),
            ("too-far", 50, "claim_disputed"),
            ("old", 20, "claim_disputed"),
            ("future", 101, "claim_disputed"),
            ("resolved", 50, "claim_added"),
        ] {
            db.execute("INSERT INTO knowledge_growth_events(event_id,loupist_id,at_ms,kind,record_id,subject_id,actor,summary) VALUES (?1,?2,?3,?4,'world',?1,'processing','changed')",rusqlite::params![id,loupist,at,kind]).unwrap();
        }
    }
    let (_, context) = build_prompt(&state, &loupist, Some(20), 100).unwrap();
    assert_eq!(context.disputed_entity_ids, vec!["n01", "n02"]);
    {
        let db = state.store.lock_loupist(&loupist).unwrap();
        for index in 7..35 {
            let id = format!("extra-{index}");
            db.execute("INSERT INTO knowledge_growth_events(event_id,loupist_id,at_ms,kind,record_id,subject_id,actor,summary) VALUES (?1,?2,100,'claim_added','world',?1,'processing','changed')",rusqlite::params![id,loupist]).unwrap();
        }
    }
    let (_, context) = build_prompt(&state, &loupist, Some(20), 100).unwrap();
    assert_eq!(context.disputed_entity_ids.len(), 20);
    assert_eq!(
        context
            .disputed_entity_ids
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        20
    );
    assert!(!context
        .disputed_entity_ids
        .iter()
        .any(|id| ["n03", "n04", "n05", "n06"].contains(&id.as_str())));
}

#[test]
fn opt_in_uses_the_model_a_new_chat_would_not_the_raw_catalog_order() {
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
    // The configuration lists a provider the user may never connect first.
    state
        .set_model_catalog(crate::providers::catalog::ModelCatalog {
            models: vec![
                model("anthropic", "claude-fixture"),
                model("openai-codex", "gpt-fixture"),
            ],
        })
        .unwrap();
    let picker_first = state.model_catalog_view().unwrap().models[0].id.clone();
    assert_eq!(picker_first, "openai-codex/gpt-fixture");
    let enable = |state: &AppState| {
        commands::settings_set(
            state,
            commands::SettingsRequest {
                loupist_id: loupist.clone(),
                enabled: true,
                schedule: None,
            },
        )
        .unwrap()
        .task
        .unwrap()
    };
    assert_eq!(enable(&state).model_profile_id, picker_first);

    // A hidden model is skipped, as the Chat picker skips it.
    let task = state
        .store
        .scheduled_tasks_list(&loupist)
        .unwrap()
        .remove(0);
    state
        .store
        .scheduled_task_delete(&loupist, &task.id, task.revision)
        .unwrap();
    state
        .set_model_visible("openai-codex/gpt-fixture", false)
        .unwrap();
    assert_eq!(enable(&state).model_profile_id, "anthropic/claude-fixture");
}

#[test]
fn shared_principles_are_not_duplicated_in_the_briefing_json() {
    let (_root, state, loupist_id) = state_fixture();
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../evals/sharing/trade-authority-principle");
    crate::sharing::commands::add::seed_shared_pack(&state, &loupist_id, &folder).unwrap();
    let (prompt, _) = super::build_prompt(
        &state,
        &loupist_id,
        None,
        chrono::Utc::now().timestamp_millis(),
    )
    .unwrap();
    let data: serde_json::Value =
        serde_json::from_str(prompt.strip_prefix(PROMPT).unwrap().trim()).unwrap();
    assert!(data.get("principles").is_none());
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
fn resident_choice_survives_briefing_enable_and_disable() {
    for choice in [None, Some(false), Some(true)] {
        let (_root, state, loupist) = state_fixture();
        let root = &state.artifacts.app_data_dir;
        let initial = desktop::read_settings(root).unwrap();
        assert!(!initial.keep_running);
        assert_eq!(initial.keep_running_choice, None);
        if let Some(choice) = choice {
            desktop::set_keep_running(&state, choice).unwrap();
        }
        for enabled in [true, false, true, false] {
            commands::settings_set(
                &state,
                commands::SettingsRequest {
                    loupist_id: loupist.clone(),
                    enabled,
                    schedule: None,
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
fn watching_only_activates_automatic_residency_without_an_explicit_choice() {
    use crate::{
        commands::watch::{update_setting, WatchSettingSetRequest},
        knowledge::watch::WatchKind,
    };
    for choice in [None, Some(false), Some(true)] {
        let (_root, state, loupist) = state_fixture();
        if let Some(choice) = choice {
            desktop::set_keep_running(&state, choice).unwrap();
        }
        state.store.watchlist_add("MORR", "NASDAQ", None).unwrap();
        let page = crate::commands::watch::followed_pages(&state, &loupist)
            .unwrap()
            .pop()
            .unwrap();
        update_setting(
            &state,
            WatchSettingSetRequest {
                loupist_id: loupist.clone(),
                asset_id: page.asset_id.clone(),
                kind: WatchKind::Filings,
                enabled: false,
                expected_revision: 1,
            },
        )
        .unwrap();
        assert_eq!(
            desktop::read_settings(&state.artifacts.app_data_dir)
                .unwrap()
                .keep_running,
            choice.unwrap_or(false)
        );
        let revision = crate::commands::assets::with_assets(&state, &loupist, false, |tx| {
            Ok(crate::knowledge::watch::switches(tx, &loupist)?
                .get(&page.asset_id)
                .map_or(1, |switch| switch.revision))
        })
        .unwrap();
        update_setting(
            &state,
            WatchSettingSetRequest {
                loupist_id: loupist,
                asset_id: page.asset_id,
                kind: WatchKind::Filings,
                enabled: true,
                expected_revision: revision,
            },
        )
        .unwrap();
        let settings = desktop::read_settings(&state.artifacts.app_data_dir).unwrap();
        assert_eq!(settings.keep_running, choice.unwrap_or(true));
        assert_eq!(settings.keep_running_choice, choice);
        assert!(!settings.notification_permission_asked);
    }
}

#[test]
fn briefing_coverage_names_unchecked_assets_and_failed_or_missing_routes() {
    use crate::knowledge::watch::{record_route, Route};
    let (_root, state, loupist) = state_fixture();
    state.store.watchlist_add("MORR", "NASDAQ", None).unwrap();
    let page = crate::commands::watch::followed_pages(&state, &loupist)
        .unwrap()
        .pop()
        .unwrap();
    crate::commands::assets::with_assets(&state, &loupist, true, |tx| {
        record_route(
            tx,
            &loupist,
            &page.asset_id,
            Route::Filings,
            "failed",
            "sec.edgar",
            100,
        )?;
        record_route(
            tx,
            &loupist,
            &page.asset_id,
            Route::Insiders,
            "needs_connection",
            "fmp.market-data",
            100,
        )?;
        record_route(
            tx,
            &loupist,
            &page.asset_id,
            Route::Calendar,
            "ok",
            "openbb",
            100,
        )
    })
    .unwrap();
    let (prompt, _) = build_prompt(&state, &loupist, None, 1_000_000).unwrap();
    let data: serde_json::Value =
        serde_json::from_str(prompt.strip_prefix(PROMPT).unwrap().trim()).unwrap();
    assert_eq!(data["coverage"]["notChecked"][0]["symbol"], "MORR");
    let routes = data["coverage"]["routes"].as_array().unwrap();
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().all(|route| route["status"] != "ok"));
    assert!(routes
        .iter()
        .any(|route| route["status"] == "needs_connection"));
}
