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
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use futures_util::stream::{FuturesUnordered, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, Semaphore};

use super::super::error::{KnowledgeError, KnowledgeResult};
use super::super::figure_check::{self, FigureVerdict};
use super::super::graph_inference::{self, InferenceDisposition, InferenceReply};
use super::super::graph_jobs::{
    self, DroppedFigure, GraphJobView, GraphProcessingSettings, JobDocument, ReconciliationCause,
    StoredSourceBinding, Unpublished,
};
use super::super::model::{
    AssertionLink, AssertionSources, AssertionWitness, GraphNode, PassageRange, StatedEnd,
    TimePoint, TimePrecision, WorldTemporal, CONFLICTS_WITH_CLAIM, END_BASIS, SUPERSEDED_BY_CLAIM,
    SUPERSEDED_BY_SPAN,
};
use super::super::world_episodes::{EndBasis, ReferenceBasis};
use super::super::KnowledgeStore;
use super::bridge::{Answered, Bridge, BridgeConfig, StageRecord, Stop};
use super::hints::{extraction_hints, FollowedInstrument};
use super::ontology::{extraction_instructions, merged_schema, Hints, Schema};
use super::pipeline::{self, Edge, Episode, FigureDrops, Graph, Node, PreviousEpisode};
use super::projection::{self, Binding, Context};
use super::search::MemoryClaimText;
use super::{index, load_body, text};
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

/// Use the same database and blob directory on a blocking thread. Keep settings
/// mutations even when a transaction fails, for the caller's quarantine merge.
async fn blocking<T: Send + 'static>(
    store: &KnowledgeStore,
    settings: &mut GraphProcessingSettings,
    work: impl FnOnce(&KnowledgeStore, &mut GraphProcessingSettings) -> KnowledgeResult<T>
        + Send
        + 'static,
) -> KnowledgeResult<T> {
    let connection = store.connection.clone();
    let root = store.blobs.root().to_path_buf();
    let loupist_id = store.loupist_id.clone();
    let mut owned_settings = settings.clone();
    let (result, updated) = tokio::task::spawn_blocking(move || {
        let result = super::super::blob::BlobStore::open(root).and_then(|blobs| {
            let store = KnowledgeStore {
                connection,
                blobs,
                loupist_id,
            };
            work(&store, &mut owned_settings)
        });
        (result, owned_settings)
    })
    .await
    .map_err(|_| KnowledgeError::Invalid("World blocking task failed"))?;
    *settings = updated;
    result
}

/// Run one claimed World job without the graph worker.
// The job boundary takes explicit store, settings, authority and inference inputs.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn drive_job<F, Fut>(
    store: &KnowledgeStore,
    settings: &mut GraphProcessingSettings,
    job_id: &str,
    auth_mode: &str,
    now_ms: i64,
    followed: &[FollowedInstrument],
    assets: &Path,
    on_inference: F,
) -> KnowledgeResult<GraphJobView>
where
    F: FnMut(Value) -> Fut,
    Fut: std::future::Future<Output = InferenceReply>,
{
    drive_job_with_account(
        store,
        settings,
        job_id,
        auth_mode,
        now_ms,
        followed,
        assets,
        None,
        None,
        |_| {},
        on_inference,
    )
    .await
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn drive_job_with_slots<F, Fut>(
    store: &KnowledgeStore,
    settings: &mut GraphProcessingSettings,
    job_id: &str,
    auth_mode: &str,
    now_ms: i64,
    followed: &[FollowedInstrument],
    assets: &Path,
    call_slots: Option<Arc<Semaphore>>,
    resolution: Option<ResolutionSlot>,
    on_inference: F,
) -> KnowledgeResult<GraphJobView>
where
    F: FnMut(Value) -> Fut,
    Fut: std::future::Future<Output = InferenceReply>,
{
    drive_job_with_account(
        store,
        settings,
        job_id,
        auth_mode,
        now_ms,
        followed,
        assets,
        call_slots,
        resolution,
        |_| {},
        on_inference,
    )
    .await
}

/// `on_account` runs before the outbox budget drops and after a refund, so the
/// daily allowance's ledger includes a call before the reservation shrinks.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn drive_job_with_account<F, Fut, A>(
    store: &KnowledgeStore,
    settings: &mut GraphProcessingSettings,
    job_id: &str,
    auth_mode: &str,
    now_ms: i64,
    followed: &[FollowedInstrument],
    assets: &Path,
    call_slots: Option<Arc<Semaphore>>,
    resolution: Option<ResolutionSlot>,
    on_account: A,
    on_inference: F,
) -> KnowledgeResult<GraphJobView>
where
    F: FnMut(Value) -> Fut,
    Fut: std::future::Future<Output = InferenceReply>,
    A: FnMut(i32),
{
    let opened_at = SystemTime::now();
    let owned_id = job_id.to_owned();
    let job = match blocking(store, settings, move |store, settings| {
        store.graph_claim_running(settings, &owned_id, now_ms)
    })
    .await?
    {
        graph_jobs::GraphJobClaim::Acquired(job) => job,
        graph_jobs::GraphJobClaim::Existing(job) => return Ok(job),
    };
    let result = drive_claimed(
        store,
        settings,
        &job,
        auth_mode,
        now_ms,
        followed,
        assets,
        call_slots,
        resolution,
        on_inference,
        on_account,
        opened_at,
    )
    .await;
    match result {
        Ok(view) => Ok(view),
        Err(_) => {
            let owned_job = job.clone();
            // Any error after claiming must release ownership. A stale attempt
            // cannot settle a replacement, because finish checks its identity.
            blocking(store, settings, move |store, settings| {
                finish(
                    store,
                    settings,
                    &owned_job,
                    owned_job.document.attempt_id.as_deref().unwrap_or_default(),
                    owned_job.document.inference_attempted,
                    now_ms,
                )
            })
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn drive_claimed<F, Fut, A>(
    store: &KnowledgeStore,
    settings: &mut GraphProcessingSettings,
    job: &GraphJobView,
    auth_mode: &str,
    now_ms: i64,
    followed: &[FollowedInstrument],
    assets: &Path,
    call_slots: Option<Arc<Semaphore>>,
    resolution: Option<ResolutionSlot>,
    mut on_inference: F,
    mut on_account: A,
    opened_at: SystemTime,
) -> KnowledgeResult<GraphJobView>
where
    F: FnMut(Value) -> Fut,
    Fut: std::future::Future<Output = InferenceReply>,
    A: FnMut(i32),
{
    let job_id = job.job_id.as_str();
    let attempt_id = job.document.attempt_id.clone().unwrap_or_default();
    let opened = job.document.clone();
    if matches!(
        opened.episode_kind.as_str(),
        "sealed_snapshot" | "world_recovery"
    ) {
        let owned_job = job.clone();
        let assets = assets.to_path_buf();
        return match blocking(store, settings, move |store, settings| {
            reindex_and_complete(
                store,
                settings,
                &owned_job,
                owned_job.document.attempt_id.as_deref().unwrap_or_default(),
                &assets,
                now_ms,
            )
        })
        .await
        {
            Ok(view) => Ok(view),
            Err(_) => finish(store, settings, job, &attempt_id, false, now_ms),
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
    let revision = job.world_revision_id.clone();
    let full = match blocking(store, settings, move |store, _| {
        store.read(|read| {
            let body = super::load_body(read.tx, &revision)?;
            Ok(world_is_full(body.nodes.len(), body.claims.len()))
        })
    })
    .await
    {
        Ok(full) => full,
        Err(_) => return finish(store, settings, job, &attempt_id, false, now_ms),
    };
    if full {
        return store
            .transact(|write| graph_jobs::pause_world_full(write.tx, job_id, &attempt_id, now_ms));
    }
    let owned_job = job.clone();
    let owned_followed = followed.to_vec();
    let owned_assets = assets.to_path_buf();
    let prepared = match blocking(store, settings, move |store, settings| {
        prepare(store, settings, &owned_job, &owned_followed, &owned_assets)
    })
    .await
    {
        Ok(prepared) => prepared,
        Err(_) => return finish(store, settings, job, &attempt_id, false, now_ms),
    };
    let started = Instant::now();
    let wall_started = SystemTime::now();
    let call_timeout = Duration::from_millis(opened.extraction.timeout_ms);
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
    let mut timed_out = false;
    let mut blocked = false;
    let mut settings_changed = false;
    let mut output_failure: Option<String> = None;
    // Calls the pipeline sends together are in flight together; each one's
    // reply goes back on its own channel, so the pipeline reads them in order.
    let mut in_flight = FuturesUnordered::new();
    let joined = loop {
        tokio::select! {
            joined = &mut task => break joined,
            request = incoming.recv() => {
                let Some((request, reply)) = request else { continue };
                let model = request["kind"] == "llm";
                let owned_id = job_id.to_owned();
                let attempt = attempt_id.clone();
                if blocked {
                    let _ = reply.send(Answered { reply: not_sent(), elapsed: Duration::ZERO });
                    continue;
                }
                if model {
                    // The ledger counts this call before the outbox budget drops.
                    on_account(1);
                }
                if blocking(store, settings, move |store, _| {
                    if model {
                        store.transact(|write| graph_jobs::reserve_model_call(write.tx, &owned_id, &attempt))
                    } else {
                        store.graph_note_inference(&owned_id, &attempt)
                    }
                }).await.is_err() {
                    if model {
                        on_account(-1);
                    }
                    blocked = true;
                    let _ = reply.send(Answered { reply: not_sent(), elapsed: Duration::ZERO });
                    continue;
                }
                let received = Instant::now();
                let call = on_inference(request);
                let slots = call_slots.clone();
                in_flight.push(async move {
                    let outcome = call_after_permit(slots, model, call_timeout, call).await;
                    (outcome, reply, received, model)
                });
            }
            Some((response, reply, received, model)) = in_flight.next(), if !in_flight.is_empty() => {
                if model && response.disposition == InferenceDisposition::NotSent {
                    store.transact(|write| graph_jobs::refund_model_call(write.tx, job_id, &attempt_id))?;
                    // The call is back in the reservation before it leaves the ledger.
                    on_account(-1);
                }
                if response.disposition == InferenceDisposition::Uncertain {
                    if let Some(kind) = graph_inference::model_output_failure(&response) {
                        output_failure = Some(kind.to_string());
                    } else {
                        uncertain = true;
                        timed_out |= response.body["status"] == "timed_out";
                    }
                }
                settings_changed |= graph_inference::refused_for_settings(&response);
                let _ = reply.send(Answered { reply: response, elapsed: received.elapsed() });
            }
        }
    };
    let now_ms = now_ms.saturating_add(
        i64::try_from(opened_at.elapsed().unwrap_or_default().as_millis()).unwrap_or(i64::MAX),
    );
    let Ok(ran) = joined else {
        return finish(store, settings, job, &attempt_id, true, now_ms);
    };
    write_stage_log(job_id, &ran.stages, started.elapsed(), &ran.result);
    if let (Some(slot), Ok(extracted)) = (&resolution, &ran.result) {
        *slot.lock().unwrap_or_else(|error| error.into_inner()) = Some(extracted.resolution);
    }
    let uncertain = uncertain || ran.uncertain;
    if let Some(kind) = output_failure.clone() {
        return store.transact(|write| {
            graph_jobs::fail_model_output(write.tx, job_id, &attempt_id, &kind, now_ms)
        });
    }
    if blocked {
        return finish(store, settings, job, &attempt_id, true, now_ms);
    }
    let extracted = match ran.result {
        Ok(extracted) => extracted,
        Err(Stop::BudgetExceeded(_)) if !uncertain => {
            return store.transact(|write| {
                graph_jobs::pause_exhausted_attempt(write.tx, job_id, &attempt_id, now_ms)
            });
        }
        Err(_) => {
            // A refused call says the settings changed. A change that landed
            // while the last call was in flight shows only in the saved row.
            let settings_changed = settings_changed
                || store
                    .read(|read| graph_jobs::saved_settings(read.tx, &job.loupist_id))
                    .ok()
                    .flatten()
                    .is_some_and(|saved| graph_inference::controls_changed(&saved, settings));
            let end = Unpublished {
                uncertain,
                paid: ran.paid,
                suspended: suspended(wall_started, started),
                settings_changed,
                cause: timed_out.then_some(ReconciliationCause::TimedOut),
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
    let _ = store.transact(|write| {
        graph_jobs::note_dropped(
            write.tx,
            job_id,
            &attempt_id,
            extracted.dropped_attributes,
            extracted.dropped_edges,
            extracted.figure_mismatch,
            extracted.dropped_figures.clone(),
        )
    });
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
    let owned_job = job.clone();
    let attempt = attempt_id.clone();
    let assets = assets.to_path_buf();
    let committed = blocking(store, settings, move |store, settings| {
        let followed = store.followed_items(&owned_job.loupist_id);
        store.transact(|write| {
            let finished = graph_jobs::publish_host_extraction(
                write,
                settings,
                &owned_job.job_id,
                &attempt,
                &extracted.artifact,
                &extracted.digest,
                now_ms,
                uncertain,
                &followed,
            )?;
            if finished.status == graph_jobs::JOB_COMPLETED {
                if let Some(revision) = finished.document.sealed_revision_id.as_deref() {
                    index_revision(write, &owned_job, revision, &assets, &mut vectors)?;
                }
            }
            Ok(finished)
        })
    })
    .await;
    match committed {
        Ok(view) => Ok(view),
        Err(_) => finish(store, settings, job, &attempt_id, uncertain, now_ms),
    }
}

/// Waiting for a shared paid-call slot consumes no provider timeout. The
/// provider future is first polled only after the permit has been acquired.
async fn call_after_permit(
    slots: Option<Arc<Semaphore>>,
    paid: bool,
    timeout: Duration,
    call: impl std::future::Future<Output = InferenceReply>,
) -> InferenceReply {
    let _permit = if let Some(slots) = slots.filter(|_| paid) {
        match slots.acquire_owned().await {
            Ok(permit) => Some(permit),
            Err(_) => return not_sent(),
        }
    } else {
        None
    };
    tokio::time::timeout(timeout, call)
        .await
        .unwrap_or_else(|_| InferenceReply {
            disposition: InferenceDisposition::Uncertain,
            body: json!({"status": "timed_out"}),
        })
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

/// The head already holds at least 95% of the node or claim limit.
pub(crate) fn world_is_full(nodes: usize, claims: usize) -> bool {
    fn reached(count: usize, limit: usize) -> bool {
        count.saturating_mul(100) >= limit.saturating_mul(95)
    }
    reached(nodes, crate::knowledge::model::MAX_WORLD_NODES)
        || reached(claims, crate::knowledge::model::MAX_WORLD_CLAIMS)
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
    /// The whole Source when this job is one of several chunks.
    source_text: Option<String>,
}

struct Extracted {
    claims: Vec<Value>,
    artifact: String,
    digest: String,
    vectors: HashMap<String, Vec<f32>>,
    resolution: pipeline::ResolutionTally,
    dropped_attributes: u32,
    dropped_edges: u32,
    figure_mismatch: u32,
    dropped_figures: Vec<DroppedFigure>,
}

/// Where a job's entity-resolution counts go for its run log.
pub(crate) type ResolutionSlot = Arc<std::sync::Mutex<Option<pipeline::ResolutionTally>>>;

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
    let (body, span_sources, source_text) = store.read(|read| {
        let source_text = if document.chunk_count.is_some_and(|count| count > 1) {
            let material = super::super::source::load_revision_span(
                read.tx,
                read.blobs,
                &job.loupist_id,
                &binding.source_record_id,
                &binding.source_revision_id,
                &binding.span_id,
            )?;
            String::from_utf8(material.bytes).ok()
        } else {
            None
        };
        let body = load_body(read.tx, &job.world_revision_id)?;
        let mut span_sources = HashMap::new();
        for claim in &body.claims {
            if body.temporal.assertions.contains_key(&claim.id) {
                continue;
            }
            for span in claim
                .evidence
                .iter()
                .map(|binding| binding.span_id.as_str())
                .chain(claim.qualifier(SUPERSEDED_BY_SPAN))
            {
                if !span_sources.contains_key(span) {
                    span_sources.insert(
                        span.to_owned(),
                        stored_witness(read.tx, &job.loupist_id, span)?,
                    );
                }
            }
        }
        Ok((body, span_sources, source_text))
    })?;
    // Earlier snapshots counted single-name facts in the stored vocabulary.
    let observed = super::super::world::relation_vocabulary(&body.claims)
        .iter()
        .take(16)
        .map(|item| item.relation.clone())
        .collect::<Vec<_>>();
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
    let (vectors, previous) = store.read(|read| {
        let vectors = index::load_vectors(read.tx, &job.loupist_id, &job.world_record_id)?;
        let previous = match &document.previous_episode_key {
            Some(key) => previous_episode(read.tx, &job.loupist_id, key)?,
            None => Vec::new(),
        };
        Ok((vectors, previous))
    })?;
    let mut graph = Graph {
        links: body.temporal.links.clone(),
        ..Graph::default()
    };
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
        let assertions = body
            .temporal
            .assertions
            .get(&claim.id)
            .cloned()
            .unwrap_or_else(|| {
                let mut assertions = AssertionSources::default();
                for span in &episodes {
                    if let Some(Some((source, witness))) = span_sources.get(span) {
                        assertions.source_ids.insert(source.clone());
                        if assertions.witness.as_ref().is_none_or(|old| {
                            super::super::model::witness_order(witness, old).is_lt()
                        }) {
                            assertions.witness = Some(witness.clone());
                        }
                    }
                }
                if let Some(time) = &claim.invalid_at {
                    if claim.qualifier(SUPERSEDED_BY_CLAIM).is_none() {
                        let witness = claim
                            .qualifier(SUPERSEDED_BY_SPAN)
                            .and_then(|span| {
                                span_sources
                                    .get(span)
                                    .and_then(Option::as_ref)
                                    .map(|(_, witness)| witness.clone())
                            })
                            .or_else(|| assertions.witness.clone());
                        if let Some(witness) = witness {
                            assertions.stated = Some(StatedEnd {
                                time: time.clone(),
                                witness,
                            });
                        }
                    }
                }
                assertions
            });
        let ending = claim
            .invalid_at
            .as_ref()
            .zip(claim.qualifier(SUPERSEDED_BY_SPAN))
            .map(|(time, span)| pipeline::Ending {
                time: time.clone(),
                by: claim.qualifier(SUPERSEDED_BY_CLAIM).map(str::to_owned),
                span: span.to_owned(),
                basis: if claim.qualifier(END_BASIS) == Some("reference") {
                    EndBasis::Reference
                } else {
                    EndBasis::Stated
                },
                ender_open: false,
            });
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
            assertions,
            ending,
            conflicts: claim
                .qualifiers
                .iter()
                .filter(|qualifier| qualifier.key == CONFLICTS_WITH_CLAIM)
                .map(|qualifier| qualifier.value.clone())
                .collect(),
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
            source: Some(pipeline::EpisodeSource {
                source_id: binding.source_record_id.clone(),
                witness: AssertionWitness {
                    span: binding.span_id.clone(),
                    publication: (document.reference_basis() == ReferenceBasis::Published)
                        .then(|| stored_time(document.publication_time.as_ref()))
                        .flatten(),
                },
                basis: document.reference_basis(),
            }),
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
        source_text,
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
pub(crate) fn stored_node(node: &GraphNode, schema: &Schema, vector: Option<&Vec<f32>>) -> Node {
    let labels = if node.kind == "Entity" || !LABEL(&node.kind) {
        vec!["Entity".to_string()]
    } else {
        vec!["Entity".to_string(), node.kind.clone()]
    };
    let mut attributes: Vec<(String, Value)> = schema
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
    for key in [
        "ticker",
        "tickers",
        "symbol",
        "symbols",
        "ticker_symbol",
        "exchange",
        "exchanges",
        "market",
        "isin",
        "figi",
        "lei",
        "cik",
        "corp_code",
        "dart_corp_code",
        "identifier",
        "identifiers",
    ] {
        if attributes.iter().any(|(name, _)| name == key) {
            continue;
        }
        if let Some(value) = node.properties.get(key) {
            if matches!(value, Value::String(_) | Value::Number(_) | Value::Array(_)) {
                attributes.push((key.to_string(), value.clone()));
            }
        }
    }
    let mut spellings = Vec::new();
    let mut keep_spelling = |spelling: &str| {
        let spelling = spelling.trim();
        if spelling.is_empty()
            || spelling == node.name
            || spellings.iter().any(|kept: &String| kept == spelling)
        {
            return;
        }
        spellings.push(spelling.to_string());
    };
    for alias in &node.aliases {
        keep_spelling(alias);
    }
    if let Some(original) = node.properties.get("original_name").and_then(Value::as_str) {
        keep_spelling(original);
    }
    Node {
        uuid: node.id.clone(),
        name: node.name.clone(),
        labels,
        summary: node.description.clone(),
        attributes,
        name_embedding: vector.cloned(),
        spellings,
    }
}

fn stored_witness(
    tx: &rusqlite::Transaction<'_>,
    owner: &str,
    span: &str,
) -> KnowledgeResult<Option<(String, AssertionWitness)>> {
    let visible = match super::super::source::require_any_visible_span(tx, owner, span) {
        Ok(visible) => visible,
        Err(KnowledgeError::Unauthorized(_) | KnowledgeError::NotFound) => return Ok(None),
        Err(error) => return Err(error),
    };
    let publication: Option<String> = tx.query_row(
        "SELECT publication_time_json FROM knowledge_source_captures WHERE record_id = ?1 AND capture_id = ?2",
        rusqlite::params![visible.record_id, visible.capture_id],
        |row| row.get(0),
    )?;
    let publication = publication
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    Ok(Some((
        visible.record_id,
        AssertionWitness {
            span: span.to_owned(),
            publication: stored_time(publication.as_ref()),
        },
    )))
}

fn stored_time(time: Option<&TimePoint>) -> Option<pipeline::Time> {
    time.filter(|time| time.precision != TimePrecision::Unknown)
        .cloned()
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
    // The whole Source is scanned once for this chunk's figures, however many facts it has.
    let source = prepared
        .source_text
        .as_deref()
        .map(crate::knowledge::numbers::FigurePassage::new);
    let output = pipeline::add_episode(
        pipeline::Input {
            episode: prepared.episode.clone(),
            previous: prepared.previous.clone(),
            schema: &prepared.schema,
            instructions: prepared.instructions.clone(),
            graph: prepared.graph.clone(),
            text: &text,
            source: source
                .as_ref()
                .map(|source| source as &dyn figure_check::Passage),
        },
        bridge,
    )?;
    let document = &prepared.document;
    let publication = wire_time(document.publication_time.as_ref())
        .map_err(|_| Stop::Rejected("publication time is invalid"))?
        .unwrap_or_else(|| json!({"precision": "unknown", "value": null, "approximate": false}));
    let effective = wire_time(document.effective_time.as_ref())
        .map_err(|_| Stop::Rejected("effective time is invalid"))?;
    let recorded_at_ms = recorded_now_ms();
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
            effective_time: effective.as_ref(),
            recorded_at_ms,
            prior_nodes: &prepared.prior_nodes,
            prior_claims: &prepared.prior_claims,
            episode_runtime: &prepared.episode.key,
        },
        &output.nodes,
        &output.edges,
    );
    let mut projected = projected;
    let mut figures = output.figures.clone();
    let chunk = crate::knowledge::numbers::FigurePassage::new(&prepared.episode.content);
    let mut passages: Vec<&dyn figure_check::Passage> = vec![&chunk];
    passages.extend(
        source
            .as_ref()
            .map(|source| source as &dyn figure_check::Passage),
    );
    check_support(
        &mut projected.claims,
        &prepared.prior_claims,
        &binding.span_id,
        &passages,
        chunk_passage(document),
        &mut figures,
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
            &text::instant(chrono::DateTime::from_timestamp_millis(recorded_at_ms)
                .unwrap_or_default()
                .fixed_offset())
        ),
    });
    let to_claim = |id: &str| projection::claim_id(&prepared.prior_claims, id);
    let mut temporal = WorldTemporal::default();
    for link in output.links {
        let mut ids = [to_claim(&link.left), to_claim(&link.right)];
        ids.sort();
        let [left, right] = ids;
        temporal.links.push(AssertionLink {
            left,
            right,
            same_assertion: link.same_assertion,
            selected_by: link.selected_by,
            same_assertion_by: link.same_assertion_by,
            reaffirmed_by: link.reaffirmed_by,
        });
    }
    temporal
        .links
        .sort_unstable_by(|a, b| (&a.left, &a.right).cmp(&(&b.left, &b.right)));
    for edge in prepared.graph.edges.iter().chain(&output.edges) {
        temporal
            .assertions
            .insert(to_claim(&edge.uuid), edge.assertions.clone());
    }
    let snapshot = json!({
        "annotations": prepared.annotations,
        "claims": claim_rows,
        "episodes": [episode],
        "format": "loupist.world_graph_snapshot",
        "format_version": 1,
        "nodes": node_rows,
        "scope": prepared.scope,
        "temporal": temporal,
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
        resolution: output.resolution,
        dropped_attributes: output.dropped.attributes,
        dropped_edges: output.dropped.edges,
        figure_mismatch: figures.count,
        dropped_figures: figures
            .examples
            .into_iter()
            .map(|example| DroppedFigure {
                proposition: example.proposition,
                stated: example.stated,
                passage: chunk_passage(document),
            })
            .collect(),
    })
}

/// The chunk of its Source a job extracts from, when the job records it.
fn chunk_passage(document: &JobDocument) -> Option<PassageRange> {
    Some(PassageRange {
        chunk_index: document.chunk_index?,
        char_start: document.char_start?,
        char_end: document.char_end?,
    })
}

/// Checks the figures of each claim this chunk newly supports against the
/// chunk, then the whole Source, and stores the verdict and the chunk's range
/// on that support. Support whose figure differs is left out, as extraction
/// leaves out such a fact; a claim it would have been the only support of keeps it.
fn check_support(
    claims: &mut [Value],
    prior: &HashMap<String, Value>,
    span: &str,
    passages: &[&dyn figure_check::Passage],
    passage: Option<PassageRange>,
    drops: &mut FigureDrops,
) {
    let known = |claim: &Value, item: &Value| {
        claim["adapter_id"]
            .as_str()
            .and_then(|id| prior.get(id))
            .and_then(|prior| prior["evidence"].as_array())
            .is_some_and(|evidence| {
                evidence
                    .iter()
                    .any(|old| old["span_id"] == item["span_id"] && old["role"] == item["role"])
            })
    };
    for claim in claims {
        let Some(evidence) = claim["evidence"].as_array() else {
            continue;
        };
        let added: Vec<usize> = evidence
            .iter()
            .enumerate()
            .filter(|(_, item)| item["span_id"] == span && !known(claim, item))
            .map(|(index, _)| index)
            .collect();
        if added.is_empty() {
            continue;
        }
        let proposition = claim["proposition"].as_str().unwrap_or_default().to_owned();
        let check = figure_check::check_in(&proposition, passages);
        let evidence = claim["evidence"]
            .as_array_mut()
            .expect("evidence was an array");
        if check.verdict == FigureVerdict::Differs {
            if evidence.len() > added.len() {
                for index in added.into_iter().rev() {
                    evidence.remove(index);
                }
                drops.note(&proposition, &check);
            }
            continue;
        }
        let stored = check.verdict.stored();
        for index in added {
            let item = &mut evidence[index];
            if let Some(stored) = stored {
                item["figure_check"] = json!(stored.as_str());
            }
            if let Some(range) = passage {
                item["passage"] = json!({
                    "chunk_index": range.chunk_index,
                    "char_start": range.char_start,
                    "char_end": range.char_end,
                });
            }
        }
    }
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

/// When an extraction is recorded: now, or an evaluation's installed clock,
/// so a fixture can record a publication on the day it states.
fn recorded_now_ms() -> i64 {
    #[cfg(feature = "eval-harness")]
    if let Some(now) = crate::eval_harness::clock::local_now() {
        return now.timestamp_millis();
    }
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn a_queued_call_gets_its_full_timeout_after_the_permit() {
        let slots = Arc::new(Semaphore::new(0));
        let future = call_after_permit(Some(slots.clone()), true, Duration::from_secs(1), async {
            tokio::time::sleep(Duration::from_millis(900)).await;
            InferenceReply {
                disposition: InferenceDisposition::Completed,
                body: json!({}),
            }
        });
        tokio::pin!(future);
        tokio::select! {
            _ = &mut future => panic!("the call cannot start without a slot"),
            _ = tokio::time::sleep(Duration::from_secs(10)) => {}
        }
        slots.add_permits(1);
        assert_eq!(future.await.disposition, InferenceDisposition::Completed);
    }

    #[tokio::test(start_paused = true)]
    async fn a_started_call_still_times_out_and_a_closed_queue_sends_nothing() {
        let reply = call_after_permit(
            Some(Arc::new(Semaphore::new(1))),
            true,
            Duration::from_secs(1),
            std::future::pending(),
        )
        .await;
        assert_eq!(reply.disposition, InferenceDisposition::Uncertain);
        assert_eq!(reply.body["status"], "timed_out");
        let slots = Arc::new(Semaphore::new(0));
        slots.close();
        let reply = call_after_permit(Some(slots), true, Duration::from_secs(1), async {
            panic!("a closed queue cannot poll the provider")
        })
        .await;
        assert_eq!(reply.disposition, InferenceDisposition::NotSent);
    }

    const SOURCE: &str = "한빛소재의 2025년 매출액은 1,234억 원이고 영업이익은 98억 원이다.";

    fn edge(fact: &str) -> Edge {
        Edge {
            uuid: fact.into(),
            source: "hanbit".into(),
            target: "hanbit".into(),
            name: "REPORTS".into(),
            fact: fact.into(),
            episodes: vec!["ep_current".into()],
            valid_at: None,
            invalid_at: None,
            expired_at: None,
            fact_embedding: None,
            assertions: AssertionSources::default(),
            ending: None,
            conflicts: Vec::new(),
        }
    }

    #[test]
    fn extraction_leaves_out_nine_differing_facts_and_keeps_eight_examples() {
        let mut facts: Vec<String> = [-3, -2, -1, 1, 2, 3, 4, 5, 6]
            .iter()
            .map(|power: &i32| {
                format!(
                    "Revenue was KRW {} won.",
                    1_234u64 * 10u64.pow((8 + power) as u32)
                )
            })
            .collect();
        facts.push("Revenue was KRW 123.4 billion.".into());
        facts.push("The operating margin was 7.9%.".into());
        let edges: Vec<Edge> = facts.iter().map(|fact| edge(fact)).collect();
        let mut extracted: Vec<usize> = (0..edges.len()).collect();
        let drops = pipeline::drop_mismatched_figures(
            &edges,
            &mut extracted,
            &[&crate::knowledge::numbers::FigurePassage::new(SOURCE)
                as &dyn figure_check::Passage],
        );
        assert_eq!(extracted, vec![9, 10]);
        assert_eq!(drops.count, 9);
        assert_eq!(drops.examples.len(), pipeline::FIGURE_EXAMPLES);
        assert_eq!(drops.examples[0].proposition, facts[0]);
        assert_eq!(drops.examples[0].stated, "123400000 won");
    }

    fn claim(id: &str, proposition: &str, spans: &[&str]) -> Value {
        json!({
            "adapter_id": id,
            "proposition": proposition,
            "evidence": spans.iter().map(|span| json!({
                "span_id": span, "role": "supports", "attribution": "publisher"
            })).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn new_support_stores_its_verdict_and_chunk_and_differing_support_is_not_added() {
        let prior = HashMap::from([
            (
                "clm_kept".to_string(),
                claim("clm_kept", "Revenue was KRW 12.34 billion.", &["spn_old"]),
            ),
            (
                "clm_margin".to_string(),
                claim("clm_margin", "The margin was 7.9%.", &["spn_old"]),
            ),
        ]);
        let mut claims = vec![
            // Resolution matched this chunk's fact with a claim whose figure differs.
            claim(
                "clm_kept",
                "Revenue was KRW 12.34 billion.",
                &["spn_old", "spn_new"],
            ),
            claim(
                "clm_margin",
                "The margin was 7.9%.",
                &["spn_old", "spn_new"],
            ),
            claim("clm_new", "Revenue was KRW 123.4 billion.", &["spn_new"]),
            claim(
                "clm_plain",
                "Hanbit Materials supplies Northwind Rail.",
                &["spn_new"],
            ),
        ];
        let range = PassageRange {
            chunk_index: 1,
            char_start: 120,
            char_end: 160,
        };
        let mut drops = FigureDrops::default();
        check_support(
            &mut claims,
            &prior,
            "spn_new",
            &[&crate::knowledge::numbers::FigurePassage::new(SOURCE)
                as &dyn figure_check::Passage],
            Some(range),
            &mut drops,
        );
        assert_eq!(drops.count, 1);
        assert_eq!(claims[0]["evidence"], prior["clm_kept"]["evidence"]);
        let passage = json!({"chunk_index": 1, "char_start": 120, "char_end": 160});
        assert_eq!(claims[1]["evidence"][0].get("figure_check"), None);
        assert_eq!(claims[1]["evidence"][1]["figure_check"], "not_found");
        assert_eq!(claims[2]["evidence"][0]["figure_check"], "matched");
        assert_eq!(claims[2]["evidence"][0]["passage"], passage);
        // No figure: nothing checked, the range is still known.
        assert_eq!(claims[3]["evidence"][0].get("figure_check"), None);
        assert_eq!(claims[3]["evidence"][0]["passage"], passage);
        // The stored claim keeps both fields through a later job's round trip.
        let stored = super::super::super::worker::adapt_worker_claim(&json!({
            "adapter_id": "clm_new", "proposition": "Revenue was KRW 123.4 billion.",
            "attribution": "publisher", "relation": "REPORTS",
            "endpoints": [{"node_id": "nod_a", "role": "source"}, {"node_id": "nod_a", "role": "target"}],
            "evidence": claims[2]["evidence"], "status": "active", "qualifiers": [],
        }))
        .unwrap();
        assert_eq!(
            stored.evidence[0].figure_check,
            Some(super::super::super::EvidenceFigure::Matched)
        );
        assert_eq!(stored.evidence[0].passage, Some(range));
        let again = super::super::super::graph_projection::worker_claim(&stored).unwrap();
        assert_eq!(again["evidence"], claims[2]["evidence"]);
    }
}
