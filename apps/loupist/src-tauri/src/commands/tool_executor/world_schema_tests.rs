use super::*;
use crate::commands::tests::{chat, profile, seed_profile};

const LOUPIST: &str = "loupist-a";

fn policy(save: bool) -> ToolPolicy {
    ToolPolicy {
        loupist_id: LOUPIST.into(),
        session_id: "chat-a".into(),
        use_memory: save,
        propose_memory_updates: save,
        memory_proposal_budget: 1,
        as_of: None,
    }
}

fn executor(temporary: &tempfile::TempDir) -> AppToolExecutor {
    let state = AppState::for_persistent_test(temporary.path().join("app"));
    let workspace = temporary.path().join(LOUPIST);
    std::fs::create_dir_all(&workspace).unwrap();
    seed_profile(state.store.as_ref(), &profile(LOUPIST, &workspace, 1));
    state
        .store
        .create_chat(&chat("chat-a", LOUPIST, 1))
        .unwrap();
    let mut capture = ToolCapture::for_chat("assistant-vocabulary-test".into());
    capture.principle_user_message_id = Some("user-1".into());
    AppToolExecutor {
        state,
        capture: Arc::new(capture),
        loupist_id: LOUPIST.into(),
        capability_ids: BTreeSet::new(),
    }
}

fn request(name: &str) -> Value {
    json!({
        "types": [{
            "name": name,
            "description": "A semiconductor process step such as etch or deposition, not the fab that runs it (Place).",
            "labels": {"en": "Process step", "ko": "공정 단계"}
        }],
        "relations": [{
            "name": "PRECEDES_STEP",
            "description": "The source process step runs before the target process step in the flow.",
            "signatures": [[name, name]],
            "labels": {"en": "runs before", "ko": "이전 단계"}
        }]
    })
}

fn changes(executor: &AppToolExecutor) -> Vec<(String, Option<String>, Option<String>)> {
    let connection = executor.state.store.lock_loupist(LOUPIST).unwrap();
    let mut query = connection
        .prepare("SELECT producer, origin_chat_id, origin_message_id FROM knowledge_schema_changes ORDER BY rowid")
        .unwrap();
    query
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

#[tokio::test]
async fn a_completed_reply_applies_the_kinds_with_the_message_that_asked() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let staged = executor
        .stage_world_schema_update(&policy(true), request("ProcessStep"), "delivery-1")
        .await
        .unwrap();
    assert_eq!(staged["status"], "staged");
    assert!(
        changes(&executor).is_empty(),
        "nothing applies before the reply finishes"
    );
    let _ = executor.commit_delivery(&policy(true), "delivery-1").await;
    let saved = executor.capture.staged_vocabulary.lock().await.clone();
    assert_eq!(saved.len(), 1);
    crate::commands::assets::with_assets(&executor.state, LOUPIST, true, |tx| {
        commit_vocabulary(tx, LOUPIST, "chat-a", 100, &saved)
    })
    .unwrap();
    assert_eq!(
        changes(&executor),
        vec![("chat".into(), Some("chat-a".into()), Some("user-1".into()))]
    );
    let connection = executor.state.store.lock_loupist(LOUPIST).unwrap();
    let live = crate::knowledge::live_extension(&connection, LOUPIST).unwrap();
    assert_eq!(live.types[0].name, "ProcessStep");
    assert_eq!(live.relations[0].name, "PRECEDES_STEP");
}

#[tokio::test]
async fn a_name_the_map_gained_since_staging_is_skipped_and_reported_and_other_errors_surface() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    executor
        .stage_world_schema_update(&policy(true), request("ProcessStep"), "delivery-1")
        .await
        .unwrap();
    let _ = executor.commit_delivery(&policy(true), "delivery-1").await;
    let saved = executor.capture.staged_vocabulary.lock().await.clone();
    let mut reviewed = saved[0].types.clone();
    reviewed[0].labels.en = "Fab step".into();
    let commit = |staged: &[StagedVocabulary], at_ms: i64| {
        crate::commands::assets::with_assets(&executor.state, LOUPIST, true, |tx| {
            commit_vocabulary(tx, LOUPIST, "chat-a", at_ms, staged)
        })
    };
    crate::commands::assets::with_assets(&executor.state, LOUPIST, true, |tx| {
        crate::knowledge::add_kinds(
            tx,
            LOUPIST,
            reviewed,
            Vec::new(),
            crate::knowledge::Provenance::new(crate::knowledge::Producer::Review, json!({})),
            50,
        )
        .map(|_| ())
    })
    .unwrap();
    let skipped = commit(&saved, 100).unwrap();
    assert_eq!(
        serde_json::to_value(&skipped).unwrap(),
        json!([
            {"name": "ProcessStep", "label": "Process step", "labelKo": "공정 단계"},
            {"name": "PRECEDES_STEP", "label": "runs before", "labelKo": "이전 단계"},
        ])
    );
    assert_eq!(
        changes(&executor),
        vec![("review".into(), None, None)],
        "only the review's change applied"
    );
    let mut orphan = saved[0].clone();
    orphan.types.clear();
    orphan.relations[0].name = "FOLLOWS_STEP".into();
    orphan.relations[0].signatures = vec![("GhostStep".into(), "GhostStep".into())];
    assert!(
        commit(&[orphan], 200).is_err(),
        "a change the map cannot take for another reason is not swallowed"
    );
}

#[tokio::test]
async fn a_stopped_reply_applies_nothing() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    executor
        .stage_world_schema_update(&policy(true), request("ProcessStep"), "delivery-1")
        .await
        .unwrap();
    executor.discard_delivery(&policy(true), "delivery-1").await;
    assert!(executor.capture.staged_vocabulary.lock().await.is_empty());
    assert!(changes(&executor).is_empty());
}

#[tokio::test]
async fn saving_off_and_known_names_are_refused() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    assert!(matches!(
        executor
            .stage_world_schema_update(&policy(false), request("ProcessStep"), "d")
            .await,
        Err(BrokerError::MemoryDisabled)
    ));
    for name in ["Technology", "Organization"] {
        assert!(executor
            .stage_world_schema_update(&policy(true), request(name), "d")
            .await
            .is_err());
    }
    let mut oversized = request("ProcessStep");
    oversized["types"][0]["description"] = json!("short");
    assert!(executor
        .stage_world_schema_update(&policy(true), oversized, "d")
        .await
        .is_err());
    assert!(executor
        .stage_world_schema_update(&policy(true), json!({}), "d")
        .await
        .is_err());
}
