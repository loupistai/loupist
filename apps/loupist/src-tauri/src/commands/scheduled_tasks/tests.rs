use super::*;
use crate::{
    commands::tests::{profile, seed_profile},
    runs::coordinator::{RunKind, RunSpec, RunTarget},
    scheduled_tasks::types::{RunStatus, Schedule, TaskDraft, TaskStatus, TaskTarget},
};
use std::collections::BTreeMap;

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
}

/// Followed assets refresh only after a briefing owns an admitted occurrence;
/// stopping the scheduler prevents a new refresh.
#[tokio::test]
async fn a_briefing_refreshes_followed_assets_after_admission_and_stops_with_the_scheduler() {
    use crate::knowledge::assets;
    let directory = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    let loupist = profile("loupist-scheduled", workspace.path(), 1);
    seed_profile(state.store.as_ref(), &loupist);
    state
        .store
        .watchlist_add("MORR", "NASDAQ", Some("Morrow Components"))
        .unwrap();
    let mut definition = draft(TaskStatus::Active);
    definition.title = "Briefings".into();
    definition.target = TaskTarget::Dedicated;
    (definition.use_memory, definition.update_memory) = (true, true);
    let task = state
        .store
        .scheduled_task_create_kind(
            &loupist.id,
            "briefing",
            definition,
            crate::scheduled_tasks::types::TaskKind::Briefing,
            1,
        )
        .unwrap();
    let asset_id = assets::asset_id("MORR", "NASDAQ");
    let refreshed = |state: &AppState| {
        crate::commands::assets::with_assets(state, &loupist.id, false, |tx| {
            assets::get_page(tx, &loupist.id, &asset_id)
        })
        .unwrap()
        .is_some_and(|page| page.refreshed_at_ms.is_some())
    };
    // A blocked admission has performed neither refresh nor a durable claim.
    use std::future::Future;
    let held = state.scheduled.hold_admission().await;
    let dispatch = state
        .scheduled
        .dispatch(&state, &loupist.id, &task.id, true);
    tokio::pin!(dispatch);
    assert!(matches!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(dispatch.as_mut().poll(cx))).await,
        std::task::Poll::Pending
    ));
    assert!(!refreshed(&state));
    assert!(state
        .store
        .scheduled_task_runs(&loupist.id, &task.id)
        .unwrap()
        .is_empty());
    drop(held);
    // The refresh now runs once; the missing model runtime then fails the run.
    let run = dispatch.await.unwrap().unwrap();
    assert!(refreshed(&state));
    assert_eq!(run.status, RunStatus::Failed);
    assert!(state
        .scheduled
        .dispatch(&state, &loupist.id, &task.id, false)
        .await
        .unwrap()
        .is_none());

    // Once the scheduler stops, no refresh starts.
    let other = tempfile::tempdir().unwrap();
    let stopped = AppState::for_test(other.path().join("app"));
    seed_profile(stopped.store.as_ref(), &loupist);
    stopped
        .store
        .watchlist_add("MORR", "NASDAQ", Some("Morrow Components"))
        .unwrap();
    let mut definition = draft(TaskStatus::Active);
    definition.target = TaskTarget::Dedicated;
    (definition.use_memory, definition.update_memory) = (true, true);
    let task = stopped
        .store
        .scheduled_task_create_kind(
            &loupist.id,
            "briefing",
            definition,
            crate::scheduled_tasks::types::TaskKind::Briefing,
            1,
        )
        .unwrap();
    stopped.scheduled.stop();
    assert!(stopped
        .scheduled
        .dispatch(&stopped, &loupist.id, &task.id, true)
        .await
        .is_err());
    assert!(!refreshed(&stopped));
    let (_raised, stop) = tokio::sync::watch::channel(true);
    assert_eq!(
        crate::commands::assets::refresh_followed(
            &stopped,
            &loupist.id,
            crate::commands::assets::BRIEFING_BUDGET,
            Some(stop),
        )
        .await
        .unwrap(),
        0
    );
    assert!(!refreshed(&stopped));
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

#[tokio::test]
async fn run_now_rejects_an_active_claim_and_finishes_one_host_run_without_model_work() {
    use tauri::Manager;
    let directory = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    seed_profile(
        state.store.as_ref(),
        &profile("loupist-a", workspace.path(), 1),
    );
    let task = state
        .store
        .scheduled_task_create_kind(
            "loupist-a",
            "earnings-loupist-a",
            draft(TaskStatus::Paused),
            crate::scheduled_tasks::types::TaskKind::Earnings,
            1,
        )
        .unwrap();
    let (claim, live) = state
        .scheduled
        .claim_host_run(&state, "loupist-a", &task.id, true)
        .await
        .unwrap()
        .unwrap();
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    app.manage(state.clone());
    let request = || TaskRequest {
        loupist_id: "loupist-a".into(),
        task_id: task.id.clone(),
    };
    assert_eq!(
        scheduled_task_run_now(request(), app.state())
            .await
            .unwrap_err()
            .code,
        "conflict"
    );
    assert_eq!(
        state
            .store
            .scheduled_task_runs("loupist-a", &task.id)
            .unwrap()
            .len(),
        1
    );
    state
        .store
        .scheduled_task_finish(
            "loupist-a",
            &claim.run.run_id,
            RunStatus::Succeeded,
            None,
            claim.run.started_at_ms + 1,
        )
        .unwrap();
    drop(live);
    let run = scheduled_task_run_now(request(), app.state())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(
        state
            .store
            .scheduled_task_runs("loupist-a", &task.id)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn schedule_commands_pause_resume_and_delete_with_undelivered_history() {
    use tauri::Manager;
    let directory = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    seed_profile(
        state.store.as_ref(),
        &profile("loupist-a", workspace.path(), 1),
    );
    let host = state
        .store
        .scheduled_task_create_kind(
            "loupist-a",
            "earnings-loupist-a",
            draft(TaskStatus::Active),
            crate::scheduled_tasks::types::TaskKind::Earnings,
            1,
        )
        .unwrap();
    let task = state
        .store
        .scheduled_task_create("loupist-a", draft(TaskStatus::Paused), 1)
        .unwrap();
    let claim = state
        .store
        .scheduled_task_claim_with("loupist-a", &task.id, 2, true, |_| Ok(()))
        .unwrap()
        .unwrap()
        .0;
    state
        .store
        .scheduled_task_finish(
            "loupist-a",
            &claim.run.run_id,
            RunStatus::Succeeded,
            None,
            3,
        )
        .unwrap();
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    app.manage(state.clone());
    let paused = scheduled_task_set_status(
        StatusRequest {
            loupist_id: "loupist-a".into(),
            task_id: host.id.clone(),
            expected_revision: host.revision,
            status: TaskStatus::Paused,
        },
        app.state(),
    )
    .unwrap();
    assert_eq!(paused.status, TaskStatus::Paused);
    let resumed = scheduled_task_set_status(
        StatusRequest {
            loupist_id: "loupist-a".into(),
            task_id: host.id,
            expected_revision: paused.revision,
            status: TaskStatus::Active,
        },
        app.state(),
    )
    .unwrap();
    assert_eq!(resumed.status, TaskStatus::Active);
    let current = state
        .store
        .scheduled_task_get("loupist-a", &task.id)
        .unwrap()
        .unwrap();
    scheduled_task_delete(
        RevisionRequest {
            loupist_id: "loupist-a".into(),
            task_id: task.id.clone(),
            expected_revision: current.revision,
        },
        app.state(),
    )
    .unwrap();
    assert!(state
        .store
        .scheduled_task_get("loupist-a", &task.id)
        .unwrap()
        .is_none());
    assert!(state
        .store
        .scheduled_task_run_get("loupist-a", &claim.run.run_id)
        .unwrap()
        .is_none());
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
        assert!(!settings.keep_running);
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
            .keep_running
    );
}
