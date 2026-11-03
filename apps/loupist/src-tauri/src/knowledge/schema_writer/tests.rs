use super::super::graph_inference::{InferenceDisposition, InferenceReply};
use super::super::graph_jobs::{ExtractionBudget, JobDocument, JOB_RUNNING};
use super::super::world_schema::{self, ExtensionField, ProposalKind, ProposalStatus};
use super::super::*;
use super::*;
use rusqlite::params;

const LOUPIST: &str = "loupist-schema";
const AT: i64 = 1_800_000_000_000;

fn setup(loupist: &str) -> (tempfile::TempDir, KnowledgeStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
    let settings = ready("Battery suppliers");
    store
        .transact(|write| store_settings(write.tx, loupist, &settings))
        .unwrap();
    (dir, store)
}

fn ready(focus: &str) -> GraphProcessingSettings {
    GraphProcessingSettings {
        enrichment_enabled: true,
        helper_provider: Some("fixture".into()),
        helper_model: Some("fixture-local".into()),
        research_focus: focus.into(),
        ..GraphProcessingSettings::default()
    }
}

fn kind(name: &str) -> ExtensionType {
    ExtensionType {
        name: name.into(),
        description: "A Loupist-specific kind kept distinct from the shared core types.".into(),
        category: ExtensionCategory::Product,
        labels: ExtensionLabels {
            en: "Process node".into(),
            ko: Some("공정 노드".into()),
        },
        blurbs: ExtensionLabels {
            en: "A process step such as etch or deposition".into(),
            ko: Some("식각이나 증착 같은 공정 단계".into()),
        },
        fields: vec![ExtensionField {
            name: "process_node".into(),
            description: "The stated process generation.".into(),
        }],
    }
}

fn relation(name: &str, source: &str, target: &str) -> ExtensionRelation {
    ExtensionRelation {
        name: name.into(),
        description: "The source is connected to the target in the stated direction.".into(),
        signatures: vec![(source.into(), target.into())],
        labels: ExtensionLabels {
            en: "made on".into(),
            ko: Some("제조 공정".into()),
        },
    }
}

fn reply(body: serde_json::Value) -> InferenceReply {
    InferenceReply {
        disposition: InferenceDisposition::Completed,
        body: serde_json::json!({
            "json_object": body,
            "usage": {"input_tokens": 20, "output_tokens": 10, "certainty": "accounted"}
        }),
    }
}

fn draft(types: Vec<ExtensionType>, relations: Vec<ExtensionRelation>) -> serde_json::Value {
    serde_json::json!({ "types": types, "relations": relations })
}

fn proposals(store: &KnowledgeStore, loupist: &str) -> Vec<(String, String)> {
    store
        .read(|write| {
            let mut query = write.tx.prepare(
                "SELECT kind, status FROM knowledge_schema_proposals
                 WHERE loupist_id = ?1 ORDER BY created_at_ms, proposal_id",
            )?;
            let rows = query
                .query_map([loupist], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap()
}

fn growth(store: &KnowledgeStore, loupist: &str) -> Vec<(String, String, String)> {
    store
        .read(|write| {
            let mut query = write.tx.prepare(
                "SELECT kind, subject_id, actor FROM knowledge_growth_events
                 WHERE loupist_id = ?1 AND kind IN ('schema_extended', 'schema_reduced')
                 ORDER BY rowid",
            )?;
            let rows = query
                .query_map([loupist], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap()
}

fn extension_version(store: &KnowledgeStore, loupist: &str) -> u32 {
    store
        .read(|write| Ok(load_settings(write.tx, loupist)?.extension.version))
        .unwrap()
}

fn claim_ready(store: &KnowledgeStore, loupist: &str, focus: &str, now: i64) -> Attempt {
    match store
        .transact(|write| claim(write.tx, loupist, focus, now))
        .unwrap()
    {
        Claimed::Ready(attempt) => *attempt,
        other => panic!("expected a ready proposal, got {}", claim_label(&other)),
    }
}

fn claim_label(claimed: &Claimed) -> &'static str {
    match claimed {
        Claimed::Ready(_) => "ready",
        Claimed::Wait => "wait",
        Claimed::Settled(_) => "settled",
    }
}

fn finish_reply(
    store: &KnowledgeStore,
    attempt: &Attempt,
    body: InferenceReply,
    now: i64,
) -> Finish {
    store
        .transact(|write| finish(write, attempt, &body, now))
        .unwrap()
}

fn source_view(loupist: &str) -> GraphJobView {
    GraphJobView {
        job_id: "job-source".into(),
        loupist_id: loupist.into(),
        world_record_id: "world".into(),
        world_revision_id: "rev".into(),
        status: "completed".into(),
        payload_digest: "digest".into(),
        created_at_ms: AT,
        host_may_redrive: false,
        document: JobDocument {
            version: 1,
            episode_kind: "source_span".into(),
            generation_id: "gen".into(),
            episode_key: "ep".into(),
            previous_episode_key: None,
            name: "Source".into(),
            body: "text".into(),
            source_binding: None,
            origin_chat_id: None,
            origin_message_id: None,
            artifact_utf8: None,
            logical_digest: None,
            publication_time: None,
            effective_time: None,
            reference_time: "2026-10-30".into(),
            source_label: "notes".into(),
            extraction: ExtractionBudget {
                model_profile: "fixture/fixture-local".into(),
                thinking_level: "off".into(),
                run_options: Default::default(),
                embedding_profile: "local-en-128".into(),
                max_llm_calls: 16,
                max_embedding_calls: 64,
                timeout_ms: 60_000,
            },
            dedup_key: "dedup".into(),
            mutation_sent: false,
            inference_attempted: false,
            usage_uncertain: false,
            lease_expires_at_ms: None,
            sealed_revision_id: None,
            attempt_id: None,
        },
    }
}

fn complete_sources(store: &KnowledgeStore, loupist: &str, times: &[i64]) {
    store
        .transact(|write| {
            for (index, at) in times.iter().enumerate() {
                write.tx.execute(
                    "INSERT INTO knowledge_graph_outbox
                     (job_id, loupist_id, world_record_id, world_revision_id, status, payload_json, payload_digest, created_at_ms)
                     VALUES (?1, ?2, 'world', 'rev', 'completed', ?3, 'digest', ?4)",
                    params![
                        format!("done-{loupist}-{index}-{at}"),
                        loupist,
                        r#"{"episodeKind":"source_span"}"#,
                        at
                    ],
                )?;
            }
            Ok(())
        })
        .unwrap();
}

fn queue_source(
    store: &KnowledgeStore,
    settings: &GraphProcessingSettings,
    loupist: &str,
) -> String {
    let source = store
        .create_record(CreateRecord {
            loupist_id: loupist.into(),
            kind: RecordKind::Source,
            title: "Sources".into(),
            at_ms: 10,
        })
        .unwrap()
        .record_id;
    let bytes = b"supplier note";
    let retained = store
        .retain_source(RetainSource {
            origin: None,
            loupist_id: loupist.into(),
            record_id: source.clone(),
            expected_revision_id: None,
            at_ms: AT,
            source_id: store
                .list_sources(loupist, &source)
                .unwrap()
                .first()
                .map(|item| item.source_id.clone()),
            title: "Notes".into(),
            source_class: SourceClass::PublisherDocument,
            origin_locator: "https://example.test/notes".into(),
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
        .unwrap();
    let world = store
        .create_record(CreateRecord {
            loupist_id: loupist.into(),
            kind: RecordKind::World,
            title: "World".into(),
            at_ms: 10,
        })
        .unwrap();
    let sealed = store
        .seal_world(SealWorld {
            loupist_id: loupist.into(),
            record_id: world.record_id.clone(),
            expected_revision_id: None,
            at_ms: AT,
            nodes: Vec::new(),
            claims: Vec::new(),
            annotations: Vec::new(),
            dependencies: Vec::new(),
        })
        .unwrap();
    store
        .graph_accept_episode(
            settings,
            AddToWorld {
                loupist_id: loupist.into(),
                world_record_id: world.record_id,
                world_revision_id: sealed.revision_id,
                source_record_id: source,
                source_revision_id: retained.revision_id,
                span_id: retained.span_id.expect("span"),
                at_ms: AT,
                binding_role: "extracted_from".into(),
            },
        )
        .unwrap()
        .job_id
}

fn node(id: &str, name: &str, kind_name: &str) -> GraphNode {
    GraphNode {
        id: id.into(),
        name: name.into(),
        aliases: Vec::new(),
        description: String::new(),
        kind: kind_name.into(),
        source_role: "source".into(),
        properties: std::collections::BTreeMap::new(),
    }
}

fn claim_fact(id: &str, relation_name: &str, proposition: &str, nodes: &[&str]) -> GraphClaim {
    GraphClaim {
        id: id.into(),
        proposition: proposition.into(),
        attribution: "Fictional filing".into(),
        relation: relation_name.into(),
        endpoints: nodes
            .iter()
            .enumerate()
            .map(|(index, node_id)| ClaimEndpoint {
                node_id: (*node_id).into(),
                role: if index == 0 { "source" } else { "target" }.into(),
            })
            .collect(),
        qualifiers: Vec::new(),
        evidence: Vec::new(),
        status: ClaimStatus::Active,
        recorded_at_ms: Some(AT),
        publication_time: None,
        effective_time: None,
        valid_at: None,
        invalid_at: None,
        expired_at: None,
    }
}

#[test]
fn custom_focus_seed_stays_pending_until_the_user_applies_it() {
    let (_dir, store) = setup(LOUPIST);
    let settings = ready("Battery suppliers");
    assert!(store
        .transact(|write| enqueue_seed(write.tx, LOUPIST, "Battery suppliers", &settings, AT))
        .unwrap());
    assert!(!store
        .transact(|write| {
            let mut later = settings.clone();
            later.research_focus = "Only cathode makers".into();
            enqueue_seed(write.tx, LOUPIST, "Only cathode makers", &later, AT + 1)
        })
        .unwrap());
    let attempt = claim_ready(&store, LOUPIST, "Battery suppliers", AT + 2);
    assert!(attempt.request.get("tools").is_none());
    let done = finish_reply(
        &store,
        &attempt,
        reply(draft(
            vec![kind("ProcessNode")],
            vec![relation("MADE_ON", "ProcessNode", "Organization")],
        )),
        AT + 3,
    );
    assert_eq!(done.status, "pending");
    assert!(done.notifies);
    assert_eq!(extension_version(&store, LOUPIST), 0);
    assert!(growth(&store, LOUPIST).is_empty());
    assert_eq!(
        proposals(&store, LOUPIST),
        vec![("seed".into(), "pending".into())]
    );
    // The open seed holds back a review until the user decides.
    complete_sources(&store, LOUPIST, &(AT + 1..AT + 11).collect::<Vec<_>>());
    assert!(store
        .transact(|write| enqueue_review(write.tx, &source_view(LOUPIST), AT + 30))
        .unwrap()
        .is_none());
    store
        .transact(|write| {
            let mut current = load_settings(write.tx, LOUPIST)?;
            world_schema::apply_proposal(
                write.tx,
                LOUPIST,
                &mut current.extension,
                &attempt.proposal_id,
                AT + 4,
            )?;
            store_settings(write.tx, LOUPIST, &current)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(extension_version(&store, LOUPIST), 1);
    assert_eq!(
        growth(&store, LOUPIST),
        vec![
            (
                "schema_extended".into(),
                "ProcessNode".into(),
                "user".into()
            ),
            ("schema_extended".into(), "MADE_ON".into(), "user".into()),
        ]
    );
    assert_eq!(
        proposals(&store, LOUPIST),
        vec![("seed".into(), "applied".into())]
    );
}

#[test]
fn source_jobs_run_while_a_seed_is_preparing() {
    let (_dir, store) = setup("loupist-a");
    let settings = ready("Battery suppliers");
    store
        .transact(|write| store_settings(write.tx, "loupist-b", &settings))
        .unwrap();
    let held = queue_source(&store, &settings, "loupist-a");
    let other = queue_source(&store, &settings, "loupist-b");
    assert!(store
        .transact(|write| enqueue_seed(write.tx, "loupist-a", "Battery suppliers", &settings, AT))
        .unwrap());
    let claimed = store.graph_mark_running(&settings, &held, AT + 1).unwrap();
    assert_eq!(claimed.status, JOB_RUNNING);
    let other_claimed = store.graph_mark_running(&settings, &other, AT).unwrap();
    assert_eq!(other_claimed.status, JOB_RUNNING);
    assert_eq!(
        proposals(&store, "loupist-a"),
        vec![("seed".into(), "preparing".into())]
    );
}

#[test]
fn invalid_seed_fails_and_empty_seed_is_dismissed() {
    let (_dir, store) = setup("loupist-invalid");
    let settings = ready("Battery suppliers");
    assert!(store
        .transact(|write| enqueue_seed(
            write.tx,
            "loupist-invalid",
            "Battery suppliers",
            &settings,
            AT
        ))
        .unwrap());
    let attempt = claim_ready(&store, "loupist-invalid", "Battery suppliers", AT + 1);
    let failed = finish_reply(
        &store,
        &attempt,
        reply(draft(vec![kind("Organization")], Vec::new())),
        AT + 2,
    );
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.reason, Some("invalid_output"));
    assert!(!failed.notifies);
    assert!(growth(&store, "loupist-invalid").is_empty());

    let (_empty_dir, empty) = setup("loupist-empty");
    let empty_settings = ready("Battery suppliers");
    assert!(empty
        .transact(|write| enqueue_seed(
            write.tx,
            "loupist-empty",
            "Battery suppliers",
            &empty_settings,
            AT
        ))
        .unwrap());
    let attempt = claim_ready(&empty, "loupist-empty", "Battery suppliers", AT + 1);
    let dismissed = finish_reply(
        &empty,
        &attempt,
        reply(serde_json::json!({ "types": [], "relations": [] })),
        AT + 2,
    );
    assert_eq!(dismissed.status, "dismissed");
    assert!(!dismissed.notifies);
    assert!(growth(&empty, "loupist-empty").is_empty());
    assert_eq!(extension_version(&empty, "loupist-empty"), 0);

    let (_call_dir, calls) = setup("loupist-call");
    assert!(calls
        .transact(|write| enqueue_seed(
            write.tx,
            "loupist-call",
            "Battery suppliers",
            &settings,
            AT
        ))
        .unwrap());
    let attempt = claim_ready(&calls, "loupist-call", "Battery suppliers", AT + 1);
    let not_sent = finish_reply(
        &calls,
        &attempt,
        InferenceReply {
            disposition: InferenceDisposition::NotSent,
            body: serde_json::json!({}),
        },
        AT + 2,
    );
    assert_eq!(not_sent.status, "failed");
    assert_eq!(not_sent.reason, Some("not_sent"));
    calls
        .transact(|write| store_settings(write.tx, "loupist-uncertain", &settings))
        .unwrap();
    let attempt = insert_preparing(&calls, "loupist-uncertain", ProposalKind::Seed);
    let uncertain = finish_reply(
        &calls,
        &attempt,
        InferenceReply {
            disposition: InferenceDisposition::Uncertain,
            body: serde_json::json!({}),
        },
        AT + 3,
    );
    assert_eq!(uncertain.reason, Some("uncertain_paid_attempt"));
}

fn insert_preparing(store: &KnowledgeStore, loupist: &str, kind_name: ProposalKind) -> Attempt {
    store
        .transact(|write| {
            world_schema::insert_proposal(
                write.tx,
                loupist,
                kind_name,
                ProposalStatus::Preparing,
                Vec::new(),
                Vec::new(),
                AT,
                None,
            )?;
            Ok(())
        })
        .unwrap();
    claim_ready(store, loupist, "Battery suppliers", AT + 1)
}

#[test]
fn review_waits_for_ten_later_sources_a_quiet_day_and_no_open_proposal() {
    let (_dir, store) = setup(LOUPIST);
    let view = source_view(LOUPIST);
    complete_sources(&store, LOUPIST, &(1..10).collect::<Vec<_>>());
    assert!(store
        .transact(|write| enqueue_review(write.tx, &view, 100))
        .unwrap()
        .is_none());
    complete_sources(&store, LOUPIST, &[10]);
    let first = store
        .transact(|write| enqueue_review(write.tx, &view, 100))
        .unwrap();
    assert!(first.is_some());
    assert_eq!(
        proposals(&store, LOUPIST),
        vec![("review".into(), "preparing".into())]
    );
    complete_sources(&store, LOUPIST, &(11..21).collect::<Vec<_>>());
    assert!(store
        .transact(|write| enqueue_review(write.tx, &view, 100 + 24 * 60 * 60 * 1000 + 1))
        .unwrap()
        .is_none());
    store
        .transact(|write| {
            write.tx.execute(
                "UPDATE knowledge_schema_proposals SET status = 'pending' WHERE proposal_id = ?1",
                [first.unwrap()],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(store
        .transact(|write| enqueue_review(write.tx, &view, 200))
        .unwrap()
        .is_none());
    let created: i64 = store
        .read(|write| {
            Ok(write.tx.query_row(
                "SELECT created_at_ms FROM knowledge_schema_proposals WHERE loupist_id = ?1",
                [LOUPIST],
                |row| row.get(0),
            )?)
        })
        .unwrap();
    store
        .transact(|write| {
            write.tx.execute(
                "UPDATE knowledge_schema_proposals SET status = 'dismissed', decided_at_ms = ?2
                 WHERE loupist_id = ?1",
                params![LOUPIST, created],
            )?;
            Ok(())
        })
        .unwrap();
    complete_sources(&store, LOUPIST, &(101..111).collect::<Vec<_>>());
    assert!(store
        .transact(|write| enqueue_review(write.tx, &view, created + 24 * 60 * 60 * 1000))
        .unwrap()
        .is_none());
    assert!(store
        .transact(|write| enqueue_review(write.tx, &view, created + 24 * 60 * 60 * 1000 + 1))
        .unwrap()
        .is_some());
}

#[test]
fn settled_seed_can_release_a_review_the_open_seed_was_blocking() {
    let (_dir, store) = setup(LOUPIST);
    let settings = ready("Battery suppliers");
    assert!(store
        .transact(|write| enqueue_seed(write.tx, LOUPIST, "Battery suppliers", &settings, AT))
        .unwrap());
    complete_sources(&store, LOUPIST, &(AT + 1..AT + 11).collect::<Vec<_>>());
    assert!(store
        .transact(|write| enqueue_review(write.tx, &source_view(LOUPIST), AT + 20))
        .unwrap()
        .is_none());
    let attempt = claim_ready(&store, LOUPIST, "Battery suppliers", AT + 21);
    let done = finish_reply(
        &store,
        &attempt,
        reply(serde_json::json!({ "types": [], "relations": [] })),
        AT + 22,
    );
    assert_eq!(done.status, "dismissed");
    assert_eq!(
        proposals(&store, LOUPIST),
        vec![
            ("seed".into(), "dismissed".into()),
            ("review".into(), "preparing".into()),
        ]
    );
}

#[test]
fn review_input_lists_untyped_names_then_open_relations_under_the_cap() {
    let (_dir, store) = setup(LOUPIST);
    let mut settings = ready("Battery suppliers");
    settings.extension.version = 1;
    settings.extension.updated_at_ms = AT;
    settings.extension.types = vec![kind("FabNode")];
    settings.extension.relations = vec![relation("FABRICATED_ON", "FabNode", "Organization")];
    store
        .transact(|write| store_settings(write.tx, LOUPIST, &settings))
        .unwrap();
    let world = store
        .create_record(CreateRecord {
            loupist_id: LOUPIST.into(),
            kind: RecordKind::World,
            title: "World".into(),
            at_ms: AT,
        })
        .unwrap();
    let long = "x".repeat(250);
    store
        .seal_world(SealWorld {
            loupist_id: LOUPIST.into(),
            record_id: world.record_id,
            expected_revision_id: None,
            at_ms: AT,
            nodes: vec![
                node("node-a", "Most Linked", "Entity"),
                node("node-b", "Least Linked", "Entity"),
                node("node-c", "Listed Organization", "Organization"),
            ],
            claims: vec![
                claim_fact("claim-001", "COVERS_NODE", &long, &["node-a", "node-b"]),
                claim_fact("claim-002", "COVERS_NODE", "Second cover", &["node-a"]),
                claim_fact("claim-003", "RELATES_TO", "Open link", &["node-a"]),
                claim_fact("claim-004", "SUPPLIES", "Core link", &["node-a", "node-c"]),
                claim_fact("claim-005", "FABRICATED_ON", "Extension link", &["node-b"]),
            ],
            annotations: Vec::new(),
            dependencies: Vec::new(),
        })
        .unwrap();
    store
        .transact(|write| {
            world_schema::insert_proposal(
                write.tx,
                LOUPIST,
                ProposalKind::Review,
                ProposalStatus::Preparing,
                Vec::new(),
                Vec::new(),
                AT + 1,
                None,
            )?;
            Ok(())
        })
        .unwrap();
    let input = |focus: &str, at: i64| {
        let attempt = claim_ready(&store, LOUPIST, focus, at);
        let text = attempt.request["messages"][1]["content"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(text.len() <= 24 * 1024);
        text
    };
    let body: serde_json::Value =
        serde_json::from_str(&input("Battery suppliers", AT + 2)).unwrap();
    // Untyped entities rank by claim count; typed ones are not proposed again.
    let names: Vec<_> = body["unclassifiedEntities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Most Linked", "Least Linked"]);
    // Only open relations are offered, each with a clipped example.
    let relations: Vec<_> = body["openRelations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(relations, ["COVERS_NODE", "RELATES_TO"]);
    assert!(
        body["openRelations"][0]["example"]
            .as_str()
            .unwrap()
            .chars()
            .count()
            <= 200
    );
    // An oversized focus is trimmed before the World evidence is dropped.
    let capped = input(&"f".repeat(40_000), AT + 3);
    assert!(capped.contains("Most Linked") && capped.contains("COVERS_NODE"));
}

#[test]
fn review_drops_known_kinds_and_fails_when_over_the_limit() {
    let (_dir, store) = setup(LOUPIST);
    let mut settings = ready("Battery suppliers");
    settings.extension.version = 1;
    settings.extension.updated_at_ms = AT;
    settings.extension.types = vec![kind("FabNode")];
    settings.extension.relations = vec![relation("FABRICATED_ON", "FabNode", "Organization")];
    store
        .transact(|write| store_settings(write.tx, LOUPIST, &settings))
        .unwrap();
    let attempt = insert_preparing(&store, LOUPIST, ProposalKind::Review);
    let done = finish_reply(
        &store,
        &attempt,
        reply(draft(
            vec![
                kind("FabNode"),
                kind("Organization"),
                kind("WaferLot"),
                kind("WaferLot"),
            ],
            vec![
                relation("SUPPLIES", "Organization", "Organization"),
                relation("RELATES_TO", "Entity", "Entity"),
                relation("FABRICATED_ON", "FabNode", "Organization"),
                relation("ETCHES_WITH", "WaferLot", "Product"),
                relation("ETCHES_WITH", "WaferLot", "Entity"),
            ],
        )),
        AT + 4,
    );
    assert_eq!(done.status, "pending");
    assert!(done.notifies);
    assert_eq!(extension_version(&store, LOUPIST), 1);
    assert!(growth(&store, LOUPIST).is_empty());
    let body: serde_json::Value = store
        .read(|write| {
            let json: String = write.tx.query_row(
                "SELECT body_json FROM knowledge_schema_proposals WHERE proposal_id = ?1",
                [&attempt.proposal_id],
                |row| row.get(0),
            )?;
            Ok(serde_json::from_str(&json)?)
        })
        .unwrap();
    assert_eq!(body["types"][0]["name"], "WaferLot");
    assert_eq!(body["types"].as_array().unwrap().len(), 1);
    assert_eq!(body["relations"][0]["name"], "ETCHES_WITH");
    assert_eq!(body["relations"].as_array().unwrap().len(), 1);

    store
        .transact(|write| {
            write.tx.execute(
                "UPDATE knowledge_schema_proposals SET status = 'dismissed', decided_at_ms = ?2
                 WHERE proposal_id = ?1",
                params![attempt.proposal_id, AT + 5],
            )?;
            Ok(())
        })
        .unwrap();
    let over = insert_preparing(&store, LOUPIST, ProposalKind::Review);
    let failed = finish_reply(
        &store,
        &over,
        reply(draft(
            vec![kind("LotA"), kind("LotB"), kind("LotC")],
            Vec::new(),
        )),
        AT + 6,
    );
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.reason, Some("over_limit"));

    let mut crowded = settings.clone();
    crowded.extension.types = vec![kind("KindA"), kind("KindB"), kind("KindC")];
    crowded.extension.relations.clear();
    store
        .transact(|write| store_settings(write.tx, "loupist-crowded", &crowded))
        .unwrap();
    let crowded_attempt = insert_preparing(&store, "loupist-crowded", ProposalKind::Review);
    let failed = finish_reply(
        &store,
        &crowded_attempt,
        reply(draft(vec![kind("KindD"), kind("KindE")], Vec::new())),
        AT + 7,
    );
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.reason, Some("over_limit"));
}

#[test]
fn restart_fails_a_preparing_seed_older_than_the_cap() {
    let (_dir, store) = setup("loupist-stale");
    let now = AT + 500_000;
    store
        .transact(|write| {
            for (loupist, created) in [
                ("loupist-stale", now - 120_001),
                ("loupist-exact", now - 120_000),
                ("loupist-young", now - 1_000),
            ] {
                store_settings(write.tx, loupist, &ready("Battery suppliers"))?;
                world_schema::insert_proposal(
                    write.tx,
                    loupist,
                    ProposalKind::Seed,
                    ProposalStatus::Preparing,
                    Vec::new(),
                    Vec::new(),
                    created,
                    None,
                )?;
            }
            assert_eq!(recover(write.tx, now)?, 1);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        proposals(&store, "loupist-stale"),
        vec![("seed".into(), "failed".into())]
    );
    assert_eq!(
        proposals(&store, "loupist-exact"),
        vec![("seed".into(), "preparing".into())]
    );
    assert_eq!(
        proposals(&store, "loupist-young"),
        vec![("seed".into(), "preparing".into())]
    );
}
