//! Replies whose saves wait on their own
//! (docs/runtime/chat-turns.md#saves-that-wait-on-their-own), through the real
//! completion path: a conversation takes its next message while its earlier
//! replies wait to save behind another conversation's reader.
use super::*;
use serde_json::Value;

use crate::{
    broker::{ToolExecutor as _, ToolMethod, ToolPolicy},
    chat::{handoff, home, surface::ChatSurface},
    commands::{chat_runtime::saves, records},
    domain::{ChatSession, MemorySaveSkipReason, MemoryUpdateResult, MemoryUpdateStatus},
    runs::coordinator::{RunRegistration, CHAT_SAVE_ACTIVITY},
};

const LOUPIST: &str = "message-saves";

/// What a reply stages before it settles.
enum Stage {
    Principle,
    BotNote(&'static str),
    /// Bot memory changes, checked by the real tool against the stored memory.
    BotChanges(Value),
    /// The same with a principle beside them.
    BotChangesAndPrinciple(Value),
    Nothing,
}

/// One reply's turn: what its stream received, and its own save.
struct Reply {
    run_id: String,
    message_id: String,
    principle_id: String,
    stream: Arc<std::sync::Mutex<Vec<ChatStreamEvent>>>,
    ended: Arc<tokio::sync::Notify>,
    task: tokio::task::JoinHandle<()>,
}

impl Reply {
    /// Waits for the reply's stream to end: at its stored answer when the save
    /// waits on its own.
    async fn stream_ended(&self) {
        self.ended.notified().await;
    }

    fn stream_ends(&self) -> Vec<ChatStreamEvent> {
        self.stream
            .lock()
            .unwrap()
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    ChatStreamEvent::Completed { .. }
                        | ChatStreamEvent::Error { .. }
                        | ChatStreamEvent::Aborted { .. }
                )
            })
            .cloned()
            .collect()
    }

    async fn saved(&mut self) {
        (&mut self.task).await.expect("the reply's save task");
    }

    /// Where its resolution falls among everything published.
    fn resolved_at(&self, state: &AppState) -> usize {
        saves::published(state, &self.run_id)
            .into_iter()
            .find(|(_, event)| event.pending.is_none())
            .expect("the save published its receipt")
            .0
    }
}

struct Fixture {
    root: tempfile::TempDir,
    state: AppState,
    thread_id: String,
    surface: ChatSurface,
}

fn cache(derived: &str) -> crate::domain::PiSessionCache {
    crate::domain::PiSessionCache {
        entries_sha256: "a".repeat(64),
        leaf_id: Some(format!("leaf-{derived}")),
        harness_digest: "b".repeat(64),
        runtime_policy_sha256: "c".repeat(64),
        runtime_surface_sha256: "d".repeat(64),
        connector_authority_sha256: "e".repeat(64),
        memory_access_enabled: true,
        memory_update_enabled: true,
        derived_session_id: derived.into(),
        context_envelope_sha256: String::new(),
        context_envelope_stale: false,
        execution_model: execution_model(),
    }
}

impl Fixture {
    fn bot() -> Self {
        Self::new(ChatSurface::Bot)
    }

    fn session() -> Self {
        Self::new(ChatSurface::Session)
    }

    fn new(surface: ChatSurface) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut state = AppState::for_folder_test(root.path().join("app"));
        crate::test_fixtures::create_folder_loupist(
            &state.store,
            LOUPIST,
            &root.path().join("workspace"),
        );
        // Scheduled task validation resolves the model; any configured runtime will do.
        let path = root.path().to_path_buf();
        Arc::make_mut(&mut state.artifacts).pi = Some(crate::app::PiArtifacts {
            executable: path.clone(),
            runtime_dir: path.clone(),
            extension: path.clone(),
            provider_extension: path.clone(),
            system_prompt: path,
            provider: String::new(),
            model: String::new(),
            thinking_level: String::new(),
            run_options: Default::default(),
            provider_credential: None,
        });
        let thread_id = if surface.is_bot() {
            home::ensure_home_thread(&state, LOUPIST).unwrap().id
        } else {
            let chat = crate::test_fixtures::chat_session("research-session", LOUPIST);
            state.store.create_chat(&chat).unwrap();
            chat.id
        };
        Self {
            root,
            state,
            thread_id,
            surface,
        }
    }

    /// Opens the Loupist again, as after the app was killed.
    fn restart(self) -> Self {
        let Self {
            root,
            state,
            thread_id,
            surface,
        } = self;
        state.close_for_restart_test();
        Self {
            state: AppState::for_folder_test(root.path().join("app")),
            root,
            thread_id,
            surface,
        }
    }

    /// Another conversation's reader; saves wait for it until it is dropped.
    fn reader(&self) -> RunRegistration {
        let (control, _) = chat_control_channel();
        self.state
            .register_chat_run(
                "reader".into(),
                LOUPIST.into(),
                "research-reader".into(),
                control,
            )
            .unwrap()
    }

    fn stored(&self) -> ChatSession {
        self.state
            .store
            .get_chat_in_loupist(LOUPIST, &self.thread_id)
            .unwrap()
            .unwrap()
    }

    fn message(&self, id: &str) -> ChatMessage {
        self.stored()
            .messages
            .into_iter()
            .find(|message| message.id == id)
            .unwrap_or_else(|| panic!("{id} is in the conversation"))
    }

    fn receipt(&self, reply: &Reply) -> MemoryUpdateResult {
        self.message(&reply.message_id).memory_update.unwrap()
    }

    fn save_finished(&self, reply: &Reply) -> Option<i64> {
        self.message(&reply.message_id)
            .progress
            .unwrap()
            .save_activity
            .unwrap()
            .finished_at_ms
    }

    fn principle_saved(&self, reply: &Reply) -> bool {
        let mut connection = self.state.store.lock_loupist(LOUPIST).unwrap();
        let tx = connection.transaction().unwrap();
        crate::knowledge::principles::principle_read(&tx, LOUPIST, &reply.principle_id).is_ok()
    }

    fn append(&self, message: ChatMessage) {
        let expected = self.stored();
        let mut chat = expected.clone();
        chat.updated_at_ms = now_ms().max(chat.updated_at_ms + 1);
        chat.messages.push(message);
        self.state.store.replace_chat(&expected, &chat).unwrap();
    }

    fn ask(&self, index: usize, text: &str) -> String {
        let id = format!("ask-{index}");
        self.append(crate::test_fixtures::chat_message(
            &id,
            ChatRole::User,
            text,
        ));
        id
    }

    fn pending(&self) -> Vec<String> {
        self.state
            .run_coordinator
            .pending_saves(LOUPIST, Some(&self.thread_id))
            .unwrap()
            .into_iter()
            .map(|save| save.run_id)
            .collect()
    }

    /// The next message of the conversation, sent while earlier replies may
    /// still be saving: its run registers on the same conversation, stages
    /// one change and settles through the real completion path.
    async fn send(
        &self,
        index: usize,
        stage: Stage,
        sealed_cache: Option<crate::domain::PiSessionCache>,
    ) -> Reply {
        let text = match stage {
            Stage::BotNote(_) => "Please keep answers short.".to_owned(),
            _ => format!("Ask {index}"),
        };
        let ask = self.ask(index, &text);
        let run_id = format!("reply-run-{index}");
        let (control, _) = chat_control_channel();
        let registration = self
            .state
            .register_chat_run(
                run_id.clone(),
                LOUPIST.into(),
                self.thread_id.clone(),
                control,
            )
            .expect("the conversation takes its next message");
        assert!(self
            .state
            .claim_run_completion(&run_id, RunKind::Chat)
            .unwrap());
        let message_id = format!("reply-{index}");
        let principle_id = format!("rec_{}", uuid::Uuid::new_v4().simple());
        let mut capture = ToolCapture::for_chat(message_id.clone());
        capture.bound_chat_id = Some(self.thread_id.clone());
        capture.principle_user_message_id = Some(ask);
        capture.surface = Some(self.surface.clone());
        capture.bot_memory_basis = self.surface.is_bot().then(|| self.stored().bot_memory);
        let executor = AppToolExecutor {
            state: self.state.clone(),
            capture: Arc::new(capture),
            loupist_id: LOUPIST.into(),
            capability_ids: Default::default(),
        };
        let (principle, bot_changes) = match stage {
            Stage::Principle => (true, None),
            Stage::BotNote(text) => (
                false,
                Some(
                    json!({"changes": [{"action": "remember", "kind": "preference", "text": text}]}),
                ),
            ),
            Stage::BotChanges(changes) => (false, Some(changes)),
            Stage::BotChangesAndPrinciple(changes) => (true, Some(changes)),
            Stage::Nothing => (false, None),
        };
        if principle {
            executor.capture.staged_principles.lock().await.push(
                crate::commands::tool_executor::StagedPrinciple {
                    record_id: principle_id.clone(),
                    expected_revision_id: None,
                    input: crate::knowledge::PrincipleInput {
                        statement: format!("Check the cash flow statement {index}"),
                        why: None,
                        example: None,
                        origin: "settings".into(),
                        origin_chat_id: None,
                        origin_message_id: None,
                    },
                },
            );
        }
        if let Some(changes) = bot_changes {
            let policy = ToolPolicy {
                loupist_id: LOUPIST.into(),
                session_id: self.thread_id.clone(),
                use_memory: true,
                propose_memory_updates: true,
                memory_proposal_budget: 4,
                as_of: None,
                display_currency: None,
            };
            executor
                .execute(&policy, ToolMethod::BotMemory, changes)
                .await
                .expect("the change is checked against the memory the turn sees");
        }
        let stream = Arc::new(std::sync::Mutex::new(Vec::new()));
        let ended = Arc::new(tokio::sync::Notify::new());
        let (events, end) = (stream.clone(), ended.clone());
        let state = self.state.clone();
        let thread_id = self.thread_id.clone();
        let (task_run, task_message) = (run_id.clone(), message_id.clone());
        let task = tokio::spawn(async move {
            let _registration = registration;
            let sink = ChatEventSink::new(move |event| {
                let last = matches!(
                    event,
                    ChatStreamEvent::Completed { .. }
                        | ChatStreamEvent::Error { .. }
                        | ChatStreamEvent::Aborted { .. }
                );
                events.lock().unwrap().push(event);
                if last {
                    end.notify_one();
                }
                Ok(())
            });
            let harness = crate::agent_harness::snapshot("chat", &[], &[]).unwrap();
            let reply = TerminalReply {
                state: &state,
                on_event: &sink,
                run_id: &task_run,
                loupist_id: LOUPIST,
                thread_id: &thread_id,
                message_id: &task_message,
                memory_revision: &None,
                harness: &harness,
            };
            let mut progress = ChatProgressProjection::new(now_ms());
            let mut failure = None;
            let model = execution_model();
            let title = String::new();
            let _ = completion::commit_completed_turn(completion::CompletedTurn {
                reply: &reply,
                task_loupist_id: LOUPIST,
                execution_model: &model,
                capture: &executor.capture,
                runtime_settled: true,
                save_started_at_ms: now_ms(),
                progress: &mut progress,
                failure: &mut failure,
                content: format!("Answer {index}."),
                sealed_cache,
                task_pins: &[],
                task_update_memory: true,
                should_generate_title: false,
                fallback_title: &title,
                chat_started_at: Instant::now(),
                setup_ms: 0,
                first_text_ms: None,
                answer_text_ms: None,
                generation_ms: None,
                resumed: false,
                token_usage: AssistantTokenUsage::default(),
            })
            .await;
            assert_eq!(failure, None);
        });
        Reply {
            run_id,
            message_id,
            principle_id,
            stream,
            ended,
            task,
        }
    }
}

fn assert_waiting(fixture: &Fixture, reply: &Reply) {
    let stored = fixture.message(&reply.message_id);
    assert_eq!(stored.status, ChatMessageStatus::Complete);
    // Until the save resolves, the stored answer carries the receipt a quit
    // or a crash would leave.
    assert_eq!(
        stored.memory_update.unwrap().skip_reason,
        Some(MemorySaveSkipReason::Interrupted)
    );
    assert_eq!(fixture.save_finished(reply), None);
    assert!(!fixture.principle_saved(reply));
}

#[tokio::test(start_paused = true)]
async fn successive_bot_replies_are_sent_at_once_and_save_in_order_after_the_reader() {
    for count in [2, 3] {
        let fixture = Fixture::bot();
        let reader = fixture.reader();
        let mut replies = Vec::new();
        for index in 0..count {
            let reply = fixture.send(index, Stage::Principle, None).await;
            reply.stream_ended().await;
            assert_eq!(
                fixture.message(&reply.message_id).content,
                format!("Answer {index}.")
            );
            replies.push(reply);
            // A newer turn reads the conversation's text but none of the
            // pending changes of the replies before it.
            for earlier in &replies {
                assert_waiting(&fixture, earlier);
            }
        }
        for reply in &replies {
            let ends = reply.stream_ends();
            assert_eq!(ends.len(), 1, "one end per reply stream");
            assert!(matches!(
                &ends[0],
                ChatStreamEvent::Completed { message_id, memory_save_skip: None, discarded_memory_items: None, save_activity: Some(activity), .. }
                    if *message_id == reply.message_id && activity.finished_at_ms.is_none()
            ));
        }
        assert_eq!(
            fixture.pending(),
            replies
                .iter()
                .map(|reply| reply.run_id.clone())
                .collect::<Vec<_>>()
        );
        // No Chat run holds the conversation; each save is listed by message.
        let activities = fixture.state.run_coordinator.activities().unwrap();
        assert!(activities
            .iter()
            .filter(|activity| activity.target == fixture.thread_id)
            .all(|activity| activity.kind == CHAT_SAVE_ACTIVITY));
        let listed = saves::pending_save_states(&fixture.state, LOUPIST).unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|save| save.message_id.clone())
                .collect::<Vec<_>>(),
            replies
                .iter()
                .map(|reply| reply.message_id.clone())
                .collect::<Vec<_>>()
        );
        assert!(listed.iter().all(|save| save.waiting_for_other_chat));

        drop(reader);
        for reply in &mut replies {
            reply.saved().await;
        }
        let order = replies
            .iter()
            .map(|reply| reply.resolved_at(&fixture.state))
            .collect::<Vec<_>>();
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
        for reply in &replies {
            let receipt = fixture.receipt(reply);
            assert_eq!(receipt.status, MemoryUpdateStatus::Applied, "{receipt:?}");
            assert_eq!(receipt.skip_reason, None);
            assert_eq!(receipt.changes.len(), 1);
            assert!(fixture.principle_saved(reply));
            assert!(fixture.save_finished(reply).is_some());
            // One save's events arrive in order: its waits, then one receipt,
            // which carries the reply exactly as stored.
            let mut events = saves::published(&fixture.state, &reply.run_id)
                .into_iter()
                .map(|(_, event)| event)
                .collect::<Vec<_>>();
            let settled = events.pop().unwrap();
            assert!(events
                .iter()
                .all(|event| event.pending.is_some() && event.message.is_none()));
            assert!(settled.pending.is_none());
            let stored =
                crate::commands::types::chat_dto_with_memory(&fixture.state, &fixture.stored())
                    .unwrap()
                    .messages
                    .into_iter()
                    .find(|message| message.id == reply.message_id)
                    .unwrap();
            assert_eq!(
                serde_json::to_value(settled.message.unwrap()).unwrap(),
                serde_json::to_value(stored).unwrap()
            );
        }
        assert!(fixture.pending().is_empty());
        assert_eq!(fixture.state.run_coordinator.active_count(), 0);
    }
}

#[tokio::test(start_paused = true)]
async fn skipping_the_first_save_leaves_the_second_waiting_and_keeps_both_receipts() {
    let fixture = Fixture::bot();
    let reader = fixture.reader();
    let mut first = fixture.send(0, Stage::Principle, None).await;
    first.stream_ended().await;
    let mut second = fixture.send(1, Stage::Principle, None).await;
    second.stream_ended().await;

    fixture
        .state
        .cancel_run(&first.run_id, RunKind::Chat)
        .await
        .unwrap();
    first.saved().await;
    assert_eq!(
        fixture.receipt(&first).skip_reason,
        Some(MemorySaveSkipReason::User)
    );
    assert!(fixture.save_finished(&first).is_some());
    assert!(!fixture.principle_saved(&first));
    let (_, settled) = saves::published(&fixture.state, &first.run_id)
        .pop()
        .unwrap();
    assert_eq!(
        settled.message.unwrap().memory_update.unwrap().skip_reason,
        Some(MemorySaveSkipReason::User)
    );
    assert_eq!(fixture.pending(), [second.run_id.clone()]);
    assert_waiting(&fixture, &second);

    drop(reader);
    second.saved().await;
    assert_eq!(fixture.receipt(&second).status, MemoryUpdateStatus::Applied);
    assert!(fixture.principle_saved(&second));
    assert_eq!(
        fixture.receipt(&first).skip_reason,
        Some(MemorySaveSkipReason::User),
        "the later save keeps the earlier receipt"
    );
}

#[tokio::test(start_paused = true)]
async fn the_ten_minute_bound_ends_one_save_and_leaves_the_next_waiting() {
    let fixture = Fixture::bot();
    let reader = fixture.reader();
    let mut first = fixture.send(0, Stage::Principle, None).await;
    first.stream_ended().await;
    tokio::time::advance(Duration::from_secs(60)).await;
    let mut second = fixture.send(1, Stage::Principle, None).await;
    second.stream_ended().await;

    tokio::time::advance(Duration::from_secs(540)).await;
    first.saved().await;
    assert_eq!(
        fixture.receipt(&first).skip_reason,
        Some(MemorySaveSkipReason::Deadline)
    );
    let activity = fixture
        .message(&first.message_id)
        .progress
        .unwrap()
        .save_activity
        .unwrap();
    assert_eq!(activity.waited_ms, 600_000);
    assert!(activity.finished_at_ms.is_some());
    assert_eq!(fixture.pending(), [second.run_id.clone()]);

    drop(reader);
    second.saved().await;
    assert_eq!(fixture.receipt(&second).status, MemoryUpdateStatus::Applied);
}

#[tokio::test(start_paused = true)]
async fn history_rewrites_wait_for_a_pending_save_and_try_again_still_works_after_it() {
    let fixture = Fixture::session();
    {
        let expected = fixture.stored();
        let mut chat = expected.clone();
        chat.versions.push(crate::domain::ChatVersion {
            id: "earlier".into(),
            after_message_id: None,
            messages: vec![
                crate::test_fixtures::chat_message("ask-old", ChatRole::User, "Ask old"),
                crate::test_fixtures::chat_message("reply-old", ChatRole::Assistant, "Old."),
            ],
            archived_at_ms: now_ms(),
        });
        fixture.state.store.replace_chat(&expected, &chat).unwrap();
    }
    let reader = fixture.reader();
    let mut first = fixture.send(0, Stage::Principle, None).await;
    first.stream_ended().await;
    // A newer turn of the same conversation failed meanwhile.
    fixture.ask(1, "Ask 1");
    let mut failed = crate::test_fixtures::chat_message("failed-1", ChatRole::Assistant, "Failed.");
    failed.status = ChatMessageStatus::Error;
    fixture.append(failed);

    let branch = |message_id: &str| {
        records::branch_chat(
            &fixture.state,
            crate::commands::types::ChatBranchRequest {
                loupist_id: LOUPIST.into(),
                thread_id: fixture.thread_id.clone(),
                message_id: message_id.into(),
                position: None,
                content: None,
            },
        )
    };
    let switch = || {
        records::switch_version(
            &fixture.state,
            crate::commands::types::ChatVersionSwitchRequest {
                loupist_id: LOUPIST.into(),
                thread_id: fixture.thread_id.clone(),
                version_id: "earlier".into(),
            },
        )
    };
    let rate = |message_id: &str| {
        records::set_feedback(
            &fixture.state,
            crate::commands::types::ChatFeedbackRequest {
                loupist_id: LOUPIST.into(),
                thread_id: fixture.thread_id.clone(),
                message_id: message_id.into(),
                rating: Some(crate::domain::ChatFeedbackRating::Up),
                correction: None,
            },
        )
    };
    let delete =
        || records::delete_chat_session(&fixture.state, LOUPIST, &fixture.thread_id, |_| Ok(()));
    let before = fixture.stored();
    assert_eq!(
        branch("ask-0").unwrap_err().code,
        "conflict",
        "editing its request"
    );
    assert_eq!(
        switch().unwrap_err().code,
        "conflict",
        "switching away from it"
    );
    assert_eq!(rate(&first.message_id).unwrap_err().code, "conflict");
    assert_eq!(delete().unwrap_err().code, "conflict");
    assert_eq!(fixture.stored(), before);
    // Try again only takes a failed or stopped reply, never the one saving.
    assert!(crate::domain::prepare_chat_retry(&fixture.stored(), now_ms()).is_ok());

    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    app.manage(fixture.state.clone());
    let receipt = chat_retry_last(
        ChatRetryRequest {
            loupist_id: LOUPIST.into(),
            thread_id: fixture.thread_id.clone(),
        },
        app.state(),
    )
    .unwrap();
    assert_eq!(receipt.prompt, "Ask 1");
    assert_eq!(
        fixture
            .stored()
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        ["ask-0", "reply-0"]
    );
    assert!(crate::domain::prepare_chat_retry(&fixture.stored(), now_ms()).is_err());
    assert_waiting(&fixture, &first);

    drop(reader);
    first.saved().await;
    assert_eq!(fixture.receipt(&first).status, MemoryUpdateStatus::Applied);
    assert!(rate(&first.message_id).is_ok());
    assert!(branch("ask-0").is_ok());
    drop(app);
}

#[tokio::test(start_paused = true)]
async fn quitting_ends_each_of_two_pending_saves_as_interrupted() {
    let fixture = Fixture::bot();
    let reader = fixture.reader();
    let mut first = fixture.send(0, Stage::Principle, None).await;
    first.stream_ended().await;
    let mut second = fixture.send(1, Stage::Principle, None).await;
    second.stream_ended().await;

    fixture.state.run_coordinator.request_shutdown().unwrap();
    first.saved().await;
    second.saved().await;
    for reply in [&first, &second] {
        assert_eq!(
            fixture.receipt(reply).skip_reason,
            Some(MemorySaveSkipReason::Interrupted)
        );
        assert!(fixture.save_finished(reply).is_some());
        assert!(!fixture.principle_saved(reply));
    }
    drop(reader);
    fixture.state.run_coordinator.drain().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_kill_keeps_two_pending_answers_and_stops_the_newer_turn_at_reopen() {
    let fixture = Fixture::bot();
    let reader = fixture.reader();
    let first = fixture.send(0, Stage::Principle, None).await;
    first.stream_ended().await;
    let second = fixture.send(1, Stage::Principle, None).await;
    second.stream_ended().await;
    // A newer turn had stored its request when the app was killed.
    fixture.ask(2, "Ask 2");
    let (control, _) = chat_control_channel();
    let newer = fixture
        .state
        .register_chat_run(
            "reply-run-2".into(),
            LOUPIST.into(),
            fixture.thread_id.clone(),
            control,
        )
        .unwrap();
    for reply in [first, second] {
        reply.task.abort();
        assert!(reply.task.await.unwrap_err().is_cancelled());
    }
    drop((newer, reader));

    let fixture = fixture.restart();
    let stored = fixture.stored();
    let ids = stored
        .messages
        .iter()
        .map(|message| message.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(&ids[..5], ["ask-0", "reply-0", "ask-1", "reply-1", "ask-2"]);
    for message in &stored.messages[..4] {
        if message.role == ChatRole::Assistant {
            assert_eq!(message.status, ChatMessageStatus::Complete);
            assert_eq!(
                message.memory_update.as_ref().unwrap().skip_reason,
                Some(MemorySaveSkipReason::Interrupted)
            );
        }
    }
    let stopped = stored.messages.last().unwrap();
    assert_eq!(stored.messages.len(), 6);
    assert_eq!(stopped.status, ChatMessageStatus::Aborted);
}

#[tokio::test(start_paused = true)]
async fn results_posted_meanwhile_stay_after_the_reply_whose_save_was_pending() {
    use crate::scheduled_tasks::{
        service,
        types::{RunStatus, Schedule, TaskDraft, TaskStatus, TaskTarget},
    };
    let fixture = Fixture::bot();
    let state = &fixture.state;
    // A research session Bot handed off earlier is still running.
    let staged = handoff::StagedHandoff {
        request: handoff::HandoffRequest::parse(&json!({
            "title": "Supplier check",
            "question": "Who else supplies the part?",
            "requestKey": "suppliers"
        }))
        .unwrap(),
        run: handoff::HandoffRun {
            model_profile_id: "fixture".into(),
            thinking_level: "medium".into(),
            run_options: Default::default(),
            use_memory: false,
            update_memory: false,
        },
    };
    let opened = handoff::commit_standalone(state, LOUPIST, &staged).unwrap();
    let (admission, _) = handoff::claim_fixture(state, &opened, "handoff-run");
    let task = service::create(
        state,
        LOUPIST,
        TaskDraft {
            title: "Morning notes".into(),
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
            run_options: Default::default(),
            use_memory: false,
            update_memory: false,
            status: TaskStatus::Active,
        },
        None,
    )
    .unwrap();

    let reader = fixture.reader();
    let mut first = fixture.send(0, Stage::Principle, None).await;
    first.stream_ended().await;
    // Both finish while the reply's save waits; Bot is free to post them.
    handoff::finish_done_fixture(state, LOUPIST, &opened.id, None).unwrap();
    drop(admission);
    let now = now_ms();
    let (claim, ()) = state
        .store
        .scheduled_task_claim_with(LOUPIST, &task.id, now, true, |_| Ok(()))
        .unwrap()
        .unwrap();
    state
        .store
        .scheduled_run_result(
            LOUPIST,
            &claim.run.run_id,
            "Inventory days fell.",
            "answer-1",
            None,
        )
        .unwrap();
    state
        .store
        .scheduled_task_finish(
            LOUPIST,
            &claim.run.run_id,
            RunStatus::Succeeded,
            None,
            now + 1,
        )
        .unwrap();
    assert_eq!(home::deliver_finished_runs(state, LOUPIST).unwrap(), 2);
    let posted = fixture.stored();
    let reply_at = posted
        .messages
        .iter()
        .position(|message| message.id == first.message_id)
        .unwrap();
    assert_eq!(posted.messages.len(), reply_at + 3);
    assert!(posted.messages[reply_at + 1..]
        .iter()
        .all(|message| message.delivery.is_some() || message.handoff.is_some()));
    assert_waiting(&fixture, &first);

    drop(reader);
    first.saved().await;
    let saved = fixture.stored();
    assert_eq!(fixture.receipt(&first).status, MemoryUpdateStatus::Applied);
    assert_eq!(
        saved.messages[reply_at + 1..],
        posted.messages[reply_at + 1..],
        "the save writes its own reply only"
    );
    assert_eq!(saved.pi_session_cache, None);
}

#[tokio::test(start_paused = true)]
async fn an_earlier_save_never_seals_its_pi_cache_over_a_newer_turn() {
    let fixture = Fixture::bot();
    let reader = fixture.reader();
    let mut first = fixture
        .send(
            0,
            Stage::Principle,
            Some(cache("11111111-1111-4111-8111-111111111111")),
        )
        .await;
    first.stream_ended().await;
    // A newer reply with nothing to save completes at once and seals its cache.
    let newer_cache = cache("22222222-2222-4222-8222-222222222222");
    let mut newer = fixture
        .send(1, Stage::Nothing, Some(newer_cache.clone()))
        .await;
    newer.saved().await;
    assert_eq!(
        fixture.stored().pi_session_cache.as_ref(),
        Some(&newer_cache)
    );
    assert_waiting(&fixture, &first);

    drop(reader);
    first.saved().await;
    assert_eq!(fixture.receipt(&first).status, MemoryUpdateStatus::Applied);
    assert_eq!(fixture.stored().pi_session_cache, Some(newer_cache));
}

#[tokio::test(start_paused = true)]
async fn a_turn_that_began_before_an_earlier_bot_memory_change_seals_no_pi_cache() {
    for earlier_note in [true, false] {
        let fixture = Fixture::bot();
        let reader = fixture.reader();
        let stage = if earlier_note {
            Stage::BotNote("Keep answers short")
        } else {
            Stage::Principle
        };
        let mut first = fixture.send(0, stage, None).await;
        first.stream_ended().await;
        let later_cache = cache("33333333-3333-4333-8333-333333333333");
        let mut second = fixture
            .send(1, Stage::Principle, Some(later_cache.clone()))
            .await;
        second.stream_ended().await;

        drop(reader);
        first.saved().await;
        second.saved().await;
        assert_eq!(fixture.receipt(&second).status, MemoryUpdateStatus::Applied);
        let stored = fixture.stored();
        assert_eq!(!stored.bot_memory.changes.is_empty(), earlier_note);
        assert_eq!(
            stored.pi_session_cache,
            (!earlier_note).then_some(later_cache),
            "a turn sealed with Bot memory older than what the earlier save kept"
        );
    }
}

#[test]
fn a_receipt_written_meanwhile_is_kept_under_the_next_user_message() {
    let fixture = Fixture::session();
    fixture.ask(0, "Ask 0");
    fixture.append(crate::test_fixtures::chat_message(
        "reply-0",
        ChatRole::Assistant,
        "Answer 0.",
    ));
    let expected = fixture.stored();
    let next = |chat: &ChatSession| {
        let mut chat = chat.clone();
        chat.messages.push(crate::test_fixtures::chat_message(
            "ask-1",
            ChatRole::User,
            "Ask 1",
        ));
        chat.updated_at_ms += 1;
        chat
    };
    // The earlier reply's save writes its receipt while the turn sets up.
    let mut receipted = expected.clone();
    receipted.messages[1].memory_update = Some(MemoryUpdateResult {
        discarded_memory_items: None,
        status: MemoryUpdateStatus::NoChange,
        skip_reason: Some(MemorySaveSkipReason::User),
        commit_id: None,
        changes: Vec::new(),
        skipped_kinds: Vec::new(),
        setup: Vec::new(),
    });
    fixture
        .state
        .store
        .replace_chat(&expected, &receipted)
        .unwrap();
    let mut chat = next(&expected);
    saves::append_after_receipts(&fixture.state, &expected, &mut chat).unwrap();
    let stored = fixture.stored();
    assert_eq!(stored, chat);
    assert_eq!(
        stored.messages[1].memory_update,
        receipted.messages[1].memory_update
    );
    assert_eq!(stored.messages[2].id, "ask-1");

    // Any other change keeps the conflict.
    let mut chat = next(&expected);
    assert_eq!(
        saves::append_after_receipts(&fixture.state, &expected, &mut chat)
            .unwrap_err()
            .code,
        "conflict"
    );
}

/// Bot's one kept entry: its id, as `bot_memory.entries` names it.
fn kept_entry(fixture: &Fixture) -> String {
    let entries =
        crate::chat::bot_memory::context(&fixture.stored(), None).block["entries"].clone();
    entries[0]["id"].as_str().unwrap().to_owned()
}

fn left_out_bot_memory(receipt: &MemoryUpdateResult) -> bool {
    receipt
        .discarded_memory_items
        .as_ref()
        .is_some_and(|items| {
            items.contains(&crate::domain::DiscardedMemoryItem {
                kind: crate::domain::DiscardedMemoryKind::BotMemory,
                title: "Bot".into(),
                reason: None,
            })
        })
}

#[tokio::test(start_paused = true)]
async fn a_later_change_to_an_entry_an_earlier_save_resolved_is_left_out_and_the_rest_saves() {
    for earlier in ["resolve", "forget"] {
        let fixture = Fixture::bot();
        let mut kept = fixture
            .send(0, Stage::BotNote("Quote prices in KRW"), None)
            .await;
        kept.saved().await;
        let entry = kept_entry(&fixture);
        let reader = fixture.reader();
        let mut first = fixture
            .send(
                1,
                Stage::BotChanges(json!({"changes": [{"action": earlier, "id": entry}]})),
                None,
            )
            .await;
        first.stream_ended().await;
        // The turn after it was checked against the stored memory, which still has the entry.
        let mut second = fixture
            .send(
                2,
                Stage::BotChangesAndPrinciple(json!({"changes": [
                    {"action": "update", "id": entry, "text": "Quote prices in USD"},
                    {"action": "remember", "kind": "decision", "text": "Keep the margin review monthly"},
                ]})),
                None,
            )
            .await;
        second.stream_ended().await;
        drop(reader);
        first.saved().await;
        second.saved().await;

        assert_eq!(fixture.receipt(&first).skip_reason, None, "{earlier}");
        let receipt = fixture.receipt(&second);
        assert_eq!(receipt.skip_reason, None, "{earlier}: {receipt:?}");
        assert_eq!(receipt.status, MemoryUpdateStatus::Applied);
        assert!(left_out_bot_memory(&receipt), "{earlier}: {receipt:?}");
        assert!(
            fixture.principle_saved(&second),
            "the reply's other Memory saves"
        );
        let texts = crate::chat::bot_memory::context(&fixture.stored(), None).block["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["text"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(texts, ["Keep the margin review monthly"], "{earlier}");
    }
}

#[tokio::test(start_paused = true)]
async fn a_later_note_past_the_entry_limit_is_left_out_and_the_rest_saves() {
    let fixture = Fixture::bot();
    let notes = (0..crate::chat::bot_memory::MAX_ENTRIES - 1)
        .map(|n| json!({"action": "remember", "kind": "context", "text": format!("Context note {n}")}))
        .collect::<Vec<_>>();
    let mut full = fixture
        .send(0, Stage::BotChanges(json!({ "changes": notes })), None)
        .await;
    full.saved().await;
    let reader = fixture.reader();
    let mut first = fixture
        .send(
            1,
            Stage::BotChanges(json!({"changes": [{"action": "remember", "kind": "decision", "text": "Sell half in March"}]})),
            None,
        )
        .await;
    first.stream_ended().await;
    let mut second = fixture
        .send(
            2,
            Stage::BotChangesAndPrinciple(json!({"changes": [{"action": "remember", "kind": "decision", "text": "Buy back in June"}]})),
            None,
        )
        .await;
    second.stream_ended().await;
    drop(reader);
    first.saved().await;
    second.saved().await;

    assert!(!left_out_bot_memory(&fixture.receipt(&first)));
    let receipt = fixture.receipt(&second);
    assert_eq!(receipt.skip_reason, None, "{receipt:?}");
    assert!(left_out_bot_memory(&receipt), "{receipt:?}");
    assert!(fixture.principle_saved(&second));
    let entries =
        crate::chat::bot_memory::context(&fixture.stored(), None).block["entries"].clone();
    assert_eq!(
        entries.as_array().unwrap().len(),
        crate::chat::bot_memory::MAX_ENTRIES
    );
    assert!(entries
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["text"] == "Sell half in March"));
}

#[tokio::test(start_paused = true)]
async fn the_next_turn_does_not_see_an_entry_an_earlier_pending_save_forgets() {
    for (change, hidden) in [("forget", true), ("resolve", true), ("update", false)] {
        let fixture = Fixture::bot();
        let mut kept = fixture
            .send(0, Stage::BotNote("Quote prices in KRW"), None)
            .await;
        kept.saved().await;
        let entry = kept_entry(&fixture);
        let reader = fixture.reader();
        let staged = match change {
            "update" => json!({"changes": [
                {"action": "update", "id": entry, "text": "Quote prices in USD"},
                {"action": "remember", "kind": "decision", "text": "Sell half in March"},
            ]}),
            action => json!({"changes": [{"action": action, "id": entry}]}),
        };
        let mut first = fixture.send(1, Stage::BotChanges(staged), None).await;
        first.stream_ended().await;
        // What the next turn's setup builds its Bot memory envelope from.
        let next = || {
            let (review, leaving) =
                saves::pending_bot_memory(&fixture.state, LOUPIST, &fixture.thread_id).unwrap();
            crate::chat::bot_memory::context_after(&fixture.stored(), None, &review, &leaving).block
                ["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["text"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        // An added or updated entry is not shown before its save applies.
        let expected: &[&str] = if hidden {
            &[]
        } else {
            &["Quote prices in KRW"]
        };
        assert_eq!(next(), expected, "{change}");
        // Skipped, the earlier save leaves the entry where it was.
        fixture
            .state
            .cancel_run(&first.run_id, RunKind::Chat)
            .await
            .unwrap();
        first.saved().await;
        assert_eq!(next(), ["Quote prices in KRW"], "{change}");
        drop(reader);
    }
}

/// Tests run side by side and reuse run ids. A save's events are read by the app
/// that published them: another app's later skip of the same run id is not this
/// reply's receipt (the interleaving a Linux runner hit).
#[tokio::test(start_paused = true)]
async fn each_app_reads_only_the_save_events_it_published() {
    let applied = Fixture::bot();
    let mut saved = applied.send(0, Stage::Principle, None).await;
    saved.saved().await;
    let skipped = Fixture::bot();
    let reader = skipped.reader();
    let mut waiting = skipped.send(0, Stage::Principle, None).await;
    waiting.stream_ended().await;
    skipped
        .state
        .cancel_run(&waiting.run_id, RunKind::Chat)
        .await
        .unwrap();
    waiting.saved().await;
    drop(reader);
    assert_eq!(saved.run_id, waiting.run_id);

    let last = |fixture: &Fixture, reply: &Reply| {
        let (_, event) = saves::published(&fixture.state, &reply.run_id)
            .pop()
            .unwrap();
        assert!(event.pending.is_none());
        event.message.unwrap().memory_update.unwrap()
    };
    assert_eq!(last(&applied, &saved).status, MemoryUpdateStatus::Applied);
    assert_eq!(
        last(&skipped, &waiting).skip_reason,
        Some(MemorySaveSkipReason::User)
    );
}
