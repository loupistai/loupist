//! "Continue all" for Sources paused at the request limit: the single Source
//! retry, applied to every Source the user confirmed.
use super::*;

const L: &str = "loupist-a";

struct Paused {
    record: String,
    jobs: Vec<String>,
}

/// A Source whose first chunk spent its whole allowance and stopped, the
/// way an extraction pauses on `paused_budget`.
fn exhausted(
    store: &KnowledgeStore,
    settings: &GraphProcessingSettings,
    world: &(String, String),
    text: &str,
    at_ms: i64,
) -> Paused {
    let record = source_module(store, L);
    let (span, revision) = retain(store, L, &record, None, text.as_bytes());
    let first = add(
        store, settings, L, &world.0, &world.1, &record, &revision, &span,
    );
    let running = claim(store, settings, &first.job_id, at_ms);
    let attempt = running.document.attempt_id.clone().unwrap();
    store
        .transact(|write| {
            for _ in 0..settings.max_llm_calls {
                reserve_model_call(write.tx, &first.job_id, &attempt)?;
            }
            pause_exhausted_attempt(write.tx, &first.job_id, &attempt, at_ms + 1)
        })
        .unwrap();
    let jobs = store
        .graph_jobs(L)
        .unwrap()
        .into_iter()
        .filter(|job| {
            job.document
                .source_binding
                .as_ref()
                .is_some_and(|binding| binding.source_record_id == record)
        })
        .map(|job| job.job_id)
        .collect();
    Paused { record, jobs }
}

fn status(store: &KnowledgeStore, record: &str) -> String {
    store
        .transact(|write| write.source_processing(L, record))
        .unwrap()
        .status
}

fn continue_all(
    store: &KnowledgeStore,
    settings: &GraphProcessingSettings,
    records: &[&Paused],
    confirmed: u32,
) -> KnowledgeResult<SourcesRetried> {
    let ids: Vec<String> = records.iter().map(|paused| paused.record.clone()).collect();
    store.transact(|write| retry_sources_processing(write.tx, settings, L, &ids, confirmed))
}

#[test]
fn continue_all_gives_each_stopped_source_one_fresh_allowance_and_skips_what_try_again_refuses() {
    let (_directory, store) = open();
    let mut settings = enabled();
    settings.max_llm_calls = 2;
    store
        .transact(|write| store_settings(write.tx, L, &settings))
        .unwrap();
    let world_ids = world(&store, L);
    let a = exhausted(
        &store,
        &settings,
        &world_ids,
        "Ada described the engine.",
        T,
    );
    let b = exhausted(
        &store,
        &settings,
        &world_ids,
        "Babbage designed it.",
        T + 10,
    );
    let c = exhausted(
        &store,
        &settings,
        &world_ids,
        "Menabrea wrote it up.",
        T + 20,
    );
    // Stopped behind a chunk whose paid attempt failed: Try again refuses it,
    // though the Library shows it stopped on the allowance like the others.
    let d = {
        let record = source_module(&store, L);
        let body = "D".repeat(EPISODE_CHUNK_CHARS * 2);
        let (span, revision) = retain(&store, L, &record, None, body.as_bytes());
        add(
            &store,
            &settings,
            L,
            &world_ids.0,
            &world_ids.1,
            &record,
            &revision,
            &span,
        );
        let jobs = store.graph_jobs(L).unwrap();
        let chunks: Vec<_> = jobs
            .iter()
            .filter(|job| {
                job.document
                    .source_binding
                    .as_ref()
                    .unwrap()
                    .source_record_id
                    == record
            })
            .collect();
        assert_eq!(chunks.len(), 2);
        store
            .transact(|write| {
                for (index, job) in chunks.iter().enumerate() {
                    let mut document = job.document.clone();
                    document.extraction.max_llm_calls = 0;
                    document.inference_attempted = index == 0;
                    let to = if index == 0 {
                        JOB_FAILED_BEFORE
                    } else {
                        JOB_PAUSED
                    };
                    write_status(write.tx, &job.job_id, JOB_QUEUED, to, &document)?;
                }
                Ok(())
            })
            .unwrap();
        Paused {
            record,
            jobs: chunks.iter().map(|job| job.job_id.clone()).collect(),
        }
    };
    for paused in [&a, &b, &c, &d] {
        assert_eq!(status(&store, &paused.record), JOB_PAUSED);
    }

    // The current settings give each a fresh allowance.
    settings.max_llm_calls = 8;
    store
        .transact(|write| store_settings(write.tx, L, &settings))
        .unwrap();
    let before = store.graph_jobs(L).unwrap();
    // A confirmation made under another allowance runs nothing.
    assert!(continue_all(&store, &settings, &[&a, &b, &c, &d], 2).is_err());
    assert_eq!(store.graph_jobs(L).unwrap(), before);

    let retried = continue_all(&store, &settings, &[&a, &b, &c, &d], 8).unwrap();
    assert_eq!(
        retried,
        SourcesRetried {
            continued: 3,
            skipped: 1
        }
    );
    for paused in [&a, &b, &c] {
        let job = store.graph_job(L, &paused.jobs[0]).unwrap();
        assert_eq!(job.status, JOB_QUEUED);
        assert_eq!(job.document.extraction.max_llm_calls, 8);
        assert!(!job.document.inference_attempted);
    }
    for job in &d.jobs {
        let stored = store.graph_job(L, job).unwrap();
        assert_eq!(
            stored,
            before
                .iter()
                .find(|old| old.job_id == *job)
                .unwrap()
                .clone()
        );
    }

    // A second click while that batch waits refills nothing.
    let queued = store.graph_jobs(L).unwrap();
    assert_eq!(
        continue_all(&store, &settings, &[&a, &b, &c, &d], 8).unwrap(),
        SourcesRetried {
            continued: 0,
            skipped: 4
        }
    );
    assert_eq!(store.graph_jobs(L).unwrap(), queued);

    // Partial completion: A finishes, B stops on its new allowance, C still waits.
    store
        .transact(|write| {
            let job = get_job(write.tx, L, &a.jobs[0])?;
            write_status(
                write.tx,
                &job.job_id,
                JOB_QUEUED,
                JOB_COMPLETED,
                &job.document,
            )?;
            Ok(())
        })
        .unwrap();
    let running = claim(&store, &settings, &b.jobs[0], T + 100);
    let attempt = running.document.attempt_id.clone().unwrap();
    store
        .transact(|write| {
            for _ in 0..8 {
                reserve_model_call(write.tx, &b.jobs[0], &attempt)?;
            }
            pause_exhausted_attempt(write.tx, &b.jobs[0], &attempt, T + 101)
        })
        .unwrap();
    let statuses: Vec<String> = [&a, &b, &c, &d]
        .iter()
        .map(|paused| status(&store, &paused.record))
        .collect();
    assert_eq!(
        statuses,
        [JOB_COMPLETED, JOB_PAUSED, JOB_QUEUED, JOB_PAUSED],
        "two Sources are paused at the request limit now"
    );
    // Continuing those two funds B once more and still skips D.
    assert_eq!(
        continue_all(&store, &settings, &[&b, &d], 8).unwrap(),
        SourcesRetried {
            continued: 1,
            skipped: 1
        }
    );
    assert_eq!(status(&store, &b.record), JOB_QUEUED);
}

#[test]
fn continue_all_names_at_least_one_source_and_skips_one_named_twice_once() {
    let (_directory, store) = open();
    let settings = enabled();
    assert!(store
        .transact(|write| retry_sources_processing(
            write.tx,
            &settings,
            L,
            &[],
            settings.max_llm_calls
        ))
        .is_err());
    let missing = vec!["rec_missing".to_string(), "rec_missing".to_string()];
    assert_eq!(
        store
            .transact(|write| retry_sources_processing(
                write.tx,
                &settings,
                L,
                &missing,
                settings.max_llm_calls
            ))
            .unwrap(),
        SourcesRetried {
            continued: 0,
            skipped: 1
        }
    );
}
