use super::*;

fn markdown(title: &str) -> Value {
    json!({"mode": "create", "title": title, "payload": {"kind": "markdown",
        "schema": "loupist-markdown/1", "markdown": format!("# {title}")}})
}

#[tokio::test]
async fn the_work_list_reads_a_loupists_artifacts_across_its_chats_with_their_titles() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let policy = policy(false, false);
    let capture = Arc::new(ToolCapture::for_chat("assistant-1".into()));
    let executor = executor_for(&state, capture.clone());
    executor
        .execute(
            &policy,
            ToolMethod::ArtifactPublish,
            markdown("Supplier map"),
        )
        .await
        .unwrap();
    let published = capture.artifacts.lock().await[0].clone();
    save_artifact_turn(&state, &published);

    let chat_title = state.store.get_chat("chat-a").unwrap().unwrap().title;
    let work = state.store.list_loupist_artifacts("loupist-a", 50).unwrap();
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].0.id, published.artifact.id);
    assert_eq!(work[0].0.chat_session_id, "chat-a");
    assert_eq!(work[0].1, chat_title);
    // The limit bounds the read.
    assert!(state
        .store
        .list_loupist_artifacts("loupist-a", 0)
        .unwrap()
        .is_empty());
}

fn line_chart_from(source: Value) -> Value {
    json!({"mode": "create", "title": "Close price", "dataset": source,
        "view": {"kind": "line", "x": "date", "y": "close"}})
}

fn price_columns() -> Value {
    json!([
        {"id": "date", "label": "Date", "kind": "date", "pointer": "/date"},
        {"id": "close", "label": "Close", "kind": "number", "pointer": "/close"}
    ])
}

#[tokio::test]
async fn an_unattended_refusal_is_recorded_on_its_scheduled_run_and_a_delegated_run_has_no_row() {
    use crate::chat::surface::ChatSurface;
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    for (surface, expected) in [
        (
            ChatSurface::Delegated {
                handoff_id: "handoff-1".into(),
            },
            crate::briefings::policy::REFUSAL,
        ),
        (
            ChatSurface::Scheduled,
            "Could not record refused unattended tool",
        ),
    ] {
        let executor = executor_for(
            &state,
            Arc::new(ToolCapture {
                unattended_run_id: Some("run-without-row".into()),
                surface: Some(surface.clone()),
                ..ToolCapture::default()
            }),
        );
        let refused = executor
            .execute(
                &policy(true, true),
                ToolMethod::NativeAuthorize,
                json!({"name": "read"}),
            )
            .await;
        assert!(
            matches!(&refused, Err(BrokerError::Execution(message)) if message == expected),
            "{surface:?}: {refused:?}"
        );
    }
}

#[tokio::test]
async fn foreign_scope_and_memory_off_are_refused_before_any_tool_runs() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    seed_loupist(&state, &temporary, "loupist-b");
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));
    let foreign = ToolPolicy {
        loupist_id: "loupist-b".into(),
        ..policy(true, false)
    };
    assert!(matches!(
        executor.ensure_scope(&foreign),
        Err(BrokerError::Execution(_))
    ));

    let socket = tool_broker_endpoint(temporary.path().join("broker.sock"));
    let calls = Arc::new(AtomicUsize::new(0));
    let broker = start_tool_broker(
        socket.clone(),
        policy(false, false),
        Arc::new(CountingExecutor {
            calls: calls.clone(),
        }),
    )
    .await
    .unwrap();
    let response = broker_request(
        &socket,
        broker.token(),
        "world.search",
        json!({"query": "quality"}),
    )
    .await;
    assert_eq!(response["error"]["code"], "memory_disabled");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    broker.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn tool_executor_keeps_the_pinned_loupist_after_its_path_is_replaced() {
    let temporary = tempfile::tempdir().unwrap();
    let root_a = temporary.path().join("loupist-a");
    let root_b = temporary.path().join("loupist-b");
    let moved_a = temporary.path().join("loupist-a-original");
    initialized_workspace(&root_a, "A");
    initialized_workspace(&root_b, "B");
    let state = AppState::for_test(temporary.path().join("app"));
    seed_profile(state.store.as_ref(), &profile("loupist-a", &root_a, 1));
    state
        .store
        .create_chat(&chat("chat-a", "loupist-a", 1))
        .unwrap();
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));

    fs::rename(&root_a, &moved_a).unwrap();
    fs::rename(&root_b, &root_a).unwrap();

    let result = executor
        .execute(
            &policy(true, false),
            ToolMethod::WorldRead,
            json!({"scope":"lens","recordId":"rec_00000000000000000000000000000001"}),
        )
        .await;
    assert!(result.is_err(), "{result:?}");
    assert_eq!(
        fs::read_to_string(root_a.join("runtime-marker")).unwrap(),
        "B\n"
    );
    assert_eq!(
        fs::read_to_string(moved_a.join("runtime-marker")).unwrap(),
        "A\n"
    );
}

#[tokio::test]
async fn artifact_revisions_need_an_exact_read_stay_in_their_chat_and_are_bounded_per_turn() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    state
        .store
        .create_chat(&chat("chat-b", "loupist-a", 1))
        .unwrap();
    let policy = policy(false, false);
    let capture = Arc::new(ToolCapture::for_chat("assistant-1".into()));
    let executor = executor_for(&state, capture.clone());
    for index in 1..=crate::chat::artifacts::MAX_CHAT_TURN_ARTIFACTS {
        executor
            .execute(
                &policy,
                ToolMethod::ArtifactPublish,
                markdown(&format!("Note {index}")),
            )
            .await
            .unwrap();
    }
    assert!(matches!(
        executor
            .execute(&policy, ToolMethod::ArtifactPublish, markdown("Note extra"))
            .await,
        Err(BrokerError::Execution(_))
    ));
    let first = capture.artifacts.lock().await[0].clone();
    save_artifact_turn(&state, &first);

    // A selected artifact is admitted only from its own Chat and a retained revision.
    let hint = |chat_id: &str, revision: u32| {
        crate::commands::artifact_context::selected_artifact_hint(
            state.store.as_ref(),
            chat_id,
            Some(&crate::commands::ChatArtifactSelection {
                artifact_id: first.artifact.id.clone(),
                revision,
            }),
        )
    };
    assert!(!hint("chat-a", 1)
        .unwrap()
        .unwrap()
        .to_string()
        .contains("# Note 1"));
    assert!(hint("chat-b", 1).is_err());
    assert!(hint("chat-a", 2).is_err());

    let next = Arc::new(ToolCapture::for_chat("assistant-2".into()));
    let next_executor = executor_for(&state, next.clone());
    let revise = json!({"mode": "revise", "artifact_id": first.artifact.id, "expected_revision": 1,
        "title": "Note 1", "payload": {"kind": "markdown", "schema": "loupist-markdown/1",
        "markdown": "# Revised"}});
    let read = json!({"artifact_id": first.artifact.id});
    assert!(matches!(
        next_executor
            .execute(&policy, ToolMethod::ArtifactPublish, revise.clone())
            .await,
        Err(BrokerError::Execution(_))
    ));
    next_executor
        .execute(&policy, ToolMethod::ArtifactRead, read.clone())
        .await
        .unwrap();
    next_executor
        .execute(&policy, ToolMethod::ArtifactPublish, revise.clone())
        .await
        .unwrap();
    assert!(next_executor
        .execute(&policy, ToolMethod::ArtifactPublish, revise)
        .await
        .is_err());
    assert_eq!(next.artifacts.lock().await.len(), 1);
    let other_chat = ToolPolicy {
        session_id: "chat-b".into(),
        ..policy
    };
    assert!(matches!(
        next_executor
            .execute(&other_chat, ToolMethod::ArtifactRead, read)
            .await,
        Err(BrokerError::MethodDenied)
    ));
}

#[tokio::test]
async fn chart_from_a_delivered_result_keeps_its_lineage_across_revisions() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let policy = policy(false, false);
    let capture = Arc::new(ToolCapture::for_chat("assistant-1".into()));
    let result_ref = delivered(
        &capture,
        provider_result(),
        json!({"data": {"rows": [
            {"date": "2026-08-01", "close": 100},
            {"date": "2026-08-02", "close": 101},
            {"date": "2026-08-03", "close": 102}
        ]}, "warnings": ["Delayed market data"]}),
    )
    .await;
    let from_result = json!({"from_result": {"result_ref": result_ref,
        "rows_pointer": "/data/rows", "columns": price_columns()}});
    let executor = executor_for(&state, capture.clone());
    executor
        .execute(
            &policy,
            ToolMethod::ChartPublish,
            line_chart_from(from_result.clone()),
        )
        .await
        .unwrap();
    let commit = capture.artifacts.lock().await.last().cloned().unwrap();
    assert_eq!(commit.datasets[0].rows.len(), 3);
    let crate::chat::chart::ChartDatasetLineage::FromResult { receipt, .. } =
        &commit.datasets[0].lineage
    else {
        panic!("result-selected chart did not persist result lineage");
    };
    assert_eq!(receipt.result_ref, result_ref);
    assert_eq!(receipt.provider.as_deref(), Some("openbb-keyless"));
    assert_eq!(receipt.warnings, ["Delayed market data"]);
    save_artifact_turn(&state, &commit);

    let next_executor = executor_for(
        &state,
        Arc::new(ToolCapture::for_chat("assistant-2".into())),
    );
    let compact = next_executor
        .execute(
            &policy,
            ToolMethod::ArtifactRead,
            json!({"artifact_id": commit.artifact.id}),
        )
        .await
        .unwrap();
    assert_eq!(compact["dataset"]["lineage"]["kind"], "from_result");
    let page = next_executor
        .execute(
            &policy,
            ToolMethod::ChartQuery,
            json!({"artifact_id": commit.artifact.id, "revision": 1, "offset": 1, "limit": 1, "columns": ["close"]}),
        )
        .await
        .unwrap();
    assert_eq!(page["rows"], json!([[101]]));
    assert_eq!(page["next_offset"], 2);
    next_executor
        .execute(
            &policy,
            ToolMethod::ChartPublish,
            json!({"mode": "revise", "artifact_id": commit.artifact.id,
                "edit_token": compact["artifact"]["edit_token"],
                "title": "Close price with note", "note": "Same immutable dataset."}),
        )
        .await
        .unwrap();
    let revised = next_executor.capture.artifacts.lock().await[0].clone();
    assert_eq!(revised.datasets[0].digest, commit.datasets[0].digest);
    // A result delivered in an earlier turn cannot seed a new chart.
    assert!(next_executor
        .execute(
            &policy,
            ToolMethod::ChartPublish,
            line_chart_from(from_result)
        )
        .await
        .is_err());
}

#[tokio::test]
async fn chart_from_a_saved_dataset_is_pinned_across_restart_and_view_revision() {
    let temporary = tempfile::tempdir().unwrap();
    let app_data_dir = temporary.path().join("app");
    let state = AppState::for_persistent_test(app_data_dir.clone());
    seed_loupist(&state, &temporary, "loupist-a");
    seed_loupist(&state, &temporary, "loupist-b");
    state
        .store
        .create_chat(&chat("chat-a", "loupist-a", 1))
        .unwrap();
    let policy = policy(false, false);
    let payload = json!({"data": {"rows": [
        {"date": "2026-08-01", "close": 100, "volume": 500},
        {"date": "2026-08-02", "close": 101, "volume": 600}
    ]}});
    let put = |loupist: &str, key: &str| {
        state
            .store
            .dataset_put(loupist, key, &payload, json!({}))
            .unwrap()
    };
    let reference = put("loupist-a", "close-prices");
    let foreign = put("loupist-b", "foreign-prices");
    let corrupt = put("loupist-a", "corrupt-prices");
    state
        .store
        .lock()
        .unwrap()
        .execute(
            "UPDATE datasets SET digest=?1 WHERE loupist_id='loupist-a' AND id=?2",
            rusqlite::params!["0".repeat(64), corrupt.dataset_id],
        )
        .unwrap();
    let chart = |source: &crate::datasets::DatasetRef| {
        line_chart_from(json!({"from_dataset": {"dataset_id": source.dataset_id,
            "revision": source.revision, "rows_pointer": "/data/rows", "columns": price_columns()}}))
    };
    let capture = Arc::new(ToolCapture::for_chat("assistant-1".into()));
    let executor = executor_for(&state, capture.clone());
    for refused in [&corrupt, &foreign] {
        assert!(executor
            .execute(&policy, ToolMethod::ChartPublish, chart(refused))
            .await
            .is_err());
    }
    executor
        .execute(&policy, ToolMethod::ChartPublish, chart(&reference))
        .await
        .unwrap();
    let commit = capture.artifacts.lock().await[0].clone();
    assert!(matches!(
        &commit.datasets[0].lineage,
        crate::chat::chart::ChartDatasetLineage::FromDataset { dataset_id, revision, digest, .. }
            if dataset_id == &reference.dataset_id && *revision == reference.revision && digest == &reference.digest
    ));
    save_artifact_turn(&state, &commit);
    let newer = state
        .store
        .dataset_put(
            "loupist-a",
            "close-prices",
            &json!({"data": {"rows": [{"date": "2026-08-01", "close": 999}]}}),
            json!({}),
        )
        .unwrap();
    assert_eq!(newer.revision, reference.revision + 1);
    drop(executor);
    state.close_for_restart_test();

    let state = AppState::for_persistent_test(app_data_dir);
    let capture = Arc::new(ToolCapture::for_chat("assistant-2".into()));
    let executor = executor_for(&state, capture.clone());
    let reopened = executor
        .execute(
            &policy,
            ToolMethod::ArtifactRead,
            json!({"artifact_id": commit.artifact.id}),
        )
        .await
        .unwrap();
    executor
        .execute(
            &policy,
            ToolMethod::ChartPublish,
            json!({"mode": "revise", "artifact_id": commit.artifact.id,
                "edit_token": reopened["artifact"]["edit_token"],
                "title": "Saved close prices as bars", "view": {"kind": "bar", "x": "date", "y": "close"}}),
        )
        .await
        .unwrap();
    let revised = capture.artifacts.lock().await[0].clone();
    assert_eq!(revised.revision.revision, 2);
    assert_eq!(revised.datasets[0], commit.datasets[0]);
    save_artifact_turn(&state, &revised);
    let reader = executor_for(
        &state,
        Arc::new(ToolCapture::for_chat("assistant-3".into())),
    );
    reader
        .execute(
            &policy,
            ToolMethod::ArtifactRead,
            json!({"artifact_id": commit.artifact.id}),
        )
        .await
        .unwrap();
    let rows = reader
        .execute(
            &policy,
            ToolMethod::ChartQuery,
            json!({"artifact_id": commit.artifact.id, "revision": 2}),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(
            rows["rows"]
                .as_str()
                .and_then(crate::custom_connections::taint::delimited_body)
                .unwrap()
        )
        .unwrap(),
        json!([["2026-08-01", 100], ["2026-08-02", 101]])
    );
    let historical = reader
        .execute(
            &policy,
            ToolMethod::ArtifactRead,
            json!({"artifact_id": commit.artifact.id, "revision_id": 1}),
        )
        .await
        .unwrap();
    assert_eq!(historical["artifact"]["revision"], 1);
    assert_eq!(
        historical["artifact"]["revision_digest"],
        commit.revision.digest
    );
    let old_rows = reader
        .execute(
            &policy,
            ToolMethod::ChartQuery,
            json!({"artifact_id": commit.artifact.id, "revision": 1}),
        )
        .await
        .unwrap();
    // Rows from an unclassified dataset come back delimited with a fresh id on every read.
    let without_ids = |value: &serde_json::Value| {
        value
            .to_string()
            .split("id=")
            .enumerate()
            .map(|(index, part)| {
                if index == 0 {
                    part.to_owned()
                } else {
                    part.trim_start_matches(|c: char| c.is_ascii_hexdigit())
                        .to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("id=")
    };
    assert_eq!(without_ids(&old_rows["rows"]), without_ids(&rows["rows"]));
    assert!(crate::commands::artifact_context::selected_artifact_hint(
        state.store.as_ref(),
        "chat-a",
        Some(&crate::commands::ChatArtifactSelection {
            artifact_id: commit.artifact.id.clone(),
            revision: 1,
        }),
    )
    .unwrap()
    .is_some());
    assert!(reader
        .execute(
            &policy,
            ToolMethod::ChartPublish,
            json!({"mode": "revise", "artifact_id": commit.artifact.id,
            "edit_token": historical["artifact"]["edit_token"], "title": "Stale edit"}),
        )
        .await
        .is_err());
}

#[tokio::test]
async fn inline_chart_data_needs_delivered_upstream_and_rejected_input_is_never_echoed() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let policy = policy(false, false);
    let capture = Arc::new(ToolCapture::for_chat("assistant-1".into()));
    let upstream = delivered(&capture, provider_result(), json!({"value": 100})).await;
    let executor = executor_for(&state, capture.clone());
    let inline = |upstream: &str, cell: Value| {
        json!({"mode": "create", "title": "Derived returns", "dataset": {"inline": {
            "columns": [{"id": "date", "label": "Date", "kind": "date"}, {"id": "return", "label": "Return", "kind": "number"}],
            "rows": [["2026-08-01", cell]], "upstream_result_refs": [upstream]}},
            "view": {"kind": "line", "x": "date", "y": ["return"]}})
    };
    executor
        .execute(
            &policy,
            ToolMethod::ChartPublish,
            inline(&upstream, json!(0.1)),
        )
        .await
        .unwrap();
    let commit = capture.artifacts.lock().await[0].clone();
    assert!(matches!(
        &commit.datasets[0].lineage,
        crate::chat::chart::ChartDatasetLineage::AgentAuthored { upstream_receipts }
            if upstream_receipts.len() == 1 && upstream_receipts[0].result_ref == upstream
    ));
    for refused in [
        inline("result:not-from-this-turn", json!(0.1)),
        inline(&upstream, json!("not-a-number")),
    ] {
        assert!(executor
            .execute(&policy, ToolMethod::ChartPublish, refused)
            .await
            .is_err());
    }
    assert_eq!(capture.artifacts.lock().await.len(), 1);

    let socket = tool_broker_endpoint(temporary.path().join("repairs.sock"));
    let broker = start_tool_broker(socket.clone(), policy, Arc::new(executor))
        .await
        .unwrap();
    let secret = "sk-private-value-must-not-appear";
    for (method, params) in [
        ("dataset.import", json!({"path": [secret]})),
        (
            "dataset.export",
            json!({"dataset_id": "ds", "revision": secret, "path": "out.json", "format": "json"}),
        ),
        (
            "chart.publish",
            json!({"mode": "create", "title": [secret]}),
        ),
    ] {
        let response = broker_request(&socket, broker.token(), method, params).await;
        assert_eq!(response["error"]["code"], "malformed_request", "{response}");
        assert!(!response.to_string().contains(secret));
    }
    broker.shutdown().await.unwrap();
}

#[tokio::test]
async fn aborted_turn_capture_persists_no_artifact_or_evidence() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let evidence_dir = temporary.path().join("loupist/loupist/evidence");
    fs::create_dir_all(&evidence_dir).unwrap();
    let capture = Arc::new(ToolCapture::for_chat("assistant-aborted".into()));
    delivered(&capture, provider_result(), json!({"value": 42})).await;
    executor_for(&state, capture.clone())
        .execute(
            &policy(false, false),
            ToolMethod::ArtifactPublish,
            markdown("Aborted artifact"),
        )
        .await
        .unwrap();
    assert_eq!(capture.artifacts.lock().await.len(), 1);
    drop(capture);
    assert!(state
        .store
        .list_chat_artifacts("chat-a")
        .unwrap()
        .is_empty());
    assert!(state
        .store
        .get_chat("chat-a")
        .unwrap()
        .unwrap()
        .messages
        .is_empty());
    assert_eq!(fs::read_dir(evidence_dir).unwrap().count(), 0);
}

#[test]
fn chart_query_window_is_bounded_by_serialized_bytes() {
    let cell = "x".repeat(8 * 1024);
    let rows = (0..200)
        .map(|_| vec![Value::String(cell.clone()); 2])
        .collect::<Vec<_>>();
    let end = bounded_chart_query_end(&rows, 0, rows.len()).unwrap();
    assert!(end > 0 && end < rows.len());
    assert!(serde_json::to_vec(&rows[..end]).unwrap().len() <= MAX_CHART_QUERY_BYTES);
    assert!(serde_json::to_vec(&rows[..=end]).unwrap().len() > MAX_CHART_QUERY_BYTES);
    assert!(
        bounded_chart_query_end(&[vec![json!("x".repeat(MAX_CHART_QUERY_BYTES))]], 0, 1).is_err()
    );
}

#[tokio::test]
async fn web_tools_use_the_capabilities_captured_for_the_run_and_refuse_unsafe_requests() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let pinned = executor_for(&state, Arc::new(ToolCapture::default()));
    assert!(pinned.capability_enabled("community.web-research"));
    let mut binding = state
        .store
        .get_loupist_capability("loupist-a", "community.web-research")
        .unwrap()
        .unwrap();
    binding.enabled = false;
    binding.granted_permissions.clear();
    binding.updated_at_ms += 1;
    state.store.save_loupist_capability(&binding).unwrap();
    // A run keeps the authority captured when it started.
    assert!(pinned.capability_enabled("community.web-research"));
    let disabled = executor_for(&state, Arc::new(ToolCapture::default()));
    let policy = policy(false, false);
    for denied in [
        disabled
            .web_search(&policy, json!({"query": "market news"}), "delivery")
            .await,
        disabled
            .web_fetch(&policy, json!({"source_id": "web:deadbeef"}), "delivery")
            .await,
    ] {
        assert!(matches!(denied, Err(BrokerError::MethodDenied)));
    }

    for refused in [
        json!({"query": "market news", "provider": "model"}),
        json!({"query": "market news", "_native_result": {}}),
    ] {
        assert!(pinned
            .web_search(&policy, refused, "delivery")
            .await
            .is_err());
    }
    for refused in [
        json!({"source_id": "web:deadbeef", "url": "https://example.com/report"}),
        json!({"url": "https://example.com/report", "offset": 1}),
        json!({"url": "http://127.0.0.1/secret"}),
    ] {
        assert!(pinned
            .web_fetch(&policy, refused, "delivery")
            .await
            .is_err());
    }
    assert!(pinned.capture.web_sources.lock().await.is_empty());
    assert!(pinned.capture.pending_deliveries.lock().await.is_empty());
}

#[tokio::test]
async fn an_exact_read_outranks_search_discovery_only_with_a_receipt() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = bare_executor(&temporary);
    let discovered = MemoryRefSnapshot {
        read_ref: "lens:quality".into(),
        kind: "lens".into(),
        title: "Quality".into(),
        excerpt: "Search hit".into(),
        as_of: Some("2026-08-01T00:00:00Z".into()),
        section: None,
        access: MemoryAccess::SearchDiscovered,
        full_record_digest: None,
        record_id: "rec_00000000000000000000000000000001".into(),
        revision_id: "rev_00000000000000000000000000000001".into(),
        object_id: "object".into(),
        span_id: None,
        receipt_id: None,
    };
    let mut exact = MemoryRefSnapshot {
        excerpt: "Exact record body".into(),
        section: Some("Boundary".into()),
        access: MemoryAccess::ExactRead,
        ..discovered.clone()
    };
    assert!(executor.stage_memory("exact", exact.clone()).await.is_err());
    exact.receipt_id = Some("rdg_00000000000000000000000000000001".into());
    for (delivery, memory) in [("exact", exact.clone()), ("discovered", discovered)] {
        executor.stage_memory(delivery, memory).await.unwrap();
        executor.capture.commit_delivery(delivery).await;
    }
    assert_eq!(
        executor.capture.memories.lock().await["lens:quality"],
        exact
    );
}

#[tokio::test]
async fn under_an_as_of_date_reports_written_later_and_charts_of_later_data_are_refused() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let today = policy(false, false);
    let cutoff = ToolPolicy {
        as_of: Some("2026-03-10".into()),
        ..policy(false, false)
    };
    let publish = |source: &'static str, method: ToolMethod, params: Value| {
        let state = state.clone();
        async move {
            let capture = Arc::new(ToolCapture::for_chat(source.into()));
            executor_for(&state, capture.clone())
                .execute(&policy(false, false), method, params)
                .await
                .unwrap();
            let commit = capture.artifacts.lock().await[0].clone();
            save_artifact_turn(&state, &commit);
            commit.artifact.id
        }
    };
    let report = publish(
        "assistant-report",
        ToolMethod::ArtifactPublish,
        markdown("FY2025 results review"),
    )
    .await;
    let rows = json!({"data": {"rows": [
        {"date": "2026-02-27", "close": 100},
        {"date": "2026-03-02", "close": 101}
    ]}});
    let chart_of = |metadata: Value, key: &str| {
        let reference = state
            .store
            .dataset_put("loupist-a", key, &rows, metadata)
            .unwrap();
        line_chart_from(json!({"from_dataset": {"dataset_id": reference.dataset_id,
            "revision": reference.revision, "rows_pointer": "/data/rows", "columns": price_columns()}}))
    };
    let later_chart = publish(
        "assistant-later-chart",
        ToolMethod::ChartPublish,
        chart_of(json!({}), "prices-today"),
    )
    .await;
    let dated_chart = publish(
        "assistant-dated-chart",
        ToolMethod::ChartPublish,
        chart_of(json!({"as_of": "2026-03-02"}), "prices-then"),
    )
    .await;

    let capture = Arc::new(ToolCapture::for_chat("assistant-reader".into()));
    let reader = executor_for(&state, capture.clone());
    let read = |artifact: &str| json!({"artifact_id": artifact});
    let rows_of = |artifact: &str| json!({"artifact_id": artifact, "revision": 1});
    // A report written after the cutoff is left out and counted.
    let refused = reader
        .execute(&cutoff, ToolMethod::ArtifactRead, read(&report))
        .await
        .unwrap_err();
    assert!(refused.to_string().contains("as_of_later"), "{refused}");
    assert_eq!(capture.as_of_left_out.lock().unwrap().counts().notes, 1);
    reader
        .execute(&today, ToolMethod::ArtifactRead, read(&report))
        .await
        .unwrap();
    // A chart of data fetched without the cutoff cannot be read or queried as of then.
    let refused = reader
        .execute(&cutoff, ToolMethod::ArtifactRead, read(&later_chart))
        .await
        .unwrap_err();
    assert!(
        refused.to_string().contains("as_of_unsupported"),
        "{refused}"
    );
    reader
        .execute(&today, ToolMethod::ArtifactRead, read(&later_chart))
        .await
        .unwrap();
    let refused = reader
        .execute(&cutoff, ToolMethod::ChartQuery, rows_of(&later_chart))
        .await
        .unwrap_err();
    assert!(
        refused.to_string().contains("as_of_unsupported"),
        "{refused}"
    );
    // A chart of data fetched under an earlier cutoff stays usable.
    reader
        .execute(&cutoff, ToolMethod::ArtifactRead, read(&dated_chart))
        .await
        .unwrap();
    let rows = reader
        .execute(&cutoff, ToolMethod::ChartQuery, rows_of(&dated_chart))
        .await
        .unwrap();
    let returned_rows: Value = serde_json::from_str(
        rows["rows"]
            .as_str()
            .and_then(crate::custom_connections::taint::delimited_body)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(returned_rows.as_array().unwrap().len(), 2);
    // Drawing a new chart under the cutoff refuses data fetched without one…
    let refused = reader
        .execute(
            &cutoff,
            ToolMethod::ChartPublish,
            chart_of(json!({}), "prices-draw-later"),
        )
        .await
        .unwrap_err();
    assert!(
        refused.to_string().contains("as_of_unsupported"),
        "{refused}"
    );
    // …and accepts data fetched under an earlier cutoff.
    reader
        .execute(
            &cutoff,
            ToolMethod::ChartPublish,
            chart_of(json!({"as_of": "2026-03-02"}), "prices-draw-then"),
        )
        .await
        .unwrap();
}
