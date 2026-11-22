use super::*;
use crate::commands::tool_executor::{
    StagedKnowledgeRead, StagedResearch, StagedResearchJournal, StagedSource,
};
use crate::domain::{ChatMessage, ChatRole, ChatSession, MemoryPolicy};
use crate::knowledge::{
    Authorship, ClaimEndpoint, CreateRecord, DependencyRole, EvidenceBinding, EvidenceRevision,
    GraphClaim, GraphNode, KnowledgeWrite, PageRead, RepresentationRole, RetainMaterial,
    RetainNewSource, SealWorld, Selector, SourceClass, TimePoint, TimePrecision,
};
use crate::store::{SqliteStore, StoreError};
use crate::test_fixtures::loupist_profile;
use base64::Engine;

fn message(id: &str, created_at_ms: i64) -> ChatMessage {
    ChatMessage {
        created_at_ms,
        ..crate::test_fixtures::chat_message(id, ChatRole::Assistant, "Saved")
    }
}

fn chat(id: &str, loupist_id: &str) -> ChatSession {
    ChatSession {
        memory_policy: MemoryPolicy {
            use_memory: true,
            update_memory: true,
        },
        set_aside_memory: Vec::new(),
        versions: Vec::new(),
        messages: vec![message("user-1", 2)],
        updated_at_ms: 2,
        ..crate::test_fixtures::chat_session(id, loupist_id)
    }
}

fn staged(bytes: &str) -> StagedSource {
    StagedSource {
        custom_origin: None,
        source_class: SourceClass::ToolResult,
        authorship: Authorship::Tool,
        title: bytes.into(),
        bytes: bytes.as_bytes().to_vec(),
        media_type: "text/plain".into(),
        origin: "https://example.test/note".into(),
        coverage: "included_full".into(),
        chat_id: "chat-1".into(),
        request_ref: "result-1".into(),
        result_ref: "result-1".into(),
        publication_time: Some(TimePoint {
            precision: TimePrecision::Date,
            value: Some("2021-05-01".into()),
            approximate: false,
        }),
        provider_identity: "example-publisher".into(),
        document_id: "doc-notes".into(),
        material_role: "original".into(),
        provided: None,
    }
}

fn text_source(loupist_id: &str, provider: &str, bytes: &[u8]) -> RetainNewSource {
    RetainNewSource {
        origin: None,
        loupist_id: loupist_id.into(),
        at_ms: 6,
        title: "Note".into(),
        source_class: SourceClass::PublisherDocument,
        origin_locator: "https://example.test/note".into(),
        provider_identity: provider.into(),
        authorship: Authorship::Publisher,
        coverage: Coverage::IncludedFull,
        origin_chat_id: None,
        request_ref: None,
        declared_digest: None,
        publication_time: None,
        material: Some(RetainMaterial {
            media_type: "text/plain".into(),
            role: RepresentationRole::Original,
            document_id: "note".into(),
            parser_version: None,
            parent_representation_id: None,
            input_span_ids: Vec::new(),
            bytes: bytes.to_vec(),
            selector: Selector::ByteRange {
                start: 0,
                end: bytes.len() as u64,
            },
            context_note: "Exact original".into(),
        }),
    }
}

fn research(
    source_reads: Vec<String>,
    world_reads: Vec<String>,
    lens_reads: Vec<String>,
) -> StagedResearch {
    StagedResearch {
        thesis_pair: None,
        source_reads: Vec::new(),
        world_reads: Vec::new(),
        lens_reads: Vec::new(),
        previous_note_ids: Vec::new(),
        working_note: None,
        observations: "The delivered material is the evidence.".into(),
        interpretation: String::new(),
        assumptions: String::new(),
        evaluation: String::new(),
        journal: Some(StagedResearchJournal {
            thesis: None,
            at_ms: 1,
            check: Some(crate::knowledge::journal::fixture_prediction()),
            outcome: None,
            judgment: "Keep the delivered material.".into(),
            reasoning: "It was acknowledged in this turn.".into(),
            earlier: Vec::new(),
            source_reads,
            world_reads,
            lens_reads,
            entities: Vec::new(),
        }),
    }
}

/// Appends one assistant turn and commits its Memory in the Chat transaction.
#[allow(clippy::too_many_arguments)]
fn save_turn(
    store: &SqliteStore,
    blobs: &BlobStore,
    loupist_id: &str,
    chat_id: &str,
    turn_id: &str,
    save_learning: bool,
    sources: &[StagedSource],
    reads: &[StagedKnowledgeRead],
    research: Option<&StagedResearch>,
) -> Result<Vec<crate::domain::MemoryUpdateChange>, StoreError> {
    let current = store.get_chat(chat_id).unwrap().unwrap();
    let at_ms = current.updated_at_ms + 10;
    let mut next = current.clone();
    next.messages.push(message(turn_id, at_ms));
    next.updated_at_ms = at_ms;
    let mut changes = Vec::new();
    store.replace_chat_with(&current, &next, |tx| {
        changes = persist_saved_turn(
            tx,
            blobs,
            loupist_id,
            chat_id,
            turn_id,
            at_ms,
            &[],
            save_learning,
            sources,
            reads,
            research,
        )
        .map_err(|error| StoreError::Invalid(error.to_string()))?;
        Ok(())
    })?;
    Ok(changes)
}

fn count(store: &SqliteStore, sql: &str) -> i64 {
    store
        .lock()
        .unwrap()
        .query_row(sql, [], |row| row.get(0))
        .unwrap()
}

fn empty_read(record_id: &str) -> HostReadRequest {
    HostReadRequest {
        as_of: None,
        scope: "lens".into(),
        record_id: record_id.into(),
        revision_id: None,
        span_id: None,
        search: String::new(),
        center_claim_id: None,
        center_node_id: None,
        kinds: Vec::new(),
        relations: Vec::new(),
        binding_roles: Vec::new(),
        statuses: Vec::new(),
        node_limit: None,
        edge_limit: None,
        byte_limit: None,
        lens_status: "considered".into(),
    }
}

fn node(id: &str, name: &str) -> GraphNode {
    GraphNode {
        id: id.into(),
        name: name.into(),
        aliases: Vec::new(),
        description: String::new(),
        kind: "company".into(),
        source_role: "subject".into(),
        properties: Default::default(),
    }
}

fn claim(node_id: &str, proposition: &str, span_id: Option<&str>) -> GraphClaim {
    GraphClaim {
        id: format!("claim-{node_id}"),
        proposition: proposition.into(),
        attribution: "publisher-a".into(),
        relation: "DESCRIBED".into(),
        endpoints: vec![ClaimEndpoint {
            node_id: node_id.into(),
            role: "subject".into(),
        }],
        qualifiers: Vec::new(),
        evidence: span_id
            .map(|span_id| EvidenceBinding {
                span_id: span_id.into(),
                role: BindingRole::Supports,
                attribution: "publisher-a".into(),
            })
            .into_iter()
            .collect(),
        status: ClaimStatus::Active,
        recorded_at_ms: Some(8),
        publication_time: None,
        effective_time: None,
        valid_at: None,
        invalid_at: None,
        expired_at: None,
    }
}

mod authority_integration;
mod delivery_integration;

#[tokio::test]
async fn world_catalog_exposes_only_current_content_with_loupist_isolation() {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    for id in ["loupist-a", "loupist-b"] {
        state.store.create_loupist(&loupist_profile(id)).unwrap();
    }
    let (record_id, older, current) = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        let record = write.create_record(CreateRecord {
            loupist_id: "loupist-a".into(),
            kind: RecordKind::World,
            title: "Supply chain".into(),
            at_ms: 5,
        })?;
        let initial = write.seal_world(SealWorld {
            loupist_id: "loupist-a".into(),
            record_id: record.record_id.clone(),
            expected_revision_id: None,
            at_ms: 6,
            nodes: vec![],
            claims: vec![],
            annotations: vec![],
            dependencies: vec![],
        })?;
        let older = initial.revision_id.clone();
        let current = write.seal_world(SealWorld {
            loupist_id: "loupist-a".into(),
            record_id: record.record_id.clone(),
            expected_revision_id: Some(older.clone()),
            at_ms: 6,
            nodes: vec![node("supplier", "Supplier")],
            claims: vec![],
            annotations: vec![],
            dependencies: vec![],
        })?;
        Ok((record.record_id, older, current.revision_id))
    })
    .unwrap();
    let request = |loupist_id: &str| KnowledgeCatalogRequest {
        loupist_id: loupist_id.into(),
    };
    assert_eq!(
        catalog(&state, request("loupist-a")).unwrap().worlds[0].revision_id,
        current
    );
    assert!(catalog(&state, request("loupist-b"))
        .unwrap()
        .worlds
        .is_empty());
    with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        assert!(matches!(
            write.get_revision("loupist-a", &older),
            Err(KnowledgeError::NotFound)
        ));
        assert_eq!(
            write.get_revision("loupist-a", &current)?.payload["nodes"][0]["id"],
            "supplier"
        );
        assert!(matches!(
            write.owned_record("loupist-b", &record_id),
            Err(KnowledgeError::Unauthorized(_))
        ));
        Ok(())
    })
    .unwrap();
}

#[test]
fn renderer_world_detail_across_a_publication() {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    let loupist = "loupist-a";
    state
        .store
        .create_loupist(&loupist_profile(loupist))
        .unwrap();
    let (record, source) = with_loupist_knowledge(&state, loupist, |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        Ok((
            write.create_record(CreateRecord {
                loupist_id: loupist.into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: 7,
            })?,
            write.retain_new_source(text_source(loupist, "publisher-a", b"Subaru figures"))?,
        ))
    })
    .unwrap();
    let snapshot = |expected_revision_id, at_ms, revenue: &str| {
        let mut claims = (0..1019)
            .map(|i| {
                let subject = if i < 2 {
                    "subaru".into()
                } else {
                    format!("name-{}", i % 462)
                };
                let statement = match i {
                    0 => revenue.into(),
                    1 => "Market cap: $12B".into(),
                    _ => format!("Figure {i}: {}", "supported measurement ".repeat(24)),
                };
                let mut fact = claim(&subject, &statement, source.span_id.as_deref());
                fact.id = format!("claim-{i}");
                fact.relation = "REPORTS".into();
                fact.endpoints.push(ClaimEndpoint {
                    node_id: subject,
                    role: "object".into(),
                });
                fact
            })
            .collect::<Vec<_>>();
        claims[0].recorded_at_ms = Some(at_ms);
        claims[1018].endpoints.push(ClaimEndpoint {
            node_id: "name-0".into(),
            role: "counterparty".into(),
        });
        SealWorld {
            loupist_id: loupist.into(),
            record_id: record.record_id.clone(),
            expected_revision_id,
            at_ms,
            nodes: std::iter::once(node("subaru", "Subaru"))
                .chain((0..462).map(|i| node(&format!("name-{i}"), &format!("Name {i}"))))
                .collect(),
            claims,
            annotations: vec![],
            dependencies: vec![EvidenceRevision {
                record_id: source.record_id.clone(),
                revision_digest: source.revision_digest.clone(),
                role: DependencyRole::WorldSource,
            }],
        }
    };
    let first = with_loupist_knowledge(&state, loupist, |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).seal_world(snapshot(None, 8, "Revenue: $31B"))
    })
    .unwrap();
    let page = map(
        &state,
        crate::knowledge::KnowledgeMapQuery {
            loupist_id: loupist.into(),
            max_nodes: Some(1000),
        },
    )
    .unwrap();
    assert_eq!(
        page.world_revision_id.as_deref(),
        Some(first.revision_id.as_str())
    );
    let detail: WorldGraphQuery = serde_json::from_value(serde_json::json!({
        "loupistId": loupist, "recordId": record.record_id, "revisionId": page.world_revision_id,
        "centerNodeId": "subaru", "byteLimit": 524288, "nodeLimit": 100, "edgeLimit": 100,
    }))
    .unwrap();
    assert_eq!(page.counts["claim"].total, 1);
    assert_eq!(page.counts["fact"].shown, 1018);
    with_loupist_knowledge(&state, loupist, |tx, blobs| {
        let revision = KnowledgeWrite::bind(tx, blobs).get_revision(loupist, &first.revision_id)?;
        assert!(serde_json::to_vec(&revision.payload)?.len() > 524288);
        assert_eq!(revision.payload["relationVocabulary"][0]["claims"], 1);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        page.nodes
            .iter()
            .find(|node| node.id == "subaru")
            .unwrap()
            .degree,
        0
    );
    let before = world_query(&state, detail.clone()).unwrap();
    assert_eq!(before.view.edges.len(), 2);
    assert_eq!(before.view.source_targets.len(), 1);
    let target = before.view.source_targets.values().next().unwrap();
    let source_request = || KnowledgeSourceReadRequest {
        loupist_id: loupist.into(),
        record_id: target.record_id.clone(),
        revision_id: target.revision_id.clone(),
        source_id: Some(target.source_id.clone()),
        span_id: Some(target.span_id.clone()),
        world_record_id: Some(target.world_record_id.clone()),
        world_revision_id: Some(target.world_revision_id.clone()),
        purpose: "renderer_inspection".into(),
        figure: None,
    };
    // Start a renderer read while publication owns the store connection.
    // It must wait for the transaction and resolve the head after the commit.
    let (current, after) = std::thread::scope(|scope| {
        let blobs = knowledge_blobs_for_loupist(&state, loupist).unwrap();
        let mut connection = state.store.lock_loupist(loupist).unwrap();
        let tx = connection.transaction().unwrap();
        let current = KnowledgeWrite::bind(&tx, &blobs)
            .seal_world(snapshot(
                Some(first.revision_id.clone()),
                9,
                "Revenue: $32B",
            ))
            .unwrap();
        let (started, wait) = std::sync::mpsc::channel();
        let request = detail.clone();
        let state = &state;
        let reader = scope.spawn(move || {
            started.send(()).unwrap();
            world_query(state, request).unwrap()
        });
        wait.recv().unwrap();
        tx.commit().unwrap();
        drop(connection);
        (current, reader.join().unwrap())
    });
    assert_eq!(after.view.revision_id, current.revision_id);
    let growth_request = |revision_id: String| crate::knowledge::GrowthEventsQuery {
        loupist_id: loupist.into(),
        since_ms: None,
        until_ms: Some(9),
        world_revision_id: Some(revision_id),
        limit: Some(5),
        cursor: None,
    };
    let growth = |request| {
        with_loupist_knowledge(&state, loupist, |tx, blobs| {
            KnowledgeWrite::bind(tx, blobs).growth_events(&request)
        })
    };
    assert_eq!(
        growth(growth_request(first.revision_id))
            .unwrap_err()
            .code
            .as_str(),
        "conflict"
    );
    assert!(growth(growth_request(current.revision_id.clone()))
        .unwrap()
        .next_cursor
        .is_some());

    let pinned_error = with_loupist_knowledge(&state, loupist, |tx, blobs| {
        world_view_with_sources(&KnowledgeWrite::bind(tx, blobs), detail)
    })
    .unwrap_err();
    assert_eq!(pinned_error.code.as_str(), "not_found");
    assert_eq!(after.view.edges.len(), 2);
    assert!(after
        .view
        .edges
        .iter()
        .any(|edge| edge.claim.proposition == "Revenue: $32B"));
    assert_eq!(
        source_read(&state, source_request())
            .unwrap()
            .selected_span_id
            .as_deref(),
        Some(target.span_id.as_str())
    );
    with_loupist_knowledge(&state, loupist, |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).seal_world(SealWorld {
            loupist_id: loupist.into(),
            record_id: record.record_id,
            expected_revision_id: Some(current.revision_id),
            at_ms: 10,
            nodes: vec![node("subaru", "Subaru")],
            claims: vec![],
            annotations: vec![],
            dependencies: vec![],
        })
    })
    .unwrap();
    assert_eq!(
        source_read(&state, source_request())
            .unwrap_err()
            .code
            .as_str(),
        "unauthorized"
    );
}

#[test]
fn a_followed_asset_reason_commits_without_a_memory_read() {
    // journal_save stages a thesis reason with no reads (the asset page is its
    // context); the reply commit must accept what staging accepted.
    let directory = tempfile::tempdir().unwrap();
    let store = SqliteStore::open_in_memory().unwrap();
    store.create_loupist(&loupist_profile("loupist-a")).unwrap();
    store.create_chat(&chat("chat-1", "loupist-a")).unwrap();
    let blobs = BlobStore::open(directory.path().join("blobs")).unwrap();
    let asset_id = crate::knowledge::assets::asset_id("NVDA", "NASDAQ");
    let page = crate::commands::assets::wire::AssetRow {
        asset_id: asset_id.clone(),
        name: "NVDA".into(),
        created_at_ms: 1,
        followed_at_ms: None,
        reference: None,
    };
    let reason = StagedResearch {
        thesis_pair: None,
        journal: Some(StagedResearchJournal {
            thesis: Some((
                crate::knowledge::assets::JournalThesis {
                    asset_id,
                    role: crate::knowledge::assets::ThesisRole::Reason,
                },
                Some(page),
            )),
            check: None,
            judgment: "Data-center growth slows in 2027 as capex normalizes.".into(),
            ..research(Vec::new(), Vec::new(), Vec::new())
                .journal
                .unwrap()
        }),
        ..research(Vec::new(), Vec::new(), Vec::new())
    };
    let changes = save_turn(
        &store,
        &blobs,
        "loupist-a",
        "chat-1",
        "turn-1",
        true,
        &[],
        &[],
        Some(&reason),
    )
    .unwrap();
    assert!(changes.iter().any(|change| change.kind == "Journal"));
    assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_journals"), 1);

    // An entry that is not a reason or line still needs an acknowledged read.
    let unread = research(Vec::new(), Vec::new(), Vec::new());
    assert!(save_turn(
        &store,
        &blobs,
        "loupist-a",
        "chat-1",
        "turn-2",
        true,
        &[],
        &[],
        Some(&unread),
    )
    .is_err());
    assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_journals"), 1);
}

#[test]
fn followed_asset_thesis_pair_commits_reason_before_line_atomically_in_both_orders() {
    for reverse in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let store = SqliteStore::open_in_memory().unwrap();
        store.create_loupist(&loupist_profile("loupist-a")).unwrap();
        store.create_chat(&chat("chat-1", "loupist-a")).unwrap();
        let blobs = BlobStore::open(directory.path().join("blobs")).unwrap();
        let mut reason = research(Vec::new(), Vec::new(), Vec::new());
        let journal = reason.journal.as_mut().unwrap();
        journal.check = None;
        let asset_id = crate::knowledge::assets::asset_id("NVDA", "NASDAQ");
        journal.thesis = Some((
            crate::knowledge::assets::JournalThesis {
                asset_id: asset_id.clone(),
                role: crate::knowledge::assets::ThesisRole::Reason,
            },
            Some(crate::commands::assets::wire::AssetRow {
                asset_id,
                name: "NVDA".into(),
                created_at_ms: 1,
                followed_at_ms: None,
                reference: None,
            }),
        ));
        let mut line = journal.clone();
        line.thesis.as_mut().unwrap().0.role = crate::knowledge::assets::ThesisRole::Line;
        line.check = Some(crate::knowledge::JournalCheck {
            claim: "Margin below 8%".into(),
            metric: "operating margin".into(),
            direction: "below".into(),
            threshold: 8.0,
            unit: Some("%".into()),
            by_date: None,
            confidence: None,
        });
        reason.thesis_pair = Some(line);
        if reverse {
            std::mem::swap(&mut reason.journal, &mut reason.thesis_pair);
        }
        // A failed reply rolls back both rows and the new asset page.
        let current = store.get_chat("chat-1").unwrap().unwrap();
        let mut failed = current.clone();
        failed.messages.push(message("failed", 30));
        assert!(store
            .replace_chat_with(&current, &failed, |tx| {
                persist_saved_turn(
                    tx,
                    &blobs,
                    "loupist-a",
                    "chat-1",
                    "failed",
                    30,
                    &[],
                    true,
                    &[],
                    &[],
                    Some(&reason),
                )
                .map_err(|e| StoreError::Invalid(e.to_string()))?;
                Err(StoreError::Invalid("failed reply".into()))
            })
            .is_err());
        assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_journals"), 0);
        let changes = save_turn(
            &store,
            &blobs,
            "loupist-a",
            "chat-1",
            "saved",
            true,
            &[],
            &[],
            Some(&reason),
        )
        .unwrap();
        assert_eq!(
            changes
                .iter()
                .filter(|change| change.kind == "Journal")
                .count(),
            2
        );
        assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_journals"), 2);
        let mut connection = store.lock_loupist("loupist-a").unwrap();
        let tx = connection.transaction().unwrap();
        let first_role: String = tx.query_row("SELECT json_extract(body_json, '$.thesis.role') FROM knowledge_journals ORDER BY rowid LIMIT 1", [], |row| row.get(0)).unwrap();
        assert_eq!(
            first_role, "reason",
            "even a line-first call writes its reason first"
        );
        let rows = crate::knowledge::journal::list(&tx, "loupist-a").unwrap();
        assert_eq!(
            rows.iter()
                .filter(|row| row
                    .check
                    .as_ref()
                    .is_some_and(|check| check.threshold == 8.0 && check.by_date.is_none()))
                .count(),
            1
        );
    }
}

#[test]
fn saved_turn_memory_commits_with_the_chat_and_follows_its_policy() {
    let directory = tempfile::tempdir().unwrap();
    let store = SqliteStore::open_in_memory().unwrap();
    let loupist_a = loupist_profile("loupist-a");
    store.create_loupist(&loupist_a).unwrap();
    store.create_loupist(&loupist_profile("loupist-b")).unwrap();
    store.create_chat(&chat("chat-1", "loupist-a")).unwrap();
    let blobs = BlobStore::open(directory.path().join("blobs")).unwrap();
    let journaled = research(vec!["result-1".into()], Vec::new(), Vec::new());
    save_turn(
        &store,
        &blobs,
        "loupist-a",
        "chat-1",
        "turn-1",
        true,
        &[staged("exact bytes")],
        &[],
        Some(&journaled),
    )
    .unwrap();
    assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_sources"), 1);
    assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_journals"), 1);

    // A stopped turn rolls its Memory back with the transcript.
    let current = store.get_chat("chat-1").unwrap().unwrap();
    let mut stopped = current.clone();
    stopped.messages.push(message("turn-stop", 30));
    let error = store
        .replace_chat_with(&current, &stopped, |tx| {
            persist_saved_turn(
                tx,
                &blobs,
                "loupist-a",
                "chat-1",
                "turn-stop",
                30,
                &[],
                true,
                &[staged("must not remain")],
                &[],
                None,
            )
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
            Err(StoreError::Conflict("stop"))
        })
        .unwrap_err();
    assert!(matches!(error, StoreError::Conflict("stop")));
    // A turn without Memory updates saves nothing.
    save_turn(
        &store,
        &blobs,
        "loupist-a",
        "chat-1",
        "turn-off",
        false,
        &[staged("not saved")],
        &[],
        None,
    )
    .unwrap();
    assert_eq!(store.get_chat("chat-1").unwrap().unwrap().messages.len(), 3);
    assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_sources"), 1);

    // Deleting the Chat keeps Memory; deleting a Loupist removes only its own.
    store
        .delete_chat(&store.get_chat("chat-1").unwrap().unwrap())
        .unwrap();
    assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_sources"), 1);
    assert_eq!(
        count(&store, "SELECT COUNT(*) FROM knowledge_chat_origins"),
        0
    );
    {
        let mut connection = store.lock().unwrap();
        let tx = connection.transaction().unwrap();
        KnowledgeWrite::bind(&tx, &blobs)
            .retain_new_source(text_source("loupist-b", "publisher-b", b"other Loupist"))
            .unwrap();
        tx.commit().unwrap();
    }
    store.delete_loupist(&loupist_a).unwrap();
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM knowledge_records WHERE owner_loupist_id = 'loupist-b'"
        ),
        1
    );
    for table in ["knowledge_journals", "knowledge_turns"] {
        assert_eq!(
            count(
                &store,
                &format!("SELECT COUNT(*) FROM {table} WHERE loupist_id = 'loupist-a'")
            ),
            0
        );
    }
}

#[test]
fn duplicate_capture_and_unknown_coverage_do_not_invent_sources() {
    let directory = tempfile::tempdir().unwrap();
    let store = SqliteStore::open_in_memory().unwrap();
    store.create_loupist(&loupist_profile("loupist-a")).unwrap();
    store.create_chat(&chat("chat-3", "loupist-a")).unwrap();
    let blobs = BlobStore::open(directory.path().join("blobs")).unwrap();
    let mut first = staged("same bytes");
    let second = StagedSource {
        request_ref: "result-2".into(),
        ..staged("same bytes")
    };
    save_turn(
        &store,
        &blobs,
        "loupist-a",
        "chat-3",
        "turn-dup",
        true,
        &[first.clone(), second],
        &[],
        None,
    )
    .unwrap();
    assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_sources"), 1);
    assert_eq!(
        count(&store, "SELECT COUNT(*) FROM knowledge_source_captures"),
        1
    );
    first.coverage = "not-a-coverage".into();
    let error = save_turn(
        &store,
        &blobs,
        "loupist-a",
        "chat-3",
        "turn-bad",
        true,
        &[first],
        &[],
        None,
    )
    .unwrap_err();
    assert!(matches!(error, StoreError::Invalid(_)));
    assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_sources"), 1);
    assert_eq!(store.get_chat("chat-3").unwrap().unwrap().messages.len(), 2);
}

#[test]
fn journals_keep_the_acknowledged_snapshot_and_refuse_unacknowledged_or_foreign_reads() {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    for id in ["loupist-a", "loupist-b"] {
        state.store.create_loupist(&loupist_profile(id)).unwrap();
    }
    state
        .store
        .create_chat(&chat("chat-ack", "loupist-a"))
        .unwrap();
    let source = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).retain_new_source(text_source(
            "loupist-a",
            "publisher-a",
            b"snapshot bytes",
        ))
    })
    .unwrap();
    let span_id = source.span_id.clone().unwrap();
    let world_id = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        let world = write.create_record(CreateRecord {
            loupist_id: "loupist-a".into(),
            kind: RecordKind::World,
            title: "World".into(),
            at_ms: 7,
        })?;
        write.seal_world(SealWorld {
            loupist_id: "loupist-a".into(),
            record_id: world.record_id.clone(),
            expected_revision_id: None,
            at_ms: 8,
            nodes: vec![node("n-alpha", "Alpha"), node("n-beta", "Beta")],
            claims: vec![
                claim("n-alpha", "Alpha preserves the statement.", Some(&span_id)),
                claim("n-beta", "Beta preserves the statement.", Some(&span_id)),
            ],
            annotations: Vec::new(),
            dependencies: vec![EvidenceRevision {
                record_id: source.record_id.clone(),
                revision_digest: source.revision_digest.clone(),
                role: DependencyRole::WorldSource,
            }],
        })?;
        Ok(world.record_id)
    })
    .unwrap();
    let lens = file_test_perspective(&state, "loupist-a", "Use the first method.");
    let charter = host_memory_context(&state, "loupist-a", "")
        .unwrap()
        .charter_delivery
        .unwrap();
    let mut source_request = empty_read(&source.record_id);
    source_request.scope = "source".into();
    source_request.span_id = Some(span_id.clone());
    let (_, source_read) = host_prepare_read(&state, "loupist-a", &source_request).unwrap();
    let mut world_request = empty_read(&world_id);
    world_request.scope = "world".into();
    world_request.center_node_id = Some("n-alpha".into());
    world_request.node_limit = Some(1);
    let (world_text, world_read) = host_prepare_read(&state, "loupist-a", &world_request).unwrap();
    assert!(world_text.contains("Alpha preserves the statement."));
    assert!(!world_text.contains("n-beta"));
    with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        let query: WorldGraphQuery = serde_json::from_value(serde_json::json!({
            "loupistId": "loupist-a", "recordId": world_id, "revisionId": world_read.revision_id,
            "centerNodeId": "n-alpha", "byteLimit": 8,
        }))?;
        assert!(matches!(
            world_view_with_sources(&KnowledgeWrite::bind(tx, blobs), query),
            Err(KnowledgeError::ItemTooLarge)
        ));
        // Later edits must not rewrite what this turn already received.
        let write = KnowledgeWrite::bind(tx, blobs);
        write.put_section(
            "loupist-a",
            &lens,
            "beliefs",
            "Use the edited method.",
            &crate::knowledge::test_support::user_origin(),
            12,
        )?;
        write.seal_world(SealWorld {
            loupist_id: "loupist-a".into(),
            record_id: world_id.clone(),
            expected_revision_id: Some(world_read.revision_id.clone()),
            at_ms: 12,
            nodes: Vec::new(),
            claims: Vec::new(),
            annotations: Vec::new(),
            dependencies: Vec::new(),
        })?;
        Ok(())
    })
    .unwrap();
    let next_charter = host_memory_context(&state, "loupist-a", "")
        .unwrap()
        .charter;
    assert_eq!(
        serde_json::from_str::<PageRead>(&next_charter.unwrap())
            .unwrap()
            .body
            .sections()[0]
            .markdown,
        "Use the edited method."
    );

    let blobs = knowledge_blobs(&state).unwrap();
    let reads = vec![source_read.clone(), world_read.clone(), charter.clone()];
    let cited = research(
        vec![source_read.read_ref.clone()],
        vec![world_read.read_ref.clone()],
        vec![charter.read_ref.clone()],
    );
    let changes = save_turn(
        &state.store,
        &blobs,
        "loupist-a",
        "chat-ack",
        "turn-ack",
        true,
        &[],
        &reads,
        Some(&cited),
    )
    .unwrap();
    let journal_id = changes
        .iter()
        .find(|change| change.kind == "Journal")
        .unwrap()
        .knowledge
        .as_ref()
        .unwrap()
        .object_id
        .clone();
    let journal = journal_read(
        &state,
        KnowledgeJournalReadRequest {
            loupist_id: "loupist-a".into(),
            journal_id: journal_id.clone(),
        },
    )
    .unwrap();
    assert!(journal.inputs.lenses[0]
        .markdown
        .contains("Use the first method."));
    assert!(!journal.inputs.lenses[0]
        .markdown
        .contains("Use the edited method."));
    assert_eq!(journal.inputs.worlds[0].delivered_view_json, world_text);
    assert!(journal.inputs.sources[0]
        .attribution
        .contains("publisher-a"));
    let receipt_id = journal.inputs.sources[0].receipt_id.clone();
    let source_text = |loupist_id: &str, receipt_id: &str| {
        journal_source_read(
            &state,
            KnowledgeJournalSourceReadRequest {
                loupist_id: loupist_id.into(),
                journal_id: journal_id.clone(),
                receipt_id: receipt_id.into(),
            },
        )
        .map(|read| read.text)
    };
    assert_eq!(
        source_text("loupist-a", &receipt_id).unwrap().as_deref(),
        Some("snapshot bytes")
    );
    assert!(source_text("loupist-b", &receipt_id).is_err());
    assert!(source_text("loupist-a", "rcp_00000000000000000000000000000000").is_err());

    // Citations need this turn's delivery and the owning Loupist.
    let save = |loupist_id: &str,
                turn_id: &str,
                reads: &[StagedKnowledgeRead],
                research: &StagedResearch| {
        save_turn(
            &state.store,
            &blobs,
            loupist_id,
            "chat-ack",
            turn_id,
            true,
            &[],
            reads,
            Some(research),
        )
    };
    assert!(save("loupist-a", "turn-unacked", &[], &cited).is_err());
    assert!(save("loupist-b", "turn-foreign", &reads, &cited).is_err());
    let analysis = StagedResearch {
        thesis_pair: None,
        journal: None,
        ..research(Vec::new(), Vec::new(), Vec::new())
    };
    save("loupist-a", "turn-analysis", &[], &analysis).unwrap();
    assert_eq!(
        count(&state.store, "SELECT COUNT(*) FROM knowledge_journals"),
        1
    );

    // Deleting the authoring Chat keeps the Journal's exact source bytes.
    state
        .store
        .delete_chat(&state.store.get_chat("chat-ack").unwrap().unwrap())
        .unwrap();
    assert_eq!(
        source_text("loupist-a", &receipt_id).unwrap().as_deref(),
        Some("snapshot bytes")
    );
}

#[tokio::test]
async fn source_catalog_lists_bounded_text_while_exact_reads_verify_the_blob() {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    state
        .store
        .create_loupist(&loupist_profile("loupist-a"))
        .unwrap();
    let original = "Visible source preview. ".repeat(4_000);
    let request = source_retain_request(
        KnowledgeSourceRetainRequest {
            loupist_id: "loupist-a".into(),
            title: "Large source".into(),
            origin_locator: "User text".into(),
            coverage: "included_full".into(),
            publication_time: None,
            text: original.clone(),
            file: None,
        },
        1_700_000_000_001,
    )
    .unwrap();
    let saved = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).retain_new_source(request)
    })
    .unwrap();
    let digest = crate::util::sha256(original.as_bytes());
    // Discovery must not open or hash the large retained file on every refresh.
    std::fs::remove_file(knowledge_blobs(&state).unwrap().path_for(&digest).unwrap()).unwrap();
    let page = catalog(
        &state,
        KnowledgeCatalogRequest {
            loupist_id: "loupist-a".into(),
        },
    )
    .unwrap();
    assert_eq!(
        page.sources[0].objects[0].summary.as_deref(),
        Some("Visible source preview…")
    );
    let span_id = saved.span_id.unwrap();
    with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        let text = write
            .source_catalog_text("loupist-a", &saved.record_id, &saved.revision_id, &span_id)?
            .unwrap();
        assert!(text.chars().count() <= 65_536);
        assert!(write
            .source_catalog_text("loupist-b", &saved.record_id, &saved.revision_id, &span_id)
            .is_err());
        assert!(write
            .read_revision_span_bytes(&saved.record_id, &span_id, &digest)
            .is_err());
        Ok(())
    })
    .unwrap();
}

#[test]
fn retained_file_survives_restart_with_original_bytes_and_guarded_export() {
    let directory = tempfile::tempdir().unwrap();
    let app_dir = directory.path().join("app");
    let original = b"%PDF-1.7 not extracted text";
    let state = AppState::for_persistent_test(app_dir.clone());
    for id in ["loupist-file", "loupist-other"] {
        state.store.create_loupist(&loupist_profile(id)).unwrap();
    }
    let file = KnowledgeSourceFile {
        media_type: "application/pdf".into(),
        file_name: "notes.pdf".into(),
        bytes_base64: base64::engine::general_purpose::STANDARD.encode(original),
    };
    let retain = |text: &str| {
        source_retain_request(
            KnowledgeSourceRetainRequest {
                loupist_id: "loupist-file".into(),
                title: "Diagram".into(),
                origin_locator: "selected file".into(),
                coverage: "included_full".into(),
                publication_time: None,
                text: text.into(),
                file: Some(KnowledgeSourceFile {
                    media_type: file.media_type.clone(),
                    file_name: file.file_name.clone(),
                    bytes_base64: file.bytes_base64.clone(),
                }),
            },
            1_700_000_000_001,
        )
    };
    assert!(retain("also pasted").is_err());
    let request = retain("").unwrap();
    let saved = with_loupist_knowledge(&state, "loupist-file", |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).retain_new_source(request)
    })
    .unwrap();
    let read_request = |loupist_id: &str, span_id: &str| KnowledgeSourceReadRequest {
        loupist_id: loupist_id.into(),
        record_id: saved.record_id.clone(),
        revision_id: saved.revision_id.clone(),
        source_id: Some(saved.source_id.clone()),
        span_id: Some(span_id.into()),
        purpose: "renderer_inspection".into(),
        world_record_id: None,
        world_revision_id: None,
        figure: None,
    };
    let span_id = saved.span_id.clone().unwrap();
    let before = source_read(&state, read_request("loupist-file", &span_id)).unwrap();
    assert_eq!(before.source_class, "user_file");
    assert_eq!(before.spans[0].media_type, "application/pdf");
    assert!(before.spans[0].text.is_none());
    assert!(
        super::source_export::export_bytes(&state, &read_request("loupist-other", &span_id))
            .is_err()
    );
    assert!(super::source_export::export_bytes(
        &state,
        &read_request("loupist-file", "forged-span")
    )
    .is_err());
    state.close_for_restart_test();

    let state = AppState::for_persistent_test(app_dir);
    let after = source_read(&state, read_request("loupist-file", &span_id)).unwrap();
    assert!(after.spans[0].text.is_none());
    let (exported, media_type, _) =
        super::source_export::export_bytes(&state, &read_request("loupist-file", &span_id))
            .unwrap();
    assert_eq!(exported, original);
    assert_eq!(media_type, "application/pdf");
}

#[test]
fn staged_source_refs_freeze_note_inputs_and_world_jobs_only_on_commit() {
    let directory = tempfile::tempdir().unwrap();
    let store = SqliteStore::open_in_memory().unwrap();
    store.create_loupist(&loupist_profile("loupist-a")).unwrap();
    let blobs = BlobStore::open(directory.path().join("knowledge-blobs")).unwrap();
    let settings = crate::knowledge::GraphProcessingSettings {
        enrichment_enabled: true,
        helper_provider: Some("fixture".into()),
        helper_model: Some("fixture".into()),
        ..Default::default()
    };
    crate::knowledge::store_settings(&store.lock().unwrap(), "loupist-a", &settings).unwrap();
    let source = staged("Original issuer claim, not the researcher's conclusion.");
    let note = StagedResearch {
        thesis_pair: None,
        source_reads: vec![source.request_ref.clone()],
        journal: None,
        working_note: Some(crate::knowledge::WorkingNoteDraft {
            title: "Claim check".into(),
            markdown: "This is interpretation, not an original source.".into(),
            change_summary: "Saved the first investigation.".into(),
            expected_research_id: None,
        }),
        ..research(Vec::new(), Vec::new(), Vec::new())
    };
    let persist = |turn_id: &str, sources: &[StagedSource], commit: bool| {
        let mut connection = store.lock().unwrap();
        let tx = connection.transaction().unwrap();
        let changes = persist_saved_turn(
            &tx,
            &blobs,
            "loupist-a",
            "chat-note",
            turn_id,
            100,
            &[],
            true,
            sources,
            &[],
            Some(&note),
        )
        .unwrap();
        let write = KnowledgeWrite::bind(&tx, &blobs);
        let notes = write.working_notes("loupist-a", "chat-note", 10).unwrap();
        if let Some(note) = notes.first() {
            assert!(write
                .read_working_note("loupist-b", &note.research_id)
                .is_err());
        }
        if commit {
            tx.commit().unwrap();
        }
        (changes, notes)
    };
    persist("turn-aborted", std::slice::from_ref(&source), false);
    for table in [
        "knowledge_sources",
        "knowledge_research_records",
        "knowledge_graph_outbox",
    ] {
        assert_eq!(count(&store, &format!("SELECT COUNT(*) FROM {table}")), 0);
    }
    let (changes, notes) = persist("turn-complete", &[source.clone(), source], true);
    assert_eq!(
        changes
            .iter()
            .filter(|change| change.kind == "Source")
            .count(),
        1
    );
    assert!(changes[0].world_job_id.is_some());
    let inputs = notes[0].inputs.as_ref().unwrap();
    assert_eq!(inputs.sources.len(), 1);
    assert!(!inputs.sources[0].revision_id.is_empty());
    assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_sources"), 1);
    assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_journals"), 0);
    assert_eq!(
        count(&store, "SELECT COUNT(*) FROM knowledge_graph_outbox"),
        1
    );
}

#[test]
fn lens_delivery_includes_every_section_and_large_base_requires_an_exact_read() {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    state
        .store
        .create_loupist(&loupist_profile("loupist-a"))
        .unwrap();
    let lens = file_test_perspective(&state, "loupist-a", "Base");
    let put = |key: &str, text: &str, at_ms: i64| {
        with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
            KnowledgeWrite::bind(tx, blobs).put_section(
                "loupist-a",
                &lens,
                key,
                text,
                &crate::knowledge::test_support::user_origin(),
                at_ms,
            )
        })
        .unwrap()
    };
    put("priorities", "Cyclical businesses: check utilization.", 91);
    put("avoid", "Disconfirm the thesis before sizing.", 92);
    let read = |lens: &str| {
        with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
            KnowledgeWrite::bind(tx, blobs).read_page("loupist-a", lens)
        })
        .unwrap()
    };
    let context = host_memory_context(&state, "loupist-a", "").unwrap();
    let delivered: PageRead = serde_json::from_str(context.charter.as_ref().unwrap()).unwrap();
    assert_eq!(
        context.charter.as_deref().unwrap(),
        read(&lens).delivery_text().unwrap()
    );
    assert_eq!(delivered.body.sections().len(), 3);
    assert!(context.charter_delivery.is_some());
    assert!(context.entries.iter().all(|entry| entry.id != lens));

    for index in 0..13 {
        put(
            &format!("more-{index}"),
            &"모든 조건을 다 읽어야 한다. ".repeat(110),
            100 + index,
        );
    }
    let context = host_memory_context(&state, "loupist-a", "").unwrap();
    assert!(context.charter_delivery.is_none());
    assert!(context.charter.as_ref().unwrap().contains("readRequired"));
    let (delivered, _) = host_prepare_read(&state, "loupist-a", &empty_read(&lens)).unwrap();
    assert_eq!(delivered, read(&lens).delivery_text().unwrap());
}

#[test]
fn world_pages_echo_the_requested_cursor_so_the_renderer_accepts_each_page() {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    state
        .store
        .create_loupist(&loupist_profile("loupist-a"))
        .unwrap();
    with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        let record = write.create_record(CreateRecord {
            loupist_id: "loupist-a".into(),
            kind: RecordKind::World,
            title: "Paginated World".into(),
            at_ms: 1,
        })?;
        let world = write.seal_world(SealWorld {
            loupist_id: "loupist-a".into(),
            record_id: record.record_id,
            expected_revision_id: None,
            at_ms: 2,
            nodes: (0..3)
                .map(|i| node(&format!("node-{i}"), &format!("Node {i}")))
                .collect(),
            claims: (0..3)
                .map(|i| claim(&format!("node-{i}"), &format!("Claim {i}"), None))
                .collect(),
            annotations: vec![],
            dependencies: vec![],
        })?;
        let mut query: WorldGraphQuery = serde_json::from_value(serde_json::json!({
            "loupistId": "loupist-a", "recordId": world.record_id, "revisionId": world.revision_id,
            "nodeLimit": 1,
        }))?;
        let mut seen = std::collections::BTreeSet::new();
        loop {
            let result = world_view_with_sources(&write, query.clone())?;
            assert_eq!(result.applied_cursor, query.cursor);
            assert_eq!(result.view.nodes.len(), 1);
            assert!(seen.insert(result.view.nodes[0].id.clone()));
            if result.view.next_cursor.is_none() {
                break;
            }
            query.cursor = result.view.next_cursor;
        }
        assert_eq!(seen.len(), 3);
        Ok(())
    })
    .unwrap();
}

#[test]
fn custom_connection_origin_is_retained_in_source_readers_catalog_and_map() {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    state
        .store
        .create_loupist(&loupist_profile("loupist-custom"))
        .unwrap();
    let origin = crate::custom_connections::taint::CustomOrigin {
        kind: "custom_connection".into(),
        connection_id: format!("custom-{}", "a".repeat(32)),
        name: "Company research".into(),
    };
    let saved = with_loupist_knowledge(&state, "loupist-custom", |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        let bytes = br#"{"company":"Example","revenue":123}"#;
        let saved = write.retain_new_source(RetainNewSource {
            origin: Some(origin.clone()),
            title: "Custom company report".into(),
            source_class: SourceClass::ToolResult,
            origin_locator: "tool:read_company".into(),
            provider_identity: format!("{}:read_company", origin.connection_id),
            authorship: Authorship::Tool,
            material: Some(RetainMaterial {
                document_id: "company-report".into(),
                media_type: "application/json".into(),
                role: RepresentationRole::ProviderPayload,
                bytes: bytes.to_vec(),
                selector: Selector::JsonPointer {
                    pointer: String::new(),
                },
                context_note: String::new(),
                parser_version: None,
                parent_representation_id: None,
                input_span_ids: vec![],
            }),
            ..text_source("loupist-custom", "unused", b"")
        })?;
        let world = write.create_record(CreateRecord {
            loupist_id: "loupist-custom".into(),
            kind: RecordKind::World,
            title: "Companies".into(),
            at_ms: 21,
        })?;
        write.seal_world(SealWorld {
            loupist_id: "loupist-custom".into(),
            record_id: world.record_id,
            expected_revision_id: None,
            at_ms: 22,
            nodes: vec![node("example-company", "Example")],
            claims: vec![claim(
                "example-company",
                "Revenue is 123",
                saved.span_id.as_deref(),
            )],
            annotations: vec![],
            dependencies: vec![],
        })?;
        let map = write.map(
            &crate::knowledge::KnowledgeMapQuery {
                loupist_id: "loupist-custom".into(),
                max_nodes: None,
            },
            &Default::default(),
        )?;
        assert_eq!(map.nodes[0].origin.as_ref(), Some(&origin));
        Ok(saved)
    })
    .unwrap();
    let detail = source_read(
        &state,
        KnowledgeSourceReadRequest {
            loupist_id: "loupist-custom".into(),
            record_id: saved.record_id,
            revision_id: saved.revision_id,
            source_id: Some(saved.source_id),
            span_id: saved.span_id,
            purpose: "renderer_inspection".into(),
            world_record_id: None,
            world_revision_id: None,
            figure: None,
        },
    )
    .unwrap();
    assert_eq!(detail.origin.as_ref(), Some(&origin));
    let catalog = catalog(
        &state,
        KnowledgeCatalogRequest {
            loupist_id: "loupist-custom".into(),
        },
    )
    .unwrap();
    assert_eq!(catalog.sources[0].objects[0].origin.as_ref(), Some(&origin));
}

mod controls;

#[test]
fn lens_reads_links_search_and_filing_stay_within_the_loupist() {
    use crate::knowledge::lens_page::{PageDelta, PageTarget};
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    for id in ["loupist-a", "loupist-b"] {
        state.store.create_loupist(&loupist_profile(id)).unwrap();
    }
    let private = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).file_section(
            "loupist-a",
            crate::knowledge::lens_page::PageKind::Method,
            "Private durable cash method",
            "steps",
            "PRIVATE_BODY_SHOULD_NOT_LEAK durable cash",
            &crate::knowledge::test_support::user_origin(),
            2,
        )
    })
    .unwrap();
    let request = |loupist: &str| KnowledgeLensReadRequest {
        loupist_id: loupist.into(),
        record_id: private.record_id.clone(),
        revision_id: None,
    };
    assert!(lens_read(&state, request("loupist-b")).is_err());
    assert!(lens_read(&state, request("loupist-a")).is_ok());
    let hits = lens_search(
        &state,
        KnowledgeLensSearchRequest {
            loupist_id: "loupist-b".into(),
            query: "durable cash".into(),
            kinds: vec![],
        },
    )
    .unwrap();
    assert!(hits.is_empty());
    let mine = lens_search(
        &state,
        KnowledgeLensSearchRequest {
            loupist_id: "loupist-a".into(),
            query: "durable cash".into(),
            kinds: vec![crate::knowledge::lens_page::LensKind::Method],
        },
    )
    .unwrap();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].record_id, private.record_id);
    let linked = with_loupist_knowledge(&state, "loupist-b", |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).file_section(
            "loupist-b",
            crate::knowledge::lens_page::PageKind::Method,
            "Borrowed",
            "steps",
            &format!("See [theirs](lens:{})", private.record_id),
            &crate::knowledge::test_support::user_origin(),
            3,
        )
    })
    .unwrap();
    assert!(linked.links.is_empty());
    assert!(!serde_json::to_string(&linked)
        .unwrap()
        .contains("PRIVATE_BODY"));
    let read_a = lens_read(&state, request("loupist-a")).unwrap();
    assert!(read_a.backlinks.is_empty());
    let refused = with_loupist_knowledge(&state, "loupist-b", |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).file(
            "loupist-b",
            &crate::knowledge::test_support::user_origin(),
            vec![PageDelta {
                target: PageTarget::Existing {
                    record_id: private.record_id.clone(),
                },
                ops: vec![crate::knowledge::lens_page::LensOp::Summary {
                    text: "Taken over".into(),
                }],
            }],
            4,
        )
    });
    assert!(refused.is_err());
    assert_eq!(
        lens_read(&state, request("loupist-a")).unwrap().revision_id,
        private.revision_id
    );
}

#[test]
fn removed_pack_sources_are_unreadable_unless_research_independently_retained_them() {
    use crate::sharing::{pack::License, store as packs};
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    state
        .store
        .create_loupist(&loupist_profile("loupist-a"))
        .unwrap();
    let (pack_only, researched) = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        let mut imported = text_source("loupist-a", "shared_pack:import-a", b"Pack reference");
        imported.source_class = SourceClass::ImportedContributor;
        imported.authorship = Authorship::ImportedContributor;
        let pack_only = write.retain_new_source(imported.clone())?;
        imported.material.as_mut().unwrap().bytes = b"Research reference".to_vec();
        imported.material.as_mut().unwrap().selector = Selector::ByteRange { start: 0, end: 18 };
        let researched = write.retain_new_source(imported)?;
        packs::insert_import(
            tx,
            "loupist-a",
            &packs::ImportRow {
                import_id: "import-a".into(),
                pack_id: "pack-a".into(),
                name: "Shared desk".into(),
                author: None,
                version: 1,
                license: License::Cc0,
                source_url: None,
                repo: None,
                path: None,
                commit: None,
                mode: packs::ImportMode::Merge,
                added_at_ms: 1,
                blocked_reason: None,
                origin: vec![],
                checked_at_ms: None,
            },
        )?;
        for source in [&pack_only, &researched] {
            packs::insert_world_ref(tx, "loupist-a", "import-a", "source", &source.record_id)?;
        }
        // The user's research retains a further capture of the same Source.
        let own = text_source("loupist-a", "publisher", b"Own retained research");
        let researched = write.retain_source(own.into_capture(
            researched.record_id.clone(),
            Some(researched.revision_id.clone()),
            Some(researched.source_id.clone()),
        ))?;
        Ok((pack_only, researched))
    })
    .unwrap();
    let request = |saved: &crate::knowledge::RetainedSource| KnowledgeSourceReadRequest {
        loupist_id: "loupist-a".into(),
        record_id: saved.record_id.clone(),
        revision_id: saved.revision_id.clone(),
        source_id: Some(saved.source_id.clone()),
        span_id: saved.span_id.clone(),
        purpose: "renderer_inspection".into(),
        world_record_id: None,
        world_revision_id: None,
        figure: None,
    };
    assert!(source_read(&state, request(&pack_only)).is_ok());
    with_loupist_knowledge(&state, "loupist-a", |tx, _| {
        packs::mark_removed(tx, "loupist-a", "import-a", 10)?;
        Ok(())
    })
    .unwrap();
    assert!(source_read(&state, request(&pack_only)).is_err());
    assert!(source_read(&state, request(&researched)).is_ok());
    let catalog = catalog(
        &state,
        KnowledgeCatalogRequest {
            loupist_id: "loupist-a".into(),
        },
    )
    .unwrap();
    assert!(!catalog
        .sources
        .iter()
        .any(|item| item.record_id == pack_only.record_id));
    assert!(catalog
        .sources
        .iter()
        .any(|item| item.record_id == researched.record_id));
    for (saved, readable) in [(&pack_only, false), (&researched, true)] {
        let read = HostReadRequest {
            scope: "source".into(),
            revision_id: Some(saved.revision_id.clone()),
            span_id: saved.span_id.clone(),
            ..empty_read(&saved.record_id)
        };
        assert_eq!(
            host_prepare_read(&state, "loupist-a", &read).is_ok(),
            readable
        );
    }
}
