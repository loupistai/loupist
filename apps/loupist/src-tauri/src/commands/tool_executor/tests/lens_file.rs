use super::knowledge_seam::{retain_publisher_source, seal_world};
use super::*;
use crate::knowledge::lens_page::{BasisRef, FilingOrigin};
use crate::knowledge::KnowledgeStore;

fn knowledge(state: &AppState) -> KnowledgeStore {
    KnowledgeStore::attach(
        state.store.clone(),
        crate::commands::knowledge::knowledge_blobs(state).unwrap(),
    )
}

fn turn_capture() -> Arc<ToolCapture> {
    Arc::new(ToolCapture {
        principle_user_message_id: Some("user-1".into()),
        bound_chat_id: Some("chat-a".into()),
        ..ToolCapture::default()
    })
}

async fn acknowledged_world_read(executor: &AppToolExecutor, world: &Value) -> (String, String) {
    let read = executor
        .world_read(
            &policy(true, true),
            json!({"scope":"world", "recordId": world["recordId"], "revisionId": world["revisionId"]}),
            "world-read",
        )
        .await
        .unwrap();
    executor.capture.commit_delivery("world-read").await;
    let view: Value = serde_json::from_str(read["text"].as_str().unwrap()).unwrap();
    let claim = view["edges"][0]["claim"]["id"].as_str().unwrap().to_owned();
    (read["readRef"].as_str().unwrap().to_owned(), claim)
}

fn subject_fact(basis: Value) -> Value {
    json!({
        "pages": [{
            "page": {"create": {"kind": "subject", "title": "Analytical Engine", "about": [{"name": "Analytical Engine"}]}},
            "ops": [{"op": "put", "key": "facts", "heading": "Facts", "markdown": "Ada described the engine.", "basis": basis}]
        }],
        "rationale": "The source states who described the engine."
    })
}

async fn commit_turn(
    state: &AppState,
    capture: &ToolCapture,
    message_id: &str,
) -> Vec<crate::domain::MemoryUpdateChange> {
    commit_turn_in(state, capture, "chat-a", message_id).await
}

async fn commit_turn_in(
    state: &AppState,
    capture: &ToolCapture,
    chat_id: &str,
    message_id: &str,
) -> Vec<crate::domain::MemoryUpdateChange> {
    let filings = capture.staged_filings.lock().await.clone();
    let sources = capture.staged_sources.lock().await.clone();
    let reads = capture.staged_reads.lock().await.clone();
    let blobs = crate::commands::knowledge::knowledge_blobs(state).unwrap();
    let mut connection = state.store.lock().unwrap();
    let tx = connection.transaction().unwrap();
    let saved = crate::commands::knowledge::persist_saved_turn_sources(
        &tx,
        &blobs,
        "loupist-a",
        chat_id,
        message_id,
        now_ms(),
        &[],
        true,
        &sources,
        &reads,
        None,
    )
    .unwrap();
    let mut changes = saved.changes;
    changes.extend(
        commit_filings(
            &tx,
            &blobs,
            "loupist-a",
            &FilingOrigin::Turn {
                chat_id: chat_id.into(),
                message_id: message_id.into(),
                user_message_id: Some("user-1".into()),
                unattended: false,
            },
            now_ms(),
            &filings,
            &saved.retained,
        )
        .unwrap(),
    );
    tx.commit().unwrap();
    changes
}

fn log_rows(state: &AppState) -> Vec<(String, String, Option<String>)> {
    let connection = state.store.lock().unwrap();
    let mut query = connection
        .prepare("SELECT outcome, origin, message_id FROM knowledge_lens_log ORDER BY rowid")
        .unwrap();
    query
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn field_of(error: BrokerError) -> String {
    match error {
        BrokerError::InvalidArguments { field, .. } => field,
        other => panic!("expected a field refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_world_fact_files_a_section_with_a_claim_basis_and_a_log_row_at_commit() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let source = retain_publisher_source(&state, b"Ada described the engine.");
    let world = seal_world(&state, &source, &["Ada described the engine"]);
    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    let world = json!({"recordId": world.record_id, "revisionId": world.revision_id});
    let (read_ref, claim_id) = acknowledged_world_read(&executor, &world).await;
    let staged = executor
        .stage_lens_file(
            &policy(true, true),
            subject_fact(json!({"refs": [{"world": read_ref, "claimId": claim_id}]})),
            "file",
        )
        .await
        .unwrap();
    assert_eq!(staged["status"], "staged");
    assert_eq!(staged["durable"], false);
    assert_eq!(staged["pages"][0]["title"], "Analytical Engine");
    assert_eq!(staged["pages"][0]["kind"], "subject");
    assert!(capture.staged_filings.lock().await.is_empty());
    executor.capture.commit_delivery("file").await;
    assert_eq!(capture.staged_filings.lock().await.len(), 1);
    assert!(
        log_rows(&state).is_empty(),
        "nothing lands before the reply commits"
    );

    let changes = commit_turn(&state, &capture, "assistant-1").await;
    assert_eq!(changes.len(), 1);
    let change = &changes[0];
    assert_eq!(
        (change.kind.as_str(), change.operation.as_str()),
        ("Lens", "create")
    );
    assert_eq!(change.title, "Analytical Engine");
    assert_eq!(change.outcomes.len(), 1);
    assert_eq!(change.outcomes[0].outcome, "added");
    assert_eq!(change.outcomes[0].key.as_deref(), Some("facts"));
    assert!(!change.outcomes[0].op_id.is_empty());
    let page = knowledge(&state)
        .read(|read| read.read_page("loupist-a", &change.record_id))
        .unwrap();
    assert_eq!(
        page.body.sections()[0].basis.refs,
        vec![BasisRef::Claim { claim_id }]
    );
    assert_eq!(
        log_rows(&state),
        vec![("added".into(), "turn".into(), Some("assistant-1".into()))]
    );
    let learned =
        crate::chat::handoff::learned_digest(&state, "loupist-a", &["assistant-1".into()])
            .unwrap()
            .unwrap();
    assert_eq!(learned.message_id, "assistant-1");
    assert_eq!(learned.pages[0].record_id, change.record_id);
    assert!(learned.pages[0].created);
}

#[tokio::test]
async fn a_turn_stopped_before_completion_files_nothing() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let source = retain_publisher_source(&state, b"Ada described the engine.");
    let world = seal_world(&state, &source, &["Ada described the engine"]);
    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    let world = json!({"recordId": world.record_id, "revisionId": world.revision_id});
    let (read_ref, claim_id) = acknowledged_world_read(&executor, &world).await;
    executor
        .stage_lens_file(
            &policy(true, true),
            subject_fact(json!({"refs": [{"world": read_ref, "claimId": claim_id}]})),
            "file",
        )
        .await
        .unwrap();
    executor.capture.discard_delivery("file").await;
    assert!(capture.staged_filings.lock().await.is_empty());
    assert!(log_rows(&state).is_empty());
    let pages = knowledge(&state)
        .read(|read| read.lens_index("loupist-a"))
        .unwrap();
    assert!(pages.iter().all(|page| page.title != "Analytical Engine"));
}

#[tokio::test]
async fn unknown_or_foreign_handles_refuse_by_field_without_echoing_them() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let source = retain_publisher_source(&state, b"Ada described the engine.");
    let world = seal_world(&state, &source, &["Ada described the engine"]);
    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    let world = json!({"recordId": world.record_id, "revisionId": world.revision_id});
    let (read_ref, claim_id) = acknowledged_world_read(&executor, &world).await;
    let policy = policy(true, true);
    let refusal = |basis: Value| {
        let executor = &executor;
        let policy = &policy;
        async move {
            let error = executor
                .stage_lens_file(policy, subject_fact(basis), "file")
                .await
                .unwrap_err();
            let text = error.to_string();
            assert!(!text.contains("rdg_forged_model_text"), "{text}");
            field_of(error)
        }
    };
    assert_eq!(
        refusal(json!({"refs": [{"world": "rdg_forged_model_text", "claimId": claim_id}]})).await,
        "pages[0].ops[0].basis.refs[0].world"
    );
    assert_eq!(
        refusal(json!({"refs": [{"world": read_ref, "claimId": "claim-999"}]})).await,
        "pages[0].ops[0].basis.refs[0].claimId"
    );
    assert_eq!(
        refusal(json!({"refs": [{"source": "rdg_forged_model_text"}]})).await,
        "pages[0].ops[0].basis.refs[0].source"
    );
    assert_eq!(
        refusal(json!({"refs": [{"lens": "rdg_forged_model_text"}]})).await,
        "pages[0].ops[0].basis.refs[0].lens"
    );
    assert_eq!(
        refusal(json!({"refs": [{"source": read_ref}]})).await,
        "pages[0].ops[0].basis.refs[0].source"
    );
    assert!(capture.staged_filings.lock().await.is_empty());
}

#[tokio::test]
async fn lens_file_requires_an_acknowledged_exact_read_and_saved_sources() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    let policy = policy(true, true);
    let lens = crate::commands::knowledge::ensure_perspective(&state, "loupist-a", "Base")
        .unwrap()
        .unwrap();
    let revise = |expected: Option<&str>, basis: Value| {
        json!({
            "pages": [{
                "page": {"recordId": lens},
                "ops": [{"op": "put", "key": "focus", "heading": "Focus", "markdown": "Base, with liquidity first.",
                    "basis": basis, "expected": expected}]
            }],
            "rationale": "Refined the standing view."
        })
    };
    let unread = executor
        .stage_lens_file(&policy, revise(None, json!({})), "unread")
        .await
        .unwrap_err();
    assert_eq!(field_of(unread), "pages[0].page.recordId");
    let read = executor
        .lens_read(&policy, json!({"recordId": lens}), "lens-read")
        .await
        .unwrap();
    let delivered: Value = serde_json::from_str(read["text"].as_str().unwrap()).unwrap();
    let digest = delivered["body"]["sections"][0]["digest"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(executor
        .stage_lens_file(&policy, revise(Some(&digest), json!({})), "unacknowledged")
        .await
        .is_err());
    executor.capture.commit_delivery("lens-read").await;
    let acknowledged = std::mem::take(&mut *capture.staged_reads.lock().await);
    *capture.context_reads.lock().await = acknowledged.clone();
    let context_only = executor
        .stage_lens_file(&policy, revise(Some(&digest), json!({})), "context-only")
        .await
        .unwrap_err();
    assert!(
        context_only.to_string().contains("lens_read"),
        "{context_only}"
    );
    *capture.staged_reads.lock().await = acknowledged;
    capture.context_reads.lock().await.clear();

    let result_ref = captured_file(&executor, "source.txt", "Exact acquired source").await;
    for unsaved in ["spn_forged_owned_id", result_ref.as_str()] {
        let error = executor
            .stage_lens_file(
                &policy,
                revise(Some(&digest), json!({"refs": [{"source": unsaved}]})),
                "unsaved-source",
            )
            .await
            .unwrap_err();
        assert!(!error.to_string().contains("spn_forged_owned_id"));
        assert_eq!(field_of(error), "pages[0].ops[0].basis.refs[0].source");
    }
    let saved = executor
        .stage_source_save(
            &policy,
            json!({"result_ref": result_ref, "title": "Source", "coverage": "included_full"}),
            "source-save",
        )
        .await
        .unwrap();
    let save_ref = saved["saveRef"].as_str().unwrap().to_owned();
    assert!(executor
        .stage_lens_file(
            &policy,
            revise(Some(&digest), json!({"refs": [{"source": save_ref}]})),
            "source-unacknowledged"
        )
        .await
        .is_err());
    executor.capture.commit_delivery("source-save").await;
    executor
        .stage_lens_file(
            &policy,
            revise(Some(&digest), json!({"refs": [{"source": save_ref}]})),
            "valid",
        )
        .await
        .unwrap();
    assert!(capture.staged_filings.lock().await.is_empty());
    executor.capture.commit_delivery("valid").await;
    assert_eq!(capture.staged_filings.lock().await.len(), 1);

    let changes = commit_turn(&state, &capture, "assistant-2").await;
    let change = changes
        .iter()
        .find(|change| change.kind == "Lens")
        .expect("the revised page is reported");
    assert_eq!(change.operation, "revise");
    assert_eq!(change.outcomes[0].outcome, "replaced");
    let page = knowledge(&state)
        .read(|read| read.read_page("loupist-a", &lens))
        .unwrap();
    let retained = changes
        .iter()
        .find(|change| change.kind == "Source")
        .and_then(|change| change.knowledge.clone())
        .expect("the Source was retained in the same commit");
    assert_eq!(
        page.body.sections()[0].basis.refs,
        vec![BasisRef::Span {
            record_id: retained.record_id,
            revision_digest: page.body.sections()[0]
                .basis
                .refs
                .first()
                .and_then(|reference| match reference {
                    BasisRef::Span {
                        revision_digest, ..
                    } => Some(revision_digest.clone()),
                    _ => None,
                })
                .unwrap(),
            span_id: retained.span_id.unwrap(),
        }]
    );
    assert_eq!(
        page.body.sections()[0].markdown,
        "Base, with liquidity first."
    );
}

#[tokio::test]
async fn a_create_whose_title_exists_merges_into_that_page() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let existing = knowledge(&state)
        .file_method(
            "loupist-a",
            "Hanbit diluted-share rule",
            "Divide by fully diluted shares.",
            &crate::knowledge::test_support::turn_origin("chat-0", "assistant-0"),
            1,
        )
        .unwrap();
    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    executor
        .stage_lens_file(
            &policy(true, true),
            json!({
                "pages": [{
                    "page": {"create": {"kind": "method", "title": "hanbit diluted-share RULE"}},
                    "ops": [{"op": "put", "basis": {"refs": [{"said": true}]}, "key": "use-when", "heading": "Use when", "markdown": "Valuing fictional Hanbit companies."}]
                }],
                "rationale": "The user named when the rule applies."
            }),
            "file",
        )
        .await
        .unwrap();
    executor.capture.commit_delivery("file").await;
    let changes = commit_turn(&state, &capture, "assistant-3").await;
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].record_id, existing.record_id);
    assert_eq!(changes[0].operation, "revise");
    let pages = knowledge(&state)
        .read(|read| read.lens_index("loupist-a"))
        .unwrap();
    assert_eq!(
        pages
            .iter()
            .filter(|page| page.title == "Hanbit diluted-share rule")
            .count(),
        1
    );
    let page = knowledge(&state)
        .read(|read| read.read_page("loupist-a", &existing.record_id))
        .unwrap();
    let keys: Vec<&str> = page
        .body
        .sections()
        .iter()
        .map(|section| section.key.as_str())
        .collect();
    assert_eq!(keys, vec!["steps", "use-when"]);
}

#[tokio::test]
async fn an_unattended_run_refuses_said_and_admits_lens_file() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let capture = Arc::new(ToolCapture {
        principle_user_message_id: Some("user-1".into()),
        unattended_run_id: Some("run-1".into()),
        ..ToolCapture::default()
    });
    let executor = executor_for(&state, capture.clone());
    let error = executor
        .stage_lens_file(
            &policy(true, true),
            subject_fact(json!({"refs": [{"said": true}]})),
            "file",
        )
        .await
        .unwrap_err();
    assert_eq!(field_of(error), "pages[0].ops[0].basis.refs[0].said");
    assert!(crate::briefings::policy::allowed_name("lens_file"));
    assert!(crate::briefings::policy::allowed_method(
        ToolMethod::LensFile
    ));
    let interactive = executor_for(&state, turn_capture());
    let staged = interactive
        .stage_lens_file(
            &policy(true, true),
            subject_fact(json!({"refs": [{"said": true}]})),
            "file",
        )
        .await
        .unwrap();
    assert_eq!(staged["status"], "staged");
}

#[tokio::test]
async fn credential_like_text_is_refused_as_sensitive_memory() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    let secret = json!({
        "pages": [{
            "page": {"create": {"kind": "method", "title": "Broker login"}},
            "ops": [{"op": "put", "key": "steps", "heading": "Steps", "markdown": "api_key=sk-live-abcdefghijklmnopqrstuvwx"}]
        }],
        "rationale": "Kept the key."
    });
    let error = executor
        .execute(&policy(true, true), ToolMethod::LensFile, secret)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("credential"), "{error}");
    let refusals = capture.tool_refusals.lock().unwrap().clone();
    assert_eq!(refusals.len(), 1);
    assert_eq!(refusals[0].tool_name, "lens_file");
    assert_eq!(
        refusals[0].reason,
        crate::domain::ChatToolRefusalReason::SensitiveMemory
    );
    assert!(capture.staged_filings.lock().await.is_empty());
}

#[tokio::test]
async fn a_handoff_card_and_a_scheduled_delivery_carry_what_the_run_learned() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    executor
        .stage_lens_file(
            &policy(true, true),
            json!({
                "pages": [{
                    "page": {"create": {"kind": "method", "title": "Covenant headroom check"}},
                    "ops": [{"op": "put", "basis": {"refs": [{"said": true}]}, "key": "steps", "heading": "Steps", "markdown": "Compare cash to the covenant floor."}]
                }],
                "rationale": "A reusable check from this session."
            }),
            "file",
        )
        .await
        .unwrap();
    executor.capture.commit_delivery("file").await;
    let changes = commit_turn(&state, &capture, "final-1").await;
    let record_id = changes[0].record_id.clone();

    let handoff = crate::chat::handoff::Handoff {
        id: "handoff-1".into(),
        loupist_id: "loupist-a".into(),
        request_key: "covenants".into(),
        chat_id: "chat-session".into(),
        title: "Covenant headroom".into(),
        prompt: "Check covenant headroom.".into(),
        run: crate::chat::handoff::HandoffRun {
            model_profile_id: "fixture".into(),
            thinking_level: "medium".into(),
            run_options: Default::default(),
            use_memory: true,
            update_memory: true,
        },
        status: crate::chat::handoff::HandoffStatus::Done,
        run_id: Some("run-1".into()),
        message_id: Some("final-1".into()),
        error: None,
        created_at_ms: 1,
        finished_at_ms: Some(2),
    };
    let card = crate::chat::handoff::message(&state, &handoff, None)
        .unwrap()
        .handoff
        .unwrap();
    let learned = card.learned.clone().expect("the card carries learned");
    assert_eq!(learned.message_id, "final-1");
    let wire = json!({"messageId": "final-1", "pages": [{"recordId": record_id,
        "title": "Covenant headroom check", "kind": "method", "created": true,
        "outcomes": [{"opId": learned.pages[0].outcomes[0].op_id, "outcome": "added", "key": "steps"}]}]});
    assert_eq!(serde_json::to_value(&card).unwrap()["learned"], wire);

    let run = crate::scheduled_tasks::types::Run {
        briefing_context: None,
        kind: crate::scheduled_tasks::types::TaskKind::Briefing,
        read_at_ms: None,
        summary: "Covenants hold.".into(),
        summary_items: Vec::new(),
        message_id: Some("final-1".into()),
        skipped_tools: Vec::new(),
        id: "scheduled-run-1".into(),
        task_id: "task-1".into(),
        loupist_id: "loupist-a".into(),
        task_revision: 1,
        task_title: "Morning briefing".into(),
        thread_id: Some("chat-run".into()),
        run_id: "run-1".into(),
        scheduled_for_ms: 1,
        started_at_ms: 1,
        finished_at_ms: Some(2),
        status: crate::scheduled_tasks::types::RunStatus::Succeeded,
        error: None,
    };
    let post = crate::chat::home::delivery_message(&state, &run).unwrap();
    let delivery = post.delivery.unwrap();
    assert_eq!(
        delivery.learned.as_ref().unwrap().pages[0].record_id,
        record_id
    );
    assert_eq!(serde_json::to_value(&delivery).unwrap()["learned"], wire);
    let nothing = crate::chat::handoff::learned_digest(&state, "loupist-a", &["final-9".into()]);
    assert!(nothing.unwrap().is_none());
    assert!(
        crate::chat::handoff::learned_digest(&state, "loupist-gone", &["final-1".into()]).is_err(),
        "a failed read is not 'nothing filed'"
    );
    let unreadable = crate::chat::handoff::message(
        &state,
        &crate::chat::handoff::Handoff {
            loupist_id: "loupist-gone".into(),
            ..handoff
        },
        None,
    )
    .unwrap();
    assert!(unreadable.handoff.unwrap().learned.is_none());
}

fn said() -> Value {
    json!({"refs": [{"said": true}]})
}

#[tokio::test]
async fn text_the_commit_would_refuse_is_refused_at_staging_by_its_field() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let source = retain_publisher_source(&state, b"Ada described the engine.");
    seal_world(&state, &source, &["Ada described the engine"]);
    let subject = |about: Value, ops: Value| json!({"page": {"create": {"kind": "subject", "title": "Hanbit Battery", "about": about}}, "ops": ops});
    let method = |markdown: &str| {
        json!({"page": {"create": {"kind": "method", "title": "Graph check"}},
            "ops": [{"op": "put", "key": "steps", "heading": "Steps", "markdown": markdown}]})
    };
    let cases = [
        (
            subject(
                json!([]),
                json!([{"op": "put", "key": "facts", "heading": "Facts",
                    "markdown": "Supplies [Northgate](world:northgate-motors).", "basis": said()}]),
            ),
            "pages[0].ops[0].markdown",
        ),
        (
            subject(
                json!([{"nodeId": "hanbit-battery"}]),
                json!([{"op": "put", "key": "facts", "heading": "Facts",
                    "markdown": "Makes LFP cathode.", "basis": said()}]),
            ),
            "pages[0].page.create.about[0].nodeId",
        ),
        (
            subject(
                json!([]),
                json!([{"op": "put", "key": "facts", "heading": "Facts", "markdown": "Makes cathode.", "basis": said()},
                    {"op": "summary", "text": "Partner of [Northgate](world:northgate-motors)."}]),
            ),
            "pages[0].ops[1].text",
        ),
        (
            method("Open world://ada first."),
            "pages[0].ops[0].markdown",
        ),
        (
            method("Start at [Ada](world:ada)."),
            "pages[0].ops[0].markdown",
        ),
        (
            method("Start at [Ada][a].\n\n[a]: world:ada"),
            "pages[0].ops[0].markdown",
        ),
    ];
    for (page, field) in cases {
        let capture = turn_capture();
        let executor = executor_for(&state, capture.clone());
        let refused = executor
            .execute(
                &policy(true, true),
                ToolMethod::LensFile,
                json!({"pages": [page], "rationale": "What the user said."}),
            )
            .await
            .unwrap_err();
        assert_eq!(field_of(refused), field);
        assert!(capture.staged_filings.lock().await.is_empty());
    }
    // A link to a node the World holds stages on a subject page.
    let capture = turn_capture();
    executor_for(&state, capture.clone())
        .execute(
            &policy(true, true),
            ToolMethod::LensFile,
            json!({"pages": [subject(json!([{"nodeId": "ada"}]), json!([{"op": "put", "key": "facts",
                "heading": "Facts", "markdown": "Read by [Ada](world:ada).", "basis": said()}]))],
                "rationale": "What the user said."}),
        )
        .await
        .unwrap();
    assert_eq!(capture.staged_filings.lock().await.len(), 1);
}

#[tokio::test]
async fn an_op_the_policy_refuses_at_commit_is_logged_and_the_reply_still_commits() {
    use crate::knowledge::{GraphNode, SealWorld};
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let source = retain_publisher_source(&state, b"Ada described the engine.");
    let world = seal_world(&state, &source, &["Ada described the engine"]);
    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    executor
        .stage_lens_file(
            &policy(true, true),
            json!({"pages": [{"page": {"create": {"kind": "subject", "title": "Analytical Engine"}},
                "ops": [{"op": "put", "key": "facts", "heading": "Facts",
                        "markdown": "Ada described [the engine](world:engine).", "basis": said()},
                    {"op": "put", "key": "overview", "heading": "Overview",
                        "markdown": "A mechanical computer.", "basis": said()}]}],
                "rationale": "What the user said."}),
            "file",
        )
        .await
        .unwrap();
    executor.capture.commit_delivery("file").await;
    // The World drops the engine before the reply commits.
    knowledge(&state)
        .transact(|write| {
            write.seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: world.record_id.clone(),
                expected_revision_id: Some(world.revision_id.clone()),
                at_ms: 1_700_000_000_001,
                nodes: vec![GraphNode {
                    id: "ada".into(),
                    name: "ada".into(),
                    aliases: Vec::new(),
                    description: "The ada.".into(),
                    kind: "Person".into(),
                    source_role: "source".into(),
                    properties: Default::default(),
                }],
                claims: Vec::new(),
                annotations: Vec::new(),
                dependencies: Vec::new(),
            })
        })
        .unwrap();
    let changes = commit_turn(&state, &capture, "assistant-1").await;
    assert_eq!(changes.len(), 1);
    let outcomes: Vec<(&str, Option<&str>)> = changes[0]
        .outcomes
        .iter()
        .map(|outcome| (outcome.outcome.as_str(), outcome.key.as_deref()))
        .collect();
    assert_eq!(
        outcomes,
        [("refused_text", None), ("added", Some("overview"))]
    );
    assert_eq!(
        log_rows(&state)
            .iter()
            .map(|(outcome, ..)| outcome.as_str())
            .collect::<Vec<_>>(),
        ["refused_text", "added"]
    );
}

#[tokio::test]
async fn a_page_archived_or_removed_before_the_reply_commits_is_logged_refused_and_the_rest_files()
{
    use crate::knowledge::lens_page::{Basis, LensOp, PageDelta, PageKind, PageTarget, SectionKey};
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let method = |title: &str| PageDelta {
        target: PageTarget::Create {
            kind: PageKind::Method,
            title: title.into(),
            anchors: Vec::new(),
        },
        ops: vec![LensOp::Put {
            key: SectionKey::new("steps").unwrap(),
            heading: "Steps".into(),
            markdown: "Read the filing.".into(),
            basis: Basis::default(),
            expected: None,
        }],
    };
    let pages: Vec<String> = knowledge(&state)
        .transact(|write| {
            write.file(
                "loupist-a",
                &crate::knowledge::test_support::turn_origin("chat-a", "assistant-0"),
                vec![method("Archived check"), method("Removed check")],
                1_000,
            )
        })
        .unwrap()
        .into_iter()
        .map(|page| page.record_id)
        .collect();
    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    let policy = policy(true, true);
    for (index, record_id) in pages.iter().enumerate() {
        let delivery = format!("read-{index}");
        executor
            .lens_read(&policy, json!({"recordId": record_id}), &delivery)
            .await
            .unwrap();
        executor.capture.commit_delivery(&delivery).await;
    }
    let revise = |record_id: &str| json!({"page": {"recordId": record_id}, "ops": [{"op": "summary", "text": "Checked again.", "said": true}]});
    executor
        .stage_lens_file(
            &policy,
            json!({"pages": [revise(&pages[0]), revise(&pages[1]),
                {"page": {"create": {"kind": "method", "title": "Fresh check"}},
                    "ops": [{"op": "put", "basis": {"refs": [{"said": true}]}, "key": "steps", "heading": "Steps", "markdown": "Start here."}]}],
                "rationale": "Revised two checks and wrote a third."}),
            "file",
        )
        .await
        .unwrap();
    executor.capture.commit_delivery("file").await;
    knowledge(&state)
        .transact(|write| {
            write.set_lens_archived("loupist-a", &pages[0], true, "Hide.", 1_001, "user")
        })
        .unwrap();
    {
        let mut connection = state.store.lock().unwrap();
        let tx = connection.transaction().unwrap();
        let mut controls = crate::knowledge::memory_controls::load(&tx, "loupist-a").unwrap();
        controls.removed.insert(pages[1].clone());
        crate::knowledge::memory_controls::save(&tx, "loupist-a", &controls).unwrap();
        tx.commit().unwrap();
    }
    let changes = commit_turn(&state, &capture, "assistant-1").await;
    let outcomes: Vec<(&str, Vec<&str>)> = changes
        .iter()
        .map(|change| {
            (
                change.title.as_str(),
                change
                    .outcomes
                    .iter()
                    .map(|outcome| outcome.outcome.as_str())
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        outcomes,
        [
            ("Archived check", vec!["refused_inactive"]),
            ("Removed check", vec!["refused_inactive"]),
            ("Fresh check", vec!["added"]),
        ]
    );
    assert!(
        knowledge(&state)
            .transact(|write| write.forget_section("loupist-a", &pages[0], "steps", 1_002))
            .is_err(),
        "the user restores an archived page before editing it"
    );
}

fn method_pages(prefix: &str, pages: usize, ops: usize) -> Value {
    let pages: Vec<Value> = (0..pages)
        .map(|page| {
            json!({
                "page": {"create": {"kind": "method", "title": format!("{prefix} method {page}")}},
                "ops": (0..ops).map(|op| json!({
                    "op": "put", "basis": {"refs": [{"said": true}]}, "key": format!("s{op}"), "heading": format!("Step {op}"),
                    "markdown": format!("Check {op}.")
                })).collect::<Vec<_>>()
            })
        })
        .collect();
    json!({"pages": pages, "rationale": "Budget check."})
}

#[tokio::test]
async fn parallel_lens_file_calls_share_one_turn_budget() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let policy = policy(true, true);
    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    let first =
        executor.execute_for_delivery(&policy, ToolMethod::LensFile, method_pages("a", 8, 5), "p1");
    let second =
        executor.execute_for_delivery(&policy, ToolMethod::LensFile, method_pages("b", 1, 1), "p2");
    let (first, second) = tokio::join!(first, second);
    let refused = match (first, second) {
        (Ok(_), Err(refused)) | (Err(refused), Ok(_)) => refused,
        (first, second) => panic!("exactly one call fits: {first:?} {second:?}"),
    };
    assert_eq!(field_of(refused), "pages");
    // A call that was never delivered gives its share back.
    capture.discard_delivery("p1").await;
    capture.discard_delivery("p2").await;
    executor
        .execute_for_delivery(&policy, ToolMethod::LensFile, method_pages("c", 8, 5), "p3")
        .await
        .unwrap();
}

#[tokio::test]
async fn a_create_landing_on_a_page_the_turn_did_not_read_only_adds_sections() {
    use crate::knowledge::lens_page::{
        Basis, BasisRef, LensOp, PageDelta, PageKind, PageTarget, SectionKey,
    };
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    crate::commands::knowledge::ensure_perspective(&state, "loupist-a", "Liquidity first.")
        .unwrap()
        .unwrap();
    let idea = knowledge(&state)
        .transact(|write| {
            write.file(
                "loupist-a",
                &crate::knowledge::test_support::turn_origin("chat-a", "assistant-0"),
                vec![PageDelta {
                    target: PageTarget::Create {
                        kind: PageKind::Idea,
                        title: "Hanbit pricing power".into(),
                        anchors: Vec::new(),
                    },
                    ops: vec![
                        LensOp::Put {
                            key: SectionKey::new("claim").unwrap(),
                            heading: "Claim".into(),
                            markdown: "Hanbit can raise LFP prices.".into(),
                            basis: Basis {
                                refs: vec![BasisRef::UserSaid {
                                    message_id: "user-0".into(),
                                }],
                                uncertainty: None,
                            },
                            expected: None,
                        },
                        LensOp::Summary {
                            text: "The user's idea: Hanbit can raise prices.".into(),
                        },
                    ],
                }],
                1_000,
            )
        })
        .unwrap()[0]
        .record_id
        .clone();
    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    let policy = policy(true, true);
    let call = |page: Value| json!({"pages": [page], "rationale": "Rewrote what the turn found."});
    for page in [
        json!({"page": {"create": {"kind": "perspective", "title": "Anything at all"}},
            "ops": [{"op": "summary", "text": "Momentum over liquidity."}]}),
        json!({"page": {"create": {"kind": "idea", "title": "hanbit PRICING power"}},
            "ops": [{"op": "put", "key": "checks", "heading": "Checks", "markdown": "Watch prices.", "basis": said()},
                {"op": "idea_status", "status": "refuted", "why": "No basis."}]}),
    ] {
        let refused = executor
            .stage_lens_file(&policy, call(page), "unread")
            .await
            .unwrap_err();
        assert!(refused.to_string().contains("lens_read"), "{refused}");
        assert!(field_of(refused).ends_with(".op"));
    }
    assert!(capture.staged_filings.lock().await.is_empty());
    // After an exact read the same create revises the page.
    executor
        .lens_read(&policy, json!({"recordId": idea}), "read")
        .await
        .unwrap();
    executor.capture.commit_delivery("read").await;
    executor
        .stage_lens_file(
            &policy,
            call(
                json!({"page": {"create": {"kind": "idea", "title": "hanbit PRICING power"}},
                "ops": [{"op": "summary", "text": "Refuted by the 2026 contract."}]}),
            ),
            "read-then-file",
        )
        .await
        .unwrap();
    executor.capture.commit_delivery("read-then-file").await;
    let changes = commit_turn(&state, &capture, "assistant-1").await;
    assert_eq!(changes[0].record_id, idea);
    assert_eq!(changes[0].outcomes[0].outcome, "summarized");
}

#[tokio::test]
async fn control_characters_are_refused_before_staging() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let call = |title: &str, heading: &str, markdown: &str, summary: &str, rationale: &str| {
        json!({"pages": [{"page": {"create": {"kind": "method", "title": title}},
            "ops": [{"op": "put", "key": "steps", "heading": heading, "markdown": markdown},
                {"op": "summary", "text": summary}]}],
            "rationale": rationale})
    };
    let clean = [
        "Hanbit rule",
        "Steps",
        "Divide by shares.",
        "Summary",
        "Why.",
    ];
    for field in 0..clean.len() {
        let mut texts = clean;
        texts[field] = "Hidden\u{1b}[8m text\u{8}";
        let capture = turn_capture();
        let refused = executor_for(&state, capture.clone())
            .execute(
                &policy(true, true),
                ToolMethod::LensFile,
                call(texts[0], texts[1], texts[2], texts[3], texts[4]),
            )
            .await;
        assert!(refused.is_err(), "{field}");
        assert!(capture.staged_filings.lock().await.is_empty());
    }
}

#[tokio::test]
async fn research_filed_in_two_chats_from_two_saved_sources_queues_one_method_review() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let settings = crate::knowledge::GraphProcessingSettings {
        enrichment_enabled: true,
        helper_provider: Some("fixture".into()),
        helper_model: Some("fixture".into()),
        ..Default::default()
    };
    crate::knowledge::store_settings(&state.store.lock().unwrap(), "loupist-a", &settings).unwrap();
    let reviews = || -> Vec<String> {
        let connection = state.store.lock().unwrap();
        let mut query = connection
            .prepare("SELECT status FROM knowledge_learning_reviews")
            .unwrap();
        query
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    for (chat, company) in [("chat-a", "Hanbit Motors"), ("chat-b", "Daeil Steel")] {
        let capture = Arc::new(ToolCapture {
            principle_user_message_id: Some(format!("user-{chat}")),
            bound_chat_id: Some(chat.into()),
            ..ToolCapture::default()
        });
        let executor = executor_for(&state, capture.clone());
        let policy = policy(true, true);
        let result_ref = captured_file(
            &executor,
            &format!("{chat}.txt"),
            &format!("{company} margins fell during its capacity ramp."),
        )
        .await;
        let saved = executor
            .stage_source_save(
                &policy,
                json!({"result_ref": result_ref, "title": format!("{company} filing"), "coverage": "included_full"}),
                "source-save",
            )
            .await
            .unwrap();
        executor.capture.commit_delivery("source-save").await;
        executor
            .stage_lens_file(
                &policy,
                json!({
                    "pages": [{
                        "page": {"create": {"kind": "subject", "title": company, "about": [{"name": company}]}},
                        "ops": [{"op": "put", "key": "ramp", "heading": "Capacity ramp",
                            "markdown": "Margins fell while the new line ramped.",
                            "basis": {"refs": [{"source": saved["saveRef"]}]}}]
                    }],
                    "rationale": "The filing reports margin pressure during the ramp."
                }),
                "file",
            )
            .await
            .unwrap();
        executor.capture.commit_delivery("file").await;
        assert!(reviews().is_empty(), "one Chat alone starts no review");
        commit_turn_in(&state, &capture, chat, &format!("assistant-{chat}")).await;
    }
    assert!(reviews().is_empty(), "names the World has not confirmed");
    let node = |id: &str, name: &str, aliases: &[&str]| crate::knowledge::GraphNode {
        id: id.into(),
        name: name.into(),
        aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
        description: format!("The {name}."),
        kind: "company".into(),
        source_role: "organization".into(),
        properties: Default::default(),
    };
    // World processing publishes onto the Loupist's World record.
    let (record_id, head): (String, Option<String>) = state
        .store
        .lock()
        .unwrap()
        .query_row(
            "SELECT r.record_id, h.revision_id FROM knowledge_records r
             LEFT JOIN knowledge_record_heads h ON h.record_id = r.record_id
             WHERE r.owner_loupist_id = 'loupist-a' AND r.kind = 'world'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    knowledge(&state)
        .transact(|write| {
            write.seal_world(crate::knowledge::SealWorld {
                loupist_id: "loupist-a".into(),
                record_id,
                expected_revision_id: head,
                at_ms: 1_700_000_000_000,
                // The name a page filed under binds only as the World spells
                // it, as a name or an alias.
                nodes: vec![
                    node("hanbit", "Hanbit Motors Co., Ltd.", &["Hanbit Motors"]),
                    node("daeil", "Daeil Steel", &[]),
                ],
                claims: Vec::new(),
                annotations: Vec::new(),
                dependencies: Vec::new(),
            })
        })
        .unwrap();
    assert_eq!(reviews(), ["queued"]);
}

#[tokio::test]
async fn a_turn_writes_a_method_only_from_the_users_words() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = executor_for(&state, turn_capture());
    let method = |basis: Value| {
        json!({"pages": [{"page": {"create": {"kind": "method", "title": "Margin ramp check"}},
            "ops": [{"op": "put", "key": "steps", "heading": "Steps", "markdown": "Compare utilization with ramp cost.", "basis": basis}]}],
            "rationale": "A check from this research."})
    };
    let unasked = executor
        .stage_lens_file(&policy(true, true), method(json!({"refs": []})), "unasked")
        .await
        .unwrap_err()
        .to_string();
    assert!(
        unasked.starts_with("pages[0].ops[0].basis: a method is written only from the user's"),
        "{unasked}"
    );
    assert!(executor.capture.staged_filings.lock().await.is_empty());
    executor
        .stage_lens_file(
            &policy(true, true),
            method(json!({"refs": [{"said": true}]})),
            "asked",
        )
        .await
        .expect("the user's words admit a method");
    executor.capture.commit_delivery("asked").await;
}

#[tokio::test]
async fn retiring_or_resummarizing_a_method_needs_the_users_words() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let executor = executor_for(&state, turn_capture());
    use crate::knowledge::lens_page::{Basis, LensOp, PageDelta, PageKind, PageTarget, SectionKey};
    let record_id = knowledge(&state)
        .transact(|write| {
            write.file(
                "loupist-a",
                &crate::knowledge::test_support::turn_origin("chat-a", "assistant-0"),
                vec![PageDelta {
                    target: PageTarget::Create {
                        kind: PageKind::Method,
                        title: "Dividend cover check".into(),
                        anchors: Vec::new(),
                    },
                    ops: vec![LensOp::Put {
                        key: SectionKey::new("steps").unwrap(),
                        heading: "Steps".into(),
                        markdown: "Divide earnings by dividends.".into(),
                        basis: Basis::default(),
                        expected: None,
                    }],
                }],
                1_000,
            )
        })
        .unwrap()[0]
        .record_id
        .clone();
    let read = executor
        .lens_read(&policy(true, true), json!({"recordId": record_id}), "read")
        .await
        .unwrap();
    executor.capture.commit_delivery("read").await;
    let delivered: Value = serde_json::from_str(read["text"].as_str().unwrap()).unwrap();
    let digest = delivered["body"]["sections"][0]["digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let change = |op: Value| json!({"pages": [{"page": {"recordId": record_id}, "ops": [op]}], "rationale": "Edit the method."});
    for (op, field) in [
        (
            json!({"op": "retire", "key": "steps", "expected": digest, "why": "Not needed."}),
            "said",
        ),
        (
            json!({"op": "summary", "text": "Applies to every payer."}),
            "said",
        ),
    ] {
        let error = executor
            .stage_lens_file(&policy(true, true), change(op), "unasked")
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with(&format!(
                "pages[0].ops[0].{field}: a method is written only"
            )),
            "{error}"
        );
    }
    executor
        .stage_lens_file(
            &policy(true, true),
            change(json!({"op": "summary", "text": "The user's narrower scope.", "said": true})),
            "asked",
        )
        .await
        .expect("the user's words admit a summary change");
}
