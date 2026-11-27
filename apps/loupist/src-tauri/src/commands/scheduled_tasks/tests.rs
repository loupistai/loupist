use super::*;
use crate::{
    app::PiArtifacts,
    commands::tests::{profile, seed_profile},
    runs::coordinator::{RunKind, RunSpec, RunTarget},
    scheduled_tasks::types::{
        RunStatus, Schedule, TaskDraft, TaskKind, TaskStatus, TaskSubject, TaskTarget,
    },
};
use std::{collections::BTreeMap, sync::Arc};

fn draft(status: TaskStatus) -> TaskDraft {
    TaskDraft {
        title: "Daily review".into(),
        prompt: "Read the local report.".into(),
        schedule: Schedule::Interval { every_minutes: 1 },
        target: TaskTarget::NewChat,
        model_profile_id: "fixture".into(),
        thinking_level: "medium".into(),
        run_options: BTreeMap::new(),
        use_memory: false,
        update_memory: false,
        status,
    }
}

#[test]
fn different_schedules_share_four_worker_slots_without_overlapping_the_same_task() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    seed_profile(
        state.store.as_ref(),
        &profile("loupist-a", workspace.path(), 1),
    );
    let tasks = (0..5)
        .map(|_| {
            let mut definition = draft(TaskStatus::Active);
            definition.target = TaskTarget::Dedicated;
            state
                .store
                .scheduled_task_create("loupist-a", definition, 1)
                .unwrap()
        })
        .collect::<Vec<_>>();
    let claim = |task: &crate::scheduled_tasks::types::Task| {
        state
            .store
            .scheduled_task_claim_with("loupist-a", &task.id, 2, true, |claim| {
                state
                    .run_coordinator
                    .register(
                        RunSpec {
                            run_id: claim.run.run_id.clone(),
                            loupist_id: "loupist-a".into(),
                            kind: RunKind::Chat,
                            target: RunTarget::ChatThread(claim.run.thread_id.clone().unwrap()),
                        },
                        || Ok(()),
                    )
                    .map_err(|_| crate::store::StoreError::Conflict("worker capacity"))
            })
    };
    let mut running = tasks
        .iter()
        .take(4)
        .map(|task| claim(task).unwrap().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(state.run_coordinator.active_count(), 4);
    assert!(claim(&tasks[0]).unwrap().is_none());
    assert!(claim(&tasks[4]).is_err());
    assert!(state
        .store
        .scheduled_task_get("loupist-a", &tasks[4].id)
        .unwrap()
        .unwrap()
        .session_thread_id
        .is_none());
    drop(running.pop());
    assert!(claim(&tasks[4]).unwrap().is_some());
}

#[tokio::test]
async fn scheduled_dispatch_defers_capacity_before_claiming_or_creating_chat() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    let loupist = profile("loupist-scheduled", workspace.path(), 1);
    seed_profile(state.store.as_ref(), &loupist);
    let task = state
        .store
        .scheduled_task_create(&loupist.id, draft(TaskStatus::Active), 1)
        .unwrap();
    let registrations = (0..crate::runs::coordinator::MAX_MODEL_WORKERS)
        .map(|index| {
            state
                .run_coordinator
                .register(
                    RunSpec {
                        run_id: format!("busy-{index}"),
                        loupist_id: loupist.id.clone(),
                        kind: RunKind::Chat,
                        target: RunTarget::ChatThread(format!("busy-chat-{index}")),
                    },
                    || Ok(()),
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(state
        .scheduled
        .dispatch(&state, &loupist.id, &task.id, false)
        .await
        .unwrap()
        .is_none());
    assert!(state
        .store
        .scheduled_task_runs(&loupist.id, &task.id)
        .unwrap()
        .is_empty());
    assert_eq!(
        state
            .store
            .scheduled_task_get(&loupist.id, &task.id)
            .unwrap()
            .unwrap()
            .revision,
        task.revision
    );
    assert_eq!(
        state
            .scheduled
            .dispatch(&state, &loupist.id, &task.id, true)
            .await
            .unwrap_err()
            .code,
        "conflict"
    );
    drop(registrations);

    let _writer = state
        .register_run(
            "writer".into(),
            loupist.id.clone(),
            RunKind::MemoryWrite,
            RunTarget::MemoryWriteSession("session".into()),
        )
        .unwrap();
    assert!(state
        .scheduled
        .dispatch(&state, &loupist.id, &task.id, false)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        state
            .scheduled
            .dispatch(&state, &loupist.id, &task.id, true)
            .await
            .unwrap_err()
            .code
            .as_str(),
        "memory_busy"
    );
    assert!(state
        .store
        .scheduled_task_runs(&loupist.id, &task.id)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn no_account_during_dispatch_pauses_auto_and_resume_names_the_settings_page() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    let loupist = profile("account-free", workspace.path(), 1);
    seed_profile(&state.store, &loupist);
    state.replace_provider_models("openai", vec![]).unwrap();
    let mut auto = draft(TaskStatus::Active);
    auto.model_profile_id.clear();
    let task = state
        .store
        .scheduled_task_create(&loupist.id, auto, 1)
        .unwrap();
    let run = state
        .scheduled
        .dispatch(&state, &loupist.id, &task.id, false)
        .await
        .unwrap()
        .unwrap();
    let paused = service::get(&state, &loupist.id, &task.id).unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(paused.status, TaskStatus::Paused);
    assert!(paused.next_run_at_ms.is_none());
    let error = service::set_status(
        &state,
        &loupist.id,
        &task.id,
        paused.revision,
        TaskStatus::Active,
    )
    .unwrap_err();
    assert_eq!(error.code, "provider_unavailable");
    assert_eq!(
        error.message,
        "Connect an AI account in Settings › AI accounts before chatting."
    );
    assert_eq!(run.error.as_deref(), Some(error.message.as_str()));
    assert_eq!(service::get(&state, &loupist.id, &task.id).unwrap(), paused);
}

#[tokio::test]
async fn scheduled_admission_failure_is_durable_and_not_retried_on_every_tick() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    let loupist = profile("loupist-scheduled", workspace.path(), 1);
    seed_profile(state.store.as_ref(), &loupist);
    let task = state
        .store
        .scheduled_task_create(&loupist.id, draft(TaskStatus::Active), 1)
        .unwrap();
    // The test app has no Pi runtime. A real admission attempt must fail, not
    // merely create a thread and report the scheduled run as successful.
    let run = state
        .scheduled
        .dispatch(&state, &loupist.id, &task.id, false)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    assert!(run.error.is_some());
    assert!(run.finished_at_ms.is_some());
    assert_eq!(
        state
            .store
            .scheduled_task_get(&loupist.id, &task.id)
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Active
    );
    assert!(state
        .scheduled
        .dispatch(&state, &loupist.id, &task.id, false)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        state
            .store
            .scheduled_task_runs(&loupist.id, &task.id)
            .unwrap()
            .len(),
        1
    );
    assert!(state
        .scheduled
        .watch(&loupist.id, &task.id, &run.run_id, Channel::new(|_| Ok(())))
        .unwrap()
        .is_none());
    assert_eq!(state.run_coordinator.active_count(), 0);
}

#[tokio::test]
async fn scheduled_dispatch_requires_current_loupist_binding_and_shutdown_admission() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    let other_workspace = tempfile::tempdir().unwrap();
    seed_profile(
        state.store.as_ref(),
        &profile("loupist-a", workspace.path(), 1),
    );
    seed_profile(
        state.store.as_ref(),
        &profile("loupist-b", other_workspace.path(), 1),
    );
    let task = state
        .store
        .scheduled_task_create("loupist-a", draft(TaskStatus::Paused), 1)
        .unwrap();
    assert!(state
        .scheduled
        .dispatch(&state, "loupist-b", &task.id, true)
        .await
        .is_err());
    assert!(state
        .store
        .scheduled_task_runs("loupist-a", &task.id)
        .unwrap()
        .is_empty());
    state.scheduled.stop();
    assert_eq!(
        state
            .scheduled
            .dispatch(&state, "loupist-a", &task.id, true)
            .await
            .unwrap_err()
            .code,
        "conflict"
    );
    assert!(state
        .store
        .scheduled_task_runs("loupist-a", &task.id)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn deletion_quarantine_blocks_scheduled_commands_and_runs_until_cleared() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    let loupist = profile("loupist-scheduled-quarantine", workspace.path(), 1);
    seed_profile(state.store.as_ref(), &loupist);
    let task = state
        .store
        .scheduled_task_create(&loupist.id, draft(TaskStatus::Active), 1)
        .unwrap();
    state.quarantine_loupist(
        &loupist.id,
        crate::app::QuarantineSource::Deletion,
        "pending deletion",
    );
    assert_eq!(
        service::list(&state, &loupist.id).unwrap_err().code,
        "loupist_storage_unavailable"
    );
    for status in [TaskStatus::Paused, TaskStatus::Active] {
        assert_eq!(
            service::set_status(&state, &loupist.id, &task.id, task.revision, status)
                .unwrap_err()
                .code,
            "loupist_storage_unavailable"
        );
    }
    assert_eq!(
        service::delete(&state, &loupist.id, &task.id, task.revision)
            .unwrap_err()
            .code,
        "loupist_storage_unavailable"
    );
    assert_eq!(
        state
            .scheduled
            .dispatch(&state, &loupist.id, &task.id, true)
            .await
            .unwrap_err()
            .code,
        "loupist_storage_unavailable"
    );
    assert!(state
        .store
        .scheduled_task_runs(&loupist.id, &task.id)
        .unwrap()
        .is_empty());
    state.clear_loupist_quarantine(&loupist.id, crate::app::QuarantineSource::Deletion);
    assert_eq!(service::list(&state, &loupist.id).unwrap().len(), 1);
    let paused = service::set_status(
        &state,
        &loupist.id,
        &task.id,
        task.revision,
        TaskStatus::Paused,
    )
    .unwrap();
    assert_eq!(paused.status, TaskStatus::Paused);
    service::delete(&state, &loupist.id, &task.id, paused.revision).unwrap();
}

#[test]
fn resident_command_persists_the_setting_across_profile_restart() {
    use tauri::Manager;
    const CHILD_ENV: &str = "LOUPIST_TEST_RESIDENT_COMMAND_DIR";
    if let Some(profile) = std::env::var_os(CHILD_ENV) {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        app.manage(AppState::for_persistent_test(profile.into()));
        let settings = crate::briefings::desktop::briefing_resident_settings_set(
            serde_json::from_value(serde_json::json!({"keepRunning": false})).unwrap(),
            app.state(),
        )
        .unwrap();
        assert!(!settings.resident.keep_running);
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("app");
    // Tauri's mock manager can retain its AppState. A joined child establishes
    // a real process exit and lock-release boundary without unsafe unmanage.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "commands::scheduled_tasks::tests::resident_command_persists_the_setting_across_profile_restart"])
        .env(CHILD_ENV, &profile).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let reopened = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    reopened.manage(AppState::for_persistent_test(profile));
    assert!(
        !crate::briefings::desktop::briefing_resident_settings_get(reopened.state())
            .unwrap()
            .resident
            .keep_running
    );
}

fn asset_daily_fixture(root: &tempfile::TempDir) -> (AppState, String) {
    learning_fixture(root, false)
}

/// `folder` opens the Loupist as a folder, as the app does, so its own config
/// governs its learning settings.
fn learning_fixture(root: &tempfile::TempDir, folder: bool) -> (AppState, String) {
    let agent = root.path().join("agent");
    std::fs::create_dir_all(agent.join("learning-presets")).unwrap();
    std::fs::write(agent.join("SYSTEM.md"), "fixture").unwrap();
    std::fs::write(
        agent.join("learning-presets/asset-daily.json"),
        r#"{
          "id": "asset-daily",
          "title": "Track {asset} ({reference})",
          "brief": "Follow {asset}.",
          "slots": {},
          "subject_kind": "asset",
          "schedule": {"kind":"weekly","days":[1,2,3,4,5],"hour":6,"minute":30},
          "program": {
            "depth": "standard",
            "scope": {"connections": "all_enabled", "web": true},
            "keeps": {"sources": true, "lens": true, "asset_reads": true}
          },
          "report": "when_new",
          "delivery": {"to": "briefing"}
        }"#,
    )
    .unwrap();
    let workspace = root.path().join("workspace");
    let mut state = if folder {
        let state = AppState::for_folder_test(root.path().join("app"));
        crate::test_fixtures::create_folder_loupist(&state.store, "loupist-a", &workspace);
        state
    } else {
        let state = AppState::for_test(root.path().join("app"));
        std::fs::create_dir_all(&workspace).unwrap();
        seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
        state
    };
    let directory = crate::commands::loupist_config_directory(&state, "loupist-a").unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("config.toml"),
        "[chat]\nmodel_profile_id = \"fixture\"\nthinking_level = \"medium\"\n",
    )
    .unwrap();
    let asset = state
        .store
        .seed_followed_listing("loupist-a", "AAA", "NASDAQ", Some("Northwind"))
        .unwrap();
    let blank = root.path().join("blank");
    Arc::make_mut(&mut state.artifacts).pi = Some(PiArtifacts {
        executable: blank.clone(),
        runtime_dir: blank.clone(),
        extension: blank.clone(),
        provider_extension: blank.clone(),
        system_prompt: agent.join("SYSTEM.md"),
        provider: String::new(),
        model: String::new(),
        thinking_level: String::new(),
        run_options: BTreeMap::new(),
        provider_credential: None,
    });
    (state, asset.asset_id)
}

#[tokio::test]
async fn a_learning_preset_fills_its_slots_and_starts_the_first_run() {
    let root = tempfile::tempdir().unwrap();
    let (state, asset_id) = asset_daily_fixture(&root);
    let request = serde_json::from_value::<PresetCommand>(serde_json::json!({
        "loupistId": "loupist-a", "preset": "asset-daily", "subjects": [{ "asset": asset_id }]
    }))
    .unwrap();
    let task = open_preset(request.clone(), &state).await.unwrap();
    assert_eq!(task.kind, TaskKind::Learning);
    assert_eq!(task.watch.preset, Some(WatchPreset::AssetDaily));
    assert_eq!(task.status, TaskStatus::Active);
    assert_eq!(task.title, "Track Northwind (AAA)");
    assert_eq!(task.prompt, "Follow Northwind.");
    assert!(task.use_memory && task.update_memory);
    assert_eq!(
        task.watch.subjects.as_ref().and_then(|items| items.first()),
        Some(&TaskSubject::Asset {
            asset: asset_id.clone(),
        })
    );
    let runs = service::runs(&state, "loupist-a", &task.id).unwrap();
    assert_eq!(runs.len(), 1);
    let again = open_preset(
        PresetCommand {
            subject: request
                .subjects
                .as_ref()
                .and_then(|subjects| subjects.first().cloned()),
            subjects: None,
            ..request
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(again.id, task.id);
    assert_eq!(
        service::runs(&state, "loupist-a", &task.id).unwrap().len(),
        1
    );
    let missing = serde_json::from_value::<PresetCommand>(serde_json::json!({
        "loupistId": "loupist-a", "preset": "not-a-preset", "subject": { "asset": asset_id }
    }));
    assert!(missing.is_err());
}

#[test]
fn every_learning_preset_is_served_with_its_slots_in_the_users_zone() {
    let root = tempfile::tempdir().unwrap();
    let (mut state, asset) = asset_daily_fixture(&root);
    let bundled = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../agent/SYSTEM.md");
    Arc::make_mut(&mut state.artifacts)
        .pi
        .as_mut()
        .unwrap()
        .system_prompt = bundled;
    let zone = chrono_tz::America::Sao_Paulo;
    let slots = |pairs: &[(&str, &str)]| {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect::<BTreeMap<_, _>>()
    };
    for (preset, subjects, filled) in [
        (
            LearningPresetId::Theme,
            None,
            slots(&[("theme", "Hidrogênio verde")]),
        ),
        (
            LearningPresetId::CompanyFilings,
            Some(vec![TaskSubject::Asset {
                asset: asset.clone(),
            }]),
            slots(&[]),
        ),
        (
            LearningPresetId::MacroSeries,
            None,
            slots(&[("series", "Fiktive Industrieproduktion")]),
        ),
        (
            LearningPresetId::Question,
            None,
            slots(&[("question", "가상 항만의 물동량은 회복되는가?")]),
        ),
        (
            LearningPresetId::Market,
            None,
            slots(&[("market", "Bourse fictive de Dakar")]),
        ),
    ] {
        let id = preset.as_str();
        let open = || {
            open_definition(
                "loupist-a",
                PresetId::Learning(preset),
                subjects.clone(),
                None,
                Some(filled.clone()),
                &state,
            )
        };
        let task = learning_from_preset(
            "loupist-a",
            preset,
            subjects.clone(),
            filled.clone(),
            &state,
            zone,
        )
        .unwrap();
        assert_eq!(task.kind, TaskKind::Learning, "{id}");
        assert_eq!(task.program.as_ref().unwrap().preset.as_deref(), Some(id));
        assert_eq!(task.watch.preset, None, "{id} is not scoped as a watch");
        assert!(matches!(
            &task.schedule,
            Schedule::Daily { timezone, .. } | Schedule::Weekly { timezone, .. }
                if timezone == "America/Sao_Paulo"
        ));
        assert_eq!(open().unwrap().id, task.id, "{id} reopens its program");
    }
    let unknown = open_definition(
        "loupist-a",
        PresetId::Learning(LearningPresetId::Market),
        None,
        None,
        Some(slots(&[("market", "Bourse fictive"), ("currency", "XOF")])),
        &state,
    )
    .unwrap_err();
    assert_eq!(unknown.code, crate::app::CommandErrorCode::PresetSlot);
    let watch = open_definition(
        "loupist-a",
        PresetId::Watch(WatchPreset::ThesisLines),
        None,
        None,
        Some(slots(&[("market", "Bourse fictive")])),
        &state,
    )
    .unwrap_err();
    assert_eq!(watch.code, crate::app::CommandErrorCode::PresetSlot);
    for (wire, preset) in [
        ("market", PresetId::Learning(LearningPresetId::Market)),
        ("asset-daily", PresetId::Watch(WatchPreset::AssetDaily)),
    ] {
        let command = serde_json::from_value::<PresetCommand>(serde_json::json!({
            "loupistId": "loupist-a", "preset": wire, "slots": {"market": "Bourse fictive"}
        }))
        .unwrap();
        assert_eq!(command.preset, preset);
    }
}

#[tokio::test]
async fn the_loupist_learning_settings_hold_runs_at_zero_and_cap_active_programs() {
    let root = tempfile::tempdir().unwrap();
    let (state, asset) = learning_fixture(&root, true);
    let directory = crate::commands::loupist_config_directory(&state, "loupist-a").unwrap();
    let write = |extra: &str| {
        std::fs::write(
            directory.join("config.toml"),
            format!(
                "{extra}\n[chat]\nmodel_profile_id = \"fixture\"\nthinking_level = \"medium\"\n"
            ),
        )
        .unwrap()
    };
    let configure = |set: crate::scoped_config::ConfigDocument| {
        let scope = crate::scoped_config::ConfigScope::Project {
            loupist_id: "loupist-a".into(),
        };
        let prior = crate::scoped_config::read_scope(
            &state.artifacts.app_data_dir,
            Some(&directory),
            scope.clone(),
        )
        .unwrap();
        super::super::configuration::update_config(
            crate::scoped_config::ConfigUpdateRequest {
                scope,
                expected_revision: prior.revision,
                set,
                unset: vec![],
            },
            &state,
        )
    };
    let refused = configure(crate::scoped_config::ConfigDocument {
        learning_daily_requests: Some(5),
        ..Default::default()
    })
    .unwrap_err();
    assert_eq!(refused.code, "invalid_configuration");
    let detail = refused.detail.unwrap();
    assert_eq!(detail["reason"], "below_learning_update");
    assert_eq!(
        detail["minimum"],
        crate::scheduled_tasks::types::LearningProgram::light_update_hard_budget()
    );
    let saved = configure(crate::scoped_config::ConfigDocument {
        learning_daily_requests: Some(0),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(saved.effective.learning_daily_requests, 0);

    let task = from_preset(
        FromPresetRequest {
            loupist_id: "loupist-a".into(),
            preset: WatchPreset::AssetDaily,
            subjects: Some(vec![TaskSubject::Asset { asset }]),
            params: None,
        },
        &state,
    )
    .unwrap();
    for _ in 0..2 {
        let held = service::run_now(&state, "loupist-a", &task.id, false)
            .await
            .unwrap_err();
        assert_eq!(held.code, "conflict");
        assert_eq!(held.detail.unwrap()["hold"]["reason"], "allowance");
    }
    assert!(service::runs(&state, "loupist-a", &task.id)
        .unwrap()
        .is_empty());

    write("learning_program_cap = 2");
    let program = |slot: &str| {
        learning_from_preset(
            "loupist-a",
            LearningPresetId::Theme,
            None,
            [("theme".to_owned(), slot.to_owned())].into(),
            &state,
            chrono_tz::Africa::Nairobi,
        )
    };
    let bundled = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../agent");
    std::fs::copy(
        bundled.join("learning-presets/theme.json"),
        root.path().join("agent/learning-presets/theme.json"),
    )
    .unwrap();
    program("Kilimo cha kidijitali").unwrap();
    let error = program("Logistique fluviale").unwrap_err();
    assert!(
        error
            .message
            .contains("up to 2 learning programs can be active"),
        "{error:?}"
    );
}

#[test]
fn asset_daily_activation_schedules_the_next_weekday_at_0630_in_the_local_zone() {
    use chrono::{Datelike, TimeZone, Timelike};

    let root = tempfile::tempdir().unwrap();
    let (state, asset) = asset_daily_fixture(&root);
    let zone = chrono_tz::America::New_York;
    let task = learning_from_preset(
        "loupist-a",
        LearningPresetId::AssetDaily,
        Some(vec![TaskSubject::Asset { asset }]),
        BTreeMap::new(),
        &state,
        zone,
    )
    .unwrap();
    assert_eq!(
        task.schedule,
        Schedule::Weekly {
            days: vec![1, 2, 3, 4, 5],
            hour: 6,
            minute: 30,
            timezone: zone.to_string(),
        }
    );
    let next_ms = task.next_run_at_ms.unwrap();
    assert!(next_ms > task.created_at_ms);
    let next = zone.timestamp_millis_opt(next_ms).unwrap();
    assert_eq!((next.hour(), next.minute()), (6, 30));
    assert!(next.weekday().number_from_monday() <= 5);
}

#[tokio::test]
async fn run_now_confirmation_admits_a_held_learning_program_with_its_frozen_budget() {
    use serde_json::{from_value, json};
    use tauri::Manager;

    let directory = tempfile::tempdir().unwrap();
    let (mut state, asset) = asset_daily_fixture(&directory);
    let task = from_preset(
        FromPresetRequest {
            loupist_id: "loupist-a".into(),
            preset: WatchPreset::AssetDaily,
            subjects: Some(vec![TaskSubject::Asset { asset }]),
            params: None,
        },
        &state,
    )
    .unwrap();
    let owner = &task.loupist_id;
    // No worker can start. The receipt distinguishes admission from launch.
    std::sync::Arc::make_mut(&mut state.artifacts).pi = None;
    let mut used = crate::store::model_usage::fixture_row(super::super::now_ms());
    used.loupist_id = Some(owner.clone());
    used.data.surface = crate::runs::log::UsageSurface::Learning;
    used.data.measures.requests = Some(132);
    state
        .store
        .put_model_usage("spent-learning-allowance", &used)
        .unwrap();

    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    app.manage(state);
    let mut request = json!({"loupist_id": owner, "task_id": task.id});
    for confirm in [None, Some(false), Some(true), None] {
        if let Some(confirm) = confirm {
            request["confirm"] = json!(confirm);
        } else {
            request.as_object_mut().unwrap().remove("confirm");
        }
        let result =
            scheduled_task_run_now(from_value(request.clone()).unwrap(), app.state()).await;
        if confirm == Some(true) {
            let run = result.unwrap();
            assert_eq!(run.task_id, task.id);
            assert_eq!(run.status, RunStatus::Failed);
            let budget = run.budget.unwrap();
            let levels = task.budget.unwrap();
            assert_eq!(
                (budget.soft, budget.hard, budget.left_at_start),
                (levels.soft, levels.hard, 0)
            );
            assert_eq!(
                budget.requests,
                levels.hard + crate::runs::budget::GRACE_ROUNDS
            );
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.code, "conflict");
            let detail = error.detail.unwrap();
            assert_eq!(detail["action"], "ask_first");
            assert_eq!(detail["hold"]["reason"], "allowance");
        }
    }
    assert_eq!(
        service::runs(&app.state(), owner, &task.id).unwrap().len(),
        1
    );
}
