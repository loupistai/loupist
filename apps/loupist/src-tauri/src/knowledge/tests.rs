use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::worker::adapt_worker_claim;
use super::*;
use crate::util::sha256;

const AT: i64 = 1_700_000_000_000;

fn open_store() -> (tempfile::TempDir, KnowledgeStore) {
    let directory = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).expect("open");
    (directory, store)
}

fn create(store: &KnowledgeStore, loupist_id: &str, kind: RecordKind, title: &str) -> OwnedRecord {
    store
        .create_record(CreateRecord {
            loupist_id: loupist_id.into(),
            kind,
            title: title.into(),
            at_ms: AT,
        })
        .unwrap()
}

fn method(store: &KnowledgeStore, loupist_id: &str, title: &str, markdown: &str) -> PageRead {
    store
        .file_method(
            loupist_id,
            title,
            markdown,
            &test_support::user_origin(),
            AT,
        )
        .unwrap()
}

fn section_text(page: &PageRead, key: &str) -> String {
    page.body
        .sections()
        .iter()
        .find(|section| section.key.as_str() == key)
        .map(|section| section.markdown.clone())
        .unwrap_or_default()
}

/// The app retains every acquisition as a new Loupist-owned Source.
fn source_request(loupist_id: &str, bytes: &[u8]) -> RetainNewSource {
    RetainNewSource {
        origin: None,
        loupist_id: loupist_id.into(),
        at_ms: AT,
        title: "QUIC specification excerpt".into(),
        source_class: SourceClass::PublisherDocument,
        origin_locator: "https://example.test/quic".into(),
        provider_identity: "example-publisher".into(),
        authorship: Authorship::Publisher,
        coverage: Coverage::IncludedFull,
        origin_chat_id: None,
        request_ref: None,
        declared_digest: None,
        publication_time: None,
        material: Some(RetainMaterial {
            media_type: "text/plain".into(),
            role: RepresentationRole::Original,
            document_id: "doc-quic".into(),
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
    }
}

fn retain_text(store: &KnowledgeStore, loupist_id: &str, bytes: &[u8]) -> RetainedSource {
    store
        .retain_new_source(source_request(loupist_id, bytes))
        .expect("retain")
}

fn read_span(store: &KnowledgeStore, source: &RetainedSource) -> KnowledgeResult<Vec<u8>> {
    store
        .read(|write| {
            write.read_revision_span_bytes(
                &source.record_id,
                source.span_id.as_deref().unwrap(),
                source.selected_digest.as_deref().unwrap(),
            )
        })
        .map(|(bytes, _media_type)| bytes)
}

fn node(id: &str, name: &str, kind: &str, properties: BTreeMap<String, Value>) -> GraphNode {
    GraphNode {
        id: id.to_string(),
        name: name.to_string(),
        aliases: Vec::new(),
        description: name.to_string(),
        kind: kind.to_string(),
        source_role: "described".to_string(),
        properties,
    }
}

fn claim(id: &str, proposition: &str, from: &str, to: &str) -> GraphClaim {
    GraphClaim {
        id: id.to_string(),
        proposition: proposition.to_string(),
        attribution: "specification".to_string(),
        relation: "depends_on".to_string(),
        endpoints: vec![
            ClaimEndpoint {
                node_id: from.to_string(),
                role: "source".to_string(),
            },
            ClaimEndpoint {
                node_id: to.to_string(),
                role: "target".to_string(),
            },
        ],
        qualifiers: Vec::new(),
        evidence: Vec::new(),
        status: ClaimStatus::Active,
        recorded_at_ms: Some(AT),
        publication_time: Some(time_at(TimePrecision::Unknown, None, false)),
        effective_time: None,
        valid_at: None,
        invalid_at: None,
        expired_at: None,
    }
}

fn time_at(precision: TimePrecision, value: Option<&str>, approximate: bool) -> TimePoint {
    TimePoint {
        precision,
        value: value.map(str::to_string),
        approximate,
    }
}

fn seal(
    store: &KnowledgeStore,
    loupist_id: &str,
    record_id: &str,
    expected: Option<String>,
    nodes: Vec<GraphNode>,
    claims: Vec<GraphClaim>,
) -> WorldRevision {
    store
        .seal_world(SealWorld {
            loupist_id: loupist_id.to_string(),
            record_id: record_id.to_string(),
            expected_revision_id: expected,
            at_ms: AT,
            nodes,
            claims,
            annotations: vec![TopicAnnotation {
                topic_id: "topic-main".to_string(),
                markdown: "A bounded explanation of the sealed snapshot.".to_string(),
                source_span_ids: Vec::new(),
            }],
            dependencies: Vec::new(),
        })
        .expect("seal")
}

/// Each Loupist owns exactly one World record.
fn new_world(
    store: &KnowledgeStore,
    loupist_id: &str,
    nodes: Vec<GraphNode>,
    claims: Vec<GraphClaim>,
) -> WorldRevision {
    let record = create(store, loupist_id, RecordKind::World, "World");
    seal(store, loupist_id, &record.record_id, None, nodes, claims)
}

fn world_query(loupist_id: &str, revision: &WorldRevision) -> WorldGraphQuery {
    serde_json::from_value(json!({
        "loupistId": loupist_id, "recordId": revision.record_id, "revisionId": revision.revision_id,
    }))
    .unwrap()
}

#[test]
fn concurrent_section_edits_from_one_read_keep_one_winner_and_skip_the_stale_writer() {
    let (_directory, store) = open_store();
    let original = method(&store, "loupist-a", "Shared method", "Original");
    let digest = original.body.sections()[0].digest.clone();
    let barrier = std::sync::Barrier::new(2);
    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = ["First revision", "Second revision"]
            .into_iter()
            .map(|body| {
                let (store, barrier, digest, record_id) =
                    (&store, &barrier, digest.clone(), original.record_id.clone());
                scope.spawn(move || {
                    barrier.wait();
                    store.transact(|write| {
                        write.file(
                            "loupist-a",
                            &test_support::user_origin(),
                            vec![lens_page::PageDelta {
                                target: lens_page::PageTarget::Existing { record_id },
                                ops: vec![lens_page::LensOp::Put {
                                    key: lens_page::SectionKey::new("steps").unwrap(),
                                    heading: "STEPS".into(),
                                    markdown: body.into(),
                                    basis: lens_page::Basis::default(),
                                    expected: Some(digest),
                                }],
                            }],
                            AT + 1,
                        )
                    })
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap().unwrap().remove(0))
            .collect()
    });
    let landed: Vec<_> = results.iter().filter(|page| page.changed).collect();
    assert_eq!(landed.len(), 1);
    assert!(results.iter().any(|page| matches!(
        page.outcomes[0].outcome,
        lens_page::OpOutcome::SkippedStale { .. }
    )));
    let current = store.read_page("loupist-a", &original.record_id).unwrap();
    assert_eq!(current.revision_id, landed[0].revision_id);
    assert!(
        ["First revision", "Second revision"].contains(&section_text(&current, "steps").as_str())
    );
}

#[test]
fn lens_reads_and_writes_are_owned_by_one_loupist() {
    let (_directory, store) = open_store();
    let saved = method(
        &store,
        "loupist-a",
        "Compare hypotheses",
        "state hypotheses",
    );
    let read = store.read_page("loupist-b", &saved.record_id).unwrap_err();
    assert_eq!(read.code(), "unauthorized");
    assert!(store.list_records("loupist-b").unwrap().is_empty());
    let foreign = store.transact(|write| {
        write.put_section(
            "loupist-b",
            &saved.record_id,
            "steps",
            "foreign",
            &test_support::user_origin(),
            AT + 1,
        )
    });
    assert_eq!(foreign.unwrap_err().code(), "unauthorized");
    let current = store.read_page("loupist-a", &saved.record_id).unwrap();
    assert_eq!(section_text(&current, "steps"), "state hypotheses");
}

#[test]
fn lens_keeps_private_references_out_of_its_payload() {
    let (_directory, store) = open_store();
    let world = new_world(&store, "loupist-a", Vec::new(), Vec::new());
    let journal = store
        .transact(|write| {
            journal::create(
                write.tx,
                "loupist-a",
                journal::JournalInput {
                    judgment: "A private judgment.".into(),
                    reasoning: String::new(),
                    entities: Vec::new(),
                    check: None,
                    thesis: None,
                },
                AT,
            )
        })
        .unwrap();
    for text in [
        format!("compare accounts of {}", world.snapshot_id),
        format!("see {}", journal.journal_id),
        format!("open [world]({})", world.record_id),
    ] {
        let refused = store.file_method(
            "loupist-a",
            "Method",
            &text,
            &test_support::user_origin(),
            AT,
        );
        assert_eq!(refused.unwrap_err().code(), "lens_body_forbidden", "{text}");
    }
    assert!(store
        .list_records("loupist-a")
        .unwrap()
        .iter()
        .all(|r| r.kind != RecordKind::Lens));
}

#[test]
fn journal_freezes_acknowledged_inputs_after_world_and_lens_updates() {
    let (_directory, store) = open_store();
    let bytes = b"retained specification";
    let source = retain_text(&store, "loupist-a", bytes);
    let quic = || {
        vec![
            node("quic", "QUIC", "technology:Protocol", BTreeMap::new()),
            node("stream", "Stream", "concept", BTreeMap::new()),
        ]
    };
    let sealed = new_world(
        &store,
        "loupist-a",
        quic(),
        vec![claim(
            "claim-old",
            "QUIC multiplexes streams",
            "quic",
            "stream",
        )],
    );
    let method = method(&store, "loupist-a", "Method", "compare accounts");
    // The same acknowledgement writes the app performs when a turn saves its journal.
    let (journal_id, receipt) = store
        .transact(|write| {
            write.open_turn(OpenTurn {
                loupist_id: "loupist-a".into(),
                chat_session_id: "chat-7".into(),
                turn_id: "turn-7".into(),
                at_ms: AT,
            })?;
            let receipt = write.record_acked_source(
                "loupist-a",
                "chat-7",
                "turn-7",
                AT,
                &source.record_id,
                source.span_id.as_deref().unwrap(),
                &source.capture_id,
                source.representation_id.as_deref().unwrap(),
                source.selected_digest.as_deref().unwrap(),
                &source.revision_id,
                &source.revision_digest,
                "example-publisher",
                "included_full",
                None,
                "text/plain",
                bytes,
            )?;
            let view = write.query_world(world_query("loupist-a", &sealed))?;
            let view_json = serde_json::to_string(&view)?;
            let world_delivery = write.record_acked_world(
                "loupist-a",
                "turn-7",
                AT,
                &sealed.record_id,
                &sealed.revision_id,
                &sealed.revision_digest,
                &view_json,
                &sha256(view_json.as_bytes()),
            )?;
            let document_json = method.delivery_text()?;
            let lens_delivery = write.record_acked_lens(
                "loupist-a",
                "turn-7",
                AT,
                &method.record_id,
                &method.revision_id,
                &method.revision_digest,
                &document_json,
                &sha256(document_json.as_bytes()),
            )?;
            let saved = write.save_research(SaveResearch {
                note_inputs: None,
                working_note: None,
                loupist_id: "loupist-a".into(),
                turn_id: "turn-7".into(),
                at_ms: AT,
                observations: "Read the specification in this turn.".into(),
                interpretation: String::new(),
                assumptions: String::new(),
                evaluation: String::new(),
                journal: Some(JournalDraft {
                    thesis: None,
                    check: Some(journal::fixture_prediction()),
                    outcome: None,
                    judgment: "QUIC multiplexes streams.".into(),
                    reasoning: "The sealed claim and the exact span agree.".into(),
                    source_receipt_ids: vec![receipt.clone()],
                    world_delivery_ids: vec![world_delivery],
                    lenses: vec![LensAcknowledgement {
                        delivery_id: lens_delivery,
                        status: LensUseStatus::Applied,
                    }],
                    earlier: Vec::new(),
                    entities: Vec::new(),
                }),
            })?;
            Ok((saved.journal.unwrap().journal_id, receipt))
        })
        .unwrap();
    seal(
        &store,
        "loupist-a",
        &sealed.record_id,
        Some(sealed.revision_id.clone()),
        quic(),
        vec![claim(
            "claim-new",
            "QUIC was replaced later",
            "quic",
            "stream",
        )],
    );
    store
        .transact(|write| {
            write.put_section(
                "loupist-a",
                &method.record_id,
                "steps",
                "a later method body",
                &test_support::user_origin(),
                AT + 1,
            )
        })
        .unwrap();

    let journal = store.get_journal("loupist-a", &journal_id).unwrap();
    let world_input = &journal.inputs.worlds[0].edges[0].claim;
    assert_eq!(world_input.proposition, "QUIC multiplexes streams");
    assert_eq!(
        journal.inputs.lenses[0].revision_digest,
        method.revision_digest
    );
    assert!(journal.inputs.lenses[0]
        .markdown
        .contains("compare accounts"));
    assert!(!journal.inputs.lenses[0]
        .markdown
        .contains("a later method body"));
    assert_eq!(journal.inputs.sources[0].revision_id, source.revision_id);
    let hidden = store.get_journal("loupist-b", &journal_id).unwrap_err();
    assert_eq!(hidden.code(), "unauthorized");
    let (read, _media_type, _pinned) = store
        .transact(|write| write.read_journal_source("loupist-a", &journal_id, &receipt))
        .unwrap();
    assert_eq!(read, bytes);

    let by_claim = |loupist: &str, claim: &str| {
        store
            .research_history(loupist, "", Some(claim), Some(40))
            .unwrap()
            .hits
    };
    let pinned = by_claim("loupist-a", "claim-old");
    assert_eq!(pinned[0].journal_id.as_deref(), Some(journal_id.as_str()));
    assert!(by_claim("loupist-a", "claim-new").is_empty());
    assert!(by_claim("loupist-b", "claim-old").is_empty());

    store
        .transact(|write| {
            let update = "UPDATE knowledge_receipt_snapshots SET media_type = 'text/x-test'";
            assert!(write.tx.execute(update, []).is_err());
            erase_loupist_private(write.tx, "loupist-a")?;
            let remaining: i64 = write.tx.query_row(
                "SELECT COUNT(*) FROM knowledge_receipt_snapshots",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(remaining, 0);
            Ok(())
        })
        .unwrap();
}

#[test]
fn world_query_pages_searches_focuses_and_keeps_claim_details() {
    let (_directory, store) = open_store();
    let nodes = vec![
        node("battery", "Battery chemistry", "concept", BTreeMap::new()),
        node(
            "quic",
            "QUIC",
            "technology:Protocol",
            BTreeMap::from([("flags".into(), json!([true, {"nested": 1}]))]),
        ),
        node(
            "treaty",
            "Example treaty",
            "geopolitics:Treaty",
            BTreeMap::from([("region".into(), json!("europe"))]),
        ),
    ];
    let mut detailed = claim(
        "claim-link",
        "The treaty names a standard",
        "treaty",
        "quic",
    );
    detailed.qualifiers = vec![Qualifier {
        key: "unit".into(),
        value: "ms".into(),
    }];
    detailed.valid_at = Some(time_at(TimePrecision::Date, Some("2020-01-02"), false));
    detailed.invalid_at = Some(time_at(TimePrecision::Date, Some("2021-03-04"), false));
    detailed.expired_at = Some(time_at(
        TimePrecision::Instant,
        Some("2024-05-06T07:08:09+09:00"),
        false,
    ));
    detailed.effective_time = Some(time_at(TimePrecision::Month, Some("2019-06"), true));
    let claims = vec![
        detailed.clone(),
        claim("claim-next", "Battery chemistry follows", "battery", "quic"),
    ];
    let forward = new_world(&store, "loupist-a", nodes.clone(), claims.clone());
    let mut reversed = nodes;
    reversed.reverse();
    let backward = new_world(&store, "loupist-b", reversed, claims);
    assert_eq!(forward.revision_digest, backward.revision_digest);
    assert_ne!(forward.record_id, backward.record_id);

    let mut paged = world_query("loupist-a", &forward);
    paged.edge_limit = Some(1);
    let page = store.query_world(paged.clone()).unwrap();
    assert_eq!((page.edges.len(), page.truncated), (1, true));
    paged.cursor = page.next_cursor.clone();
    let next = store.query_world(paged).unwrap();
    assert_ne!(next.edges[0].id, page.edges[0].id);
    let mut stale = world_query("loupist-b", &backward);
    stale.cursor = page.next_cursor;
    assert_eq!(store.query_world(stale).unwrap_err().code(), "cursor_stale");

    let mut search = world_query("loupist-a", &forward);
    search.search = "Treaty".into();
    let found = store.query_world(search.clone()).unwrap();
    assert_eq!(found.edges.len(), 1);
    assert_eq!(found.edges[0].claim, detailed);
    let treaty = found.nodes.iter().find(|node| node.id == "treaty").unwrap();
    assert_eq!(treaty.properties["region"], "europe");

    // An exact claim focus ignores browse filters.
    search.center_claim_id = Some("claim-next".into());
    search.statuses = vec![ClaimStatus::Withdrawn];
    let focused = store.query_world(search.clone()).unwrap();
    assert_eq!(focused.edges.len(), 1);
    assert_eq!(focused.edges[0].id, "claim-next");
    search.center_claim_id = Some("missing".into());
    assert!(matches!(
        store.query_world(search),
        Err(KnowledgeError::NotFound)
    ));
    let mut tiny = world_query("loupist-a", &forward);
    tiny.byte_limit = Some(1);
    assert_eq!(
        store.query_world(tiny).unwrap_err().code(),
        "item_too_large"
    );
}

#[test]
fn world_reads_obey_revision_pins_and_loupist_ownership() {
    let (_directory, store) = open_store();
    let nodes = || {
        vec![
            node("subject", "Subject", "concept", BTreeMap::new()),
            node("object", "Object", "concept", BTreeMap::new()),
        ]
    };
    let first = new_world(
        &store,
        "loupist-a",
        nodes(),
        vec![claim("claim-old", "first claim", "subject", "object")],
    );
    let duplicate = store.create_record(CreateRecord {
        loupist_id: "loupist-a".into(),
        kind: RecordKind::World,
        title: "Second World".into(),
        at_ms: AT,
    });
    assert!(matches!(duplicate, Err(KnowledgeError::Invalid(_))));
    let current = seal(
        &store,
        "loupist-a",
        &first.record_id,
        Some(first.revision_id.clone()),
        nodes(),
        vec![claim("claim-new", "later claim", "subject", "object")],
    );
    let source_revision = retain_text(&store, "loupist-a", b"other original").revision_id;
    let other_world = new_world(&store, "loupist-b", nodes(), Vec::new());
    for (loupist, revision_id, code) in [
        ("loupist-a", first.revision_id.clone(), "not_found"),
        ("loupist-b", current.revision_id.clone(), "unauthorized"),
        ("loupist-a", source_revision, "unauthorized"),
        ("loupist-a", other_world.revision_id, "unauthorized"),
    ] {
        let mut query = world_query(loupist, &current);
        query.revision_id = revision_id.clone();
        let read = store.query_world(query).unwrap_err();
        assert_eq!(read.code(), code, "query {loupist} {revision_id}");
    }
    assert!(store
        .query_world(world_query("loupist-a", &current))
        .is_ok());
}

#[test]
fn source_retention_rejects_invalid_material_and_credentials_and_rolls_back() {
    let (_directory, store) = open_store();
    let mut multi = source_request("loupist-a", b"one");
    multi.material.as_mut().unwrap().selector = Selector::Multi {
        documents: vec!["doc-a".into(), "doc-b".into()],
    };
    let mut wrong_part = source_request("loupist-a", b"alpha beta");
    wrong_part.material.as_mut().unwrap().selector = Selector::Part {
        document_id: "doc-other".into(),
        part_id: "part-1".into(),
        start: 0,
        end: 5,
    };
    let mut credential = source_request("loupist-a", b"private page");
    credential.origin_locator = "https://user:password@example.test/private".into();
    for request in [multi, wrong_part, credential] {
        let error = store.retain_new_source(request).unwrap_err();
        assert_eq!(error.code(), "invalid");
    }
    let rolled_back = store.transact(|write| -> KnowledgeResult<()> {
        write.retain_new_source(source_request("loupist-a", b"temporary"))?;
        Err(KnowledgeError::Invalid("abort"))
    });
    assert!(rolled_back.is_err());
    assert!(store.list_records("loupist-a").unwrap().is_empty());
}

#[test]
fn new_sources_deduplicate_per_loupist_and_reject_a_redirected_index() {
    let (_directory, store) = open_store();
    let request = source_request("loupist-a", b"Identical words");
    let first = store.retain_new_source(request.clone()).unwrap();
    let mut repeated = request.clone();
    repeated.at_ms += 1;
    repeated.title = "Different display title".into();
    repeated.origin_chat_id = Some("second-chat".into());
    repeated.request_ref = Some("tool-result-2".into());
    assert_eq!(store.retain_new_source(repeated).unwrap(), first);

    let mut another_origin = request.clone();
    another_origin.origin_locator = "https://example.test/second".into();
    let mut excerpt = request.clone();
    excerpt.coverage = Coverage::IncludedExcerpt;
    excerpt.material.as_mut().unwrap().role = RepresentationRole::Excerpt;
    let mut markdown = request.clone();
    markdown.material.as_mut().unwrap().media_type = "text/markdown".into();
    let mut metadata = request.clone();
    metadata.material = None;
    metadata.coverage = Coverage::MetadataOnly;
    let distinct: Vec<_> = [another_origin, excerpt, markdown, metadata.clone()]
        .into_iter()
        .map(|variant| store.retain_new_source(variant).unwrap())
        .collect();
    assert!(distinct
        .iter()
        .all(|saved| saved.record_id != first.record_id));
    assert_eq!(store.retain_new_source(metadata).unwrap(), distinct[3]);
    assert_eq!(store.list_records("loupist-a").unwrap().len(), 5);

    let other_loupist = RetainNewSource {
        loupist_id: "loupist-b".into(),
        ..request.clone()
    };
    assert_ne!(
        store.retain_new_source(other_loupist).unwrap().record_id,
        first.record_id
    );
    let foreign = store.read(|write| write.owned_record("loupist-b", &first.record_id));
    assert_eq!(foreign.unwrap_err().code(), "unauthorized");

    // A restored database whose dedup index points at another capture cannot
    // redirect the original acquisition.
    store
        .transact(|write| {
            let identity: String = write.tx.query_row(
                "SELECT acquisition_identity FROM knowledge_source_captures WHERE record_id = ?1",
                [&first.record_id],
                |row| row.get(0),
            )?;
            write
                .tx
                .execute_batch("DROP TRIGGER knowledge_source_captures_no_update")?;
            write.tx.execute(
                "UPDATE knowledge_source_captures SET acquisition_identity = NULL WHERE record_id = ?1",
                [&first.record_id],
            )?;
            write.tx.execute(
                "UPDATE knowledge_source_captures SET acquisition_identity = ?1 WHERE record_id = ?2",
                rusqlite::params![identity, distinct[0].record_id],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        store.retain_new_source(request),
        Err(KnowledgeError::DigestMismatch)
    ));
}

#[test]
fn retained_source_survives_chat_deletion_and_reopen_and_detects_tampered_bytes() {
    let (directory, store) = open_store();
    let mut request = source_request("loupist-a", b"hello world");
    request.origin_chat_id = Some("chat-1".into());
    request.material.as_mut().unwrap().selector = Selector::ByteRange { start: 6, end: 11 };
    let retained = store.retain_new_source(request).unwrap();
    let search = |needle: &str| {
        store
            .read(|write| {
                write.prepare_search("loupist-a", "source", needle, 20, &SearchOptions::default())
            })
            .unwrap()
            .rank(None)
            .documents
    };
    let hits = search("world");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].span_id, retained.span_id.clone().unwrap());
    assert!(search("hello").is_empty());

    let deleted = store.transact(|write| note_chat_deleted_in(write.tx, "loupist-a", "chat-1"));
    assert_eq!(deleted.unwrap(), 1);
    drop(store);
    let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
    assert_eq!(read_span(&store, &retained).unwrap(), b"world");

    let digest = retained.material_digest.clone().unwrap();
    let blob = directory
        .path()
        .join("knowledge-blobs/sha256")
        .join(&digest[..2])
        .join(&digest);
    std::fs::write(blob, b"tampered").unwrap();
    assert_eq!(
        read_span(&store, &retained).unwrap_err().code(),
        "digest_mismatch"
    );
}

#[test]
fn loupist_erasure_removes_derived_representation_chains_and_preserves_other_loupists() {
    let (_directory, store) = open_store();
    let original = retain_text(&store, "loupist-a", b"original");
    let other = retain_text(&store, "loupist-b", b"original");
    // Fixtures and evals append derived captures to an existing Source.
    let mut derived = source_request("loupist-a", b"normalized").into_capture(
        original.record_id.clone(),
        Some(original.revision_id.clone()),
        Some(original.source_id.clone()),
    );
    let material = derived.material.as_mut().unwrap();
    material.role = RepresentationRole::Derived;
    material.parent_representation_id = original.representation_id.clone();
    material.input_span_ids = vec![original.span_id.clone().unwrap()];
    store.retain_source(derived).unwrap();

    let erased = store
        .transact(|write| erase_loupist_private(write.tx, "loupist-a"))
        .unwrap();
    assert_eq!(erased, 1);
    assert!(store.list_records("loupist-a").unwrap().is_empty());
    assert_eq!(read_span(&store, &other).unwrap(), b"original");
    store
        .read(|write| {
            let count = |sql: &str| write.tx.query_row(sql, [], |row| row.get::<_, i64>(0));
            assert_eq!(
                count("SELECT COUNT(*) FROM knowledge_source_representations")?,
                1
            );
            assert_eq!(
                count("SELECT armed FROM knowledge_erasure WHERE id = 1")?,
                0
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn world_source_links_resolve_both_directions_for_the_owner_only() {
    let (_directory, store) = open_store();
    let mut dependencies = Vec::new();
    let mut claims = Vec::new();
    let mut sources = Vec::new();
    for index in 0..3 {
        let source = retain_text(&store, "loupist-a", format!("Excerpt {index}").as_bytes());
        dependencies.push(EvidenceRevision {
            record_id: source.record_id.clone(),
            revision_digest: source.revision_digest.clone(),
            role: DependencyRole::WorldSource,
        });
        let mut edge = claim(&format!("claim-{index}"), "A supplies B", "a", "b");
        edge.evidence.push(EvidenceBinding {
            span_id: source.span_id.clone().unwrap(),
            role: BindingRole::ExtractedFrom,
            attribution: format!("Source {index}"),
        });
        claims.push(edge);
        sources.push(source);
    }
    let record = create(&store, "loupist-a", RecordKind::World, "World");
    let mut request = SealWorld {
        loupist_id: "loupist-a".into(),
        record_id: record.record_id,
        expected_revision_id: None,
        at_ms: AT,
        nodes: vec![
            node("a", "A", "company", BTreeMap::new()),
            node("b", "B", "company", BTreeMap::new()),
        ],
        claims,
        annotations: Vec::new(),
        dependencies,
    };
    let world = store.seal_world(request.clone()).unwrap();
    let spans: Vec<_> = sources
        .iter()
        .map(|source| source.span_id.clone().unwrap())
        .collect();
    let targets = |loupist: &str, spans: &[String]| {
        store.read(|write| {
            write.world_source_targets(loupist, &world.record_id, &world.revision_id, spans)
        })
    };
    let resolved = targets("loupist-a", &spans).unwrap();
    for source in &sources {
        let target = &resolved[source.span_id.as_ref().unwrap()];
        assert_eq!(
            (&target.record_id, &target.revision_id),
            (&source.record_id, &source.revision_id)
        );
    }
    assert!(targets("loupist-b", &spans[..1]).is_err());
    assert!(targets("loupist-a", &["spn_not_cited".into()]).is_err());
    assert!(targets("loupist-a", &vec![spans[0].clone(); 257]).is_err());

    let related = |loupist: &str, revision_id: &str| {
        let source = &sources[0];
        store.read(|write| {
            write.source_related_claims(loupist, &source.record_id, revision_id, &source.source_id)
        })
    };
    let first = related("loupist-a", &sources[0].revision_id).unwrap();
    assert_eq!(
        (
            first[0].claim_id.as_str(),
            first[0].world_revision_id.as_str()
        ),
        ("claim-0", world.revision_id.as_str())
    );
    request.expected_revision_id = Some(world.revision_id.clone());
    request.claims[0].proposition = "A supplies B after qualification".into();
    let later = store.seal_world(request).unwrap();
    let current = related("loupist-a", &sources[0].revision_id).unwrap();
    assert_eq!(current[0].world_revision_id, later.revision_id);
    assert!(current[0].proposition.ends_with("after qualification"));
    assert!(related("loupist-b", &sources[0].revision_id).is_err());
    assert!(related("loupist-a", &later.revision_id).is_err());
}

#[test]
fn canonical_json_time_points_and_worker_claims_are_validated_at_the_host() {
    let canonical = super::canonical::canonicalize_json_text(r#"{"n":1e2,"city":"서울"}"#).unwrap();
    let parsed: Value = serde_json::from_str(&canonical).unwrap();
    assert_eq!(parsed["n"].as_f64(), Some(100.0));
    assert_eq!(parsed["city"], "서울");
    for (precision, value, expected) in [
        (TimePrecision::Date, "2021-02-29", None),
        (TimePrecision::Date, "2020-02-29", Some("2020-02-29")),
        (
            TimePrecision::Instant,
            "2020-01-02T03:04:05Z",
            Some("2020-01-02T03:04:05+00:00"),
        ),
        (TimePrecision::Instant, "2020-01-02T03:04:05", None),
    ] {
        let validated = validate_time_point(&time_at(precision, Some(value), false));
        assert_eq!(
            validated.ok().and_then(|point| point.value).as_deref(),
            expected,
            "{value}"
        );
    }
    let mut worker = json!({
        "adapter_id": "clm_example",
        "proposition": "A fact",
        "attribution": "graphiti",
        "relation": "RELATES_TO",
        "endpoints": [
            {"node_id": "subject", "role": "source"},
            {"node_id": "object", "role": "target"}
        ],
        "qualifiers": [],
        "evidence": [
            {"span_id": "spn_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "role": "supports", "attribution": "one"},
            {"span_id": "spn_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "role": "opposes", "attribution": "two"}
        ],
        "status": "active",
        "recorded_at_ms": 10,
        "publication_time": {"precision": "year", "value": "2020", "approximate": false},
        "effective_time": null,
        "valid_at": {"precision": "date", "value": "2020-01-02", "approximate": false},
        "invalid_at": {"precision": "date", "value": "2021-01-02", "approximate": false},
        "expired_at": {"precision": "instant", "value": "2024-01-02T00:00:00Z", "approximate": false},
    });
    let adapted = adapt_worker_claim(&worker).unwrap();
    assert_eq!(adapted.evidence[1].role, BindingRole::Opposes);
    assert_ne!(adapted.invalid_at, adapted.expired_at);
    worker["endpoints"] = json!(["subject", "object"]);
    assert!(adapt_worker_claim(&worker).is_err());
}

#[test]
fn blob_publication_rejects_symlink_parents_and_unverifiable_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let blobs = BlobStore::open(directory.path().join("blobs")).unwrap();
    let outside = directory.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, directory.path().join("blobs").join("sha256")).unwrap();
    let digest = sha256(b"secret");
    let redirected = blobs.write_file(&digest, b"secret").unwrap_err();
    assert_eq!(redirected.code(), "invalid");
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
    std::fs::remove_file(directory.path().join("blobs").join("sha256")).unwrap();
    blobs.write_file(&digest, b"secret").unwrap();
    let path = blobs.path_for(&digest).unwrap();
    std::fs::write(&path, b"corrupt").unwrap();
    let mismatch = blobs.write_file(&digest, b"secret").unwrap_err();
    assert_eq!(mismatch.code(), "digest_mismatch");
    assert_eq!(std::fs::read(&path).unwrap(), b"corrupt");
    let planted = path.parent().unwrap().join(".planted.partial");
    std::fs::write(&planted, b"leave-me").unwrap();
    let other = sha256(b"other-bytes");
    if other[..2] == digest[..2] {
        blobs.write_file(&other, b"other-bytes").unwrap();
    } else {
        blobs.write_file(&digest, b"secret").unwrap_err();
    }
    assert_eq!(std::fs::read(&planted).unwrap(), b"leave-me");
    std::fs::remove_file(&path).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| blobs.write_file(&digest, b"secret").unwrap());
        }
    });
    assert_eq!(blobs.read(&digest).unwrap(), b"secret");
}

#[test]
fn world_size_caps_are_enforced() {
    let (_directory, store) = open_store();
    let record = create(&store, "loupist-cap", RecordKind::World, "Too many nodes");
    let too_many = (0..=MAX_WORLD_NODES)
        .map(|index| node(&format!("n{index}"), "n", "concept", BTreeMap::new()))
        .collect();
    let refused = store.seal_world(SealWorld {
        loupist_id: "loupist-cap".into(),
        record_id: record.record_id,
        expected_revision_id: None,
        at_ms: AT,
        nodes: too_many,
        claims: Vec::new(),
        annotations: Vec::new(),
        dependencies: Vec::new(),
    });
    assert!(refused.is_err());
}

#[test]
fn lens_wiki_links_are_owned_follow_current_heads_and_exclude_code() {
    let (_directory, store) = open_store();
    let b = method(&store, "loupist-a", "B", "An independent method");
    let foreign = method(&store, "loupist-b", "Private method", "Private content");
    let source = create(&store, "loupist-a", RecordKind::Source, "Source");
    let a = method(
        &store,
        "loupist-a",
        "A",
        &format!(
            "[B](lens:{}) [Foreign](lens:{}) [Source](lens:{}) [Missing](lens:missing-method)",
            b.record_id, foreign.record_id, source.record_id
        ),
    );
    let code = method(
        &store,
        "loupist-a",
        "Code examples",
        &format!(
            "`[A](lens:{0})`\n\n```md\n[A](lens:{0})\n```\n\n![image](lens:{0})",
            a.record_id
        ),
    );
    let ids = |links: &[crate::knowledge::lens::LensLinkView]| {
        links
            .iter()
            .map(|link| link.record_id.clone())
            .collect::<Vec<_>>()
    };
    let read_a = store.read_page("loupist-a", &a.record_id).unwrap();
    assert_eq!(ids(&read_a.links), std::slice::from_ref(&b.record_id));
    assert!(read_a.backlinks.is_empty());
    assert!(!serde_json::to_string(&read_a)
        .unwrap()
        .contains("Private method"));
    let read_b = store.read_page("loupist-a", &b.record_id).unwrap();
    assert_eq!(ids(&read_b.backlinks), std::slice::from_ref(&a.record_id));
    assert!(store
        .read_page("loupist-a", &code.record_id)
        .unwrap()
        .links
        .is_empty());
    assert_eq!(
        store
            .read_page("loupist-b", &a.record_id)
            .unwrap_err()
            .code(),
        "unauthorized"
    );
    store
        .transact(|write| {
            write.put_section(
                "loupist-a",
                &a.record_id,
                "steps",
                "No outgoing links",
                &test_support::user_origin(),
                AT + 1,
            )
        })
        .unwrap();
    assert!(store
        .read_page("loupist-a", &b.record_id)
        .unwrap()
        .backlinks
        .is_empty());
}

#[test]
fn lens_unresolved_links_stay_in_the_text_without_entering_the_link_index() {
    let (_directory, store) = open_store();
    let markdown = (0..40)
        .map(|i| format!("[Topic {i}](lens:missing-{i})"))
        .collect::<Vec<_>>()
        .join("\n");
    let saved = method(&store, "loupist-a", "Many topics", &markdown);
    assert!(saved.links.is_empty());
    assert_eq!(section_text(&saved, "steps"), markdown);
}

mod growth;

mod memory_reset;
