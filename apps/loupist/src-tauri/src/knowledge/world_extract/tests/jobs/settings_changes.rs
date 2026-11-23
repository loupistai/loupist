//! A change of the World settings under a running attempt is a host
//! interruption: the chunk runs again on the new settings, spends no retry and
//! keeps the calls it already dispatched. Every call goes through the host's
//! own check (`current_inference`), as in the graph worker.

use super::*;
use crate::knowledge::blob::BlobStore;
use crate::knowledge::graph_inference::refused_for_settings;
use crate::knowledge::graph_jobs::{WorldModel, WorldModelChoice};

/// Something the host saves while the call to `at` is in flight, and how that
/// call then comes back (answered by the script when `reply` is None).
struct Change {
    at: &'static str,
    apply: Box<dyn FnMut() + Send>,
    reply: Option<InferenceDisposition>,
}

fn save(store: &crate::store::SqliteStore, settings: &GraphProcessingSettings) {
    graph_jobs::store_settings(&store.lock_loupist(LOUPIST).unwrap(), LOUPIST, settings).unwrap();
}

/// Save `next` as the World settings while the call to `at` is in flight.
fn saving(fixture: &Fixture, at: &'static str, next: GraphProcessingSettings) -> Change {
    let host = fixture.store.connection.clone();
    Change {
        at,
        apply: Box::new(move || save(&host, &next)),
        reply: None,
    }
}

/// The settings a new attempt claims with, read as the graph worker reads them.
fn reload(fixture: &mut Fixture) {
    fixture.settings = fixture
        .store
        .read(|read| graph_jobs::load_settings(read.tx, LOUPIST))
        .unwrap();
}

/// One attempt with the frozen settings it was claimed with.
fn drive_checked(
    fixture: &mut Fixture,
    job: &GraphJobView,
    script: &Arc<Mutex<Script>>,
    change: Option<Change>,
) -> GraphJobView {
    let host = fixture.store.connection.clone();
    let frozen = fixture.settings.clone();
    let script = script.clone();
    let change = Arc::new(Mutex::new(change));
    runtime()
        .block_on(world_extract::drive_job(
            &fixture.store,
            &mut fixture.settings,
            &job.job_id,
            "fixture",
            AT + 1_000,
            &[],
            &assets(),
            move |request: Value| {
                let (host, frozen, script, change) =
                    (host.clone(), frozen.clone(), script.clone(), change.clone());
                async move {
                    let checked = crate::knowledge::graph_service::current_inference(
                        &host,
                        LOUPIST,
                        &frozen,
                        None,
                        &assets(),
                        &request,
                    )
                    .await;
                    if refused_for_settings(&checked) || request["kind"] == "embedding" {
                        return checked;
                    }
                    let title = request["response_schema"]["title"].as_str().unwrap_or("");
                    let due = change.lock().unwrap().take_if(|change| change.at == title);
                    if let Some(mut change) = due {
                        (change.apply)();
                        if let Some(disposition) = change.reply {
                            script.lock().unwrap().calls.push(title.to_string());
                            return InferenceReply {
                                disposition,
                                body: json!({"status": "uncertain"}),
                            };
                        }
                    }
                    let reply = script.lock().unwrap().answer(&request);
                    reply
                }
            },
        ))
        .unwrap()
}

fn episode(script: &Arc<Mutex<Script>>) {
    let mut script = script.lock().unwrap();
    script.reply("ExtractedEntities", entities(&["Acme Corp", "Beta Motors"]));
    script.reply(
        "ExtractedEdges",
        json!({"edges": [relation("Acme Corp", "Beta Motors", "SUPPLIES", SUPPLY)]}),
    );
    script.reply("CoreAttributeBatch", json!({"entities": []}));
}

fn model(fixture: &Fixture, name: &str) -> GraphProcessingSettings {
    GraphProcessingSettings {
        helper_model: Some(name.into()),
        ..fixture.settings.clone()
    }
}

/// Interrupted, not failed: queued again with no retry spent and nothing
/// left marked as attempted or uncertain.
fn assert_interrupted(view: &GraphJobView, fixture: &Fixture) {
    assert_eq!(view.status, graph_jobs::JOB_QUEUED);
    assert_eq!(view.document.interrupted_attempts, 0);
    assert_eq!(view.document.host_interruptions, 0);
    assert_eq!(view.document.unsent_attempts, 0);
    assert!(!view.document.inference_attempted && !view.document.usage_uncertain);
    assert!(view.document.attempt_id.is_none());
    assert!(fixture.settings.quarantined_generations.is_empty());
}

#[test]
fn a_settings_change_mid_attempt_runs_the_chunk_again_on_the_new_settings() {
    let mut fixture = Fixture::new();
    save(&fixture.store.connection, &fixture.settings);
    let job = fixture.queue("Supply note", SUPPLY);
    let allowance = job.document.extraction.max_llm_calls;
    let script = Arc::new(Mutex::new(Script::default()));
    script
        .lock()
        .unwrap()
        .reply("ExtractedEntities", entities(&["Acme Corp", "Beta Motors"]));
    let change = saving(
        &fixture,
        "ExtractedEntities",
        model(&fixture, "fixture-next"),
    );
    let head = fixture.head();
    let stopped = drive_checked(&mut fixture, &job, &script, Some(change));
    assert_interrupted(&stopped, &fixture);
    // The entity call went out and stays spent; nothing after it was sent.
    assert_eq!(script.lock().unwrap().calls, ["ExtractedEntities"]);
    assert_eq!(stopped.document.extraction.max_llm_calls, allowance - 1);
    assert_eq!(fixture.head(), head);

    reload(&mut fixture);
    episode(&script);
    let finished = drive_checked(&mut fixture, &stopped, &script, None);
    assert_eq!(finished.status, graph_jobs::JOB_COMPLETED);
    assert_eq!(
        finished.document.extraction.model_profile,
        "fixture/fixture-next"
    );
    // The second attempt spent its own calls from what was left; nothing came back.
    let calls = script.lock().unwrap().calls.len() as u32;
    assert_eq!(
        finished.document.extraction.max_llm_calls,
        allowance - calls
    );
}

#[test]
fn settings_changed_in_a_row_leave_the_source_queued_and_never_quarantine_it() {
    let mut fixture = Fixture::new();
    save(&fixture.store.connection, &fixture.settings);
    let job = fixture.queue("Supply note", SUPPLY);
    let allowance = job.document.extraction.max_llm_calls;
    let script = Arc::new(Mutex::new(Script::default()));
    let mut view = job;
    for (round, name) in ["fixture-b", "fixture-c", "fixture-d"]
        .into_iter()
        .enumerate()
    {
        script
            .lock()
            .unwrap()
            .reply("ExtractedEntities", entities(&["Acme Corp", "Beta Motors"]));
        let change = saving(&fixture, "ExtractedEntities", model(&fixture, name));
        view = drive_checked(&mut fixture, &view, &script, Some(change));
        assert_interrupted(&view, &fixture);
        assert_eq!(
            view.document.extraction.max_llm_calls,
            allowance - round as u32 - 1
        );
        reload(&mut fixture);
    }
    episode(&script);
    let finished = drive_checked(&mut fixture, &view, &script, None);
    assert_eq!(finished.status, graph_jobs::JOB_COMPLETED);
    assert_eq!(
        finished.document.extraction.model_profile,
        "fixture/fixture-d"
    );
    assert_eq!(finished.document.interrupted_attempts, 0);
}

#[test]
fn a_change_while_a_call_is_in_flight_keeps_that_uncertain_call_spent() {
    let mut fixture = Fixture::new();
    save(&fixture.store.connection, &fixture.settings);
    let job = fixture.queue("Supply note", SUPPLY);
    let allowance = job.document.extraction.max_llm_calls;
    let script = Arc::new(Mutex::new(Script::default()));
    let mut change = saving(
        &fixture,
        "ExtractedEntities",
        model(&fixture, "fixture-next"),
    );
    change.reply = Some(InferenceDisposition::Uncertain);
    let stopped = drive_checked(&mut fixture, &job, &script, Some(change));
    // No later call was refused: the saved row shows the change.
    assert_interrupted(&stopped, &fixture);
    assert_eq!(stopped.document.extraction.max_llm_calls, allowance - 1);

    reload(&mut fixture);
    episode(&script);
    let finished = drive_checked(&mut fixture, &stopped, &script, None);
    assert_eq!(finished.status, graph_jobs::JOB_COMPLETED);
    let calls = script.lock().unwrap().calls.len() as u32;
    assert_eq!(
        finished.document.extraction.max_llm_calls,
        allowance - calls
    );

    // Without a change, the same uncertain call spends the one retry.
    let mut still = Fixture::new();
    save(&still.store.connection, &still.settings);
    let job = still.queue("Supply note", SUPPLY);
    let script = Arc::new(Mutex::new(Script::default()));
    script
        .lock()
        .unwrap()
        .fail
        .insert("ExtractedEntities".into(), InferenceDisposition::Uncertain);
    let retried = drive_checked(&mut still, &job, &script, None);
    assert_eq!(
        (
            retried.status.as_str(),
            retried.document.interrupted_attempts
        ),
        (graph_jobs::JOB_QUEUED, 1)
    );
}

#[test]
fn an_account_disconnect_that_moves_recommended_mid_job_reruns_on_the_new_model() {
    let mut fixture = Fixture::new();
    fixture.settings.world_model_choice = WorldModelChoice::Recommended;
    fixture.settings.helper_model_profile_id = Some("fixture/fixture-local".into());
    save(&fixture.store.connection, &fixture.settings);
    let job = fixture.queue("Supply note", SUPPLY);
    let script = Arc::new(Mutex::new(Script::default()));
    script
        .lock()
        .unwrap()
        .reply("ExtractedEntities", entities(&["Acme Corp", "Beta Motors"]));
    // The account behind the recommended model goes away: the host points
    // Recommended at the model the remaining account offers.
    let host = fixture.store.connection.clone();
    let blobs = fixture.store.blobs.root().to_path_buf();
    let remaining = WorldModel {
        profile_id: "fixture/fixture-other".into(),
        provider: "fixture".into(),
        model: "fixture-other".into(),
        thinking_level: "low".into(),
        run_options: Default::default(),
    };
    let change = Change {
        at: "ExtractedEntities",
        apply: Box::new(move || {
            let knowledge = KnowledgeStore::attach(host.clone(), BlobStore::open(&blobs).unwrap());
            assert!(knowledge
                .transact(|write| write.follow_recommended_world_model(
                    LOUPIST,
                    Some(&remaining),
                    AT
                ))
                .unwrap());
        }),
        reply: None,
    };
    let stopped = drive_checked(&mut fixture, &job, &script, Some(change));
    assert_interrupted(&stopped, &fixture);

    reload(&mut fixture);
    episode(&script);
    let finished = drive_checked(&mut fixture, &stopped, &script, None);
    assert_eq!(finished.status, graph_jobs::JOB_COMPLETED);
    assert_eq!(
        finished.document.extraction.model_profile,
        "fixture/fixture-other"
    );
    assert_eq!(finished.document.extraction.thinking_level, "low");
}
