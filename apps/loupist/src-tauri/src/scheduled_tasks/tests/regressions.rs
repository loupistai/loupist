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

#[tokio::test]
async fn undecodable_running_receipt_does_not_block_other_loupists() {
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let mut loupists = Vec::new();
    for name in ["broken", "healthy"] {
        let folder = temp.path().join(name);
        std::fs::create_dir(&folder).unwrap();
        loupists.push(create_folder_loupist(&state, &folder).await.unwrap());
    }
    let mut runs = Vec::new();
    for loupist in &loupists {
        let task = state
            .store
            .scheduled_task_create(&loupist.id, draft(), 1_000)
            .unwrap();
        runs.push(
            claim_task(&state.store, &loupist.id, &task.id, 2_000, true)
                .unwrap()
                .unwrap()
                .run,
        );
    }
    state
        .store
        .lock_loupist(&loupists[0].id)
        .unwrap()
        .execute(
            "UPDATE scheduled_task_runs SET data_json='{}' WHERE id=?1",
            [&runs[0].id],
        )
        .unwrap();
    let local_task = state
        .store
        .scheduled_task_create(&loupists[0].id, draft(), 1_000)
        .unwrap();
    let local_orphan = claim_task(&state.store, &loupists[0].id, &local_task.id, 2_000, true)
        .unwrap()
        .unwrap()
        .run;
    let due = state
        .store
        .scheduled_task_create(&loupists[1].id, draft(), 1_000)
        .unwrap();

    state.scheduled.poll(&state).await.unwrap();
    assert_eq!(
        state
            .store
            .scheduled_task_run_get(&loupists[0].id, &local_orphan.run_id)
            .unwrap()
            .unwrap()
            .status,
        RunStatus::Interrupted
    );
    assert_eq!(
        state
            .store
            .scheduled_task_run_get(&loupists[1].id, &runs[1].run_id)
            .unwrap()
            .unwrap()
            .status,
        RunStatus::Interrupted
    );
    // Admission reaches the healthy task, whose unconfigured model fails locally.
    let receipts = state
        .store
        .scheduled_task_runs(&loupists[1].id, &due.id)
        .unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].status, RunStatus::Failed);
    state.scheduled.poll(&state).await.unwrap();
    let status: String = state
        .store
        .lock_loupist(&loupists[0].id)
        .unwrap()
        .query_row(
            "SELECT status FROM scheduled_task_runs WHERE id=?1",
            [&runs[0].id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "running");
}

#[test]
fn recovery_without_orphans_does_not_need_write_access() {
    let store = fixture();
    store
        .lock()
        .unwrap()
        .execute_batch("PRAGMA query_only=ON")
        .unwrap();
    assert_eq!(store.scheduled_tasks_recover_running(3_000).unwrap(), 0);
    store
        .lock()
        .unwrap()
        .execute_batch("PRAGMA query_only=OFF")
        .unwrap();
    let task = store
        .scheduled_task_create("loupist-a", draft(), 1_000)
        .unwrap();
    let live = claim_task(&store, "loupist-a", &task.id, 2_000, true)
        .unwrap()
        .unwrap()
        .run;
    store
        .lock()
        .unwrap()
        .execute_batch("PRAGMA query_only=ON")
        .unwrap();
    assert_eq!(
        store
            .scheduled_tasks_recover_running_except(
                3_000,
                &std::collections::HashSet::from([live.run_id])
            )
            .unwrap(),
        0
    );
}

#[test]
fn repeated_recovery_warnings_report_each_skipped_run_only_once() {
    let warnings = super::super::store::RecoveryWarnings::default();
    let mut reported = Vec::new();
    for id in [
        "bad-run-a",
        "bad-run-a",
        "bad-run-b",
        "bad-run-a",
        "bad-run-b",
    ] {
        warnings.report_once(id, || reported.push(id));
    }
    assert_eq!(reported, ["bad-run-a", "bad-run-b"]);
}

#[test]
fn batched_lists_preserve_missing_and_foreign_chat_rejection() {
    for status in [TaskStatus::Active, TaskStatus::Paused] {
        for owner in [Some("loupist-a"), Some("loupist-b"), None] {
            let store = fixture();
            store
                .create_chat(&crate::test_fixtures::chat_session("target", "loupist-a"))
                .unwrap();
            let task = store
                .scheduled_task_create(
                    "loupist-a",
                    TaskDraft {
                        target: TaskTarget::ExistingChat {
                            thread_id: "target".into(),
                        },
                        status,
                        ..draft()
                    },
                    1_000,
                )
                .unwrap();
            match owner {
                Some(owner) => {
                    store
                        .lock()
                        .unwrap()
                        .execute(
                            "UPDATE chat_sessions SET loupist_id=?1 WHERE id='target'",
                            [owner],
                        )
                        .unwrap();
                }
                None => {
                    store
                        .lock()
                        .unwrap()
                        .execute("DELETE FROM chat_sessions WHERE id='target'", [])
                        .unwrap();
                }
            }
            let readable =
                owner == Some("loupist-a") || (owner.is_none() && status != TaskStatus::Active);
            assert_eq!(
                store.scheduled_task_get("loupist-a", &task.id).is_ok(),
                readable
            );
            assert_eq!(
                store.scheduled_tasks_list("loupist-a").unwrap().len(),
                usize::from(readable)
            );
            assert_eq!(
                store
                    .scheduled_tasks_list_with_latest("loupist-a")
                    .unwrap()
                    .len(),
                usize::from(readable)
            );
        }
    }
}

#[test]
fn task_lists_batch_reference_queries_independently_of_task_count() {
    use std::{ffi::c_void, sync::atomic::AtomicUsize};

    // Trace the real SQLite connection. The callback borrows a stable boxed
    // counter, and the guard unregisters it before freeing that counter.
    struct QueryCounter<'a> {
        store: &'a SqliteStore,
        statements: Box<AtomicUsize>,
    }
    unsafe extern "C" fn count_statement(
        _: u32,
        context: *mut c_void,
        _: *mut c_void,
        _: *mut c_void,
    ) -> i32 {
        // SAFETY: QueryCounter keeps this box alive until the trace is removed.
        unsafe { &*context.cast::<AtomicUsize>() }.fetch_add(1, Ordering::SeqCst);
        0
    }
    impl<'a> QueryCounter<'a> {
        fn new(store: &'a SqliteStore) -> Self {
            let mut counter = Self {
                store,
                statements: Box::default(),
            };
            let connection = store.lock().unwrap();
            // SAFETY: the connection is locked and the boxed callback context
            // stays at this address until Drop unregisters the callback.
            let result = unsafe {
                rusqlite::ffi::sqlite3_trace_v2(
                    connection.handle(),
                    rusqlite::ffi::SQLITE_TRACE_STMT as u32,
                    Some(count_statement),
                    (&mut *counter.statements as *mut AtomicUsize).cast(),
                )
            };
            assert_eq!(result, rusqlite::ffi::SQLITE_OK);
            counter
        }
        fn take(&self) -> usize {
            self.statements.swap(0, Ordering::SeqCst)
        }
    }
    impl Drop for QueryCounter<'_> {
        fn drop(&mut self) {
            let connection = self.store.lock().unwrap();
            // SAFETY: the connection is locked; no callback retains the box
            // after sqlite3_trace_v2 removes this registration.
            unsafe {
                rusqlite::ffi::sqlite3_trace_v2(connection.handle(), 0, None, std::ptr::null_mut())
            };
        }
    }
    let store = fixture();
    for index in 0..12 {
        let thread_id = format!("batch-target-{index}");
        store
            .create_chat(&crate::test_fixtures::chat_session(&thread_id, "loupist-a"))
            .unwrap();
        let task = store
            .scheduled_task_create(
                "loupist-a",
                TaskDraft {
                    target: TaskTarget::ExistingChat { thread_id },
                    ..draft()
                },
                1_000,
            )
            .unwrap();
        let run = claim_task(&store, "loupist-a", &task.id, 2_000, true)
            .unwrap()
            .unwrap()
            .run;
        store
            .scheduled_task_finish("loupist-a", &run.run_id, RunStatus::Succeeded, None, 3_000)
            .unwrap();
    }
    let queries = QueryCounter::new(&store);
    assert_eq!(store.scheduled_tasks_list("loupist-a").unwrap().len(), 12);
    assert!(
        queries.take() <= 4,
        "task list issued per-row reference queries"
    );
    assert_eq!(
        store
            .scheduled_tasks_list_with_latest("loupist-a")
            .unwrap()
            .len(),
        12
    );
    assert!(
        queries.take() <= 4,
        "latest-run list issued per-row reference queries"
    );
}
