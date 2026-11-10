use super::*;
mod publication;
mod recovery;
use crate::knowledge::{
    Authorship, Coverage, CreateRecord, KnowledgeStore, RecordKind, RetainMaterial, RetainSource,
    Selector, SourceClass,
};

const T: i64 = 1_700_000_000_000;

fn open() -> (tempfile::TempDir, KnowledgeStore) {
    let directory = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).expect("open");
    (directory, store)
}

fn enabled() -> GraphProcessingSettings {
    GraphProcessingSettings {
        enrichment_enabled: true,
        helper_provider: Some("fixture".to_string()),
        helper_model: Some("fixture-local".to_string()),
        ..GraphProcessingSettings::default()
    }
}

fn source_request(loupist: &str, module: &str, bytes: &[u8]) -> RetainSource {
    RetainSource {
        origin: None,
        loupist_id: loupist.to_string(),
        record_id: module.to_string(),
        expected_revision_id: None,
        at_ms: T,
        source_id: None,
        title: "Notes".to_string(),
        source_class: SourceClass::PublisherDocument,
        origin_locator: "https://example.test/notes".to_string(),
        provider_identity: "example-publisher".to_string(),
        authorship: Authorship::Publisher,
        coverage: Coverage::IncludedFull,
        origin_chat_id: None,
        request_ref: None,
        declared_digest: None,
        publication_time: None,
        material: Some(RetainMaterial {
            media_type: "text/plain".to_string(),
            role: crate::knowledge::RepresentationRole::Original,
            document_id: "doc-notes".to_string(),
            parser_version: None,
            parent_representation_id: None,
            input_span_ids: Vec::new(),
            bytes: bytes.to_vec(),
            selector: Selector::ByteRange {
                start: 0,
                end: bytes.len() as u64,
            },
            context_note: "exact retained range".to_string(),
        }),
    }
}

fn retain(
    store: &KnowledgeStore,
    loupist: &str,
    module: &str,
    expected: Option<String>,
    bytes: &[u8],
) -> (String, String) {
    let retained = store
        .retain_source(RetainSource {
            expected_revision_id: expected,
            source_id: store
                .list_sources(loupist, module)
                .unwrap()
                .first()
                .map(|source| source.source_id.clone()),
            ..source_request(loupist, module, bytes)
        })
        .expect("retain");
    (retained.span_id.expect("span"), retained.revision_id)
}

fn world(store: &KnowledgeStore, loupist: &str) -> (String, String) {
    let module = store
        .create_record(CreateRecord {
            loupist_id: loupist.to_string(),
            kind: RecordKind::World,
            title: "World".to_string(),
            at_ms: 10,
        })
        .unwrap();
    let sealed = store
        .seal_world(crate::knowledge::SealWorld {
            loupist_id: loupist.to_string(),
            record_id: module.record_id.clone(),
            expected_revision_id: None,
            at_ms: T,
            nodes: Vec::new(),
            claims: Vec::new(),
            annotations: Vec::new(),
            dependencies: Vec::new(),
        })
        .unwrap();
    (module.record_id, sealed.revision_id)
}

fn source_module(store: &KnowledgeStore, loupist: &str) -> String {
    store
        .create_record(CreateRecord {
            loupist_id: loupist.to_string(),
            kind: RecordKind::Source,
            title: "Sources".to_string(),
            at_ms: 10,
        })
        .unwrap()
        .record_id
}

// Fixture inputs keep the independent World and Source revision identities explicit.
#[allow(clippy::too_many_arguments)]
fn add(
    store: &KnowledgeStore,
    settings: &GraphProcessingSettings,
    loupist: &str,
    world_module: &str,
    revision: &str,
    source_module: &str,
    source_revision: &str,
    span: &str,
) -> GraphJobView {
    store
        .graph_accept_episode(
            settings,
            AddToWorld {
                loupist_id: loupist.to_string(),
                world_record_id: world_module.to_string(),
                world_revision_id: revision.to_string(),
                source_record_id: source_module.to_string(),
                source_revision_id: source_revision.to_string(),
                span_id: span.to_string(),
                at_ms: T,
                binding_role: "extracted_from".to_string(),
            },
        )
        .unwrap()
}

/// One queued source job for `loupist` in a fresh World.
fn queued(
    store: &KnowledgeStore,
    settings: &GraphProcessingSettings,
    loupist: &str,
    bytes: &[u8],
) -> GraphJobView {
    let source = source_module(store, loupist);
    let (span, source_revision) = retain(store, loupist, &source, None, bytes);
    let (world_module, revision) = world(store, loupist);
    add(
        store,
        settings,
        loupist,
        &world_module,
        &revision,
        &source,
        &source_revision,
        &span,
    )
}

fn claim(
    store: &KnowledgeStore,
    settings: &GraphProcessingSettings,
    job_id: &str,
    at_ms: i64,
) -> GraphJobView {
    store.graph_mark_running(settings, job_id, at_ms).unwrap()
}

/// Claim, record a paid attempt, and publish through the app's completion path.
fn publish(
    store: &KnowledgeStore,
    settings: &mut GraphProcessingSettings,
    job: &GraphJobView,
    artifact: &str,
    at_ms: i64,
) -> KnowledgeResult<GraphJobView> {
    let running = claim(store, settings, &job.job_id, at_ms);
    assert_eq!(running.status, JOB_RUNNING);
    let attempt = running.document.attempt_id.clone().unwrap();
    store.graph_note_inference(&job.job_id, &attempt)?;
    store.graph_publish_and_complete(
        settings,
        &job.job_id,
        &attempt,
        artifact,
        &sha256(artifact.as_bytes()),
        at_ms + 100,
        false,
    )
}

fn heads(store: &KnowledgeStore) -> Vec<String> {
    store
        .read(|write| queued_loupist_heads(write.tx, &[]))
        .unwrap()
        .into_iter()
        .map(|job| job.job_id)
        .collect()
}

fn head(store: &KnowledgeStore, loupist: &str, record_id: &str) -> String {
    store
        .read(|write| write.owned_record(loupist, record_id))
        .unwrap()
        .revision_id
        .unwrap()
}

fn world_artifact(spans: &[&str]) -> String {
    let claims: Vec<Value> = spans
        .iter()
        .enumerate()
        .map(|(index, span)| {
            let (letter, proposition, relation) = [
                ("c", "Ada directed the engine.", "DIRECTED"),
                ("d", "Ada left the engine.", "LEFT"),
            ][index];
            json!({
                "adapter_id": format!("clm_{}", letter.repeat(32)),
                "proposition": proposition,
                "attribution": "example-publisher",
                "relation": relation,
                "endpoints": [
                    {"node_id": "nod_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "role": "source"},
                    {"node_id": "nod_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "role": "target"}
                ],
                "qualifiers": [],
                "evidence": [{
                    "span_id": span,
                    "role": "extracted_from",
                    "attribution": "example-publisher"
                }],
                "status": "active",
                "recorded_at_ms": T,
                "publication_time": {"precision": "unknown", "value": null, "approximate": false},
                "effective_time": null,
                "valid_at": null,
                "invalid_at": null,
                "expired_at": null
            })
        })
        .collect();
    let node = |id: &str, name: &str| {
        json!({
            "adapter_id": id, "name": name, "aliases": [], "description": "",
            "kind": "Entity", "source_role": "source", "properties": {}
        })
    };
    json!({
        "format": "loupist.world_graph_snapshot",
        "nodes": [
            node("nod_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "Ada"),
            node("nod_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "Engine")
        ],
        "claims": claims
    })
    .to_string()
}

#[test]
fn per_loupist_preferences_and_deletion_do_not_affect_other_loupists() {
    let store = crate::store::SqliteStore::open_in_memory().unwrap();
    let mut connection = store.lock().unwrap();
    let first = GraphProcessingSettings {
        helper_model_profile_id: Some("fixture/fixture-local".into()),
        ..enabled()
    };
    let second = GraphProcessingSettings {
        paused: true,
        ..Default::default()
    };
    store_settings(&connection, "loupist-a", &first).unwrap();
    store_settings(&connection, "loupist-b", &second).unwrap();
    assert_eq!(load_settings(&connection, "loupist-a").unwrap(), first);
    let tx = connection.transaction().unwrap();
    crate::knowledge::erase_loupist_private(&tx, "loupist-a").unwrap();
    tx.commit().unwrap();
    assert_eq!(
        load_settings(&connection, "loupist-a").unwrap(),
        GraphProcessingSettings::default()
    );
    assert_eq!(load_settings(&connection, "loupist-b").unwrap(), second);
}

#[test]
fn renderer_settings_cannot_spoof_authority_or_clear_quarantine() {
    let valid = serde_json::to_value(GraphProcessingSettings::default().public_settings()).unwrap();
    for key in [
        "helperAuthMode",
        "helperProvider",
        "authKind",
        "embeddingProfile",
    ] {
        let mut spoof = valid.clone();
        spoof[key] = json!("oauth");
        assert!(
            serde_json::from_value::<GraphPublicSettings>(spoof).is_err(),
            "{key}"
        );
    }
    let mut current = enabled();
    current.inactive_loupists = vec!["loupist-b".to_string()];
    current.quarantined_generations = vec!["gen_locked".to_string()];
    let mut tampered = current.public_settings();
    tampered.paused = true;
    current.apply_public(&tampered, None);
    assert_eq!(current.inactive_loupists, ["loupist-b"]);
    assert_eq!(current.quarantined_generations, ["gen_locked"]);
    assert!(current.paused);
    let store = crate::store::SqliteStore::open_in_memory().unwrap();
    let unbounded = GraphProcessingSettings {
        max_llm_calls: 65,
        ..Default::default()
    };
    assert!(store_settings(&store.lock().unwrap(), "loupist-a", &unbounded).is_err());
}

#[test]
fn graph_jobs_require_the_loupists_own_world_revision() {
    let (_directory, store) = open();
    let settings = enabled();
    let (world_id, world_revision) = world(&store, "loupist-a");
    let source = source_module(&store, "loupist-a");
    let (span, source_revision) = retain(&store, "loupist-a", &source, None, b"owned note");
    let request = |loupist: &str, world_record_id: &str| AddToWorld {
        loupist_id: loupist.to_string(),
        world_record_id: world_record_id.to_string(),
        world_revision_id: world_revision.clone(),
        source_record_id: source.clone(),
        source_revision_id: source_revision.clone(),
        span_id: span.clone(),
        at_ms: T,
        binding_role: "extracted_from".to_string(),
    };
    let lens = store
        .file_method(
            "loupist-a",
            "Lens",
            "A method body.",
            &crate::knowledge::test_support::user_origin(),
            10,
        )
        .unwrap();
    let wrong_kind = store.graph_accept_episode(&settings, request("loupist-a", &lens.record_id));
    assert_eq!(wrong_kind.unwrap_err().code(), "invalid");
    let other_loupist = store.graph_accept_episode(&settings, request("loupist-b", &world_id));
    assert_eq!(other_loupist.unwrap_err().code(), "unauthorized");
    store
        .transact(|write| {
            require_world_read(write.tx, "loupist-a", &world_id, &world_revision)?;
            assert!(matches!(
                require_world_read(write.tx, "loupist-a", &world_id, &source_revision),
                Err(KnowledgeError::Unauthorized(_))
            ));
            Ok(())
        })
        .unwrap();
    assert!(store.graph_jobs("loupist-a").unwrap().is_empty());
}

#[test]
fn underfunded_source_chunks_wait_without_attempts_or_blocking_other_sources() {
    let (_directory, store) = open();
    let mut settings = enabled();
    let loupist = "loupist-a";
    let first = queued(
        &store,
        &settings,
        loupist,
        "A".repeat(EPISODE_CHUNK_CHARS * 17).as_bytes(),
    );
    let frozen = store.graph_jobs(loupist).unwrap();
    assert!(frozen
        .iter()
        .all(|job| job.document.extraction.max_llm_calls < 2));
    let short_source = source_module(&store, loupist);
    let (short_span, short_revision) = retain(
        &store,
        loupist,
        &short_source,
        None,
        b"Ada built the engine.",
    );
    let short = add(
        &store,
        &settings,
        loupist,
        &first.world_record_id,
        &first.world_revision_id,
        &short_source,
        &short_revision,
        &short_span,
    );
    assert_eq!(
        claim(&store, &settings, &first.job_id, T + 100).status,
        JOB_PAUSED
    );
    for old in &frozen {
        let current = store.graph_job(loupist, &old.job_id).unwrap();
        assert_eq!(current.status, JOB_PAUSED);
        assert_eq!(current.document, old.document);
    }
    assert_eq!(heads(&store), std::slice::from_ref(&short.job_id));
    store
        .transact(|write| resume_processing(write.tx, &mut settings, loupist))
        .unwrap();
    assert_eq!(
        claim(&store, &settings, &first.job_id, T + 200).status,
        JOB_PAUSED
    );
    assert_eq!(
        claim(&store, &settings, &short.job_id, T + 300).status,
        JOB_RUNNING
    );
}

#[test]
fn a_long_source_becomes_linked_chunks_that_run_in_order() {
    let (_directory, store) = open();
    let mut settings = enabled();
    let loupist = "loupist-a";
    let text = format!("{}\n\n{}", "A".repeat(7_000), "Tail of the note.");
    let first = queued(&store, &settings, loupist, text.as_bytes());
    let jobs = store.graph_jobs(loupist).unwrap();
    assert_eq!(jobs.len(), 2);
    let second = jobs.iter().find(|job| job.job_id != first.job_id).unwrap();
    assert_eq!(
        second.document.previous_episode_key.as_deref(),
        Some(first.document.episode_key.as_str())
    );
    assert!(second.document.body.contains("Tail"));
    assert_eq!(heads(&store), std::slice::from_ref(&first.job_id));
    let running = claim(&store, &settings, &first.job_id, T + 100);
    assert!(heads(&store).is_empty());
    let artifact = r#"{"nodes":[],"claims":[]}"#;
    store
        .graph_publish_and_complete(
            &mut settings,
            &first.job_id,
            running.document.attempt_id.as_deref().unwrap(),
            artifact,
            &sha256(artifact.as_bytes()),
            T + 200,
            false,
        )
        .unwrap();
    assert_eq!(heads(&store), std::slice::from_ref(&second.job_id));
}

#[test]
fn projection_normalizes_relation_and_caps_evidence() {
    let evidence: Vec<Value> = (0..40)
        .map(|index| {
            json!({
                "span_id": format!("spn_{index}"),
                "role": "extracted_from",
                "attribution": "example-publisher"
            })
        })
        .collect();
    let mut artifact = serde_json::from_str::<Value>(&world_artifact(&["spn_1"])).unwrap();
    let claims = artifact["claims"].as_array_mut().unwrap();
    claims[0]["relation"] = json!("공급한다");
    claims[0]["evidence"] = Value::Array(evidence);
    claims.push(json!({"adapter_id": "not-a-claim"}));
    let (_nodes, projected, _annotations) = project_worker_artifact(&artifact.to_string()).unwrap();
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].relation, "RELATES_TO");
    assert_eq!(projected[0].evidence.len(), 32);
    assert!(projected[0]
        .qualifiers
        .iter()
        .any(|qualifier| qualifier.key == "original_relation" && qualifier.value == "공급한다"));
}

#[test]
fn a_queued_job_survives_reopening_the_database() {
    let (directory, store) = open();
    let job = queued(
        &store,
        &enabled(),
        "loupist-a",
        b"Ada described the engine.",
    );
    assert_eq!(job.document.body, "Ada described the engine.");
    assert!(job.document.publication_time.is_none());
    drop(store);
    let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
    assert_eq!(store.graph_job("loupist-a", &job.job_id).unwrap(), job);
}

#[test]
fn pause_and_resume_restore_the_queue_without_resetting_the_job_budget() {
    let (_directory, store) = open();
    let mut settings = enabled();
    let job = queued(&store, &settings, "loupist-a", b"paused note");
    let pause = |settings: &mut GraphProcessingSettings| {
        store.transact(|write| pause_processing(write.tx, settings, "loupist-a"))
    };
    let resume = |settings: &mut GraphProcessingSettings| {
        store.transact(|write| resume_processing(write.tx, settings, "loupist-a"))
    };
    assert_eq!(pause(&mut settings).unwrap(), 1);
    assert!(heads(&store).is_empty());
    let still = claim(&store, &settings, &job.job_id, T);
    assert_eq!(still.status, JOB_PAUSED);
    assert_eq!(still.document, job.document);

    settings.max_llm_calls = 12;
    settings.max_embedding_calls = 24;
    assert_eq!(resume(&mut settings).unwrap(), 1);
    assert_eq!(heads(&store), std::slice::from_ref(&job.job_id));
    assert_eq!(resume(&mut settings).unwrap(), 0);
    let running = claim(&store, &settings, &job.job_id, T + 100);
    assert_eq!(running.status, JOB_RUNNING);
    assert_eq!(running.document.extraction, job.document.extraction);
    assert_eq!(
        running.document.lease_expires_at_ms,
        Some(T + 100 + attempt_budget_ms(job.document.extraction.timeout_ms) as i64)
    );
}

#[test]
fn resume_does_not_redrive_attempted_terminal_or_other_loupist_jobs() {
    let (_directory, store) = open();
    let mut settings = enabled();
    let job = queued(&store, &settings, "loupist-a", b"resume note");
    store
        .transact(|write| pause_processing(write.tx, &mut settings, "loupist-a"))
        .unwrap();
    type ResumeCase = (&'static str, &'static str, fn(&mut JobDocument));
    let cases: &[ResumeCase] = &[
        ("inference", JOB_PAUSED, |doc| {
            doc.inference_attempted = true
        }),
        ("uncertain", JOB_PAUSED, |doc| doc.usage_uncertain = true),
        ("attempt", JOB_PAUSED, |doc| {
            doc.attempt_id = Some(mint_id("atm"))
        }),
        ("lease", JOB_PAUSED, |doc| {
            doc.lease_expires_at_ms = Some(T + 100)
        }),
        ("quarantined", JOB_PAUSED, |doc| {
            doc.generation_id = "gen_locked".into()
        }),
        ("cancelled", JOB_CANCELLED, |_| {}),
        ("reconcile", JOB_RECONCILE, |_| {}),
        ("failed", JOB_FAILED_BEFORE, |_| {}),
        ("completed", JOB_COMPLETED, |_| {}),
        ("running", JOB_RUNNING, |_| {}),
    ];
    let seed =
        |loupist: &str, world: &str, revision: &str, document: &JobDocument, status: &str| {
            store
                .transact(|write| {
                    let inserted = insert_job(
                        write.tx,
                        loupist,
                        world,
                        revision,
                        &document.dedup_key,
                        job.created_at_ms,
                        document,
                    )?;
                    write_status(write.tx, &inserted.job_id, JOB_QUEUED, status, document)?;
                    load_job(write.tx, &inserted.job_id)
                })
                .unwrap()
        };
    let mut preserved = Vec::new();
    for (name, status, edit) in cases {
        let mut document = job.document.clone();
        document.dedup_key.push_str(name);
        edit(&mut document);
        preserved.push(seed(
            "loupist-a",
            &job.world_record_id,
            &job.world_revision_id,
            &document,
            status,
        ));
    }
    let (other_world, other_revision) = world(&store, "loupist-b");
    preserved.push(seed(
        "loupist-b",
        &other_world,
        &other_revision,
        &job.document,
        JOB_PAUSED,
    ));
    settings.quarantined_generations.push("gen_locked".into());
    assert_eq!(
        store
            .transact(|write| resume_processing(write.tx, &mut settings, "loupist-a"))
            .unwrap(),
        1
    );
    assert_eq!(
        store.graph_job("loupist-a", &job.job_id).unwrap().status,
        JOB_QUEUED
    );
    for expected in preserved {
        assert_eq!(
            store
                .graph_job(&expected.loupist_id, &expected.job_id)
                .unwrap(),
            expected
        );
    }
}

#[test]
fn publication_cites_one_current_revision_per_source() {
    let (_directory, store) = open();
    let mut settings = enabled();
    let loupist = "loupist-a";
    let source = source_module(&store, loupist);
    let (span_one, source_revision) =
        retain(&store, loupist, &source, None, b"Ada directed the engine.");
    let (world_module, revision) = world(&store, loupist);
    let job = add(
        &store,
        &settings,
        loupist,
        &world_module,
        &revision,
        &source,
        &source_revision,
        &span_one,
    );
    // The source changes after the job froze its input.
    let (span_two, source_head) = retain(
        &store,
        loupist,
        &source,
        Some(source_revision),
        b"Ada left the engine.",
    );
    let published = publish(
        &store,
        &mut settings,
        &job,
        &world_artifact(&[&span_one, &span_two]),
        T + 100,
    )
    .unwrap();
    let sealed = store
        .get_revision(
            loupist,
            published.document.sealed_revision_id.as_deref().unwrap(),
        )
        .unwrap();
    let head_digest = store
        .get_revision(loupist, &source_head)
        .unwrap()
        .revision_digest;
    assert_eq!(sealed.dependencies.len(), 1);
    assert_eq!(sealed.dependencies[0].revision_digest, head_digest);
    let text = sealed.payload.to_string();
    assert!(
        text.contains(&span_one) && text.contains(&span_two),
        "{text}"
    );
}

#[test]
fn a_stale_generation_does_not_replace_a_newer_world_head() {
    let (_directory, store) = open();
    let settings = enabled();
    let loupist = "loupist-a";
    let job = queued(&store, &settings, loupist, b"stale note");
    let running = claim(&store, &settings, &job.job_id, T + 50);
    let newer = store
        .seal_world(crate::knowledge::SealWorld {
            loupist_id: loupist.to_string(),
            record_id: job.world_record_id.clone(),
            expected_revision_id: Some(job.world_revision_id.clone()),
            at_ms: T + 100,
            nodes: Vec::new(),
            claims: Vec::new(),
            annotations: Vec::new(),
            dependencies: Vec::new(),
        })
        .unwrap();
    let artifact = "{}";
    let published = store.graph_publish_and_complete(
        &mut settings.clone(),
        &job.job_id,
        running.document.attempt_id.as_deref().unwrap(),
        artifact,
        &sha256(artifact.as_bytes()),
        T + 200,
        false,
    );
    assert_eq!(published.unwrap().status, JOB_RECONCILE);
    assert_eq!(
        head(&store, loupist, &job.world_record_id),
        newer.revision_id
    );
}

#[test]
fn unattempted_sources_rebase_to_the_latest_world_and_run_one_at_a_time() {
    let (_directory, store) = open();
    let mut settings = enabled();
    let loupist = "loupist-a";
    let source = source_module(&store, loupist);
    let (span, source_revision) = retain(&store, loupist, &source, None, b"first report");
    let (world_module, revision) = world(&store, loupist);
    let first = add(
        &store,
        &settings,
        loupist,
        &world_module,
        &revision,
        &source,
        &source_revision,
        &span,
    );
    let (span_two, source_two) = retain(
        &store,
        loupist,
        &source,
        Some(source_revision),
        b"second report",
    );
    let second = add(
        &store,
        &settings,
        loupist,
        &world_module,
        &revision,
        &source,
        &source_two,
        &span_two,
    );
    let first_running = claim(&store, &settings, &first.job_id, T + 100);
    assert_eq!(first_running.status, JOB_RUNNING);
    assert_eq!(
        claim(&store, &settings, &second.job_id, T + 101).status,
        JOB_QUEUED
    );
    let artifact = r#"{"nodes":[],"claims":[]}"#;
    store
        .graph_publish_and_complete(
            &mut settings,
            &first.job_id,
            first_running.document.attempt_id.as_deref().unwrap(),
            artifact,
            &sha256(artifact.as_bytes()),
            T + 200,
            false,
        )
        .unwrap();
    let latest = head(&store, loupist, &world_module);
    assert_ne!(latest, revision);
    let second_running = claim(&store, &settings, &second.job_id, T + 300);
    assert_eq!(second_running.status, JOB_RUNNING);
    assert_eq!(second_running.world_revision_id, latest);
    assert_eq!(
        second_running.document.sealed_revision_id.as_deref(),
        Some(latest.as_str())
    );
}

#[test]
fn enabling_processing_backfills_saved_sources_and_deduplicates_paused_work() {
    let (_directory, store) = open();
    let loupist = "loupist-a";
    let source = source_module(&store, loupist);
    retain(
        &store,
        loupist,
        &source,
        None,
        b"Stored before a helper existed",
    );
    let backfill = |loupist: &str, settings: &GraphProcessingSettings, at: i64| {
        store
            .transact(|write| write.enqueue_existing_sources(loupist, settings, at))
            .unwrap()
    };
    assert!(backfill(loupist, &GraphProcessingSettings::default(), 20).is_empty());
    let settings = GraphProcessingSettings {
        paused: true,
        ..enabled()
    };
    let jobs = backfill(loupist, &settings, 30);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].status, JOB_PAUSED);
    assert!(backfill(loupist, &settings, 40).is_empty());
    assert_eq!(store.graph_jobs(loupist).unwrap().len(), 1);
    assert!(backfill("loupist-b", &settings, 50).is_empty());
}

#[test]
fn automatic_world_processing_keeps_pdf_bytes_and_uses_only_its_extracted_text() {
    let (_directory, store) = open();
    let loupist = "loupist-a";
    let source = source_module(&store, loupist);
    let bytes = b"%PDF-1.7 ASCII file bytes are not extracted document text";
    let mut request = RetainSource {
        at_ms: 10,
        title: "Original PDF".into(),
        source_class: SourceClass::UserFile,
        origin_locator: "fixture:original.pdf".into(),
        provider_identity: "local-file".into(),
        authorship: Authorship::User,
        ..source_request(loupist, &source, bytes)
    };
    if let Some(material) = request.material.as_mut() {
        material.media_type = "application/pdf".into();
    }
    let original = store.retain_source(request.clone()).unwrap();
    assert!(store
        .transact(|write| write.enqueue_source_world(loupist, &original, &enabled(), 20))
        .unwrap()
        .is_none());
    assert!(store.graph_jobs(loupist).unwrap().is_empty());
    let preserved = store
        .transact(|write| {
            load_revision_span(
                write.tx,
                write.blobs,
                loupist,
                &source,
                &original.revision_id,
                original.span_id.as_deref().unwrap(),
            )
        })
        .unwrap();
    assert_eq!(preserved.bytes, bytes);

    let text = b"The report says Ada described the engine.";
    request.expected_revision_id = Some(original.revision_id);
    request.source_id = Some(original.source_id);
    request.at_ms = 30;
    request.material = Some(RetainMaterial {
        media_type: "text/plain".into(),
        role: super::super::RepresentationRole::ExtractedText,
        document_id: "pdf-extracted".into(),
        parser_version: Some("fixture-parser-1".into()),
        parent_representation_id: original.representation_id,
        input_span_ids: vec![original.span_id.unwrap()],
        bytes: text.to_vec(),
        selector: Selector::ByteRange {
            start: 0,
            end: text.len() as u64,
        },
        context_note: "Extracted text of original PDF".into(),
    });
    let extracted = store.retain_source(request).unwrap();
    let job = store
        .transact(|write| write.enqueue_source_world(loupist, &extracted, &enabled(), 40))
        .unwrap()
        .unwrap();
    assert_eq!(job.document.body.as_bytes(), text);
    assert_eq!(
        job.document.source_binding.unwrap().span_id,
        extracted.span_id.unwrap()
    );
    assert_eq!(store.graph_jobs(loupist).unwrap().len(), 1);
}

#[test]
fn sealed_evidence_without_jobs_is_not_reextracted_but_unused_capture_is_queued() {
    let (_directory, store) = open();
    let loupist = "loupist-a";
    let source = source_module(&store, loupist);
    let (used, first) = retain(&store, loupist, &source, None, b"Ada described the engine.");
    let (unused, latest) = retain(
        &store,
        loupist,
        &source,
        Some(first),
        b"Another capture has not been extracted.",
    );
    let (world_id, world_revision) = world(&store, loupist);
    let (nodes, claims, annotations) = project_worker_artifact(&world_artifact(&[&used])).unwrap();
    let source_revision = store
        .transact(|write| super::super::revision::load_revision(write.tx, &latest))
        .unwrap();
    store
        .seal_world(super::super::SealWorld {
            loupist_id: loupist.into(),
            record_id: world_id,
            expected_revision_id: Some(world_revision),
            at_ms: T + 10,
            nodes,
            claims,
            annotations,
            dependencies: vec![super::super::EvidenceRevision {
                record_id: source,
                revision_digest: source_revision.revision_digest,
                role: super::super::DependencyRole::WorldSource,
            }],
        })
        .unwrap();
    let other_source = source_module(&store, loupist);
    let (other_unused, _) = retain(
        &store,
        loupist,
        &other_source,
        None,
        b"A separate source also needs extraction.",
    );
    let backfill = |at: i64| {
        store
            .transact(|write| write.enqueue_existing_sources(loupist, &enabled(), at))
            .unwrap()
    };
    let spans: std::collections::BTreeSet<_> = backfill(T + 20)
        .into_iter()
        .map(|job| job.document.source_binding.unwrap().span_id)
        .collect();
    assert_eq!(
        spans,
        std::collections::BTreeSet::from([unused, other_unused])
    );
    assert!(backfill(T + 30).is_empty());
}

#[test]
fn processed_chat_source_stamps_world_growth_and_a_map_source_does_not() {
    let loupist = "loupist-origin";
    let world_growth = |store: &KnowledgeStore| -> Vec<crate::knowledge::GrowthEvent> {
        store
            .growth_events(&crate::knowledge::GrowthEventsQuery {
                loupist_id: loupist.into(),
                since_ms: None,
                until_ms: None,
                limit: Some(100),
                cursor: None,
            })
            .unwrap()
            .events
            .into_iter()
            .filter(|event| event.kind == "entity_added" || event.kind == "claim_added")
            .collect()
    };

    let (_turn_dir, turn_store) = open();
    let mut settings = enabled();
    let saved = turn_store
        .transact(|write| {
            write.retain_new_source_with_origin(
                crate::knowledge::RetainNewSource {
                    origin: None,
                    loupist_id: loupist.into(),
                    at_ms: T,
                    title: "Turn note".into(),
                    source_class: SourceClass::UserFile,
                    origin_locator: "https://example.test/turn".into(),
                    provider_identity: "user".into(),
                    authorship: Authorship::User,
                    coverage: Coverage::IncludedFull,
                    origin_chat_id: Some("chat-a".into()),
                    request_ref: None,
                    declared_digest: None,
                    publication_time: None,
                    material: source_request(loupist, "", b"Ada directed the engine.").material,
                },
                "message-a",
            )
        })
        .unwrap();
    let (world_id, revision) = world(&turn_store, loupist);
    let span = saved.span_id.clone().expect("span");
    let job = add(
        &turn_store,
        &settings,
        loupist,
        &world_id,
        &revision,
        &saved.record_id,
        &saved.revision_id,
        &span,
    );
    assert_eq!(job.document.origin_chat_id.as_deref(), Some("chat-a"));
    assert_eq!(job.document.origin_message_id.as_deref(), Some("message-a"));
    publish(
        &turn_store,
        &mut settings,
        &job,
        &world_artifact(&[&span]),
        T + 100,
    )
    .unwrap();
    let events = world_growth(&turn_store);
    assert!(events.iter().any(|event| event.kind == "entity_added"));
    assert!(events.iter().any(|event| event.kind == "claim_added"));
    assert!(events.iter().all(|event| {
        event.actor == "processing"
            && event.origin_chat_id.as_deref() == Some("chat-a")
            && event.origin_message_id.as_deref() == Some("message-a")
    }));

    let (_map_dir, map_store) = open();
    let mut settings = enabled();
    let source = source_module(&map_store, loupist);
    let (span, source_revision) =
        retain(&map_store, loupist, &source, None, b"Ada left the engine.");
    let (world_id, revision) = world(&map_store, loupist);
    let job = add(
        &map_store,
        &settings,
        loupist,
        &world_id,
        &revision,
        &source,
        &source_revision,
        &span,
    );
    assert!(job.document.origin_chat_id.is_none());
    publish(
        &map_store,
        &mut settings,
        &job,
        &world_artifact(&[&span]),
        T + 100,
    )
    .unwrap();
    let events = world_growth(&map_store);
    assert!(!events.is_empty());
    assert!(events
        .iter()
        .all(|event| event.origin_chat_id.is_none() && event.origin_message_id.is_none()));
}

#[test]
fn a_chunk_runs_once_its_earlier_chunk_can_no_longer_finish() {
    let (_directory, store) = open();
    let settings = enabled();
    let loupist = "loupist-a";
    let text = format!("{}\n\n{}", "A".repeat(7_000), "Tail of the note.");
    let first = queued(&store, &settings, loupist, text.as_bytes());
    let second = store
        .graph_jobs(loupist)
        .unwrap()
        .into_iter()
        .find(|job| job.job_id != first.job_id)
        .unwrap();
    assert_eq!(heads(&store), std::slice::from_ref(&first.job_id));
    store
        .transact(|write| {
            assert!(write_status(
                write.tx,
                &first.job_id,
                JOB_QUEUED,
                JOB_CANCELLED,
                &first.document
            )?);
            Ok(())
        })
        .unwrap();
    // The earlier chunk is only context; without it the rest of the Source still reaches the World.
    assert_eq!(heads(&store), std::slice::from_ref(&second.job_id));
}

/// Claim and record a paid attempt; returns its attempt id.
fn paid_attempt(
    store: &KnowledgeStore,
    settings: &GraphProcessingSettings,
    job_id: &str,
    at_ms: i64,
) -> String {
    let running = claim(store, settings, job_id, at_ms);
    let attempt = running.document.attempt_id.clone().unwrap();
    store.graph_note_inference(job_id, &attempt).unwrap();
    attempt
}

#[test]
fn sleep_through_an_attempt_runs_it_again_before_the_one_retry() {
    let (_directory, store) = open();
    let mut settings = enabled();
    let job = queued(&store, &settings, "loupist-a", b"sleepy note");
    let cut = Unpublished {
        uncertain: true,
        paid: true,
        suspended: true,
    };
    for expected in 1..=HOST_INTERRUPTION_RETRIES {
        let attempt = paid_attempt(&store, &settings, &job.job_id, T);
        let view = store
            .graph_finish_unpublished(&mut settings, &job.job_id, &attempt, cut, T)
            .unwrap();
        assert_eq!(view.status, JOB_QUEUED);
        assert_eq!(view.document.host_interruptions, expected);
        assert_eq!(view.document.interrupted_attempts, 0);
        assert!(!view.document.inference_attempted && !view.document.usage_uncertain);
    }
    // The host allowance is spent: the one retry, then quarantine.
    let attempt = paid_attempt(&store, &settings, &job.job_id, T);
    let view = store
        .graph_finish_unpublished(&mut settings, &job.job_id, &attempt, cut, T)
        .unwrap();
    assert_eq!(
        (view.status.as_str(), view.document.interrupted_attempts),
        (JOB_QUEUED, 1)
    );
    let attempt = paid_attempt(&store, &settings, &job.job_id, T);
    let view = store
        .graph_finish_unpublished(&mut settings, &job.job_id, &attempt, cut, T)
        .unwrap();
    assert_eq!(view.status, JOB_RECONCILE);
    assert!(settings
        .quarantined_generations
        .contains(&view.document.generation_id));
}

#[test]
fn a_failure_while_awake_keeps_the_one_retry_rule() {
    let (_directory, store) = open();
    let mut settings = enabled();
    let job = queued(&store, &settings, "loupist-a", b"awake note");
    let failed = Unpublished {
        uncertain: true,
        paid: true,
        suspended: false,
    };
    let attempt = paid_attempt(&store, &settings, &job.job_id, T);
    let view = store
        .graph_finish_unpublished(&mut settings, &job.job_id, &attempt, failed, T)
        .unwrap();
    assert_eq!(
        (view.status.as_str(), view.document.interrupted_attempts),
        (JOB_QUEUED, 1)
    );
    assert_eq!(view.document.host_interruptions, 0);
    let attempt = paid_attempt(&store, &settings, &job.job_id, T);
    let view = store
        .graph_finish_unpublished(&mut settings, &job.job_id, &attempt, failed, T)
        .unwrap();
    assert_eq!(view.status, JOB_RECONCILE);
}

#[test]
fn a_paused_queue_keeps_an_unsent_attempt_paused() {
    let (_directory, store) = open();
    let mut settings = enabled();
    let job = queued(&store, &settings, "loupist-a", b"unsent note");
    let attempt = paid_attempt(&store, &settings, &job.job_id, T);
    settings.paused = true;
    let view = store
        .graph_finish_unpublished(
            &mut settings,
            &job.job_id,
            &attempt,
            Unpublished::default(),
            T,
        )
        .unwrap();
    assert_eq!(view.status, JOB_PAUSED);
    assert_eq!(view.document.unsent_attempts, 1);
    assert!(!view.document.inference_attempted);
}

#[test]
fn the_old_default_request_limit_reads_as_the_new_one_and_the_cap_is_five_minutes() {
    let (_directory, store) = open();
    let mut settings = enabled();
    settings.timeout_ms = 60_000;
    // Stored as the earlier default; validation runs on the value read.
    store
        .transact(|write| {
            write.tx.execute(
                "INSERT INTO app_settings (id, data_json) VALUES ('world_processing:loupist-a', ?1)",
                [serde_json::to_string(&settings).unwrap()],
            )?;
            Ok(())
        })
        .unwrap();
    let loaded = store
        .read(|read| load_settings(read.tx, "loupist-a"))
        .unwrap();
    assert_eq!(loaded.timeout_ms, DEFAULT_TIMEOUT_MS);
    settings.timeout_ms = MAX_TIMEOUT_MS;
    assert!(settings.validate().is_ok());
    settings.timeout_ms = MAX_TIMEOUT_MS + 1;
    assert!(settings.validate().is_err());
    // An attempt allows every model wave its full limit.
    assert!(attempt_budget_ms(MAX_TIMEOUT_MS) >= 5 * MAX_TIMEOUT_MS);
    assert_eq!(attempt_budget_ms(1_000), LOCAL_OPERATION_BUDGET_MS);
}

#[test]
fn a_queued_job_runs_under_the_saved_request_limit() {
    let (_directory, store) = open();
    let mut settings = enabled();
    settings.timeout_ms = 30_000;
    let job = queued(&store, &settings, "loupist-a", b"limit note");
    assert_eq!(job.document.extraction.timeout_ms, 30_000);
    settings.timeout_ms = 200_000;
    let running = claim(&store, &settings, &job.job_id, T);
    assert_eq!(running.document.extraction.timeout_ms, 200_000);
    assert_eq!(
        running.document.extraction.max_llm_calls,
        job.document.extraction.max_llm_calls
    );
}

#[test]
fn sources_the_current_chat_saved_come_before_an_older_backlog() {
    let (_directory, store) = open();
    let settings = enabled();
    let loupist = "loupist-a";
    let source = source_module(&store, loupist);
    let (world_module, revision) = world(&store, loupist);
    let mut jobs = Vec::new();
    let mut previous = None;
    for body in [b"backlog one".as_slice(), b"backlog two", b"chat source"] {
        let (span, source_revision) = retain(&store, loupist, &source, previous.clone(), body);
        jobs.push(add(
            &store,
            &settings,
            loupist,
            &world_module,
            &revision,
            &source,
            &source_revision,
            &span,
        ));
        previous = Some(source_revision);
    }
    store
        .transact(|write| {
            for (index, job) in jobs.iter().enumerate() {
                write.tx.execute(
                    "UPDATE knowledge_graph_outbox SET created_at_ms = ?1 WHERE job_id = ?2",
                    params![T + index as i64, job.job_id],
                )?;
            }
            Ok(())
        })
        .unwrap();
    // The newest Source came from the Chat the user is in.
    let mut chat = jobs[2].clone();
    chat.document.origin_chat_id = Some("chat-now".into());
    store
        .transact(|write| {
            assert!(write_status(
                write.tx,
                &chat.job_id,
                JOB_QUEUED,
                JOB_QUEUED,
                &chat.document
            )?);
            Ok(())
        })
        .unwrap();
    let head_with = |hot: &[String]| {
        store
            .read(|read| queued_loupist_heads(read.tx, hot))
            .unwrap()
            .into_iter()
            .map(|job| job.job_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(head_with(&[]), [jobs[0].job_id.clone()]);
    assert_eq!(head_with(&["chat-now".to_string()]), [chat.job_id.clone()]);
    assert_eq!(
        head_with(&["chat-other".to_string()]),
        [jobs[0].job_id.clone()]
    );
}

#[test]
fn queued_sources_count_as_awaiting_the_world_until_their_job_settles() {
    let (_directory, store) = open();
    let mut settings = enabled();
    let job = queued(&store, &settings, "loupist-a", b"pending note");
    let source = job
        .document
        .source_binding
        .clone()
        .unwrap()
        .source_record_id;
    let awaiting = || {
        store
            .read(|read| sources_awaiting_world(read.tx, "loupist-a"))
            .unwrap()
    };
    assert!(awaiting().contains(&source));
    assert_eq!(store.read(|read| backlog(read.tx)).unwrap(), 1);
    let attempt = paid_attempt(&store, &settings, &job.job_id, T);
    assert!(awaiting().contains(&source), "a running job still awaits");
    store
        .graph_finish(
            &mut settings,
            &job.job_id,
            &attempt,
            JOB_FAILED_BEFORE,
            false,
            false,
            T,
        )
        .unwrap();
    // Its one retry, then quarantine: settled either way.
    let again = paid_attempt(&store, &settings, &job.job_id, T);
    store
        .graph_finish(
            &mut settings,
            &job.job_id,
            &again,
            JOB_FAILED_BEFORE,
            false,
            false,
            T,
        )
        .unwrap();
    assert!(
        !awaiting().contains(&source),
        "a settled job no longer awaits"
    );
    assert_eq!(store.read(|read| backlog(read.tx)).unwrap(), 0);
}
