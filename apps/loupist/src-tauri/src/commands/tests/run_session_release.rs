//! A session the scheduler made is its run's record until the user writes in
//! it: it goes with its run or its task, through the user's own Delete path.
use super::*;
use crate::{
    commands::types::chat_dto,
    domain::{LoupistProfile, ScheduledTurn},
    scheduled_tasks::{
        service,
        types::{RunStatus, Schedule, Task, TaskDraft, TaskKind, TaskStatus, TaskTarget},
    },
    test_fixtures::chat_message,
};

fn draft(target: TaskTarget) -> TaskDraft {
    TaskDraft {
        title: "Daily chip check".into(),
        prompt: "Check the chip makers.".into(),
        schedule: Schedule::Interval { every_minutes: 60 },
        target,
        model_profile_id: "fixture".into(),
        thinking_level: "medium".into(),
        run_options: Default::default(),
        use_memory: false,
        update_memory: false,
        status: TaskStatus::Paused,
    }
}

fn task(state: &AppState, loupist: &str, target: TaskTarget) -> Task {
    state
        .store
        .scheduled_task_create(loupist, draft(target), now_ms())
        .unwrap()
}

fn current(state: &AppState, task: &Task) -> Task {
    state
        .store
        .scheduled_task_get(&task.loupist_id, &task.id)
        .unwrap()
        .unwrap()
}

/// One finished run, claimed as the scheduler claims it, whose session holds
/// the task's request as dispatch stores it. Returns the run's session.
fn ran(state: &AppState, task: &Task) -> String {
    let now = now_ms();
    let (claim, ()) = state
        .store
        .scheduled_task_claim_with(&task.loupist_id, &task.id, now, true, |_| Ok(()))
        .unwrap()
        .unwrap();
    let thread = claim.run.thread_id.clone().unwrap();
    let expected = state.store.get_chat(&thread).unwrap().unwrap();
    let mut chat = expected.clone();
    chat.messages.push(ChatMessage {
        scheduled: Some(ScheduledTurn {
            task_id: task.id.clone(),
            run_id: claim.run.run_id.clone(),
            task_title: task.title.clone(),
            task_kind: TaskKind::Custom,
        }),
        ..chat_message(
            &format!("request-{}", claim.run.run_id),
            ChatRole::User,
            &task.prompt,
        )
    });
    state.store.replace_chat(&expected, &chat).unwrap();
    state
        .store
        .scheduled_task_finish(
            &task.loupist_id,
            &claim.run.run_id,
            RunStatus::Succeeded,
            None,
            now,
        )
        .unwrap();
    thread
}

fn write_in(state: &AppState, thread: &str) {
    let expected = state.store.get_chat(thread).unwrap().unwrap();
    let mut chat = expected.clone();
    chat.messages.push(chat_message(
        &format!("reply-{}", chat.messages.len()),
        ChatRole::User,
        "Why did the memory makers move?",
    ));
    state.store.replace_chat(&expected, &chat).unwrap();
}

fn exists(state: &AppState, thread: &str) -> bool {
    state.store.get_chat(thread).unwrap().is_some()
}

async fn run_now(state: &AppState, task: &Task) {
    // The test app has no Pi, so the run fails, but it is claimed and recorded.
    let run = state
        .scheduled
        .dispatch(state, &task.loupist_id, &task.id, true)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, RunStatus::Failed);
}

fn app() -> (tempfile::TempDir, AppState, LoupistProfile) {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let loupist = profile("loupist-a", &workspace, 1);
    seed_profile(state.store.as_ref(), &loupist);
    (directory, state, loupist)
}

fn delete(state: &AppState, task: &Task) {
    service::delete(
        state,
        &task.loupist_id,
        &task.id,
        current(state, task).revision,
    )
    .unwrap();
}

#[tokio::test]
async fn a_run_leaving_the_history_takes_its_session_unless_the_user_wrote_in_it() {
    let (_directory, state, loupist) = app();
    let fresh = task(&state, &loupist.id, TaskTarget::NewChat);
    let sessions = (0..crate::scheduled_tasks::types::MAX_RUN_HISTORY)
        .map(|_| ran(&state, &fresh))
        .collect::<Vec<_>>();
    write_in(&state, &sessions[1]);

    // The 51st run pushes the oldest out of the history, and its session with it.
    run_now(&state, &fresh).await;
    assert!(!exists(&state, &sessions[0]));
    assert!(sessions[1..].iter().all(|thread| exists(&state, thread)));
    let history = state
        .store
        .scheduled_task_runs(&loupist.id, &fresh.id)
        .unwrap();
    assert_eq!(history.len(), 50);

    // The next one pushes out a run whose session the user wrote in: that is
    // their research now. The one after takes the next session as before.
    run_now(&state, &fresh).await;
    assert!(exists(&state, &sessions[1]));
    let listed = state.store.get_chat(&sessions[1]).unwrap().unwrap();
    assert!(!chat_dto(&listed).unwrap().hide_from_recent);
    run_now(&state, &fresh).await;
    assert!(exists(&state, &sessions[1]));
    assert!(!exists(&state, &sessions[2]));
    assert!(sessions[3..].iter().all(|thread| exists(&state, thread)));
}

#[tokio::test]
async fn a_continuing_tasks_session_stays_while_the_task_does() {
    let (_directory, state, loupist) = app();
    let continuing = task(&state, &loupist.id, TaskTarget::Dedicated);
    let session = ran(&state, &continuing);
    for _ in 1..crate::scheduled_tasks::types::MAX_RUN_HISTORY {
        assert_eq!(ran(&state, &continuing), session);
    }
    run_now(&state, &continuing).await;
    run_now(&state, &continuing).await;
    assert!(
        exists(&state, &session),
        "runs leaving history keep the task's own session"
    );
    assert_eq!(
        current(&state, &continuing).session_thread_id.as_deref(),
        Some(session.as_str())
    );

    // Deleting the task takes the session it alone wrote in.
    delete(&state, &continuing);
    assert!(!exists(&state, &session));

    // One the user wrote in stays theirs.
    let written = task(&state, &loupist.id, TaskTarget::Dedicated);
    let theirs = ran(&state, &written);
    write_in(&state, &theirs);
    ran(&state, &written);
    delete(&state, &written);
    assert!(exists(&state, &theirs));
}

#[tokio::test]
async fn deleting_a_task_takes_only_the_run_sessions_nobody_wrote_in() {
    let (_directory, state, loupist) = app();
    let fresh = task(&state, &loupist.id, TaskTarget::NewChat);
    let sessions = (0..3).map(|_| ran(&state, &fresh)).collect::<Vec<_>>();
    write_in(&state, &sessions[2]);
    // A task that writes into a chat of the user's own never takes that chat.
    let own = chat("chat-own", &loupist.id, 1);
    state.store.create_chat(&own).unwrap();
    let into_own = task(
        &state,
        &loupist.id,
        TaskTarget::ExistingChat {
            thread_id: own.id.clone(),
        },
    );
    assert_eq!(ran(&state, &into_own), own.id);

    delete(&state, &fresh);
    delete(&state, &into_own);
    assert!(!exists(&state, &sessions[0]));
    assert!(!exists(&state, &sessions[1]));
    assert!(exists(&state, &sessions[2]));
    assert!(exists(&state, &own.id));
    assert!(state
        .store
        .scheduled_tasks_list(&loupist.id)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn startup_releases_what_earlier_runs_left_and_keeps_what_they_cannot_tell() {
    let (_directory, state, loupist) = app();
    let fresh = task(&state, &loupist.id, TaskTarget::NewChat);
    let kept = ran(&state, &fresh);
    let request = |id: &str| ChatMessage {
        scheduled: Some(ScheduledTurn {
            task_id: "scheduled-task-gone".into(),
            run_id: "scheduled-run-gone".into(),
            task_title: "Deleted task".into(),
            task_kind: TaskKind::Custom,
        }),
        ..chat_message(id, ChatRole::User, "Check the chip makers.")
    };
    let mut orphan = chat("scheduled-chat-orphan", &loupist.id, 1);
    orphan.messages.push(request("request-orphan"));
    orphan.messages.push(chat_message(
        "answer-orphan",
        ChatRole::Assistant,
        "Nothing crossed.",
    ));
    let empty = chat("scheduled-chat-empty", &loupist.id, 1);
    let mut written = chat("scheduled-chat-written", &loupist.id, 1);
    written.messages.push(request("request-written"));
    written.messages.push(chat_message(
        "reply-written",
        ChatRole::User,
        "And the memory makers?",
    ));
    // An older build stored a run's request with no record: it reads as the user's.
    let mut older = chat("scheduled-chat-older", &loupist.id, 1);
    older.messages.push(chat_message(
        "request-older",
        ChatRole::User,
        "Check the chip makers.",
    ));
    for session in [&orphan, &empty, &written, &older] {
        state.store.create_chat(session).unwrap();
    }

    assert_eq!(crate::commands::records::release_host_sessions(&state), 2);
    assert!(!exists(&state, &orphan.id));
    assert!(!exists(&state, &empty.id));
    for session in [&kept, &written.id, &older.id] {
        assert!(exists(&state, session), "{session}");
    }
    assert_eq!(crate::commands::records::release_host_sessions(&state), 0);
}

#[tokio::test]
async fn a_released_session_is_deleted_with_everything_that_hangs_off_it() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("loupist");
    std::fs::create_dir(&folder).unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let loupist = create_folder_loupist(&state, &folder).await.unwrap();
    let fresh = task(&state, &loupist.id, TaskTarget::NewChat);
    let session = ran(&state, &fresh);
    state
        .store
        .lock_loupist(&loupist.id)
        .unwrap()
        .execute(
            "INSERT INTO chat_artifacts (id, chat_session_id, kind, title, current_revision, created_at_ms, updated_at_ms, data_json, current_content_json) VALUES ('artifact-run', ?1, 'markdown', 'Chip notes', 1, 3, 3, '{}', '{}')",
            [&session],
        )
        .unwrap();
    let durable = state.loupist_files_root(&loupist.id).unwrap();
    let runtime = &state.artifacts.deletion_root;
    let attachments = std::path::PathBuf::from("loupists")
        .join(&loupist.id)
        .join("attachments")
        .join(&session);
    let cache = std::path::PathBuf::from("loupists")
        .join(&loupist.id)
        .join("pi-sessions")
        .join(&session);
    durable.ensure_private_subdirectory(&attachments).unwrap();
    runtime.ensure_private_subdirectory(&cache).unwrap();
    fs::write(
        durable
            .absolute_path(&attachments)
            .unwrap()
            .join("file.txt"),
        "attachment",
    )
    .unwrap();
    fs::write(
        runtime.absolute_path(&cache).unwrap().join("session.jsonl"),
        "conversation",
    )
    .unwrap();

    delete(&state, &fresh);
    assert!(!exists(&state, &session));
    assert!(state
        .store
        .get_chat_artifact("artifact-run")
        .unwrap()
        .is_none());
    assert!(!durable.entry_exists(&attachments).unwrap());
    assert!(!runtime.entry_exists(&cache).unwrap());
    assert!(state.store.list_deletion_journals().unwrap().is_empty());
}

#[tokio::test]
async fn deleting_the_opening_task_ends_a_wait_in_its_retained_session() {
    let (_directory, state, loupist) = app();
    let task = task(&state, &loupist.id, TaskTarget::Dedicated);
    let thread = ran(&state, &task);
    write_in(&state, &thread);
    let chat = state.store.get_chat(&thread).unwrap().unwrap();
    let opening = chat.opening_scheduled_turn().unwrap();
    let pending = crate::chat::questions::register(
        "deleted-task-question",
        &loupist.id,
        Some(&thread),
        Some(&opening.task_id),
        2,
    );
    let listed = || async {
        crate::commands::attention::collect(&state)
            .await
            .unwrap()
            .into_iter()
            .any(|row| {
                row.kind == crate::commands::attention::AttentionKind::Question
                    && row.chat_id.as_ref() == Some(&thread)
            })
    };
    assert!(listed().await);
    delete(&state, &task);
    assert!(exists(&state, &thread));
    assert!(!listed().await);
    assert!(pending.await.is_err());
}
