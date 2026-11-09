use std::collections::BTreeMap;

use super::*;
use crate::{
    chat::home::{deliver_finished_runs, ensure_home_thread, home_thread_id, is_home},
    scheduled_tasks::{
        service,
        types::{RunStatus, Schedule, TaskDraft, TaskStatus, TaskTarget},
    },
};

fn fixture(temporary: &tempfile::TempDir) -> AppState {
    let mut state = AppState::for_test(temporary.path().join("app"));
    seed_profile(
        state.store.as_ref(),
        &profile("loupist-a", &temporary.path().join("workspace"), 1),
    );
    // Task validation resolves the model; any configured Pi runtime will do.
    let path = temporary.path().to_path_buf();
    Arc::make_mut(&mut state.artifacts).pi = Some(crate::app::PiArtifacts {
        executable: path.clone(),
        runtime_dir: path.clone(),
        extension: path.clone(),
        provider_extension: path.clone(),
        system_prompt: path,
        provider: String::new(),
        model: String::new(),
        thinking_level: String::new(),
        run_options: BTreeMap::new(),
        provider_credential: None,
    });
    state
}

fn draft(title: &str) -> TaskDraft {
    TaskDraft {
        title: title.into(),
        prompt: "Compare inventory days across what I follow.".into(),
        schedule: Schedule::Weekly {
            days: vec![1],
            hour: 8,
            minute: 0,
            timezone: "Asia/Seoul".into(),
        },
        target: TaskTarget::NewChat,
        model_profile_id: "fixture".into(),
        thinking_level: "off".into(),
        run_options: BTreeMap::new(),
        use_memory: false,
        update_memory: false,
        status: TaskStatus::Active,
    }
}

/// Runs the task once by hand and records its answer, like a finished scheduled Chat.
fn finish_run(state: &AppState, task_id: &str, answer: &str, status: RunStatus) -> String {
    let now = now_ms();
    let (claim, ()) = state
        .store
        .scheduled_task_claim_with("loupist-a", task_id, now, true, |_| Ok(()))
        .unwrap()
        .unwrap();
    if status == RunStatus::Succeeded {
        state
            .store
            .scheduled_run_result("loupist-a", &claim.run.run_id, answer, "answer-1")
            .unwrap();
    }
    state
        .store
        .scheduled_task_finish("loupist-a", &claim.run.run_id, status, None, now + 1)
        .unwrap();
    claim.run.run_id
}

#[test]
fn handed_off_task_delivers_one_bot_receipt_after_reopening() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary);
    let id = "bot-handoff-retry";
    for _ in 0..2 {
        service::create(&state, "loupist-a", draft("Weekly comparison"), Some(id)).unwrap();
    }
    assert_eq!(deliver_finished_runs(&state, "loupist-a").unwrap(), 1);
    assert_eq!(deliver_finished_runs(&state, "loupist-a").unwrap(), 0);
    let home = ensure_home_thread(&state, "loupist-a").unwrap();
    let crate::scheduled_tasks::types::ScheduleReceipt::Task(receipt) =
        &home.messages[0].schedule_receipts[0]
    else {
        panic!("expected task receipt");
    };
    assert_eq!(home.messages[0].id, id);
    assert_eq!(receipt.status, TaskStatus::Active);
    assert!(!receipt.use_memory && !receipt.update_memory);
    assert!(receipt.next_run_at_ms.is_some());
}

#[test]
fn every_loupist_has_exactly_one_home_thread() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary);
    let first = ensure_home_thread(&state, "loupist-a").unwrap();
    let again = ensure_home_thread(&state, "loupist-a").unwrap();
    assert_eq!(first.id, home_thread_id("loupist-a"));
    assert_eq!(first, again);
    assert!(is_home(&first));
    let chats = state.store.list_chats_for_loupist("loupist-a").unwrap();
    assert_eq!(chats.iter().filter(|chat| is_home(chat)).count(), 1);
    assert!(chat_dto(&first).unwrap().home);
}

#[test]
fn a_finished_run_posts_one_short_message_into_the_thread_without_holdings_values() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary);
    let preview = state
        .store
        .preview_csv_bytes(b"symbol,market,name,quantity,cost,value,currency\nHANUL,KRX,Hanul Precision,4321,77.25,98765.43,KRW\n")
        .unwrap();
    state
        .store
        .holdings_commit_csv(&preview.preview_id)
        .unwrap();
    ensure_home_thread(&state, "loupist-a").unwrap();
    let task = service::create(&state, "loupist-a", draft("Compare inventory days"), None).unwrap();
    let run_id = finish_run(
        &state,
        &task.id,
        "Hanul Precision's inventory days rose to 61.\n\nThe longer answer stays in the run's Chat.",
        RunStatus::Succeeded,
    );

    assert_eq!(deliver_finished_runs(&state, "loupist-a").unwrap(), 1);
    // Delivery is idempotent: a second pass posts nothing.
    assert_eq!(deliver_finished_runs(&state, "loupist-a").unwrap(), 0);

    let home = state
        .store
        .get_chat(&home_thread_id("loupist-a"))
        .unwrap()
        .unwrap();
    assert_eq!(home.messages.len(), 1);
    let message = &home.messages[0];
    assert_eq!(
        message.content,
        "Hanul Precision's inventory days rose to 61."
    );
    assert!(message.host_composed);
    let delivery = message.delivery.as_ref().unwrap();
    assert_eq!(delivery.run_id, run_id);
    assert_eq!(delivery.task_title, "Compare inventory days");
    assert_eq!(delivery.status, RunStatus::Succeeded);
    assert!(delivery.schedule.is_some());
    assert!(delivery.run_thread_id.is_some());
    assert!(home.pi_session_cache.is_none());
    let stored = serde_json::to_string(&home).unwrap();
    assert!(!stored.contains("4321") && !stored.contains("98765"));
}

#[test]
fn a_failed_run_is_posted_as_unfinished_and_a_busy_thread_receives_it_later() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary);
    let home = ensure_home_thread(&state, "loupist-a").unwrap();
    let task = service::create(&state, "loupist-a", draft("Morning notes"), None).unwrap();
    finish_run(&state, &task.id, "", RunStatus::Failed);

    // A reply running in the Thread owns it; delivery waits.
    let running = state
        .register_run(
            "turn-1".into(),
            "loupist-a".into(),
            RunKind::Chat,
            RunTarget::ChatThread(home.id.clone()),
        )
        .unwrap();
    assert_eq!(deliver_finished_runs(&state, "loupist-a").unwrap(), 0);
    drop(running);
    assert_eq!(deliver_finished_runs(&state, "loupist-a").unwrap(), 1);

    let home = state.store.get_chat(&home.id).unwrap().unwrap();
    assert_eq!(home.messages[0].content, "Morning notes could not finish.");
    assert_eq!(
        home.messages[0].delivery.as_ref().unwrap().status,
        RunStatus::Failed
    );
}

#[test]
fn a_scheduled_result_posted_into_the_bot_leaves_its_memory_and_review_queue_alone() {
    use crate::chat::bot_memory::{
        self, BotMemoryAction, BotMemoryKind, BotMemoryNote, StagedBotMemory,
    };

    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary);
    let expected = ensure_home_thread(&state, "loupist-a").unwrap();
    let mut home = expected.clone();
    home.messages = vec![
        crate::test_fixtures::chat_message("u1", ChatRole::User, "Brief me on chips every Monday."),
        crate::test_fixtures::chat_message("r1", ChatRole::Assistant, "Scheduled."),
    ];
    let staged = StagedBotMemory {
        entry_id: "bm-chips".into(),
        action: BotMemoryAction::Remember(BotMemoryNote {
            kind: BotMemoryKind::Context,
            text: "Reads the chip briefing on Mondays".into(),
            basis: vec!["u1".into()],
        }),
    };
    bot_memory::settle_reply(&mut home, "r1", 2, &[staged], &[], Some("u1")).unwrap();
    home.updated_at_ms += 1;
    state.store.replace_chat(&expected, &home).unwrap();

    let task = service::create(&state, "loupist-a", draft("Chip briefing"), None).unwrap();
    finish_run(
        &state,
        &task.id,
        "Chip inventories fell for a third week.",
        RunStatus::Succeeded,
    );
    assert_eq!(deliver_finished_runs(&state, "loupist-a").unwrap(), 1);

    let delivered = state.store.get_chat(&home.id).unwrap().unwrap();
    assert_eq!(delivered.messages.len(), 3);
    assert!(delivered.messages[2].delivery.is_some());
    // The schedule stays the host's record; the post is neither memory nor queued for review.
    assert_eq!(delivered.bot_memory, home.bot_memory);
    assert_eq!(delivered.bot_memory.unreviewed, ["u1"]);
}

#[test]
fn pause_and_resume_check_the_task_revision() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary);
    let task = service::create(&state, "loupist-a", draft("Weekly review"), None).unwrap();
    let paused = service::set_status(
        &state,
        "loupist-a",
        &task.id,
        task.revision,
        TaskStatus::Paused,
    )
    .unwrap();
    assert_eq!(paused.status, TaskStatus::Paused);
    assert!(paused.next_run_at_ms.is_none());
    // The stale revision loses: someone else changed the task in between.
    assert!(service::set_status(
        &state,
        "loupist-a",
        &task.id,
        task.revision,
        TaskStatus::Active
    )
    .is_err());
    let resumed = service::set_status(
        &state,
        "loupist-a",
        &task.id,
        paused.revision,
        TaskStatus::Active,
    )
    .unwrap();
    assert_eq!(resumed.status, TaskStatus::Active);
    assert!(resumed.next_run_at_ms.is_some());
}

#[test]
fn a_crossed_line_is_announced_once_in_the_thread_and_waits_in_needs_attention() {
    use crate::knowledge::{assets, journal, JournalCheck};
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary);
    let home = ensure_home_thread(&state, "loupist-a").unwrap();
    let asset_id = crate::commands::assets::with_assets(&state, "loupist-a", true, |tx| {
        let page = assets::ensure_page(
            tx,
            "loupist-a",
            &assets::AssetIdentity {
                symbol: "HANU".into(),
                market: "KRX".into(),
                name: "Hanul Precision".into(),
            },
            None,
            home.created_at_ms,
        )?;
        let thesis = |role| {
            Some(assets::JournalThesis {
                asset_id: page.asset_id.clone(),
                role,
            })
        };
        journal::create(
            tx,
            "loupist-a",
            journal::JournalInput {
                judgment: "Margins recover.".into(),
                reasoning: String::new(),
                entities: vec![],
                check: None,
                thesis: thesis(assets::ThesisRole::Reason),
            },
            home.created_at_ms,
        )?;
        let line = journal::create(
            tx,
            "loupist-a",
            journal::JournalInput {
                judgment: "One customer above 25% would change my mind.".into(),
                reasoning: String::new(),
                entities: vec![],
                check: Some(JournalCheck {
                    claim: "One customer above 25% of revenue".into(),
                    metric: "Largest customer share".into(),
                    direction: "above".into(),
                    threshold: 25.0,
                    unit: Some("%".into()),
                    by_date: None,
                    confidence: None,
                }),
                thesis: thesis(assets::ThesisRole::Line),
            },
            home.created_at_ms,
        )?;
        assets::apply_update(
            tx,
            "loupist-a",
            &assets::ThesisUpdate {
                asset_id: page.asset_id.clone(),
                to: None,
                reason: None,
                reason_id: None,
                line: Some((line.journal_id, 27.0)),
                evidence: vec![assets::AssetEvidence {
                    tool: "dart_filing_read".into(),
                    title: "분기보고서".into(),
                    url: Some("https://dart.fss.or.kr/dsaf001/main.do?rcpNo=20261030000001".into()),
                    date: Some("2026-10-30".into()),
                    ..Default::default()
                }],
                fact_date: chrono::NaiveDate::from_ymd_opt(2026, 10, 30).unwrap(),
            },
            Some((home.id.as_str(), "message-1")),
            home.created_at_ms + 1,
        )?;
        Ok(page.asset_id)
    })
    .unwrap();
    assert_eq!(deliver_finished_runs(&state, "loupist-a").unwrap(), 1);
    assert_eq!(
        deliver_finished_runs(&state, "loupist-a").unwrap(),
        0,
        "posted once"
    );
    let thread = state
        .store
        .get_chat(&home_thread_id("loupist-a"))
        .unwrap()
        .unwrap();
    let last = thread.messages.last().unwrap();
    let alert = last.alert.clone().unwrap();
    assert_eq!(
        (
            alert.name.as_str(),
            alert.value.as_str(),
            alert.fact_date.as_str()
        ),
        ("Hanul Precision", "27%", "2026-10-30")
    );
    assert!(last.content.contains("Thesis marked Broken"));
    let crossed = |state: &AppState| {
        tauri::async_runtime::block_on(crate::commands::attention::collect(state))
            .unwrap()
            .into_iter()
            .any(|item| {
                item.kind == crate::commands::attention::AttentionKind::LineCrossed
                    && item.record_id.as_deref() == Some(asset_id.as_str())
            })
    };
    assert!(crossed(&state));
    crate::commands::assets::with_assets(&state, "loupist-a", true, |tx| {
        assets::mark_seen(tx, "loupist-a", &asset_id, now_ms())
    })
    .unwrap();
    assert!(!crossed(&state));
}
