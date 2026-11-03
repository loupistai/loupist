//! Wakeable in-process World, learning and schema job loop.
//!
//! Jobs live in each Loupist folder database. This loop aggregates candidates
//! from registered folders. Pi owns the shared provider credentials; extraction receives only
//! host-mediated inference results, never keys or OAuth tokens.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::{Mutex, Notify};

use super::blob::BlobStore;
use super::error::{KnowledgeError, KnowledgeResult};
use super::graph_inference::dispatch_inference;
use super::graph_jobs::{load_settings, store_settings, GraphProcessingSettings};
use super::world_extract::hints::FollowedInstrument;
use super::KnowledgeStore;
use crate::runs::maintenance::{MaintenanceBlocker, MaintenanceCoordinator};
use crate::store::{LoupistStore, SqliteStore};

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
    learning_events: tokio::sync::broadcast::Sender<serde_json::Value>,
    schema_events: tokio::sync::broadcast::Sender<serde_json::Value>,
    stopped: AtomicBool,
    task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    pump_lock: Mutex<()>,
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
        let inner = Arc::new(Inner {
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
            pump_lock: Mutex::new(()),
        });
        Self {
            lifetime: Arc::new(SupervisorLifetime {
                inner: Arc::downgrade(&inner),
            }),
            inner,
        }
    }

    pub fn runtime_available(&self) -> bool {
        self.semantic_assets().is_some() && self.inner.inference.is_some()
    }

    pub fn semantic_assets(&self) -> Option<PathBuf> {
        let assets = embed_assets(self.inner.resource_dir.as_deref());
        assets.join("config.json").is_file().then_some(assets)
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
            self.inner
                .store
                .for_each_database(|_, connection| {
                    let tx = connection
                        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                    super::graph_jobs::recover_interrupted(&tx)
                        .map_err(|e| crate::store::StoreError::Invalid(e.to_string()))?;
                    super::learning::recover(&tx)
                        .map_err(|e| crate::store::StoreError::Invalid(e.to_string()))?;
                    super::schema_writer::recover(&tx, now_ms())
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

    pub fn subscribe_learning(&self) -> tokio::sync::broadcast::Receiver<serde_json::Value> {
        self.inner.learning_events.subscribe()
    }

    pub fn subscribe_schema(&self) -> tokio::sync::broadcast::Receiver<serde_json::Value> {
        self.inner.schema_events.subscribe()
    }

    pub fn wake(&self) {
        self.ensure_started();
        self.inner.notify.notify_one();
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
                    inner.run().await;
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
        // Local embeddings and SQLite only: no model call.
        let assets = embed_assets(self.inner.resource_dir.as_deref());
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
        published_by: None,
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
        self.shutdown.notify_waiters();
        self.notify.notify_waiters();
        let task = self.task.lock().expect("graph task lock").take();
        if let Some(task) = &task {
            task.abort();
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

    async fn run(&self) {
        let mut checked_at = now_ms();
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
                .for_each_database(|_, connection| {
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
            let next_learning = next_learning.into_iter().chain(next_probe).min();
            if let Some(at) = next_learning {
                tokio::select! {
                    _ = &mut notified => {},
                    _ = tokio::time::sleep(std::time::Duration::from_millis(
                        at.saturating_sub(now_ms()).max(0) as u64,
                    )) => {},
                }
            } else {
                notified.await;
            }
            if self.stopped.load(Ordering::SeqCst) {
                break;
            }
            // Keep deadlines that expire between the last admission check and
            // the next wait; comparing with a later wall time could miss them.
            checked_at = now_ms();
            loop {
                if self.stopped.load(Ordering::SeqCst) {
                    break;
                }
                match self.until_shutdown(self.pump_once()).await {
                    Ok(Some(_)) => continue,
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        }
    }

    async fn pump_once(&self) -> KnowledgeResult<Option<String>> {
        // Public drains and the background loop share one queue consumer.
        // Keep its result with the claim through publication.
        let _pump = self.pump_lock.lock().await;
        if self.stopped.load(Ordering::SeqCst) {
            return Ok(None);
        }
        if let Some(id) = self.pump_schema().await? {
            return Ok(Some(id));
        }
        if let Some(id) = self.pump_learning().await? {
            return Ok(Some(id));
        }
        let candidates = self
            .store
            .for_each_database(|_, connection| {
                let tx = connection.transaction()?;
                super::graph_jobs::queued_loupist_heads(&tx)
                    .map_err(|e| crate::store::StoreError::Invalid(e.to_string()))
            })
            .map_err(|_| KnowledgeError::Invalid("could not read Loupist jobs"))?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let mut selected = None;
        for candidate in candidates {
            let settings = load_settings(
                &*self
                    .store
                    .lock_loupist(&candidate.loupist_id)
                    .map_err(|_| KnowledgeError::LockPoisoned)?,
                &candidate.loupist_id,
            )?;
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
            selected = Some((candidate, settings, auth_mode));
            break;
        }
        let Some((job, mut settings, auth_mode)) = selected else {
            return Ok(None);
        };
        let knowledge = loupist_knowledge(self.store.clone(), &job.loupist_id, &self.app_data)?;
        let _activity = self
            .maintenance
            .admit_activity_when_available(MaintenanceBlocker {
                id: format!("graph-{}", job.job_id),
                kind: "graph".into(),
                label: "Knowledge map processing".into(),
            })
            .await
            .map_err(|_| KnowledgeError::Invalid("graph activity gate is unavailable"))?;
        let mut run_log =
            crate::runs::log::RunLog::new("world", &job.loupist_id, &job.job_id, now_ms());
        run_log.model = settings.helper_model.as_ref().map(|model| {
            format!(
                "{}/{}",
                settings.helper_provider.as_deref().unwrap_or("unknown"),
                model
            )
        });
        run_log.effort = Some(settings.thinking_level.clone());
        let usage = Arc::new(std::sync::Mutex::new(
            crate::runs::log::InferenceUsage::default(),
        ));
        let calls = Arc::new(WorldCalls::default());
        let recovery = job.document.episode_kind == "world_recovery";
        let inference = self.inference.clone();
        let shared_store = self.store.clone();
        let inference_loupist = job.loupist_id.clone();
        let frozen = settings.clone();
        let assets = embed_assets(self.resource_dir.as_deref());
        let callback_usage = usage.clone();
        let callback_calls = calls.clone();
        let followed = self
            .store
            .map_context()
            .map(|context| {
                context
                    .watchlist
                    .into_iter()
                    .chain(context.holdings)
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
            async move {
                let kind = request
                    .get("kind")
                    .or_else(|| request.pointer("/params/kind"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let paid = kind == "llm";
                if paid {
                    usage.lock().unwrap_or_else(|e| e.into_inner()).begin();
                }
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
                    usage.lock().unwrap_or_else(|e| e.into_inner()).observe(
                        &reply.body["usage"],
                        reply.disposition == super::graph_inference::InferenceDisposition::NotSent,
                    );
                }
                calls.observe(&kind, &reply);
                reply
            }
        };
        let driven = super::world_extract::drive_job(
            &knowledge,
            &mut settings,
            &job.job_id,
            &auth_mode,
            now_ms(),
            &followed,
            &embed_assets(self.resource_dir.as_deref()),
            on_inference,
        )
        .await;
        run_log.usage = usage.lock().unwrap_or_else(|e| e.into_inner()).snapshot();
        calls.record(&mut run_log);
        match &driven {
            Ok(view) => match view.status.as_str() {
                "completed" | "reconciled_without_retry" => run_log.finish("ok", None),
                "queued" | "paused_budget" => run_log.finish("deferred", None),
                "cancelled_before_start" => run_log.finish("cancelled", None),
                _ => run_log.finish("failed", Some("world_job_failed")),
            },
            Err(_) => run_log.finish("failed", Some("world_job_failed")),
        }
        if let Ok(view) = &driven {
            if view.document.sealed_revision_id != job.document.sealed_revision_id
                && view.document.sealed_revision_id.is_some()
            {
                run_log.memory_changes.insert("World".into(), 1);
            }
        }
        run_log.record(self.store.as_ref());
        if recovery && driven.is_err() {
            knowledge.transact(|write| super::graph_jobs::fail_recovery(write.tx, &job.job_id))?;
        }
        if !self.stopped.load(Ordering::SeqCst) {
            merge_quarantine(&self.store, &job.loupist_id, &settings)?;
        }
        driven.map(|view| {
            // A queued job that could not be claimed needs a settings change or
            // another attempt to finish before this loop can make progress.
            (view.status != super::graph_jobs::JOB_QUEUED).then_some(view.job_id)
        })
    }

    async fn pump_schema(&self) -> KnowledgeResult<Option<String>> {
        let candidates = self
            .store
            .for_each_database(|_, connection| {
                let mut query = connection.prepare(
                    "SELECT loupist_id, proposal_id FROM knowledge_schema_proposals
                     WHERE status = 'preparing' ORDER BY created_at_ms, proposal_id",
                )?;
                let rows = query
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .map_err(|_| KnowledgeError::Invalid("could not read schema proposals"))?;
        for (loupist, proposal_id) in candidates.into_iter().flatten() {
            let knowledge = loupist_knowledge(self.store.clone(), &loupist, &self.app_data)?;
            let settings = load_settings(
                &*self
                    .store
                    .lock_loupist(&loupist)
                    .map_err(|_| KnowledgeError::LockPoisoned)?,
                &loupist,
            )?;
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
                    label: "Extending the World schema".into(),
                })
                .await
                .map_err(|_| KnowledgeError::Invalid("schema activity gate is unavailable"))?;
            let started = now_ms();
            let claimed = knowledge.transact(|write| {
                super::schema_writer::claim(write.tx, &loupist, &profile.focus, now_ms())
            })?;
            match claimed {
                super::schema_writer::Claimed::Wait => continue,
                super::schema_writer::Claimed::Settled(finish) => {
                    self.note_schema(&loupist, &proposal_id, &settings, started, &finish, None);
                    return Ok(Some(proposal_id));
                }
                super::schema_writer::Claimed::Ready(attempt) => {
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
                    let finished = knowledge.transact(|write| {
                        super::schema_writer::finish(write, &attempt, &reply, now_ms())
                    });
                    if let Ok(finish) = &finished {
                        self.note_schema(
                            &loupist,
                            &attempt.proposal_id,
                            &attempt.settings,
                            started,
                            finish,
                            Some(&reply),
                        );
                    } else {
                        let mut run_log = crate::runs::log::RunLog::new(
                            "schema",
                            &loupist,
                            &attempt.proposal_id,
                            started,
                        );
                        run_log.finish("failed", Some("schema_proposal_failed"));
                        run_log.record(self.store.as_ref());
                    }
                    finished?;
                    return Ok(Some(attempt.proposal_id));
                }
            }
        }
        Ok(None)
    }

    fn note_schema(
        &self,
        loupist: &str,
        proposal_id: &str,
        settings: &GraphProcessingSettings,
        started: i64,
        finish: &super::schema_writer::Finish,
        reply: Option<&super::graph_inference::InferenceReply>,
    ) {
        let mut run_log = crate::runs::log::RunLog::new("schema", loupist, proposal_id, started);
        run_log.model = settings.helper_model.as_ref().map(|model| {
            format!(
                "{}/{}",
                settings.helper_provider.as_deref().unwrap_or("unknown"),
                model
            )
        });
        run_log.effort = Some(settings.thinking_level.clone());
        if let Some(reply) = reply {
            let mut usage = crate::runs::log::InferenceUsage::default();
            usage.begin();
            usage.observe(
                &reply.body["usage"],
                reply.disposition == super::graph_inference::InferenceDisposition::NotSent,
            );
            run_log.usage = usage.snapshot();
        }
        super::schema_writer::note_finish(&mut run_log, finish);
        run_log.record(self.store.as_ref());
        if finish.notifies {
            let _ = self
                .schema_events
                .send(super::schema_writer::schema_change_event(loupist));
        }
    }

    async fn pump_learning(&self) -> KnowledgeResult<Option<String>> {
        let candidates = self.store.for_each_database(|_, connection| {
            let mut q=connection.prepare("SELECT loupist_id,review_id FROM knowledge_learning_reviews WHERE status IN ('pending','queued') ORDER BY created_at_ms")?;
            let rows=q.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<Result<Vec<_>,_>>()?;
            Ok(rows)
        }).map_err(|_|KnowledgeError::Invalid("could not read learning jobs"))?;
        for (loupist, review_id) in candidates.into_iter().flatten() {
            let knowledge = loupist_knowledge(self.store.clone(), &loupist, &self.app_data)?;
            let settings = load_settings(
                &*self
                    .store
                    .lock_loupist(&loupist)
                    .map_err(|_| KnowledgeError::LockPoisoned)?,
                &loupist,
            )?;
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
            let attempt = knowledge.transact(|w| {
                super::learning::claim(w.tx, &loupist, &profile.name, &profile.focus, now_ms())
            })?;
            let Some(attempt) = attempt else {
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
                        .send(serde_json::json!({"loupistId":loupist,"review":review}));
                    return Ok(Some(review_id));
                }
                continue;
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
            let mut usage = crate::runs::log::InferenceUsage::default();
            usage.begin();
            usage.observe(
                &reply.body["usage"],
                reply.disposition == super::graph_inference::InferenceDisposition::NotSent,
            );
            run_log.usage = usage.snapshot();
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
                    .send(serde_json::json!({"loupistId":loupist,"review":review}));
            }
            run_log.record(self.store.as_ref());
            return Ok(Some(attempt.review_id));
        }
        Ok(None)
    }
}

async fn current_inference(
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
    if settings.helper_provider != frozen.helper_provider
        || settings.helper_model != frozen.helper_model
        || settings.thinking_level != frozen.thinking_level
        || settings.run_options != frozen.run_options
        || settings.max_llm_calls != frozen.max_llm_calls
        || settings.timeout_ms != frozen.timeout_ms
    {
        return not_sent_reply(serde_json::json!({})).await;
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
        let staged = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/embed_assets");
        if staged.join("config.json").is_file() {
            return staged;
        }
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../src/knowledge/embed_assets")
    }
    #[cfg(not(debug_assertions))]
    {
        resource_dir
            .unwrap_or_else(|| Path::new(""))
            .join("embed_assets")
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
