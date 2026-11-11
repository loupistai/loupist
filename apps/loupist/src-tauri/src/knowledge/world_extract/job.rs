//! World jobs in-process: the extraction pipeline runs on a blocking task, its
//! model calls go through the host's inference callback on the async side, and
//! the sealed World and the derived index commit in one transaction.
//!
//! Nothing outside SQLite is mutated, so a failed or interrupted job leaves no
//! partial index. A job that attempted paid
//! inference and did not complete is still quarantined for reconciliation, and
//! an uncertain call is never retried.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use futures_util::stream::{FuturesUnordered, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

use super::super::error::{KnowledgeError, KnowledgeResult};
use super::super::graph_inference::{self, InferenceDisposition, InferenceReply};
use super::super::graph_jobs::{
    self, GraphJobView, GraphProcessingSettings, JobDocument, StoredSourceBinding, Unpublished,
};
use super::super::model::{GraphNode, TimePoint, TimePrecision, WorldSnapshotBody};
use super::super::KnowledgeStore;
use super::bridge::{Answered, Bridge, BridgeConfig, StageRecord, Stop};
use super::hints::{extraction_hints, observed_relation_names, FollowedInstrument};
use super::ontology::{extraction_instructions, merged_schema, Hints, Schema};
use super::pipeline::{self, Edge, Episode, Graph, Node, PreviousEpisode};
use super::projection::{self, Binding, Context};
use super::search::MemoryClaimText;
use super::{index, text};
use crate::util::sha256;

const LABEL: fn(&str) -> bool = |kind| {
    let mut chars = kind.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_alphabetic())
        && kind.len() <= 64
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
};

fn not_sent() -> InferenceReply {
    InferenceReply {
        disposition: InferenceDisposition::NotSent,
        body: json!({"status": "not_sent"}),
    }
}

/// Run one claimed World job without the graph worker.
// The job boundary takes explicit store, settings, authority and inference inputs.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn drive_job<F, Fut>(
    store: &KnowledgeStore,
    settings: &mut GraphProcessingSettings,
    job_id: &str,
    auth_mode: &str,
    now_ms: i64,
    followed: &[FollowedInstrument],
    assets: &Path,
    mut on_inference: F,
) -> KnowledgeResult<GraphJobView>
where
    F: FnMut(Value) -> Fut,
    Fut: std::future::Future<Output = InferenceReply>,
{
    let opened_at = SystemTime::now();
    let job = match store.graph_claim_running(settings, job_id, now_ms)? {
        graph_jobs::GraphJobClaim::Acquired(job) => job,
        graph_jobs::GraphJobClaim::Existing(job) => return Ok(job),
    };
    let attempt_id = job.document.attempt_id.clone().unwrap_or_default();
    let opened = job.document.clone();
    if matches!(
        opened.episode_kind.as_str(),
        "sealed_snapshot" | "world_recovery"
    ) {
        return match reindex_and_complete(store, settings, &job, &attempt_id, assets, now_ms) {
            Ok(view) => Ok(view),
            Err(_) => finish(store, settings, &job, &attempt_id, false, now_ms),
        };
    }
    let provider = settings.helper_provider.as_deref().unwrap_or("");
    if graph_inference::admit_helper(provider, auth_mode).is_err() {
        // The provider went away after selection; nothing was sent.
        return store.graph_finish_unpublished(
            settings,
            &job.job_id,
            &attempt_id,
            Unpublished::default(),
            now_ms,
        );
    }
    let prepared = match prepare(store, settings, &job, followed, assets) {
        Ok(prepared) => prepared,
        Err(_) => return finish(store, settings, &job, &attempt_id, false, now_ms),
    };
    let started = Instant::now();
    let wall_started = SystemTime::now();
    let deadline = started
        + Duration::from_millis(graph_jobs::attempt_budget_ms(opened.extraction.timeout_ms));
    let (requests, mut incoming) = mpsc::channel::<(Value, oneshot::Sender<Answered>)>(1);
    let config = BridgeConfig {
        scope: json!({
            "loupist_id": job.loupist_id,
            "world_record_id": job.world_record_id,
            "generation_id": opened.generation_id,
        }),
        operation_id: job.job_id.clone(),
        profile: opened.extraction.model_profile.clone(),
        fast_profile: prepared.hints.fast_model.clone(),
        timeout_ms: opened.extraction.timeout_ms,
        max_llm_calls: opened.extraction.max_llm_calls,
        max_embedding_calls: opened.extraction.max_embedding_calls,
    };
    let task = tokio::task::spawn_blocking(move || run_pipeline(prepared, config, requests));
    tokio::pin!(task);
    let mut uncertain = false;
    let mut blocked = false;
    // Calls the pipeline sends together are in flight together; each one's
    // reply goes back on its own channel, so the pipeline reads them in order.
    let mut in_flight = FuturesUnordered::new();
    let joined = loop {
        tokio::select! {
            joined = &mut task => break joined,
            request = incoming.recv() => {
                let Some((request, reply)) = request else { continue };
                if blocked || store.graph_note_inference(job_id, &attempt_id).is_err() {
                    blocked = true;
                    let _ = reply.send(Answered { reply: not_sent(), elapsed: Duration::ZERO });
                    continue;
                }
                let received = Instant::now();
                let remaining = deadline.saturating_duration_since(received);
                let call = on_inference(request);
                in_flight.push(async move {
                    (tokio::time::timeout(remaining, call).await, reply, received)
                });
            }
            Some((outcome, reply, received)) = in_flight.next(), if !in_flight.is_empty() => {
                let response = outcome.unwrap_or_else(|_| InferenceReply {
                    // The call was sent; its outcome is unknown.
                    disposition: InferenceDisposition::Uncertain,
                    body: json!({"status": "uncertain"}),
                });
                if response.disposition == InferenceDisposition::Uncertain {
                    uncertain = true;
                }
                let _ = reply.send(Answered { reply: response, elapsed: received.elapsed() });
            }
        }
    };
    let now_ms = now_ms.saturating_add(
        i64::try_from(opened_at.elapsed().unwrap_or_default().as_millis()).unwrap_or(i64::MAX),
    );
    let Ok(ran) = joined else {
        return finish(store, settings, &job, &attempt_id, true, now_ms);
    };
    write_stage_log(job_id, &ran.stages, started.elapsed(), &ran.result);
    let uncertain = uncertain || ran.uncertain;
    if blocked {
        return finish(store, settings, &job, &attempt_id, true, now_ms);
    }
    let extracted = match ran.result {
        Ok(extracted) => extracted,
        Err(Stop::BudgetExceeded(_)) if !uncertain => {
            return store.transact(|write| {
                graph_jobs::pause_exhausted_attempt(write.tx, job_id, &attempt_id, now_ms)
            });
        }
        Err(_) => {
            let end = Unpublished {
                uncertain,
                paid: ran.paid,
                suspended: suspended(wall_started, started),
            };
            return store.graph_finish_unpublished(settings, job_id, &attempt_id, end, now_ms);
        }
    };
    let spans: Vec<String> = extracted
        .claims
        .iter()
        .filter_map(|claim| claim["evidence"].as_array())
        .flatten()
        .filter_map(|item| item["span_id"].as_str().map(str::to_string))
        .collect();
    let empty_extraction = opened.source_binding.is_some() && extracted.claims.is_empty();
    let validated = graph_jobs::binding_matches(&opened, &spans) || empty_extraction;
    if !validated || (empty_extraction && !artifact_has_bound_episode(&opened, &extracted.artifact))
    {
        return store.graph_finish(
            settings,
            job_id,
            &attempt_id,
            graph_jobs::JOB_COMPLETED,
            uncertain,
            false,
            now_ms,
        );
    }
    let mut vectors = extracted.vectors;
    let followed = store.followed_items();
    let committed = store.transact(|write| {
        let finished = graph_jobs::publish_and_complete(
            write,
            settings,
            job_id,
            &attempt_id,
            &extracted.artifact,
            &extracted.digest,
            now_ms,
            uncertain,
            &followed,
        )?;
        if finished.status == graph_jobs::JOB_COMPLETED {
            if let Some(revision) = finished.document.sealed_revision_id.as_deref() {
                index_revision(write, &job, revision, assets, &mut vectors)?;
            }
        }
        Ok(finished)
    });
    match committed {
        Ok(view) => Ok(view),
        Err(_) => finish(store, settings, &job, &attempt_id, uncertain, now_ms),
    }
}

/// Sleep longer than this during an attempt marks it cut short by the host.
const SUSPEND_GAP: Duration = Duration::from_secs(30);

/// Whether the Mac slept while the attempt ran: the wall clock moved on while
/// the monotonic clock, which stops during sleep on macOS and Linux, did not.
fn suspended(wall_started: SystemTime, started: Instant) -> bool {
    let wall = SystemTime::now()
        .duration_since(wall_started)
        .unwrap_or_default();
    wall.saturating_sub(started.elapsed()) > SUSPEND_GAP
}

/// End an attempt that did not publish. With no paid attempt it can run again;
/// an attempted or uncertain call quarantines the generation as before.
fn finish(
    store: &KnowledgeStore,
    settings: &mut GraphProcessingSettings,
    job: &GraphJobView,
    attempt_id: &str,
    uncertain: bool,
    now_ms: i64,
) -> KnowledgeResult<GraphJobView> {
    store.graph_finish(
        settings,
        &job.job_id,
        attempt_id,
        graph_jobs::JOB_FAILED_BEFORE,
        uncertain,
        false,
        now_ms,
    )
}

/// Recovery, reload and sealed-World indexing: rebuild the derived index from
/// the confirmed revision and complete the job in the same transaction.
fn reindex_and_complete(
    store: &KnowledgeStore,
    settings: &mut GraphProcessingSettings,
    job: &GraphJobView,
    attempt_id: &str,
    assets: &Path,
    now_ms: i64,
) -> KnowledgeResult<GraphJobView> {
    let mut vectors = precompute(store, job, &job.world_revision_id, assets)?;
    store.transact(|write| {
        let finished = graph_jobs::complete_reindex(write, settings, job, attempt_id, now_ms)?;
        if finished.status == graph_jobs::JOB_COMPLETED {
            index_revision(write, job, &job.world_revision_id, assets, &mut vectors)?;
        }
        Ok(finished)
    })
}

fn load_body(
    tx: &rusqlite::Transaction<'_>,
    revision_id: &str,
) -> KnowledgeResult<WorldSnapshotBody> {
    let stored = super::super::revision::load_revision(tx, revision_id)?;
    Ok(serde_json::from_str(&stored.payload_json)?)
}

/// Vectors the index would need for `revision`, computed outside any lock.
fn precompute(
    store: &KnowledgeStore,
    job: &GraphJobView,
    revision: &str,
    assets: &Path,
) -> KnowledgeResult<HashMap<String, Vec<f32>>> {
    let model = index::model_stamp(assets)?;
    let texts = store.read(|read| {
        let body = load_body(read.tx, revision)?;
        index::missing_texts(
            read.tx,
            &job.loupist_id,
            &job.world_record_id,
            &body,
            &model,
        )
    })?;
    let vectors = index::embed_texts(assets, &texts)?;
    Ok(texts.into_iter().zip(vectors).collect())
}

/// Sync the index to `revision` inside the caller's transaction.
fn index_revision(
    write: &super::super::KnowledgeWrite<'_>,
    job: &GraphJobView,
    revision: &str,
    assets: &Path,
    vectors: &mut HashMap<String, Vec<f32>>,
) -> KnowledgeResult<()> {
    let model = index::model_stamp(assets)?;
    let body = load_body(write.tx, revision)?;
    let missing: Vec<String> = index::missing_texts(
        write.tx,
        &job.loupist_id,
        &job.world_record_id,
        &body,
        &model,
    )?
    .into_iter()
    .filter(|text| !vectors.contains_key(text))
    .collect();
    for (text, vector) in missing.iter().zip(index::embed_texts(assets, &missing)?) {
        vectors.insert(text.clone(), vector);
    }
    index::sync(
        write.tx,
        &job.loupist_id,
        &job.world_record_id,
        revision,
        &body,
        &model,
        vectors,
    )
}

/// Bring the stored index to `revision` when it is the World head. A newer
/// head, or a concurrent writer that got there first, leaves it untouched.
pub(crate) fn ensure_index(
    store: &KnowledgeStore,
    loupist_id: &str,
    world_record_id: &str,
    revision: &str,
    assets: &Path,
) -> KnowledgeResult<bool> {
    let model = index::model_stamp(assets)?;
    let current = store.read(|read| {
        Ok(matches!(
            index::stamp(read.tx, loupist_id, world_record_id)?,
            Some((stamped, stored)) if stamped == revision && stored == model
        ))
    })?;
    if current {
        return Ok(true);
    }
    let texts = store.read(|read| {
        let body = load_body(read.tx, revision)?;
        index::missing_texts(read.tx, loupist_id, world_record_id, &body, &model)
    })?;
    let mut vectors: HashMap<String, Vec<f32>> = texts
        .iter()
        .cloned()
        .zip(index::embed_texts(assets, &texts)?)
        .collect();
    store.transact(|write| {
        let head = super::super::authority::require_record(write.tx, loupist_id, world_record_id)?
            .head_revision_id;
        if head.as_deref() != Some(revision) {
            return Ok(false);
        }
        let body = load_body(write.tx, revision)?;
        let missing: Vec<String> =
            index::missing_texts(write.tx, loupist_id, world_record_id, &body, &model)?
                .into_iter()
                .filter(|text| !vectors.contains_key(text))
                .collect();
        for (text, vector) in missing.iter().zip(index::embed_texts(assets, &missing)?) {
            vectors.insert(text.clone(), vector);
        }
        index::sync(
            write.tx,
            loupist_id,
            world_record_id,
            revision,
            &body,
            &model,
            &vectors,
        )?;
        Ok(true)
    })
}

/// Everything the blocking task needs, owned.
struct Prepared {
    hints: Hints,
    schema: Schema,
    instructions: String,
    episode: Episode,
    previous: Vec<PreviousEpisode>,
    graph: Graph,
    prior_nodes: HashMap<String, Value>,
    prior_claims: HashMap<String, Value>,
    annotations: Value,
    scope: Value,
    document: JobDocument,
    binding: StoredSourceBinding,
}

struct Extracted {
    claims: Vec<Value>,
    artifact: String,
    digest: String,
    vectors: HashMap<String, Vec<f32>>,
}

struct Ran {
    result: Result<Extracted, Stop>,
    uncertain: bool,
    /// A paid model call went out (completed or uncertain).
    paid: bool,
    stages: Vec<StageRecord>,
}

fn prepare(
    store: &KnowledgeStore,
    settings: &GraphProcessingSettings,
    job: &GraphJobView,
    followed: &[FollowedInstrument],
    assets: &Path,
) -> KnowledgeResult<Prepared> {
    let document = job.document.clone();
    let binding = document
        .source_binding
        .clone()
        .ok_or(KnowledgeError::Invalid(
            "graph episode has no source binding",
        ))?;
    let observed = observed_relation_names(store, &job.loupist_id);
    let hints = Hints::from_value(&extraction_hints(
        settings,
        &document.body,
        &observed,
        followed,
    ));
    let schema = merged_schema(&hints.extension);
    let instructions = extraction_instructions(&hints);
    let valid_at = text::from_isoformat(&document.reference_time)
        .ok_or(KnowledgeError::Invalid("graph reference time is invalid"))?;
    ensure_index(
        store,
        &job.loupist_id,
        &job.world_record_id,
        &job.world_revision_id,
        assets,
    )?;
    let (body, vectors, previous) = store.read(|read| {
        let body = load_body(read.tx, &job.world_revision_id)?;
        let vectors = index::load_vectors(read.tx, &job.loupist_id, &job.world_record_id)?;
        let previous = match &document.previous_episode_key {
            Some(key) => previous_episode(read.tx, &job.loupist_id, key)?,
            None => Vec::new(),
        };
        Ok((body, vectors, previous))
    })?;
    let mut graph = Graph::default();
    for node in &body.nodes {
        graph
            .nodes
            .push(stored_node(node, &schema, vectors.nodes.get(&node.id)));
    }
    // ensure_anchors: aliases and identifiers join an existing same-name node's summary.
    for anchor in &hints.anchors {
        let name = anchor.name.trim();
        let addition = text::prefix_chars(
            &anchor
                .aliases
                .iter()
                .chain(&anchor.identifiers)
                .filter(|item| !item.trim().is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
            2000,
        );
        if addition.is_empty() {
            continue;
        }
        for node in graph.nodes.iter_mut().filter(|node| node.name == name) {
            if node.summary.is_empty() {
                node.summary = addition.clone();
            } else if !node.summary.contains(&addition) {
                node.summary = format!("{} {addition}", node.summary);
            }
        }
    }
    for claim in &body.claims {
        if claim.endpoints.len() != 2 {
            continue;
        }
        let mut episodes: Vec<String> = Vec::new();
        for evidence in &claim.evidence {
            if !episodes.contains(&evidence.span_id) {
                episodes.push(evidence.span_id.clone());
            }
        }
        graph.edges.push(Edge {
            uuid: claim.id.clone(),
            source: claim.endpoints[0].node_id.clone(),
            target: claim.endpoints[1].node_id.clone(),
            name: claim.relation.clone(),
            fact: claim.proposition.clone(),
            episodes,
            valid_at: stored_time(claim.valid_at.as_ref()),
            invalid_at: stored_time(claim.invalid_at.as_ref()),
            expired_at: stored_time(claim.expired_at.as_ref()),
            fact_embedding: vectors.claims.get(&claim.id).cloned(),
        });
    }
    let mut prior_nodes = HashMap::new();
    for node in &body.nodes {
        prior_nodes.insert(
            node.id.clone(),
            super::super::graph_projection::worker_node(node)?,
        );
    }
    let mut prior_claims = HashMap::new();
    for claim in &body.claims {
        prior_claims.insert(
            claim.id.clone(),
            super::super::graph_projection::worker_claim(claim)?,
        );
    }
    Ok(Prepared {
        hints,
        schema,
        instructions,
        episode: Episode {
            key: document.episode_key.clone(),
            content: document.body.clone(),
            valid_at,
        },
        previous,
        graph,
        prior_nodes,
        prior_claims,
        annotations: Value::Array(super::super::graph_projection::worker_annotations(&body)),
        scope: json!({
            "generation_id": document.generation_id,
            "loupist_id": job.loupist_id,
            "world_record_id": job.world_record_id,
        }),
        document,
        binding,
    })
}

/// The completed chunk named by `previous_episode_key`, as Graphiti's
/// previous-episode context.
fn previous_episode(
    tx: &rusqlite::Transaction<'_>,
    loupist_id: &str,
    key: &str,
) -> KnowledgeResult<Vec<PreviousEpisode>> {
    use rusqlite::OptionalExtension;
    let payload: Option<String> = tx
        .query_row(
            "SELECT payload_json FROM knowledge_graph_outbox
             WHERE loupist_id = ?1 AND status = 'completed'
               AND json_extract(payload_json, '$.episodeKey') = ?2
             ORDER BY created_at_ms DESC LIMIT 1",
            rusqlite::params![loupist_id, key],
            |row| row.get(0),
        )
        .optional()?;
    let Some(payload) = payload else {
        return Ok(Vec::new());
    };
    let document: JobDocument = serde_json::from_str(&payload)?;
    Ok(vec![PreviousEpisode {
        content: document.body,
        valid_at: text::from_isoformat(&document.reference_time),
    }])
}

/// A stored node as Graphiti read it back from the index.
fn stored_node(node: &GraphNode, schema: &Schema, vector: Option<&Vec<f32>>) -> Node {
    let labels = if node.kind == "Entity" || !LABEL(&node.kind) {
        vec!["Entity".to_string()]
    } else {
        vec!["Entity".to_string(), node.kind.clone()]
    };
    let attributes = schema
        .entity_type(&node.kind)
        .map(|entity| {
            entity
                .fields
                .iter()
                .filter_map(|(name, _)| match node.properties.get(name) {
                    Some(Value::String(text)) => Some((name.clone(), Value::String(text.clone()))),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    Node {
        uuid: node.id.clone(),
        name: node.name.clone(),
        labels,
        summary: node.description.clone(),
        attributes,
        name_embedding: vector.cloned(),
        spellings: Vec::new(),
    }
}

/// The index's datetime for a stored time: instants and dates only.
fn stored_time(time: Option<&TimePoint>) -> Option<pipeline::Time> {
    let time = time?;
    let value = time.value.as_deref()?;
    match time.precision {
        TimePrecision::Instant => text::from_isoformat(value),
        TimePrecision::Date => text::from_isoformat(&format!("{value}T00:00:00+00:00")),
        _ => None,
    }
}

fn wire_time(time: Option<&TimePoint>) -> KnowledgeResult<Option<Value>> {
    match time {
        None => Ok(None),
        Some(time) => {
            let time = super::super::time::normalize_time_point(time)?;
            Ok(Some(json!({
                "precision": time.precision.as_str(),
                "value": time.value,
                "approximate": time.approximate,
            })))
        }
    }
}

fn run_pipeline(
    prepared: Prepared,
    config: BridgeConfig,
    requests: mpsc::Sender<(Value, oneshot::Sender<Answered>)>,
) -> Ran {
    let mut infer = |batch: Vec<Value>| -> Vec<Answered> {
        // Send every request before waiting on any reply.
        let waiting: Vec<_> = batch
            .into_iter()
            .map(|request| {
                let (reply, response) = oneshot::channel();
                requests
                    .blocking_send((request, reply))
                    .is_ok()
                    .then_some(response)
            })
            .collect();
        waiting
            .into_iter()
            .map(|response| {
                response
                    .and_then(|response| response.blocking_recv().ok())
                    .unwrap_or_else(|| Answered {
                        reply: not_sent(),
                        elapsed: Duration::ZERO,
                    })
            })
            .collect()
    };
    let mut bridge = Bridge::concurrent(config, &mut infer);
    let result = extract(&prepared, &mut bridge);
    Ran {
        result,
        uncertain: bridge.uncertain,
        paid: bridge.llm_calls > 0,
        stages: std::mem::take(&mut bridge.stages),
    }
}

fn extract(prepared: &Prepared, bridge: &mut Bridge<'_>) -> Result<Extracted, Stop> {
    let text = MemoryClaimText::new(
        prepared
            .graph
            .edges
            .iter()
            .map(|edge| (edge.uuid.as_str(), edge.name.as_str(), edge.fact.as_str())),
    )
    .map_err(|_| Stop::Rejected("claim text index is unavailable"))?;
    let output = pipeline::add_episode(
        pipeline::Input {
            episode: prepared.episode.clone(),
            previous: prepared.previous.clone(),
            schema: &prepared.schema,
            instructions: prepared.instructions.clone(),
            graph: prepared.graph.clone(),
            text: &text,
        },
        bridge,
    )?;
    let document = &prepared.document;
    let publication = wire_time(document.publication_time.as_ref())
        .map_err(|_| Stop::Rejected("publication time is invalid"))?
        .unwrap_or_else(|| json!({"precision": "unknown", "value": null, "approximate": false}));
    let effective = wire_time(document.effective_time.as_ref())
        .map_err(|_| Stop::Rejected("effective time is invalid"))?;
    let recorded_at_ms = chrono::Utc::now().timestamp_millis();
    let binding = &prepared.binding;
    let projected = projection::project_extraction(
        &Context {
            source_role: if binding.binding_role == "analysis" {
                "analysis"
            } else {
                "source"
            },
            binding: Binding {
                span_id: &binding.span_id,
                binding_role: &binding.binding_role,
                attribution: &binding.attribution,
            },
            publication_time: &publication,
            effective_time: effective.as_ref(),
            recorded_at_ms,
            prior_nodes: &prepared.prior_nodes,
            prior_claims: &prepared.prior_claims,
            episode_runtime: &prepared.episode.key,
        },
        &output.nodes,
        &output.edges,
    );
    // _merge_graph: stored rows, then this episode's projection by id.
    let mut nodes: HashMap<String, Value> = prepared.prior_nodes.clone();
    for node in &projected.nodes {
        if let Some(id) = node["adapter_id"].as_str() {
            nodes.insert(id.to_string(), node.clone());
        }
    }
    let mut claims: HashMap<String, Value> = prepared.prior_claims.clone();
    for claim in &projected.claims {
        if let Some(id) = claim["adapter_id"].as_str() {
            claims.insert(id.to_string(), claim.clone());
        }
    }
    if nodes.len() > 10_000 || claims.len() > 10_000 {
        return Err(Stop::Rejected("World export exceeds its bounds"));
    }
    let mut node_rows: Vec<Value> = nodes.into_values().collect();
    node_rows.sort_by(|left, right| {
        left["adapter_id"]
            .as_str()
            .cmp(&right["adapter_id"].as_str())
    });
    let mut claim_rows: Vec<Value> = claims.into_values().collect();
    claim_rows.sort_by(|left, right| {
        left["adapter_id"]
            .as_str()
            .cmp(&right["adapter_id"].as_str())
    });
    let episode = json!({
        "episode_key": document.episode_key,
        "name": document.name,
        "body": document.body,
        "source_label": if document.source_label.trim().is_empty() {
            "retained source"
        } else {
            document.source_label.as_str()
        },
        "reference_time": document.reference_time,
        "publication_time": publication,
        "effective_time": effective,
        "source_binding": wire_binding(binding),
        "created_at": text::isoformat(
            &chrono::DateTime::from_timestamp_millis(recorded_at_ms)
                .unwrap_or_default()
                .fixed_offset()
        ),
    });
    let snapshot = json!({
        "annotations": prepared.annotations,
        "claims": claim_rows,
        "episodes": [episode],
        "format": "loupist.world_graph_snapshot",
        "format_version": 1,
        "nodes": node_rows,
        "scope": prepared.scope,
    });
    let artifact = super::super::canonical::canonical_string(&snapshot);
    let digest = sha256(artifact.as_bytes());
    let mut vectors = HashMap::new();
    for node in &output.nodes {
        if let Some(vector) = &node.name_embedding {
            vectors.insert(node.name.clone(), vector.clone());
        }
    }
    for edge in &output.edges {
        if let Some(vector) = &edge.fact_embedding {
            vectors.insert(edge.fact.clone(), vector.clone());
        }
    }
    Ok(Extracted {
        claims: projected.claims,
        artifact,
        digest,
        vectors,
    })
}

fn wire_binding(binding: &StoredSourceBinding) -> Value {
    json!({
        "source_id": binding.source_id,
        "capture_id": binding.capture_id,
        "representation_id": binding.representation_id,
        "span_id": binding.span_id,
        "payload_digest": binding.payload_digest,
        "binding_role": binding.binding_role,
        "attribution": binding.attribution,
    })
}

/// The confirmed artifact must hold the exact frozen Source episode once.
fn artifact_has_bound_episode(document: &JobDocument, artifact: &str) -> bool {
    let Some(binding) = document.source_binding.as_ref() else {
        return false;
    };
    let Ok(snapshot) = serde_json::from_str::<Value>(artifact) else {
        return false;
    };
    if sha256(document.body.as_bytes()) != binding.payload_digest {
        return false;
    }
    let episodes = snapshot["episodes"].as_array().cloned().unwrap_or_default();
    let matching: Vec<&Value> = episodes
        .iter()
        .filter(|episode| episode["episode_key"].as_str() == Some(document.episode_key.as_str()))
        .collect();
    matching.len() == 1
        && matching[0]["body"].as_str() == Some(document.body.as_str())
        && matching[0]["source_binding"] == wire_binding(binding)
}

/// Per-call stage names for evaluation (no prompt or provider text).
fn write_stage_log(
    job_id: &str,
    stages: &[StageRecord],
    elapsed: Duration,
    result: &Result<Extracted, Stop>,
) {
    #[cfg(any(debug_assertions, feature = "eval-harness"))]
    {
        use std::io::Write;
        let Some(path) = std::env::var_os("LOUPIST_WORLD_STAGE_LOG") else {
            return;
        };
        let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        else {
            return;
        };
        for stage in stages {
            let mut record = json!({
                "job": job_id,
                "kind": stage.kind,
                "stage": stage.stage,
                "ms": stage.ms,
                "usage": stage.usage,
            });
            if stage.kind == "embedding" {
                record["texts"] = json!(stage.texts);
            }
            let _ = writeln!(file, "{record}");
        }
        let mut done = json!({
            "job": job_id,
            "kind": "episode",
            "stage": if result.is_ok() { "done" } else { "failed" },
            "ms": elapsed.as_millis() as u64,
            "engine": "rust",
        });
        if let Ok(extracted) = result {
            done["claims"] = json!(extracted.claims.len());
        }
        let _ = writeln!(file, "{done}");
    }
    #[cfg(not(any(debug_assertions, feature = "eval-harness")))]
    {
        let _ = (job_id, stages, elapsed, result);
    }
}
