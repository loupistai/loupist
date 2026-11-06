use super::*;
use crate::domain::ChatMessageStatus;

fn bound_capture(user_message_id: Option<&str>, chat_id: &str) -> Arc<ToolCapture> {
    Arc::new(ToolCapture {
        principle_user_message_id: user_message_id.map(str::to_owned),
        bound_chat_id: Some(chat_id.into()),
        ..ToolCapture::default()
    })
}

async fn saved_source(executor: &AppToolExecutor, name: &str, contents: &str) -> String {
    let result_ref = captured_file(executor, name, contents).await;
    let delivery = new_id("save");
    let saved = executor
        .stage_source_save(
            &policy(true, true),
            json!({"result_ref": result_ref, "title": name, "coverage": "included_full"}),
            &delivery,
        )
        .await
        .unwrap();
    executor.capture.commit_delivery(&delivery).await;
    saved["saveRef"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn source_save_stages_exact_acknowledged_bytes_that_notes_can_cite() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));
    let policy = policy(true, true);
    let result_ref = captured_file(&executor, "report.txt", "Exact delivered report").await;
    let saved = executor
        .stage_source_save(
            &policy,
            json!({"result_ref": result_ref, "title": "Report", "coverage": "included_full"}),
            "save",
        )
        .await
        .unwrap();
    let lookup = json!({"saveRef": saved["saveRef"]});
    let note = |reference: &Value| {
        json!({"workingNote": {"title": "Report", "markdown": "The report says 42.",
            "changeSummary": "Saved", "expectedResearchId": null}, "sourceReads": [reference]})
    };
    assert!(
        executor.world_job(&policy, lookup.clone()).await.is_err(),
        "an unacknowledged save grants no authority"
    );
    assert!(executor
        .stage_research(&policy, note(&saved["saveRef"]), "note-before-ack")
        .await
        .is_err());

    executor.capture.commit_delivery("save").await;
    let status = executor.world_job(&policy, lookup).await.unwrap();
    assert_eq!(status["status"], "waiting_for_turn_commit");
    assert_eq!(
        executor.capture.staged_sources.lock().await[0].bytes,
        b"Exact delivered report"
    );
    assert!(
        executor
            .stage_research(&policy, note(&json!(result_ref)), "producer-receipt")
            .await
            .is_err(),
        "a raw tool receipt is not a saved Source"
    );
    executor
        .stage_research(&policy, note(&saved["saveRef"]), "note")
        .await
        .unwrap();
    assert!(executor.capture.staged_research.lock().await.is_none());
    executor.capture.discard_delivery("note").await;
    assert!(executor.capture.staged_research.lock().await.is_none());
}

#[tokio::test]
async fn source_save_refuses_derived_output_saved_context_and_unseen_pages() {
    let page = format!("{}Closing figure.", "Operating details. ".repeat(4000));
    let _network = crate::web::script::install(vec![(
        "https://example.com/long-report",
        vec![crate::web::script::ok("text/plain", page.into_bytes())],
    )]);
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let mut past = chat("past-chat", "loupist-a", 1);
    past.messages = vec![
        chat_message("question", ChatRole::User, "A hypothetical scenario?"),
        chat_message("answer", ChatRole::Assistant, "A hypothetical scenario."),
    ];
    state.store.create_chat(&past).unwrap();
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));
    let policy = policy(true, true);
    fs::write(temporary.path().join("loupist/rows.json"), "[1,2]").unwrap();
    let save = |result: Value, coverage: &str| json!({"result_ref": result["result_ref"], "title": "Saved", "coverage": coverage});

    let derived = executor
        .execute(
            &policy,
            ToolMethod::DatasetImport,
            json!({"path": "rows.json"}),
        )
        .await
        .unwrap();
    let recalled = executor
        .execute(
            &policy,
            ToolMethod::ConversationMemory,
            json!({"action": "read", "chatId": "past-chat", "messageId": "answer"}),
        )
        .await
        .unwrap();
    for refused in [derived, recalled] {
        assert!(refused["result_ref"].is_string());
        assert!(executor
            .stage_source_save(&policy, save(refused, "included_full"), "refused")
            .await
            .is_err());
    }

    let partial = executor
        .execute(
            &policy,
            ToolMethod::WebFetch,
            json!({"url": "https://example.com/long-report"}),
        )
        .await
        .unwrap();
    assert!(!partial["next_offset"].is_null());
    assert!(executor
        .stage_source_save(&policy, save(partial.clone(), "included_full"), "full")
        .await
        .is_err());
    assert!(executor.capture.staged_sources.lock().await.is_empty());
    executor
        .stage_source_save(
            &policy,
            save(partial.clone(), "included_excerpt"),
            "excerpt",
        )
        .await
        .unwrap();
    executor.capture.commit_delivery("excerpt").await;
    let retained = executor.capture.staged_sources.lock().await[0]
        .bytes
        .clone();
    let delivered = executor
        .capture
        .run_result(partial["result_ref"].as_str().unwrap())
        .await
        .unwrap()
        .payload;
    assert_eq!(retained, serde_json::to_vec(&delivered).unwrap());
}

#[tokio::test]
async fn conversation_memory_reads_only_completed_messages_of_the_current_loupist() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    seed_loupist(&state, &temporary, "loupist-b");
    let message = |id: &str, role: ChatRole, status: ChatMessageStatus| ChatMessage {
        status,
        created_at_ms: 2,
        ..chat_message(id, role, &format!("research {id}"))
    };
    for (chat_id, loupist_id, messages) in [
        (
            "past-chat",
            "loupist-a",
            vec![
                message("complete-user", ChatRole::User, ChatMessageStatus::Complete),
                message(
                    "complete-answer",
                    ChatRole::Assistant,
                    ChatMessageStatus::Complete,
                ),
                message("failed-user", ChatRole::User, ChatMessageStatus::Complete),
                message(
                    "failed-answer",
                    ChatRole::Assistant,
                    ChatMessageStatus::Error,
                ),
                message("pending-user", ChatRole::User, ChatMessageStatus::Complete),
            ],
        ),
        (
            "other-chat",
            "loupist-b",
            vec![
                message("other-user", ChatRole::User, ChatMessageStatus::Complete),
                message(
                    "other-answer",
                    ChatRole::Assistant,
                    ChatMessageStatus::Complete,
                ),
            ],
        ),
    ] {
        let mut saved = chat(chat_id, loupist_id, 1);
        saved.messages = messages;
        saved.updated_at_ms = 2;
        state.store.create_chat(&saved).unwrap();
    }
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));
    let policy = policy(true, true);
    let found = executor
        .conversation_memory(
            &policy,
            json!({"action":"search","query":"research"}),
            "search",
        )
        .await
        .unwrap();
    let ids: Vec<_> = found["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["messageId"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["complete-answer", "complete-user"]);
    let selected = executor
        .conversation_memory(
            &policy,
            json!({"action":"read","chatId":"past-chat","messageId":"complete-user","role":"user"}),
            "read",
        )
        .await
        .unwrap();
    assert_eq!(selected["text"], "research complete-user");
    assert_eq!(selected["notEvidence"], true);
    for (chat_id, message_id, role) in [
        ("past-chat", "complete-answer", Some("user")),
        ("past-chat", "failed-user", None),
        ("past-chat", "failed-answer", None),
        ("past-chat", "pending-user", None),
        ("other-chat", "other-answer", None),
    ] {
        let mut args = json!({"action":"read","chatId":chat_id,"messageId":message_id});
        if let Some(role) = role {
            args["role"] = json!(role);
        }
        assert!(
            executor
                .conversation_memory(&policy, args, "refused")
                .await
                .is_err(),
            "{message_id}"
        );
    }
    assert!(matches!(
        executor
            .conversation_memory(
                &ToolPolicy {
                    use_memory: false,
                    ..policy
                },
                json!({"action":"search","query":"research"}),
                "disabled",
            )
            .await,
        Err(BrokerError::MemoryDisabled)
    ));
}

#[tokio::test]
async fn conversation_memory_finds_and_pages_a_long_unicode_message_without_loss() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let original = format!(
        "{}삼성전자는 HBM capacity expansion을 가정한다. 끝.",
        "이전 문맥🙂 ".repeat(2500)
    );
    let mut saved = chat("ranked-chat", "loupist-a", 1);
    saved.messages = vec![
        chat_message("premise", ChatRole::User, &original),
        chat_message("answer", ChatRole::Assistant, "HBM is uncertain."),
    ];
    state.store.create_chat(&saved).unwrap();
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));
    let policy = policy(true, false);
    let search = |query: &str| {
        executor.conversation_memory(
            &policy,
            json!({"action":"search","query":query,"role":"user"}),
            "search",
        )
    };
    let hit = search("삼성전자 HBM").await.unwrap()["messages"][0].clone();
    assert_eq!(hit["messageId"], "premise");
    assert!(hit["snippet"].as_str().unwrap().contains("삼성전자는 HBM"));
    // A lexical miss offers the recent-messages fallback instead of a dead end.
    let miss = search("original Korean premise").await.unwrap();
    assert!(miss["messages"].as_array().unwrap().is_empty());
    let recent = executor
        .conversation_memory(&policy, miss["fallbackArgs"].clone(), "recent")
        .await
        .unwrap();
    assert_eq!(recent["messages"][0]["messageId"], "premise");

    let mut offset = 0;
    let mut recovered = String::new();
    loop {
        let read = executor
            .conversation_memory(
                &policy,
                json!({"action":"read","chatId":"ranked-chat","messageId":"premise","offset":offset,"limit":7000}),
                "read",
            )
            .await
            .unwrap();
        recovered.push_str(read["text"].as_str().unwrap());
        match read["nextOffset"].as_u64() {
            Some(next) => offset = next,
            None => break,
        }
    }
    assert_eq!(recovered, original);
}

#[tokio::test]
async fn note_and_journal_saves_refuse_unbound_unacknowledged_or_malformed_input() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = executor_for(&state, bound_capture(Some("user-message"), "chat-a"));
    let policy = policy(true, true);
    let save_ref = saved_source(&executor, "source.txt", "Margins improved.").await;
    let journal = json!({"judgment":"Margins will improve", "sourceReads":[save_ref]});
    for unbound in [
        bound_capture(None, "chat-a"),
        bound_capture(Some("user-message"), "other-chat"),
    ] {
        let executor = executor_for(&state, unbound);
        let own_source = saved_source(&executor, "own.txt", "Margins improved.").await;
        assert!(executor
            .stage_journal(
                &policy,
                json!({"judgment":"Margins will improve", "sourceReads":[own_source]}),
                "unbound",
                false
            )
            .await
            .is_err());
    }
    for (field, value) in [
        ("sourceReads", json!(["unacknowledged"])),
        ("worldReads", json!(["unacknowledged"])),
        ("lensReads", json!(["unacknowledged"])),
        ("sourceReads", json!([])),
        ("judgment", json!("")),
        ("check", json!({"claim":"missing metric"})),
        ("entities", json!(["duplicate", "duplicate"])),
    ] {
        let mut request = journal.clone();
        request[field] = value;
        assert!(
            executor
                .stage_journal(&policy, request, "invalid-journal", false)
                .await
                .is_err(),
            "{field}"
        );
        executor.capture.commit_delivery("invalid-journal").await;
    }

    let note = json!({"title":"Review", "markdown":"Evidence", "changeSummary":"New"});
    let with_note = |field: &str, value: Value| {
        let mut request = json!({"workingNote": note});
        request[field] = value;
        request
    };
    for request in [
        json!({}),
        json!({"workingNote": "not an object"}),
        json!({"workingNote": {"title": 12, "markdown": "Evidence", "changeSummary": "New"}}),
        json!({"workingNote": {"title": "Review", "markdown": "Invalid\u{0001}text", "changeSummary": "New"}}),
        with_note("sourceReads", json!("not an array")),
        with_note("lensReads", json!(["duplicate", " duplicate "])),
        with_note(
            "worldReads",
            json!((0..=8).map(|i| format!("read-{i}")).collect::<Vec<_>>()),
        ),
    ] {
        assert!(
            executor
                .stage_research(&policy, request.clone(), "invalid-note")
                .await
                .is_err(),
            "{request}"
        );
        executor.capture.commit_delivery("invalid-note").await;
    }
    assert!(executor.capture.staged_research.lock().await.is_none());

    // The same capture accepts the valid forms, so each refusal above is field-specific.
    executor
        .stage_journal(&policy, journal, "journal", false)
        .await
        .unwrap();
    executor.capture.discard_delivery("journal").await;
    executor
        .stage_research(&policy, with_note("sourceReads", json!([save_ref])), "note")
        .await
        .unwrap();
}

#[tokio::test]
async fn misrouted_note_references_name_the_right_field_without_echoing_model_text() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let capture = bound_capture(Some("user-message"), "chat-a");
    let executor = executor_for(&state, capture.clone());
    let policy = policy(true, true);
    let result_ref = captured_file(&executor, "source.txt", "value 42").await;
    let lens = crate::commands::knowledge::ensure_perspective(&state, "loupist-a", "Base")
        .unwrap()
        .unwrap();
    let lens_read = executor
        .lens_read(&policy, json!({"recordId": lens}), "lens")
        .await
        .unwrap();
    executor.capture.commit_delivery("lens").await;
    let lens_ref = lens_read["readRef"].as_str().unwrap().to_owned();
    let socket = tool_broker_endpoint(temporary.path().join("routes.sock"));
    let broker = start_tool_broker(socket.clone(), policy, Arc::new(executor))
        .await
        .unwrap();
    for (method, base) in [
        (
            "note.save",
            json!({"workingNote": {"title":"Review", "markdown":"Evidence", "changeSummary":"New", "expectedResearchId":null}}),
        ),
        (
            "journal.save",
            json!({"judgment":"Wait", "reasoning":"Evidence"}),
        ),
    ] {
        for (reference, guidance) in [
            (lens_ref.as_str(), "lensReads"),
            (result_ref.as_str(), "source_save"),
            ("rdg_untrusted_model_text", "acknowledged"),
        ] {
            let mut request = base.clone();
            request["sourceReads"] = json!([reference]);
            let rejected = broker_request(&socket, broker.token(), method, request).await;
            let message = rejected["error"]["message"].as_str().unwrap();
            assert!(message.contains(guidance), "{method}: {message}");
            assert!(!message.contains("rdg_untrusted_model_text"), "{message}");
            assert!(capture.staged_research.lock().await.is_none());
        }
        let mut corrected = base;
        corrected["lensReads"] = json!([lens_ref]);
        let accepted = broker_request(&socket, broker.token(), method, corrected).await;
        assert!(accepted.get("error").is_none(), "{accepted}");
        assert!(capture.staged_research.lock().await.take().is_some());
    }
    broker.shutdown().await.unwrap();
}

#[tokio::test]
async fn principle_save_publishes_only_after_acknowledgment_and_turn_commit() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = executor_for(&state, bound_capture(Some("user-rule"), "chat-a"));
    let request =
        json!({"statement":"Check cash flow before profit","why":"Cash tests conversion"});
    assert!(executor
        .stage_principle_save(&policy(false, true), request.clone(), "memory-off")
        .await
        .is_err());
    executor
        .stage_principle_save(&policy(true, true), request.clone(), "cancelled")
        .await
        .unwrap();
    executor.capture.discard_delivery("cancelled").await;
    assert!(executor.capture.staged_principles.lock().await.is_empty());
    let accepted = executor
        .stage_principle_save(&policy(true, true), request, "accepted")
        .await
        .unwrap();
    executor.capture.commit_delivery("accepted").await;
    let staged = executor.capture.staged_principles.lock().await.clone();
    {
        let mut connection = state.store.lock_loupist("loupist-a").unwrap();
        let tx = connection.transaction().unwrap();
        crate::commands::knowledge::principles::persist_principles(&tx, "loupist-a", &staged, 6)
            .unwrap();
        tx.commit().unwrap();
    }
    let context = crate::commands::knowledge::host_memory_context(&state, "loupist-a", "").unwrap();
    let principle = &context.principles[0];
    assert_eq!(principle.record_id, accepted["recordId"].as_str().unwrap());
    assert_eq!(principle.origin_message_id.as_deref(), Some("user-rule"));

    // Replacing a principle requires that this turn was given its current revision.
    let replace = json!({"statement":"Check operating cash flow before profit",
        "replaces_record_id": principle.record_id});
    assert!(executor
        .stage_principle_save(&policy(true, true), replace.clone(), "unread")
        .await
        .is_err());
    executor
        .capture
        .principle_context
        .lock()
        .await
        .insert(principle.record_id.clone(), principle.revision_id.clone());
    executor
        .stage_principle_save(&policy(true, true), replace, "admitted")
        .await
        .unwrap();
}

#[tokio::test]
async fn a_saved_source_records_at_save_whether_it_may_ever_be_shared() {
    use crate::knowledge::Authorship;
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));
    let producer = |runtime: &str, tool: &str, provider: &str| RunResultProducer {
        origin: RunResultOrigin::AcquiredOriginal,
        runtime_id: runtime.into(),
        tool_name: tool.into(),
        provider: Some(provider.into()),
    };
    for (producer, expected) in [
        // Every Korea Investment read is the user's own account data.
        (
            producer("native-finance", "kis_read", "koreainvestment.market-data"),
            Authorship::User,
        ),
        (
            producer("native-web", "web_fetch", "www.sec.gov"),
            Authorship::Publisher,
        ),
        (
            producer("native-finance", "dart_filing_read", "opendart.disclosures"),
            Authorship::Publisher,
        ),
        (
            producer("native-finance", "krx_daily_quote", "krx.market-data"),
            Authorship::Publisher,
        ),
        (
            producer("openbb", "equity_price_quote", "sec"),
            Authorship::Publisher,
        ),
        // A producer the save path does not know stays private.
        (
            producer("future-runtime", "future_tool", "future-feed.data"),
            Authorship::Tool,
        ),
    ] {
        let label = format!("{}:{}", producer.runtime_id, producer.tool_name);
        let result_ref = delivered(
            &executor.capture,
            producer,
            json!({"source_id": "fixture", "data": [1, 2]}),
        )
        .await;
        let delivery = new_id("save");
        executor
            .stage_source_save(
                &policy(true, true),
                json!({"result_ref": result_ref, "title": label, "coverage": "included_full"}),
                &delivery,
            )
            .await
            .unwrap();
        executor.capture.commit_delivery(&delivery).await;
        let staged = executor.capture.staged_sources.lock().await;
        let saved = staged.iter().find(|source| source.title == label).unwrap();
        assert_eq!(saved.authorship, expected, "{label}");
    }
}
