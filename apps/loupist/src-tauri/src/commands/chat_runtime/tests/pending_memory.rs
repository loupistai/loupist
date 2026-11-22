use super::*;
use crate::{
    broker::{ToolExecutor as _, ToolMethod, ToolPolicy},
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
    let sink = ChatEventSink::new(move |event| {
        if matches!(
            event,
            ChatStreamEvent::Saving {
                waiting_for_other_chat: true,
                ..
            }
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
        use crate::commands::tests::{chat, profile, seed_profile};

        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        let profile = profile("pending-memory", &root.path().join("workspace"), 1);
        seed_profile(&state.store, &profile);
        let mut chat = chat("pending-chat", &profile.id, 1);
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
        let mut capture = ToolCapture::default();
        capture.bound_chat_id = Some(chat.id.clone());
        capture.principle_user_message_id = Some(USER_MESSAGE.into());
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
            "ops": [{"op": "put", "key": "steps", "heading": "Steps", "markdown": "Compare design and operating capacity.", "basis": {"refs": [{"said": true}]}}]
        }],
        "rationale": "Keep the comparison method."
    })).await;
    let change = fixture.finish("Lens").await;
    fixture.read(|_, write, loupist| {
        let page = write.read_page(loupist, &change.record_id)?;
        assert_eq!(page.title, "Capacity checks");
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
