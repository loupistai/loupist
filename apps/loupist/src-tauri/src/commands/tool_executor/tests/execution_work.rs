use super::*;

#[tokio::test]
async fn cached_result_sizes_enforce_the_shared_pending_and_delivered_byte_budget() {
    let capture = ToolCapture::default();
    let payload = json!({"data":"x".repeat(16 * 1024 * 1024)});
    capture
        .stage_run_result_with_delivery(
            "first",
            provider_result(),
            &json!({}),
            payload.clone(),
            vec![],
            None,
        )
        .await
        .unwrap();
    assert!(capture
        .stage_run_result_with_delivery(
            "second",
            provider_result(),
            &json!({}),
            payload.clone(),
            vec![],
            None
        )
        .await
        .is_err());
    capture.discard_delivery("first").await;
    capture
        .stage_run_result_with_delivery(
            "second",
            provider_result(),
            &json!({}),
            payload.clone(),
            vec![],
            None,
        )
        .await
        .unwrap();
    capture.commit_delivery("second").await;
    assert!(capture
        .stage_run_result_with_delivery(
            "third",
            provider_result(),
            &json!({}),
            payload,
            vec![],
            None
        )
        .await
        .is_err());
}

#[tokio::test]
async fn scope_reads_the_loupists_folder_database() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(temporary.path().join("app"));
    seed_loupist(&state, &temporary, "loupist-a");
    state
        .store
        .create_chat(&chat("chat-a", "loupist-a", 1))
        .unwrap();
    let executor = AppToolExecutor {
        state,
        capture: Arc::new(ToolCapture::default()),
        loupist_id: "loupist-a".into(),
        capability_ids: BTreeSet::new(),
    };
    executor.ensure_scope(&policy(false, false)).unwrap();
}

#[tokio::test]
async fn scope_reads_the_live_binding_without_decoding_the_transcript() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    seed_loupist(&state, &temporary, "loupist-b");
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));
    let policy = policy(false, false);
    state
        .store
        .lock()
        .unwrap()
        .execute(
            "UPDATE chat_sessions SET data_json = 'invalid transcript' WHERE id = 'chat-a'",
            [],
        )
        .unwrap();
    executor.ensure_scope(&policy).unwrap();
    state
        .store
        .lock()
        .unwrap()
        .execute(
            "UPDATE chat_sessions SET loupist_id = 'loupist-b' WHERE id = 'chat-a'",
            [],
        )
        .unwrap();
    assert!(executor.ensure_scope(&policy).is_err());
    state
        .store
        .lock()
        .unwrap()
        .execute("DELETE FROM chat_sessions WHERE id = 'chat-a'", [])
        .unwrap();
    assert!(executor.ensure_scope(&policy).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn blocking_tools_run_outside_the_async_worker_and_return_their_result() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));
    let async_thread = std::thread::current().id();
    let result = executor
        .blocking_tool(move |_| {
            assert_ne!(std::thread::current().id(), async_thread);
            Ok(json!({"value": "unchanged"}))
        })
        .await
        .unwrap();
    assert_eq!(result, json!({"value": "unchanged"}));
}

#[tokio::test]
async fn captured_result_size_is_preserved_when_reserved_delivered_and_discarded() {
    let capture = ToolCapture::default();
    let payload = json!({"text": "한글\n\"".repeat(100)});
    let reference = capture
        .stage_run_result_with_delivery(
            "first",
            provider_result(),
            &json!({}),
            payload.clone(),
            vec![],
            Some(&payload),
        )
        .await
        .unwrap();
    let bytes = {
        let pending = capture.pending_deliveries.lock().await;
        let saved = &pending["first"].run_results[&reference];
        assert_eq!(
            saved.bytes,
            run_result_bytes(&saved.result, serde_json::to_vec(&payload).unwrap().len())
        );
        saved.bytes
    };
    capture.commit_delivery("first").await;
    assert_eq!(capture.run_results.lock().await.bytes, bytes);
    assert_eq!(
        capture.run_result(&reference).await.unwrap().payload,
        payload
    );
    capture
        .stage_run_result_with_delivery(
            "second",
            provider_result(),
            &json!({}),
            json!({"more": true}),
            vec![],
            None,
        )
        .await
        .unwrap();
    capture.discard_delivery("second").await;
    assert_eq!(capture.run_results.lock().await.bytes, bytes);
    assert!(capture.pending_deliveries.lock().await.is_empty());
}
