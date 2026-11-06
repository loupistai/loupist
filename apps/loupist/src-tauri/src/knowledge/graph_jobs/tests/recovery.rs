use super::*;

const AT: i64 = 1_700_000_000_000;

fn queued_pair(store: &KnowledgeStore, loupist: &str) -> (GraphJobView, GraphJobView) {
    let settings = enabled();
    store
        .transact(|write| store_settings(write.tx, loupist, &settings))
        .unwrap();
    let source = source_module(store, loupist);
    let (span_a, source_a) = retain(store, loupist, &source, None, b"First retained source");
    let (span_b, source_b) = retain(
        store,
        loupist,
        &source,
        Some(source_a.clone()),
        b"Second retained source",
    );
    let (world, revision) = world(store, loupist);
    (
        add(
            store, &settings, loupist, &world, &revision, &source, &source_b, &span_a,
        ),
        add(
            store, &settings, loupist, &world, &revision, &source, &source_b, &span_b,
        ),
    )
}

fn interrupt_paid(store: &KnowledgeStore, job: &GraphJobView) -> GraphJobView {
    let mut settings = enabled();
    let running = claim(store, &settings, &job.job_id, AT);
    let attempt = running.document.attempt_id.as_deref().unwrap();
    store.graph_note_inference(&job.job_id, attempt).unwrap();
    let interrupted = store
        .graph_finish(
            &mut settings,
            &job.job_id,
            attempt,
            JOB_RECONCILE,
            true,
            false,
        )
        .unwrap();
    store
        .transact(|write| store_settings(write.tx, &job.loupist_id, &settings))
        .unwrap();
    interrupted
}

/// Use up the one retry so the generation is quarantined.
fn quarantine_by_interruptions(store: &KnowledgeStore, job: &GraphJobView) -> GraphJobView {
    assert_eq!(interrupt_paid(store, job).status, JOB_QUEUED);
    let interrupted = interrupt_paid(store, job);
    assert_eq!(interrupted.status, JOB_RECONCILE);
    interrupted
}

fn recover(store: &KnowledgeStore, loupist: &str, at_ms: i64) -> GraphJobView {
    store
        .transact(|write| recover_world(write.tx, write.blobs, loupist, at_ms))
        .unwrap()
}

fn settings_of(store: &KnowledgeStore, loupist: &str) -> GraphProcessingSettings {
    store
        .read(|write| load_settings(write.tx, loupist))
        .unwrap()
}

fn finish_recovery(store: &KnowledgeStore, job: &GraphJobView) -> KnowledgeResult<GraphJobView> {
    let mut settings = settings_of(store, &job.loupist_id);
    let running = claim(store, &settings, &job.job_id, AT + 100);
    store.transact(|write| {
        complete_reindex(
            write,
            &mut settings,
            job,
            running.document.attempt_id.as_deref().unwrap(),
            AT + 200,
        )
    })
}

fn status(store: &KnowledgeStore, job: &GraphJobView) -> String {
    store
        .graph_job(&job.loupist_id, &job.job_id)
        .unwrap()
        .status
}

#[test]
fn startup_recovers_future_leases_and_persists_each_loupists_pause_and_quarantine() {
    let (_dir, store) = open();
    let mut cases = Vec::new();
    for (loupist, paused, uncertain) in [
        ("loupist-safe", false, false),
        ("loupist-paused", true, false),
        ("loupist-uncertain", false, true),
    ] {
        let (job, _) = queued_pair(&store, loupist);
        let mut document = claim(&store, &enabled(), &job.job_id, AT).document;
        document.lease_expires_at_ms = Some(9_000_000_000_000);
        document.inference_attempted = uncertain;
        document.usage_uncertain = uncertain;
        // Its one retry is already spent, so the uncertain call quarantines.
        document.interrupted_attempts = u8::from(uncertain);
        let settings = GraphProcessingSettings {
            paused,
            ..enabled()
        };
        store
            .transact(|write| {
                store_settings(write.tx, loupist, &settings)?;
                assert!(write_status(
                    write.tx,
                    &job.job_id,
                    JOB_RUNNING,
                    JOB_RUNNING,
                    &document
                )?);
                Ok(())
            })
            .unwrap();
        cases.push((job, paused, uncertain));
    }
    let recover_all = || {
        store
            .transact(|write| recover_interrupted(write.tx))
            .unwrap()
    };
    assert_eq!(recover_all(), 3);
    assert_eq!(recover_all(), 0);
    for (job, paused, uncertain) in cases {
        let recovered = store.graph_job(&job.loupist_id, &job.job_id).unwrap();
        let expected = if uncertain {
            JOB_RECONCILE
        } else if paused {
            JOB_PAUSED
        } else {
            JOB_QUEUED
        };
        assert_eq!(recovered.status, expected);
        assert!(recovered.document.attempt_id.is_none());
        assert!(recovered.document.lease_expires_at_ms.is_none());
        let saved = settings_of(&store, &job.loupist_id);
        assert_eq!(saved.paused, paused);
        assert_eq!(
            saved
                .quarantined_generations
                .contains(&job.document.generation_id),
            uncertain
        );
    }
    let candidates = store.read(|write| queued_loupist_heads(write.tx)).unwrap();
    assert!(!candidates
        .iter()
        .any(|job| job.loupist_id == "loupist-uncertain"));
    let safe = candidates
        .iter()
        .find(|job| job.loupist_id == "loupist-safe")
        .unwrap();
    assert_eq!(
        claim(&store, &enabled(), &safe.job_id, AT + 1).status,
        JOB_RUNNING
    );
}

#[test]
fn empty_world_recovery_retires_uncertain_work_without_paid_replay_and_unblocks_queue() {
    let (_dir, store) = open();
    let loupist = "loupist-recover";
    let (first, second) = queued_pair(&store, loupist);
    let status_of = |loupist: &str| {
        store
            .read(|write| recovery_status(write.tx, loupist))
            .unwrap()
    };
    assert_eq!(status_of(loupist), None);
    let interrupted = quarantine_by_interruptions(&store, &first);
    assert_eq!(status_of(loupist), Some("reconciliation_required"));
    assert_eq!(status_of("loupist-other"), None);
    assert!(heads(&store).is_empty());
    let job = recover(&store, loupist, AT + 1);
    assert_eq!(status_of(loupist), Some("recovering"));
    assert_ne!(job.document.generation_id, first.document.generation_id);
    assert_eq!(job.document.extraction.max_llm_calls, 0);
    assert_eq!(heads(&store), std::slice::from_ref(&job.job_id));
    assert_eq!(recover(&store, loupist, AT + 2).job_id, job.job_id);
    assert_eq!(finish_recovery(&store, &job).unwrap().status, JOB_COMPLETED);
    assert_eq!(status_of(loupist), None);
    let retired = store.graph_job(loupist, &first.job_id).unwrap();
    assert_eq!(retired.status, JOB_RECONCILED);
    assert_eq!(retired.document, interrupted.document);
    let rebased = store.graph_job(loupist, &second.job_id).unwrap();
    assert_eq!(rebased.status, JOB_QUEUED);
    assert_eq!(rebased.document.generation_id, job.document.generation_id);
    assert_eq!(
        head(&store, loupist, &first.world_record_id),
        first.world_revision_id
    );
    let settings = settings_of(&store, loupist);
    assert!(settings.quarantined_generations.is_empty());
    assert!(store
        .transact(|write| write.enqueue_existing_sources(loupist, &settings, AT + 300))
        .unwrap()
        .is_empty());
    assert_eq!(heads(&store), std::slice::from_ref(&second.job_id));
    assert_eq!(
        claim(&store, &settings, &second.job_id, AT + 301).status,
        JOB_RUNNING
    );
}

#[test]
fn failed_recovery_uses_a_new_generation_and_preserves_disabled_paused_preferences() {
    let (_dir, store) = open();
    let loupist = "loupist-retry";
    let (first, _) = queued_pair(&store, loupist);
    quarantine_by_interruptions(&store, &first);
    let settings = GraphProcessingSettings {
        paused: true,
        enrichment_enabled: false,
        helper_provider: None,
        helper_model: None,
        ..settings_of(&store, loupist)
    };
    store
        .transact(|write| store_settings(write.tx, loupist, &settings))
        .unwrap();
    let failed = recover(&store, loupist, AT + 1);
    let running = claim(&store, &settings, &failed.job_id, AT + 2);
    assert_eq!(running.status, JOB_RUNNING);
    store
        .transact(|write| fail_recovery(write.tx, &failed.job_id))
        .unwrap();
    assert_eq!(status(&store, &failed), JOB_FAILED_BEFORE);
    let retry = recover(&store, loupist, AT + 3);
    assert_ne!(retry.document.generation_id, failed.document.generation_id);
    finish_recovery(&store, &retry).unwrap();
    let saved = settings_of(&store, loupist);
    assert!(saved.paused && !saved.enrichment_enabled && saved.helper_model.is_none());
    assert_eq!(status(&store, &first), JOB_RECONCILED);
    // The local rebuild made no inference attempt; its failure stays a failure.
    assert_eq!(status(&store, &failed), JOB_FAILED_BEFORE);
    assert!(heads(&store).is_empty());
}

#[test]
fn moved_world_head_and_stale_attempt_cannot_retire_uncertainty() {
    let (_dir, store) = open();
    let loupist = "loupist-fenced";
    let (first, _) = queued_pair(&store, loupist);
    quarantine_by_interruptions(&store, &first);
    let job = recover(&store, loupist, AT + 1);
    let mut settings = settings_of(&store, loupist);
    let running = claim(&store, &settings, &job.job_id, AT + 2);
    let attempt = running.document.attempt_id.as_deref().unwrap();
    assert!(store
        .transact(|write| complete_reindex(write, &mut settings, &job, "stale_attempt", AT + 3))
        .is_err());
    assert_eq!(status(&store, &first), JOB_RECONCILE);
    let advanced = store
        .seal_world(crate::knowledge::SealWorld {
            loupist_id: loupist.into(),
            record_id: first.world_record_id.clone(),
            expected_revision_id: Some(first.world_revision_id.clone()),
            at_ms: AT + 4,
            nodes: vec![],
            claims: vec![],
            annotations: vec![],
            dependencies: vec![],
        })
        .unwrap();
    assert!(store
        .transact(|write| complete_reindex(write, &mut settings, &job, attempt, AT + 5))
        .is_err());
    assert_eq!(status(&store, &first), JOB_RECONCILE);
    assert_eq!(status(&store, &job), JOB_RUNNING);
    store
        .transact(|write| fail_recovery(write.tx, &job.job_id))
        .unwrap();
    let replacement = recover(&store, loupist, AT + 6);
    assert_eq!(replacement.world_revision_id, advanced.revision_id);
    assert_ne!(
        replacement.document.generation_id,
        job.document.generation_id
    );
}

#[test]
fn a_quarantined_world_rebuilds_without_the_user_and_a_failed_rebuild_waits_for_them() {
    let (_dir, store) = open();
    let loupist = "loupist-unattended";
    let (first, second) = queued_pair(&store, loupist);
    let unattended = |at_ms: i64| {
        store
            .transact(|write| recover_world_unattended(write.tx, write.blobs, loupist, at_ms))
            .unwrap()
    };
    assert!(unattended(AT).is_none(), "nothing needs recovery");
    quarantine_by_interruptions(&store, &first);
    let rebuild = unattended(AT + 1).expect("a quarantined World queues its rebuild");
    assert_eq!(rebuild.document.extraction.max_llm_calls, 0);
    assert!(unattended(AT + 2).is_none(), "one rebuild at a time");
    let settings = settings_of(&store, loupist);
    claim(&store, &settings, &rebuild.job_id, AT + 3);
    store
        .transact(|write| fail_recovery(write.tx, &rebuild.job_id))
        .unwrap();
    assert!(
        unattended(AT + 4).is_none(),
        "a failed rebuild waits for the explicit action"
    );
    let explicit = recover(&store, loupist, AT + 5);
    finish_recovery(&store, &explicit).unwrap();
    assert_eq!(status(&store, &first), JOB_RECONCILED);
    assert_eq!(heads(&store), std::slice::from_ref(&second.job_id));
    quarantine_by_interruptions(&store, &second);
    assert!(
        unattended(AT + 7).is_some(),
        "after a completed rebuild, the next interruption rebuilds unattended again"
    );
}

#[test]
fn an_interrupted_paid_attempt_runs_once_more_and_a_second_quarantines() {
    let (_dir, store) = open();
    let loupist = "loupist-retry-once";
    let (first, _) = queued_pair(&store, loupist);
    let retried = interrupt_paid(&store, &first);
    assert_eq!(retried.status, JOB_QUEUED, "nothing reached the World");
    assert_eq!(retried.document.interrupted_attempts, 1);
    assert!(!retried.document.inference_attempted && !retried.document.usage_uncertain);
    assert!(settings_of(&store, loupist)
        .quarantined_generations
        .is_empty());
    assert_eq!(
        store
            .read(|write| recovery_status(write.tx, loupist))
            .unwrap(),
        None
    );
    assert_eq!(interrupt_paid(&store, &first).status, JOB_RECONCILE);
    assert!(settings_of(&store, loupist)
        .quarantined_generations
        .contains(&first.document.generation_id));
}

#[test]
fn startup_runs_an_interrupted_paid_attempt_once_more() {
    let (_dir, store) = open();
    for (loupist, earlier, expected) in [
        ("loupist-first-cut", 0, JOB_QUEUED),
        ("loupist-second-cut", 1, JOB_RECONCILE),
    ] {
        let (job, _) = queued_pair(&store, loupist);
        let mut document = claim(&store, &enabled(), &job.job_id, AT).document;
        document.inference_attempted = true;
        document.interrupted_attempts = earlier;
        store
            .transact(|write| {
                assert!(write_status(
                    write.tx,
                    &job.job_id,
                    JOB_RUNNING,
                    JOB_RUNNING,
                    &document
                )?);
                Ok(())
            })
            .unwrap();
        store
            .transact(|write| recover_interrupted(write.tx))
            .unwrap();
        let recovered = store.graph_job(loupist, &job.job_id).unwrap();
        assert_eq!(recovered.status, expected, "{loupist}");
        assert_eq!(
            settings_of(&store, loupist)
                .quarantined_generations
                .contains(&job.document.generation_id),
            expected == JOB_RECONCILE,
            "{loupist}"
        );
    }
}
