use std::{future, sync::Arc, time::Duration};

use serde_json::json;
use tokio::{
    sync::{watch, Notify},
    task::JoinSet,
};

use super::*;
use crate::{
    app::AppState,
    commands::{now_ms, triggers},
    scheduled_tasks::{service, trigger},
};

const WAIT: Duration = Duration::from_secs(3);

fn add_loupist(state: &AppState, root: &std::path::Path, id: &str) {
    let workspace = root.join(id);
    crate::test_fixtures::create_folder_loupist(&state.store, id, &workspace);
    std::fs::create_dir_all(workspace.join(".loupist")).unwrap();
}

fn check(state: &AppState, owner: &str, created: i64) -> Task {
    let script = triggers::install_template(state, owner, "price-band", "Watch price").unwrap();
    let mut task = state
        .store
        .scheduled_task_create(owner, draft(), created)
        .unwrap();
    task.trigger = Some(TaskTrigger {
        script: triggers::script_ref(&script),
        summary: "Watch price".into(),
        when_met_summary: None,
        args: Default::default(),
        tools: Vec::new(),
        active_hours: None,
        approved: Some(TriggerApproval {
            version: script.summary.version,
            sha256: script.summary.sha256,
            at_ms: created,
            configuration: None,
            test: None,
        }),
        pending: None,
        state: TriggerPhase::On,
        script_state: json!({}),
        replay_boundary: Some(ReplayBoundary::INITIAL),
        last: None,
        evaluations: Vec::new(),
        counters: Default::default(),
        approved_versions: Vec::new(),
        stale_after_days: 5,
        reads_per_check: 1,
        proposal_test: None,
    });
    let revision = task.revision;
    state
        .store
        .scheduled_task_write(task, revision, created)
        .unwrap()
}

fn verdict(met: bool, observed_at: i64) -> trigger::CheckOutput {
    let as_of = chrono::DateTime::from_timestamp_millis(observed_at)
        .unwrap()
        .to_rfc3339();
    trigger::output_from_worker(
        &json!({"status":"completed","check_verdict":{
            "met":met,"reading":"Price moved","as_of":as_of,"state":{"seen":true}
        }}),
        vec![json!({"call":{"tool":"price"},"response":{"status":"ok"}})],
        1,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn slow_check_does_not_delay_due_tasks_or_run_now() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState::for_test(root.path().join("app"));
    for id in ["slow", "due", "manual"] {
        add_loupist(&state, root.path(), id);
    }
    let now = now_ms();
    let slow = check(&state, "slow", now - 700_000);
    let due = state
        .store
        .scheduled_task_create("due", draft(), now - 600_000)
        .unwrap();
    let manual = state
        .store
        .scheduled_task_create("manual", draft(), now)
        .unwrap();

    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    *state.scheduled.inner.check_evaluator.lock().unwrap() = Some((
        Duration::from_secs(30),
        Arc::new({
            let entered = entered.clone();
            let release = release.clone();
            move |_, _| {
                let (entered, release) = (entered.clone(), release.clone());
                Box::pin(async move {
                    entered.notify_one();
                    release.notified().await;
                    Ok(verdict(false, now))
                })
            }
        }),
    ));
    state.scheduled.start(&state).unwrap();
    tokio::time::timeout(WAIT, entered.notified())
        .await
        .unwrap();

    let later = state
        .store
        .scheduled_task_create("due", draft(), now - 500_000)
        .unwrap();
    state.scheduled.changed();
    let start = std::time::Instant::now();
    tokio::time::timeout(WAIT, async {
        let run = service::run_now(&state, "manual", &manual.id, false)
            .await
            .unwrap();
        assert_eq!(run.task_id, manual.id);
        for task in [&due, &later] {
            loop {
                if state
                    .store
                    .scheduled_task_runs("due", &task.id)
                    .unwrap()
                    .len()
                    == 1
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    })
    .await
    .expect("another due task and Run now must reach admission while the check is suspended");
    assert!(start.elapsed() < WAIT);
    assert!(state
        .store
        .scheduled_task_get("slow", &slow.id)
        .unwrap()
        .unwrap()
        .trigger
        .unwrap()
        .last
        .is_none());
    release.notify_one();
    state.scheduled.stop();
    state.scheduled.drain().await;
}

#[tokio::test]
async fn edits_pauses_and_deletion_win_over_a_running_evaluation() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState::for_test(root.path().join("app"));
    let owner = "research";
    add_loupist(&state, root.path(), owner);
    let now = now_ms();
    for action in ["edit", "pause", "delete"] {
        let task = check(&state, owner, now - 600_000);
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        *state.scheduled.inner.check_evaluator.lock().unwrap() = Some((
            WAIT,
            Arc::new({
                let entered = entered.clone();
                let release = release.clone();
                move |_, _| {
                    let (entered, release) = (entered.clone(), release.clone());
                    Box::pin(async move {
                        entered.notify_one();
                        release.notified().await;
                        Ok(verdict(true, now))
                    })
                }
            }),
        ));
        let evaluation = tokio::spawn({
            let state = state.clone();
            let id = task.id.clone();
            async move { state.scheduled.dispatch(&state, owner, &id, false).await }
        });
        tokio::time::timeout(WAIT, entered.notified())
            .await
            .unwrap();
        let current = state
            .store
            .scheduled_task_get(owner, &task.id)
            .unwrap()
            .unwrap();
        let expected = match action {
            "edit" => {
                let mut draft = current.draft();
                draft.title = "New title".into();
                Some(
                    state
                        .store
                        .scheduled_task_update(owner, &current.id, current.revision, draft, now)
                        .unwrap(),
                )
            }
            "pause" => Some(
                state
                    .store
                    .scheduled_task_set_status(
                        owner,
                        &current.id,
                        current.revision,
                        TaskStatus::Paused,
                        now,
                    )
                    .unwrap(),
            ),
            "delete" => {
                state
                    .store
                    .scheduled_task_delete(owner, &current.id, current.revision)
                    .unwrap();
                None
            }
            _ => unreachable!(),
        };
        release.notify_one();
        assert!(evaluation.await.unwrap().unwrap().is_none());
        assert_eq!(
            state.store.scheduled_task_get(owner, &task.id).unwrap(),
            expected
        );
        assert!(state
            .store
            .scheduled_tasks_activity(owner)
            .unwrap()
            .is_empty());
        assert!(state
            .store
            .get_chat(&crate::chat::home::home_thread_id(owner))
            .unwrap()
            .is_none());
        assert!(state.scheduled.inner.checks.lock().unwrap().is_empty());
    }
}

#[tokio::test(start_paused = true)]
async fn whole_evaluation_timeout_settles_the_occurrence_and_releases_its_claim() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState::for_test(root.path().join("app"));
    let owner = "research";
    add_loupist(&state, root.path(), owner);
    let now = now_ms();
    let task = check(&state, owner, now - 600_000);
    let due = task.next_run_at_ms.unwrap();
    let entered = Arc::new(Notify::new());
    let limit = Duration::from_secs(10);
    *state.scheduled.inner.check_evaluator.lock().unwrap() = Some((
        limit,
        Arc::new({
            let entered = entered.clone();
            move |_, _| {
                let entered = entered.clone();
                Box::pin(async move {
                    entered.notify_one();
                    future::pending().await
                })
            }
        }),
    ));
    let evaluation = tokio::spawn({
        let state = state.clone();
        let id = task.id.clone();
        async move { state.scheduled.dispatch(&state, owner, &id, false).await }
    });
    entered.notified().await;
    tokio::time::advance(limit).await;
    assert!(evaluation.await.unwrap().unwrap().is_none());
    let settled = state
        .store
        .scheduled_task_get(owner, &task.id)
        .unwrap()
        .unwrap();
    assert!(settled.next_run_at_ms.unwrap() > now);
    let check = settled.trigger.unwrap();
    let timeout = check.last.unwrap();
    assert_eq!(timeout.scheduled_for_ms, due);
    assert_eq!(timeout.outcome, TriggerOutcome::Error);
    assert_eq!(
        timeout.error.as_deref(),
        Some("The check exceeded its evaluation time limit.")
    );
    assert_eq!(timeout.reads, MAX_TRIGGER_READS_PER_EVALUATION as u32);
    assert_eq!(check.counters.today.reads, timeout.reads);
    assert!(state.scheduled.inner.checks.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_manual_and_due_dispatches_evaluate_each_task_once() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState::for_test(root.path().join("app"));
    let owner = "research";
    add_loupist(&state, root.path(), owner);
    let now = now_ms();
    let tasks: Vec<_> = (0..8)
        .map(|_| check(&state, owner, now - 600_000))
        .collect();
    let (release, waiting) = watch::channel(false);
    let (started, mut evaluations) = tokio::sync::mpsc::unbounded_channel();
    *state.scheduled.inner.check_evaluator.lock().unwrap() = Some((
        Duration::from_secs(30),
        Arc::new(move |_, id| {
            let started = started.clone();
            let id = id.to_owned();
            let mut waiting = waiting.clone();
            Box::pin(async move {
                started.send(id).unwrap();
                waiting.wait_for(|ready| *ready).await.unwrap();
                Ok(verdict(false, now))
            })
        }),
    ));

    let mut dispatches = JoinSet::new();
    const DUPLICATES: usize = 16;
    for attempt in 0..DUPLICATES {
        for task in &tasks {
            let id = task.id.clone();
            let state = state.clone();
            dispatches.spawn(async move {
                state
                    .scheduled
                    .dispatch(&state, owner, &id, attempt % 2 == 0)
                    .await
            });
        }
    }
    tokio::time::timeout(WAIT, async {
        for _ in 0..tasks.len() * (DUPLICATES - 1) {
            assert!(dispatches
                .join_next()
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .is_none());
        }
    })
    .await
    .expect("all losing dispatches return while the evaluations are suspended");
    let mut started = std::collections::HashSet::new();
    for _ in &tasks {
        let id = tokio::time::timeout(WAIT, evaluations.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(started.insert(id), "duplicate evaluation");
    }
    release.send_replace(true);
    while let Some(result) = dispatches.join_next().await {
        assert!(result.unwrap().unwrap().is_none());
    }
    assert!(evaluations.try_recv().is_err());
    for task in tasks {
        let trigger = state
            .store
            .scheduled_task_get(owner, &task.id)
            .unwrap()
            .unwrap()
            .trigger
            .unwrap();
        assert_eq!(trigger.evaluations.len(), 1);
        assert_eq!(trigger.evaluations[0].outcome, TriggerOutcome::NotMet);
        assert_eq!(trigger.counters.today.reads, 1);
    }
    assert!(state.scheduled.inner.checks.lock().unwrap().is_empty());
}
