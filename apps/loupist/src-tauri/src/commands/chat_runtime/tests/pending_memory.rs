use super::*;
use crate::{
    broker::{ToolExecutor as _, ToolMethod, ToolPolicy},
    chat::surface::ChatSurface,
    commands::{assets::with_assets, knowledge::knowledge_blobs_for_loupist},
    domain::{MemoryUpdateChange, MemoryUpdateStatus},
    knowledge::{self, GraphProcessingSettings, KnowledgeResult, KnowledgeWrite},
};
use serde_json::Value;

pub(super) async fn commit_test_reply(
    state: &AppState,
    chat: &crate::domain::ChatSession,
    capture: &ToolCapture,
    content: &str,
    while_pending: Option<impl FnOnce()>,
) -> ChatMessage {
    let wait = while_pending.is_some();
    let loupist = chat.loupist_id.as_str();
    let older = while_pending.as_ref().map(|_| {
        let (control, _) = chat_control_channel();
        state
            .register_chat_run("older".into(), loupist.into(), "older-chat".into(), control)
            .unwrap()
    });
    let (control, _) = chat_control_channel();
    let _run = state
        .register_chat_run("settled".into(), loupist.into(), chat.id.clone(), control)
        .unwrap();
    assert!(state
        .claim_run_completion("settled", RunKind::Chat)
        .unwrap());
    let pending = Arc::new(tokio::sync::Notify::new());
    let queued = pending.clone();
    // An interactive reply's stream ends with its stored answer while the
    // save waits on its own; another reply's stream reports the wait itself.
    let sink = ChatEventSink::new(move |event| {
        if matches!(
            event,
            ChatStreamEvent::Saving {
                waiting_for_other_chat: true,
                ..
            } | ChatStreamEvent::Completed { .. }
        ) {
            queued.notify_one();
        }
        Ok(())
    });
    let reply = TerminalReply {
        state,
        on_event: &sink,
        run_id: "settled",
        loupist_id: loupist,
        thread_id: &chat.id,
        message_id: "settled-reply",
        memory_revision: &None,
        harness: &crate::agent_harness::snapshot("chat", &[], &[]).unwrap(),
    };
    let mut progress = ChatProgressProjection::new(now_ms());
    let mut failure = None;
    let model = execution_model();
    let title = String::new();
    let mut save = Box::pin(tokio::time::timeout(
        Duration::from_secs(660),
        completion::commit_completed_turn(completion::CompletedTurn {
            reply: &reply,
            task_loupist_id: loupist,
            execution_model: &model,
            capture,
            runtime_settled: true,
            save_started_at_ms: now_ms(),
            progress: &mut progress,
            failure: &mut failure,
            content: content.into(),
            sealed_cache: None,
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
        }),
    ));
    if let (Some(reader), Some(while_pending)) = (older, while_pending) {
        tokio::select! {
            _ = pending.notified() => {}
            _ = &mut save => panic!("save finished before the older reader drained"),
        }
        tokio::time::advance(Duration::from_secs(599)).await;
        while_pending();
        drop(reader);
    }
    let _ = save.await.expect("the test reply should finish saving");
    let persisted = state.store.get_chat(&chat.id).unwrap().unwrap();
    let message = persisted.messages.into_iter().last().unwrap();
    assert_eq!(
        message
            .progress
            .as_ref()
            .unwrap()
            .save_activity
            .as_ref()
            .unwrap()
            .waited_ms,
        if wait { 599_000 } else { 0 },
    );
    message
}

const MATERIAL: &str = "The manufacturer published a new capacity estimate.";
const USER_MESSAGE: &str = "save-request";

struct PendingMemoryFixture {
    _root: tempfile::TempDir,
    chat: crate::domain::ChatSession,
    executor: AppToolExecutor,
    policy: ToolPolicy,
}

impl PendingMemoryFixture {
    fn new(user_text: &str) -> Self {
        Self::for_surface(user_text, ChatSurface::Session)
    }

    fn for_surface(user_text: &str, surface: ChatSurface) -> Self {
        use crate::commands::tests::{chat, profile, seed_profile};

        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        let profile = profile("pending-memory", &root.path().join("workspace"), 1);
        seed_profile(&state.store, &profile);
        let thread_id = if surface.is_bot() {
            crate::chat::home::home_thread_id(&profile.id)
        } else {
            "pending-chat".into()
        };
        let mut chat = chat(&thread_id, &profile.id, 1);
        chat.messages.push(crate::test_fixtures::chat_message(
            USER_MESSAGE,
            ChatRole::User,
            user_text,
        ));
        state.store.create_chat(&chat).unwrap();
        let policy = ToolPolicy {
            loupist_id: profile.id.clone(),
            session_id: chat.id.clone(),
            use_memory: true,
            propose_memory_updates: true,
            memory_proposal_budget: 4,
            as_of: None,
            display_currency: None,
        };
        let mut capture = ToolCapture::for_chat("settled-reply".into());
        capture.bound_chat_id = Some(chat.id.clone());
        capture.principle_user_message_id = Some(USER_MESSAGE.into());
        capture.surface = Some(surface);
        let executor = AppToolExecutor {
            state,
            capture: Arc::new(capture),
            loupist_id: profile.id,
            capability_ids: Default::default(),
        };
        Self {
            _root: root,
            chat,
            executor,
            policy,
        }
    }

    async fn stage(&self, method: ToolMethod, arguments: Value) -> Value {
        self.executor
            .execute(&self.policy, method, arguments)
            .await
            .unwrap()
    }

    async fn stage_source(&self) -> Value {
        let pasted = self
            .stage(ToolMethod::PastedCapture, json!({"text": MATERIAL}))
            .await;
        self.stage(ToolMethod::SourceSave, json!({
            "result_ref": pasted["result_ref"], "title": "Capacity notice", "coverage": "included_full",
        })).await["saveRef"].clone()
    }

    async fn finish(&self, kind: &str) -> MemoryUpdateChange {
        let message = commit_test_reply(
            &self.executor.state,
            &self.chat,
            &self.executor.capture,
            "I kept what you asked for.",
            Some(|| {}),
        )
        .await;
        let update = message.memory_update.unwrap();
        assert_eq!(update.status, MemoryUpdateStatus::Applied, "{update:?}");
        assert!(update.discarded_memory_items.is_none());
        let mut changes = update
            .changes
            .into_iter()
            .filter(|change| change.kind == kind);
        let change = changes.next().unwrap();
        assert!(changes.next().is_none());
        change
    }

    fn read(
        &self,
        verify: impl FnOnce(&rusqlite::Transaction<'_>, KnowledgeWrite<'_>, &str) -> KnowledgeResult<()>,
    ) {
        let state = &self.executor.state;
        let loupist = &self.policy.loupist_id;
        let blobs = knowledge_blobs_for_loupist(state, loupist).unwrap();
        with_assets(state, loupist, false, |tx| {
            verify(tx, KnowledgeWrite::bind(tx, &blobs), loupist)
        })
        .unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn an_explicit_bot_memory_edit_waits_for_the_older_reader() {
    let fixture = PendingMemoryFixture::for_surface("Please keep answers short.", ChatSurface::Bot);
    fixture
        .stage(
            ToolMethod::BotMemory,
            json!({"changes": [
                {"action": "remember", "kind": "preference", "text": "Keep answers short"}
            ]}),
        )
        .await;
    let state = &fixture.executor.state;
    let chat = &fixture.chat;
    let reply = commit_test_reply(
        state,
        chat,
        &fixture.executor.capture,
        "Will do.",
        Some(|| {
            let waiting = state.store.get_chat(&chat.id).unwrap().unwrap();
            assert!(waiting.bot_memory.unreviewed.is_empty());
        }),
    )
    .await;
    assert!(reply.memory_update.unwrap().skip_reason.is_none());
    let stored = state.store.get_chat(&chat.id).unwrap().unwrap();
    let context = crate::chat::bot_memory::context(&stored, None);
    assert_eq!(context.block["entries"][0]["text"], "Keep answers short");
}

#[tokio::test(start_paused = true)]
async fn a_staged_bot_hand_off_waits_for_the_older_reader() {
    use crate::chat::handoff::{self, HandoffRequest, HandoffRun, StagedHandoff};

    let fixture =
        PendingMemoryFixture::for_surface("Compare these manufacturers.", ChatSurface::Bot);
    let capture = &fixture.executor.capture;
    *capture.staged_handoff.lock().await = Some(StagedHandoff {
        request: HandoffRequest::parse(&json!({
            "title": "Manufacturer comparison",
            "question": "Compare these manufacturers.",
            "requestKey": "manufacturers"
        }))
        .unwrap(),
        run: HandoffRun {
            model_profile_id: "fixture".into(),
            thinking_level: "medium".into(),
            run_options: Default::default(),
            use_memory: true,
            update_memory: true,
        },
    });
    let state = &fixture.executor.state;
    let chat = &fixture.chat;
    let reply = commit_test_reply(
        state,
        chat,
        capture,
        "I opened a session.",
        Some(|| {
            assert!(handoff::list(state, &chat.loupist_id).unwrap().is_empty());
        }),
    )
    .await;
    let handoffs = handoff::list(state, &chat.loupist_id).unwrap();
    assert_eq!(handoffs.len(), 1);
    assert_eq!(
        reply.research_session_id.as_deref(),
        Some(handoffs[0].chat_id.as_str())
    );
}

#[tokio::test(start_paused = true)]
async fn a_pending_save_keeps_the_source_bytes() {
    let fixture = PendingMemoryFixture::new(MATERIAL);
    fixture.stage_source().await;
    let change = fixture.finish("Source").await;
    let target = change.knowledge.unwrap();
    fixture.read(|_, write, _| {
        let (bytes, _) = write.read_revision_span_bytes(
            &target.record_id,
            target.span_id.as_deref().unwrap(),
            &crate::util::sha256(MATERIAL.as_bytes()),
        )?;
        assert_eq!(bytes, MATERIAL.as_bytes());
        Ok(())
    });
}

#[tokio::test(start_paused = true)]
async fn a_pending_save_keeps_a_lens_section() {
    let fixture =
        PendingMemoryFixture::new("Keep this method. Compare design and operating capacity.");
    fixture.stage(ToolMethod::LensFile, json!({
        "pages": [{
            "page": {"create": {"kind": "method", "title": "Capacity checks"}},
            "ops": [
                {"op": "put", "key": "steps", "heading": "Steps", "markdown": "Compare design and operating capacity.", "basis": {"refs": [{"said": true}]}},
                {"op": "summary", "text": "Check capacity by comparing its design and operating figures.", "said": true}
            ]
        }],
        "rationale": "Keep the comparison method."
    })).await;
    let change = fixture.finish("Lens").await;
    fixture.read(|_, write, loupist| {
        let page = write.read_page(loupist, &change.record_id)?;
        assert_eq!(page.title, "Capacity checks");
        assert_eq!(
            page.body.summary(),
            "Check capacity by comparing its design and operating figures."
        );
        assert_eq!(page.body.sections()[0].heading, "Steps");
        Ok(())
    });
}

#[tokio::test(start_paused = true)]
async fn a_pending_save_keeps_a_journal_entry() {
    let fixture = PendingMemoryFixture::new(MATERIAL);
    let source = fixture.stage_source().await;
    let judgment = "Treat the capacity increase as provisional.";
    fixture
        .stage(
            ToolMethod::JournalSave,
            json!({"judgment": judgment, "sourceReads": [source]}),
        )
        .await;
    let change = fixture.finish("Journal").await;
    fixture.read(|_, write, loupist| {
        assert_eq!(
            write.get_journal(loupist, &change.record_id)?.judgment,
            judgment
        );
        Ok(())
    });
}

#[tokio::test(start_paused = true)]
async fn a_pending_save_keeps_a_working_note() {
    let fixture = PendingMemoryFixture::new("Keep a note about checking operating capacity.");
    let markdown = "Check operating capacity against the next filing.";
    fixture.stage(ToolMethod::NoteSave, json!({
        "workingNote": {"title":"Capacity check","markdown":markdown,"changeSummary":"Started the capacity check"}
    })).await;
    let change = fixture.finish("Research").await;
    fixture.read(|_, write, loupist| {
        assert_eq!(
            write
                .read_working_note(loupist, &change.record_id)?
                .markdown,
            markdown
        );
        Ok(())
    });
}

#[tokio::test(start_paused = true)]
async fn a_pending_save_keeps_the_world_enqueue() {
    let fixture = PendingMemoryFixture::new(MATERIAL);
    let loupist = &fixture.policy.loupist_id;
    with_assets(&fixture.executor.state, loupist, true, |tx| {
        knowledge::store_settings(
            tx,
            loupist,
            &GraphProcessingSettings {
                enrichment_enabled: true,
                paused: true,
                helper_provider: Some("openai".into()),
                helper_model: Some("fixture-model".into()),
                ..Default::default()
            },
        )
    })
    .unwrap();
    fixture.stage_source().await;
    let change = fixture.finish("Source").await;
    fixture.read(|tx, _, loupist| {
        let job = knowledge::get_job(tx, loupist, &change.world_job_id.unwrap())?;
        assert_eq!(job.status, "paused_budget");
        assert_eq!(
            job.document.source_binding.unwrap().source_record_id,
            change.knowledge.unwrap().record_id
        );
        Ok(())
    });
}

/// A research reply may keep every Source its budget admits and file several
/// Lens pages. The receipt listing them must not refuse the turn's whole save.
#[tokio::test(start_paused = true)]
async fn a_reply_keeping_sixteen_sources_and_three_lens_pages_saves_them_all() {
    let notices = (1..=crate::broker::MAX_CHAT_TURN_SOURCES)
        .map(|index| format!("Notice {index}: the maker revised capacity line {index}."))
        .collect::<Vec<_>>();
    let fixture = PendingMemoryFixture::new(&format!(
        "Keep these methods and notices.\n{}",
        notices.join("\n")
    ));
    for (index, notice) in notices.iter().enumerate() {
        let pasted = fixture
            .stage(ToolMethod::PastedCapture, json!({"text": notice}))
            .await;
        fixture.stage(ToolMethod::SourceSave, json!({
            "result_ref": pasted["result_ref"], "title": format!("Capacity notice {}", index + 1), "coverage": "included_full",
        })).await;
    }
    let pages = ["Capacity checks", "Utilization checks", "Inventory checks"].map(|title| json!({
        "page": {"create": {"kind": "method", "title": title}},
        "ops": [
            {"op": "put", "key": "steps", "heading": "Steps", "markdown": "Compare design and operating capacity.", "basis": {"refs": [{"said": true}]}},
            {"op": "summary", "text": format!("{title} compare design and operating figures."), "said": true}
        ]
    }));
    fixture
        .stage(
            ToolMethod::LensFile,
            json!({"pages": pages, "rationale": "Keep the comparison methods."}),
        )
        .await;
    let message = commit_test_reply(
        &fixture.executor.state,
        &fixture.chat,
        &fixture.executor.capture,
        "I kept the notices and filed three methods.",
        Some(|| {}),
    )
    .await;
    let update = message.memory_update.unwrap();
    assert_eq!(update.skip_reason, None, "{update:?}");
    assert_eq!(update.status, MemoryUpdateStatus::Applied);
    let count = |kind: &str| {
        update
            .changes
            .iter()
            .filter(|change| change.kind == kind)
            .count()
    };
    assert_eq!(count("Source"), 16);
    assert_eq!(count("Lens"), 3);
}

/// Another writer, such as a World publication, holds the Loupist's store for
/// `hold` from the moment the save waits, then lets go.
fn hold_the_store<'h>(
    fixture: &PendingMemoryFixture,
    hold: Duration,
    holder: &'h std::cell::RefCell<Option<std::thread::JoinHandle<()>>>,
) -> impl FnOnce() + 'h {
    let state = fixture.executor.state.clone();
    let loupist = fixture.policy.loupist_id.clone();
    move || {
        let (locked, store_held) = std::sync::mpsc::channel();
        *holder.borrow_mut() = Some(std::thread::spawn(move || {
            let _connection = state.store.lock_loupist(&loupist).unwrap();
            locked.send(()).unwrap();
            std::thread::sleep(hold);
        }));
        store_held.recv().unwrap();
    }
}

fn source_records(fixture: &PendingMemoryFixture) -> i64 {
    let mut count = 0;
    fixture.read(|tx, _, loupist| {
        count = tx.query_row(
            "SELECT COUNT(*) FROM knowledge_records WHERE owner_loupist_id = ?1 AND kind = 'source'",
            [loupist],
            |row| row.get(0),
        )?;
        Ok(())
    });
    count
}

/// A save whose store stays busy through every retry says so, rather than only
/// that saving failed, and lists what it did not keep.
#[tokio::test(start_paused = true)]
async fn a_save_behind_a_store_busy_past_every_retry_names_the_cause() {
    crate::store::TEST_SAVE_BUDGET.set(Some(Duration::from_secs(1)));
    let fixture = PendingMemoryFixture::new(MATERIAL);
    fixture.stage_source().await;
    let holder = std::cell::RefCell::new(None);
    // Three attempts of a second each; the skipped receipt is written after.
    let message = commit_test_reply(
        &fixture.executor.state,
        &fixture.chat,
        &fixture.executor.capture,
        "I kept the notice.",
        Some(hold_the_store(
            &fixture,
            Duration::from_millis(3_500),
            &holder,
        )),
    )
    .await;
    holder.into_inner().unwrap().join().unwrap();
    let update = message.memory_update.unwrap();
    assert_eq!(
        update.skip_reason,
        Some(crate::domain::MemorySaveSkipReason::StoreBusy)
    );
    assert_eq!(
        update.discarded_memory_items.unwrap()[0].title,
        "Capacity notice"
    );
    assert_eq!(source_records(&fixture), 0);
}

#[tokio::test(start_paused = true)]
async fn a_save_behind_a_busy_store_saves_on_a_retry_and_only_once() {
    crate::store::TEST_SAVE_BUDGET.set(Some(Duration::from_secs(1)));
    let fixture = PendingMemoryFixture::new(MATERIAL);
    fixture.stage_source().await;
    let holder = std::cell::RefCell::new(None);
    let message = commit_test_reply(
        &fixture.executor.state,
        &fixture.chat,
        &fixture.executor.capture,
        "I kept the notice.",
        Some(hold_the_store(
            &fixture,
            Duration::from_millis(1_500),
            &holder,
        )),
    )
    .await;
    holder.into_inner().unwrap().join().unwrap();
    let update = message.memory_update.unwrap();
    assert_eq!(update.skip_reason, None, "{update:?}");
    assert_eq!(update.status, MemoryUpdateStatus::Applied);
    assert!(update.discarded_memory_items.is_none());
    assert_eq!(
        update
            .changes
            .iter()
            .filter(|change| change.kind == "Source")
            .count(),
        1
    );
    assert_eq!(source_records(&fixture), 1);
}

/// One refused item does not lose the others: its stage is left out, the rest
/// saves, and the receipt names what was not kept and why.
#[tokio::test(start_paused = true)]
async fn one_refused_item_is_left_out_and_the_rest_of_the_turn_saves() {
    let fixture = PendingMemoryFixture::new(MATERIAL);
    fixture.stage_source().await;
    fixture
        .stage(
            ToolMethod::LensFile,
            json!({"pages": [{
                "page": {"create": {"kind": "method", "title": "Capacity checks"}},
                "ops": [
                    {"op": "put", "key": "steps", "heading": "Steps", "markdown": "Compare design and operating capacity.", "basis": {"refs": [{"said": true}]}},
                    {"op": "summary", "text": "Capacity checks compare design and operating figures.", "said": true}
                ]
            }], "rationale": "Keep the capacity method."}),
        )
        .await;
    // A principle edit naming a revision that does not exist is refused.
    fixture
        .executor
        .capture
        .staged_principles
        .lock()
        .await
        .push(crate::commands::tool_executor::StagedPrinciple {
            record_id: format!("rec_{}", uuid::Uuid::new_v4().simple()),
            expected_revision_id: Some(format!("rev_{}", uuid::Uuid::new_v4().simple())),
            input: crate::knowledge::PrincipleInput {
                statement: "Check cash flow in both years".into(),
                why: None,
                example: None,
                origin: "settings".into(),
                origin_chat_id: None,
                origin_message_id: None,
            },
        });
    let message = commit_test_reply(
        &fixture.executor.state,
        &fixture.chat,
        &fixture.executor.capture,
        "I kept the notice and the principle.",
        Some(|| {}),
    )
    .await;
    let update = message.memory_update.unwrap();
    assert_eq!(update.skip_reason, None, "{update:?}");
    assert_eq!(update.status, MemoryUpdateStatus::Applied);
    assert_eq!(
        update
            .changes
            .iter()
            .map(|change| change.kind.as_str())
            .collect::<Vec<_>>(),
        ["Source", "Lens"]
    );
    let left_out = update.discarded_memory_items.unwrap();
    assert_eq!(left_out.len(), 1, "{left_out:?}");
    assert_eq!(
        left_out[0].kind,
        crate::domain::DiscardedMemoryKind::Principle
    );
    assert_eq!(
        left_out[0].reason,
        Some(crate::domain::MemorySaveSkipReason::Invalid)
    );
    assert_eq!(source_records(&fixture), 1);
}
