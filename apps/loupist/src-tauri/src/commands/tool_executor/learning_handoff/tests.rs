use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::commands::tests::{profile, seed_profile};
use crate::commands::tool_executor::schedules::ScheduleContext;

fn fixture(temp: &tempfile::TempDir, surface: ChatSurface) -> (AppToolExecutor, ToolPolicy) {
    let state = AppState::for_test(temp.path().join("app"));
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
    let thread = if surface == ChatSurface::Bot {
        "home-loupist-a"
    } else {
        "chat-research"
    };
    state
        .store
        .create_chat(&crate::commands::tests::chat(thread, "loupist-a", 1))
        .unwrap();
    let mut capture = ToolCapture::for_chat("message-a".into());
    capture.bound_chat_id = Some(thread.into());
    capture.surface = Some(surface);
    capture.schedule_context = Some(ScheduleContext {
        research_model: None,
        model_profile_id: "fixture".into(),
        thinking_level: "medium".into(),
        run_options: BTreeMap::new(),
        use_memory: true,
        update_memory: true,
    });
    let executor = AppToolExecutor {
        state,
        capture: Arc::new(capture),
        loupist_id: "loupist-a".into(),
        capability_ids: BTreeSet::new(),
    };
    let policy = ToolPolicy {
        loupist_id: "loupist-a".into(),
        session_id: thread.into(),
        use_memory: true,
        propose_memory_updates: true,
        memory_proposal_budget: 1,
        as_of: None,
        display_currency: None,
    };
    (executor, policy)
}

fn request() -> Value {
    json!({
        "request": "Keep learning about container shipping rates between Asia and Europe, and tell me when they turn.",
        "subjects": [{"name": "Drewry World Container Index"}, {"asset": "asset-maersk"}],
    })
}

fn queued(executor: &AppToolExecutor) -> Vec<BotQuestion> {
    questions::pending(&executor.state, "loupist-a").unwrap()
}

#[tokio::test]
async fn a_research_session_posts_the_request_to_bot_once() {
    let temp = tempfile::tempdir().unwrap();
    let (executor, policy) = fixture(&temp, ChatSurface::Session);
    let result = executor.learning_handoff(&policy, request()).await.unwrap();
    assert_eq!(result["handed_off"], true);
    let again = executor.learning_handoff(&policy, request()).await.unwrap();
    assert_eq!(again["handed_off"], true);
    let queue = queued(&executor);
    assert_eq!(queue.len(), 1, "a retried hand-off is the same request");
    let handed = &queue[0];
    assert!(handed.receipt.task_id.starts_with(LEARNING_HANDOFF_PREFIX));
    assert!(handed
        .prompt
        .starts_with("Keep learning about container shipping rates"));
    assert!(handed.prompt.contains("asset-maersk"));
    assert!(handed.use_memory && handed.update_memory);
    assert!(
        crate::scheduled_tasks::service::list(&executor.state, "loupist-a")
            .unwrap()
            .is_empty(),
        "the session saves no program; Bot drafts it"
    );
}

#[tokio::test]
async fn the_hand_off_follows_learning_program_saves_third_party_text_guard() {
    for tainted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, ChatSurface::Session);
        if tainted {
            executor.capture.turn_taint.mark_untrusted_text();
        }
        let save_guard = executor
            .capture
            .turn_taint
            .enforce(ToolMethod::LearningProgramSave, &request());
        let result = executor.learning_handoff(&policy, request()).await;
        assert_eq!(result.is_ok(), save_guard.is_ok(), "tainted={tainted}");
        if tainted {
            assert!(matches!(result, Err(BrokerError::UntrustedTextPaused)));
            assert!(queued(&executor).is_empty(), "nothing reached Bot");
        } else {
            assert_eq!(queued(&executor).len(), 1);
        }
    }
}

#[tokio::test]
async fn bot_unattended_runs_and_bad_requests_are_refused() {
    for surface in [ChatSurface::Bot, ChatSurface::Scheduled] {
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, surface.clone());
        assert!(
            executor.learning_handoff(&policy, request()).await.is_err(),
            "{surface:?}"
        );
        assert!(queued(&executor).is_empty(), "{surface:?}");
    }
    let temp = tempfile::tempdir().unwrap();
    let (executor, policy) = fixture(&temp, ChatSurface::Session);
    let nine = (0..9)
        .map(|index| json!({"name": format!("Index {index}")}))
        .collect::<Vec<_>>();
    for bad in [
        json!({"request": "  "}),
        json!({"request": "a".repeat(8 * 1024 + 1)}),
        json!({"request": "Follow the yen.", "subjects": nine}),
        json!({"request": "Follow the yen.", "title": "Yen"}),
    ] {
        assert!(executor.learning_handoff(&policy, bad).await.is_err());
    }
    let mut historical = policy.clone();
    historical.as_of = Some("2025-03-31".into());
    assert!(executor
        .learning_handoff(&historical, request())
        .await
        .is_err());
    assert!(queued(&executor).is_empty());
}
