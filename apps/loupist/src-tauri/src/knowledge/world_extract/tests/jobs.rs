//! The in-process World job, the derived index and World search, end to end
//! against a scripted model and the real local embedding model.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use crate::knowledge::graph_inference::{InferenceDisposition, InferenceReply};
use crate::knowledge::graph_jobs::{self, GraphJobView, GraphProcessingSettings};
use crate::knowledge::lens_page::{
    AnchorInput, Basis, BasisRef, FilingOrigin, LensOp, PageDelta, PageKind, PageTarget, SectionKey,
};
use crate::knowledge::world_extract::{self, index};
use crate::knowledge::{
    Authorship, Coverage, CreateRecord, KnowledgeStore, RecordKind, RepresentationRole,
    RetainMaterial, RetainSource, SealWorld, Selector, SourceClass, WorldSnapshotBody,
};

const LOUPIST: &str = "loupist-extract";
const AT: i64 = 1_760_000_000_000;

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../src/knowledge/embed_assets")
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

struct Fixture {
    store: KnowledgeStore,
    settings: GraphProcessingSettings,
    world: String,
    _directory: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
        let world = store
            .create_record(CreateRecord {
                loupist_id: LOUPIST.into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: AT,
            })
            .unwrap();
        store
            .seal_world(SealWorld {
                loupist_id: LOUPIST.into(),
                record_id: world.record_id.clone(),
                expected_revision_id: None,
                at_ms: AT,
                nodes: vec![],
                claims: vec![],
                annotations: vec![],
                dependencies: vec![],
            })
            .unwrap();
        Self {
            store,
            settings: GraphProcessingSettings {
                enrichment_enabled: true,
                helper_provider: Some("fixture".into()),
                helper_model: Some("fixture-local".into()),
                ..GraphProcessingSettings::default()
            },
            world: world.record_id,
            _directory: directory,
        }
    }

    fn head(&self) -> String {
        self.store
            .read(|read| read.owned_record(LOUPIST, &self.world))
            .unwrap()
            .revision_id
            .unwrap()
    }

    fn body(&self) -> WorldSnapshotBody {
        let revision = self.store.get_revision(LOUPIST, &self.head()).unwrap();
        serde_json::from_value(revision.payload).unwrap()
    }

    /// Retain one Source and queue its World job against the current head.
    fn queue(&self, title: &str, body: &str) -> GraphJobView {
        let record = self
            .store
            .create_record(CreateRecord {
                loupist_id: LOUPIST.into(),
                kind: RecordKind::Source,
                title: title.into(),
                at_ms: AT,
            })
            .unwrap();
        let retained = self
            .store
            .retain_source(RetainSource {
                origin: None,
                loupist_id: LOUPIST.into(),
                record_id: record.record_id.clone(),
                expected_revision_id: None,
                at_ms: AT,
                source_id: None,
                title: title.into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "https://example.test/note".into(),
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
                    document_id: "doc".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    bytes: body.as_bytes().to_vec(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: body.len() as u64,
                    },
                    context_note: "exact retained range".into(),
                }),
            })
            .unwrap();
        self.store
            .graph_accept_episode(
                &self.settings,
                graph_jobs::AddToWorld {
                    loupist_id: LOUPIST.into(),
                    world_record_id: self.world.clone(),
                    world_revision_id: self.head(),
                    source_record_id: record.record_id,
                    source_revision_id: retained.revision_id,
                    span_id: retained.span_id.unwrap(),
                    at_ms: AT,
                    binding_role: "extracted_from".into(),
                },
            )
            .unwrap()
    }

    fn stamp(&self) -> Option<String> {
        self.store
            .read(|read| index::stamp(read.tx, LOUPIST, &self.world))
            .unwrap()
            .map(|(revision, _)| revision)
    }

    fn index_rows(&self) -> (i64, i64, i64) {
        self.store
            .read(|read| {
                let count = |table: &str| -> i64 {
                    read.tx
                        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                            row.get(0)
                        })
                        .unwrap_or(-1)
                };
                Ok((
                    count("world_index_nodes"),
                    count("world_index_claims"),
                    count("world_index_claim_text"),
                ))
            })
            .unwrap()
    }
}

/// A scripted World model: replies by response schema, failures by stage.
#[derive(Default)]
struct Script {
    replies: HashMap<String, VecDeque<Value>>,
    fail: HashMap<String, InferenceDisposition>,
    calls: Vec<String>,
    /// Extracted spellings the model resolves to an existing node's name.
    same_as: HashMap<String, String>,
    /// Required evidence and policy text on a scripted relation extraction.
    required_edge_text: Vec<String>,
}

impl Script {
    fn reply(&mut self, title: &str, value: Value) {
        self.replies
            .entry(title.to_string())
            .or_default()
            .push_back(value);
    }

    fn answer(&mut self, request: &Value) -> InferenceReply {
        if request["kind"] == "embedding" {
            let inputs: Vec<String> = request["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item.as_str().unwrap().to_string())
                .collect();
            let vectors =
                crate::knowledge::graph_inference::embed_local_english_batch(&assets(), &inputs)
                    .unwrap();
            return InferenceReply {
                disposition: InferenceDisposition::Completed,
                body: json!({"status": "ok", "json_object": null, "vectors": vectors}),
            };
        }
        let title = request["response_schema"]["title"]
            .as_str()
            .unwrap_or("")
            .to_string();
        self.calls.push(title.clone());
        if title == "ExtractedEdges" {
            let messages = request["messages"].to_string();
            for text in &self.required_edge_text {
                assert!(messages.contains(text), "missing extraction text: {text}");
            }
        }
        if let Some(disposition) = self.fail.get(&title) {
            return InferenceReply {
                disposition: disposition.clone(),
                body: json!({"status": "uncertain"}),
            };
        }
        let value = match title.as_str() {
            "NodeResolutions" => resolve_by_name(request, &self.same_as),
            _ => self
                .replies
                .get_mut(&title)
                .and_then(VecDeque::pop_front)
                .unwrap_or_else(|| panic!("no scripted reply for {title}")),
        };
        InferenceReply {
            disposition: InferenceDisposition::Completed,
            body: json!({
                "status": "ok",
                "json_object": value,
                "usage": {"input_tokens": 100, "output_tokens": 10, "certainty": "accounted"},
            }),
        }
    }
}

/// Answer `dedupe_nodes` by name: a candidate with the same name, or the name
/// `same_as` maps the extracted one to, is the duplicate.
fn resolve_by_name(request: &Value, same_as: &HashMap<String, String>) -> Value {
    let user = request["messages"][1]["content"].as_str().unwrap();
    let between = |open: &str, close: &str| -> Value {
        let start = user.find(open).unwrap() + open.len();
        let end = start + user[start..].find(close).unwrap();
        serde_json::from_str(user[start..end].trim()).unwrap()
    };
    let entities = between("<ENTITIES>", "</ENTITIES>");
    let existing = between("<EXISTING ENTITIES>", "</EXISTING ENTITIES>");
    let resolutions: Vec<Value> = entities
        .as_array()
        .unwrap()
        .iter()
        .map(|entity| {
            let name = entity["name"].as_str().unwrap();
            let name = same_as.get(name).map_or(name, String::as_str);
            let duplicate = existing
                .as_array()
                .unwrap()
                .iter()
                .find(|candidate| candidate["name"] == name)
                .map_or(-1, |candidate| candidate["candidate_id"].as_i64().unwrap());
            json!({"id": entity["id"], "name": entity["name"], "duplicate_candidate_id": duplicate})
        })
        .collect();
    json!({"entity_resolutions": resolutions})
}

fn drive(
    fixture: &mut Fixture,
    job: &GraphJobView,
    script: &Arc<Mutex<Script>>,
    auth_mode: &str,
) -> GraphJobView {
    let script = script.clone();
    runtime()
        .block_on(world_extract::drive_job(
            &fixture.store,
            &mut fixture.settings,
            &job.job_id,
            auth_mode,
            AT + 1_000,
            &[],
            &assets(),
            move |request: Value| {
                let reply = script.lock().unwrap().answer(&request);
                std::future::ready(reply)
            },
        ))
        .unwrap()
}

fn entities(names: &[&str]) -> Value {
    json!({"extracted_entities": names
        .iter()
        .map(|name| json!({"name": name, "entity_type_id": 1}))
        .collect::<Vec<_>>()})
}

fn relation(source: &str, target: &str, kind: &str, fact: &str) -> Value {
    json!({"source_entity_name": source, "target_entity_name": target, "relation_type": kind, "fact": fact})
}

const SUPPLY: &str = "Acme Corp supplies lithium cells to Beta Motors.";
const PARTNER: &str = "Acme Corp partners with Gamma Labs on solid-state cells.";

#[test]
fn source_metric_labels_survive_publication_with_english_renderings() {
    let mut fixture = Fixture::new();
    let observations = [
        ("operating profit", "Acme Corp reported Q2 FY2026 operating profit of USD 11 million."),
        ("operating income", "Acme Corp reported Q2 FY2026 operating income of USD 10 million."),
        ("EBIT", "Acme Corp reported Q2 FY2026 EBIT of USD 9 million."),
        ("adjusted operating profit", "Acme Corp reported Q2 FY2026 adjusted operating profit of USD 8 million."),
        ("영업이익", "Acme Corp reported Q2 FY2026 영업이익 (operating profit) of KRW 13 billion."),
        ("조정 영업이익", "Acme Corp reported Q2 FY2026 조정 영업이익 (adjusted operating profit) of KRW 14 billion."),
        ("매출 비중", "Acme Corp reported Beta Motors' Q2 FY2026 매출 비중 (share of revenue) of 37%."),
    ];
    let source = "Acme Corp reported Q2 FY2026 operating profit USD 11 million, operating income USD 10 million, EBIT USD 9 million, and adjusted operating profit USD 8 million. Acme Corp의 2026 회계연도 2분기 영업이익은 130억 원, 조정 영업이익은 140억 원이며 Beta Motors의 매출 비중은 37%였다.";
    let mut script = Script {
        required_edge_text: vec![
            "Keep a measurement's metric name exactly as written in the Source".into(),
            "original wording and language".into(),
            "English rendering beside it".into(),
            "Never substitute a synonym, different accounting term or acronym".into(),
            source.into(),
        ],
        ..Script::default()
    };
    script.reply("ExtractedEntities", entities(&["Acme Corp"]));
    script.reply(
        "ExtractedEdges",
        json!({"edges": observations.iter().map(|(_, fact)| {
        relation("Acme Corp", "Acme Corp", "REPORTS", fact)
    }).collect::<Vec<_>>()}),
    );
    script.reply("CoreAttributeBatch", json!({"entities": []}));
    let script = Arc::new(Mutex::new(script));
    let job = fixture.queue("Bilingual metric labels", source);
    assert_eq!(
        drive(&mut fixture, &job, &script, "fixture").status,
        graph_jobs::JOB_COMPLETED
    );
    let body = fixture.body();
    assert_eq!(body.claims.len(), observations.len());
    for (label, fact) in observations {
        let published = body
            .claims
            .iter()
            .find(|claim| claim.proposition == fact)
            .unwrap();
        assert!(published.proposition.contains(label));
        assert_eq!(
            published.endpoints[0].node_id,
            published.endpoints[1].node_id
        );
        assert_eq!(published.evidence.len(), 1);
    }
}

#[test]
fn subject_measurement_claim_survives_publication_reingestion_and_reopen() {
    let mut fixture = Fixture::new();
    let script = Arc::new(Mutex::new(Script::default()));
    let fact = "Acme Corp reported Q4 FY2026 GAAP net income USD 42,960 million.";
    for _ in 0..2 {
        {
            let mut script = script.lock().unwrap();
            // A mistaken value node in extraction cannot reach the Map.
            script.reply("ExtractedEntities", json!({"extracted_entities": [
                {"name": "Acme Corp", "entity_type_id": 1},
                {"name": "Acme Corp Q4 FY2026 GAAP net income USD 42,960 million", "entity_type_id": 8}
            ]}));
            script.reply(
                "ExtractedEdges",
                json!({"edges": [relation("Acme Corp", "Acme Corp", "REPORTS", fact)]}),
            );
            script.reply("CoreAttributeBatch", json!({"entities": []}));
        }
        let job = fixture.queue("Reported earnings", fact);
        assert_eq!(
            drive(&mut fixture, &job, &script, "fixture").status,
            graph_jobs::JOB_COMPLETED
        );
        let body = fixture.body();
        assert_eq!(body.nodes.len(), 1);
        assert_eq!(body.claims.len(), 1);
        assert_eq!(body.claims[0].proposition, fact);
        assert_eq!(
            body.claims[0].endpoints[0].node_id,
            body.claims[0].endpoints[1].node_id
        );
        assert_eq!(fixture.index_rows(), (1, 1, 1));
    }
    assert_eq!(fixture.body().claims[0].evidence.len(), 2);
    let revision = fixture.head();
    let database = fixture._directory.path().join("knowledge.sqlite");
    drop(fixture.store);
    let reopened = KnowledgeStore::open(database).unwrap();
    let result = world_extract::query_world(
        &reopened,
        LOUPIST,
        &fixture.world,
        &revision,
        "Acme Corp net income Q4 FY2026",
        &assets(),
    )
    .unwrap()
    .unwrap();
    assert!(result.to_string().contains("42,960 million"));
    let stored: WorldSnapshotBody =
        serde_json::from_value(reopened.get_revision(LOUPIST, &revision).unwrap().payload).unwrap();
    assert_eq!(stored.nodes.len(), 1);
    assert_eq!(stored.claims.len(), 1);
}

/// The first two Sources: a new World, then a repeated fact and a new partner.
fn two_sources(fixture: &mut Fixture) -> (GraphJobView, GraphJobView, Arc<Mutex<Script>>) {
    let script = Arc::new(Mutex::new(Script::default()));
    {
        let mut script = script.lock().unwrap();
        script.reply("ExtractedEntities", entities(&["Acme Corp", "Beta Motors"]));
        script.reply(
            "ExtractedEdges",
            json!({"edges": [relation("Acme Corp", "Beta Motors", "SUPPLIES", SUPPLY)]}),
        );
        script.reply(
            "CoreAttributeBatch",
            json!({"entities": [{"index": 0, "attributes": {"ticker": "ACME"}}]}),
        );
    }
    let first = fixture.queue("Supply note", SUPPLY);
    let first = drive(fixture, &first, &script, "fixture");
    {
        let mut script = script.lock().unwrap();
        script.reply(
            "ExtractedEntities",
            entities(&["Acme Corp", "Beta Motors", "Gamma Labs"]),
        );
        script.reply(
            "ExtractedEdges",
            json!({"edges": [
                relation("Acme Corp", "Beta Motors", "SUPPLIES", SUPPLY),
                relation("Acme Corp", "Gamma Labs", "PARTNERS_WITH", PARTNER),
            ]}),
        );
        script.reply(
            "EdgeDuplicate",
            json!({"duplicate_facts": [], "contradicted_facts": []}),
        );
        script.reply("CoreAttributeBatch", json!({"entities": []}));
    }
    let second = fixture.queue("Partner note", &format!("{SUPPLY} {PARTNER}"));
    let second = drive(fixture, &second, &script, "fixture");
    (first, second, script)
}

impl Fixture {
    /// File an idea page whose subject anchor names `company`, as a Chat turn does.
    fn file_about(&self, company: &str) -> String {
        let origin = FilingOrigin::Turn {
            chat_id: "chat-1".into(),
            message_id: "assistant-1".into(),
            user_message_id: Some("user-1".into()),
            unattended: false,
        };
        let filed = self
            .store
            .transact(|write| {
                write.file(
                    LOUPIST,
                    &origin,
                    vec![PageDelta {
                        target: PageTarget::Create {
                            kind: PageKind::Idea,
                            title: format!("{company} supply thesis"),
                            anchors: vec![AnchorInput::Name {
                                name: company.into(),
                            }],
                        },
                        ops: vec![LensOp::Put {
                            key: SectionKey::new("thesis").unwrap(),
                            heading: "Thesis".into(),
                            markdown: "Supply contracts anchor the margin.".into(),
                            basis: Basis {
                                refs: vec![BasisRef::UserSaid {
                                    message_id: "user-1".into(),
                                }],
                                uncertainty: None,
                            },
                            expected: None,
                        }],
                    }],
                    AT + 500,
                )
            })
            .unwrap();
        filed[0].record_id.clone()
    }

    /// The subject anchor of `record_id`: its filed name and the node it resolved to.
    fn subject_anchor(&self, record_id: &str) -> (String, Option<String>) {
        self.store
            .read(|read| {
                Ok(read.tx.query_row(
                    "SELECT name_fold, node_id FROM knowledge_lens_anchors WHERE record_id = ?1 AND role = 'subject'",
                    [record_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?)
            })
            .unwrap()
    }
}

#[test]
fn a_spelling_the_resolver_confirms_becomes_an_alias_that_resolves_pending_anchors() {
    let mut fixture = Fixture::new();
    let script = Arc::new(Mutex::new(Script::default()));
    {
        let mut script = script.lock().unwrap();
        script.reply("ExtractedEntities", entities(&["Acme Corp", "Beta Motors"]));
        script.reply(
            "ExtractedEdges",
            json!({"edges": [relation("Acme Corp", "Beta Motors", "SUPPLIES", SUPPLY)]}),
        );
        script.reply("CoreAttributeBatch", json!({"entities": []}));
    }
    let first = fixture.queue("Supply note", SUPPLY);
    assert_eq!(
        drive(&mut fixture, &first, &script, "fixture").status,
        graph_jobs::JOB_COMPLETED
    );
    // A Chat files about the company under a spelling the World does not hold.
    let page = fixture.file_about("Acme Corporation");
    assert_eq!(
        fixture.subject_anchor(&page),
        ("acme corporation".into(), None)
    );
    // The next Source spells it that way, and the resolver says it is Acme Corp.
    let partner = "Acme Corporation partners with Gamma Labs on solid-state cells.";
    {
        let mut script = script.lock().unwrap();
        script
            .same_as
            .insert("Acme Corporation".into(), "Acme Corp".into());
        script.reply(
            "ExtractedEntities",
            entities(&["Acme Corporation", "Gamma Labs"]),
        );
        script.reply(
            "ExtractedEdges",
            json!({"edges": [relation("Acme Corporation", "Gamma Labs", "PARTNERS_WITH", partner)]}),
        );
        script.reply(
            "EdgeDuplicate",
            json!({"duplicate_facts": [], "contradicted_facts": []}),
        );
        script.reply("CoreAttributeBatch", json!({"entities": []}));
    }
    let second = fixture.queue("Partner note", partner);
    assert_eq!(
        drive(&mut fixture, &second, &script, "fixture").status,
        graph_jobs::JOB_COMPLETED
    );
    let body = fixture.body();
    let mut names: Vec<&str> = body.nodes.iter().map(|node| node.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["Acme Corp", "Beta Motors", "Gamma Labs"]);
    let acme = body
        .nodes
        .iter()
        .find(|node| node.name == "Acme Corp")
        .unwrap();
    assert_eq!(acme.aliases, ["Acme Corporation"]);
    assert_eq!(
        fixture.subject_anchor(&page),
        ("acme corporation".into(), Some(acme.id.clone()))
    );
    // A spelling that folds to a known alias adds nothing.
    let shouted = "ACME Corporation hires at Gamma Labs.";
    {
        let mut script = script.lock().unwrap();
        script
            .same_as
            .insert("ACME Corporation".into(), "Acme Corp".into());
        script.reply(
            "ExtractedEntities",
            entities(&["ACME Corporation", "Gamma Labs"]),
        );
        script.reply(
            "ExtractedEdges",
            json!({"edges": [relation("ACME Corporation", "Gamma Labs", "HIRES_FROM", shouted)]}),
        );
        script.reply(
            "EdgeDuplicate",
            json!({"duplicate_facts": [], "contradicted_facts": []}),
        );
        script.reply("CoreAttributeBatch", json!({"entities": []}));
    }
    let third = fixture.queue("Hiring note", shouted);
    assert_eq!(
        drive(&mut fixture, &third, &script, "fixture").status,
        graph_jobs::JOB_COMPLETED
    );
    let body = fixture.body();
    let acme = body
        .nodes
        .iter()
        .find(|node| node.name == "Acme Corp")
        .unwrap();
    assert_eq!(acme.aliases, ["Acme Corporation"]);
}

#[test]
fn jobs_seal_the_world_and_its_index_together_and_reuse_stored_entities() {
    let mut fixture = Fixture::new();
    let (first, second, script) = two_sources(&mut fixture);
    assert_eq!(first.status, graph_jobs::JOB_COMPLETED);
    assert_eq!(second.status, graph_jobs::JOB_COMPLETED);
    let body = fixture.body();
    let mut names: Vec<&str> = body.nodes.iter().map(|node| node.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["Acme Corp", "Beta Motors", "Gamma Labs"]);
    assert_eq!(body.claims.len(), 2);
    let supply = body
        .claims
        .iter()
        .find(|claim| claim.proposition == SUPPLY)
        .unwrap();
    // The repeated fact gained the second Source's support instead of a duplicate claim.
    assert_eq!(supply.evidence.len(), 2);
    assert_eq!(supply.evidence[1].role.as_str(), "supports");
    let acme = body
        .nodes
        .iter()
        .find(|node| node.name == "Acme Corp")
        .unwrap();
    assert_eq!(acme.properties.get("ticker"), Some(&json!("ACME")));
    assert!(acme.description.contains(PARTNER));
    // Stored entities were found by name vector and resolved by the model.
    let calls = script.lock().unwrap().calls.clone();
    assert!(calls.contains(&"NodeResolutions".to_string()));
    // The index is stamped with the sealed head and holds every row.
    assert_eq!(fixture.stamp(), Some(fixture.head()));
    assert_eq!(fixture.index_rows(), (3, 2, 2));
    assert_eq!(
        second.document.sealed_revision_id.as_deref(),
        Some(fixture.head().as_str())
    );
}

#[test]
fn an_uncertain_paid_call_runs_once_more_then_quarantines_without_touching_the_world_or_index() {
    let mut fixture = Fixture::new();
    let (_, _, script) = two_sources(&mut fixture);
    let head = fixture.head();
    let rows = fixture.index_rows();
    {
        let mut script = script.lock().unwrap();
        script.reply("ExtractedEntities", entities(&["Acme Corp", "Delta Inc"]));
        script
            .fail
            .insert("ExtractedEdges".into(), InferenceDisposition::Uncertain);
    }
    let job = fixture.queue("Deal note", "Acme Corp acquired Delta Inc.");
    let retried = drive(&mut fixture, &job, &script, "fixture");
    assert_eq!(
        retried.status,
        graph_jobs::JOB_QUEUED,
        "the first cut runs again"
    );
    assert!(fixture.settings.quarantined_generations.is_empty());
    assert_eq!(fixture.head(), head);
    script
        .lock()
        .unwrap()
        .reply("ExtractedEntities", entities(&["Acme Corp", "Delta Inc"]));
    let finished = drive(&mut fixture, &job, &script, "fixture");
    assert_eq!(finished.status, graph_jobs::JOB_RECONCILE);
    assert_eq!(finished.document.interrupted_attempts, 1);
    assert!(finished.document.usage_uncertain);
    assert!(finished.document.inference_attempted);
    assert!(!finished.document.mutation_sent);
    assert!(fixture
        .settings
        .quarantined_generations
        .contains(&finished.document.generation_id));
    assert_eq!(fixture.head(), head);
    assert_eq!(fixture.stamp(), Some(head));
    assert_eq!(fixture.index_rows(), rows);
    // A provider that went away before any paid attempt costs nothing: the job
    // queues again a few times, then fails without quarantine and may run again.
    let mut fresh = Fixture::new();
    let job = fresh.queue("Note", SUPPLY);
    for attempt in 1..=3 {
        let queued = drive(&mut fresh, &job, &script, "missing");
        assert_eq!(queued.status, graph_jobs::JOB_QUEUED);
        assert_eq!(queued.document.unsent_attempts, attempt);
        assert!(!queued.document.inference_attempted);
        assert_eq!(queued.document.interrupted_attempts, 0);
    }
    let failed = drive(&mut fresh, &job, &script, "missing");
    assert_eq!(failed.status, graph_jobs::JOB_FAILED_BEFORE);
    assert!(failed.host_may_redrive);
    assert!(fresh.settings.quarantined_generations.is_empty());
}

#[test]
fn a_failed_commit_rolls_back_the_seal_and_leaves_no_partial_index() {
    let mut fixture = Fixture::new();
    // A claim-text table that cannot take rows makes the index write fail at commit.
    fixture
        .store
        .transact(|write| {
            write.tx.execute_batch(
                "CREATE TABLE world_index_claim_text (loupist_id TEXT, world_record_id TEXT, claim_id TEXT);",
            )?;
            Ok(())
        })
        .unwrap();
    let initial = fixture.head();
    let script = Arc::new(Mutex::new(Script::default()));
    let replies = || {
        let mut script = script.lock().unwrap();
        script.reply("ExtractedEntities", entities(&["Acme Corp", "Beta Motors"]));
        script.reply(
            "ExtractedEdges",
            json!({"edges": [relation("Acme Corp", "Beta Motors", "SUPPLIES", SUPPLY)]}),
        );
        script.reply("CoreAttributeBatch", json!({"entities": []}));
    };
    replies();
    let job = fixture.queue("Supply note", SUPPLY);
    let retried = drive(&mut fixture, &job, &script, "fixture");
    assert_eq!(
        retried.status,
        graph_jobs::JOB_QUEUED,
        "the first cut runs again"
    );
    assert_eq!(fixture.head(), initial);
    replies();
    let finished = drive(&mut fixture, &job, &script, "fixture");
    assert_eq!(finished.status, graph_jobs::JOB_RECONCILE);
    assert_eq!(fixture.head(), initial, "the World seal rolled back");
    assert_eq!(fixture.stamp(), Some(initial));
    let (nodes, claims, _) = fixture.index_rows();
    assert_eq!((nodes, claims), (0, 0));
}

#[test]
fn recovery_reindexes_from_the_confirmed_world_and_clears_quarantine() {
    let mut fixture = Fixture::new();
    let (_, _, script) = two_sources(&mut fixture);
    let bad_reply = || {
        script.lock().unwrap().reply(
            "ExtractedEntities",
            json!({"extracted_entities": "not a list"}),
        )
    };
    bad_reply();
    let job = fixture.queue("Bad reply", "Acme Corp is mentioned.");
    assert_eq!(
        drive(&mut fixture, &job, &script, "fixture").status,
        graph_jobs::JOB_QUEUED
    );
    bad_reply();
    let failed = drive(&mut fixture, &job, &script, "fixture");
    assert_eq!(failed.status, graph_jobs::JOB_RECONCILE);
    // Drop the derived rows: recovery must rebuild them from the sealed World alone.
    fixture
        .store
        .transact(|write| {
            write.tx.execute_batch(
                "DELETE FROM world_index_nodes; DELETE FROM world_index_claims;
                 DELETE FROM world_index_claim_text; DELETE FROM world_index_state;",
            )?;
            Ok(())
        })
        .unwrap();
    let recovery = fixture
        .store
        .transact(|write| graph_jobs::recover_world(write.tx, write.blobs, LOUPIST, AT + 2_000))
        .unwrap();
    let recovered = drive(&mut fixture, &recovery, &script, "missing");
    assert_eq!(recovered.status, graph_jobs::JOB_COMPLETED);
    assert_eq!(fixture.stamp(), Some(fixture.head()));
    assert_eq!(fixture.index_rows(), (3, 2, 2));
    let settings = fixture
        .store
        .read(|read| graph_jobs::load_settings(read.tx, LOUPIST))
        .unwrap();
    assert!(!settings
        .quarantined_generations
        .contains(&failed.document.generation_id));
}

#[test]
fn world_search_ranks_claims_and_entity_neighborhoods_without_a_worker() {
    let mut fixture = Fixture::new();
    let (first, _, _) = two_sources(&mut fixture);
    let head = fixture.head();
    let search = |query: &str| {
        world_extract::query_world(
            &fixture.store,
            LOUPIST,
            &fixture.world,
            &head,
            query,
            &assets(),
        )
        .unwrap()
        .unwrap()
    };
    let found = search("solid-state partnership with Gamma");
    assert_eq!(found["mode"], "hybrid");
    assert_eq!(found["indexed"], true);
    let edges = found["view"]["view"]["edges"].as_array().unwrap();
    assert_eq!(edges[0]["proposition"], PARTNER);
    // An entity hit brings its whole neighborhood.
    let neighborhood = search("Acme Corp");
    let propositions: Vec<&str> = neighborhood["view"]["view"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| edge["proposition"].as_str().unwrap())
        .collect();
    assert!(propositions.contains(&SUPPLY) && propositions.contains(&PARTNER));
    let nodes = neighborhood["view"]["view"]["nodes"].as_array().unwrap();
    assert!(nodes
        .iter()
        .all(|node| node.get("name_embedding").is_none()));
    // A replaced revision is gone; only the head can be searched.
    let earlier = first.document.sealed_revision_id.unwrap();
    assert_ne!(earlier, head);
    assert!(world_extract::query_world(
        &fixture.store,
        LOUPIST,
        &fixture.world,
        &earlier,
        "Acme",
        &assets(),
    )
    .is_err());
    // A World sealed outside a job (here: the index dropped) is indexed on first search.
    fixture
        .store
        .transact(|write| {
            write
                .tx
                .execute_batch("DELETE FROM world_index_state; DELETE FROM world_index_nodes;")?;
            Ok(())
        })
        .unwrap();
    let rebuilt = search("Gamma Labs");
    assert!(!rebuilt["view"]["view"]["edges"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(fixture.stamp(), Some(head));
    assert_eq!(fixture.index_rows(), (3, 2, 2));
}

/// Calls whose inputs do not depend on each other are in flight together:
/// entity resolution with relation extraction, relation resolution with
/// attributes. The World is the one one call after another produces.
#[test]
fn independent_calls_of_one_episode_are_in_flight_together() {
    let mut fixture = Fixture::new();
    let script = Arc::new(Mutex::new(Script::default()));
    {
        let mut script = script.lock().unwrap();
        script.reply("ExtractedEntities", entities(&["Acme Corp", "Beta Motors"]));
        script.reply(
            "ExtractedEdges",
            json!({"edges": [relation("Acme Corp", "Beta Motors", "SUPPLIES", SUPPLY)]}),
        );
        script.reply(
            "CoreAttributeBatch",
            json!({"entities": [{"index": 0, "attributes": {"ticker": "ACME"}}]}),
        );
    }
    let first = fixture.queue("Supply note", SUPPLY);
    assert_eq!(
        drive(&mut fixture, &first, &script, "fixture").status,
        graph_jobs::JOB_COMPLETED
    );
    {
        let mut script = script.lock().unwrap();
        script.reply(
            "ExtractedEntities",
            entities(&["Acme Corp", "Beta Motors", "Gamma Labs"]),
        );
        script.reply(
            "ExtractedEdges",
            json!({"edges": [
                relation("Acme Corp", "Beta Motors", "SUPPLIES", SUPPLY),
                relation("Acme Corp", "Gamma Labs", "PARTNERS_WITH", PARTNER),
            ]}),
        );
        script.reply(
            "EdgeDuplicate",
            json!({"duplicate_facts": [], "contradicted_facts": []}),
        );
        script.reply("CoreAttributeBatch", json!({"entities": []}));
    }
    let second = fixture.queue("Partner note", &format!("{SUPPLY} {PARTNER}"));
    // Each model reply takes a moment; record which calls were out at once.
    let flying = Arc::new(Mutex::new((
        Vec::<String>::new(),
        Vec::<Vec<String>>::new(),
    )));
    let (shared, observed) = (script.clone(), flying.clone());
    let finished = runtime()
        .block_on(world_extract::drive_job(
            &fixture.store,
            &mut fixture.settings,
            &second.job_id,
            "fixture",
            AT + 2_000,
            &[],
            &assets(),
            move |request: Value| {
                let reply = shared.lock().unwrap().answer(&request);
                let title = request["response_schema"]["title"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let llm = request["kind"] == "llm";
                let observed = observed.clone();
                async move {
                    if llm {
                        {
                            let mut state = observed.lock().unwrap();
                            state.0.push(title.clone());
                            let now = state.0.clone();
                            state.1.push(now);
                        }
                        // Long enough that a loaded CI runner still starts the
                        // sibling call while this one is in flight (100 ms was not).
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        observed.lock().unwrap().0.retain(|flying| *flying != title);
                    }
                    reply
                }
            },
        ))
        .unwrap();
    assert_eq!(finished.status, graph_jobs::JOB_COMPLETED);
    let snapshots = flying.lock().unwrap().1.clone();
    let together = |a: &str, b: &str| {
        snapshots
            .iter()
            .any(|seen| seen.iter().any(|t| t == a) && seen.iter().any(|t| t == b))
    };
    assert!(
        together("NodeResolutions", "ExtractedEdges"),
        "{snapshots:?}"
    );
    assert!(
        together("EdgeDuplicate", "CoreAttributeBatch"),
        "{snapshots:?}"
    );
    assert!(
        !snapshots
            .iter()
            .any(|seen| seen.len() > 1 && seen.iter().any(|t| t == "ExtractedEntities")),
        "entities come first, alone: {snapshots:?}"
    );
    // The same calls, in the same order, as one after another.
    let calls = script.lock().unwrap().calls.clone();
    assert_eq!(
        &calls[calls.len() - 5..],
        [
            "ExtractedEntities",
            "NodeResolutions",
            "ExtractedEdges",
            "EdgeDuplicate",
            "CoreAttributeBatch"
        ]
    );
    let body = fixture.body();
    assert_eq!(body.claims.len(), 2);
    assert_eq!(body.nodes.len(), 3);
}

#[test]
fn an_exhausted_frozen_budget_does_not_repeat_paid_calls_or_quarantine_other_sources() {
    let mut fixture = Fixture::new();
    fixture.settings.max_llm_calls = 2;
    let job = fixture.queue("Underfunded note", SUPPLY);
    let initial = fixture.head();
    let script = Arc::new(Mutex::new(Script::default()));
    {
        let mut script = script.lock().unwrap();
        script.reply("ExtractedEntities", entities(&["Acme Corp", "Beta Motors"]));
        script.reply(
            "ExtractedEdges",
            json!({"edges": [relation("Acme Corp", "Beta Motors", "SUPPLIES", SUPPLY)]}),
        );
    }
    let stopped = drive(&mut fixture, &job, &script, "fixture");
    assert_eq!(stopped.status, graph_jobs::JOB_PAUSED);
    assert_eq!(script.lock().unwrap().calls.len(), 2);
    assert!(stopped.document.inference_attempted);
    assert_eq!(stopped.document.interrupted_attempts, 0);
    assert!(stopped.document.attempt_id.is_none());
    assert!(fixture.settings.quarantined_generations.is_empty());
    assert_eq!(fixture.head(), initial);
    fixture
        .store
        .transact(|write| graph_jobs::resume_processing(write.tx, &mut fixture.settings, LOUPIST))
        .unwrap();
    assert_eq!(
        drive(&mut fixture, &job, &script, "fixture").status,
        graph_jobs::JOB_PAUSED
    );
    assert_eq!(script.lock().unwrap().calls.len(), 2);

    fixture.settings.max_llm_calls = 16;
    {
        let mut script = script.lock().unwrap();
        script.reply("ExtractedEntities", entities(&["Acme Corp", "Beta Motors"]));
        script.reply(
            "ExtractedEdges",
            json!({"edges": [relation("Acme Corp", "Beta Motors", "SUPPLIES", SUPPLY)]}),
        );
        script.reply("CoreAttributeBatch", json!({"entities": []}));
    }
    let next = fixture.queue("Funded note", SUPPLY);
    assert_eq!(
        drive(&mut fixture, &next, &script, "fixture").status,
        graph_jobs::JOB_COMPLETED
    );
    // A fresh allowance is available only through the explicit Source action.
    let source = job
        .document
        .source_binding
        .as_ref()
        .unwrap()
        .source_record_id
        .clone();
    let retried = fixture
        .store
        .transact(|write| {
            graph_jobs::retry_source_processing(write.tx, &fixture.settings, LOUPIST, &source)
        })
        .unwrap();
    {
        let mut script = script.lock().unwrap();
        script.reply("ExtractedEntities", entities(&["Acme Corp", "Beta Motors"]));
        script.reply("ExtractedEdges", json!({"edges": []}));
        script.reply("CoreAttributeBatch", json!({"entities": []}));
    }
    assert_eq!(
        drive(&mut fixture, &retried, &script, "fixture").status,
        graph_jobs::JOB_COMPLETED
    );
}

#[test]
fn publication_uses_the_completion_time_after_inference() {
    let mut fixture = Fixture::new();
    let job = fixture.queue("Supply note", SUPPLY);
    let script = Arc::new(Mutex::new(Script::default()));
    {
        let mut script = script.lock().unwrap();
        script.reply("ExtractedEntities", entities(&["Acme Corp", "Beta Motors"]));
        script.reply(
            "ExtractedEdges",
            json!({"edges": [relation("Acme Corp", "Beta Motors", "SUPPLIES", SUPPLY)]}),
        );
        script.reply("CoreAttributeBatch", json!({"entities": []}));
    }
    let start_ms = AT + 1_000;
    let completed = runtime()
        .block_on(world_extract::drive_job(
            &fixture.store,
            &mut fixture.settings,
            &job.job_id,
            "fixture",
            start_ms,
            &[],
            &assets(),
            move |request| {
                let script = script.clone();
                async move {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    script.lock().unwrap().answer(&request)
                }
            },
        ))
        .unwrap();
    assert_eq!(completed.status, graph_jobs::JOB_COMPLETED);
    let head = fixture.head();
    let (revision_at, growth_at): (i64, i64) = fixture
        .store
        .read(|read| {
            Ok((
                read.tx.query_row(
                    "SELECT created_at_ms FROM knowledge_record_revisions WHERE revision_id = ?1",
                    [&head],
                    |row| row.get(0),
                )?,
                read.tx.query_row(
                    "SELECT MAX(at_ms) FROM knowledge_growth_events WHERE record_id = ?1",
                    [&fixture.world],
                    |row| row.get(0),
                )?,
            ))
        })
        .unwrap();
    assert!(
        revision_at >= start_ms + 25,
        "publication must follow the delayed call"
    );
    assert_eq!(growth_at, revision_at);
}

#[test]
fn a_driver_error_settles_the_claim_instead_of_leaving_the_world_running() {
    let mut fixture = Fixture::new();
    fixture.settings.max_llm_calls = 2;
    let job = fixture.queue("Underfunded note", SUPPLY);
    fixture
        .store
        .transact(|write| {
            write.tx.execute_batch(
            "CREATE TRIGGER refuse_budget_pause BEFORE UPDATE OF status ON knowledge_graph_outbox
             WHEN OLD.status = 'running' AND NEW.status = 'paused_budget'
             BEGIN SELECT RAISE(FAIL, 'test storage failure'); END;",
        )?;
            Ok(())
        })
        .unwrap();
    let script = Arc::new(Mutex::new(Script::default()));
    {
        let mut script = script.lock().unwrap();
        script.reply("ExtractedEntities", entities(&["Acme Corp", "Beta Motors"]));
        script.reply(
            "ExtractedEdges",
            json!({"edges": [
                relation("Acme Corp", "Beta Motors", "SUPPLIES", SUPPLY)
            ]}),
        );
    }
    let settled = drive(&mut fixture, &job, &script, "fixture");
    assert_eq!(settled.status, graph_jobs::JOB_QUEUED);
    assert!(settled.document.attempt_id.is_none());
    assert!(settled.document.lease_expires_at_ms.is_none());
    assert_eq!(settled.document.extraction.max_llm_calls, 0);
    assert_eq!(settled.document.interrupted_attempts, 1);
    assert_eq!(
        fixture.store.graph_job(LOUPIST, &job.job_id).unwrap(),
        settled
    );
}
