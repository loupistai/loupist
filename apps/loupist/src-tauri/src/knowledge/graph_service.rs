//! Wakeable in-process World, learning and schema job loop.
//!
//! Jobs live in each Loupist folder database. This loop aggregates candidates
//! from registered folders. Pi owns the shared provider credentials; extraction receives only
//! host-mediated inference results, never keys or OAuth tokens.
//!
//! Schema reviews, method learning and quarantine recovery run one unit at a
//! time. World jobs run up to [`MAX_WORLD_JOBS`] at once, never two for one
//! Loupist (each publishes on its own World head), and their paid model calls
//! share [`MAX_WORLD_CALLS`] slots. Runnable jobs start as user attempts, then
//! chat-origin work, then background, oldest first inside a class. After three
//! chat-origin claims in a row, one runnable background job is claimed.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tokio::{
    sync::{Notify, Semaphore},
    task::JoinSet,
};

use super::blob::BlobStore;
use super::error::{KnowledgeError, KnowledgeResult};
use super::graph_inference::dispatch_inference;
use super::graph_jobs::{load_settings, store_settings, GraphProcessingSettings};
use super::world_extract::hints::FollowedInstrument;
use super::KnowledgeStore;
use crate::runs::maintenance::{MaintenanceBlocker, MaintenanceCoordinator};
use crate::store::SqliteStore;

pub struct GraphSupervisor {
    // Background work owns Inner, but must not keep the application owner alive.
    lifetime: Arc<SupervisorLifetime>,
    inner: Arc<Inner>,
}

struct SupervisorLifetime {
    inner: std::sync::Weak<Inner>,
}

impl Drop for SupervisorLifetime {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            inner.cancel();
        }
    }
}

struct Inner {
    store: Arc<SqliteStore>,
    inference: Option<crate::providers::inference::ProviderInferenceService>,
    maintenance: MaintenanceCoordinator,
    app_data: PathBuf,
    resource_dir: Option<PathBuf>,
    notify: Notify,
    shutdown: Notify,
    started: AtomicBool,
    startup_recovered: std::sync::Mutex<bool>,
    learning_events: tokio::sync::broadcast::Sender<crate::events::LearningReviewFinished>,
    schema_events: tokio::sync::broadcast::Sender<crate::events::WorldSchemaChanged>,
    stopped: AtomicBool,
    task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Serializes World selection and publication of its in-process claim.
    selection: std::sync::Mutex<()>,
    /// Schema and learning share one lane, independent of World jobs.
    reviews_in_flight: AtomicBool,
    /// World jobs in flight, by Loupist.
    running: std::sync::Mutex<HashMap<String, Worker>>,
    /// Jobs that ran without progress; skipped until a wake from outside the loop.
    stalled: std::sync::Mutex<HashSet<String>>,
    /// Waiting Source spans (`revision:span`) a queue pass has already read to
    /// tell price data apart, so each is read once per process.
    price_read: std::sync::Mutex<std::collections::BTreeSet<String>>,
    /// A wake from outside the loop (a save, a setting, a connection) may
    /// unblock a stalled job; a finished worker's wake does not.
    external: AtomicBool,
    /// Recent Chat turns per Loupist.
    attention: std::sync::Mutex<HashMap<String, Attention>>,
    /// When each Loupist last started a World job, as a sequence number.
    served: std::sync::Mutex<HashMap<String, u64>>,
    /// Chat-origin claims in a row, including user attempts. A background claim clears it.
    chat_claims: Mutex<u8>,
    serve_sequence: AtomicU64,
    /// In-flight paid World model calls across every Loupist.
    call_slots: Arc<Semaphore>,
    keep_awake: crate::power::KeepAwake,
}

/// World jobs that run at once, each for a different Loupist.
pub(crate) const MAX_WORLD_JOBS: usize = 2;
/// Paid World model calls in flight at once. Several jobs, and calls one
/// episode sends together, share them, so a provider sees a bounded load.
pub(crate) const MAX_WORLD_CALLS: usize = 4;
/// A Loupist with a Chat turn this recent goes first.
const ATTENTION_WINDOW_MS: i64 = 30 * 60 * 1000;

struct Worker {
    job_id: String,
    abort: Option<tokio::task::AbortHandle>,
}

struct WorldClaim {
    inner: Arc<Inner>,
    loupist: String,
}

impl Drop for WorldClaim {
    fn drop(&mut self) {
        self.inner
            .running
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.loupist);
        self.inner.notify.notify_one();
    }
}

#[derive(Default)]
struct Attention {
    at_ms: i64,
    chats: HashMap<String, i64>,
}

/// A World job chosen to run, with the settings it was chosen under.
struct Selected {
    job: super::graph_jobs::GraphJobView,
    settings: GraphProcessingSettings,
    auth_mode: String,
}

/// Lower ranks run first. A Loupist with a recent Chat turn comes before the
/// rest, latest turn first; the others go by when they last started a job,
/// longest ago (or never) first, then by the age of their next job.
#[cfg(test)]
pub(super) fn schedule_rank(
    attended_at: Option<i64>,
    served: u64,
    created_at_ms: i64,
) -> (u8, i64, i64) {
    match attended_at {
        Some(at) => (0, -at, created_at_ms),
        None => (1, served as i64, created_at_ms),
    }
}

/// Inference the host made for one World job, by kind. Refusals are not calls.
#[derive(Default)]
struct WorldCalls {
    llm: AtomicU64,
    embedding: AtomicU64,
}

impl WorldCalls {
    fn observe(&self, kind: &str, reply: &super::graph_inference::InferenceReply) {
        if reply.disposition == super::graph_inference::InferenceDisposition::NotSent {
            return;
        }
        let counter = match kind {
            "llm" => &self.llm,
            "embedding" => &self.embedding,
            _ => return,
        };
        counter.fetch_add(1, Ordering::SeqCst);
    }

    fn record(&self, run_log: &mut crate::runs::log::RunLog) {
        run_log
            .inference_calls
            .insert("llm".into(), self.llm.load(Ordering::SeqCst));
        run_log
            .inference_calls
            .insert("embedding".into(), self.embedding.load(Ordering::SeqCst));
    }
}

impl Clone for GraphSupervisor {
    fn clone(&self) -> Self {
        Self {
            lifetime: self.lifetime.clone(),
            inner: self.inner.clone(),
        }
    }
}

impl GraphSupervisor {
    pub fn with_inference(
        store: Arc<SqliteStore>,
        app_data: PathBuf,
        resource_dir: Option<PathBuf>,
        maintenance: MaintenanceCoordinator,
        inference: Option<crate::providers::inference::ProviderInferenceService>,
    ) -> Self {
        // Tests never hold a real power assertion.
        #[cfg(not(test))]
        let keep_awake = crate::power::KeepAwake::new(store.clone());
        #[cfg(test)]
        let keep_awake = crate::power::KeepAwake::with_backend(
            store.clone(),
            crate::power::testing::FakePower::new(true),
        );
        let inner = Arc::new(Inner {
            keep_awake,
            store,
            inference,
            maintenance,
            app_data,
            resource_dir,
            notify: Notify::new(),
            shutdown: Notify::new(),
            started: AtomicBool::new(false),
            startup_recovered: std::sync::Mutex::new(false),
            learning_events: tokio::sync::broadcast::channel(64).0,
            schema_events: tokio::sync::broadcast::channel(64).0,
            stopped: AtomicBool::new(false),
            task: std::sync::Mutex::new(None),
            selection: std::sync::Mutex::new(()),
            reviews_in_flight: AtomicBool::new(false),
            running: std::sync::Mutex::new(HashMap::new()),
            stalled: std::sync::Mutex::new(HashSet::new()),
            price_read: std::sync::Mutex::new(std::collections::BTreeSet::new()),
            external: AtomicBool::new(false),
            attention: std::sync::Mutex::new(HashMap::new()),
            served: std::sync::Mutex::new(HashMap::new()),
            chat_claims: Mutex::new(0),
            serve_sequence: AtomicU64::new(0),
            call_slots: Arc::new(Semaphore::new(MAX_WORLD_CALLS)),
        });
        Self {
            lifetime: Arc::new(SupervisorLifetime {
                inner: Arc::downgrade(&inner),
            }),
            inner,
        }
    }

    /// The settings the next World job of this Loupist runs with.
    #[cfg(test)]
    pub(crate) fn settings_for_work(
        &self,
        loupist_id: &str,
    ) -> KnowledgeResult<GraphProcessingSettings> {
        self.inner.settings_for_work(loupist_id)
    }

    pub fn runtime_available(&self) -> bool {
        self.semantic_assets().is_some() && self.inner.inference.is_some()
    }

    pub fn semantic_assets(&self) -> Option<PathBuf> {
        let assets = embed_assets(self.inner.resource_dir.as_deref());
        assets.join("config.json").is_file().then_some(assets)
    }

    /// The bundled embedding model if it has loaded. Until then this returns
    /// `None` at once, and searches rank by words and Map names alone: the
    /// first load takes seconds, and a search never waits for it.
    pub fn semantic_model(&self) -> Option<super::LocalModel> {
        self.semantic_lookup().model
    }

    /// The model if it has loaded, and otherwise whether it is still loading,
    /// in one look: a search that ranked without it says so even when the load
    /// ends a moment later.
    pub(crate) fn semantic_lookup(&self) -> super::ModelLookup {
        self.semantic_assets()
            .map(|assets| super::loaded_local_model(&assets))
            .unwrap_or_default()
    }

    /// Start loading the embedding model on its own thread, off the launch path.
    pub fn prewarm_semantic_model(&self) {
        let _ = self.semantic_model();
    }

    /// Tests and headless evals rank with the model from their first search.
    #[cfg(any(test, feature = "eval-harness"))]
    pub fn wait_for_semantic_model(&self) -> bool {
        self.semantic_assets()
            .is_some_and(|assets| super::local_model(&assets).is_ok())
    }

    /// Start only after the application owns this library and has stopped its
    /// orphaned process groups. Recovery and the first wake need no UI action.
    pub fn start(&self) -> KnowledgeResult<()> {
        let mut recovered = self
            .inner
            .startup_recovered
            .lock()
            .map_err(|_| KnowledgeError::LockPoisoned)?;
        if !*recovered {
            let store = Arc::clone(&self.inner.store);
            self.inner
                .store
                .for_each_database(|_, connection| {
                    let tx = connection
                        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                    super::graph_jobs::recover_with_usage(store.as_ref(), &tx)
                        .map_err(|e| crate::store::StoreError::Invalid(e.to_string()))?;
                    super::learning::recover(&tx, now_ms())
                        .map_err(|e| crate::store::StoreError::Invalid(e.to_string()))?;
                    tx.commit()?;
                    Ok(())
                })
                .map_err(|_| KnowledgeError::Invalid("could not recover Loupist databases"))?;
            *recovered = true;
        }
        self.wake();
        Ok(())
    }

    pub fn subscribe_learning(
        &self,
    ) -> tokio::sync::broadcast::Receiver<crate::events::LearningReviewFinished> {
        self.inner.learning_events.subscribe()
    }

    pub fn subscribe_schema(
        &self,
    ) -> tokio::sync::broadcast::Receiver<crate::events::WorldSchemaChanged> {
        self.inner.schema_events.subscribe()
    }

    pub fn wake(&self) {
        self.ensure_started();
        self.inner.external.store(true, Ordering::SeqCst);
        self.inner.notify.notify_one();
    }

    pub(crate) fn keep_awake(&self) -> &crate::power::KeepAwake {
        &self.inner.keep_awake
    }

    /// A Chat turn started in `chat_id`: this Loupist's World jobs, and among
    /// them the Sources that Chat saves, go first for a while.
    pub fn note_attention(&self, loupist_id: &str, chat_id: &str) {
        let at = now_ms();
        let mut attention = self
            .inner
            .attention
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        attention.retain(|_, entry| at.saturating_sub(entry.at_ms) <= ATTENTION_WINDOW_MS);
        let entry = attention.entry(loupist_id.to_string()).or_default();
        entry.at_ms = at;
        entry
            .chats
            .retain(|_, seen| at.saturating_sub(*seen) <= ATTENTION_WINDOW_MS);
        entry.chats.insert(chat_id.to_string(), at);
    }

    pub fn ensure_started(&self) {
        if self.inner.stopped.load(Ordering::SeqCst) {
            return;
        }
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let inner = self.inner.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let task = handle.spawn(async move {
                    Inner::run(inner).await;
                });
                *self.inner.task.lock().expect("graph task lock") = Some(task);
            }
            Err(_) => self.inner.started.store(false, Ordering::SeqCst),
        }
    }

    pub async fn shutdown(&self) -> KnowledgeResult<()> {
        if let Some(task) = self.inner.cancel_task() {
            let _ = task.await;
        }
        Ok(())
    }

    #[cfg(any(test, feature = "eval-harness"))]
    pub async fn pump_once(&self) -> KnowledgeResult<Option<String>> {
        if self.inner.stopped.load(Ordering::SeqCst) {
            return Ok(None);
        }
        self.inner.until_shutdown(self.inner.pump_once()).await
    }

    #[cfg(any(test, feature = "eval-harness"))]
    pub fn index_sealed_world(
        &self,
        loupist_id: &str,
        world_record_id: &str,
        world_revision_id: &str,
    ) -> KnowledgeResult<super::graph_jobs::GraphJobView> {
        let knowledge =
            loupist_knowledge(self.inner.store.clone(), loupist_id, &self.inner.app_data)?;
        let settings = load_settings(
            &*self
                .inner
                .store
                .lock_loupist(loupist_id)
                .map_err(|_| KnowledgeError::LockPoisoned)?,
            loupist_id,
        )?;
        let job = knowledge.graph_accept_sealed_world(
            &settings,
            super::graph_jobs::SealedWorldIndex {
                loupist_id: loupist_id.to_string(),
                world_record_id: world_record_id.to_string(),
                world_revision_id: world_revision_id.to_string(),
                at_ms: now_ms(),
            },
        )?;
        self.wake();
        Ok(job)
    }

    pub async fn query_indexed(
        &self,
        loupist_id: &str,
        world_record_id: &str,
        world_revision_id: &str,
        search: &str,
    ) -> KnowledgeResult<serde_json::Value> {
        self.inner
            .until_shutdown(self.query_indexed_inner(
                loupist_id,
                world_record_id,
                world_revision_id,
                search,
            ))
            .await
    }

    async fn query_indexed_inner(
        &self,
        loupist_id: &str,
        world_record_id: &str,
        world_revision_id: &str,
        search: &str,
    ) -> KnowledgeResult<serde_json::Value> {
        if search.is_empty() || search.len() > 500 {
            return Err(KnowledgeError::Invalid("graph query is invalid"));
        }
        let knowledge =
            loupist_knowledge(self.inner.store.clone(), loupist_id, &self.inner.app_data)?;
        let revision = knowledge.get_revision(loupist_id, world_revision_id)?;
        if revision.record_id != world_record_id || revision.kind != super::model::RecordKind::World
        {
            return Err(KnowledgeError::NotFound);
        }
        // Local embeddings and SQLite only: no model call. While the model is
        // still loading, answer from the snapshot rather than wait for it.
        let assets = embed_assets(self.inner.resource_dir.as_deref());
        if super::loaded_local_model(&assets).model.is_none() {
            return canonical_snapshot(
                &knowledge,
                loupist_id,
                world_record_id,
                world_revision_id,
                search,
            );
        }
        let (loupist, world, revision, query) = (
            loupist_id.to_string(),
            world_record_id.to_string(),
            world_revision_id.to_string(),
            search.to_string(),
        );
        let indexed = tokio::task::spawn_blocking(move || {
            let result = super::world_extract::query_world(
                &knowledge, &loupist, &world, &revision, &query, &assets,
            );
            (knowledge, result)
        })
        .await
        .map_err(|_| KnowledgeError::Invalid("World search failed"))?;
        let (knowledge, result) = indexed;
        match result? {
            Some(view) => Ok(view),
            None => canonical_snapshot(
                &knowledge,
                loupist_id,
                world_record_id,
                world_revision_id,
                search,
            ),
        }
    }
}

fn canonical_snapshot(
    knowledge: &KnowledgeStore,
    loupist_id: &str,
    world_record_id: &str,
    world_revision_id: &str,
    search: &str,
) -> KnowledgeResult<serde_json::Value> {
    let view = knowledge.query_world(super::model::WorldGraphQuery {
        loupist_id: loupist_id.to_string(),
        record_id: world_record_id.to_string(),
        revision_id: world_revision_id.to_string(),
        search: search.to_string(),
        center_claim_id: None,
        center_node_id: None,
        kinds: Vec::new(),
        relations: Vec::new(),
        binding_roles: Vec::new(),
        statuses: Vec::new(),
        publication_times: Vec::new(),
        as_of: None,
        cursor: None,
        node_limit: Some(25),
        edge_limit: Some(25),
        byte_limit: Some(262_144),
    })?;
    Ok(serde_json::json!({
        "mode": "snapshot_lexical",
        "indexed": false,
        "revisionId": world_revision_id,
        "generationId": null,
        "view": view
    }))
}

fn not_sent_reply(
    _: serde_json::Value,
) -> std::future::Ready<super::graph_inference::InferenceReply> {
    std::future::ready(super::graph_inference::InferenceReply {
        disposition: super::graph_inference::InferenceDisposition::NotSent,
        body: serde_json::json!({"status": "not_sent"}),
    })
}

impl Inner {
    fn cancel_task(&self) -> Option<tokio::task::JoinHandle<()>> {
        self.stopped.store(true, Ordering::SeqCst);
        self.keep_awake.release();
        self.shutdown.notify_waiters();
        self.notify.notify_waiters();
        let task = self.task.lock().expect("graph task lock").take();
        if let Some(task) = &task {
            task.abort();
        }
        // A worker cut off here stays running in SQLite; the next start
        // recovers it within its host allowance.
        for worker in self
            .running
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
        {
            if let Some(abort) = &worker.abort {
                abort.abort();
            }
        }
        task
    }

    fn cancel(&self) {
        drop(self.cancel_task());
    }

    async fn until_shutdown<T>(
        &self,
        operation: impl std::future::Future<Output = KnowledgeResult<T>>,
    ) -> KnowledgeResult<T> {
        let mut operation = Box::pin(operation);
        let shutdown = self.shutdown.notified();
        tokio::pin!(shutdown);
        // Register before checking the flag; shutdown cannot be lost between
        // that check and the first poll of the IPC/startup operation.
        shutdown.as_mut().enable();
        if self.stopped.load(Ordering::SeqCst) {
            return Err(KnowledgeError::Invalid("graph runtime is stopped"));
        }
        let result = tokio::select! {
            biased;
            _ = &mut shutdown => Err(KnowledgeError::Invalid("graph runtime is stopped")),
            result = &mut operation => result,
        };
        drop(operation);
        result
    }

    async fn run(self: Arc<Self>) {
        let mut checked_at = now_ms();
        let mut reviews = JoinSet::new();
        let mut review_requested = false;
        loop {
            if self.stopped.load(Ordering::SeqCst) {
                break;
            }
            // Register before inspecting SQLite so shutdown cannot be lost
            // during the synchronous deadline lookup.
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.stopped.load(Ordering::SeqCst) {
                break;
            }
            let next_learning = self
                .store
                .for_each_readable_database(|_, connection| {
                    let tx = connection.transaction()?;
                    super::learning::next_wake_at(&tx, checked_at)
                        .map_err(|e| crate::store::StoreError::Invalid(e.to_string()))
                })
                .ok()
                .and_then(|deadlines| deadlines.into_iter().flatten().min());
            // A held account's next probe also wakes the loop (providers spec §3.1).
            // A probe that is already due was offered to the last pump.
            let next_probe =
                crate::providers::status::next_probe_at(&self.store).filter(|at| *at > now_ms());
            // While the Mac is kept awake, notice a switch to battery power.
            let next_power = self
                .keep_awake
                .poll_interval()
                .map(|interval| now_ms().saturating_add(interval.as_millis() as i64));
            // Held jobs stay queued. Local midnight is when the allowance starts again.
            let next_midnight = next_local_midnight_ms(now_ms());
            let next_ending = match self.store.for_each_database(|_, connection| {
                let tx = connection.transaction()?;
                let next = super::world_followups::reconcile_worlds(&tx, now_ms())
                    .map_err(|error| crate::store::StoreError::Invalid(error.to_string()))?;
                tx.commit()?;
                Ok(next)
            }) {
                Ok(deadlines) => deadlines.into_iter().flatten().min(),
                Err(error) => {
                    eprintln!("Could not reconcile World endings: {error}");
                    None
                }
            };
            let next_learning = next_learning
                .into_iter()
                .chain(next_probe)
                .chain(next_power)
                .chain(next_midnight)
                .chain(next_ending)
                .min();
            let deadline = async {
                if let Some(at) = next_learning {
                    tokio::time::sleep(std::time::Duration::from_millis(
                        at.saturating_sub(now_ms()).max(0) as u64,
                    ))
                    .await;
                } else {
                    std::future::pending::<()>().await;
                }
            };
            tokio::select! {
                biased;
                _ = &mut notified => review_requested = true,
                _ = deadline => {
                    review_requested = true;
                    if next_midnight.is_some_and(|at| now_ms() >= at) {
                        self.stalled
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .clear();
                    }
                }
                Some(result) = reviews.join_next(), if !reviews.is_empty() => {
                    review_requested |= matches!(result, Ok(Ok(Some(_))));
                    if !review_requested {
                        continue;
                    }
                }
            }
            if self.stopped.load(Ordering::SeqCst) {
                break;
            }
            if self.external.swap(false, Ordering::SeqCst) {
                self.stalled
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clear();
            }
            // Keep deadlines that expire between the last admission check and
            // the next wait; comparing with a later wall time could miss them.
            checked_at = now_ms();
            if review_requested && reviews.is_empty() {
                review_requested = false;
                let inner = self.clone();
                reviews.spawn(async move { inner.pump_reviews().await });
            }
            if let Err(error) = self.step() {
                eprintln!("Could not select World work: {error}");
            }
            if !self.stopped.load(Ordering::SeqCst) {
                self.update_keep_awake();
            }
        }
    }

    /// Keep the Mac awake while World jobs run; a queue that cannot move on its
    /// own (blocked, stalled, waiting for a provider) lets it sleep.
    fn update_keep_awake(&self) {
        let pending = self.running_count() > 0;
        let remaining = if pending && self.keep_awake.enabled() {
            self.store
                .for_each_database(|_, connection| {
                    let tx = connection.transaction()?;
                    super::graph_jobs::backlog(&tx)
                        .map_err(|e| crate::store::StoreError::Invalid(e.to_string()))
                })
                .map(|counts| counts.into_iter().sum())
                .unwrap_or(0)
        } else {
            0
        };
        self.keep_awake
            .update(crate::power::Demand { pending, remaining });
    }

    /// Start World work for every free slot. Reviews drive their own I/O.
    fn step(self: &Arc<Self>) -> KnowledgeResult<bool> {
        let _selection = self.selection.lock().expect("World selection");
        if self.stopped.load(Ordering::SeqCst) {
            return Ok(false);
        }
        self.recover_quarantined()?;
        let mut started = false;
        while self.running_count() < MAX_WORLD_JOBS {
            let Some(selected) = self.select_world_job()? else {
                break;
            };
            self.spawn_world_job(selected);
            started = true;
        }
        Ok(started)
    }

    /// Public drains run one unit to completion. World jobs the background
    /// loop runs meanwhile are for other Loupists.
    #[cfg(any(test, feature = "eval-harness"))]
    async fn pump_once(self: &Arc<Self>) -> KnowledgeResult<Option<String>> {
        if let Some(id) = self.pump_reviews().await? {
            return Ok(Some(id));
        }
        let selected = {
            let _selection = self.selection.lock().expect("World selection");
            if self.stopped.load(Ordering::SeqCst) || self.running_count() >= MAX_WORLD_JOBS {
                return Ok(None);
            }
            self.recover_quarantined()?;
            self.select_world_job()?
        };
        let Some(selected) = selected else {
            return Ok(None);
        };
        let _claim = WorldClaim {
            inner: self.clone(),
            loupist: selected.job.loupist_id.clone(),
        };
        self.run_world_job(selected).await
    }

    async fn pump_reviews(&self) -> KnowledgeResult<Option<String>> {
        if self.stopped.load(Ordering::SeqCst)
            || self.reviews_in_flight.swap(true, Ordering::AcqRel)
        {
            return Ok(None);
        }
        struct ReviewClaim<'a>(&'a AtomicBool);
        impl Drop for ReviewClaim<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _claim = ReviewClaim(&self.reviews_in_flight);
        if let Some(id) = self.pump_schema().await? {
            return Ok(Some(id));
        }
        self.pump_learning().await
    }

    fn running_count(&self) -> usize {
        self.running
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len()
    }

    /// Recent Chat turns: when each Loupist last had one, and the Chats.
    /// Claim order is the allowance classes; this remains for [`schedule_rank`].
    #[allow(dead_code)]
    fn attention_now(&self, at: i64) -> (HashMap<String, i64>, Vec<String>) {
        let attention = self
            .attention
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut loupists = HashMap::new();
        let mut chats = Vec::new();
        for (loupist, entry) in attention.iter() {
            if at.saturating_sub(entry.at_ms) > ATTENTION_WINDOW_MS {
                continue;
            }
            loupists.insert(loupist.clone(), entry.at_ms);
            chats.extend(
                entry
                    .chats
                    .iter()
                    .filter(|(_, seen)| at.saturating_sub(**seen) <= ATTENTION_WINDOW_MS)
                    .map(|(chat, _)| chat.clone()),
            );
        }
        chats.sort();
        (loupists, chats)
    }

    /// The next World job to run, among Loupists with no job in flight.
    /// The Loupist is marked running before this returns.
    /// Callers hold `selection`.
    /// Waiting work of price-data Sources leaves the queue before any of it can
    /// be selected, without a model call ([`super::graph_jobs::retire_price_jobs`]).
    /// A folder that cannot be read keeps its work for the next pass.
    fn retire_price_jobs(&self) {
        let known = self
            .price_read
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let Ok(waiting) = self.store.for_each_readable_database(|_, connection| {
            let mut query = connection.prepare(
                "SELECT DISTINCT loupist_id,
                        json_extract(payload_json, '$.sourceBinding.sourceRevisionId') || ':' ||
                        json_extract(payload_json, '$.sourceBinding.spanId')
                 FROM knowledge_graph_outbox
                 WHERE status IN ('queued', 'paused_budget', 'failed_before_mutation', 'cancelled_before_start')
                   AND json_valid(payload_json)
                   AND json_extract(payload_json, '$.episodeKind') = 'source_span'
                   AND json_extract(payload_json, '$.sourceBinding.spanId') IS NOT NULL",
            )?;
            let rows = query
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        }) else {
            return;
        };
        let loupists: std::collections::BTreeSet<String> = waiting
            .into_iter()
            .flatten()
            .filter(|(_, span)| !known.contains(span))
            .map(|(loupist, _)| loupist)
            .collect();
        for loupist in loupists {
            let Ok(knowledge) = loupist_knowledge(self.store.clone(), &loupist, &self.app_data)
            else {
                continue;
            };
            let Ok(found) = knowledge.transact(|write| {
                super::graph_jobs::retire_price_jobs(
                    write.tx,
                    write.blobs,
                    &loupist,
                    &known,
                    now_ms(),
                )
            }) else {
                continue;
            };
            if found.retired > 0 {
                eprintln!(
                    "World queue: {} jobs for price-data Sources left the queue",
                    found.retired
                );
            }
            self.price_read
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .extend(found.classified);
        }
    }

    fn select_world_job(&self) -> KnowledgeResult<Option<Selected>> {
        self.retire_price_jobs();
        let busy: HashSet<String> = self
            .running
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .keys()
            .cloned()
            .collect();
        let mut stalled = self
            .stalled
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let candidates = self
            .store
            .for_each_readable_database(|_, connection| {
                let tx = connection.transaction()?;
                let (candidates, reclaimed) = (|| {
                    let reclaimed = super::graph_jobs::recover_expired(&tx, now_ms(), &busy)?;
                    let mut rows = Vec::new();
                    for job in super::graph_jobs::queued_runnable(&tx)? {
                        let calls = super::graph_jobs::dispatch_calls(&tx, &job)?;
                        rows.push((job, calls));
                    }
                    Ok::<_, KnowledgeError>((rows, reclaimed))
                })()
                .map_err(|e| crate::store::StoreError::Invalid(e.to_string()))?;
                tx.commit()?;
                for id in reclaimed {
                    stalled.remove(&id);
                    self.stalled
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .remove(&id);
                    // Recovery controls ran before selection. A newly quarantined
                    // attempt needs another pass even when no job can be selected.
                    self.notify.notify_one();
                }
                Ok(candidates)
            })
            .map_err(|_| KnowledgeError::Invalid("could not read Loupist jobs"))?
            .into_iter()
            .flatten()
            .filter(|(candidate, _)| {
                !busy.contains(&candidate.loupist_id) && !stalled.contains(&candidate.job_id)
            })
            .collect::<Vec<_>>();
        let mut ready = Vec::new();
        for (candidate, calls) in candidates {
            let settings = self.settings_for_work(&candidate.loupist_id)?;
            if !super::graph_jobs::ready_to_run(&settings, &candidate) {
                continue;
            }
            let auth_mode = inference_auth_mode(self.inference.as_ref(), &settings);
            if candidate.document.episode_kind == "source_span"
                && super::graph_inference::admit_helper(
                    settings.helper_provider.as_deref().unwrap_or(""),
                    &auth_mode,
                )
                .is_err()
            {
                continue;
            }
            ready.push((
                Selected {
                    job: candidate,
                    settings,
                    auth_mode,
                },
                calls,
            ));
        }
        let limits = super::graph_jobs::allowance_limits(&self.store);
        let admission = crate::scheduled_tasks::allowance::Admission::prepare(&self.store, None)
            .map_err(|_| KnowledgeError::Invalid("could not read the daily allowance"))?;
        let state = admission
            .state(None, limits, now_ms())
            .map_err(|_| KnowledgeError::Invalid("could not read the daily allowance"))?;
        ready.retain(|(selected, calls)| {
            let provider = selected
                .settings
                .helper_provider
                .as_deref()
                .filter(|provider| !provider.is_empty());
            !super::graph_jobs::allowance_refuses(&state, &selected.job, *calls, provider)
        });
        let ranked: Vec<_> = ready
            .iter()
            .map(|(selected, _)| selected.job.clone())
            .collect();
        let streak = *self
            .chat_claims
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let picked = super::graph_jobs::pick_claim(&ranked, streak).map(|job| job.job_id.clone());
        let selected = picked.and_then(|id| {
            ready
                .into_iter()
                .find(|(selected, _)| selected.job.job_id == id)
                .map(|(selected, _)| selected)
        });
        if let Some(selected) = &selected {
            let loupist = selected.job.loupist_id.clone();
            let sequence = self.serve_sequence.fetch_add(1, Ordering::SeqCst) + 1;
            self.served
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(loupist.clone(), sequence);
            self.running
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(
                    loupist,
                    Worker {
                        job_id: selected.job.job_id.clone(),
                        abort: None,
                    },
                );
        }
        Ok(selected)
    }

    /// A Loupist's settings as its next model call should use them. A
    /// Recommended World model is resolved again first, so the job uses what
    /// the bundled list recommends for the accounts connected now; that change
    /// is saved like any other and asks nothing of the user.
    fn settings_for_work(&self, loupist_id: &str) -> KnowledgeResult<GraphProcessingSettings> {
        let saved = || {
            load_settings(
                &*self
                    .store
                    .lock_loupist(loupist_id)
                    .map_err(|_| KnowledgeError::LockPoisoned)?,
                loupist_id,
            )
        };
        let settings = saved()?;
        if settings.world_model_choice != super::graph_jobs::WorldModelChoice::Recommended {
            return Ok(settings);
        }
        // Without the host's catalog (tests, a missing runtime) the saved model stands.
        let Some(recommended) = self
            .inference
            .as_ref()
            .and_then(|service| service.world_models().ok())
            .map(|models| models.recommend(Some(loupist_id)))
        else {
            return Ok(settings);
        };
        if settings.world_model() == recommended {
            return Ok(settings);
        }
        let followed = loupist_knowledge(self.store.clone(), loupist_id, &self.app_data).and_then(
            |knowledge| {
                knowledge.transact(|write| {
                    write.follow_recommended_world_model(loupist_id, recommended.as_ref(), now_ms())
                })
            },
        );
        if followed.is_err() {
            eprintln!("Could not update the recommended World model for {loupist_id}");
            return Ok(settings);
        }
        saved()
    }

    fn spawn_world_job(self: &Arc<Self>, selected: Selected) {
        let loupist = selected.job.loupist_id.clone();
        let job_id = selected.job.job_id.clone();
        let inner = self.clone();
        let worker_job = job_id.clone();
        let claim = WorldClaim {
            inner: inner.clone(),
            loupist: loupist.clone(),
        };
        let task = tokio::spawn(async move {
            let _claim = claim;
            let result = inner.until_shutdown(inner.run_world_job(selected)).await;
            if !matches!(result, Ok(Some(_))) {
                inner
                    .stalled
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .insert(worker_job);
            }
        });
        if let Some(worker) = self
            .running
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get_mut(&loupist)
            .filter(|worker| worker.job_id == job_id)
        {
            worker.abort = Some(task.abort_handle());
        }
    }

    /// Run one selected World job to its next status. Some(job id) when it
    /// made progress; None when it stayed queued and needs a change first.
    async fn run_world_job(&self, selected: Selected) -> KnowledgeResult<Option<String>> {
        let Selected {
            job,
            mut settings,
            auth_mode,
        } = selected;
        let knowledge = loupist_knowledge(self.store.clone(), &job.loupist_id, &self.app_data)?;
        let _activity = self
            .maintenance
            .admit_activity_when_available(MaintenanceBlocker {
                id: format!("graph-{}", job.job_id),
                kind: "graph".into(),
                label: "World building".into(),
            })
            .await
            .map_err(|_| KnowledgeError::Invalid("graph activity gate is unavailable"))?;
        let started_at = now_ms();
        let mut run_log =
            crate::runs::log::RunLog::new("world", &job.loupist_id, &job.job_id, started_at);
        run_log.queue_wait_ms = Some(started_at.saturating_sub(job.created_at_ms).max(0) as u64);
        run_log.model = settings.helper_model.as_ref().map(|model| {
            format!(
                "{}/{}",
                settings.helper_provider.as_deref().unwrap_or("unknown"),
                model
            )
        });
        run_log.effort = Some(settings.thinking_level.clone());
        let usage = Arc::new(std::sync::Mutex::new(
            super::graph_inference::CallLedger::default(),
        ));
        let calls = Arc::new(WorldCalls::default());
        let stage_ms = Arc::new(std::sync::Mutex::new(BTreeMap::<String, u64>::new()));
        let callback_stages = stage_ms.clone();
        let call_slots = self.call_slots.clone();
        let recovery = job.document.episode_kind == "world_recovery";
        let inference = self.inference.clone();
        let shared_store = self.store.clone();
        let inference_loupist = job.loupist_id.clone();
        let frozen = settings.clone();
        let started_with = settings.clone();
        let assets = embed_assets(self.resource_dir.as_deref());
        let callback_usage = usage.clone();
        let callback_calls = calls.clone();
        let account_hold: Arc<std::sync::Mutex<Option<String>>> =
            Arc::new(std::sync::Mutex::new(None));
        let callback_hold = account_hold.clone();
        let account = Arc::new(WorldAccount::new(
            self.store.clone(),
            format!("world:{}:{started_at}", job.job_id),
            job.loupist_id.clone(),
            started_at,
            super::graph_jobs::allowance_origin(&job.document),
            settings
                .helper_provider
                .clone()
                .filter(|provider| !provider.is_empty())
                .unwrap_or_else(|| "unknown".into()),
            settings
                .helper_model
                .clone()
                .filter(|model| !model.is_empty())
                .unwrap_or_else(|| "unknown".into()),
            settings.thinking_level.clone(),
        ));
        let callback_account = account.clone();
        let final_account = account.clone();
        let on_account = move |delta: i32| {
            let _ = account.account(delta, now_ms());
        };
        // The extraction hints name what this job's Loupist follows.
        let followed = self
            .store
            .map_context(&job.loupist_id)
            .map(|context| {
                context
                    .watchlist
                    .into_iter()
                    .map(|item| {
                        let symbol = item.symbol;
                        let name = item
                            .display_name
                            .filter(|name| !name.trim().is_empty())
                            .unwrap_or_else(|| symbol.clone());
                        FollowedInstrument { name, symbol }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let on_inference = move |request: serde_json::Value| {
            let inference = inference.clone();
            let shared_store = shared_store.clone();
            let loupist = inference_loupist.clone();
            let frozen = frozen.clone();
            let assets = assets.clone();
            let usage = callback_usage.clone();
            let calls = callback_calls.clone();
            let stages = callback_stages.clone();
            let account_hold = callback_hold.clone();
            // A model call whose count the ledger refused is not sent: it
            // goes back as not sent, and the job refunds its reservation.
            let refused = request["kind"] == "llm" && !callback_account.admit_call();
            async move {
                if refused {
                    return super::graph_inference::InferenceReply {
                        disposition: super::graph_inference::InferenceDisposition::NotSent,
                        body: serde_json::json!({
                            "status": "not_sent",
                            "message": "the call could not be counted",
                        }),
                    };
                }
                let field = |name: &str| {
                    request
                        .get(name)
                        .or_else(|| request.pointer(&format!("/params/{name}")))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                };
                let kind = field("kind");
                let paid = kind == "llm";
                if paid {
                    usage.lock().unwrap_or_else(|e| e.into_inner()).begin();
                }
                let sent = Instant::now();
                let reply = current_inference(
                    &shared_store,
                    &loupist,
                    &frozen,
                    inference.as_ref(),
                    &assets,
                    &request,
                )
                .await;
                if paid {
                    usage
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .observe(&reply);
                    if reply.disposition != super::graph_inference::InferenceDisposition::NotSent {
                        let stage = field("stage");
                        let elapsed = sent.elapsed().as_millis() as u64;
                        *stages
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .entry(if stage.is_empty() {
                                "llm".into()
                            } else {
                                stage
                            })
                            .or_default() += elapsed;
                    }
                }
                calls.observe(&kind, &reply);
                if let Some(kind) = reply
                    .body
                    .get("account_hold")
                    .and_then(serde_json::Value::as_str)
                    .filter(|kind| matches!(*kind, "plan_limit" | "out_of_credit"))
                {
                    *account_hold
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = Some(kind.to_string());
                }
                reply
            }
        };
        let resolution = super::world_extract::ResolutionSlot::default();
        let driven = super::world_extract::drive_job_with_account(
            &knowledge,
            &mut settings,
            &job.job_id,
            &auth_mode,
            now_ms(),
            &followed,
            &embed_assets(self.resource_dir.as_deref()),
            Some(call_slots),
            Some(resolution.clone()),
            on_account,
            on_inference,
        )
        .await;
        if let Some(tally) = *resolution.lock().unwrap_or_else(|e| e.into_inner()) {
            run_log.entity_resolution = BTreeMap::from([
                ("proposed".to_string(), u64::from(tally.proposed)),
                ("refused".to_string(), u64::from(tally.refused)),
                ("byCode".to_string(), u64::from(tally.by_code)),
            ]);
        }
        usage
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .apply(&mut run_log);
        run_log.stage_ms = std::mem::take(&mut *stage_ms.lock().unwrap_or_else(|e| e.into_inner()));
        calls.record(&mut run_log);
        run_log.world_origin = Some(super::graph_jobs::allowance_origin(&job.document));
        let account_failure = account_hold
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let noted = account_failure.as_deref();
        match &driven {
            Ok(view) => match view.status.as_str() {
                "completed" | "reconciled_without_retry" => run_log.finish("ok", None),
                "queued" | "paused_budget" => run_log.finish("deferred", noted),
                "cancelled_before_start" => run_log.finish("cancelled", noted),
                _ => run_log.finish("failed", noted.or(Some("world_job_failed"))),
            },
            Err(_) => run_log.finish("failed", noted.or(Some("world_job_failed"))),
        }
        if let Ok(view) = &driven {
            if view.document.sealed_revision_id != job.document.sealed_revision_id
                && view.document.sealed_revision_id.is_some()
            {
                run_log.memory_changes.insert("World".into(), 1);
            }
        }
        // The final row replaces the attempt's own ledger row: it carries the
        // calls of the day it ends on; earlier days keep their own rows.
        let ended_at = run_log
            .started_at_ms
            .saturating_add(i64::try_from(run_log.duration_ms).unwrap_or(i64::MAX));
        run_log.requests = run_log
            .requests
            .map(|sent| final_account.final_requests(sent, ended_at));
        run_log.record(self.store.as_ref());
        if driven.as_ref().ok().is_some_and(|view| {
            view.status != super::graph_jobs::JOB_QUEUED || view.document.attempt_id.is_some()
        }) {
            let mut streak = self
                .chat_claims
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            *streak = super::graph_jobs::note_claim_streak(*streak, &job.document);
        }
        if recovery && driven.is_err() {
            knowledge.transact(|write| super::graph_jobs::fail_recovery(write.tx, &job.job_id))?;
        }
        if !self.stopped.load(Ordering::SeqCst) {
            merge_quarantine(&self.store, &job.loupist_id, &settings)?;
        }
        // Settings saved since this attempt started are the change a requeued
        // job waits for, so its next claim runs on them.
        let moved_on = self
            .store
            .lock_loupist(&job.loupist_id)
            .ok()
            .and_then(|connection| load_settings(&connection, &job.loupist_id).ok())
            .is_some_and(|saved| super::graph_inference::controls_changed(&saved, &started_with));
        if let Ok(view) = &driven {
            if view.status == super::graph_jobs::JOB_QUEUED
                && view.document.attempt_id.is_none()
                && super::graph_jobs::queued_holds(&self.store, &job.loupist_id, now_ms())
                    .ok()
                    .is_some_and(|holds| holds.contains_key(&view.job_id))
            {
                // A held job stays queued and is skipped on the next claim. Stalling
                // it would keep it out after the allowance starts again.
                return Ok(Some(view.job_id.clone()));
            }
        }
        driven.map(|view| made_progress(&job, &view, moved_on).then_some(view.job_id))
    }

    async fn pump_schema(&self) -> KnowledgeResult<Option<String>> {
        let at = now_ms();
        let candidates = self
            .store
            .for_each_readable_database(|_, connection| {
                let mut query = connection.prepare(
                    "SELECT DISTINCT owner_loupist_id FROM knowledge_records WHERE kind = 'world' ORDER BY owner_loupist_id",
                )?;
                let loupists = query
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(loupists
                    .into_iter()
                    .filter(|loupist| super::schema_writer::due(connection, loupist, at).unwrap_or(false))
                    .collect::<Vec<_>>())
            })
            .map_err(|_| KnowledgeError::Invalid("could not read schema reviews"))?;
        for loupist in candidates.into_iter().flatten() {
            let knowledge = loupist_knowledge(self.store.clone(), &loupist, &self.app_data)?;
            let settings = self.settings_for_work(&loupist)?;
            if super::graph_inference::admit_helper(
                settings.helper_provider.as_deref().unwrap_or(""),
                &inference_auth_mode(self.inference.as_ref(), &settings),
            )
            .is_err()
            {
                continue;
            }
            let Some(profile) = self
                .store
                .get_loupist(&loupist)
                .map_err(|_| KnowledgeError::NotFound)?
            else {
                continue;
            };
            let _activity = self
                .maintenance
                .admit_activity_when_available(MaintenanceBlocker {
                    id: format!("schema-{loupist}"),
                    kind: "graph".into(),
                    label: "Reviewing what the World tracks".into(),
                })
                .await
                .map_err(|_| KnowledgeError::Invalid("schema activity gate is unavailable"))?;
            let started = now_ms();
            let claimed = {
                let admission = crate::scheduled_tasks::allowance::Admission::prepare(
                    self.store.as_ref(),
                    Some(&loupist),
                )
                .map_err(|_| KnowledgeError::Invalid("could not read the daily allowance"))?;
                let limits = super::graph_jobs::allowance_limits(self.store.as_ref());
                knowledge.transact(|write| {
                    let connection: &rusqlite::Connection = write.tx;
                    let state = admission
                        .state(Some(connection), limits, now_ms())
                        .map_err(|_| {
                            KnowledgeError::Invalid("could not read the daily allowance")
                        })?;
                    super::schema_writer::claim(
                        write.tx,
                        &loupist,
                        &profile.focus,
                        now_ms(),
                        Some(&state),
                    )
                })?
            };
            let super::schema_writer::Claimed::Ready(attempt) = claimed else {
                continue;
            };
            let attempt = *attempt;
            let reply = current_inference(
                &self.store,
                &loupist,
                &attempt.settings,
                self.inference.as_ref(),
                &embed_assets(self.resource_dir.as_deref()),
                &attempt.request,
            )
            .await;
            let mut accounted =
                crate::runs::log::RunLog::new("schema", &loupist, &attempt.review_id, started);
            accounted.model = attempt.settings.helper_model.as_ref().map(|model| {
                format!(
                    "{}/{}",
                    attempt
                        .settings
                        .helper_provider
                        .as_deref()
                        .unwrap_or("unknown"),
                    model
                )
            });
            record_sent_review(self.store.as_ref(), &mut accounted, &reply)
                .map_err(|_| KnowledgeError::Invalid("could not record the review"))?;
            let finished = knowledge
                .transact(|write| super::schema_writer::finish(write, &attempt, &reply, now_ms()));
            if let Ok(finish) = &finished {
                self.note_schema(
                    &loupist,
                    &attempt.review_id,
                    &attempt.settings,
                    started,
                    finish,
                    Some(&reply),
                );
            } else {
                let mut run_log =
                    crate::runs::log::RunLog::new("schema", &loupist, &attempt.review_id, started);
                run_log.finish("failed", Some("schema_review_failed"));
                run_log.record(self.store.as_ref());
            }
            finished?;
            return Ok(Some(attempt.review_id));
        }
        Ok(None)
    }

    fn note_schema(
        &self,
        loupist: &str,
        review_id: &str,
        settings: &GraphProcessingSettings,
        started: i64,
        finish: &super::schema_writer::Finish,
        reply: Option<&super::graph_inference::InferenceReply>,
    ) {
        let mut run_log = crate::runs::log::RunLog::new("schema", loupist, review_id, started);
        run_log.model = settings.helper_model.as_ref().map(|model| {
            format!(
                "{}/{}",
                settings.helper_provider.as_deref().unwrap_or("unknown"),
                model
            )
        });
        run_log.effort = Some(settings.thinking_level.clone());
        if let Some(reply) = reply {
            let mut usage = super::graph_inference::CallLedger::default();
            usage.begin();
            usage.observe(reply);
            usage.apply(&mut run_log);
        }
        super::schema_writer::note_finish(&mut run_log, finish);
        run_log.record(self.store.as_ref());
        if finish.notifies {
            let _ = self
                .schema_events
                .send(super::schema_writer::schema_change_event(loupist));
        }
    }

    /// An interrupted provider call quarantines its World until a rebuild.
    /// Queue that rebuild here so the map keeps growing without a settings visit.
    fn recover_quarantined(&self) -> KnowledgeResult<()> {
        let quarantined = self
            .store
            .for_each_readable_database(|_, connection| {
                let mut query = connection.prepare(
                    "SELECT DISTINCT loupist_id FROM knowledge_graph_outbox WHERE status = 'reconcile_required'",
                )?;
                let rows = query
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .map_err(|_| KnowledgeError::Invalid("could not read Loupist jobs"))?;
        for loupist in quarantined.into_iter().flatten() {
            // One unreadable folder must not stop the other Loupists' processing;
            // its settings still offer the explicit recovery.
            let Ok(knowledge) = loupist_knowledge(self.store.clone(), &loupist, &self.app_data)
            else {
                continue;
            };
            let _ = knowledge.transact(|write| {
                super::graph_jobs::recover_world_unattended(
                    write.tx,
                    write.blobs,
                    &loupist,
                    now_ms(),
                )
            });
        }
        Ok(())
    }

    async fn pump_learning(&self) -> KnowledgeResult<Option<String>> {
        let candidates = self.store.for_each_readable_database(|_, connection| {
            let mut q=connection.prepare("SELECT loupist_id,review_id FROM knowledge_learning_reviews WHERE status IN ('pending','queued') ORDER BY created_at_ms")?;
            let rows=q.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<Result<Vec<_>,_>>()?;
            Ok(rows)
        }).map_err(|_|KnowledgeError::Invalid("could not read learning jobs"))?;
        for (loupist, review_id) in candidates.into_iter().flatten() {
            let knowledge = loupist_knowledge(self.store.clone(), &loupist, &self.app_data)?;
            let settings = self.settings_for_work(&loupist)?;
            if super::graph_inference::admit_helper(
                settings.helper_provider.as_deref().unwrap_or(""),
                &inference_auth_mode(self.inference.as_ref(), &settings),
            )
            .is_err()
            {
                continue;
            }
            let profile = self
                .store
                .get_loupist(&loupist)
                .map_err(|_| KnowledgeError::NotFound)?;
            let Some(profile) = profile else { continue };
            let _activity = self
                .maintenance
                .admit_activity_when_available(MaintenanceBlocker {
                    id: format!("learning-{loupist}"),
                    kind: "graph".into(),
                    label: "Learning research methods".into(),
                })
                .await
                .map_err(|_| KnowledgeError::Invalid("learning activity gate is unavailable"))?;
            let review_started_at = now_ms();
            enum LearningStart {
                Ready(Box<super::learning::Attempt>),
                Held,
                Idle,
            }
            let started = {
                let admission = crate::scheduled_tasks::allowance::Admission::prepare(
                    self.store.as_ref(),
                    Some(&loupist),
                )
                .map_err(|_| KnowledgeError::Invalid("could not read the daily allowance"))?;
                let limits = super::graph_jobs::allowance_limits(self.store.as_ref());
                knowledge.transact(|w| {
                    let connection: &rusqlite::Connection = w.tx;
                    let state = admission
                        .state(Some(connection), limits, now_ms())
                        .map_err(|_| {
                            KnowledgeError::Invalid("could not read the daily allowance")
                        })?;
                    let provider = settings
                        .helper_provider
                        .as_deref()
                        .filter(|provider| !provider.is_empty());
                    if crate::scheduled_tasks::allowance::admit_world_attempt(
                        &state,
                        crate::scheduled_tasks::allowance::WorldOrigin::Background,
                        1,
                        provider,
                    )
                    .is_err()
                    {
                        return Ok(LearningStart::Held);
                    }
                    Ok(
                        match super::learning::claim(
                            w.tx,
                            &loupist,
                            &profile.name,
                            &profile.focus,
                            now_ms(),
                            Some(&state),
                        )? {
                            Some(attempt) => LearningStart::Ready(Box::new(attempt)),
                            None => LearningStart::Idle,
                        },
                    )
                })?
            };
            let attempt = match started {
                LearningStart::Held => continue,
                LearningStart::Ready(attempt) => *attempt,
                LearningStart::Idle => {
                    let reviews = knowledge.read(|w| {
                        super::learning::list(
                            w.tx,
                            &super::learning::ReviewQuery {
                                loupist_id: loupist.clone(),
                                limit: Some(1),
                            },
                        )
                    })?;
                    if let Some(review) = reviews.first().filter(|r| r.review_id == review_id) {
                        let mut run_log = crate::runs::log::RunLog::new(
                            "learning",
                            &loupist,
                            &review.review_id,
                            review_started_at,
                        );
                        run_log.finish("deferred", None);
                        run_log
                            .notes
                            .push("Review deferred before model execution.".into());
                        run_log.record(self.store.as_ref());
                        let _ = self
                            .learning_events
                            .send(crate::events::LearningReviewFinished {
                                loupist_id: loupist.clone(),
                                review: review.clone(),
                            });
                        return Ok(Some(review_id));
                    }
                    continue;
                }
            };
            let mut run_log = crate::runs::log::RunLog::new(
                "learning",
                &loupist,
                &attempt.review_id,
                review_started_at,
            );
            run_log.model = attempt.settings.helper_model.as_ref().map(|model| {
                format!(
                    "{}/{}",
                    attempt
                        .settings
                        .helper_provider
                        .as_deref()
                        .unwrap_or("unknown"),
                    model
                )
            });
            run_log.effort = Some(attempt.settings.thinking_level.clone());
            let request = super::learning::request(&attempt)?;
            let reply = current_inference(
                &self.store,
                &loupist,
                &attempt.settings,
                self.inference.as_ref(),
                &embed_assets(self.resource_dir.as_deref()),
                &request,
            )
            .await;
            let mut usage = super::graph_inference::CallLedger::default();
            usage.begin();
            usage.observe(&reply);
            usage.apply(&mut run_log);
            record_sent_review(self.store.as_ref(), &mut run_log, &reply)
                .map_err(|_| KnowledgeError::Invalid("could not record the review"))?;
            let finished =
                knowledge.transact(|w| super::learning::finish(w, &attempt, &reply, now_ms()));
            if finished.is_err() {
                run_log.finish("failed", Some("learning_store_failed"));
                run_log.record(self.store.as_ref());
            }
            finished?;
            let reviews = knowledge.read(|w| {
                super::learning::list(
                    w.tx,
                    &super::learning::ReviewQuery {
                        loupist_id: loupist.clone(),
                        limit: Some(1),
                    },
                )
            })?;
            if let Some(review) = reviews.first() {
                run_log.finish(
                    if review.status == "failed" {
                        "failed"
                    } else if review.status == "deferred" {
                        "deferred"
                    } else {
                        "ok"
                    },
                    (review.status == "failed").then_some("learning_review_failed"),
                );
                if review.lens_record_id.is_some() {
                    run_log.memory_changes.insert("Lens".into(), 1);
                }
                let _ = self
                    .learning_events
                    .send(crate::events::LearningReviewFinished {
                        loupist_id: loupist.clone(),
                        review: review.clone(),
                    });
            }
            run_log.record(self.store.as_ref());
            return Ok(Some(attempt.review_id));
        }
        Ok(None)
    }
}

/// Whether a World attempt moved its job on. A queued job that could not be
/// claimed, or that sent nothing, needs a settings change or another attempt
/// before the loop can make progress. A job queued again for its one retry,
/// after the Mac slept through it, or on settings that changed under it
/// (`moved_on`, or calls spent on the way) did make progress.
fn made_progress(
    before: &super::graph_jobs::GraphJobView,
    after: &super::graph_jobs::GraphJobView,
    moved_on: bool,
) -> bool {
    after.status != super::graph_jobs::JOB_QUEUED
        || after.document.interrupted_attempts > before.document.interrupted_attempts
        || after.document.host_interruptions > before.document.host_interruptions
        || after.document.extraction.max_llm_calls != before.document.extraction.max_llm_calls
        || moved_on
}

pub(super) async fn current_inference(
    store: &SqliteStore,
    loupist_id: &str,
    frozen: &GraphProcessingSettings,
    service: Option<&crate::providers::inference::ProviderInferenceService>,
    assets: &Path,
    request: &serde_json::Value,
) -> super::graph_inference::InferenceReply {
    let settings = store
        .lock_loupist(loupist_id)
        .map_err(|_| KnowledgeError::LockPoisoned)
        .and_then(|connection| load_settings(&connection, loupist_id));
    let Ok(settings) = settings else {
        return not_sent_reply(serde_json::json!({})).await;
    };
    if super::graph_inference::controls_changed(&settings, frozen) {
        return super::graph_inference::settings_changed_reply();
    }
    let request = request
        .get("params")
        .cloned()
        .unwrap_or_else(|| request.clone());
    if request
        .pointer("/scope/loupist_id")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|scope| scope != loupist_id)
    {
        return not_sent_reply(serde_json::json!({})).await;
    }
    dispatch_inference(&settings, service, assets, &request).await
}

fn merge_quarantine(
    store: &SqliteStore,
    loupist_id: &str,
    driven: &GraphProcessingSettings,
) -> KnowledgeResult<()> {
    let connection = store
        .lock_loupist(loupist_id)
        .map_err(|_| KnowledgeError::LockPoisoned)?;
    let mut current = load_settings(&connection, loupist_id)?;
    for generation in &driven.quarantined_generations {
        // The attempt can have mutated its in-memory settings before its
        // transaction rolled back. Only a committed reconciliation receipt
        // can quarantine the generation in the app settings database.
        let committed: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM knowledge_graph_outbox
             WHERE loupist_id = ?1 AND status = 'reconcile_required' AND json_valid(payload_json)
               AND json_extract(payload_json, '$.generationId') = ?2)",
            rusqlite::params![loupist_id, generation],
            |row| row.get(0),
        )?;
        if !committed {
            continue;
        }
        if !current.quarantined_generations.contains(generation) {
            current.quarantined_generations.push(generation.clone());
        }
    }
    store_settings(&connection, loupist_id, &current)
}

fn inference_auth_mode(
    service: Option<&crate::providers::inference::ProviderInferenceService>,
    settings: &GraphProcessingSettings,
) -> String {
    #[cfg(any(test, feature = "e2e"))]
    if matches!(
        settings.helper_provider.as_deref(),
        Some("fixture" | "fixture-local")
    ) {
        return "fixture".into();
    }
    service
        .and_then(|service| {
            service
                .readiness(
                    settings.helper_provider.as_deref().unwrap_or(""),
                    settings.helper_model.as_deref().unwrap_or(""),
                )
                .ok()
        })
        .filter(|ready| ready.available)
        .and_then(|ready| ready.auth_kind)
        .unwrap_or_else(|| "missing".into())
}

fn loupist_knowledge(
    store: Arc<SqliteStore>,
    loupist_id: &str,
    app_data: &Path,
) -> KnowledgeResult<KnowledgeStore> {
    if store.is_folder_catalog() {
        return KnowledgeStore::for_loupist(store, loupist_id);
    }
    let root = store
        .database_path()
        .and_then(Path::parent)
        .unwrap_or(app_data)
        .join("knowledge-blobs");
    Ok(KnowledgeStore::attach(store, BlobStore::open(root)?))
}

fn embed_assets(resource_dir: Option<&Path>) -> PathBuf {
    if let Some(root) = resource_dir {
        let bundled = root.join("embed_assets");
        if bundled.join("config.json").is_file() {
            return bundled;
        }
    }
    #[cfg(debug_assertions)]
    {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/embed_assets")
    }
    #[cfg(not(debug_assertions))]
    {
        resource_dir
            .unwrap_or_else(|| Path::new(""))
            .join("embed_assets")
    }
}

/// The next local midnight after `now_ms`, when a held World job can be claimed again.
pub(super) fn next_local_midnight_ms(now_ms: i64) -> Option<i64> {
    let local = crate::local_time::at_ms(now_ms)?;
    let tomorrow = local.date_naive().succ_opt()?;
    let midnight = tomorrow
        .and_hms_opt(0, 0, 0)?
        .and_local_timezone(*local.offset())
        .single()?;
    let at = midnight.timestamp_millis();
    Some(if at > now_ms {
        at
    } else {
        at.saturating_add(86_400_000)
    })
}

/// One World attempt's running ledger rows: every model call is counted
/// before the outbox budget drops, and a call the ledger refused is never sent.
///
/// Each call counts on the local day it was made, in whatever time zone the
/// day is read. The attempt's own row (the id its final record replaces)
/// holds the calls of one quarter hour of UTC: every zone's midnight falls on
/// a quarter hour, so those calls share a local day everywhere. When a call
/// falls in a later quarter hour, the earlier calls first move to a row of
/// their own, `<id>:<quarter start>`, dated by their last call.
pub(super) struct WorldAccount {
    store: Arc<SqliteStore>,
    pub(super) id: String,
    loupist_id: String,
    started_at: i64,
    origin: crate::scheduled_tasks::allowance::WorldOrigin,
    provider: String,
    model: String,
    effort: String,
    requests: AtomicU64,
    /// Calls counted up whose ledger write failed and that were not yet refused.
    unrecorded: AtomicU64,
    day: std::sync::Mutex<AccountDay>,
}

/// Every time zone's offset is a whole number of quarter hours.
const DAY_SLOT_MS: i64 = 15 * 60_000;

/// The quarter hour the attempt's own row is on.
struct AccountDay {
    slot: i64,
    /// Calls counted in earlier quarter hours, each in its own row.
    earlier: u64,
    /// When the last call in `slot` was counted.
    last_at_ms: i64,
}

impl WorldAccount {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        store: Arc<SqliteStore>,
        id: String,
        loupist_id: String,
        started_at: i64,
        origin: crate::scheduled_tasks::allowance::WorldOrigin,
        provider: String,
        model: String,
        effort: String,
    ) -> Self {
        Self {
            store,
            id,
            loupist_id,
            started_at,
            origin,
            provider,
            model,
            effort,
            requests: AtomicU64::new(0),
            unrecorded: AtomicU64::new(0),
            day: std::sync::Mutex::new(AccountDay {
                slot: started_at.div_euclid(DAY_SLOT_MS),
                earlier: 0,
                last_at_ms: started_at,
            }),
        }
    }

    /// Counts `delta` calls (a refund is negative) and writes the ledger row
    /// of the day the call was made on. A counted call whose row was not
    /// stored is refused by the next [`Self::admit_call`]; a refund whose row
    /// was not stored leaves the ledger counting too much, never too little.
    pub(super) fn account(&self, delta: i32, at_ms: i64) -> crate::store::StoreResult<()> {
        let mut day = self.day.lock().unwrap_or_else(|error| error.into_inner());
        let step = u64::from(delta.unsigned_abs());
        let before = self.requests.load(Ordering::SeqCst);
        let next = if delta >= 0 {
            before.saturating_add(step)
        } else {
            before.saturating_sub(step)
        };
        self.requests.store(next, Ordering::SeqCst);
        // A refund rewrites the row where its last counted call left it.
        let written = (|| {
            if delta > 0 {
                self.move_to_day(&mut day, before, at_ms)?;
                day.last_at_ms = at_ms;
            }
            self.write(&self.id, next.saturating_sub(day.earlier), day.last_at_ms)
        })();
        if written.is_err() && delta > 0 {
            self.unrecorded.fetch_add(step, Ordering::SeqCst);
        }
        written
    }

    /// Whether the call just counted may be sent: false once for each call
    /// whose ledger write failed.
    pub(super) fn admit_call(&self) -> bool {
        self.unrecorded
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |pending| {
                pending.checked_sub(1)
            })
            .is_err()
    }

    /// The calls the attempt's final row carries when it ends at `at_ms`:
    /// those of its own row's quarter hour, or none when that has passed and
    /// its calls moved to their own row.
    pub(super) fn final_requests(&self, sent: u64, at_ms: i64) -> u64 {
        let mut day = self.day.lock().unwrap_or_else(|error| error.into_inner());
        let counted = self.requests.load(Ordering::SeqCst);
        // A day that cannot move keeps its calls on the final row: counted
        // late, never lost.
        let _ = self.move_to_day(&mut day, counted, at_ms);
        sent.saturating_sub(day.earlier)
    }

    /// Requests counted so far, every day included.
    #[cfg(test)]
    pub(super) fn requests(&self) -> u64 {
        self.requests.load(Ordering::SeqCst)
    }

    /// Before counting in a later quarter hour than the attempt's own row,
    /// keep that row's calls on their day in a row of their own.
    fn move_to_day(
        &self,
        day: &mut AccountDay,
        counted: u64,
        at_ms: i64,
    ) -> crate::store::StoreResult<()> {
        let slot = at_ms.div_euclid(DAY_SLOT_MS);
        if slot == day.slot {
            return Ok(());
        }
        let calls = counted.saturating_sub(day.earlier);
        if calls > 0 {
            let id = format!("{}:{}", self.id, day.slot * DAY_SLOT_MS);
            self.write(&id, calls, day.last_at_ms)?;
        }
        day.slot = slot;
        day.earlier = counted;
        day.last_at_ms = at_ms;
        Ok(())
    }

    fn write(&self, id: &str, requests: u64, at_ms: i64) -> crate::store::StoreResult<()> {
        write_world_reservation(
            &self.store,
            &WorldReservation {
                id,
                loupist_id: &self.loupist_id,
                started_at: self.started_at,
                requests,
                origin: self.origin,
                provider: &self.provider,
                model: &self.model,
                effort: &self.effort,
            },
            at_ms,
        )
    }
}

struct WorldReservation<'a> {
    id: &'a str,
    loupist_id: &'a str,
    started_at: i64,
    requests: u64,
    origin: crate::scheduled_tasks::allowance::WorldOrigin,
    provider: &'a str,
    model: &'a str,
    effort: &'a str,
}

/// Write a sent review's usage before its reservation is released. A call that
/// was not sent leaves the ledger alone. A sent call whose row cannot be stored
/// is an error, so the caller keeps the reservation.
pub(super) fn record_sent_review(
    store: &SqliteStore,
    log: &mut crate::runs::log::RunLog,
    reply: &super::graph_inference::InferenceReply,
) -> crate::store::StoreResult<()> {
    let mut usage = super::graph_inference::CallLedger::default();
    usage.begin();
    usage.observe(reply);
    usage.apply(log);
    if reply.disposition == super::graph_inference::InferenceDisposition::NotSent {
        return Ok(());
    }
    if !log.record_usage(store)? {
        return Err(crate::store::StoreError::Invalid(
            "review usage was not recorded".into(),
        ));
    }
    Ok(())
}

fn write_world_reservation(
    store: &SqliteStore,
    reservation: &WorldReservation<'_>,
    now: i64,
) -> crate::store::StoreResult<()> {
    use crate::runs::log::UsageSurface;
    use crate::store::model_usage::{
        ModelUsageData, ModelUsageKind, ModelUsageMeasures, ModelUsageRow, ModelUsageStatus,
    };
    let row = ModelUsageRow {
        finished_at_ms: now,
        loupist_id: Some(reservation.loupist_id.to_owned()),
        task_id: None,
        data: ModelUsageData {
            kind: ModelUsageKind::World,
            surface: UsageSurface::World,
            world_origin: Some(reservation.origin),
            provider: reservation.provider.to_owned(),
            credential_kind: None,
            model: reservation.model.to_owned(),
            effort: Some(reservation.effort.to_owned()),
            chat_id: None,
            run_id: reservation.id.to_owned(),
            started_at_ms: reservation.started_at,
            duration_ms: now.saturating_sub(reservation.started_at).max(0) as u64,
            status: ModelUsageStatus::Deferred,
            failure_kind: None,
            measures: ModelUsageMeasures {
                requests: Some(reservation.requests),
                ..Default::default()
            },
            accounts: Vec::new(),
        },
    };
    store.put_model_usage(reservation.id, &row)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
