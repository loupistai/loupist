use super::super::graph_inference::{InferenceDisposition, InferenceReply};
use super::super::world_schema::{self, ExtensionField};
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

fn changes(store: &KnowledgeStore, loupist: &str) -> Vec<(String, serde_json::Value)> {
    store
        .read(|write| {
            let mut query = write.tx.prepare(
                "SELECT producer, evidence_json FROM knowledge_schema_changes
                 WHERE loupist_id = ?1 ORDER BY rowid",
            )?;
            let rows = query
                .query_map([loupist], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows
                .into_iter()
                .map(|(producer, json)| (producer, serde_json::from_str(&json).unwrap()))
                .collect())
        })
        .unwrap()
}

fn live(store: &KnowledgeStore, loupist: &str) -> WorldExtension {
    store
        .read(|write| Ok(load_settings(write.tx, loupist)?.extension))
        .unwrap()
}

fn spans(store: &KnowledgeStore, loupist: &str, count: usize) -> Vec<String> {
    (0..count)
        .map(|index| {
            let source = store
                .create_record(CreateRecord {
                    loupist_id: loupist.into(),
                    kind: RecordKind::Source,
                    title: format!("Source {index}"),
                    at_ms: 10,
                })
                .unwrap()
                .record_id;
            let bytes = format!("supplier note {index}").into_bytes();
            store
                .retain_source(RetainSource {
                    origin: None,
                    loupist_id: loupist.into(),
                    record_id: source,
                    expected_revision_id: None,
                    at_ms: AT,
                    source_id: None,
                    title: format!("Notes {index}"),
                    source_class: SourceClass::PublisherDocument,
                    origin_locator: format!("https://example.test/notes-{index}"),
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
                        document_id: format!("doc-notes-{index}"),
                        parser_version: None,
                        parent_representation_id: None,
                        input_span_ids: Vec::new(),
                        bytes: bytes.clone(),
                        selector: Selector::ByteRange {
                            start: 0,
                            end: bytes.len() as u64,
                        },
                        context_note: "exact retained range".into(),
                    }),
                })
                .unwrap()
                .span_id
                .expect("span")
        })
        .collect()
}

fn seal(store: &KnowledgeStore, loupist: &str, nodes: Vec<GraphNode>, claims: Vec<GraphClaim>) {
    let world = store
        .create_record(CreateRecord {
            loupist_id: loupist.into(),
            kind: RecordKind::World,
            title: "World".into(),
            at_ms: AT,
        })
        .unwrap();
    store
        .seal_world(SealWorld {
            loupist_id: loupist.into(),
            record_id: world.record_id,
            expected_revision_id: None,
            at_ms: AT,
            nodes,
            claims,
            annotations: Vec::new(),
            dependencies: Vec::new(),
        })
        .unwrap();
}

fn cited(mut claim: GraphClaim, span: &str) -> GraphClaim {
    claim.evidence = vec![EvidenceBinding {
        span_id: span.into(),
        role: BindingRole::Supports,
        attribution: "Fictional filing".into(),
    }];
    claim
}

fn claim_ready(store: &KnowledgeStore, loupist: &str, focus: &str, now: i64) -> Attempt {
    match store
        .transact(|write| claim(write.tx, loupist, focus, now))
        .unwrap()
    {
        Claimed::Ready(attempt) => *attempt,
        Claimed::Wait => panic!("expected a ready review"),
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

fn due_review(store: &KnowledgeStore) {
    complete_sources(store, LOUPIST, &(AT + 1..AT + 11).collect::<Vec<_>>());
}

fn supplier_claims(spans: &[String], count: usize) -> Vec<GraphClaim> {
    (0..count)
        .map(|index| {
            cited(
                claim_fact(
                    &format!("claim-{index:03}"),
                    "COVERS_NODE",
                    "A fab covers a node",
                    &["node-a", "node-b"],
                ),
                &spans[index % spans.len()],
            )
        })
        .collect()
}

fn review_draft(store: &KnowledgeStore, draft_body: serde_json::Value) -> Finish {
    let attempt = claim_ready(store, LOUPIST, "Battery suppliers", AT + 100);
    finish_reply(store, &attempt, reply(draft_body), AT + 101)
}

#[test]
fn a_relation_with_six_claims_across_three_sources_applies_at_once() {
    let (_dir, store) = setup(LOUPIST);
    let spans = spans(&store, LOUPIST, 3);
    seal(
        &store,
        LOUPIST,
        vec![
            node("node-a", "Alpha Fab", "Organization"),
            node("node-b", "Node N3", "Entity"),
        ],
        supplier_claims(&spans, 7),
    );
    due_review(&store);
    let done = review_draft(
        &store,
        draft(
            Vec::new(),
            vec![relation("COVERS_NODE", "Organization", "Entity")],
        ),
    );
    assert_eq!(done.status, "applied");
    assert!(done.notifies);
    let rows = changes(&store, LOUPIST);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "review");
    assert_eq!(rows[0].1["relations"][0]["claims"], 7);
    assert_eq!(rows[0].1["relations"][0]["sources"], 3);
    let extension = live(&store, LOUPIST);
    assert_eq!(extension.version, 1);
    assert_eq!(extension.relations[0].name, "COVERS_NODE");
    assert_eq!(
        growth(&store, LOUPIST),
        vec![(
            "schema_extended".into(),
            "COVERS_NODE".into(),
            "processing".into()
        )]
    );
}

#[test]
fn a_relation_with_two_claims_is_refused_and_nothing_changes() {
    let (_dir, store) = setup(LOUPIST);
    let spans = spans(&store, LOUPIST, 3);
    seal(
        &store,
        LOUPIST,
        vec![
            node("node-a", "Alpha Fab", "Organization"),
            node("node-b", "Node N3", "Entity"),
        ],
        supplier_claims(&spans, 2),
    );
    due_review(&store);
    let done = review_draft(
        &store,
        draft(
            Vec::new(),
            vec![relation("COVERS_NODE", "Organization", "Entity")],
        ),
    );
    assert_eq!(done.status, "refused");
    assert!(!done.notifies);
    assert!(changes(&store, LOUPIST).is_empty());
    assert_eq!(live(&store, LOUPIST).version, 0);
    assert!(growth(&store, LOUPIST).is_empty());
}

#[test]
fn six_claims_from_two_sources_are_refused() {
    let (_dir, store) = setup(LOUPIST);
    let spans = spans(&store, LOUPIST, 2);
    seal(
        &store,
        LOUPIST,
        vec![
            node("node-a", "Alpha Fab", "Organization"),
            node("node-b", "Node N3", "Entity"),
        ],
        supplier_claims(&spans, 8),
    );
    due_review(&store);
    let done = review_draft(
        &store,
        draft(
            Vec::new(),
            vec![relation("COVERS_NODE", "Organization", "Entity")],
        ),
    );
    assert_eq!(done.status, "refused");
    assert!(changes(&store, LOUPIST).is_empty());
}

fn untyped_names(count: usize) -> Vec<GraphNode> {
    (0..count)
        .map(|index| {
            node(
                &format!("node-{index}"),
                &format!("Node N{index}"),
                "Entity",
            )
        })
        .collect()
}

#[test]
fn a_type_covering_five_untyped_names_applies_and_four_is_refused() {
    for (covered, status) in [(5, "applied"), (4, "refused")] {
        let (_dir, store) = setup(LOUPIST);
        seal(&store, LOUPIST, untyped_names(6), Vec::new());
        due_review(&store);
        let mut item = serde_json::to_value(kind("ProcessNode")).unwrap();
        item["covers"] = (0..covered).map(|index| format!("Node N{index}")).collect();
        let done = review_draft(
            &store,
            serde_json::json!({ "types": [item], "relations": [] }),
        );
        assert_eq!(done.status, status, "{covered} names");
        assert_eq!(
            changes(&store, LOUPIST).len(),
            usize::from(status == "applied")
        );
    }
}

#[test]
fn a_repeated_core_kind_is_dropped_and_the_review_is_dismissed() {
    let (_dir, store) = setup(LOUPIST);
    seal(&store, LOUPIST, untyped_names(2), Vec::new());
    due_review(&store);
    let done = review_draft(
        &store,
        draft(
            Vec::new(),
            vec![relation("SUPPLIES", "Organization", "Organization")],
        ),
    );
    assert_eq!(done.status, "dismissed");
    assert!(changes(&store, LOUPIST).is_empty());
}

#[test]
fn review_waits_for_ten_later_sources_and_a_quiet_day() {
    let (_dir, store) = setup(LOUPIST);
    seal(&store, LOUPIST, untyped_names(2), Vec::new());
    let due = |at: i64| store.transact(|write| due(write.tx, LOUPIST, at)).unwrap();
    complete_sources(&store, LOUPIST, &(AT + 1..AT + 10).collect::<Vec<_>>());
    assert!(!due(AT + 50), "nine Sources are not enough");
    complete_sources(&store, LOUPIST, &[AT + 10]);
    assert!(due(AT + 50));
    let attempt = claim_ready(&store, LOUPIST, "Battery suppliers", AT + 60);
    assert!(attempt.request.get("tools").is_none());
    assert!(!due(AT + 61));
    complete_sources(&store, LOUPIST, &(AT + 100..AT + 110).collect::<Vec<_>>());
    assert!(!due(AT + 62));
    assert!(due(AT + 60 + 24 * 60 * 60 * 1000 + 1));
}

#[test]
fn review_input_lists_untyped_names_then_open_relations_under_the_cap() {
    let (_dir, store) = setup(LOUPIST);
    store
        .transact(|write| {
            world_schema::add_kinds(
                write.tx,
                LOUPIST,
                vec![kind("FabNode")],
                vec![relation("FABRICATED_ON", "FabNode", "Organization")],
                world_schema::Provenance::new(world_schema::Producer::Chat, serde_json::json!({})),
                AT,
            )
        })
        .unwrap();
    let long = "x".repeat(250);
    seal(
        &store,
        LOUPIST,
        vec![
            node("node-a", "Most Linked", "Entity"),
            node("node-b", "Least Linked", "Entity"),
            node("node-c", "Listed Organization", "Organization"),
        ],
        vec![
            claim_fact("claim-001", "COVERS_NODE", &long, &["node-a", "node-b"]),
            claim_fact("claim-002", "COVERS_NODE", "Second cover", &["node-a"]),
            claim_fact("claim-003", "RELATES_TO", "Open link", &["node-a"]),
            claim_fact("claim-004", "SUPPLIES", "Core link", &["node-a", "node-c"]),
            claim_fact("claim-005", "FABRICATED_ON", "Extension link", &["node-b"]),
        ],
    );
    due_review(&store);
    let attempt = claim_ready(&store, LOUPIST, "Battery suppliers", AT + 100);
    let text = attempt.request["messages"][1]["content"].as_str().unwrap();
    assert!(text.len() <= 24 * 1024);
    let body: serde_json::Value = serde_json::from_str(text).unwrap();
    let names = |key: &str| -> Vec<String> {
        body[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        names("unclassifiedEntities"),
        ["Most Linked", "Least Linked"]
    );
    assert_eq!(names("openRelations"), ["COVERS_NODE", "RELATES_TO"]);
    assert!(
        body["openRelations"][0]["example"]
            .as_str()
            .unwrap()
            .chars()
            .count()
            <= 200
    );
}
