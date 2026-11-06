//! Saved Memory under an as-of cutoff (finance-point-in-time.md §3.2): what
//! the Loupist learned later is left out of searches, exact reads and the
//! automatic context, and counted for the answer's note.
use super::*;
use crate::knowledge::{
    Authorship, BindingRole, ClaimEndpoint, ClaimStatus, Coverage, CreateRecord, DependencyRole,
    EvidenceBinding, EvidenceRevision, GraphClaim, GraphNode, KnowledgeStore, RecordKind,
    RepresentationRole, RetainMaterial, RetainNewSource, SealWorld, Selector, SourceClass,
    TimePoint, TimePrecision,
};

fn knowledge(state: &AppState) -> KnowledgeStore {
    KnowledgeStore::attach(
        state.store.clone(),
        crate::commands::knowledge::knowledge_blobs(state).unwrap(),
    )
}

fn day(value: &str) -> TimePoint {
    TimePoint {
        value: Some(value.into()),
        precision: TimePrecision::Date,
        approximate: false,
    }
}

fn as_of(cutoff: &str) -> ToolPolicy {
    ToolPolicy {
        as_of: Some(cutoff.into()),
        ..policy(true, false)
    }
}

/// A publisher filing retained with its publication date: (record, digest, span).
fn retain(state: &AppState, title: &str, published: &str, text: &str) -> (String, String, String) {
    let retained = knowledge(state)
        .transact(|write| {
            write.retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "loupist-a".into(),
                at_ms: 1_700_000_000_000,
                title: title.into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: format!("https://example.test/{published}"),
                provider_identity: "example-publisher".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                publication_time: Some(day(published)),
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: format!("doc-{published}"),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    bytes: text.as_bytes().to_vec(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: text.len() as u64,
                    },
                    context_note: "exact retained range".into(),
                }),
            })
        })
        .unwrap();
    (
        retained.record_id,
        retained.revision_digest,
        retained.span_id.unwrap(),
    )
}

#[tokio::test]
async fn a_cutoff_leaves_out_later_sources_and_world_facts_and_counts_them() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let early = retain(
        &state,
        "Solsup guidance filing",
        "2026-02-20",
        "Guidance: revenue near 440bn.",
    );
    let later = retain(
        &state,
        "Solsup annual filing",
        "2026-03-18",
        "Revenue was 452.3bn.",
    );
    let node = |id: &str| GraphNode {
        id: id.into(),
        name: id.into(),
        aliases: vec![format!("{id} alias")],
        description: format!("Later summary of {id}."),
        kind: "company".into(),
        source_role: "source".into(),
        properties: Default::default(),
    };
    let claim =
        |id: &str, proposition: &str, target: &str, span: &str, published: &str| GraphClaim {
            id: id.into(),
            proposition: proposition.into(),
            attribution: "example-publisher".into(),
            relation: "reported".into(),
            endpoints: vec![
                ClaimEndpoint {
                    node_id: "solsup".into(),
                    role: "source".into(),
                },
                ClaimEndpoint {
                    node_id: target.into(),
                    role: "target".into(),
                },
            ],
            qualifiers: Vec::new(),
            evidence: vec![EvidenceBinding {
                span_id: span.into(),
                role: BindingRole::ExtractedFrom,
                attribution: "example-publisher".into(),
            }],
            status: ClaimStatus::Active,
            recorded_at_ms: Some(1_700_000_000_000),
            publication_time: Some(day(published)),
            effective_time: None,
            valid_at: None,
            invalid_at: None,
            expired_at: None,
        };
    let world = knowledge(&state)
        .transact(|write| {
            let record = write.create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "Solsup".into(),
                at_ms: 1_700_000_000_000,
            })?;
            write.seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: record.record_id.clone(),
                expected_revision_id: None,
                at_ms: 1_700_000_000_000,
                nodes: vec![node("solsup"), node("guidance"), node("results")],
                claims: vec![
                    claim(
                        "claim-guidance",
                        "Solsup guided revenue near 440bn",
                        "guidance",
                        &early.2,
                        "2026-02-20",
                    ),
                    claim(
                        "claim-results",
                        "Solsup reported revenue of 452.3bn",
                        "results",
                        &later.2,
                        "2026-03-18",
                    ),
                ],
                annotations: Vec::new(),
                dependencies: [&early, &later]
                    .into_iter()
                    .map(|source| EvidenceRevision {
                        record_id: source.0.clone(),
                        revision_digest: source.1.clone(),
                        role: DependencyRole::WorldSource,
                    })
                    .collect(),
            })?;
            Ok(record.record_id)
        })
        .unwrap();
    let capture = Arc::new(ToolCapture {
        turn_as_of: Some("2026-03-10".into()),
        ..ToolCapture::default()
    });
    let executor = executor_for(&state, capture.clone());
    let cutoff = as_of("2026-03-10");

    // Search keeps the filing published by then.
    let found = executor
        .execute(
            &cutoff,
            ToolMethod::WorldSearch,
            json!({"scope": "source", "query": "Solsup filing"}),
        )
        .await
        .unwrap();
    let records: Vec<_> = found["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|hit| hit["recordId"].as_str())
        .collect();
    assert!(records.contains(&early.0.as_str()), "{found}");
    assert!(!records.contains(&later.0.as_str()), "{found}");
    // An exact read of the later filing is refused, not delivered.
    let refused = executor
        .execute(
            &cutoff,
            ToolMethod::WorldRead,
            json!({"scope": "source", "recordId": later.0, "spanId": later.2}),
        )
        .await
        .unwrap_err();
    assert!(refused.to_string().contains("as_of_later"), "{refused}");
    // A World read keeps the relationship supported by the earlier filing and
    // reduces names to name and kind.
    let read = executor
        .execute(
            &cutoff,
            ToolMethod::WorldRead,
            json!({"scope": "world", "recordId": world}),
        )
        .await
        .unwrap();
    let text = read["text"].as_str().unwrap();
    assert!(text.contains("guided revenue near 440bn"), "{text}");
    assert!(!text.contains("452.3bn"), "{text}");
    assert!(!text.contains("Later summary"), "{text}");
    assert!(!text.contains("\"results\""), "{text}");
    // Without a cutoff both stay.
    let today = executor
        .execute(
            &policy(true, false),
            ToolMethod::WorldRead,
            json!({"scope": "world", "recordId": world}),
        )
        .await
        .unwrap();
    assert!(today["text"].as_str().unwrap().contains("452.3bn"));

    let report = capture
        .as_of_report(&crate::domain::ChatToolTrace::default())
        .unwrap();
    assert_eq!(report.cutoff, "2026-03-10");
    assert_eq!(report.left_out.sources, 1);
    assert_eq!(report.left_out.facts, 1);
}

#[tokio::test]
async fn a_perspective_written_later_is_left_out_of_the_turn_and_its_reads() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let standing = crate::commands::knowledge::ensure_perspective(
        &state,
        "loupist-a",
        "Buy durable compounders.",
    )
    .unwrap()
    .unwrap();
    let today = crate::commands::knowledge::host_memory_context_for_turn(
        &state,
        "loupist-a",
        "",
        true,
        &[],
        None,
    )
    .unwrap()
    .unwrap();
    assert!(today.charter.is_some());
    let cutoff = crate::commands::knowledge::as_of::Cutoff::parse(Some("2026-03-10"));
    let earlier = crate::commands::knowledge::host_memory_context_for_turn(
        &state,
        "loupist-a",
        "",
        true,
        &[],
        cutoff,
    )
    .unwrap()
    .unwrap();
    // The perspective was written after the cutoff; principles still apply.
    assert!(earlier.charter.is_none());
    assert!(earlier.left_out.methods.contains(&standing));
    let capture = Arc::new(ToolCapture::default());
    let executor = executor_for(&state, capture.clone());
    let refused = executor
        .execute(
            &as_of("2026-03-10"),
            ToolMethod::LensRead,
            json!({"recordId": standing}),
        )
        .await
        .unwrap_err();
    assert!(refused.to_string().contains("as_of_later"), "{refused}");
}

/// A page whose section and summary were revised after the cutoff: its head
/// is later, so the whole page is later. Returns (revised page, untouched page).
fn revised_after_the_cutoff(state: &AppState) -> (String, String) {
    use crate::knowledge::lens_page::{LensOp, PageDelta, PageTarget};
    let store = knowledge(state);
    let origin = crate::knowledge::test_support::turn_origin("chat-a", "assistant-0");
    let revised = store
        .file_method(
            "loupist-a",
            "Cutoff review",
            "Original check.",
            &origin,
            noon("2026-03-01"),
        )
        .unwrap();
    let untouched = store
        .file_method(
            "loupist-a",
            "Cutoff baseline",
            "Original check.",
            &origin,
            noon("2026-03-01"),
        )
        .unwrap();
    store
        .file_method(
            "loupist-a",
            "Cutoff review",
            "LATERSECTION result is 452.3bn.",
            &origin,
            noon("2026-03-18"),
        )
        .unwrap();
    store
        .transact(|write| {
            write.file(
                "loupist-a",
                &origin,
                vec![PageDelta {
                    target: PageTarget::Existing {
                        record_id: revised.record_id.clone(),
                    },
                    ops: vec![LensOp::Summary {
                        text: "LATERSUMMARY revenue was 452.3bn.".into(),
                    }],
                }],
                noon("2026-03-18") + 1,
            )
        })
        .unwrap();
    (revised.record_id, untouched.record_id)
}

#[tokio::test]
async fn a_page_revised_after_the_cutoff_is_left_out_of_reads_search_and_context() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let (revised, untouched) = revised_after_the_cutoff(&state);
    let capture = Arc::new(ToolCapture::default());
    let executor = executor_for(&state, capture.clone());
    let cutoff = as_of("2026-03-10");

    let refused = executor
        .execute(&cutoff, ToolMethod::LensRead, json!({"recordId": revised}))
        .await
        .unwrap_err();
    assert!(refused.to_string().contains("as_of_later"), "{refused}");
    assert!(!refused.to_string().contains("LATER"), "{refused}");
    let read = executor
        .execute(
            &cutoff,
            ToolMethod::LensRead,
            json!({"recordId": untouched}),
        )
        .await
        .unwrap();
    assert!(read["text"].as_str().unwrap().contains("Original check."));

    for query in ["LATERSECTION", "Cutoff"] {
        let found = executor
            .execute(&cutoff, ToolMethod::LensSearch, json!({"query": query}))
            .await
            .unwrap();
        let text = found.to_string();
        assert!(!text.contains("LATER"), "{query}: {text}");
        assert!(!text.contains(&revised), "{query}: {text}");
    }

    let context = crate::commands::knowledge::host_memory_context_for_turn(
        &state,
        "loupist-a",
        "Cutoff review",
        true,
        &[],
        crate::commands::knowledge::as_of::Cutoff::parse(Some("2026-03-10")),
    )
    .unwrap()
    .unwrap();
    assert!(context.entries.iter().all(|entry| entry.id != revised));
    assert!(context.entries.iter().any(|entry| entry.id == untouched));
    assert!(context
        .relevant
        .iter()
        .all(|pointer| pointer.read_args["recordId"] != revised.as_str()));
    assert!(context.left_out.methods.contains(&revised));
    assert!(capture
        .as_of_left_out
        .lock()
        .unwrap()
        .methods
        .contains(&revised));
}

#[tokio::test]
async fn an_as_of_read_lists_only_backlinks_and_history_known_by_then() {
    use crate::knowledge::lens_page::{
        Basis, BasisRef, LensOp, PageDelta, PageKind, PageTarget, SectionKey,
    };
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let origin = crate::knowledge::test_support::turn_origin("chat-a", "assistant-0");
    let said = |key: &str, markdown: &str, expected: Option<String>| LensOp::Put {
        key: SectionKey::new(key).unwrap(),
        heading: key.to_uppercase(),
        markdown: markdown.into(),
        basis: Basis {
            refs: vec![BasisRef::UserSaid {
                message_id: "user-0".into(),
            }],
            uncertainty: None,
        },
        expected,
    };
    let subject = |title: &str, ops: Vec<LensOp>| PageDelta {
        target: PageTarget::Create {
            kind: PageKind::Subject,
            title: title.into(),
            anchors: Vec::new(),
        },
        ops,
    };
    let store = knowledge(&state);
    let hanbit = store
        .transact(|write| {
            write.file(
                "loupist-a",
                &origin,
                vec![subject(
                    "Hanbit Battery",
                    vec![said("facts", "Hanbit supplies Northgate.", None)],
                )],
                noon("2026-01-10"),
            )
        })
        .unwrap()[0]
        .record_id
        .clone();
    // In June: a stale edit is logged on Hanbit without changing it, and a new
    // page links to it.
    store
        .transact(|write| {
            write.file(
                "loupist-a",
                &origin,
                vec![
                    PageDelta {
                        target: PageTarget::Existing {
                            record_id: hanbit.clone(),
                        },
                        ops: vec![said(
                            "facts",
                            "JUNE-ONLY cancellation.",
                            Some("0".repeat(64)),
                        )],
                    },
                    subject(
                        "JUNE-ONLY Northgate cancellation",
                        vec![said(
                            "facts",
                            &format!("Cancelled [Hanbit](lens:{hanbit}) in June."),
                            None,
                        )],
                    ),
                ],
                noon("2026-06-10"),
            )
        })
        .unwrap();
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));
    let read = executor
        .execute(
            &as_of("2026-02-01"),
            ToolMethod::LensRead,
            json!({"recordId": hanbit}),
        )
        .await
        .unwrap();
    let text = read["text"].as_str().unwrap();
    let page: Value = serde_json::from_str(text).unwrap();
    assert_eq!(page["backlinks"], json!([]));
    assert_eq!(page["history"].as_array().unwrap().len(), 1, "{text}");
    assert!(!text.contains("JUNE-ONLY"), "{text}");
    // Today both are there.
    let today = executor
        .execute(
            &policy(true, false),
            ToolMethod::LensRead,
            json!({"recordId": hanbit}),
        )
        .await
        .unwrap();
    let today: Value = serde_json::from_str(today["text"].as_str().unwrap()).unwrap();
    assert_eq!(today["backlinks"].as_array().unwrap().len(), 1);
    assert_eq!(today["history"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn journal_entries_and_history_keep_to_the_cutoff_and_reviews_wait_for_the_check_date() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let turn_capture = || {
        Arc::new(ToolCapture {
            principle_user_message_id: Some("user-message".into()),
            bound_chat_id: Some("chat-a".into()),
            ..ToolCapture::default()
        })
    };
    let today = chrono::Local::now().date_naive();
    let check_day = (today + chrono::Days::new(30)).to_string();
    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    let result_ref =
        captured_file(&executor, "preview.txt", "Q4 preview: revenue near 440bn").await;
    let saved = executor
        .stage_source_save(
            &policy(true, true),
            json!({"result_ref": result_ref, "title": "Q4 preview", "coverage": "included_full",
                "publicationTime": {"precision": "date", "value": "2026-02-20"}}),
            "preview",
        )
        .await
        .unwrap();
    executor.capture.commit_delivery("preview").await;
    executor
        .execute(
            &policy(true, true),
            ToolMethod::JournalSave,
            json!({"judgment": "Solsup's FY2025 revenue will beat 445bn", "sourceReads": [saved["saveRef"]],
                "check": {"claim": "Revenue above 445bn", "metric": "revenue",
                    "direction": "above", "threshold": 445, "byDate": check_day}}),
        )
        .await
        .unwrap();
    persist_turn(&state, "chat-a", "turn-save", &capture).await;
    let journal_id =
        crate::commands::knowledge::host_search(&state, "loupist-a", "Solsup", "research")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .find_map(|hit| hit["journalId"].as_str().map(str::to_owned))
            .unwrap();

    // Before the entry was written, it is left out and counted.
    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    let yesterday = (today - chrono::Days::new(1)).to_string();
    let refused = executor
        .execute(
            &as_of(&yesterday),
            ToolMethod::ConversationMemory,
            json!({"action": "read", "journalId": journal_id}),
        )
        .await
        .unwrap_err();
    assert!(refused.to_string().contains("as_of_later"), "{refused}");
    assert_eq!(capture.as_of_left_out.lock().unwrap().counts().notes, 1);

    // On or after the entry but before its check date, a review is refused.
    let cutoff = ToolPolicy {
        as_of: Some(today.to_string()),
        ..policy(true, true)
    };
    let read = executor
        .execute(
            &cutoff,
            ToolMethod::ConversationMemory,
            json!({"action": "read", "journalId": journal_id}),
        )
        .await
        .unwrap();
    let review = executor
        .execute(
            &cutoff,
            ToolMethod::JournalReview,
            json!({"readRef": read["readRef"], "outcome": {"observed": 452.3, "observedAt": today.to_string()},
                "sourceReads": []}),
        )
        .await
        .unwrap_err();
    assert!(
        review.to_string().contains("as_of_before_check"),
        "{review}"
    );
    assert!(capture.staged_research.lock().await.is_none());

    // This Chat's history keeps messages written by then.
    let mut chat = state.store.get_chat("chat-a").unwrap().unwrap();
    let expected = chat.clone();
    let at = |date: &str| {
        chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .unwrap()
            .timestamp_millis()
    };
    for (id, role, date) in [
        ("early-q", ChatRole::User, "2026-03-01"),
        ("early-a", ChatRole::Assistant, "2026-03-01"),
        ("late-q", ChatRole::User, "2026-04-01"),
        ("late-a", ChatRole::Assistant, "2026-04-01"),
    ] {
        let mut message = chat_message(id, role, id);
        message.created_at_ms = at(date);
        chat.messages.push(message);
    }
    state.store.replace_chat(&expected, &chat).unwrap();
    let capture = turn_capture();
    let executor = executor_for(&state, capture.clone());
    let history = executor
        .execute(&as_of("2026-03-10"), ToolMethod::ChatHistory, json!({}))
        .await
        .unwrap();
    let ids: Vec<_> = history["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|message| message["message_id"].as_str())
        .collect();
    assert_eq!(ids, ["early-q", "early-a"]);
    assert_eq!(capture.as_of_left_out.lock().unwrap().counts().notes, 2);
}

#[tokio::test]
async fn a_source_published_after_the_cutoff_is_not_saved() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let capture = Arc::new(ToolCapture {
        bound_chat_id: Some("chat-a".into()),
        ..ToolCapture::default()
    });
    let executor = executor_for(&state, capture.clone());
    let result_ref = captured_file(&executor, "results.txt", "FY2025 revenue 452.3bn").await;
    let cutoff = ToolPolicy {
        as_of: Some("2026-03-10".into()),
        ..policy(true, true)
    };
    let save = |published: &'static str, delivery: &'static str| {
        let executor = executor.clone();
        let result_ref = result_ref.clone();
        let cutoff = cutoff.clone();
        async move {
            executor
                .stage_source_save(
                    &cutoff,
                    json!({"result_ref": result_ref, "title": "Annual results", "coverage": "included_full",
                        "publicationTime": {"precision": "date", "value": published}}),
                    delivery,
                )
                .await
        }
    };
    let refused = save("2026-03-18", "later").await.unwrap_err();
    assert!(refused.to_string().contains("as_of_later"), "{refused}");
    // Undated material could never be read as of a date, so it is not saved.
    let undated = executor
        .stage_source_save(
            &cutoff,
            json!({"result_ref": result_ref, "title": "Annual results", "coverage": "included_full"}),
            "undated",
        )
        .await
        .unwrap_err();
    assert!(undated.to_string().contains("as_of_undated"), "{undated}");
    save("2026-03-02", "earlier").await.unwrap();
}

fn noon(date: &str) -> i64 {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap()
        .timestamp_millis()
}

/// A World whose guidance relationship (published 2026-02-20) also cites the
/// later filing and was invalidated on 2026-04-01; the results relationship is
/// published 2026-03-18. Annotations: one citing the early filing, one citing
/// the later filing, one citing nothing. Returns (record, revision, early, later).
fn later_world(state: &AppState) -> (String, String, String, String) {
    let early = retain(
        state,
        "Solsup guidance filing",
        "2026-02-20",
        "Guidance: revenue near 440bn.",
    );
    let later = retain(
        state,
        "Solsup annual filing",
        "2026-03-18",
        "Revenue was 452.3bn.",
    );
    let node = |id: &str| GraphNode {
        id: id.into(),
        name: id.into(),
        aliases: vec![format!("{id} alias")],
        description: format!("Later summary of {id}: revenue 452.3bn."),
        kind: "company".into(),
        source_role: "source".into(),
        properties: Default::default(),
    };
    let claim =
        |id: &str, proposition: &str, target: &str, spans: &[&str], published: &str| GraphClaim {
            id: id.into(),
            proposition: proposition.into(),
            attribution: "example-publisher".into(),
            relation: "reported".into(),
            endpoints: vec![
                ClaimEndpoint {
                    node_id: "solsup".into(),
                    role: "source".into(),
                },
                ClaimEndpoint {
                    node_id: target.into(),
                    role: "target".into(),
                },
            ],
            qualifiers: Vec::new(),
            evidence: spans
                .iter()
                .map(|span| EvidenceBinding {
                    span_id: (*span).into(),
                    role: BindingRole::Supports,
                    attribution: "example-publisher".into(),
                })
                .collect(),
            status: ClaimStatus::Active,
            recorded_at_ms: Some(1_700_000_000_000),
            publication_time: Some(day(published)),
            effective_time: None,
            valid_at: None,
            invalid_at: None,
            expired_at: None,
        };
    let mut guidance = claim(
        "claim-guidance",
        "Solsup guided revenue near 440bn",
        "guidance",
        &[&early.2, &later.2],
        "2026-02-20",
    );
    guidance.invalid_at = Some(day("2026-04-01"));
    let results = claim(
        "claim-results",
        "Solsup reported revenue of 452.3bn",
        "results",
        &[&later.2],
        "2026-03-18",
    );
    let annotation =
        |topic: &str, markdown: &str, spans: &[&str]| crate::knowledge::TopicAnnotation {
            topic_id: topic.into(),
            markdown: markdown.into(),
            source_span_ids: spans.iter().map(|span| (*span).to_owned()).collect(),
        };
    let revision = knowledge(state)
        .transact(|write| {
            let record = write.create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "Solsup".into(),
                at_ms: 1_700_000_000_000,
            })?;
            write.seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: record.record_id.clone(),
                expected_revision_id: None,
                at_ms: 1_700_000_000_000,
                nodes: vec![node("solsup"), node("guidance"), node("results")],
                claims: vec![guidance, results],
                annotations: vec![
                    annotation("solsup", "Solsup guides conservatively.", &[&early.2]),
                    annotation("guidance", "Guidance was beaten at 452.3bn.", &[&later.2]),
                    annotation("results", "Results topic, written later.", &[]),
                ],
                dependencies: [&early, &later]
                    .into_iter()
                    .map(|source| EvidenceRevision {
                        record_id: source.0.clone(),
                        revision_digest: source.1.clone(),
                        role: DependencyRole::WorldSource,
                    })
                    .collect(),
            })
        })
        .unwrap();
    (revision.record_id, revision.revision_id, early.2, later.2)
}

#[tokio::test]
async fn graph_search_views_and_annotations_keep_to_the_cutoff() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let (world, revision, early, later) = later_world(&state);
    let edge = |id: &str, target: &str, spans: &[&str]| {
        json!({"adapter_id": id, "proposition": format!("{id} proposition"),
            "endpoints": [{"node_id": "solsup", "role": "source"}, {"node_id": target, "role": "target"}],
            "evidence": spans.iter().map(|span| json!({"span_id": span, "role": "supports", "attribution": "p"})).collect::<Vec<_>>(),
            "invalid_at": {"precision": "date", "value": "2026-04-01", "approximate": false},
            "expired_at": null})
    };
    let node = |id: &str| {
        json!({"adapter_id": id, "name": id, "kind": "company", "aliases": ["alias"],
            "description": format!("Later summary of {id}: revenue 452.3bn."), "properties": {}})
    };
    let hits = |extra: Vec<Value>| {
        let mut rows = vec![
            json!({"scope": "world", "recordId": world, "revisionId": revision,
            "retrieval": "rrf", "indexed": true, "view": {"usage": {}, "view": {
                "nodes": [node("solsup"), node("guidance"), node("results")],
                "edges": [edge("claim-guidance", "guidance", &[&early, &later]),
                    edge("claim-results", "results", &[&later])]}}}),
        ];
        rows.extend(extra);
        rows
    };
    let annotation = |topic: &str| {
        json!({"scope": "world", "recordId": world, "revisionId": revision, "objectId": topic,
            "title": "Solsup", "snippet": topic})
    };
    let mut rows = hits(vec![
        annotation("solsup"),
        annotation("guidance"),
        annotation("results"),
        // A World hit without an object id says nothing datable.
        json!({"scope": "world", "recordId": world, "revisionId": revision, "title": "Solsup"}),
    ]);
    let cutoff = crate::commands::knowledge::as_of::Cutoff::parse(Some("2026-03-10")).unwrap();
    let mut left = crate::commands::knowledge::as_of::LeftOut::default();
    crate::commands::knowledge::as_of::filter_hits(
        &state,
        "loupist-a",
        &mut rows,
        cutoff,
        &mut left,
    )
    .unwrap();
    let encoded = serde_json::to_string(&rows).unwrap();
    assert!(!encoded.contains("452.3bn"), "{encoded}");
    assert!(!encoded.contains("claim-results"), "{encoded}");
    assert!(!encoded.contains(&later), "{encoded}");
    assert!(!encoded.contains("2026-04-01"), "{encoded}");
    let view = &rows[0]["view"]["view"];
    assert_eq!(view["edges"].as_array().unwrap().len(), 1);
    assert_eq!(view["edges"][0]["evidence"].as_array().unwrap().len(), 1);
    let names: Vec<_> = view["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["adapter_id"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["solsup", "guidance"]);
    assert!(view["nodes"][0].get("description").is_none());
    // Only the annotation every Source of which was published by then stays.
    let topics: Vec<_> = rows[1..]
        .iter()
        .map(|hit| hit["objectId"].as_str().unwrap())
        .collect();
    assert_eq!(topics, ["solsup"]);
    assert!(left.facts.contains("claim-results"));

    // Without a cutoff the search keeps everything.
    let today = hits(Vec::new());
    assert!(serde_json::to_string(&today).unwrap().contains("452.3bn"));
}

#[tokio::test]
async fn a_world_read_shows_relationships_as_they_stood_at_the_cutoff() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let (world, _, early, later) = later_world(&state);
    let executor = executor_for(&state, Arc::new(ToolCapture::default()));
    let read = |policy: ToolPolicy| {
        let executor = executor.clone();
        let world = world.clone();
        async move {
            let read = executor
                .execute(
                    &policy,
                    ToolMethod::WorldRead,
                    json!({"scope": "world", "recordId": world}),
                )
                .await
                .unwrap();
            let text = read["text"].as_str().unwrap();
            serde_json::from_str::<Value>(
                crate::custom_connections::taint::delimited_body(text).unwrap_or(text),
            )
            .unwrap()
        }
    };
    let then = read(as_of("2026-03-10")).await;
    let edges = then["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 1, "{then}");
    let claim = &edges[0]["claim"];
    // The invalidation is dated after the cutoff, and the later filing's
    // evidence was not published yet.
    assert!(claim["invalidAt"].is_null(), "{claim}");
    let spans: Vec<_> = claim["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|binding| binding["spanId"].as_str().unwrap())
        .collect();
    assert_eq!(spans, [early.as_str()]);
    let encoded = then.to_string();
    assert!(!encoded.contains(&later), "{encoded}");
    // Today the relationship shows its invalidation and both filings.
    let today = read(policy(true, false)).await;
    let claim = today["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["id"] == "claim-guidance")
        .unwrap()["claim"]
        .clone();
    assert_eq!(claim["invalidAt"]["value"], "2026-04-01");
    assert_eq!(claim["evidence"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn later_notes_are_not_named_under_a_cutoff() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let note = |chat: &str, at_ms: i64, title: &str, markdown: &str, previous: Vec<String>| {
        knowledge(&state)
            .transact(|write| {
                write.open_turn(crate::knowledge::OpenTurn {
                    loupist_id: "loupist-a".into(),
                    chat_session_id: chat.into(),
                    turn_id: chat.into(),
                    at_ms,
                })?;
                write.save_research(crate::knowledge::SaveResearch {
                    loupist_id: "loupist-a".into(),
                    turn_id: chat.into(),
                    at_ms,
                    observations: String::new(),
                    interpretation: String::new(),
                    assumptions: String::new(),
                    evaluation: String::new(),
                    journal: None,
                    note_inputs: Some(crate::knowledge::NoteInputDraft {
                        source_receipt_ids: Vec::new(),
                        world_delivery_ids: Vec::new(),
                        lenses: Vec::new(),
                        previous_note_ids: previous,
                    }),
                    working_note: Some(crate::knowledge::WorkingNoteDraft {
                        title: title.into(),
                        markdown: markdown.into(),
                        change_summary: "Saved".into(),
                        expected_research_id: None,
                    }),
                })
            })
            .unwrap()
            .research_id
    };
    let original = note(
        "chat-old",
        noon("2026-03-01"),
        "Original premise",
        "Original premise: revenue near 440bn.",
        Vec::new(),
    );
    note(
        "chat-new",
        noon("2026-04-01"),
        "Revenue came in at 452.3bn",
        "Correction after the annual filing.",
        vec![original.clone()],
    );
    let capture = Arc::new(ToolCapture::default());
    let executor = executor_for(&state, capture.clone());
    let cutoff = as_of("2026-03-10");
    // The exact read of the earlier note no longer names the later one.
    let then = executor
        .execute(
            &cutoff,
            ToolMethod::ConversationMemory,
            json!({"action": "read", "noteId": original}),
        )
        .await
        .unwrap();
    assert!(!then.to_string().contains("452.3bn"), "{then}");
    let today = executor
        .execute(
            &policy(true, false),
            ToolMethod::ConversationMemory,
            json!({"action": "read", "noteId": original}),
        )
        .await
        .unwrap();
    assert!(today.to_string().contains("452.3bn"), "{today}");
    // Nor does a search hit for it.
    let found = executor
        .execute(
            &cutoff,
            ToolMethod::ConversationMemory,
            json!({"action": "search", "query": "original premise"}),
        )
        .await
        .unwrap();
    assert!(!found.to_string().contains("452.3bn"), "{found}");
    assert!(found["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|hit| hit["researchId"] == original.as_str()));
    // Nor the automatic context's pointer to it.
    let context = crate::commands::knowledge::host_memory_context_for_turn(
        &state,
        "loupist-a",
        "original premise",
        true,
        &[],
        crate::commands::knowledge::as_of::Cutoff::parse(Some("2026-03-10")),
    )
    .unwrap()
    .unwrap();
    assert!(context
        .relevant
        .iter()
        .any(|pointer| pointer.read_args["noteId"] == original.as_str()));
    assert!(context
        .relevant
        .iter()
        .all(|pointer| pointer.later_notes.is_empty()));
}

#[tokio::test]
async fn a_search_counts_only_later_messages_it_would_have_shown() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let mut chat = state.store.get_chat("chat-a").unwrap().unwrap();
    let expected = chat.clone();
    for (id, role, date, text) in [
        (
            "early-q",
            ChatRole::User,
            "2026-03-01",
            "What is Solsup's guidance?",
        ),
        (
            "early-a",
            ChatRole::Assistant,
            "2026-03-01",
            "Solsup guided revenue near 440bn.",
        ),
        (
            "late-q",
            ChatRole::User,
            "2026-04-01",
            "Did Solsup beat its guidance?",
        ),
        (
            "late-a",
            ChatRole::Assistant,
            "2026-04-01",
            "Yes, Solsup reported 452.3bn.",
        ),
        ("other-q", ChatRole::User, "2026-04-02", "Weather in Seoul?"),
        ("other-a", ChatRole::Assistant, "2026-04-02", "Sunny."),
    ] {
        let mut message = chat_message(id, role, text);
        message.created_at_ms = noon(date);
        chat.messages.push(message);
    }
    state.store.replace_chat(&expected, &chat).unwrap();
    let capture = Arc::new(ToolCapture::default());
    let executor = executor_for(&state, capture.clone());
    let found = executor
        .execute(
            &as_of("2026-03-10"),
            ToolMethod::ConversationMemory,
            json!({"action": "search", "query": "Solsup guidance", "role": "assistant"}),
        )
        .await
        .unwrap();
    let ids: Vec<_> = found["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["messageId"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["early-a"]);
    // One later answer matched; the unrelated later chat is not counted.
    assert_eq!(capture.as_of_left_out.lock().unwrap().counts().notes, 1);
}
