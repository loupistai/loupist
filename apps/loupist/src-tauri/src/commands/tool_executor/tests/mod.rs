use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::*;
use crate::{
    broker::{start_tool_broker, tool_broker_endpoint},
    commands::{
        enabled_execute_capability_ids,
        tests::{chat, initialized_workspace, profile, seed_profile},
    },
    domain::{ChatMessage, ChatRole},
    test_fixtures::chat_message,
};

fn captured_capabilities(state: &AppState, loupist_id: &str) -> BTreeSet<String> {
    enabled_execute_capability_ids(state, loupist_id)
        .unwrap()
        .into_iter()
        .collect()
}

/// An executor for loupist-a whose profile is not seeded.
fn bare_executor(temporary: &tempfile::TempDir) -> AppToolExecutor {
    let workspace = temporary.path().join("loupist");
    fs::create_dir_all(&workspace).unwrap();
    AppToolExecutor {
        state: AppState::for_test(temporary.path().join("app")),
        capture: Arc::new(ToolCapture::default()),
        loupist_id: "loupist-a".into(),
        capability_ids: BTreeSet::new(),
    }
}

/// loupist-a with its workspace at `<temporary>/loupist` and an empty chat-a.
fn seeded_state(temporary: &tempfile::TempDir) -> AppState {
    let state = AppState::for_test(temporary.path().join("app"));
    seed_loupist(&state, temporary, "loupist-a");
    state
        .store
        .create_chat(&chat("chat-a", "loupist-a", 1))
        .unwrap();
    state
}

fn seed_loupist(state: &AppState, temporary: &tempfile::TempDir, loupist_id: &str) {
    let workspace = temporary.path().join(if loupist_id == "loupist-a" {
        "loupist"
    } else {
        loupist_id
    });
    fs::create_dir_all(&workspace).unwrap();
    seed_profile(state.store.as_ref(), &profile(loupist_id, &workspace, 1));
}

fn executor_for(state: &AppState, capture: Arc<ToolCapture>) -> AppToolExecutor {
    AppToolExecutor {
        capability_ids: captured_capabilities(state, "loupist-a"),
        state: state.clone(),
        capture,
        loupist_id: "loupist-a".into(),
    }
}

fn policy(use_memory: bool, propose_memory_updates: bool) -> ToolPolicy {
    ToolPolicy {
        loupist_id: "loupist-a".into(),
        session_id: "chat-a".into(),
        use_memory,
        propose_memory_updates,
        memory_proposal_budget: u8::from(propose_memory_updates),
        as_of: None,
    }
}

/// Stages a provider result through the executor's capture path and acknowledges it.
async fn delivered(capture: &ToolCapture, producer: RunResultProducer, payload: Value) -> String {
    let delivery_id = new_id("test-result");
    let result_ref = capture
        .stage_run_result_with_delivery(
            &delivery_id,
            producer,
            &json!({"symbol": "TEST"}),
            payload,
            Vec::new(),
            None,
        )
        .await
        .unwrap();
    capture.commit_delivery(&delivery_id).await;
    result_ref
}

fn provider_result() -> RunResultProducer {
    RunResultProducer {
        origin: RunResultOrigin::AcquiredOriginal,
        runtime_id: "openbb".into(),
        tool_name: "equity_price_historical".into(),
        provider: Some("openbb-keyless".into()),
    }
}

/// Captures a workspace file with the real tool and acknowledges the delivery.
async fn captured_file(executor: &AppToolExecutor, name: &str, contents: &str) -> String {
    let profile = executor
        .state
        .store
        .get_loupist("loupist-a")
        .unwrap()
        .unwrap();
    fs::write(Path::new(&profile.workspace_root).join(name), contents).unwrap();
    let delivery = new_id("capture");
    let policy = policy(true, true);
    let result = executor
        .execute_for_delivery(
            &policy,
            ToolMethod::WorkspaceCapture,
            json!({"path": name}),
            &delivery,
        )
        .await
        .unwrap();
    executor.commit_delivery(&policy, &delivery).await.unwrap();
    result["result_ref"].as_str().unwrap().to_owned()
}

/// Commits the staged turn exactly as the chat runtime does.
async fn persist_turn(state: &AppState, chat_id: &str, turn_id: &str, capture: &ToolCapture) {
    let sources = capture.staged_sources.lock().await.clone();
    let reads = capture.staged_reads.lock().await.clone();
    let research = capture.staged_research.lock().await.clone();
    let blobs = crate::commands::knowledge::knowledge_blobs(state).unwrap();
    let mut connection = state.store.lock().unwrap();
    let tx = connection.transaction().unwrap();
    crate::commands::knowledge::persist_saved_turn(
        &tx,
        &blobs,
        "loupist-a",
        chat_id,
        turn_id,
        now_ms(),
        &[],
        true,
        &sources,
        &reads,
        research.as_ref(),
    )
    .unwrap();
    tx.commit().unwrap();
}

/// Saves the assistant message that owns a published artifact, as a finished turn does.
pub(super) fn save_artifact_turn(state: &AppState, commit: &ArtifactCommit) {
    let mut updated = state.store.get_chat("chat-a").unwrap().unwrap();
    let expected = updated.clone();
    updated.messages.push(ChatMessage {
        created_at_ms: commit.artifact.updated_at_ms,
        artifact_refs: vec![commit.revision.artifact_ref(commit.artifact.title.clone())],
        ..chat_message(
            &commit.revision.source_message_id,
            ChatRole::Assistant,
            "Published.",
        )
    });
    updated.updated_at_ms = commit.artifact.updated_at_ms;
    state
        .store
        .save_chat_with_artifacts(&expected, &updated, std::slice::from_ref(commit))
        .unwrap();
}

struct CountingExecutor {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ToolExecutor for CountingExecutor {
    async fn execute(
        &self,
        _policy: &ToolPolicy,
        _method: ToolMethod,
        _params: Value,
    ) -> Result<Value, BrokerError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"unexpected": true}))
    }
}

async fn broker_request(socket: &Path, token: &str, method: &str, params: Value) -> Value {
    #[cfg(unix)]
    let mut stream = tokio::net::UnixStream::connect(socket).await.unwrap();
    #[cfg(windows)]
    let mut stream = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(socket)
        .unwrap();
    let mut request = serde_json::to_vec(&json!({
        "protocol": "loupist-tool/1",
        "id": "request-1",
        "token": token,
        "method": method,
        "params": params,
    }))
    .unwrap();
    request.push(b'\n');
    stream.write_all(&request).await.unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let response: Value = serde_json::from_str(&line).unwrap();
    let ack = json!({
        "protocol": "loupist-tool/1",
        "id": "request-1",
        "delivered": true,
    });
    reader
        .get_mut()
        .write_all(format!("{ack}\n").as_bytes())
        .await
        .unwrap();
    reader.get_mut().shutdown().await.unwrap();
    line.clear();
    reader.read_line(&mut line).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap()["committed"],
        true
    );
    response
}

#[test]
fn run_result_receipts_record_origin_provider_and_warnings() {
    assert_eq!(
        result_warnings(&json!({
            "warnings": ["top-level"],
            "structuredContent": {"warnings": [
                "top-level",
                {"category": "OpenBBWarning", "message": "provider fallback"}
            ]},
            "quality": {"warning": "partial"}
        })),
        ["OpenBBWarning: provider fallback", "partial", "top-level"]
    );
    assert_eq!(
        result_origin(&json!({"arguments": {"symbol": "NVDA"}})).as_deref(),
        Some("NVDA")
    );
    let search = run_result_producer(
        ToolMethod::WebSearch,
        &json!({}),
        &json!({"selected_provider": "exa_public", "source_id": "web:opaque"}),
    )
    .unwrap();
    assert_eq!(search.provider.as_deref(), Some("exa_public"));
    let fetch = run_result_producer(
        ToolMethod::WebFetch,
        &json!({}),
        &json!({"source_id": "web:opaque", "final_url": "https://investor.example.test/r"}),
    )
    .unwrap();
    assert_eq!(fetch.provider.as_deref(), Some("investor.example.test"));
}

mod knowledge_seam;
mod lens_file;
mod memory_as_of;
mod memory_authority;
mod question;
mod scope_and_artifacts;
mod shared_delivery;
mod untrusted_text;
mod web_as_of;
mod workspace;

mod custom_connections;

#[test]
fn world_reads_report_their_delivered_names_once_and_in_order() {
    let read = |payload| StagedKnowledgeRead {
        read_ref: "rdg_00000000000000000000000000000001".into(),
        scope: "world".into(),
        record_id: "rec_world".into(),
        span_id: None,
        revision_id: "rev_world".into(),
        object_id: "rec_world".into(),
        title: "World".into(),
        payload,
    };
    let world = |nodes: &str| AckedPayload::World {
        revision_digest: "digest".into(),
        view_json: format!(r#"{{"nodes":{nodes},"edges":[]}}"#),
        view_digest: "view".into(),
    };
    let reads = vec![
        read(world(r#"[{"id":"ent:hanul"},{"id":"ent:morrow"}]"#)),
        read(AckedPayload::Journal {
            journal_id: "journal-1".into(),
            delivered_text: "note".into(),
            digest: "digest".into(),
            coverage: "included_full".into(),
        }),
        read(world(r#"[{"id":"ent:morrow"},{"id":"ent:bluehaven"}]"#)),
    ];
    assert_eq!(
        delivered_world_node_ids(&reads),
        vec!["ent:hanul", "ent:morrow", "ent:bluehaven"]
    );
    let many = (0..40)
        .map(|index| format!(r#"{{"id":"n{index}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        delivered_world_node_ids(&[read(world(&format!("[{many}]")))]).len(),
        crate::chat::progress::MAX_PROGRESS_ENTITY_IDS
    );
}

#[tokio::test]
async fn plan_updates_return_a_short_receipt_or_the_field_to_fix() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));
    let policy = policy(false, false);
    let receipt = executor
        .execute(
            &policy,
            ToolMethod::PlanUpdate,
            json!({"items": [
                {"id": "filings", "label": "Read the filings", "status": "active"},
                {"id": "prices", "label": "Compare prices", "status": "pending"},
            ]}),
        )
        .await
        .unwrap();
    assert_eq!(receipt, json!({"status": "plan_updated", "items": 2}));
    // The plan is presentation only; it never becomes citable evidence.
    assert!(executor.capture.run_results.lock().await.results.is_empty());
    let refused = executor
        .execute(
            &policy,
            ToolMethod::PlanUpdate,
            json!({"items": [
                {"id": "a", "label": "One", "status": "active"},
                {"id": "b", "label": "Two", "status": "active"},
            ]}),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        refused,
        BrokerError::InvalidArguments { ref field, .. } if field == "items"
    ));
}

#[tokio::test]
async fn kis_max_age_reuse_never_crosses_accounts() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let kis = crate::finance_data::KIS_SOURCE_ID;
    let executor = AppToolExecutor {
        capability_ids: BTreeSet::from([kis.to_owned()]),
        ..executor_for(&state, Arc::new(ToolCapture::default()))
    };
    let use_account = |account: &str| {
        crate::finance_credentials::stage(
            &state.credential_environment,
            kis,
            &BTreeMap::from([
                ("app_key".to_owned(), "test-kis-app-key".to_owned()),
                ("app_secret".to_owned(), "test-kis-app-secret".to_owned()),
                ("account_number".to_owned(), account.to_owned()),
                ("account_product_code".to_owned(), "01".to_owned()),
            ]),
        )
        .unwrap();
        let candidate = crate::finance_credentials::candidate(&state.credential_environment, kis)
            .unwrap()
            .unwrap();
        crate::finance_credentials::finish_verification(
            &state.credential_environment,
            kis,
            candidate.revision(),
            crate::finance_credentials::VerificationOutcome::Verified,
            1,
            None,
        )
        .unwrap();
    };
    let balance = json!({"operation_id": "domestic_stock.inquire_balance", "params": {}});
    let reader = policy(false, false);
    let key = || executor.persistent_request_key(&reader, ToolMethod::KisRead, &balance);

    use_account("11111111");
    let first = key().await;
    assert_eq!(first, key().await);
    executor
        .state
        .store
        .dataset_put(
            "loupist-a",
            &first,
            &json!({"balance": "first account"}),
            json!({
            "fetched_at": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)}),
        )
        .unwrap();
    assert!(executor
        .state
        .store
        .dataset_fresh("loupist-a", &first, 3600)
        .unwrap()
        .is_some());

    // After switching accounts the identical request no longer finds the
    // first account's saved balances, and the key holds no account number.
    use_account("22222222");
    let second = key().await;
    assert_ne!(first, second);
    assert!(executor
        .state
        .store
        .dataset_fresh("loupist-a", &second, 3600)
        .unwrap()
        .is_none());
    for value in ["11111111", "22222222", "test-kis-app-secret"] {
        assert!(!first.contains(value) && !second.contains(value));
    }
    crate::finance_credentials::delete(&state.credential_environment, kis).unwrap();
}
