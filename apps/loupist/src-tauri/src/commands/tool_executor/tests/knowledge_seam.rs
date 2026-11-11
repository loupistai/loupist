use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::*;
use crate::knowledge::{
    Authorship, Coverage, CreateRecord, KnowledgeStore, RecordKind, RepresentationRole,
    RetainMaterial, RetainNewSource, Selector, SourceClass,
};

pub(super) struct RetainedFixture {
    record_id: String,
    revision_digest: String,
    span_id: String,
}

fn knowledge(state: &AppState) -> KnowledgeStore {
    KnowledgeStore::attach(
        state.store.clone(),
        crate::commands::knowledge::knowledge_blobs(state).unwrap(),
    )
}

pub(super) fn retain_publisher_source(state: &AppState, bytes: &[u8]) -> RetainedFixture {
    retain_authored_source(state, bytes, Authorship::Publisher)
}

pub(super) fn retain_authored_source(
    state: &AppState,
    bytes: &[u8],
    authorship: Authorship,
) -> RetainedFixture {
    let retained = knowledge(state)
        .transact(|write| {
            write.retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "loupist-a".into(),
                at_ms: 1_700_000_000_000,
                title: "Notes".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "https://example.test/notes".into(),
                provider_identity: "example-publisher".into(),
                authorship,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                publication_time: Some(crate::knowledge::TimePoint {
                    value: Some("2026-07-14".into()),
                    precision: crate::knowledge::TimePrecision::Date,
                    approximate: false,
                }),
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "doc-notes".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    bytes: bytes.to_vec(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: bytes.len() as u64,
                    },
                    context_note: "exact retained range".into(),
                }),
            })
        })
        .unwrap();
    RetainedFixture {
        record_id: retained.record_id,
        revision_digest: retained.revision_digest,
        span_id: retained.span_id.unwrap(),
    }
}

/// One World claim per proposition, each bound to the retained Source span.
pub(super) fn seal_world(
    state: &AppState,
    source: &RetainedFixture,
    propositions: &[&str],
) -> crate::knowledge::RecordRevision {
    use crate::knowledge::{
        BindingRole, ClaimEndpoint, ClaimStatus, DependencyRole, EvidenceBinding, EvidenceRevision,
        GraphClaim, GraphNode, SealWorld,
    };
    let node = |id: &str, kind: &str| GraphNode {
        id: id.into(),
        name: id.into(),
        aliases: Vec::new(),
        description: format!("The {id}."),
        kind: kind.into(),
        source_role: "source".into(),
        properties: Default::default(),
    };
    let claims = propositions
        .iter()
        .enumerate()
        .map(|(index, proposition)| GraphClaim {
            id: format!("claim-{index:03}"),
            proposition: (*proposition).into(),
            attribution: "example-publisher".into(),
            relation: "described".into(),
            endpoints: vec![
                ClaimEndpoint {
                    node_id: "ada".into(),
                    role: "source".into(),
                },
                ClaimEndpoint {
                    node_id: "engine".into(),
                    role: "target".into(),
                },
            ],
            qualifiers: Vec::new(),
            evidence: vec![EvidenceBinding {
                span_id: source.span_id.clone(),
                role: BindingRole::ExtractedFrom,
                attribution: "example-publisher".into(),
            }],
            status: ClaimStatus::Active,
            recorded_at_ms: Some(1_700_000_000_000),
            publication_time: None,
            effective_time: None,
            valid_at: None,
            invalid_at: None,
            expired_at: None,
        })
        .collect();
    knowledge(state)
        .transact(|write| {
            let record = write.create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "Described accounts".into(),
                at_ms: 1_700_000_000_000,
            })?;
            let sealed = write.seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: record.record_id,
                expected_revision_id: None,
                at_ms: 1_700_000_000_000,
                nodes: vec![node("ada", "person"), node("engine", "concept")],
                claims,
                annotations: Vec::new(),
                dependencies: vec![EvidenceRevision {
                    record_id: source.record_id.clone(),
                    revision_digest: source.revision_digest.clone(),
                    role: DependencyRole::WorldSource,
                }],
            })?;
            write.get_revision("loupist-a", &sealed.revision_id)
        })
        .unwrap()
}

async fn nack_read(socket: &Path, token: &str, params: Value) -> Value {
    #[cfg(unix)]
    let mut stream = tokio::net::UnixStream::connect(socket).await.unwrap();
    #[cfg(windows)]
    let mut stream = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(socket)
        .unwrap();
    let mut request = serde_json::to_vec(&json!({
        "protocol": "loupist-tool/1", "id": "request-nack", "token": token,
        "method": "world.read", "params": params
    }))
    .unwrap();
    request.push(b'\n');
    stream.write_all(&request).await.unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let ack = json!({"protocol": "loupist-tool/1", "id": "request-nack", "delivered": false});
    let _ = reader
        .get_mut()
        .write_all(format!("{ack}\n").as_bytes())
        .await;
    serde_json::from_str(&line).unwrap()
}

#[tokio::test]
async fn only_acknowledged_reads_become_receipts_and_turn_inputs() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let source = retain_publisher_source(&state, b"acknowledged bytes");
    let capture = Arc::new(ToolCapture::default());
    let executor = Arc::new(executor_for(&state, capture.clone()));
    let socket = tool_broker_endpoint(temporary.path().join("knowledge.sock"));
    let broker = start_tool_broker(socket.clone(), policy(true, true), executor.clone())
        .await
        .unwrap();
    let missing = broker_request(
        &socket,
        broker.token(),
        "world.read",
        json!({"scope": "source", "recordId": source.record_id, "spanId": "spn_00000000000000000000000000000000"}),
    )
    .await;
    assert_eq!(missing["ok"], false);

    let discovery = broker_request(
        &socket,
        broker.token(),
        "world.search",
        json!({"scope": "source", "query": "Notes"}),
    )
    .await;
    let hit = discovery["result"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hit| hit["recordId"] == source.record_id)
        .expect("retained Source is discoverable")
        .clone();
    assert!(capture.staged_reads.lock().await.is_empty());
    let read = broker_request(
        &socket,
        broker.token(),
        "world.read",
        hit["readArgs"].clone(),
    )
    .await;
    // A publisher's retained text comes back delimited; the bytes are unchanged.
    assert_eq!(
        read["result"]["text"]
            .as_str()
            .and_then(crate::custom_connections::taint::delimited_body),
        Some("acknowledged bytes")
    );
    assert_eq!(
        read["result"]["source"]["publicationTime"]["value"],
        "2026-07-14"
    );
    assert_eq!(capture.staged_reads.lock().await.len(), 1);
    let cited = capture
        .memories
        .lock()
        .await
        .values()
        .next()
        .cloned()
        .unwrap();
    assert_eq!(cited.record_id, source.record_id);
    assert_eq!(
        cited.receipt_id.as_deref(),
        read["result"]["readRef"].as_str()
    );

    // A delivery the agent did not acknowledge leaves no receipt behind.
    let nack_socket = tool_broker_endpoint(temporary.path().join("knowledge-nack.sock"));
    let nack_broker = start_tool_broker(nack_socket.clone(), policy(true, true), executor)
        .await
        .unwrap();
    let nack = nack_read(&nack_socket, nack_broker.token(), hit["readArgs"].clone()).await;
    assert_eq!(nack["ok"], true);
    nack_broker.shutdown().await.unwrap();
    assert_eq!(capture.staged_reads.lock().await.len(), 1);

    let submitted = broker_request(
        &socket,
        broker.token(),
        "note.save",
        json!({"workingNote": {"title": "Contribution plan", "markdown": "User assumption: 500 monthly at 4%.",
            "changeSummary": "Saved the working assumptions", "expectedResearchId": null}}),
    )
    .await;
    assert_eq!(submitted["ok"], true);
    persist_turn(&state, "chat-a", "turn-ack", &capture).await;
    let current =
        crate::commands::knowledge::current_working_context(&state, "loupist-a", "chat-a")
            .unwrap()
            .unwrap();
    let context = broker_request(
        &socket,
        broker.token(),
        "conversation.memory",
        json!({"action": "read", "noteId": current.research_id}),
    )
    .await;
    assert_eq!(context["result"]["notEvidence"], true);
    assert_eq!(
        context["result"]["note"]["markdown"],
        "User assumption: 500 monthly at 4%."
    );
    assert_eq!(
        capture.staged_reads.lock().await.len(),
        1,
        "working context must not create a source receipt"
    );
    broker.shutdown().await.unwrap();
}

async fn indexed_world_search(
    temporary: &tempfile::TempDir,
    missing_assets: bool,
) -> (Value, String) {
    let mut state = seeded_state(temporary);
    if missing_assets {
        let resources = temporary.path().join("resources");
        fs::create_dir_all(resources.join("embed_assets")).unwrap();
        fs::write(resources.join("embed_assets/config.json"), b"{}").unwrap();
        state.graph = crate::knowledge::GraphSupervisor::with_inference(
            state.store.clone(),
            temporary.path().to_path_buf(),
            Some(resources),
            state.maintenance.clone(),
            None,
        );
    }
    let source = retain_publisher_source(&state, b"Ada described the engine.");
    let world = seal_world(&state, &source, &["Ada described the engine"]);
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));
    let result = executor
        .world_search(
            &policy(true, false),
            json!({"query":"described","scope":"world"}),
        )
        .await
        .unwrap();
    (result, world.record_id)
}

#[tokio::test]
async fn world_search_fuses_indexed_hits_and_keeps_lexical_results_when_assets_fail() {
    let temporary = tempfile::tempdir().unwrap();
    let (indexed, world) = indexed_world_search(&temporary, false).await;
    let hits = indexed["hits"].as_array().unwrap();
    let graph = hits
        .iter()
        .find(|hit| hit["recordId"] == world && hit["indexed"] == true)
        .unwrap_or_else(|| panic!("indexed search missing: {indexed}"));
    assert_eq!(graph["mode"], "hybrid");
    assert!(hits
        .iter()
        .any(|hit| hit["recordId"] == world && hit["indexed"] != true));
    let temporary = tempfile::tempdir().unwrap();
    let (failed, world) = indexed_world_search(&temporary, true).await;
    assert!(failed["hits"]
        .as_array()
        .unwrap()
        .iter()
        .any(|hit| hit["recordId"] == world));
    assert_eq!(failed["partial"], true);
    assert!(!failed["warnings"].as_array().unwrap().is_empty());
    assert!(!failed.to_string().contains("resources/embed_assets"));
}

#[test]
fn graph_retrieval_fuses_shared_claims_once_and_keeps_lexical_hits() {
    let mut hits = vec![
        json!({"scope":"world", "recordId":"rec_w", "objectId":"clm_shared", "score":9}),
        json!({"scope":"world", "recordId":"rec_w", "objectId":"clm_lexical", "score":4}),
        json!({"scope":"source", "recordId":"rec_s", "objectId":"spn_1", "score":3}),
    ];
    super::super::memory_reads::fuse_ranked_world_hits(
        &mut hits,
        "rec_w",
        "rev_1",
        json!("World"),
        "hybrid",
        json!({
            "view": {"edges": [
                {"adapter_id":"clm_graph", "proposition":"Graph-only match"},
                {"adapter_id":"clm_shared", "proposition":"Matched by both indexes"}
            ]},
            "usage": {"llm_calls":0, "embedding_calls":1, "certainty":"accounted"}
        }),
    );
    let lexical: Vec<_> = hits
        .iter()
        .filter_map(|hit| hit.get("objectId").and_then(Value::as_str))
        .collect();
    assert_eq!(lexical, ["clm_lexical", "spn_1"]);
    let ranked = hits.iter().find(|hit| hit["indexed"] == true).unwrap();
    let view = ranked["view"].get("view").unwrap_or(&ranked["view"]);
    assert_eq!(view["edges"][0]["adapter_id"], "clm_shared");
    assert!(ranked["score"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn world_search_hit_opens_the_matched_claim_beyond_the_first_page() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let lines: Vec<String> = (0..40)
        .map(|index| {
            if index == 39 {
                "QuasarMetric was corrected to 120 million USD.".to_string()
            } else {
                format!("Background observation {index} concerns the engine.")
            }
        })
        .collect();
    let source = retain_publisher_source(&state, lines.join("\n").as_bytes());
    let propositions: Vec<&str> = lines.iter().map(String::as_str).collect();
    let world = seal_world(&state, &source, &propositions);
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));
    let policy = policy(true, false);
    let edge_ids = |read: &Value| -> Vec<String> {
        let text = read["text"].as_str().unwrap();
        let view: Value = serde_json::from_str(
            crate::custom_connections::taint::delimited_body(text).unwrap_or(text),
        )
        .unwrap();
        view["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|edge| edge["claim"]["id"].as_str().unwrap().to_owned())
            .collect()
    };
    let browse = executor
        .world_read(
            &policy,
            json!({"scope":"world", "recordId":world.record_id, "revisionId":world.revision_id}),
            "browse",
        )
        .await
        .unwrap();
    assert!(!edge_ids(&browse).contains(&"claim-039".to_owned()));
    let found = executor
        .world_search(&policy, json!({"query":"QuasarMetric", "scope":"world"}))
        .await
        .unwrap();
    // A lexical hit names the claim; a fused World-index hit centers its read on it.
    let hit = found["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hit| {
            hit["objectId"] == "claim-039" || hit["readArgs"]["centerClaimId"] == "claim-039"
        })
        .expect("the target claim is discoverable");
    let read = executor
        .world_read(&policy, hit["readArgs"].clone(), "matched")
        .await
        .unwrap();
    assert!(edge_ids(&read).contains(&"claim-039".to_owned()));
}

#[tokio::test]
async fn a_journal_saved_in_one_turn_is_reviewed_once_through_acknowledged_evidence() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    seed_loupist(&state, &temporary, "loupist-b");
    let turn_capture = || {
        Arc::new(ToolCapture {
            principle_user_message_id: Some("user-message".into()),
            bound_chat_id: Some("chat-a".into()),
            ..ToolCapture::default()
        })
    };
    let turn = policy(true, true);
    let save_source = |executor: AppToolExecutor, name: &'static str, published: &'static str| async move {
        let result_ref = captured_file(&executor, name, "On 1970-01-02 the metric was 12").await;
        let saved = executor
            .stage_source_save(
                &policy(true, true),
                json!({"result_ref": result_ref, "title": name, "coverage": "included_full",
                    "publicationTime": {"precision": "date", "value": published}}),
                name,
            )
            .await
            .unwrap();
        executor.capture.commit_delivery(name).await;
        saved["saveRef"].clone()
    };

    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    let basis = save_source(executor.clone(), "basis.txt", "1970-01-01").await;
    executor
        .execute(
            &turn,
            ToolMethod::JournalSave,
            json!({"judgment": "Private packaging conclusion", "sourceReads": [basis],
                "check": {"claim": "Metric will exceed the threshold", "metric": "metric",
                    "direction": "above", "threshold": 10, "byDate": "1970-01-02", "confidence": 70}}),
        )
        .await
        .unwrap();
    persist_turn(&state, "chat-a", "turn-save", &capture).await;
    let found =
        crate::commands::knowledge::host_search(&state, "loupist-a", "packaging", "research")
            .unwrap();
    let journal_id = found
        .as_array()
        .unwrap()
        .iter()
        .find_map(|hit| hit["journalId"].as_str())
        .unwrap()
        .to_owned();
    assert!(crate::commands::knowledge::host_prepare_journal_read(
        &state,
        "loupist-b",
        &journal_id
    )
    .is_err());

    let review = |executor: AppToolExecutor,
                  read_ref: Value,
                  observed_at: String,
                  sources: Value| async move {
        executor
            .execute(
                &policy(true, true),
                ToolMethod::JournalReview,
                json!({"readRef": read_ref, "outcome": {"observed": 12, "observedAt": observed_at},
                    "sourceReads": sources}),
            )
            .await
    };
    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    let read = executor
        .execute(
            &turn,
            ToolMethod::ConversationMemory,
            json!({"action": "read", "journalId": journal_id}),
        )
        .await
        .unwrap();
    let cited = capture
        .memories
        .lock()
        .await
        .values()
        .next()
        .cloned()
        .unwrap();
    assert_eq!(cited.kind, "journal");
    assert_eq!(cited.receipt_id.as_deref(), read["readRef"].as_str());
    let future = (chrono::Utc::now().date_naive() + chrono::Days::new(2)).to_string();
    for (read_ref, observed_at) in [
        (json!("rdg_previous_turn"), "1970-01-02".to_owned()),
        (read["readRef"].clone(), future),
    ] {
        assert!(review(executor.clone(), read_ref, observed_at, json!([]))
            .await
            .is_err());
    }
    assert!(capture.staged_research.lock().await.is_none());
    let observation = save_source(executor.clone(), "observation.txt", "1970-01-02").await;
    review(
        executor.clone(),
        read["readRef"].clone(),
        "1970-01-02".into(),
        json!([observation]),
    )
    .await
    .unwrap();
    persist_turn(&state, "chat-a", "turn-review", &capture).await;
    let reviewed = knowledge(&state)
        .get_journal("loupist-a", &journal_id)
        .unwrap();
    assert_eq!(reviewed.outcome.as_ref().unwrap().result, "came_true");
    assert_eq!(reviewed.inputs.earlier[0].delivered_text, read["text"]);

    // A reviewed entry cannot be reviewed again in a later turn.
    let executor = executor_for(&state, turn_capture());
    let again = executor
        .execute(
            &turn,
            ToolMethod::ConversationMemory,
            json!({"action": "read", "journalId": journal_id}),
        )
        .await
        .unwrap();
    let observation = save_source(executor.clone(), "again.txt", "1970-01-02").await;
    assert!(review(
        executor,
        again["readRef"].clone(),
        "1970-01-02".into(),
        json!([observation])
    )
    .await
    .is_err());
}

#[tokio::test]
async fn memory_writes_that_look_like_secrets_are_refused_and_recorded_once() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let capture = Arc::new(ToolCapture::default());
    let executor = executor_for(&state, capture.clone());
    let policy = policy(true, true);
    let secret = json!({"workingNote": {"title": "Broker login", "markdown": "api_key=sk-live-abcdefghijklmnopqrstuvwx",
        "changeSummary": "Kept the key", "expectedResearchId": null}});
    for _ in 0..2 {
        assert!(executor
            .execute(&policy, ToolMethod::NoteSave, secret.clone())
            .await
            .is_err());
    }
    assert!(capture.staged_research.lock().await.is_none());
    let refusals = capture.tool_refusals.lock().unwrap().clone();
    assert_eq!(refusals.len(), 1);
    assert_eq!(
        refusals[0].reason,
        crate::domain::ChatToolRefusalReason::SensitiveMemory
    );
    // Ordinary research text still saves.
    let plain = json!({"workingNote": {"title": "Savings plan", "markdown": "Assumed monthly contribution: 500.",
        "changeSummary": "Created the plan", "expectedResearchId": null}});
    executor
        .execute(&policy, ToolMethod::NoteSave, plain)
        .await
        .unwrap();
}

#[tokio::test]
async fn working_note_save_requires_delivery_memory_authority_and_current_revision() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let capture = Arc::new(ToolCapture::default());
    let executor = executor_for(&state, capture.clone());
    let policy = policy(true, true);
    let request = json!({"workingNote": {"title": "Savings plan", "markdown": "Assumed monthly contribution: 500.",
        "changeSummary": "Created the plan", "expectedResearchId": null}});
    executor
        .execute_for_delivery(
            &policy,
            ToolMethod::NoteSave,
            request.clone(),
            "undelivered",
        )
        .await
        .unwrap();
    executor.discard_delivery(&policy, "undelivered").await;
    assert!(capture.staged_research.lock().await.is_none());
    let mut stale = request.clone();
    stale["workingNote"]["expectedResearchId"] = json!(format!("rsh_{:032x}", 99));
    for (policy, request) in [
        (
            ToolPolicy {
                propose_memory_updates: false,
                ..policy.clone()
            },
            request.clone(),
        ),
        (policy.clone(), stale),
    ] {
        assert!(executor
            .execute(&policy, ToolMethod::NoteSave, request)
            .await
            .is_err());
    }
    executor
        .execute(&policy, ToolMethod::NoteSave, request)
        .await
        .unwrap();
    persist_turn(&state, "chat-a", "working-turn", &capture).await;
    let note = crate::commands::knowledge::current_working_context(&state, "loupist-a", "chat-a")
        .unwrap()
        .unwrap();
    let read = json!({"action": "read", "noteId": note.research_id});
    executor
        .execute(&policy, ToolMethod::ConversationMemory, read.clone())
        .await
        .unwrap();
    capture.staged_reads.lock().await.clear();
    capture.memories.lock().await.clear();
    for denied in [
        ToolPolicy {
            use_memory: false,
            ..policy.clone()
        },
        ToolPolicy {
            loupist_id: "loupist-b".into(),
            ..policy.clone()
        },
    ] {
        assert!(executor
            .execute(&denied, ToolMethod::ConversationMemory, read.clone())
            .await
            .is_err());
    }
    assert!(capture.staged_reads.lock().await.is_empty());
    assert!(capture.memories.lock().await.is_empty());

    // Replacing this Chat's note must name its current revision, not supersede it.
    *capture.staged_research.lock().await = None;
    let mut correction = json!({"workingNote": {"title": "Corrected plan", "markdown": "Contribution is 600.",
        "changeSummary": "User correction", "expectedResearchId": null}});
    assert!(executor
        .execute(&policy, ToolMethod::NoteSave, correction.clone())
        .await
        .is_err());
    correction["workingNote"]["expectedResearchId"] = json!(note.research_id);
    executor
        .execute(&policy, ToolMethod::NoteSave, correction)
        .await
        .unwrap();
    persist_turn(&state, "chat-a", "working-correction-turn", &capture).await;
    let updated =
        crate::commands::knowledge::current_working_context(&state, "loupist-a", "chat-a")
            .unwrap()
            .unwrap();
    assert_eq!(updated.markdown, "Contribution is 600.");
}

#[tokio::test]
async fn a_later_chat_supersedes_a_note_and_the_earlier_note_points_to_it() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    state
        .store
        .create_chat(&chat("chat-b", "loupist-a", 1))
        .unwrap();
    let mut note_ids = Vec::new();
    for (chat_id, markdown) in [
        ("chat-a", "Monthly contribution: 500."),
        ("chat-b", "Monthly contribution: 750."),
    ] {
        let capture = Arc::new(ToolCapture::default());
        let executor = executor_for(&state, capture.clone());
        let policy = ToolPolicy {
            session_id: chat_id.into(),
            ..policy(true, true)
        };
        let mut request = json!({"workingNote": {"title": "Savings plan", "markdown": markdown,
            "changeSummary": "Corrected monthly contribution", "expectedResearchId": null}});
        if let Some(previous) = note_ids.last() {
            executor
                .execute(
                    &policy,
                    ToolMethod::ConversationMemory,
                    json!({"action": "read", "noteId": previous}),
                )
                .await
                .unwrap();
            // Another Chat's note is superseded, never edited in place.
            request["workingNote"]["expectedResearchId"] = json!(previous);
            assert!(executor
                .execute(&policy, ToolMethod::NoteSave, request.clone())
                .await
                .is_err());
            request["workingNote"]["expectedResearchId"] = json!(null);
            request["previousNoteIds"] = json!([previous]);
        }
        executor
            .execute(&policy, ToolMethod::NoteSave, request)
            .await
            .unwrap();
        persist_turn(&state, chat_id, &format!("turn-{chat_id}"), &capture).await;
        let current =
            crate::commands::knowledge::current_working_context(&state, "loupist-a", chat_id)
                .unwrap()
                .unwrap();
        note_ids.push(current.research_id);
    }
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));
    let policy = policy(true, false);
    let found = executor
        .execute(
            &policy,
            ToolMethod::ConversationMemory,
            json!({"action":"search", "query":"Savings plan"}),
        )
        .await
        .unwrap();
    assert_eq!(found["notes"][0]["researchId"], note_ids[1]);
    let old = executor
        .execute(
            &policy,
            ToolMethod::ConversationMemory,
            json!({"action":"read", "noteId": note_ids[0]}),
        )
        .await
        .unwrap();
    assert_eq!(old["note"]["markdown"], "Monthly contribution: 500.");
    assert_eq!(old["note"]["laterNotes"][0]["noteId"], note_ids[1]);
    assert_eq!(old["note"]["laterNotes"][0]["chatId"], "chat-b");
}
