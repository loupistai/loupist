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
            &[],
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

#[test]
fn an_invalid_new_node_does_not_block_the_valid_part_of_a_publication() {
    let (_directory, store) = open();
    let mut settings = enabled();
    let publications = prepare(&store, &settings);
    let mut artifact: Value = serde_json::from_str(&publications.first_artifact).unwrap();
    let mut long_name = artifact["nodes"][0].clone();
    long_name["adapter_id"] = json!("long_name");
    long_name["name"] = json!("한".repeat(500));
    artifact["nodes"].as_array_mut().unwrap().push(long_name);
    let mut invalid = artifact["nodes"][0].clone();
    invalid["adapter_id"] = json!("bad_node");
    invalid["properties"] = json!({"invalid key": true});
    artifact["nodes"].as_array_mut().unwrap().push(invalid);
    let mut invalid_claim = artifact["claims"][0].clone();
    invalid_claim["adapter_id"] = json!("bad_claim");
    invalid_claim["endpoints"][0]["node_id"] = json!("bad_node");
    artifact["claims"]
        .as_array_mut()
        .unwrap()
        .push(invalid_claim);
    let completed = publish(
        &store,
        &mut settings,
        &publications.first,
        &artifact.to_string(),
        AT,
    )
    .unwrap();
    assert_eq!(completed.status, JOB_COMPLETED);
    let body: crate::knowledge::WorldSnapshotBody = serde_json::from_value(
        store
            .get_revision(
                LOUPIST,
                completed.document.sealed_revision_id.as_deref().unwrap(),
            )
            .unwrap()
            .payload,
    )
    .unwrap();
    assert_eq!(body.claims.len(), 1);
    assert!(body.nodes.iter().all(|node| node.id != "bad_node"));
    assert!(body
        .nodes
        .iter()
        .any(|node| node.id == "long_name" && node.name.len() <= 200));
    assert!(settings.quarantined_generations.is_empty());
}

#[test]
fn repeated_authorized_spans_do_not_hide_a_later_foreign_span() {
    let (_directory, store) = open();
    let publications = prepare(&store, &enabled());
    let (nodes, mut claims, annotations) =
        project_worker_artifact(&publications.first_artifact, &[]).unwrap();
    let evidence = claims[0].evidence[0].clone();
    claims[0].evidence = vec![evidence; 31];
    let mut body = crate::knowledge::WorldSnapshotBody {
        nodes,
        claims,
        annotations,
        relation_vocabulary: Vec::new(),
        temporal: Default::default(),
    };
    store
        .read(|read| crate::knowledge::world::authorize_spans(read.tx, LOUPIST, &body))
        .unwrap();
    let foreign = source_module(&store, "loupist-other");
    let (foreign_span, _) = retain(&store, "loupist-other", &foreign, None, b"Foreign evidence");
    let mut foreign = body.claims[0].evidence[0].clone();
    foreign.span_id = foreign_span;
    body.claims[0].evidence.push(foreign);
    assert!(store
        .read(|read| crate::knowledge::world::authorize_spans(read.tx, LOUPIST, &body))
        .is_err());
}

#[test]
fn growth_uses_the_publishing_source_even_when_two_jobs_finish_at_the_same_time() {
    let (_dir, store) = open();
    let mut settings = enabled();
    let fixture = prepare(&store, &settings);
    let first = publish(
        &store,
        &mut settings,
        &fixture.first,
        &fixture.first_artifact,
        AT,
    )
    .unwrap();
    let mut second_artifact: Value = serde_json::from_str(&fixture.second_artifact).unwrap();
    let fact = &mut second_artifact["claims"][1];
    let fact_subject = fact["endpoints"][0]["node_id"].clone();
    fact["endpoints"][1]["node_id"] = fact_subject.clone();
    fact["relation"] = json!("REPORTS");
    fact["proposition"] = json!("Ada reported 5 completed runs.");
    let second = publish(
        &store,
        &mut settings,
        &fixture.second,
        &second_artifact.to_string(),
        AT,
    )
    .unwrap();
    let events = store
        .growth_events(&crate::knowledge::GrowthEventsQuery {
            loupist_id: LOUPIST.into(),
            since_ms: Some(AT + 100),
            until_ms: Some(AT + 100),
            world_revision_id: second.document.sealed_revision_id.clone(),
            limit: None,
            cursor: None,
        })
        .unwrap()
        .events;
    for (job, event_count) in [(first, 3), (second, 1)] {
        let binding = job.document.source_binding.unwrap();
        let revision = job.document.sealed_revision_id.unwrap();
        let published: Vec<_> = events
            .iter()
            .filter_map(|event| event.publication.as_ref())
            .filter(|publication| publication.revision_id == revision)
            .collect();
        assert_eq!(published.len(), event_count);
        for publication in published {
            let source = publication.source.as_ref().unwrap();
            assert_eq!(source.record_id, binding.source_record_id);
            assert_eq!(source.revision_id, binding.source_revision_id);
            assert_eq!(source.source_id, binding.source_id);
            assert_eq!(source.title, "Notes");
            assert_eq!(
                publication.fact_subject_id.as_deref(),
                if event_count == 1 {
                    fact_subject.as_str()
                } else {
                    None
                },
            );
        }
    }
}

#[test]
fn growth_records_no_single_source_for_a_manual_publication_with_several_sources() {
    let (_dir, store) = open();
    let fixture = prepare(&store, &enabled());
    let dependencies = [&fixture.first, &fixture.second].map(|job| {
        let binding = job.document.source_binding.as_ref().unwrap();
        let revision = store
            .get_revision(LOUPIST, &binding.source_revision_id)
            .unwrap();
        crate::knowledge::EvidenceRevision {
            record_id: binding.source_record_id.clone(),
            revision_digest: revision.revision_digest,
            role: crate::knowledge::DependencyRole::WorldSource,
        }
    });
    let at_ms = AT + 100;
    let published = store
        .seal_world(crate::knowledge::SealWorld {
            loupist_id: LOUPIST.into(),
            record_id: fixture.world,
            expected_revision_id: Some(fixture.first.world_revision_id),
            at_ms,
            nodes: vec![crate::knowledge::GraphNode {
                id: "nod_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                name: "Ada".into(),
                aliases: Vec::new(),
                description: String::new(),
                kind: "Entity".into(),
                source_role: "source".into(),
                properties: Default::default(),
            }],
            claims: Vec::new(),
            annotations: Vec::new(),
            dependencies: Vec::from(dependencies),
        })
        .unwrap();
    let events = store
        .growth_events(&crate::knowledge::GrowthEventsQuery {
            loupist_id: LOUPIST.into(),
            since_ms: Some(at_ms),
            until_ms: Some(at_ms),
            world_revision_id: Some(published.revision_id.clone()),
            limit: None,
            cursor: None,
        })
        .unwrap()
        .events;
    let [event] = events.as_slice() else {
        panic!("expected one world change: {events:?}");
    };
    let publication = event.publication.as_ref().unwrap();
    assert_eq!(publication.revision_id, published.revision_id);
    assert!(publication.source.is_none());
}

#[test]
fn source_chunks_record_exact_character_ranges_and_bound_episode_changes() {
    let (_directory, store) = open();
    let mut settings = enabled();
    let body = format!(
        "{}\n\nSecond filing paragraph",
        "한".repeat(EPISODE_CHUNK_CHARS - 2)
    );
    let first = queued(&store, &settings, LOUPIST, body.as_bytes());
    let source = first.document.source_binding.as_ref().unwrap();

    let mut artifact: Value = serde_json::from_str(&world_artifact(&[&source.span_id])).unwrap();
    let template = artifact["claims"][0].clone();
    let rows = artifact["claims"].as_array_mut().unwrap();
    for i in 0..260 {
        let mut claim = template.clone();
        claim["adapter_id"] = json!(format!("clm_{i:032x}"));
        claim["proposition"] = json!(format!("The source records observation {i}."));
        rows.push(claim);
    }
    let first_done = publish(&store, &mut settings, &first, &artifact.to_string(), AT).unwrap();
    let next = store
        .graph_jobs(LOUPIST)
        .unwrap()
        .into_iter()
        .find(|job| job.document.chunk_index == Some(1))
        .unwrap();
    let second_done = publish(
        &store,
        &mut settings,
        &next,
        &artifact.to_string(),
        AT + 1_000,
    )
    .unwrap();
    let episodes = store
        .read(|read| {
            super::super::super::world_episodes::by_source(
                read.tx,
                LOUPIST,
                &source.source_record_id,
            )
        })
        .unwrap();
    assert_eq!(episodes.len(), 2);
    assert_eq!(
        episodes[0].sealed_revision_id,
        first_done.document.sealed_revision_id.unwrap()
    );
    assert_eq!(
        episodes[1].sealed_revision_id,
        second_done.document.sealed_revision_id.unwrap()
    );
    assert_eq!(episodes[0].changes.created.len(), 256);
    assert_eq!(episodes[0].changes.truncated["created"], 5);
    assert!(episodes[1].changes.created.is_empty());
    assert!(episodes[1].changes.ended.is_empty());
    assert_eq!(episodes[0].reference_basis, Some(ReferenceBasis::Received));
    assert_eq!(
        episodes[0].model.as_ref().unwrap().model_profile,
        first.document.extraction.model_profile
    );
    let chars: Vec<_> = body.chars().collect();
    assert_eq!(episodes[0].char_start, Some(0));
    assert_eq!(episodes[0].char_end, episodes[1].char_start);
    assert_eq!(episodes[1].char_end, Some(chars.len() as u32));
    for (row, job) in episodes.iter().zip([first, next]) {
        let actual: String = chars
            [row.char_start.unwrap() as usize..row.char_end.unwrap() as usize]
            .iter()
            .collect();
        assert_eq!(actual, job.document.body);
        assert_eq!(row.chunk_count, Some(2));
    }
}

fn episode_count(store: &crate::knowledge::KnowledgeStore) -> i64 {
    store
        .read(|read| {
            Ok(read
                .tx
                .query_row("SELECT COUNT(*) FROM knowledge_world_episodes", [], |row| {
                    row.get(0)
                })?)
        })
        .unwrap()
}

fn published_revisions(
    store: &crate::knowledge::KnowledgeStore,
) -> std::collections::BTreeSet<String> {
    store
        .growth_events(&crate::knowledge::GrowthEventsQuery {
            loupist_id: LOUPIST.into(),
            since_ms: None,
            until_ms: None,
            world_revision_id: None,
            limit: None,
            cursor: None,
        })
        .unwrap()
        .events
        .into_iter()
        .filter_map(|event| event.publication.map(|publication| publication.revision_id))
        .collect()
}

#[test]
fn a_chunk_episode_commits_with_its_seal_and_an_aborted_step_leaves_nothing_to_resume() {
    let (directory, store) = open();
    let mut settings = enabled();
    let publications = prepare(&store, &settings);
    assert_eq!(episode_count(&store), 0);
    let first = publish(
        &store,
        &mut settings,
        &publications.first,
        &publications.first_artifact,
        AT,
    )
    .unwrap();
    let first_revision = first.document.sealed_revision_id.clone().unwrap();
    assert_eq!(head(&store, LOUPIST, &publications.world), first_revision);
    let source = publications
        .first
        .document
        .source_binding
        .as_ref()
        .unwrap()
        .source_record_id
        .clone();
    let episodes = store
        .read(|read| super::super::super::world_episodes::by_source(read.tx, LOUPIST, &source))
        .unwrap();
    assert_eq!(episodes.len(), 1);
    assert_eq!(episodes[0].job_id, first.job_id);
    assert_eq!(episodes[0].sealed_revision_id, first_revision);
    assert!(!episodes[0].backfilled);
    let events = store
        .growth_events(&crate::knowledge::GrowthEventsQuery {
            loupist_id: LOUPIST.into(),
            since_ms: None,
            until_ms: None,
            world_revision_id: Some(first_revision.clone()),
            limit: None,
            cursor: None,
        })
        .unwrap()
        .events;
    let for_seal = |kind: &str| {
        events
            .iter()
            .filter(|event| {
                event.kind == kind
                    && event
                        .publication
                        .as_ref()
                        .is_some_and(|publication| publication.revision_id == first_revision)
            })
            .filter_map(|event| event.subject_id.clone())
            .collect::<std::collections::BTreeSet<_>>()
    };
    let added = for_seal("claim_added");
    assert!(!added.is_empty());
    assert_eq!(
        episodes[0]
            .changes
            .created
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        added
    );
    assert_eq!(
        episodes[0]
            .changes
            .entities_added
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        for_seal("entity_added")
    );
    let revisions = published_revisions(&store);

    let second = claim(&store, &settings, &publications.second.job_id, AT + 300);
    let attempt = second.document.attempt_id.clone().unwrap();
    let aborted: KnowledgeResult<()> = store.transact(|write| {
        publish_and_complete(
            write,
            &mut settings,
            &second.job_id,
            &attempt,
            &publications.second_artifact,
            &sha256(publications.second_artifact.as_bytes()),
            AT + 400,
            false,
            &[],
        )?;
        Err(KnowledgeError::Invalid("fixture transaction rollback"))
    });
    assert!(matches!(
        aborted,
        Err(KnowledgeError::Invalid("fixture transaction rollback"))
    ));
    assert_eq!(episode_count(&store), 1);
    assert_eq!(head(&store, LOUPIST, &publications.world), first_revision);
    assert_eq!(published_revisions(&store), revisions);
    let parked = store.graph_job(LOUPIST, &second.job_id).unwrap();
    assert_eq!(parked.status, JOB_RUNNING);
    assert_eq!(
        parked.document.sealed_revision_id.as_deref(),
        Some(first_revision.as_str()),
        "the claim keeps the head it read; the aborted seal writes no new revision"
    );
    let second_source = publications
        .second
        .document
        .source_binding
        .as_ref()
        .unwrap()
        .source_record_id
        .clone();
    assert!(store
        .read(|read| {
            super::super::super::world_episodes::by_source(read.tx, LOUPIST, &second_source)
        })
        .unwrap()
        .is_empty());

    let path = directory.path().join("knowledge.sqlite");
    drop(store);
    let store = KnowledgeStore::open(&path).unwrap();
    assert_eq!(episode_count(&store), 1);
    assert_eq!(head(&store, LOUPIST, &publications.world), first_revision);
    assert_eq!(
        store.graph_job(LOUPIST, &second.job_id).unwrap().status,
        JOB_RUNNING
    );
    let recovered = store
        .transact(|write| super::super::recover_interrupted(write.tx))
        .unwrap();
    assert_eq!(recovered, 1);
    let resumed = store.graph_job(LOUPIST, &second.job_id).unwrap();
    assert_eq!(resumed.status, JOB_QUEUED);
    assert_eq!(
        resumed.document.sealed_revision_id.as_deref(),
        Some(first_revision.as_str())
    );
    assert_eq!(episode_count(&store), 1);
    let done = publish(
        &store,
        &mut settings,
        &resumed,
        &publications.second_artifact,
        AT + 1_000,
    )
    .unwrap();
    let second_revision = done.document.sealed_revision_id.unwrap();
    assert_eq!(done.status, JOB_COMPLETED);
    assert_ne!(second_revision, first_revision);
    assert_eq!(head(&store, LOUPIST, &publications.world), second_revision);
    assert_eq!(episode_count(&store), 2);
    let episodes = store
        .read(|read| super::super::super::world_episodes::by_source(read.tx, LOUPIST, &source))
        .unwrap();
    assert_eq!(episodes.len(), 1, "the second chunk has its own source");
    let second_episodes = store
        .read(|read| {
            super::super::super::world_episodes::by_source(read.tx, LOUPIST, &second_source)
        })
        .unwrap();
    assert_eq!(second_episodes.len(), 1);
    assert_eq!(second_episodes[0].job_id, second.job_id);
    assert_eq!(second_episodes[0].sealed_revision_id, second_revision);
    assert!(published_revisions(&store).contains(&second_revision));
    assert!(published_revisions(&store).contains(&first_revision));
}
