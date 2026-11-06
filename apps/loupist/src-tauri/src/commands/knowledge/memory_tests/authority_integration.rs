use super::*;
use crate::knowledge::CreateRecord;
use crate::knowledge::RetainedSource;

fn retain_capture(
    state: &AppState,
    previous: Option<&RetainedSource>,
    text: &str,
    at_ms: i64,
) -> RetainedSource {
    let request = source_retain_request(
        KnowledgeSourceRetainRequest {
            loupist_id: "loupist-a".into(),
            title: text.into(),
            origin_locator: "https://example.test/source".into(),
            coverage: "included_full".into(),
            publication_time: None,
            text: text.into(),
            file: None,
        },
        at_ms,
    )
    .unwrap();
    with_loupist_knowledge(state, "loupist-a", |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        match previous {
            None => write.retain_new_source(request),
            Some(previous) => write.retain_source(request.into_capture(
                previous.record_id.clone(),
                Some(previous.revision_id.clone()),
                Some(previous.source_id.clone()),
            )),
        }
    })
    .unwrap()
}

#[test]
fn newer_captures_keep_world_citations_readable_and_refuse_historical_or_foreign_reads() {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    for id in ["loupist-a", "loupist-b"] {
        state.store.create_loupist(&loupist_profile(id)).unwrap();
    }
    let other = retain_capture(&state, None, "unrelated bytes", 2);
    let first = retain_capture(&state, None, "historical cited bytes", 3);
    let span = first.span_id.clone().unwrap();
    let (world_id, sealed) = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        let world = write.create_record(CreateRecord {
            loupist_id: "loupist-a".into(),
            kind: RecordKind::World,
            title: "World".into(),
            at_ms: 1,
        })?;
        let sealed = write.seal_world(SealWorld {
            loupist_id: "loupist-a".into(),
            record_id: world.record_id.clone(),
            expected_revision_id: None,
            at_ms: 4,
            nodes: Vec::new(),
            claims: Vec::new(),
            annotations: vec![crate::knowledge::TopicAnnotation {
                topic_id: "tech".into(),
                markdown: "Uses this historical capture.".into(),
                source_span_ids: vec![span.clone()],
            }],
            dependencies: vec![EvidenceRevision {
                record_id: first.record_id.clone(),
                revision_digest: first.revision_digest.clone(),
                role: DependencyRole::WorldSource,
            }],
        })?;
        Ok((world.record_id, sealed.revision_id))
    })
    .unwrap();
    let second = retain_capture(&state, Some(&first), "newer capture", 5);

    let target = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).world_source_targets(
            "loupist-a",
            &world_id,
            &sealed,
            std::slice::from_ref(&span),
        )
    })
    .unwrap()
    .remove(&span)
    .unwrap();
    assert_eq!(target.record_id, first.record_id);
    assert_eq!(target.revision_id, second.revision_id);
    let read = |loupist: &str, record: &str, revision: &str, span: &str| {
        source_read(
            &state,
            KnowledgeSourceReadRequest {
                loupist_id: loupist.into(),
                record_id: record.into(),
                revision_id: revision.into(),
                source_id: None,
                span_id: Some(span.into()),
                purpose: "renderer_inspection".into(),
                world_record_id: Some(world_id.clone()),
                world_revision_id: Some(sealed.clone()),
            },
        )
    };
    let detail = read("loupist-a", &first.record_id, &second.revision_id, &span).unwrap();
    assert_eq!(detail.source_id, first.source_id);
    assert_eq!(
        detail.spans[0].text.as_deref(),
        Some("historical cited bytes")
    );
    let second_span = second.span_id.clone().unwrap();
    for (loupist, record, revision, span) in [
        ("loupist-a", &first.record_id, &first.revision_id, &span),
        (
            "loupist-a",
            &other.record_id,
            &other.revision_id,
            other.span_id.as_ref().unwrap(),
        ),
        (
            "loupist-a",
            &first.record_id,
            &first.revision_id,
            &second_span,
        ),
        ("loupist-b", &first.record_id, &first.revision_id, &span),
    ] {
        assert!(read(loupist, record, revision, span).is_err());
    }

    // Model reads only see the current capture of a Source.
    let source_request = |revision: &str, span: &str| HostReadRequest {
        as_of: None,
        scope: "source".into(),
        revision_id: Some(revision.into()),
        span_id: Some(span.into()),
        ..empty_read(&first.record_id)
    };
    for span in [&span, &second_span] {
        assert_eq!(
            host_prepare_read(
                &state,
                "loupist-a",
                &source_request(&first.revision_id, span)
            )
            .unwrap_err()
            .code,
            "not_found"
        );
    }
    let (_, current) = host_prepare_read(
        &state,
        "loupist-a",
        &source_request(&second.revision_id, &second_span),
    )
    .unwrap();
    assert!(matches!(
        current.payload,
        crate::commands::tool_executor::AckedPayload::Source { ref bytes, .. }
            if bytes == b"newer capture"
    ));
}
