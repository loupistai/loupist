use super::*;
use crate::knowledge::{
    AddToWorld, CreateRecord, RecordKind, RetainMaterial, RetainSource, Selector, SourceClass,
};

fn supervised(directory: &Path, resources: Option<PathBuf>) -> (GraphSupervisor, KnowledgeStore) {
    let store = Arc::new(SqliteStore::open(directory.join("app.sqlite")).unwrap());
    let supervisor = GraphSupervisor::with_inference(
        store.clone(),
        directory.to_path_buf(),
        resources,
        MaintenanceCoordinator::default(),
        None,
    );
    let blobs = BlobStore::open(directory.join("knowledge-blobs")).unwrap();
    (supervisor, KnowledgeStore::attach(store, blobs))
}

#[tokio::test]
async fn only_the_last_supervisor_owner_cancels_and_releases_its_background_loop() {
    let directory = tempfile::tempdir().unwrap();
    let (supervisor, _knowledge) = supervised(directory.path(), None);
    let other = supervisor.clone();
    let inner = Arc::downgrade(&supervisor.inner);
    supervisor.ensure_started();
    tokio::task::yield_now().await;
    drop(supervisor);
    assert!(!inner.upgrade().unwrap().stopped.load(Ordering::SeqCst));
    drop(other);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while inner.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("background loop released its owned state");
}

#[cfg(unix)]
#[tokio::test]
async fn startup_recovers_an_orphaned_attempt_once_and_waits_for_provider_inference() {
    let directory = tempfile::tempdir().unwrap();
    let (supervisor, knowledge) = supervised(directory.path(), None);
    let loupist = "loupist-a";
    let (source, digest, span) = retained_source(
        &knowledge,
        loupist,
        b"Ada described the engine.",
        1_700_000_000_000,
    );
    let (world, revision) = sealed_world(&knowledge, loupist, &source, &digest, &span);
    let source_revision = knowledge
        .read(|read| read.owned_record(loupist, &source))
        .unwrap()
        .revision_id
        .unwrap();
    let settings = GraphProcessingSettings {
        enrichment_enabled: true,
        helper_provider: Some("openai".to_string()),
        helper_model: Some("gpt-test".to_string()),
        ..GraphProcessingSettings::default()
    };
    store_settings(&supervisor.inner.store.lock().unwrap(), loupist, &settings).unwrap();
    let job = knowledge
        .graph_accept_episode(
            &settings,
            AddToWorld {
                loupist_id: loupist.to_string(),
                world_record_id: world,
                world_revision_id: revision,
                source_record_id: source,
                source_revision_id: source_revision,
                span_id: span,
                at_ms: 1_700_000_000_000,
                binding_role: "extracted_from".to_string(),
            },
        )
        .unwrap();
    let status = || knowledge.graph_job(loupist, &job.job_id).unwrap().status;
    // No provider inference service: the job is not dispatched.
    assert!(supervisor.pump_once().await.unwrap().is_none());
    assert_eq!(status(), super::super::graph_jobs::JOB_QUEUED);
    // Startup owns the app-data lock, so an undispatched prior attempt is
    // recovered immediately, even before its lease expires.
    knowledge
        .graph_mark_running(&settings, &job.job_id, now_ms())
        .unwrap();
    supervisor.start().unwrap();
    assert_eq!(status(), super::super::graph_jobs::JOB_QUEUED);
    // Only the first start carries orphan-recovery authority.
    knowledge
        .graph_mark_running(&settings, &job.job_id, now_ms())
        .unwrap();
    supervisor.start().unwrap();
    assert_eq!(status(), super::super::graph_jobs::JOB_RUNNING);
    supervisor.shutdown().await.unwrap();
}

#[tokio::test]
async fn loupist_settings_pause_and_frozen_model_controls_are_checked_on_every_callback() {
    use super::super::graph_inference::InferenceDisposition::{Completed, NotSent};
    let store = SqliteStore::open_in_memory().unwrap();
    let assets = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../src/knowledge/embed_assets");
    let original = GraphProcessingSettings {
        enrichment_enabled: true,
        helper_provider: Some("openai".into()),
        helper_model: Some("gpt-test".into()),
        ..Default::default()
    };
    let save = |loupist: &str, settings: &GraphProcessingSettings| {
        store_settings(&store.lock().unwrap(), loupist, settings).unwrap()
    };
    let request = serde_json::json!({"kind":"embedding", "inputs":["Ada described the engine"], "timeout_ms":1000});
    let dispatch = |loupist: &'static str| {
        current_inference(&store, loupist, &original, None, &assets, &request)
    };
    save("loupist-a", &original);
    save(
        "loupist-b",
        &GraphProcessingSettings {
            paused: true,
            ..original.clone()
        },
    );
    assert_eq!(dispatch("loupist-a").await.disposition, Completed);
    assert_eq!(dispatch("loupist-b").await.disposition, NotSent);
    // A saved model or control change after the job froze its inputs refuses the callback.
    let mut changed = original.clone();
    changed.helper_provider = Some("xai".into());
    save("loupist-a", &changed);
    assert_eq!(dispatch("loupist-a").await.disposition, NotSent);
    let mut changed = original.clone();
    changed.thinking_level = "high".into();
    save("loupist-a", &changed);
    assert_eq!(dispatch("loupist-a").await.disposition, NotSent);
    let mut changed = original.clone();
    changed
        .run_options
        .insert("serviceTier".into(), "priority".into());
    save("loupist-a", &changed);
    assert_eq!(dispatch("loupist-a").await.disposition, NotSent);
    // Quarantine merges into the saved settings without touching other controls or Loupists.
    let driven = GraphProcessingSettings {
        quarantined_generations: vec!["gen_new".into()],
        ..original.clone()
    };
    merge_quarantine(&store, "loupist-a", &driven).unwrap();
    let merged = load_settings(&store.lock().unwrap(), "loupist-a").unwrap();
    assert_eq!(merged.run_options, changed.run_options);
    assert_eq!(merged.quarantined_generations, ["gen_new"]);
    assert!(load_settings(&store.lock().unwrap(), "loupist-b")
        .unwrap()
        .quarantined_generations
        .is_empty());
}

fn retained_source(
    store: &KnowledgeStore,
    loupist: &str,
    bytes: &[u8],
    at_ms: i64,
) -> (String, String, String) {
    let record = store
        .create_record(CreateRecord {
            loupist_id: loupist.into(),
            kind: RecordKind::Source,
            title: "Notes".into(),
            at_ms,
        })
        .unwrap();
    let retained = store
        .retain_source(RetainSource {
            origin: None,
            loupist_id: loupist.into(),
            record_id: record.record_id.clone(),
            expected_revision_id: None,
            at_ms,
            source_id: None,
            title: "Notes".into(),
            source_class: SourceClass::PublisherDocument,
            origin_locator: "https://example.test/notes".into(),
            provider_identity: "example-publisher".into(),
            authorship: crate::knowledge::Authorship::Publisher,
            coverage: crate::knowledge::Coverage::IncludedFull,
            origin_chat_id: None,
            request_ref: None,
            declared_digest: None,
            publication_time: None,
            material: Some(RetainMaterial {
                media_type: "text/plain".into(),
                role: crate::knowledge::RepresentationRole::Original,
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
    (
        record.record_id,
        retained.revision_digest,
        retained.span_id.unwrap(),
    )
}

/// A World with one claim citing `span` from `source`.
fn sealed_world(
    store: &KnowledgeStore,
    loupist: &str,
    source: &str,
    digest: &str,
    span: &str,
) -> (String, String) {
    use crate::knowledge::{
        BindingRole, ClaimEndpoint, ClaimStatus, DependencyRole, EvidenceBinding, EvidenceRevision,
        GraphClaim, GraphNode, SealWorld,
    };
    let record = store
        .create_record(CreateRecord {
            loupist_id: loupist.into(),
            kind: RecordKind::World,
            title: "World".into(),
            at_ms: 1_700_000_000_000,
        })
        .unwrap();
    let node = |id: &str, name: &str| GraphNode {
        id: id.to_string(),
        name: name.to_string(),
        aliases: Vec::new(),
        description: String::new(),
        kind: "concept".to_string(),
        source_role: "source".to_string(),
        properties: std::collections::BTreeMap::new(),
    };
    let endpoint = |node_id: &str, role: &str| ClaimEndpoint {
        node_id: node_id.to_string(),
        role: role.to_string(),
    };
    let sealed = store
        .seal_world(SealWorld {
            loupist_id: loupist.to_string(),
            record_id: record.record_id.clone(),
            expected_revision_id: None,
            at_ms: 1_700_000_000_000,
            nodes: vec![node("ada", "Ada"), node("engine", "Engine")],
            claims: vec![GraphClaim {
                id: "described".to_string(),
                proposition: "Ada described the engine".to_string(),
                attribution: "example-publisher".to_string(),
                relation: "described".to_string(),
                endpoints: vec![endpoint("ada", "source"), endpoint("engine", "target")],
                qualifiers: Vec::new(),
                evidence: vec![EvidenceBinding {
                    span_id: span.to_string(),
                    role: BindingRole::ExtractedFrom,
                    attribution: "example-publisher".to_string(),
                }],
                status: ClaimStatus::Active,
                recorded_at_ms: Some(1_700_000_000_000),
                publication_time: None,
                effective_time: None,
                valid_at: None,
                invalid_at: None,
                expired_at: None,
            }],
            annotations: Vec::new(),
            dependencies: vec![EvidenceRevision {
                record_id: source.to_string(),
                revision_digest: digest.to_string(),
                role: DependencyRole::WorldSource,
            }],
        })
        .unwrap();
    (record.record_id, sealed.revision_id)
}

#[tokio::test]
async fn rust_engine_searches_a_sealed_world_without_a_worker_or_neo4j() {
    let directory = tempfile::tempdir().unwrap();
    let (supervisor, knowledge) = supervised(directory.path(), None);
    let loupist = "loupist-a";
    let (source, digest, span) = retained_source(
        &knowledge,
        loupist,
        b"Ada described the engine.",
        1_700_000_000_000,
    );
    let (world, revision) = sealed_world(&knowledge, loupist, &source, &digest, &span);
    // A pre-port cache may remain on disk, but only SQLite supplies the World.
    let obsolete = directory.path().join("graph-instance");
    std::fs::create_dir(&obsolete).unwrap();
    std::fs::write(obsolete.join("stale"), b"unread legacy data").unwrap();
    let job = supervisor
        .index_sealed_world(loupist, &world, &revision)
        .unwrap();
    assert!(job.document.artifact_utf8.is_none());
    assert_eq!(
        supervisor.pump_once().await.unwrap().as_deref(),
        Some(job.job_id.as_str())
    );
    let finished = knowledge.graph_job(loupist, &job.job_id).unwrap();
    assert_eq!(finished.status, super::super::graph_jobs::JOB_COMPLETED);
    assert!(!finished.document.inference_attempted);
    assert!(!finished.document.mutation_sent);
    let found = supervisor
        .query_indexed(loupist, &world, &revision, "Ada engine")
        .await
        .unwrap();
    assert_eq!(found["mode"], "hybrid");
    assert_eq!(found["indexed"], true);
    assert_eq!(found["view"]["view"]["edges"][0]["adapter_id"], "described");
    assert!(!directory.path().join("graph-leases").exists());
    assert_eq!(
        std::fs::read(obsolete.join("stale")).unwrap(),
        b"unread legacy data"
    );
    supervisor.shutdown().await.unwrap();
}

#[tokio::test]
async fn rust_engine_drives_queued_jobs_without_a_staged_worker_or_runtime() {
    let directory = tempfile::tempdir().unwrap();
    let (supervisor, knowledge) = supervised(directory.path(), None);
    let loupist = "loupist-a";
    let settings = GraphProcessingSettings {
        enrichment_enabled: true,
        helper_provider: Some("fixture".into()),
        helper_model: Some("fixture-local".into()),
        ..GraphProcessingSettings::default()
    };
    store_settings(&supervisor.inner.store.lock().unwrap(), loupist, &settings).unwrap();
    let (source, digest, span) = retained_source(
        &knowledge,
        loupist,
        b"Ada described the engine.",
        1_700_000_000_000,
    );
    let (world, revision) = sealed_world(&knowledge, loupist, &source, &digest, &span);
    let source_revision = knowledge
        .read(|read| read.owned_record(loupist, &source))
        .unwrap()
        .revision_id
        .unwrap();
    let job = knowledge
        .graph_accept_episode(
            &settings,
            AddToWorld {
                loupist_id: loupist.into(),
                world_record_id: world,
                world_revision_id: revision,
                source_record_id: source,
                source_revision_id: source_revision,
                span_id: span,
                at_ms: 1_700_000_000_000,
                binding_role: "extracted_from".into(),
            },
        )
        .unwrap();
    // No worker or graph runtime is staged; the job still runs in-process. With
    // no provider service the paid call is not sent, which costs nothing: the
    // job queues again, spends no retry and never quarantines the World.
    for attempt in 1..=3u8 {
        assert_eq!(supervisor.pump_once().await.unwrap(), None);
        let queued = knowledge.graph_job(loupist, &job.job_id).unwrap();
        assert_eq!(queued.status, super::super::graph_jobs::JOB_QUEUED);
        assert_eq!(queued.document.unsent_attempts, attempt);
        assert_eq!(queued.document.interrupted_attempts, 0);
        assert!(!queued.document.inference_attempted);
    }
    assert!(
        load_settings(&supervisor.inner.store.lock().unwrap(), loupist)
            .unwrap()
            .quarantined_generations
            .is_empty()
    );
    // Two paid attempts that published nothing quarantine the generation.
    let mut driven = settings.clone();
    for _ in 0..2 {
        let running = knowledge
            .graph_mark_running(&driven, &job.job_id, now_ms())
            .unwrap();
        let attempt = running.document.attempt_id.clone().unwrap();
        knowledge
            .graph_note_inference(&job.job_id, &attempt)
            .unwrap();
        knowledge
            .graph_finish(
                &mut driven,
                &job.job_id,
                &attempt,
                super::super::graph_jobs::JOB_FAILED_BEFORE,
                true,
                false,
                now_ms(),
            )
            .unwrap();
    }
    merge_quarantine(&supervisor.inner.store, loupist, &driven).unwrap();
    let finished = knowledge.graph_job(loupist, &job.job_id).unwrap();
    assert_eq!(finished.document.interrupted_attempts, 1);
    assert_eq!(finished.status, super::super::graph_jobs::JOB_RECONCILE);
    assert!(finished.document.inference_attempted);
    assert!(!finished.document.mutation_sent);
    assert!(!directory.path().join("graph-instance").exists());
    // The next pump rebuilds the quarantined World without a settings visit
    // and never replays the interrupted call.
    let rebuild = supervisor
        .pump_once()
        .await
        .unwrap()
        .expect("an unattended rebuild runs");
    assert_ne!(rebuild, job.job_id);
    assert_eq!(
        knowledge.graph_job(loupist, &rebuild).unwrap().status,
        super::super::graph_jobs::JOB_COMPLETED
    );
    assert_eq!(
        knowledge.graph_job(loupist, &job.job_id).unwrap().status,
        super::super::graph_jobs::JOB_RECONCILED
    );
    assert_eq!(supervisor.pump_once().await.unwrap(), None);
}

#[test]
fn schedule_rank_puts_the_attended_loupist_first_then_takes_turns() {
    // A recent Chat turn wins over every unattended Loupist, latest turn first.
    assert!(schedule_rank(Some(10), 9, 500) < schedule_rank(None, 0, 1));
    assert!(schedule_rank(Some(20), 9, 500) < schedule_rank(Some(10), 0, 1));
    // Otherwise the Loupist served longest ago (or never) goes next, then the
    // older job.
    assert!(schedule_rank(None, 0, 900) < schedule_rank(None, 3, 1));
    assert!(schedule_rank(None, 3, 1) < schedule_rank(None, 3, 2));
}

/// Queue `count` Source jobs for `loupist` in a fresh World, `at_ms` apart.
fn queued_jobs(knowledge: &KnowledgeStore, loupist: &str, count: usize, at_ms: i64) -> Vec<String> {
    let settings = fixture_settings();
    let (first, digest, span) =
        retained_source(knowledge, loupist, b"Ada described the engine.", at_ms);
    let (world, revision) = sealed_world(knowledge, loupist, &first, &digest, &span);
    (0..count)
        .map(|index| {
            let at = at_ms + index as i64;
            let (source, _, span) = retained_source(
                knowledge,
                loupist,
                format!("Note {index} about the engine.").as_bytes(),
                at,
            );
            let source_revision = knowledge
                .read(|read| read.owned_record(loupist, &source))
                .unwrap()
                .revision_id
                .unwrap();
            knowledge
                .graph_accept_episode(
                    &settings,
                    AddToWorld {
                        loupist_id: loupist.into(),
                        world_record_id: world.clone(),
                        world_revision_id: revision.clone(),
                        source_record_id: source,
                        source_revision_id: source_revision,
                        span_id: span,
                        at_ms: at,
                        binding_role: "extracted_from".into(),
                    },
                )
                .unwrap()
                .job_id
        })
        .collect()
}

fn fixture_settings() -> GraphProcessingSettings {
    GraphProcessingSettings {
        enrichment_enabled: true,
        helper_provider: Some("fixture".into()),
        helper_model: Some("fixture-local".into()),
        ..GraphProcessingSettings::default()
    }
}

fn select(supervisor: &GraphSupervisor) -> Option<(String, String)> {
    supervisor
        .inner
        .select_world_job()
        .unwrap()
        .map(|selected| (selected.job.loupist_id, selected.job.job_id))
}

fn finish_worker(supervisor: &GraphSupervisor, loupist: &str) {
    supervisor.inner.running.lock().unwrap().remove(loupist);
}

#[tokio::test]
async fn a_backlog_in_one_loupist_no_longer_holds_back_another() {
    let directory = tempfile::tempdir().unwrap();
    let (supervisor, knowledge) = supervised(directory.path(), None);
    for loupist in ["loupist-backlog", "loupist-new"] {
        store_settings(
            &supervisor.inner.store.lock().unwrap(),
            loupist,
            &fixture_settings(),
        )
        .unwrap();
    }
    let backlog = queued_jobs(&knowledge, "loupist-backlog", 3, 1_700_000_000_000);
    let fresh = queued_jobs(&knowledge, "loupist-new", 1, 1_700_000_100_000);
    // Neither has been served: the older job goes first.
    assert_eq!(
        select(&supervisor),
        Some(("loupist-backlog".into(), backlog[0].clone()))
    );
    finish_worker(&supervisor, "loupist-backlog");
    // With one slot free, the Loupist that has waited takes its turn before the
    // backlog's next job (the old queue drained the backlog first).
    assert_eq!(
        select(&supervisor),
        Some(("loupist-new".into(), fresh[0].clone()))
    );
    // One job per Loupist at a time: the backlog runs alongside it.
    assert_eq!(select(&supervisor).unwrap().0, "loupist-backlog");
    assert_eq!(
        select(&supervisor),
        None,
        "both Loupists already have a job running"
    );
}

#[tokio::test]
async fn the_loupist_with_a_recent_chat_turn_goes_first() {
    let directory = tempfile::tempdir().unwrap();
    let (supervisor, knowledge) = supervised(directory.path(), None);
    for loupist in ["loupist-backlog", "loupist-research"] {
        store_settings(
            &supervisor.inner.store.lock().unwrap(),
            loupist,
            &fixture_settings(),
        )
        .unwrap();
    }
    queued_jobs(&knowledge, "loupist-backlog", 3, 1_700_000_000_000);
    let research = queued_jobs(&knowledge, "loupist-research", 1, 1_700_000_100_000);
    supervisor.note_attention("loupist-research", "chat-research");
    assert_eq!(
        select(&supervisor),
        Some(("loupist-research".into(), research[0].clone()))
    );
    finish_worker(&supervisor, "loupist-research");
    // A Loupist with nothing runnable does not hold the others back.
    knowledge
        .graph_mark_running(&fixture_settings(), &research[0], now_ms())
        .unwrap();
    assert_eq!(select(&supervisor).unwrap().0, "loupist-backlog");
}

#[tokio::test]
async fn a_job_that_made_no_progress_waits_for_a_wake_from_outside_the_loop() {
    let directory = tempfile::tempdir().unwrap();
    let (supervisor, knowledge) = supervised(directory.path(), None);
    store_settings(
        &supervisor.inner.store.lock().unwrap(),
        "loupist-a",
        &fixture_settings(),
    )
    .unwrap();
    let jobs = queued_jobs(&knowledge, "loupist-a", 1, 1_700_000_000_000);
    supervisor
        .inner
        .stalled
        .lock()
        .unwrap()
        .insert(jobs[0].clone());
    assert_eq!(select(&supervisor), None);
    supervisor.inner.stalled.lock().unwrap().clear();
    assert_eq!(
        select(&supervisor),
        Some(("loupist-a".into(), jobs[0].clone()))
    );
}

#[tokio::test]
async fn the_loop_runs_world_jobs_for_different_loupists_up_to_the_job_limit() {
    let directory = tempfile::tempdir().unwrap();
    let (supervisor, knowledge) = supervised(directory.path(), None);
    let loupists = ["loupist-a", "loupist-b", "loupist-c"];
    for (index, loupist) in loupists.iter().enumerate() {
        store_settings(
            &supervisor.inner.store.lock().unwrap(),
            loupist,
            &fixture_settings(),
        )
        .unwrap();
        queued_jobs(
            &knowledge,
            loupist,
            2,
            1_700_000_000_000 + index as i64 * 1_000,
        );
    }
    let inner = supervisor.inner.clone();
    assert!(inner.step().await.unwrap());
    assert_eq!(inner.running_count(), MAX_WORLD_JOBS);
    let running: HashSet<String> = inner.running.lock().unwrap().keys().cloned().collect();
    assert_eq!(
        running.len(),
        MAX_WORLD_JOBS,
        "never two jobs for one Loupist"
    );
    // With no provider nothing is sent: each worker frees its slot and its job
    // waits for a wake from outside the loop.
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while inner.running_count() > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("workers finish");
    assert_eq!(inner.stalled.lock().unwrap().len(), MAX_WORLD_JOBS);
    assert!(inner.step().await.unwrap());
    let next: Vec<String> = inner.running.lock().unwrap().keys().cloned().collect();
    assert_eq!(next.len(), 1);
    assert!(
        !running.contains(&next[0]),
        "the Loupist that waited runs now"
    );
    supervisor.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_mac_stays_awake_while_world_jobs_run_and_sleeps_when_they_stop() {
    let directory = tempfile::tempdir().unwrap();
    let (supervisor, knowledge) = supervised(directory.path(), None);
    store_settings(
        &supervisor.inner.store.lock().unwrap(),
        "loupist-a",
        &fixture_settings(),
    )
    .unwrap();
    queued_jobs(&knowledge, "loupist-a", 2, 1_700_000_000_000);
    let inner = supervisor.inner.clone();
    // Off by default: running work never holds the Mac awake.
    assert!(select(&supervisor).is_some());
    inner.update_keep_awake();
    assert!(!supervisor.keep_awake().status().active);
    let status = supervisor.keep_awake().set_enabled(true).unwrap();
    assert!(
        status.active,
        "turning it on applies to the work already running"
    );
    inner.update_keep_awake();
    let status = supervisor.keep_awake().status();
    assert!(status.active);
    assert_eq!(status.remaining, 2);
    assert!(supervisor.keep_awake().poll_interval().is_some());
    // The queue stopped moving on its own: the Mac may sleep.
    finish_worker(&supervisor, "loupist-a");
    inner.update_keep_awake();
    assert!(!supervisor.keep_awake().status().active);
    assert!(supervisor.keep_awake().poll_interval().is_none());
    // Shutdown drops the hold.
    assert!(select(&supervisor).is_some());
    inner.update_keep_awake();
    assert!(supervisor.keep_awake().status().active);
    supervisor.shutdown().await.unwrap();
    assert!(!supervisor.keep_awake().status().active);
}
