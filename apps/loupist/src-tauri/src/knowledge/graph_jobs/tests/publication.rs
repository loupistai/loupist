use super::*;

const AT: i64 = 1_700_000_000_000;
const LOUPIST: &str = "loupist-publication";

struct Publications {
    world: String,
    second_source: String,
    second_source_revision: String,
    second_span: String,
    first: GraphJobView,
    second: GraphJobView,
    first_artifact: String,
    second_artifact: String,
}

fn prepare(store: &KnowledgeStore, settings: &GraphProcessingSettings) -> Publications {
    let source = source_module(store, LOUPIST);
    let (span, source_revision) = retain(store, LOUPIST, &source, None, b"First report");
    let second_source = source_module(store, LOUPIST);
    let (second_span, second_source_revision) =
        retain(store, LOUPIST, &second_source, None, b"Second report");
    let (world, revision) = world(store, LOUPIST);
    let first = add(
        store,
        settings,
        LOUPIST,
        &world,
        &revision,
        &source,
        &source_revision,
        &span,
    );
    let second = add(
        store,
        settings,
        LOUPIST,
        &world,
        &revision,
        &second_source,
        &second_source_revision,
        &second_span,
    );
    Publications {
        world,
        second_source,
        second_source_revision,
        second_span: second_span.clone(),
        first,
        second,
        first_artifact: world_artifact(&[&span]),
        second_artifact: world_artifact(&[&span, &second_span]),
    }
}

#[test]
fn rebuild_projection_keeps_closed_sources_directed_roles_and_unknown_recording_times() {
    let (_dir, store) = open();
    let mut settings = enabled();
    let publications = prepare(&store, &settings);
    let mut artifact: Value = serde_json::from_str(&publications.second_artifact).unwrap();
    for claim in artifact["claims"].as_array_mut().unwrap() {
        claim["endpoints"][0]["role"] = json!("subject");
        claim["endpoints"][1]["role"] = json!("object");
        claim["recorded_at_ms"] = Value::Null;
    }
    publish(
        &store,
        &mut settings,
        &publications.first,
        &publications.first_artifact,
        AT,
    )
    .unwrap();
    let published = publish(
        &store,
        &mut settings,
        &publications.second,
        &artifact.to_string(),
        AT,
    )
    .unwrap();
    let revision = published.document.sealed_revision_id.unwrap();
    let body: crate::knowledge::WorldSnapshotBody =
        serde_json::from_value(store.get_revision(LOUPIST, &revision).unwrap().payload).unwrap();
    assert_eq!(body.claims.len(), 2);
    for claim in &body.claims {
        assert_eq!(claim.endpoints[0].role, "subject");
        assert_eq!(claim.endpoints[1].role, "object");
        assert_eq!(claim.recorded_at_ms, None);
        assert!(!claim.evidence.is_empty());
    }
    assert_eq!(head(&store, LOUPIST, &publications.world), revision);
}

#[test]
fn successive_world_jobs_keep_only_current_content_and_lightweight_receipts() {
    let (_dir, store) = open();
    let mut settings = enabled();
    let publications = prepare(&store, &settings);
    let first = publish(
        &store,
        &mut settings,
        &publications.first,
        &publications.first_artifact,
        AT,
    )
    .unwrap();
    let first_revision = first.document.sealed_revision_id.clone().unwrap();
    let second = publish(
        &store,
        &mut settings,
        &publications.second,
        &publications.second_artifact,
        AT,
    )
    .unwrap();
    assert_eq!(second.status, JOB_COMPLETED);
    let first = store.graph_job(LOUPIST, &first.job_id).unwrap();
    assert!(first.document.artifact_utf8.is_none());
    assert!(second.document.artifact_utf8.is_none());
    let second_revision = second.document.sealed_revision_id.as_ref().unwrap();
    assert!(matches!(
        store.get_revision(LOUPIST, &first_revision),
        Err(KnowledgeError::NotFound)
    ));
    let current = store.get_revision(LOUPIST, second_revision).unwrap();
    assert_eq!(current.payload["claims"].as_array().unwrap().len(), 2);
    assert_eq!(current.dependencies.len(), 2);
    assert_eq!(head(&store, LOUPIST, &publications.world), *second_revision);
    let repeated = add(
        &store,
        &settings,
        LOUPIST,
        &publications.world,
        second_revision,
        &publications.second_source,
        &publications.second_source_revision,
        &publications.second_span,
    );
    assert_eq!(repeated.job_id, second.job_id);
    assert_eq!(store.graph_jobs(LOUPIST).unwrap().len(), 2);
}

#[test]
fn rolled_back_world_publication_preserves_current_content_and_head() {
    let (_dir, store) = open();
    let mut settings = enabled();
    let publications = prepare(&store, &settings);
    let first = publish(
        &store,
        &mut settings,
        &publications.first,
        &publications.first_artifact,
        AT,
    )
    .unwrap();
    let first_revision = first.document.sealed_revision_id.unwrap();
    let second = claim(&store, &settings, &publications.second.job_id, AT + 300);
    let result: KnowledgeResult<()> = store.transact(|write| {
        publish_and_complete(
            write,
            &mut settings,
            &second.job_id,
            second.document.attempt_id.as_deref().unwrap(),
            &publications.second_artifact,
            &sha256(publications.second_artifact.as_bytes()),
            AT + 400,
            false,
        )?;
        Err(KnowledgeError::Invalid("fixture transaction rollback"))
    });
    assert!(matches!(
        result,
        Err(KnowledgeError::Invalid("fixture transaction rollback"))
    ));
    assert_eq!(head(&store, LOUPIST, &publications.world), first_revision);
    assert_eq!(
        store.graph_job(LOUPIST, &second.job_id).unwrap().status,
        JOB_RUNNING
    );
    assert_eq!(
        store
            .get_revision(LOUPIST, &first_revision)
            .unwrap()
            .payload["claims"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
