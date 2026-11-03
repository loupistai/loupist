use super::*;
use crate::{app::AppState, commands::loupist::create_folder_loupist};

#[test]
fn definition_edits_preserve_pending_and_overdue_occurrences() {
    let store = fixture();
    for schedule in [
        Schedule::Interval { every_minutes: 5 },
        Schedule::Once { at_ms: 301_000 },
        Schedule::Daily {
            hour: 0,
            minute: 5,
            timezone: "UTC".into(),
        },
    ] {
        let task = store
            .scheduled_task_create(
                "loupist-a",
                TaskDraft {
                    schedule,
                    ..draft()
                },
                1_000,
            )
            .unwrap();
        let mut current = task.clone();
        for now in [241_000, 601_000] {
            let mut edited = current.draft();
            edited.title = format!("Renamed at {now}");
            edited.prompt = "Updated instructions".into();
            edited.use_memory = false;
            current = store
                .scheduled_task_update("loupist-a", &current.id, current.revision, edited, now)
                .unwrap();
            assert_eq!(current.next_run_at_ms, task.next_run_at_ms);
        }
        assert!(store
            .scheduled_tasks_due_for_loupists(601_000, 100, &["loupist-a".into()])
            .unwrap()
            .iter()
            .any(|due| due.id == task.id));
        let changed = TaskDraft {
            schedule: Schedule::Interval { every_minutes: 10 },
            ..current.draft()
        };
        current = store
            .scheduled_task_update("loupist-a", &current.id, current.revision, changed, 601_000)
            .unwrap();
        assert_eq!(current.next_run_at_ms, Some(1_201_000));
        current = store
            .scheduled_task_set_status(
                "loupist-a",
                &current.id,
                current.revision,
                TaskStatus::Paused,
                602_000,
            )
            .unwrap();
        assert_eq!(current.next_run_at_ms, None);
        current = store
            .scheduled_task_set_status(
                "loupist-a",
                &current.id,
                current.revision,
                TaskStatus::Active,
                700_000,
            )
            .unwrap();
        assert_eq!(current.next_run_at_ms, Some(1_300_000));
    }
}

#[test]
fn scheduled_activity_retains_latest_terminal_receipts_with_loupist_scope() {
    let store = fixture();
    let task = store
        .scheduled_task_create("loupist-a", draft(), 1_000)
        .unwrap();
    let other = store
        .scheduled_task_create("loupist-b", draft(), 1_000)
        .unwrap();
    claim_task(&store, "loupist-b", &other.id, 2_000, true)
        .unwrap()
        .unwrap();
    let first = claim_task(&store, "loupist-a", &task.id, 2_000, true)
        .unwrap()
        .unwrap()
        .run;
    assert_eq!(
        store.scheduled_tasks_activity("loupist-a").unwrap(),
        vec![first.clone()]
    );
    store
        .scheduled_task_finish(
            "loupist-a",
            &first.run_id,
            RunStatus::Succeeded,
            None,
            2_000,
        )
        .unwrap();
    let second = claim_task(&store, "loupist-a", &task.id, 2_000, true)
        .unwrap()
        .unwrap()
        .run;
    store
        .scheduled_task_finish(
            "loupist-a",
            &second.run_id,
            RunStatus::Succeeded,
            None,
            2_001,
        )
        .unwrap();
    let activity = store.scheduled_tasks_activity("loupist-a").unwrap();
    assert_eq!(activity.len(), 1);
    assert_eq!(activity[0].run_id, second.run_id);
    assert_eq!(activity[0].status, RunStatus::Succeeded);
    assert_eq!(
        store.scheduled_tasks_activity("loupist-b").unwrap()[0].status,
        RunStatus::Running
    );
    let current = store
        .scheduled_task_get("loupist-a", &task.id)
        .unwrap()
        .unwrap();
    store
        .scheduled_task_delete("loupist-a", &task.id, current.revision)
        .unwrap();
    assert!(store
        .scheduled_tasks_activity("loupist-a")
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn returning_folder_recovers_orphans_without_interrupting_live_runs() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("research");
    std::fs::create_dir(&folder).unwrap();
    let app = temp.path().join("app");
    let state = AppState::for_folder_test(app.clone());
    let loupist = create_folder_loupist(&state, &folder).await.unwrap();
    let task = state
        .store
        .scheduled_task_create(&loupist.id, draft(), 1_000)
        .unwrap();
    let orphan = claim_task(&state.store, &loupist.id, &task.id, 2_000, true)
        .unwrap()
        .unwrap()
        .run;
    state.close_for_restart_test();
    let unavailable = temp.path().join("unmounted");
    std::fs::rename(&folder, &unavailable).unwrap();
    let state = AppState::for_folder_test(app);
    state.scheduled.poll(&state).await.unwrap();
    std::fs::rename(&unavailable, &folder).unwrap();
    let live_task = state
        .store
        .scheduled_task_create(&loupist.id, draft(), 1_000)
        .unwrap();
    let live = claim_task(&state.store, &loupist.id, &live_task.id, 2_000, true)
        .unwrap()
        .unwrap()
        .run;
    state.scheduled.inner.live.lock().unwrap().insert(
        live.run_id.clone(),
        super::super::LiveRun {
            run: live.clone(),
            baseline: None,
            replay: Default::default(),
            subscriber: None,
        },
    );
    // Update installation must block recovery writes as well as new starts.
    let maintenance = state.maintenance.begin_update().unwrap();
    assert_eq!(
        state.scheduled.poll(&state).await.unwrap_err().code,
        "maintenance_active"
    );
    assert_eq!(
        state
            .store
            .scheduled_task_run_get(&loupist.id, &orphan.run_id)
            .unwrap()
            .unwrap()
            .status,
        RunStatus::Running
    );
    drop(maintenance);
    state.scheduled.poll(&state).await.unwrap();
    assert_eq!(
        state
            .store
            .scheduled_task_run_get(&loupist.id, &orphan.run_id)
            .unwrap()
            .unwrap()
            .status,
        RunStatus::Interrupted
    );
    assert_eq!(
        state
            .store
            .scheduled_task_run_get(&loupist.id, &live.run_id)
            .unwrap()
            .unwrap()
            .status,
        RunStatus::Running
    );
    let recovered = state
        .store
        .scheduled_task_get(&loupist.id, &task.id)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.status, TaskStatus::Paused);
    state
        .store
        .scheduled_task_delete(&loupist.id, &task.id, recovered.revision)
        .unwrap();
    // A terminal callback may have disappeared while the folder was absent.
    state
        .scheduled
        .inner
        .live
        .lock()
        .unwrap()
        .remove(&live.run_id);
    state.scheduled.poll(&state).await.unwrap();
    assert_eq!(
        state
            .store
            .scheduled_task_run_get(&loupist.id, &live.run_id)
            .unwrap()
            .unwrap()
            .status,
        RunStatus::Interrupted
    );
}

#[tokio::test]
async fn unavailable_loupist_backlog_does_not_starve_healthy_schedules() {
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let blocked_folder = temp.path().join("blocked");
    let healthy_folder = temp.path().join("healthy");
    std::fs::create_dir(&blocked_folder).unwrap();
    std::fs::create_dir(&healthy_folder).unwrap();
    let blocked = create_folder_loupist(&state, &blocked_folder)
        .await
        .unwrap();
    let healthy = create_folder_loupist(&state, &healthy_folder)
        .await
        .unwrap();
    for _ in 0..MAX_TASKS_PER_LOUPIST {
        state
            .store
            .scheduled_task_create(&blocked.id, draft(), 1_000)
            .unwrap();
    }
    let task = state
        .store
        .scheduled_task_create(&healthy.id, draft(), 2_000)
        .unwrap();
    state.quarantine_loupist(
        &blocked.id,
        crate::app::QuarantineSource::MemoryWrite,
        "recover first",
    );
    state.scheduled.poll(&state).await.unwrap();
    // No configured model: a durable failure proves this Loupist reached admission.
    let runs = state
        .store
        .scheduled_task_runs(&healthy.id, &task.id)
        .unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, RunStatus::Failed);
    assert!(state
        .store
        .scheduled_tasks_activity(&blocked.id)
        .unwrap()
        .is_empty());
    state.scheduled.poll(&state).await.unwrap();
    assert_eq!(
        state
            .store
            .scheduled_task_runs(&healthy.id, &task.id)
            .unwrap()
            .len(),
        1
    );
}
