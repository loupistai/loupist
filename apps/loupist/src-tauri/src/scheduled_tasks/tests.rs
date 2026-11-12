use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Barrier,
    },
};

use chrono::DateTime;

use crate::{
    domain::LoupistProfile,
    store::{SqliteStore, StoreError},
};

use super::{
    schedule::{next_run_after, validate_schedule},
    types::*,
};

mod regressions;

fn at(value: &str) -> i64 {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp_millis()
}

fn loupist(id: &str) -> LoupistProfile {
    LoupistProfile {
        root_filesystem_identity: None,
        ..crate::test_fixtures::loupist_profile(id)
    }
}

fn fixture() -> SqliteStore {
    let store = SqliteStore::open_in_memory().unwrap();
    store.create_loupist(&loupist("loupist-a")).unwrap();
    store.create_loupist(&loupist("loupist-b")).unwrap();
    store
}

fn claim_task(
    store: &SqliteStore,
    loupist_id: &str,
    id: &str,
    now: i64,
    manual: bool,
) -> Result<Option<Claim>, StoreError> {
    store
        .scheduled_task_claim_with(loupist_id, id, now, manual, |_| Ok(()))
        .map(|claimed| claimed.map(|(claim, ())| claim))
}

fn draft() -> TaskDraft {
    TaskDraft {
        title: "Morning notes".into(),
        prompt: "Read today's notes and summarize them.".into(),
        schedule: Schedule::Interval { every_minutes: 5 },
        target: TaskTarget::NewChat,
        model_profile_id: "fixture/model".into(),
        thinking_level: "off".into(),
        run_options: BTreeMap::new(),
        use_memory: true,
        update_memory: false,
        status: TaskStatus::Active,
    }
}

#[test]
fn recurrences_are_strict_timezone_aware_and_skip_dst_gaps_and_folds() {
    let daily = |hour, minute, timezone: &str| Schedule::Daily {
        hour,
        minute,
        timezone: timezone.into(),
    };
    let sunday = "2026-10-11T09:00:00Z";
    let weekly = Schedule::Weekly {
        days: vec![0, 3],
        hour: 9,
        minute: 0,
        timezone: "UTC".into(),
    };
    let new_york = "America/New_York";
    let cases = [
        (daily(9, 0, "UTC"), at(sunday) - 1, Some(sunday)),
        (daily(9, 0, "UTC"), at(sunday), Some("2026-10-12T09:00:00Z")),
        // Weekdays start on Sunday.
        (weekly.clone(), at(sunday) - 1, Some(sunday)),
        (weekly, at(sunday), Some("2026-10-14T09:00:00Z")),
        (
            daily(9, 0, "Asia/Seoul"),
            at("2026-10-11T00:00:00Z"),
            Some("2026-10-12T00:00:00Z"),
        ),
        (Schedule::Once { at_ms: at(sunday) }, at(sunday), None),
        // A nonexistent local time is skipped; a repeated one runs once.
        (
            daily(2, 30, new_york),
            at("2026-03-08T05:00:00Z"),
            Some("2026-03-09T06:30:00Z"),
        ),
        (
            daily(1, 30, new_york),
            at("2026-11-01T04:00:00Z"),
            Some("2026-11-01T05:30:00Z"),
        ),
        (
            daily(1, 30, new_york),
            at("2026-11-01T05:30:00Z"),
            Some("2026-11-02T06:30:00Z"),
        ),
        (
            daily(1, 30, new_york),
            at("2026-11-01T06:00:00Z"),
            Some("2026-11-02T06:30:00Z"),
        ),
    ];
    for (schedule, after, expected) in cases {
        assert_eq!(
            next_run_after(&schedule, after).unwrap(),
            expected.map(at),
            "{schedule:?} after {after}"
        );
    }
    assert_eq!(
        next_run_after(&Schedule::Interval { every_minutes: 5 }, at(sunday)).unwrap(),
        Some(at(sunday) + 300_000)
    );
}

#[test]
fn invalid_schedule_and_unbounded_task_inputs_are_rejected() {
    for schedule in [
        Schedule::Once { at_ms: -1 },
        Schedule::Interval { every_minutes: 0 },
        Schedule::Interval {
            every_minutes: 10081,
        },
        Schedule::Daily {
            hour: 24,
            minute: 0,
            timezone: "UTC".into(),
        },
        Schedule::Daily {
            hour: 9,
            minute: 0,
            timezone: "not/a-zone".into(),
        },
        Schedule::Weekly {
            days: vec![1, 1],
            hour: 9,
            minute: 0,
            timezone: "UTC".into(),
        },
        Schedule::Weekly {
            days: vec![],
            hour: 9,
            minute: 0,
            timezone: "UTC".into(),
        },
    ] {
        assert!(validate_schedule(&schedule).is_err());
    }
    assert!(next_run_after(&Schedule::Interval { every_minutes: 1 }, i64::MAX).is_err());
    let store = fixture();
    let mut value = draft();
    value.prompt = "x".repeat(MAX_PROMPT_BYTES + 1);
    assert!(store.scheduled_task_create("loupist-a", value, 1).is_err());
    let mut json = serde_json::to_value(draft()).unwrap();
    json["credentials"] = serde_json::json!("not accepted");
    assert!(serde_json::from_value::<TaskDraft>(json).is_err());
    assert!(store.scheduled_tasks_list("loupist-a").unwrap().is_empty());
}

#[test]
fn task_edits_use_revision_cas_and_target_ownership() {
    let store = fixture();
    let task = store
        .scheduled_task_create("loupist-a", draft(), 1)
        .unwrap();
    assert_eq!(task.next_run_at_ms, Some(300_001));
    assert!(store
        .scheduled_task_get("loupist-b", &task.id)
        .unwrap()
        .is_none());
    assert!(store
        .scheduled_task_update("loupist-b", &task.id, task.revision, draft(), 2)
        .is_err());
    let mut edit = task.draft();
    edit.title = "Edited notes".into();
    let changed = store
        .scheduled_task_update("loupist-a", &task.id, task.revision, edit.clone(), 2)
        .unwrap();
    assert_eq!(changed.revision, 2);
    assert!(matches!(
        store.scheduled_task_update("loupist-a", &task.id, task.revision, edit, 3),
        Err(StoreError::Conflict(_))
    ));
    let claim = claim_task(&store, "loupist-a", &task.id, 3, true)
        .unwrap()
        .unwrap();
    let mut foreign = draft();
    foreign.target = TaskTarget::ExistingChat {
        thread_id: claim.run.thread_id.unwrap(),
    };
    assert!(store
        .scheduled_task_create("loupist-b", foreign, 4)
        .is_err());
    assert!(store
        .scheduled_tasks_activity("loupist-b")
        .unwrap()
        .is_empty());
    assert_eq!(
        store.scheduled_tasks_activity("loupist-a").unwrap().len(),
        1
    );
}

#[test]
fn task_list_includes_only_the_latest_run_and_breaks_same_time_ties() {
    let store = fixture();
    let empty = store
        .scheduled_task_create("loupist-a", draft(), 1)
        .unwrap();
    let task = store
        .scheduled_task_create("loupist-a", draft(), 1)
        .unwrap();
    store
        .scheduled_task_create("loupist-b", draft(), 1)
        .unwrap();
    let first = claim_task(&store, "loupist-a", &task.id, 2, true)
        .unwrap()
        .unwrap()
        .run;
    store
        .scheduled_task_finish("loupist-a", &first.run_id, RunStatus::Succeeded, None, 2)
        .unwrap();
    let second = claim_task(&store, "loupist-a", &task.id, 2, true)
        .unwrap()
        .unwrap()
        .run;
    let list = store.scheduled_tasks_list_with_latest("loupist-a").unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(
        list.iter()
            .find(|item| item.task.id == empty.id)
            .unwrap()
            .latest_run,
        None
    );
    assert_eq!(
        list.iter()
            .find(|item| item.task.id == task.id)
            .unwrap()
            .latest_run,
        Some(second.clone())
    );
    let failed = store
        .scheduled_task_finish(
            "loupist-a",
            &second.run_id,
            RunStatus::Failed,
            Some("Provider disconnected."),
            3,
        )
        .unwrap();
    let item = store
        .scheduled_tasks_list_with_latest("loupist-a")
        .unwrap()
        .into_iter()
        .find(|item| item.task.id == task.id)
        .unwrap();
    assert_eq!(item.latest_run, Some(failed));
    assert_eq!(item.task.status, TaskStatus::Active);
    assert_eq!(
        store.scheduled_tasks_list_with_latest("loupist-b").unwrap()[0].latest_run,
        None
    );
    assert_eq!(
        store
            .scheduled_task_runs("loupist-a", &task.id)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn due_claim_coalesces_missed_intervals_and_freezes_the_execution_definition() {
    let store = fixture();
    let task = store
        .scheduled_task_create("loupist-a", draft(), 1)
        .unwrap();
    assert!(claim_task(&store, "loupist-a", &task.id, 300_000, false)
        .unwrap()
        .is_none());
    let claim = claim_task(&store, "loupist-a", &task.id, 900_001, false)
        .unwrap()
        .unwrap();
    assert_eq!(claim.task, task);
    assert_eq!(claim.run.scheduled_for_ms, 300_001);
    assert_eq!(claim.run.started_at_ms, 900_001);
    let stored = store
        .scheduled_task_get("loupist-a", &task.id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.next_run_at_ms, Some(1_200_001));
    let chat = store
        .get_chat(claim.run.thread_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(chat.title, task.title);
    assert_eq!(chat.memory_policy.use_memory, task.use_memory);
    assert!(!chat.memory_policy.update_memory);
    assert!(claim_task(&store, "loupist-a", &task.id, 1_200_001, false)
        .unwrap()
        .is_none());
    assert!(store
        .scheduled_task_delete("loupist-a", &task.id, stored.revision)
        .is_err());
    let mut edit = stored.draft();
    edit.prompt = "A later prompt must not alter this run.".into();
    edit.title = "Later title".into();
    let edited = store
        .scheduled_task_update("loupist-a", &task.id, stored.revision, edit, 900_002)
        .unwrap();
    store
        .scheduled_task_finish(
            "loupist-a",
            &claim.run.run_id,
            RunStatus::Failed,
            Some("provider failed"),
            900_003,
        )
        .unwrap();
    assert_eq!(
        store
            .scheduled_task_get("loupist-a", &task.id)
            .unwrap()
            .unwrap(),
        edited
    );
    let connection = store.lock().unwrap();
    let snapshot: String = connection
        .query_row(
            "SELECT task_snapshot_json FROM scheduled_task_runs WHERE id=?1",
            [&claim.run.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(serde_json::from_str::<Task>(&snapshot).unwrap(), task);
}

#[test]
fn busy_admission_rolls_back_the_occurrence_and_new_chat() {
    for (schedule, target) in [
        (Schedule::Interval { every_minutes: 5 }, TaskTarget::NewChat),
        (Schedule::Once { at_ms: 300_001 }, TaskTarget::NewChat),
        (
            Schedule::Interval { every_minutes: 5 },
            TaskTarget::Dedicated,
        ),
    ] {
        let store = fixture();
        let mut definition = draft();
        definition.schedule = schedule;
        definition.target = target;
        let task = store
            .scheduled_task_create("loupist-a", definition, 1)
            .unwrap();
        let mut attempted_chat = None;
        let result =
            store.scheduled_task_claim_with("loupist-a", &task.id, 300_001, false, |claim| {
                assert_eq!(claim.task, task);
                assert_eq!(claim.run.status, RunStatus::Running);
                attempted_chat = claim.run.thread_id.clone();
                Err::<(), _>(StoreError::Conflict("model worker capacity is full"))
            });
        assert!(matches!(result, Err(StoreError::Conflict(_))));
        assert_eq!(
            store
                .scheduled_task_get("loupist-a", &task.id)
                .unwrap()
                .unwrap(),
            task
        );
        assert!(store
            .scheduled_task_runs("loupist-a", &task.id)
            .unwrap()
            .is_empty());
        assert!(store
            .list_chats_for_loupist("loupist-a")
            .unwrap()
            .is_empty());
        assert!(store
            .get_chat(attempted_chat.as_deref().unwrap())
            .unwrap()
            .is_none());
        // The same occurrence remains available when capacity becomes free.
        assert!(claim_task(&store, "loupist-a", &task.id, 300_002, false)
            .unwrap()
            .is_some());
    }
}

struct AdmissionResource {
    thread_id: String,
    released: Arc<AtomicBool>,
}

impl Drop for AdmissionResource {
    fn drop(&mut self) {
        self.released.store(true, Ordering::SeqCst);
    }
}

#[test]
fn claim_admission_is_owned_on_commit_and_released_when_commit_fails() {
    let store = fixture();
    let task = store
        .scheduled_task_create("loupist-a", draft(), 1)
        .unwrap();
    let released = Arc::new(AtomicBool::new(false));
    let (claim, admission) = store
        .scheduled_task_claim_with("loupist-a", &task.id, 300_001, false, |claim| {
            Ok(AdmissionResource {
                thread_id: claim.run.thread_id.clone().unwrap(),
                released: released.clone(),
            })
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        Some(admission.thread_id.as_str()),
        claim.run.thread_id.as_deref()
    );
    assert!(!released.load(Ordering::SeqCst));
    assert!(store.get_chat(&admission.thread_id).unwrap().is_some());
    drop(admission);
    assert!(released.load(Ordering::SeqCst));

    let store = fixture();
    let task = store
        .scheduled_task_create("loupist-a", draft(), 1)
        .unwrap();
    store.lock().unwrap().execute_batch("CREATE TABLE scheduled_commit_failure(task_id TEXT REFERENCES scheduled_tasks(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_scheduled_commit AFTER INSERT ON scheduled_task_runs BEGIN INSERT INTO scheduled_commit_failure(task_id) VALUES('missing-task'); END;").unwrap();
    let released = Arc::new(AtomicBool::new(false));
    let result = store.scheduled_task_claim_with("loupist-a", &task.id, 300_001, false, |claim| {
        Ok(AdmissionResource {
            thread_id: claim.run.thread_id.clone().unwrap(),
            released: released.clone(),
        })
    });
    assert!(matches!(result, Err(StoreError::Sqlite(_))));
    assert!(released.load(Ordering::SeqCst));
    assert_eq!(
        store
            .scheduled_task_get("loupist-a", &task.id)
            .unwrap()
            .unwrap(),
        task
    );
    assert!(store
        .scheduled_task_runs("loupist-a", &task.id)
        .unwrap()
        .is_empty());
    assert!(store
        .list_chats_for_loupist("loupist-a")
        .unwrap()
        .is_empty());
}

#[test]
fn manual_runs_preserve_recurrence_and_once_requires_no_automatic_replay() {
    let store = fixture();
    let task = store
        .scheduled_task_create("loupist-a", draft(), 1)
        .unwrap();
    let manual = claim_task(&store, "loupist-a", &task.id, 2, true)
        .unwrap()
        .unwrap();
    assert_eq!(
        store
            .scheduled_task_get("loupist-a", &task.id)
            .unwrap()
            .unwrap()
            .next_run_at_ms,
        task.next_run_at_ms
    );
    store
        .scheduled_task_finish(
            "loupist-a",
            &manual.run.run_id,
            RunStatus::Succeeded,
            None,
            3,
        )
        .unwrap();
    let mut once = draft();
    once.schedule = Schedule::Once { at_ms: 100 };
    let task = store.scheduled_task_create("loupist-a", once, 1).unwrap();
    let run = claim_task(&store, "loupist-a", &task.id, 100, false)
        .unwrap()
        .unwrap()
        .run;
    let completed = store
        .scheduled_task_get("loupist-a", &task.id)
        .unwrap()
        .unwrap();
    assert_eq!(completed.status, TaskStatus::Completed);
    assert_eq!(completed.next_run_at_ms, None);
    store
        .scheduled_task_finish(
            "loupist-a",
            &run.run_id,
            RunStatus::Failed,
            Some("failed once"),
            101,
        )
        .unwrap();
    assert!(claim_task(&store, "loupist-a", &task.id, 102, false)
        .unwrap()
        .is_none());
    assert!(claim_task(&store, "loupist-a", &task.id, 102, true)
        .unwrap()
        .is_some());
}

#[test]
fn restart_records_interruption_and_schedules_the_next_occurrence() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("store.sqlite3");
    let store = SqliteStore::open(&path).unwrap();
    store.create_loupist(&loupist("loupist-a")).unwrap();
    let task = store
        .scheduled_task_create("loupist-a", draft(), 1)
        .unwrap();
    let run = claim_task(&store, "loupist-a", &task.id, 2, true)
        .unwrap()
        .unwrap()
        .run;
    drop(store);
    let store = SqliteStore::open(path).unwrap();
    assert_eq!(store.scheduled_tasks_recover_running(3).unwrap(), 1);
    assert_eq!(store.scheduled_tasks_recover_running(4).unwrap(), 0);
    let saved = store
        .scheduled_task_run_get("loupist-a", &run.run_id)
        .unwrap()
        .unwrap();
    assert_eq!(saved.status, RunStatus::Interrupted);
    assert_eq!(saved.finished_at_ms, Some(3));
    let saved = store
        .scheduled_task_get("loupist-a", &task.id)
        .unwrap()
        .unwrap();
    assert_eq!(saved.status, TaskStatus::Active);
    assert!(saved.last_error.unwrap().contains("Interrupted"));
    assert_eq!(
        store
            .scheduled_tasks_due_for_loupists(999_999, 100, &["loupist-a".into()])
            .unwrap()
            .len(),
        1
    );
    assert_eq!(store.list_chats_for_loupist("loupist-a").unwrap().len(), 1);
}

#[test]
fn prelaunch_setup_failure_pauses_and_duplicate_completion_is_idempotent() {
    let store = fixture();
    let task = store
        .scheduled_task_create("loupist-a", draft(), 1)
        .unwrap();
    let run = claim_task(&store, "loupist-a", &task.id, 300_001, false)
        .unwrap()
        .unwrap()
        .run;
    let finished = store
        .scheduled_task_finish(
            "loupist-a",
            &run.run_id,
            RunStatus::Failed,
            Some("the selected Pi model is hidden from Chat"),
            300_002,
        )
        .unwrap();
    let paused = store
        .scheduled_task_get("loupist-a", &task.id)
        .unwrap()
        .unwrap();
    assert_eq!(paused.status, TaskStatus::Paused);
    assert_eq!(paused.next_run_at_ms, None);
    assert_eq!(paused.last_error, finished.error);
    assert_eq!(
        store
            .scheduled_task_finish(
                "loupist-a",
                &run.run_id,
                RunStatus::Failed,
                Some("duplicate result"),
                300_003
            )
            .unwrap(),
        finished
    );
    assert_eq!(
        store
            .scheduled_task_get("loupist-a", &task.id)
            .unwrap()
            .unwrap(),
        paused
    );
    assert!(store
        .scheduled_task_finish(
            "loupist-a",
            &run.run_id,
            RunStatus::Succeeded,
            None,
            300_003
        )
        .is_err());
    let resumed = store
        .scheduled_task_set_status(
            "loupist-a",
            &task.id,
            paused.revision,
            TaskStatus::Active,
            900_000,
        )
        .unwrap();
    assert_eq!(resumed.next_run_at_ms, Some(1_200_000));
    assert_eq!(resumed.last_error, None);
}

#[test]
fn simultaneous_claims_create_exactly_one_run_and_chat() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("store.sqlite3");
    let first = SqliteStore::open(&path).unwrap();
    first.create_loupist(&loupist("loupist-a")).unwrap();
    let task = first
        .scheduled_task_create("loupist-a", draft(), 1)
        .unwrap();
    let second = SqliteStore::open(&path).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles = [first, second]
        .into_iter()
        .map(|store| {
            let barrier = barrier.clone();
            let id = task.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                claim_task(&store, "loupist-a", &id, 300_001, false)
                    .unwrap()
                    .is_some()
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|claimed| *claimed)
            .count(),
        1
    );
    let store = SqliteStore::open(path).unwrap();
    assert_eq!(
        store
            .scheduled_task_runs("loupist-a", &task.id)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(store.list_chats_for_loupist("loupist-a").unwrap().len(), 1);
}

#[test]
fn deleting_a_chat_pauses_its_target_and_preserves_loadable_history_atomically() {
    let store = fixture();
    let seed = store
        .scheduled_task_create("loupist-a", draft(), 1)
        .unwrap();
    let run = claim_task(&store, "loupist-a", &seed.id, 2, true)
        .unwrap()
        .unwrap()
        .run;
    store
        .scheduled_task_finish("loupist-a", &run.run_id, RunStatus::Succeeded, None, 3)
        .unwrap();
    let chat = store
        .get_chat(run.thread_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    let mut existing = draft();
    existing.target = TaskTarget::ExistingChat {
        thread_id: chat.id.clone(),
    };
    let task = store
        .scheduled_task_create("loupist-a", existing, 4)
        .unwrap();
    let running = claim_task(&store, "loupist-a", &task.id, 5, true)
        .unwrap()
        .unwrap()
        .run;
    let mut stale = chat.clone();
    stale.title = "stale deletion".into();
    assert!(store.delete_chat(&stale).is_err());
    assert_eq!(
        store
            .scheduled_task_get("loupist-a", &task.id)
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Active
    );
    assert_eq!(
        store
            .scheduled_task_run_get("loupist-a", &running.run_id)
            .unwrap()
            .unwrap()
            .thread_id,
        Some(chat.id.clone())
    );
    store.delete_chat(&chat).unwrap();
    let paused = store
        .scheduled_task_get("loupist-a", &task.id)
        .unwrap()
        .unwrap();
    assert_eq!(paused.status, TaskStatus::Paused);
    assert!(paused.last_error.unwrap().contains("deleted"));
    assert!(store
        .scheduled_task_set_status(
            "loupist-a",
            &task.id,
            paused.revision,
            TaskStatus::Active,
            6
        )
        .is_err());
    assert!(claim_task(&store, "loupist-a", &task.id, 6, true).is_err());
    let history = store.scheduled_task_runs("loupist-a", &task.id).unwrap();
    assert_eq!(history[0].status, RunStatus::Interrupted);
    assert_eq!(history[0].thread_id, None);
    assert_eq!(
        store.scheduled_task_runs("loupist-a", &seed.id).unwrap()[0].thread_id,
        None
    );
}

#[test]
fn deleting_a_loupist_cascades_its_schedules_and_history_only() {
    let store = fixture();
    let a = store
        .scheduled_task_create("loupist-a", draft(), 1)
        .unwrap();
    let b = store
        .scheduled_task_create("loupist-b", draft(), 1)
        .unwrap();
    claim_task(&store, "loupist-a", &a.id, 2, true)
        .unwrap()
        .unwrap();
    claim_task(&store, "loupist-b", &b.id, 2, true)
        .unwrap()
        .unwrap();
    store.delete_loupist(&loupist("loupist-a")).unwrap();
    assert!(store.get_loupist("loupist-a").unwrap().is_none());
    assert!(store
        .list_chats_for_loupist("loupist-a")
        .unwrap()
        .is_empty());
    assert_eq!(
        store.scheduled_tasks_activity("loupist-b").unwrap().len(),
        1
    );
    assert_eq!(store.scheduled_tasks_list("loupist-b").unwrap().len(), 1);
    let connection = store.lock().unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM scheduled_task_runs WHERE loupist_id='loupist-a'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}

#[test]
fn run_history_is_bounded_and_task_limit_is_enforced() {
    let store = fixture();
    let task = store
        .scheduled_task_create("loupist-a", draft(), 1)
        .unwrap();
    for index in 0..55 {
        let now = 2 + index * 2;
        let run = claim_task(&store, "loupist-a", &task.id, now, true)
            .unwrap()
            .unwrap()
            .run;
        store
            .scheduled_task_finish(
                "loupist-a",
                &run.run_id,
                RunStatus::Succeeded,
                None,
                now + 1,
            )
            .unwrap();
    }
    let history = store.scheduled_task_runs("loupist-a", &task.id).unwrap();
    assert_eq!(history.len(), MAX_RUN_HISTORY);
    assert_eq!(history[0].started_at_ms, 110);
    assert_eq!(history.last().unwrap().started_at_ms, 12);
    for _ in 1..MAX_TASKS_PER_LOUPIST {
        store
            .scheduled_task_create("loupist-a", draft(), 1)
            .unwrap();
    }
    assert!(store
        .scheduled_task_create("loupist-a", draft(), 1)
        .is_err());
}

#[test]
fn dedicated_schedules_keep_separate_sessions_across_runs_and_mode_changes() {
    let store = fixture();
    let mut definition = draft();
    definition.target = TaskTarget::Dedicated;
    let a = store
        .scheduled_task_create("loupist-a", definition.clone(), 1)
        .unwrap();
    let b = store
        .scheduled_task_create("loupist-a", definition, 1)
        .unwrap();
    let first = claim_task(&store, "loupist-a", &a.id, 2, true)
        .unwrap()
        .unwrap();
    let other = claim_task(&store, "loupist-a", &b.id, 2, true)
        .unwrap()
        .unwrap();
    assert_ne!(first.run.thread_id, other.run.thread_id);
    assert!(claim_task(&store, "loupist-a", &a.id, 3, true)
        .unwrap()
        .is_none());
    store
        .scheduled_task_finish(
            "loupist-a",
            &first.run.run_id,
            RunStatus::Succeeded,
            None,
            3,
        )
        .unwrap();
    let second = claim_task(&store, "loupist-a", &a.id, 4, true)
        .unwrap()
        .unwrap();
    assert_eq!(first.run.thread_id, second.run.thread_id);
    store
        .scheduled_task_finish(
            "loupist-a",
            &second.run.run_id,
            RunStatus::Succeeded,
            None,
            5,
        )
        .unwrap();
    let saved = store
        .scheduled_task_get("loupist-a", &a.id)
        .unwrap()
        .unwrap();
    let mut fresh = saved.draft();
    fresh.target = TaskTarget::NewChat;
    let updated = store
        .scheduled_task_update("loupist-a", &a.id, saved.revision, fresh, 6)
        .unwrap();
    assert!(updated.session_thread_id.is_none());
    let third = claim_task(&store, "loupist-a", &a.id, 7, true)
        .unwrap()
        .unwrap();
    assert_ne!(first.run.thread_id, third.run.thread_id);
    store
        .scheduled_task_finish(
            "loupist-a",
            &third.run.run_id,
            RunStatus::Succeeded,
            None,
            8,
        )
        .unwrap();
    let fourth = claim_task(&store, "loupist-a", &a.id, 9, true)
        .unwrap()
        .unwrap();
    assert_ne!(third.run.thread_id, fourth.run.thread_id);
    assert!(store
        .get_chat(first.run.thread_id.as_deref().unwrap())
        .unwrap()
        .is_some());
}

#[test]
fn retried_schedule_create_is_idempotent_and_distinct_keys_allow_multiple_tasks() {
    let store = fixture();
    let a = store
        .scheduled_task_create_idempotent("loupist-a", "scheduled-request-a", draft(), 1)
        .unwrap();
    assert_eq!(
        store
            .scheduled_task_create_idempotent("loupist-a", "scheduled-request-a", draft(), 2)
            .unwrap(),
        a
    );
    let mut changed = draft();
    changed.title = "Different request".into();
    assert!(store
        .scheduled_task_create_idempotent("loupist-a", "scheduled-request-a", changed, 3)
        .is_err());
    store
        .scheduled_task_create_idempotent("loupist-a", "scheduled-request-b", draft(), 3)
        .unwrap();
    assert_eq!(store.scheduled_tasks_list("loupist-a").unwrap().len(), 2);
}

#[test]
fn briefing_kind_is_unique_and_preserves_dedicated_chat_and_memory() {
    let store = fixture();
    let mut definition = draft();
    definition.title = "Briefings".into();
    definition.target = TaskTarget::Dedicated;
    definition.update_memory = true;
    let task = store
        .scheduled_task_create_kind(
            "loupist-a",
            "briefing-a",
            definition.clone(),
            TaskKind::Briefing,
            100,
        )
        .unwrap();
    assert!(store
        .scheduled_task_create_kind(
            "loupist-a",
            "briefing-b",
            definition.clone(),
            TaskKind::Briefing,
            100
        )
        .is_err());
    let other = store
        .scheduled_task_create_kind(
            "loupist-b",
            "briefing-b",
            definition,
            TaskKind::Briefing,
            100,
        )
        .unwrap();
    assert_eq!(other.kind, TaskKind::Briefing);
    let first = store
        .scheduled_task_claim_with("loupist-a", &task.id, 200, true, |_| Ok(()))
        .unwrap()
        .unwrap()
        .0;
    let chat = store
        .get_chat(first.run.thread_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(chat.title, "Briefings");
    assert!(chat.memory_policy.use_memory && chat.memory_policy.update_memory);
    store
        .scheduled_task_finish(
            "loupist-a",
            &first.run.run_id,
            RunStatus::Succeeded,
            None,
            300,
        )
        .unwrap();
    let second = store
        .scheduled_task_claim_with("loupist-a", &task.id, 400, true, |_| Ok(()))
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(first.run.thread_id, second.run.thread_id);
    assert_eq!(second.run.kind, TaskKind::Briefing);
}

#[test]
fn inbox_includes_custom_and_briefing_results_and_marks_only_owned_items() {
    let store = fixture();
    let custom = store
        .scheduled_task_create("loupist-a", draft(), 100)
        .unwrap();
    let mut definition = draft();
    definition.target = TaskTarget::Dedicated;
    definition.update_memory = true;
    let briefing = store
        .scheduled_task_create_kind(
            "loupist-a",
            "briefing-a",
            definition,
            TaskKind::Briefing,
            100,
        )
        .unwrap();
    let mut ids = Vec::new();
    for (index, task) in [custom, briefing].iter().enumerate() {
        let run = store
            .scheduled_task_claim_with("loupist-a", &task.id, 200 + index as i64, true, |_| Ok(()))
            .unwrap()
            .unwrap()
            .0
            .run;
        store
            .scheduled_run_result(
                "loupist-a",
                &run.run_id,
                "# Heading\n\n**Revenue** changed.\n\nLater details.",
                "message-a",
            )
            .unwrap();
        store
            .scheduled_task_finish("loupist-a", &run.run_id, RunStatus::Succeeded, None, 300)
            .unwrap();
        ids.push(run.run_id);
    }
    assert_eq!(store.inbox_unread_count("loupist-a").unwrap(), 2);
    assert_eq!(store.inbox_unread_count("loupist-b").unwrap(), 0);
    let page = store.inbox_runs("loupist-a", None, 1).unwrap();
    assert_eq!(page[0].kind, TaskKind::Briefing);
    assert_eq!(page[0].summary, "Revenue changed.");
    assert_eq!(page[0].message_id.as_deref(), Some("message-a"));
    assert_eq!(
        store
            .inbox_runs("loupist-a", Some(&page[0].run_id), 1)
            .unwrap()[0]
            .kind,
        TaskKind::Custom
    );
    assert!(store.inbox_mark_read("loupist-b", &ids, 400).is_err());
    store.inbox_mark_read("loupist-a", &ids[..1], 400).unwrap();
    assert_eq!(store.inbox_unread_count("loupist-a").unwrap(), 1);
    store.inbox_mark_read("loupist-a", &ids, 500).unwrap();
    assert_eq!(store.inbox_unread_count("loupist-a").unwrap(), 0);
    assert_eq!(
        store
            .scheduled_task_run_get("loupist-a", &ids[0])
            .unwrap()
            .unwrap()
            .read_at_ms,
        Some(400)
    );
}

#[test]
fn recovered_inbox_retains_tool_refusals_and_is_unread_once() {
    let store = fixture();
    let task = store
        .scheduled_task_create("loupist-a", draft(), 100)
        .unwrap();
    let run = store
        .scheduled_task_claim_with("loupist-a", &task.id, 200, true, |_| Ok(()))
        .unwrap()
        .unwrap()
        .0
        .run;
    assert_eq!(store.inbox_unread_count("loupist-a").unwrap(), 0);
    for name in ["read", "browser"] {
        assert!(crate::briefings::policy::enforce(
            &store,
            "loupist-a",
            Some(&run.run_id),
            crate::broker::ToolMethod::NativeAuthorize,
            &serde_json::json!({"name":name})
        )
        .is_err());
    }
    assert_eq!(store.scheduled_tasks_recover_running(300).unwrap(), 1);
    assert_eq!(store.scheduled_tasks_recover_running(400).unwrap(), 0);
    let recovered = &store.inbox_runs("loupist-a", None, 10).unwrap()[0];
    assert_eq!(recovered.status, RunStatus::Interrupted);
    assert_eq!(
        recovered
            .skipped_tools
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        ["read", "browser"]
    );
    assert_eq!(store.inbox_unread_count("loupist-a").unwrap(), 1);
}

fn edit_row(store: &SqliteStore, table: &str, set: &str, id: &str) {
    let connection = store.lock_loupist("loupist-a").unwrap();
    connection
        .execute(
            &format!("UPDATE {table} SET data_json={set} WHERE id=?1"),
            [id],
        )
        .unwrap();
}

#[test]
fn a_task_another_version_wrote_is_listed_and_kept_but_never_run_or_changed() {
    let store = fixture();
    let now = at("2026-10-11T09:00:00Z");
    let chat = crate::test_fixtures::chat_session("chat-target", "loupist-a");
    store.create_chat(&chat).unwrap();
    let newer = store
        .scheduled_task_create(
            "loupist-a",
            TaskDraft {
                target: TaskTarget::ExistingChat {
                    thread_id: chat.id.clone(),
                },
                ..draft()
            },
            now,
        )
        .unwrap();
    let kept = store
        .scheduled_task_create("loupist-a", draft(), now)
        .unwrap();
    // A newer build's kind and field, at the same schema version.
    edit_row(
        &store,
        "scheduled_tasks",
        r#"json_set(data_json,'$.kind','digest','$.digest_sections',json('["moves"]'))"#,
        &newer.id,
    );

    let listed = store.scheduled_tasks_list("loupist-a").unwrap();
    let read = listed.iter().find(|task| task.id == newer.id).unwrap();
    assert_eq!(read.kind, TaskKind::Unknown("digest".into()));
    assert_eq!(
        serde_json::to_value(read).unwrap()["kind"],
        serde_json::json!("digest")
    );
    let due = store
        .scheduled_tasks_due_for_loupists(now + 60 * 60_000, 100, &["loupist-a".into()])
        .unwrap();
    assert_eq!(
        due.iter().map(|task| &task.id).collect::<Vec<_>>(),
        [&kept.id]
    );
    let refused = |result: Result<_, StoreError>| matches!(result, Err(StoreError::Conflict(_)));
    assert!(refused(
        claim_task(&store, "loupist-a", &newer.id, now + 1, true).map(|_| ())
    ));
    assert!(refused(
        store
            .scheduled_task_set_status("loupist-a", &newer.id, 1, TaskStatus::Paused, now + 1)
            .map(|_| ())
    ));
    assert!(refused(
        store
            .scheduled_task_update("loupist-a", &newer.id, 1, draft(), now + 1)
            .map(|_| ())
    ));
    // Folder admission accepts it, and a rewrite this version makes keeps the kind.
    {
        let connection = store.lock_loupist("loupist-a").unwrap();
        super::store::validate_saved_tasks(&connection).unwrap();
    }
    store.delete_chat(&chat).unwrap();
    let after = store
        .scheduled_task_get("loupist-a", &newer.id)
        .unwrap()
        .unwrap();
    assert_eq!(after.kind, TaskKind::Unknown("digest".into()));
    assert_eq!(after.status, TaskStatus::Paused);
    store
        .scheduled_task_delete("loupist-a", &newer.id, after.revision)
        .unwrap();
}

#[test]
fn one_unreadable_task_or_run_never_fails_a_listing_page_or_the_due_scan() {
    let store = fixture();
    let now = at("2026-10-11T09:00:00Z");
    let broken = store
        .scheduled_task_create("loupist-a", draft(), now)
        .unwrap();
    let kept = store
        .scheduled_task_create("loupist-a", draft(), now)
        .unwrap();
    let mut runs = Vec::new();
    for offset in [1, 2] {
        let claim = claim_task(&store, "loupist-a", &kept.id, now + offset, true)
            .unwrap()
            .unwrap();
        store
            .scheduled_task_finish(
                "loupist-a",
                &claim.run.run_id,
                RunStatus::Succeeded,
                None,
                now + offset,
            )
            .unwrap();
        runs.push(claim.run);
    }
    edit_row(
        &store,
        "scheduled_tasks",
        r#"json_set(data_json,'$.schedule','hourly')"#,
        &broken.id,
    );
    // The newest run cannot be read; the older one can.
    edit_row(
        &store,
        "scheduled_task_runs",
        r#"json_set(data_json,'$.skipped_tools','none')"#,
        &runs[1].id,
    );

    let listed = store.scheduled_tasks_list("loupist-a").unwrap();
    assert_eq!(
        listed.iter().map(|task| &task.id).collect::<Vec<_>>(),
        [&kept.id]
    );
    let with_latest = store.scheduled_tasks_list_with_latest("loupist-a").unwrap();
    assert_eq!(with_latest.len(), 1);
    assert!(with_latest[0].latest_run.is_none());
    let due = store
        .scheduled_tasks_due_for_loupists(now + 60 * 60_000, 100, &["loupist-a".into()])
        .unwrap();
    assert_eq!(
        due.iter().map(|task| &task.id).collect::<Vec<_>>(),
        [&kept.id]
    );
    assert_eq!(
        store.scheduled_task_runs("loupist-a", &kept.id).unwrap(),
        [store
            .scheduled_task_run_get("loupist-a", &runs[0].run_id)
            .unwrap()
            .unwrap()]
    );
    // The inbox cursor moves past the unreadable run to the readable one.
    let (first, cursor) = store.inbox_page("loupist-a", None, 1).unwrap();
    assert!(first.is_empty());
    assert_eq!(cursor.as_deref(), Some(runs[1].run_id.as_str()));
    let (second, _) = store.inbox_page("loupist-a", cursor.as_deref(), 1).unwrap();
    assert_eq!(
        second.into_iter().map(|run| run.run_id).collect::<Vec<_>>(),
        [runs[0].run_id.clone()]
    );
    // Reading that one task and admitting the folder stay strict.
    assert!(store.scheduled_task_get("loupist-a", &broken.id).is_err());
    let connection = store.lock_loupist("loupist-a").unwrap();
    assert!(super::store::validate_saved_tasks(&connection).is_err());
}

/// SQL reads, indexes and edits address task and run JSON by serde name
/// (`scheduled_tasks/store.rs`): the briefing index on `$.kind`, the due scan's
/// `$.kind`, inbox reads of `$.read_at_ms` and `$.briefing_context`, and the
/// `json_set` edits for a deleted Chat and a copied folder. A serde rename must
/// fail here rather than silently miss.
#[test]
fn sql_json_paths_name_fields_tasks_and_runs_serialize() {
    let sqlite = rusqlite::Connection::open_in_memory().unwrap();
    let extract = |json: &str, path: &str| -> Option<String> {
        sqlite
            .query_row(
                "SELECT json_type(?1, ?2), json_extract(?1, ?2)",
                [json, path],
                |row| {
                    let kind: Option<String> = row.get(0)?;
                    let value: rusqlite::types::Value = row.get(1)?;
                    Ok(kind.map(|_| match value {
                        rusqlite::types::Value::Text(text) => text,
                        other => format!("{other:?}"),
                    }))
                },
            )
            .unwrap()
    };
    let store = fixture();
    let now = at("2026-10-11T09:00:00Z");
    let task = store
        .scheduled_task_create("loupist-a", draft(), now)
        .unwrap();
    let claim = claim_task(&store, "loupist-a", &task.id, now + 1, true)
        .unwrap()
        .unwrap();
    let run = store
        .scheduled_task_finish(
            "loupist-a",
            &claim.run.run_id,
            RunStatus::Failed,
            Some("Provider unavailable."),
            now + 2,
        )
        .unwrap();

    let briefing = serde_json::to_string(&Task {
        kind: TaskKind::Briefing,
        ..task.clone()
    })
    .unwrap();
    assert_eq!(extract(&briefing, "$.kind").as_deref(), Some("briefing"));
    for kind in TaskKind::KNOWN {
        let json = serde_json::to_string(&Task {
            kind: kind.clone(),
            ..task.clone()
        })
        .unwrap();
        assert_eq!(extract(&json, "$.kind").as_deref(), Some(kind.as_str()));
    }
    for status in [
        TaskStatus::Active,
        TaskStatus::Paused,
        TaskStatus::Completed,
    ] {
        let json = serde_json::to_string(&Task {
            status,
            ..task.clone()
        })
        .unwrap();
        assert_eq!(extract(&json, "$.status").as_deref(), Some(status.as_str()));
    }
    let json = serde_json::to_string(&task).unwrap();
    assert!(extract(&json, "$.next_run_at_ms").is_some());

    let run = Run {
        read_at_ms: None,
        briefing_context: Some(crate::briefings::BriefingContext {
            symbols: Vec::new(),
            principles_checked: 0,
            new_facts: 0,
            disputed_entity_ids: vec!["entity-1".into()],
            changed: Default::default(),
            ahead: Vec::new(),
            assets: Vec::new(),
            earnings: Vec::new(),
            watch_events: Vec::new(),
        }),
        ..run
    };
    let json = serde_json::to_string(&run).unwrap();
    for path in [
        "$.read_at_ms",
        "$.briefing_context.disputedEntityIds",
        "$.thread_id",
        "$.finished_at_ms",
        "$.error",
    ] {
        assert!(extract(&json, path).is_some(), "{path}");
    }
    for status in [
        RunStatus::Running,
        RunStatus::Succeeded,
        RunStatus::Failed,
        RunStatus::Interrupted,
    ] {
        let json = serde_json::to_string(&Run {
            status,
            ..run.clone()
        })
        .unwrap();
        assert_eq!(extract(&json, "$.status").as_deref(), Some(status.as_str()));
    }
}
