//! `pasted_capture` and `source_save`: material the user pasted into a Chat
//! becomes a user-provided Source copied from the user's own message.
use super::*;
use crate::domain::{ChatToolRefusalReason, HandoffNote, NumberCheckKind, NumberCheckStatus};
use crate::knowledge::{Authorship, Coverage, ProvidedOrigin, SourceClass};

const URL: &str = "https://fictional.example/solsup/backlog-2025";
const PASTED: &str = "솔숲식품이 2025년 3월 12일에 알린 수주잔고 공시야. 본문은 이렇다.\n\n솔숲식품은 2025년 3월 12일 현재\n수주잔고 수치가 184.6이고 단위는 십억 원이라고 알렸다. 출처 주소는 https://fictional.example/solsup/backlog-2025 이다. 정리해 줘.";
/// The body as the user's message holds it, line break included.
const BODY: &str =
    "솔숲식품은 2025년 3월 12일 현재\n수주잔고 수치가 184.6이고 단위는 십억 원이라고 알렸다.";
/// The same body as a model would retype it on one line.
const RETYPED: &str =
    "솔숲식품은 2025년 3월 12일 현재 수주잔고 수치가 184.6이고 단위는 십억 원이라고 알렸다.";
const PASTED_AT_MS: i64 = 1_795_124_420_893;

fn user(id: &str, content: &str, created_at_ms: i64) -> ChatMessage {
    ChatMessage {
        created_at_ms,
        ..chat_message(id, ChatRole::User, content)
    }
}

/// loupist-a with chat-a holding `messages` and an empty chat-b.
fn pasting_state(temporary: &tempfile::TempDir, messages: Vec<ChatMessage>) -> AppState {
    let state = AppState::for_test(temporary.path().join("app"));
    seed_loupist(&state, temporary, "loupist-a");
    let mut session = chat("chat-a", "loupist-a", 1);
    session.messages = messages;
    state.store.create_chat(&session).unwrap();
    state
}

fn chat_a_turn() -> Arc<ToolCapture> {
    Arc::new(ToolCapture {
        bound_chat_id: Some("chat-a".into()),
        principle_user_message_id: Some("user-paste".into()),
        ..ToolCapture::for_chat(new_id("reply"))
    })
}

async fn capture_paste(
    executor: &AppToolExecutor,
    policy: &ToolPolicy,
    args: Value,
) -> Result<Value, BrokerError> {
    let delivery = new_id("paste");
    let result = executor
        .execute_for_delivery(policy, ToolMethod::PastedCapture, args, &delivery)
        .await?;
    executor.commit_delivery(policy, &delivery).await.unwrap();
    Ok(result)
}

fn invalid_field(error: &BrokerError) -> &str {
    match error {
        BrokerError::InvalidArguments { field, .. } => field,
        other => panic!("expected a field refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn pasted_text_is_kept_as_the_users_own_bytes_in_a_user_provided_source() {
    let temporary = tempfile::tempdir().unwrap();
    let state = pasting_state(&temporary, vec![user("user-paste", PASTED, PASTED_AT_MS)]);
    let capture = chat_a_turn();
    let executor = executor_for(&state, capture.clone());
    let policy = policy(true, true);
    let captured = capture_paste(
        &executor,
        &policy,
        json!({"text": RETYPED, "url": URL, "publisher": "솔숲식품"}),
    )
    .await
    .unwrap();
    // The bytes come from the user's message, never from the model's retyping.
    assert_eq!(captured["content"], BODY);
    assert_eq!(captured["source_class"], "user_provided");
    assert_eq!(captured["pasted"]["message_id"], "user-paste");
    assert_eq!(captured["pasted_at"], "2026-11-19T21:40:20.893Z");
    assert_eq!(captured["stated_url"], URL);
    assert_eq!(captured["stated_publisher"], "솔숲식품");
    assert!(captured["next"].is_string());
    let citation = captured["citation_url"].as_str().unwrap();
    assert!(citation.starts_with("#evidence-ev_"), "{captured}");

    // The answer can cite it in this turn, and a number check labels it
    // user-provided and finds the figure in what the user pasted.
    let result_ref = captured["result_ref"].as_str().unwrap().to_owned();
    let result = capture.run_result(&result_ref).await.unwrap();
    assert_eq!(
        result.origin, None,
        "a stated link is not where the text was read"
    );
    let evidence = super::super::evidence::project(&result);
    assert_eq!(evidence.kind, crate::domain::ChatEvidenceKind::UserProvided);
    assert_eq!(evidence.url, None);
    assert_eq!(evidence.excerpt, BODY);
    let link = crate::commands::tool_executor::number_checks::EvidenceLink {
        index: 0,
        text: "1,846억 원".into(),
        evidence_id: evidence.id.clone(),
    };
    let check = crate::commands::tool_executor::number_checks::check_link(
        &link,
        &result,
        Some(&evidence),
        &RunResultRegistry::default(),
    )
    .unwrap();
    assert_eq!(check.kind, NumberCheckKind::Passage);
    assert_eq!(check.status, NumberCheckStatus::Rounded);

    // Without saving on, the capture is evidence only.
    let read_only = capture_paste(
        &executor,
        &super::policy(true, false),
        json!({"text": RETYPED}),
    )
    .await
    .unwrap();
    assert!(read_only.get("next").is_none());

    let saved = executor
        .stage_source_save(
            &policy,
            json!({"result_ref": result_ref, "title": "Solsup backlog", "coverage": "included_excerpt",
                "publicationTime": {"precision": "date", "value": "2025-03-12"}}),
            "save",
        )
        .await
        .unwrap();
    assert_eq!(saved["status"], "staged");
    executor.commit_delivery(&policy, "save").await.unwrap();
    let staged = capture.staged_sources.lock().await.clone();
    assert_eq!(staged.len(), 1);
    let source = &staged[0];
    assert_eq!(source.source_class, SourceClass::UserProvided);
    assert_eq!(source.authorship, Authorship::User);
    assert_eq!(source.bytes, BODY.as_bytes());
    assert_eq!(source.origin, URL);
    assert_eq!(source.provider_identity, "user");
    assert_eq!(source.material_role, "excerpt");
    let provided = ProvidedOrigin {
        chat_id: "chat-a".into(),
        message_id: "user-paste".into(),
        pasted_at_ms: PASTED_AT_MS,
        stated_url: Some(URL.into()),
        stated_publisher: Some("솔숲식품".into()),
    };
    assert_eq!(source.provided.as_ref(), Some(&provided));

    // Both acknowledged saves in this reply resolve to the final attribution.
    let mut earlier = source.clone();
    earlier.title = "Unattributed excerpt".into();
    earlier.publication_time = None;
    earlier.request_ref = "save-earlier".into();
    earlier.provided.as_mut().unwrap().stated_publisher = None;
    let staged = [earlier, source.clone()];
    let changes = {
        let blobs = crate::commands::knowledge::knowledge_blobs(&state).unwrap();
        let mut connection = state.store.lock().unwrap();
        let tx = connection.transaction().unwrap();
        let changes = crate::commands::knowledge::persist_saved_turn(
            &tx,
            &blobs,
            "loupist-a",
            "chat-a",
            "turn-paste",
            now_ms(),
            &[],
            true,
            &staged,
            &[],
            None,
        )
        .unwrap();
        tx.commit().unwrap();
        changes
    };
    assert_eq!(changes.len(), 1);
    assert!(changes[0].user_provided);
    assert_eq!(changes[0].title, "Solsup backlog");
    assert_eq!(
        serde_json::to_value(&changes[0]).unwrap()["userProvided"],
        true
    );
    let catalog = crate::commands::knowledge::catalog(
        &state,
        crate::commands::knowledge::KnowledgeCatalogRequest {
            loupist_id: "loupist-a".into(),
        },
    )
    .unwrap();
    let record = catalog
        .sources
        .iter()
        .find(|record| record.title == "Solsup backlog")
        .unwrap();
    assert!(record.objects[0].user_provided);
    assert_eq!(record.objects[0].origin_label.as_deref(), Some(URL));
    let detail = crate::commands::knowledge::source_read(
        &state,
        crate::commands::knowledge::KnowledgeSourceReadRequest {
            loupist_id: "loupist-a".into(),
            record_id: record.record_id.clone(),
            revision_id: record.revision_id.clone(),
            source_id: None,
            span_id: None,
            purpose: "renderer_inspection".into(),
            world_record_id: None,
            world_revision_id: None,
        },
    )
    .unwrap();
    assert_eq!(detail.source_class, "user_provided");
    assert_eq!(detail.provided.as_ref(), Some(&provided));
    assert_eq!(detail.spans[0].text.as_deref(), Some(BODY));
    assert_eq!(
        detail.publication_time.unwrap().value.as_deref(),
        Some("2025-03-12")
    );
}

#[tokio::test]
async fn only_text_the_user_sent_in_this_chat_can_be_captured() {
    let temporary = tempfile::tempdir().unwrap();
    let mut assistant = chat_message(
        "reply-1",
        ChatRole::Assistant,
        "My summary: Solsup reported a backlog of 184.6 billion won as of 12 March 2025.",
    );
    assistant.created_at_ms = PASTED_AT_MS + 10;
    let handoff = ChatMessage {
        handoff_note: Some(HandoffNote {
            title: "Backlog".into(),
            question: Some("Check the order backlog".into()),
            brief: None,
            untrusted_id: None,
        }),
        ..user(
            "handoff-1",
            "Check the order backlog of 990 billion won for Hanul Mills.",
            PASTED_AT_MS + 20,
        )
    };
    let state = pasting_state(
        &temporary,
        vec![user("user-paste", PASTED, PASTED_AT_MS), assistant, handoff],
    );
    let mut other = chat("chat-b", "loupist-a", 1);
    other.messages = vec![user(
        "user-other",
        "Marrow Glass original FY2024 gross margin was 40.0 percent.",
        PASTED_AT_MS,
    )];
    state.store.create_chat(&other).unwrap();
    let capture = chat_a_turn();
    let executor = executor_for(&state, capture.clone());
    let policy = policy(true, true);
    let refused = |args: Value| {
        let executor = executor.clone();
        let policy = policy.clone();
        async move { capture_paste(&executor, &policy, args).await.unwrap_err() }
    };

    // The model's own words, even a faithful paraphrase of the paste.
    for text in [
        "My summary: Solsup reported a backlog of 184.6 billion won",
        "솔숲식품은 2025년 3월 12일 현재 수주잔고가 184.6십억 원이라고 알렸다.",
        "솔숲식품은 2025년 3월 12일 현재 수주잔고 수치가 185.6이고",
    ] {
        assert_eq!(
            invalid_field(&refused(json!({"text": text})).await),
            "text",
            "{text}"
        );
    }
    // Text from another Chat, and a hand-off's host-composed request.
    for text in [
        "Marrow Glass original FY2024 gross margin was 40.0 percent.",
        "Check the order backlog of 990 billion won",
    ] {
        assert_eq!(
            invalid_field(&refused(json!({"text": text})).await),
            "text",
            "{text}"
        );
    }
    // Tool output never becomes the user's: a fetched page the turn holds is not a message.
    let fetched = delivered(
        &capture,
        RunResultProducer {
            origin: RunResultOrigin::AcquiredOriginal,
            runtime_id: "native-web".into(),
            tool_name: "web_fetch".into(),
            provider: None,
        },
        json!({"content": "Hanul Mills reported an order backlog of 512.0 billion won.",
            "url": "https://hanul.example/backlog"}),
    )
    .await;
    assert!(capture.run_result(&fetched).await.is_some());
    assert_eq!(
        invalid_field(
            &refused(
                json!({"text": "Hanul Mills reported an order backlog of 512.0 billion won."})
            )
            .await
        ),
        "text"
    );
    // Stated details must be the user's too.
    assert_eq!(
        invalid_field(
            &refused(json!({"text": RETYPED, "url": "https://fictional.example/solsup/other"}))
                .await
        ),
        "url"
    );
    assert_eq!(
        invalid_field(&refused(json!({"text": RETYPED, "publisher": "Solsup Foods"})).await),
        "publisher"
    );
    assert!(capture_paste(
        &executor,
        &policy,
        json!({"text": RETYPED, "publisher": " 솔숲식품 "})
    )
    .await
    .is_ok());
    // A source_save of a result the user never pasted stays an ordinary Source.
    executor
        .stage_source_save(
            &policy,
            json!({"result_ref": fetched, "title": "Hanul", "coverage": "included_full"}),
            "save-web",
        )
        .await
        .unwrap();
    executor.commit_delivery(&policy, "save-web").await.unwrap();
    let staged = capture.staged_sources.lock().await.clone();
    assert_eq!(staged[0].source_class, SourceClass::ToolResult);
    assert_eq!(staged[0].provided, None);

    // Another Chat's binding, or a run nobody typed into, captures nothing.
    let elsewhere = executor_for(
        &state,
        Arc::new(ToolCapture {
            bound_chat_id: Some("chat-b".into()),
            ..ToolCapture::default()
        }),
    );
    assert!(matches!(
        capture_paste(&elsewhere, &policy, json!({"text": RETYPED})).await,
        Err(BrokerError::MethodDenied)
    ));
    let unattended = executor_for(
        &state,
        Arc::new(ToolCapture {
            bound_chat_id: Some("chat-a".into()),
            unattended_run_id: Some("run-1".into()),
            ..ToolCapture::default()
        }),
    );
    assert!(
        capture_paste(&unattended, &policy, json!({"text": RETYPED}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_pasted_credential_is_not_saved_to_memory() {
    let temporary = tempfile::tempdir().unwrap();
    let pasted = "Our vendor notice. api_key=sk-live-0123456789abcdef0123 and revenue was 12.";
    let state = pasting_state(&temporary, vec![user("user-paste", pasted, PASTED_AT_MS)]);
    let capture = chat_a_turn();
    let executor = executor_for(&state, capture.clone());
    let policy = policy(true, true);
    let captured = capture_paste(&executor, &policy, json!({"text": pasted}))
        .await
        .unwrap();
    let error = executor
        .stage_source_save(
            &policy,
            json!({"result_ref": captured["result_ref"], "title": "Vendor", "coverage": "included_full"}),
            "save",
        )
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("tool execution failed: Not saved"),
        "{error}"
    );
    assert!(capture.staged_sources.lock().await.is_empty());
    let refusals = capture.tool_refusals.lock().unwrap().clone();
    assert_eq!(refusals.len(), 1);
    assert_eq!(refusals[0].reason, ChatToolRefusalReason::SensitiveMemory);
}

#[test]
fn a_user_provided_source_needs_its_pasted_message() {
    let request = |class: SourceClass, authorship: Authorship| crate::knowledge::RetainNewSource {
        origin: None,
        loupist_id: "loupist-a".into(),
        at_ms: PASTED_AT_MS,
        title: "Solsup backlog".into(),
        source_class: class,
        origin_locator: "Pasted text".into(),
        provider_identity: "user".into(),
        authorship,
        coverage: Coverage::IncludedFull,
        origin_chat_id: Some("chat-a".into()),
        request_ref: None,
        declared_digest: None,
        publication_time: None,
        material: Some(crate::knowledge::RetainMaterial {
            media_type: "text/plain".into(),
            role: crate::knowledge::RepresentationRole::Original,
            document_id: "paste.a".into(),
            parser_version: None,
            parent_representation_id: None,
            input_span_ids: Vec::new(),
            bytes: BODY.as_bytes().to_vec(),
            selector: crate::knowledge::Selector::ByteRange {
                start: 0,
                end: BODY.len() as u64,
            },
            context_note: "Exact delivered original".into(),
        }),
    };
    let provided = ProvidedOrigin {
        chat_id: "chat-a".into(),
        message_id: "user-paste".into(),
        pasted_at_ms: PASTED_AT_MS,
        stated_url: None,
        stated_publisher: None,
    };
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    seed_loupist(&state, &temporary, "loupist-a");
    let store = crate::knowledge::KnowledgeStore::attach(
        state.store.clone(),
        crate::commands::knowledge::knowledge_blobs(&state).unwrap(),
    );
    let retain = |request: crate::knowledge::RetainNewSource, provided: Option<&ProvidedOrigin>| {
        store.transact(|write| write.retain_new_source_with_origin(request, "turn-1", provided))
    };
    // A user-provided class without the message, or a message on another class.
    assert!(retain(request(SourceClass::UserProvided, Authorship::User), None).is_err());
    assert!(retain(
        request(SourceClass::UserStatement, Authorship::User),
        Some(&provided)
    )
    .is_err());
    assert!(retain(
        request(SourceClass::UserProvided, Authorship::Publisher),
        Some(&provided)
    )
    .is_err());
    let other_chat = ProvidedOrigin {
        chat_id: "chat-b".into(),
        ..provided.clone()
    };
    assert!(retain(
        request(SourceClass::UserProvided, Authorship::User),
        Some(&other_chat)
    )
    .is_err());
    let saved = retain(
        request(SourceClass::UserProvided, Authorship::User),
        Some(&provided),
    )
    .unwrap();
    let revision = store
        .read(|read| read.get_revision("loupist-a", &saved.revision_id))
        .unwrap();
    let payload: crate::knowledge::SourceRecordPayload =
        serde_json::from_value(revision.payload).unwrap();
    assert_eq!(payload.source.source_class, SourceClass::UserProvided);
    assert_eq!(
        payload.source.captures[0].provided.as_ref(),
        Some(&provided)
    );
}
