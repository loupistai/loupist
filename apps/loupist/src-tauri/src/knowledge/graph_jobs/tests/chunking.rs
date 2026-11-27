use super::*;
use crate::knowledge::graph_jobs::chunking::{CHUNKER_BLOCKS, CHUNKER_PARAGRAPHS};

const LOUPIST: &str = "loupist-chunks";

/// A fictional half-year report: German unit line, a Markdown table longer
/// than one chunk.
fn table_source() -> String {
    let mut body = String::from(
        "Nordwind Logistik AG – Halbjahresbericht\n\nAngaben in Mio. EUR\n\n\
         | Segment | H1 2026 | H1 2025 |\n| --- | ---: | ---: |\n",
    );
    for row in 0..400 {
        body.push_str(&format!(
            "| Region {row:03} | 1.{row:03},5 | 9{row:02},2 |\n"
        ));
    }
    body.push_str("\nAusblick: stabil.\n");
    assert!(body.chars().count() > EPISODE_CHUNK_CHARS);
    body
}

fn span_jobs(store: &KnowledgeStore) -> Vec<GraphJobView> {
    let mut jobs = store.graph_jobs(LOUPIST).unwrap();
    jobs.sort_by_key(|job| job.document.chunk_index);
    jobs
}

fn accept_with(
    store: &KnowledgeStore,
    settings: &GraphProcessingSettings,
    request: AddToWorld,
    chunker: u32,
) -> GraphJobView {
    store
        .transact(|write| accept_cut(write.tx, write.blobs, settings, request, chunker))
        .unwrap()
}

fn request(world: &(String, String), source: &str, revision: &str, span: &str) -> AddToWorld {
    AddToWorld {
        loupist_id: LOUPIST.into(),
        world_record_id: world.0.clone(),
        world_revision_id: world.1.clone(),
        source_record_id: source.into(),
        source_revision_id: revision.into(),
        span_id: span.into(),
        at_ms: T,
        binding_role: "extracted_from".into(),
    }
}

#[test]
fn a_continuation_chunk_repeats_its_header_and_keeps_its_true_source_range() {
    let (_directory, store) = open();
    let mut settings = enabled();
    let body = table_source();
    let first = queued(&store, &settings, LOUPIST, body.as_bytes());
    let jobs = span_jobs(&store);
    assert!(jobs.len() >= 2);
    assert!(jobs
        .iter()
        .all(|job| job.document.chunker == CHUNKER_BLOCKS));
    let next = &jobs[1];
    let context = "Angaben in Mio. EUR\n| Segment | H1 2026 | H1 2025 |\n| --- | ---: | ---: |\n";
    let (marker, rest) = next.document.body.split_once('\n').unwrap();
    assert!(marker.starts_with('['), "{marker}");
    assert!(rest.starts_with(context));
    assert!(rest[context.len()..].starts_with("| Region "));

    let source = first.document.source_binding.clone().unwrap();
    let artifact = world_artifact(&[&source.span_id]);
    publish(&store, &mut settings, &first, &artifact, T).unwrap();
    publish(&store, &mut settings, next, &artifact, T + 1_000).unwrap();
    let episodes = store
        .read(|read| {
            super::super::super::world_episodes::by_source(
                read.tx,
                LOUPIST,
                &source.source_record_id,
            )
        })
        .unwrap();
    let chars: Vec<char> = body.chars().collect();
    assert_eq!(episodes.len(), 2);
    assert_eq!(episodes[0].char_start, Some(0));
    assert_eq!(episodes[0].char_end, episodes[1].char_start);
    for (episode, job) in episodes.iter().zip([&first, next]) {
        let passage: String = chars
            [episode.char_start.unwrap() as usize..episode.char_end.unwrap() as usize]
            .iter()
            .collect();
        assert!(job.document.body.ends_with(&passage));
        assert_eq!(episode.char_start, job.document.char_start);
        assert_eq!(episode.char_end, job.document.char_end);
    }
    assert_ne!(
        episodes[1].char_end.unwrap() - episodes[1].char_start.unwrap(),
        next.document.body.chars().count() as u32,
        "the repeated context is not part of the range"
    );
    // A job stored before ranges were recorded maps back to the same passage.
    let mut legacy = next.clone();
    legacy.document.char_start = None;
    legacy.document.char_end = None;
    let mapped = store
        .transact(|write| world_episode(write, &legacy, T))
        .unwrap();
    assert_eq!(
        (mapped.char_start, mapped.char_end),
        (next.document.char_start, next.document.char_end)
    );
}

#[test]
fn an_older_job_document_reads_as_chunker_one_and_round_trips() {
    let (_directory, store) = open();
    let job = queued(&store, &enabled(), LOUPIST, table_source().as_bytes());
    let mut stored = serde_json::to_value(&job.document).unwrap();
    assert_eq!(stored["chunker"], json!(CHUNKER_BLOCKS));
    stored.as_object_mut().unwrap().remove("chunker");
    let older: JobDocument = serde_json::from_value(stored.clone()).unwrap();
    assert_eq!(older.chunker, CHUNKER_PARAGRAPHS);
    assert_eq!(serde_json::to_value(&older).unwrap(), stored);
}

#[test]
fn a_half_read_source_cut_with_version_one_is_cut_the_same_way_again() {
    let (_directory, store) = open();
    let mut settings = enabled();
    let body = table_source();
    let source = source_module(&store, LOUPIST);
    let (span, revision) = retain(&store, LOUPIST, &source, None, body.as_bytes());
    let world = world(&store, LOUPIST);
    // As an older build left it: cut with version 1, its first chunk read.
    let first = accept_with(
        &store,
        &settings,
        request(&world, &source, &revision, &span),
        CHUNKER_PARAGRAPHS,
    );
    let artifact = world_artifact(&[&span]);
    publish(&store, &mut settings, &first, &artifact, T).unwrap();
    let before = span_jobs(&store);
    let v1 = chunk_episode(&body, CHUNKER_PARAGRAPHS).unwrap();
    assert_ne!(v1, chunk_episode(&body, CHUNKER_BLOCKS).unwrap());
    assert_eq!(before.len(), v1.len());

    let world_head = head(&store, LOUPIST, &world.0);
    let again = add(
        &store,
        &settings,
        LOUPIST,
        &world.0,
        &world_head,
        &source,
        &revision,
        &span,
    );
    assert_eq!(again.job_id, first.job_id);
    let after = span_jobs(&store);
    assert_eq!(
        after.iter().map(|job| &job.job_id).collect::<Vec<_>>(),
        before.iter().map(|job| &job.job_id).collect::<Vec<_>>()
    );
    for (job, chunk) in after.iter().zip(&v1) {
        assert_eq!(job.document.chunker, CHUNKER_PARAGRAPHS);
        assert_eq!(job.document.body, chunk.body);
    }
}

#[test]
fn a_one_chunk_source_has_the_same_payload_digest_under_both_versions() {
    let body =
        "(단위: 백만원)\n| 과목 | 당기 | 전기 |\n|---|---:|---:|\n| 매출액 | 67,500 | 72,300 |\n";
    let mut digests = Vec::new();
    for chunker in [CHUNKER_PARAGRAPHS, CHUNKER_BLOCKS] {
        let (_directory, store) = open();
        let settings = enabled();
        let source = source_module(&store, LOUPIST);
        let (span, revision) = retain(&store, LOUPIST, &source, None, body.as_bytes());
        let world = world(&store, LOUPIST);
        let job = accept_with(
            &store,
            &settings,
            request(&world, &source, &revision, &span),
            chunker,
        );
        assert_eq!(job.document.body, body);
        digests.push(job.document.source_binding.unwrap().payload_digest);
    }
    assert_eq!(digests[0], digests[1]);
    assert_eq!(digests[0], sha256(body.as_bytes()));
}
