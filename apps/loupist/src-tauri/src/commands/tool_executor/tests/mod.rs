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
        display_currency: None,
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
    let mut research = capture.staged_research.lock().await.clone();
    if let Some(research) = &mut research {
        assert!(!crate::commands::tool_executor::finish_setup_theses(
            &state.store,
            "loupist-a",
            research,
        )
        .unwrap());
    }
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
    broker_request_attributed(socket, token, method, params, None).await
}

async fn broker_request_attributed(
    socket: &Path,
    token: &str,
    method: &str,
    params: Value,
    call_id: Option<&str>,
) -> Value {
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
        "call_id": call_id,
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
mod pasted;
mod question;
mod scope_and_artifacts;
mod shared_delivery;
mod untrusted_text;
mod web_as_of;
mod workspace;

mod custom_connections;
mod execution_work;
mod grep_trust;
mod refusals;
mod release_boundaries;
mod runtime_profile;

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
        evidence_origin: None,
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
    let key = || async {
        let credential = executor.provider_credential_bundle(kis).await.unwrap();
        executor.persistent_request_key(&reader, ToolMethod::KisRead, &balance, Some(&credential))
    };

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

#[tokio::test]
async fn direct_saved_key_tools_refuse_with_the_permission_recovery() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = executor_for(
        &state,
        Arc::new(ToolCapture {
            saved_keys_needing_permission: BTreeSet::from([
                "openfigi.lookup".into(),
                "fred.macro".into(),
            ]),
            ..Default::default()
        }),
    );
    for (method, params) in [
        (ToolMethod::AssetLookup, json!({"query": "ACME"})),
        (
            ToolMethod::McpCall,
            json!({
                "server_id": "openbb",
                "tool_name": "economy_fred_series",
                "arguments": { "provider": "fred" }
            }),
        ),
    ] {
        let result = executor
            .execute_for_delivery(&policy(false, false), method, params, "refusal")
            .await;
        assert_eq!(
            crate::broker::error_body(&result.unwrap_err()),
            json!({
                "code": "keychain_permission_required",
                "message": "This connection's saved key needs the user's permission in macOS first.",
                "retryable": false,
                "next": "Tell the user to open Settings > Data connections and choose Allow access, then ask again. Continue this reply without this connection."
            })
        );
    }
    assert_eq!(
        crate::finance_credentials::test_store::backend_calls(&state.credential_environment),
        0
    );
}

#[cfg(unix)]
fn python_executor(state: &AppState, temporary: &tempfile::TempDir) -> AppToolExecutor {
    static CACHE: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let workspace = temporary.path().join("loupist");
    let absent = temporary.path().join("absent");
    let roots = crate::agent_harness::native_policy::NativeRoots::new(
        &workspace,
        &absent,
        &state.artifacts.app_data_dir,
        &workspace.join(".loupist"),
        &absent,
        &absent,
        None,
    );
    let mut executor = executor_for(
        state,
        Arc::new(ToolCapture {
            native: NativeGate::for_turn(roots),
            ..ToolCapture::for_chat("python-test".into())
        }),
    );
    Arc::make_mut(&mut executor.state.artifacts).python_cache_root = CACHE
        .get_or_init(|| tempfile::tempdir().unwrap())
        .path()
        .to_path_buf();
    executor
}

#[cfg(unix)]
#[tokio::test]
async fn python_pending_round_marks_tagged_files_before_dispatching_custom_reads() {
    let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = python_executor(&state, &temporary);
    let input = "Outside text in a tagged file";
    crate::commands::export_trust::record(&state, input.as_bytes()).unwrap();
    fs::write(temporary.path().join("loupist/input.txt"), input).unwrap();

    let result = executor
        .execute(
            &policy(false, false),
            ToolMethod::PythonRun,
            json!({"code": r#"
import loupist
with open('input.txt') as source:
    loupist.call('mcp__custom-0123456789abcdef0123456789abcdef__quote', query=source.read())
"#}),
        )
        .await
        .unwrap();
    assert_eq!(result["rounds"], 2, "{result}");
    assert_eq!(result["reads"][0]["status"], "untrusted_text_paused");
}

#[cfg(unix)]
#[tokio::test]
async fn python_writeback_tags_new_and_replaced_files_from_broker_reads() {
    let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = python_executor(&state, &temporary);
    let output = temporary.path().join("loupist/outputs");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("existing.txt"), "previous content").unwrap();
    fs::write(output.join("untouched.txt"), "independent content").unwrap();
    let data = state
        .store
        .dataset_put(
            "loupist-a",
            "outside-text",
            &json!({"text": "Provider text"}),
            json!({"trust":"third_party_text"}),
        )
        .unwrap();

    let result = executor
        .execute(
            &policy(true, false),
            ToolMethod::PythonRun,
            json!({"inputs":[{"name":"source", "dataset":{
                "dataset_id":data.dataset_id, "revision":data.revision, "pointer":"/text"
            }}], "code":r#"
import loupist
from pathlib import Path
source = loupist.inputs['source']
Path('outputs/new.txt').write_text('New: ' + source)
Path('outputs/existing.txt').write_text('Update: ' + source)
"#}),
        )
        .await
        .unwrap();
    assert_eq!(result["status"], "completed", "{result}");
    for (name, expected, tagged) in [
        ("new.txt", "New: Provider text", true),
        ("existing.txt", "Update: Provider text", true),
        ("untouched.txt", "independent content", false),
    ] {
        let content = fs::read(output.join(name)).unwrap();
        assert_eq!(content, expected.as_bytes());
        assert_eq!(
            crate::commands::export_trust::contains(&state, &sha256(&content)).unwrap(),
            tagged,
            "{name}"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn python_replay_screens_named_arguments_and_unprinted_values() {
    let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = python_executor(&state, &temporary);
    let data = state
        .store
        .dataset_put(
            "loupist-a",
            "fixture-response",
            &json!({"items":["sk-fixture-key"]}),
            json!({"trust":"numeric"}),
        )
        .unwrap();

    for (input, expected_replays) in [
        (
            json!({"name":"x", "call":{"tool":"followed", "arguments":{}}}),
            1,
        ),
        (
            json!({"name":"x", "call":{"tool":"followed", "arguments":{"password":"fixture"}}}),
            1,
        ),
        (
            json!({"name":"x", "dataset":{"dataset_id":data.dataset_id, "revision":data.revision}}),
            1,
        ),
    ] {
        let result = TOOL_CALL_ID
            .scope(
                new_id("python-replay-test"),
                executor.execute(
                    &policy(true, false),
                    ToolMethod::PythonRun,
                    json!({"inputs":[input], "code":"print('safe output')"}),
                ),
            )
            .await
            .unwrap();
        assert_eq!(result["status"], "completed", "{result}");
        assert!(result["stdout"].as_str().unwrap().contains("safe output"));
        let replays: u32 = state
            .store
            .lock_loupist("loupist-a")
            .unwrap()
            .query_row(
                "SELECT count(*) FROM datasets WHERE json_extract(metadata_json, '$.origin') = 'python_replay'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(replays, expected_replays, "{input}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn python_dataset_selection_keeps_its_host_receipt_separate_from_the_guest_value() {
    let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = python_executor(&state, &temporary);
    let dataset = state
        .store
        .dataset_put(
            "loupist-a",
            "dataset-with-reference-field",
            &json!({"row":{"count":21,"result_ref":"stored row field"}}),
            json!({"trust":"numeric"}),
        )
        .unwrap();
    let result = executor.execute(
        &policy(true, false),
        ToolMethod::PythonRun,
        json!({"args":{"dataset_id":dataset.dataset_id,"revision":dataset.revision,"pointer":"/row"},"code":r#"
import loupist
row = loupist.dataset(**loupist.args)
assert row == {'count': 21, 'result_ref': 'stored row field'}
print(row['count'] * 2)
"#}),
    ).await.unwrap();
    assert_eq!(result["status"], "completed", "{result}");
    assert_eq!(result["stdout"].as_str().unwrap().trim(), "42");
    let parent = executor
        .capture
        .run_result(result["result_ref"].as_str().unwrap())
        .await
        .unwrap();
    assert_eq!(parent.upstream_result_refs.len(), 1);
    let reference = &parent.upstream_result_refs[0];
    let receipt = executor.capture.run_result(reference).await.unwrap();
    assert_eq!(receipt.producer.tool_name, "dataset_describe");
    assert_eq!(receipt.payload["dataset_ref"], json!(dataset));
    assert_eq!(
        result["reads"][0]["citation_url"],
        evidence::citation_url(reference)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn python_check_screens_retained_results_and_preserves_safe_state() {
    let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = python_executor(&state, &temporary);
    let allowed = [crate::scheduled_tasks::types::TriggerToolGrant {
        name: "followed".into(),
        provider: None,
    }];
    let code = r#"
import loupist
loupist.followed()
loupist.state['visits'] += 1
if loupist.args.get('echo'):
    loupist.state.update(loupist.args)
loupist.result(False, 'No change', '2026-11-24')
"#;
    let run = |args, after_outside_text, use_memory| {
        executor.run_check(super::python_reads::CheckRun {
            code,
            after_outside_text,
            use_memory,
            args,
            state: json!({"visits":1}),
            now: "2026-11-24T00:00:00Z".into(),
            as_of: None,
            allowed: &allowed,
            discover_providers: false,
            replay: None,
            max_rounds: 3,
            max_calls: 3,
        })
    };
    let clean = run(Default::default(), false, true).await.unwrap();
    assert_eq!(clean.state, json!({"visits":2}));
    assert_eq!(clean.ledger.len(), 1);
    let replay = |ledger| {
        executor.run_check(super::python_reads::CheckRun {
            code: r#"
import loupist
try:
    loupist.followed()
except loupist.Refused:
    pass
loupist.state['visits'] += 1
loupist.result(False, 'No change', '2026-11-24')
"#,
            after_outside_text: false,
            use_memory: true,
            args: Default::default(),
            state: json!({"visits":1}),
            now: "2026-11-24T00:00:00Z".into(),
            as_of: None,
            allowed: &allowed,
            discover_providers: false,
            replay: Some(ledger),
            max_rounds: 1,
            max_calls: 0,
        })
    };
    assert_eq!(
        replay(clean.ledger.as_slice()).await.unwrap().state,
        clean.state
    );
    assert!(
        matches!(replay(&[]).await, Err(BrokerError::Execution(message)) if message == "Recorded inputs do not cover this replay")
    );
    let sensitive = json!({"password":"fixture"});
    let screened = run(sensitive.as_object().unwrap().clone(), false, true)
        .await
        .unwrap();
    assert!(screened.ledger.is_empty());
    assert_eq!(screened.state, clean.state);
    let copied = json!({"password":"fixture", "echo":true});
    let screened = run(copied.as_object().unwrap().clone(), false, true)
        .await
        .unwrap();
    assert_eq!(screened.error.as_deref(), Some("sensitive_text"));
    assert_eq!(screened.reads, 1);
    assert!(screened.ledger.is_empty());
    assert_eq!(screened.state, json!({}));
    let private = run(Default::default(), false, false).await.unwrap();
    assert_eq!(private.ledger[0]["response"]["code"], "needs_permission");
    assert_ne!(private.state, clean.state);
    assert!(matches!(
        run(Default::default(), true, true).await,
        Err(BrokerError::UntrustedTextPaused)
    ));
    assert!(executor.capture.turn_taint.has_untrusted_content());
}

#[cfg(unix)]
#[tokio::test]
async fn python_ledger_runs_a_script_saved_in_an_earlier_reply_with_scheduled_reads() {
    let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let current = executor_for(&state, Arc::new(ToolCapture::for_chat("reply-a".into())));
    let data = state
        .store
        .dataset_put(
            "loupist-a",
            "quarters",
            &json!([7, 8, 9]),
            json!({"as_of":"2026-11-24"}),
        )
        .unwrap();
    let saved = current
        .execute(
            &policy(true, true),
            ToolMethod::ScriptSave,
            json!({
                "name":"quarter-total", "summary":"Sum the retained quarterly data.",
                "code":"import loupist\nprint(sum(loupist.dataset(**loupist.args)))",
            }),
        )
        .await
        .unwrap();
    assert_eq!(saved["version"], 1);
    let (folder, roots) = crate::python_run::ledger::empty_roots().unwrap();
    let unattended = executor_for(
        &state,
        Arc::new(ToolCapture {
            native: NativeGate::for_turn(roots),
            unattended_run_id: Some("scheduled-test".into()),
            ..ToolCapture::default()
        }),
    );
    let mut policy = policy(true, false);
    policy.as_of = Some("2026-11-24".into());
    let request =
        json!({"script":"quarter-total@1", "args":{"dataset_id":data.dataset_id, "revision":1}});
    let result = unattended
        .execute(&policy, ToolMethod::PythonRun, request.clone())
        .await
        .unwrap();
    assert_eq!(result["status"], "completed", "{result}");
    let printed = |result: &Value| {
        let stdout = result["stdout"].as_str().unwrap();
        assert!(stdout.starts_with("<<untrusted "), "{result}");
        assert!(stdout.lines().any(|line| line == "24"), "{result}");
    };
    printed(&result);
    assert_eq!(result["rounds"], 2);
    let second = unattended
        .execute(&policy, ToolMethod::PythonRun, request)
        .await
        .unwrap();
    printed(&second);
    assert_eq!(second["rounds"], 1);
    drop(folder);
}
