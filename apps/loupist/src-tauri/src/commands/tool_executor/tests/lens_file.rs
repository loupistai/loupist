//! `lens_file`: handles resolve against this turn's acknowledged reads and
//! saves; pages are written only when the reply commits.
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

/// Reads the World and acknowledges the delivery; returns the readRef and
/// the first claim id the view carried.
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

/// Commits the staged turn as the chat runtime does: Sources first, then filings.
async fn commit_turn(
    state: &AppState,
    capture: &ToolCapture,
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
        "chat-a",
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
                chat_id: "chat-a".into(),
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
        crate::chat::handoff::learned_digest(&state, "loupist-a", &["assistant-1".into()]).unwrap();
    assert_eq!(learned["messageId"], "assistant-1");
    assert_eq!(learned["pages"][0]["recordId"], json!(change.record_id));
    assert_eq!(learned["pages"][0]["created"], true);
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
    // The turn stopped: its delivery is dropped and no commit runs.
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
    // A World readRef is not a Source handle: the field names the scope it needs.
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
                "ops": [{"op": "put", "key": "beliefs", "heading": "Beliefs", "markdown": "Base, with liquidity first.",
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
    // A page admitted only as turn context has not been exact-read by the model.
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

    // At commit the same-turn save resolves to the Source the commit retained.
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
                    "ops": [{"op": "put", "key": "use-when", "heading": "Use when", "markdown": "Valuing fictional Hanbit companies."}]
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
    // The same call in an interactive turn cites the user's message.
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
                    "ops": [{"op": "put", "key": "steps", "heading": "Steps", "markdown": "Compare cash to the covenant floor."}]
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
    let card = crate::chat::handoff::message(&state, &handoff, None).unwrap();
    let learned = card
        .handoff
        .unwrap()
        .learned
        .expect("the card carries learned");
    assert_eq!(learned["messageId"], "final-1");
    assert_eq!(learned["pages"][0]["recordId"], json!(record_id));
    assert_eq!(learned["pages"][0]["title"], "Covenant headroom check");
    assert_eq!(learned["pages"][0]["created"], true);
    assert_eq!(learned["pages"][0]["outcomes"][0]["outcome"], "added");

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
        delivery.learned.unwrap()["pages"][0]["recordId"],
        json!(record_id)
    );
    // A message that filed nothing carries no digest.
    let nothing = crate::chat::handoff::learned_digest(&state, "loupist-a", &["final-9".into()]);
    assert!(nothing.is_none());
}
