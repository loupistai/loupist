use super::*;
use crate::commands::tests::{chat, profile, seed_profile};
fn policy() -> ToolPolicy {
    ToolPolicy {
        loupist_id: "loupist-a".into(),
        session_id: "chat-a".into(),
        use_memory: false,
        propose_memory_updates: false,
        memory_proposal_budget: 0,
        as_of: None,
    }
}
fn executor(temporary: &tempfile::TempDir) -> AppToolExecutor {
    let state = AppState::for_persistent_test(temporary.path().join("app"));
    for id in ["loupist-a", "loupist-b"] {
        let workspace = temporary.path().join(id);
        std::fs::create_dir_all(&workspace).unwrap();
        seed_profile(state.store.as_ref(), &profile(id, &workspace, 1));
    }
    state
        .store
        .create_chat(&chat("chat-a", "loupist-a", 1))
        .unwrap();
    AppToolExecutor {
        state,
        capture: Arc::new(ToolCapture::for_chat("assistant-dataset-test".into())),
        loupist_id: "loupist-a".into(),
        capability_ids: BTreeSet::new(),
    }
}

#[tokio::test]
async fn dataset_export_writes_owned_pinned_full_data_without_overwriting() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let rows = (0..6000)
        .map(|index| json!({"day": index, "value": format!("{}.{:02}", index, index % 100)}))
        .collect::<Vec<_>>();
    let payload = json!({"data":{"rows":rows}});
    let reference = executor
        .state
        .store
        .dataset_put(
            "loupist-a",
            "prices",
            &payload,
            json!({"as_of":"2025-12-31"}),
        )
        .unwrap();
    let foreign = executor
        .state
        .store
        .dataset_put("loupist-b", "foreign", &payload, json!({}))
        .unwrap();
    let call = json!({"dataset_id":reference.dataset_id,"revision":reference.revision,"pointer":"/data/rows","path":"exports/prices.csv","format":"csv","columns":["day","value"]});
    let result = executor
        .execute(&policy(), ToolMethod::DatasetExport, call.clone())
        .await
        .unwrap();
    // An export is an ordinary research file: never the Loupist's private
    // folder, git metadata, a Skill folder or a file loaded as instructions.
    for protected in [
        ".loupist/config.toml",
        ".loupist/skills/planted/SKILL.md",
        ".agents/skills/planted.md",
        ".git/hooks/post-checkout",
        "notes/.hidden/prices.csv",
        "AGENTS.md",
        "agents.override.md",
        "CLAUDE.md",
    ] {
        let mut refused = call.clone();
        refused["path"] = json!(protected);
        let error = executor
            .execute(&policy(), ToolMethod::DatasetExport, refused)
            .await
            .unwrap_err();
        assert!(
            matches!(&error, BrokerError::WithNext { source, .. } if matches!(**source, BrokerError::ProtectedPath)),
            "{protected}: {error:?}"
        );
        assert!(!temporary.path().join("loupist-a").join(protected).exists());
    }
    let exported = temporary.path().join("loupist-a/exports/prices.csv");
    let bytes = std::fs::read(&exported).unwrap();
    assert_eq!(String::from_utf8_lossy(&bytes).lines().count(), 6001);
    assert_eq!(result["content_sha256"], sha256(&bytes));
    let provenance: Value = serde_json::from_slice(
        &std::fs::read(
            temporary
                .path()
                .join("loupist-a/exports/prices.csv.provenance.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(provenance["dataset_ref"], json!(reference));
    assert_eq!(provenance["source_metadata"]["as_of"], "2025-12-31");

    let mut foreign_call = call.clone();
    foreign_call["dataset_id"] = json!(foreign.dataset_id);
    foreign_call["path"] = json!("foreign.csv");
    let mut traversal = call.clone();
    traversal["path"] = json!("../escape.csv");
    for refused in [call, foreign_call, traversal] {
        assert!(executor
            .execute(&policy(), ToolMethod::DatasetExport, refused)
            .await
            .is_err());
    }
    assert_eq!(std::fs::read(&exported).unwrap(), bytes);
    assert!(!temporary.path().join("loupist-a/foreign.csv").exists());
    assert!(!temporary.path().join("escape.csv").exists());
}

#[tokio::test]
async fn dataset_reads_are_citable_saved_context_with_exact_slice_and_lineage() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let payload = json!({"data":{"rows":[{"year":2024,"value":10},{"year":2025,"value":20}]}});
    let reference = executor
        .state
        .store
        .dataset_put("loupist-a", "evidence-slice", &payload, json!({}))
        .unwrap();
    let mut original = payload;
    original["dataset_ref"] = json!(reference);
    let upstream = executor
        .capture
        .stage_run_result_with_delivery(
            "original",
            RunResultProducer {
                origin: RunResultOrigin::AcquiredOriginal,
                runtime_id: "native-finance".into(),
                tool_name: "dart_financials".into(),
                provider: Some("issuer".into()),
            },
            &json!({"symbol":"TEST"}),
            original,
            vec![],
            None,
        )
        .await
        .unwrap();
    executor.capture.commit_delivery("original").await;
    let output = executor
        .execute(
            &policy(),
            ToolMethod::DatasetQuery,
            json!({"dataset_id":reference.dataset_id,"revision":reference.revision,
                "pointer":"/data/rows","offset":1,"limit":1,"columns":["year","value"]}),
        )
        .await
        .unwrap();
    let result_ref = output["result_ref"].as_str().unwrap();
    let captured = executor.capture.run_result(result_ref).await.unwrap();
    assert_eq!(captured.producer.origin, RunResultOrigin::SavedContext);
    assert_eq!(
        captured.upstream_result_refs,
        std::slice::from_ref(&upstream)
    );
    assert_eq!(captured.payload["rows"], json!([{"year":2025,"value":20}]));
    let receipt = executor
        .capture
        .evidence_refs
        .lock()
        .await
        .iter()
        .find(|item| item.id == evidence::evidence_id(result_ref))
        .cloned()
        .unwrap();
    assert_eq!(receipt.dataset_ref.as_ref(), Some(&reference));
    assert_eq!(
        receipt.upstream_evidence_ids,
        [evidence::evidence_id(&upstream)]
    );
}

#[tokio::test]
async fn imported_local_results_are_revisioned_and_charts_keep_their_frozen_rows() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let workspace = temporary.path().join("loupist-a");
    let source = executor
        .state
        .store
        .dataset_put(
            "loupist-a",
            "source",
            &json!([{"year":2024,"value":10}]),
            json!({"as_of":"2025-12-31"}),
        )
        .unwrap();
    std::fs::write(
        workspace.join("output.json"),
        r#"[{"year":2024,"value":20},{"year":2025,"value":40}]"#,
    )
    .unwrap();
    let import = json!({"path":"output.json","input_refs":[{"name":"source","dataset_id":source.dataset_id,"revision":1}]});
    let result = executor
        .execute(&policy(), ToolMethod::DatasetImport, import.clone())
        .await
        .unwrap();
    let reference: crate::datasets::DatasetRef =
        serde_json::from_value(result["dataset_ref"].clone()).unwrap();
    let (_, _, metadata) = executor
        .state
        .store
        .dataset("loupist-a", &reference.dataset_id, 1)
        .unwrap();
    assert!(
        metadata.get("as_of").is_none(),
        "derived data does not inherit as_of"
    );
    assert_eq!(
        executor
            .capture
            .run_result(result["result_ref"].as_str().unwrap())
            .await
            .unwrap()
            .producer
            .origin,
        RunResultOrigin::Derived
    );
    executor.execute(&policy(), ToolMethod::ChartPublish, json!({
        "mode":"create", "title":"Local result", "dataset":{"from_dataset":{
            "dataset_id":reference.dataset_id,"revision":1,"rows_pointer":"",
            "columns":[{"id":"year","label":"Year","kind":"number","pointer":"/year"},{"id":"value","label":"Value","kind":"number","pointer":"/value"}]
        }},"view":{"kind":"bar","x":"year","y":["value"]}
    })).await.unwrap();
    let frozen = executor.capture.artifacts.lock().await[0].datasets[0]
        .rows
        .clone();
    assert_eq!(
        frozen,
        vec![vec![json!(2024), json!(20)], vec![json!(2025), json!(40)]]
    );

    std::fs::write(
        workspace.join("output.json"),
        "[{\"year\":2025,\"value\":99}]",
    )
    .unwrap();
    let updated = executor.dataset_import(import).unwrap();
    assert_eq!(updated["dataset_ref"]["dataset_id"], reference.dataset_id);
    assert_eq!(updated["dataset_ref"]["revision"], 2);
    assert_eq!(
        executor.capture.artifacts.lock().await[0].datasets[0].rows,
        frozen
    );
    let state = executor.state.clone();
    drop(executor);
    state.close_for_restart_test();
    let reopened = AppState::for_persistent_test(temporary.path().join("app"));
    assert_eq!(
        reopened
            .store
            .dataset("loupist-a", &reference.dataset_id, 2)
            .unwrap()
            .1,
        json!([{"year":2025,"value":99}])
    );
}

#[tokio::test]
async fn dataset_import_rejects_invalid_files_and_foreign_lineage_before_persistence() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let workspace = temporary.path().join("loupist-a");
    std::fs::write(workspace.join("good.json"), "[1,2]").unwrap();
    std::fs::write(workspace.join("bad.json"), "{broken").unwrap();
    std::fs::write(workspace.join("binary.json"), [0xff, 0xfe]).unwrap();
    let foreign = executor
        .state
        .store
        .dataset_put("loupist-b", "foreign", &json!([1]), json!({}))
        .unwrap();
    for request in [
        json!({"path":"../escape.json"}),
        json!({"path":"binary.json"}),
        json!({"path":".loupist/data.json"}),
        json!({"path":"bad.json"}),
        json!({"path":"good.json","expected_sha256":"0".repeat(64)}),
        json!({"path":"good.json","input_refs":[{"name":"foreign","dataset_id":foreign.dataset_id,"revision":1}]}),
        json!({"path":"good.json","loupist_id":"loupist-b"}),
    ] {
        assert!(executor
            .execute(&policy(), ToolMethod::DatasetImport, request)
            .await
            .is_err());
    }
    let oversized = std::fs::File::create(workspace.join("large.json")).unwrap();
    oversized
        .set_len(crate::datasets::MAX_DATASET_BYTES as u64 + 1)
        .unwrap();
    assert!(executor
        .dataset_import(json!({"path":"large.json"}))
        .is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(workspace.join("good.json"), workspace.join("link.json"))
            .unwrap();
        std::os::unix::fs::symlink(&workspace, workspace.join("linked-dir")).unwrap();
        for linked in ["link.json", "linked-dir/good.json"] {
            assert!(executor.dataset_import(json!({"path": linked})).is_err());
        }
    }
    let db = executor.state.store.lock_loupist("loupist-a").unwrap();
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM datasets WHERE loupist_id='loupist-a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}
