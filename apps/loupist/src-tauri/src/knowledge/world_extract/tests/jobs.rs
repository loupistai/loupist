//! The in-process World job, the derived index and World search, end to end
//! against a scripted model and the real local embedding model.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use crate::knowledge::graph_inference::{InferenceDisposition, InferenceReply};
use crate::knowledge::graph_jobs::{self, GraphJobView, GraphProcessingSettings};
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
        if let Some(disposition) = self.fail.get(&title) {
            return InferenceReply {
                disposition: disposition.clone(),
                body: json!({"status": "uncertain"}),
            };
        }
        let value = match title.as_str() {
            "NodeResolutions" => resolve_by_name(request),
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

/// Answer `dedupe_nodes` by exact name: a candidate with the same name is the duplicate.
fn resolve_by_name(request: &Value) -> Value {
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
            let duplicate = existing
                .as_array()
                .unwrap()
                .iter()
                .find(|candidate| candidate["name"] == entity["name"])
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
    // A disconnected provider fails before any paid attempt and may run again.
    let mut fresh = Fixture::new();
    let job = fresh.queue("Note", SUPPLY);
    let failed = drive(&mut fresh, &job, &script, "missing");
    assert_eq!(failed.status, graph_jobs::JOB_FAILED_BEFORE);
    assert!(failed.host_may_redrive);
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
