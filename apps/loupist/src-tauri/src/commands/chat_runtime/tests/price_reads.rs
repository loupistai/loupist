use super::*;
use crate::{
    broker::{ToolExecutor as _, ToolMethod, ToolPolicy},
    chat::surface::ChatSurface,
    commands::assets::{replay_current, with_assets},
    finance_data::KRX_SOURCE_ID,
    knowledge::assets::saved_read,
};
use serde_json::Value;

fn cache_krx(
    executor: &AppToolExecutor,
    policy: &ToolPolicy,
    request: &Value,
    bars: Value,
) -> crate::datasets::DatasetRef {
    let key = executor.persistent_request_key(policy, ToolMethod::KrxDailyQuote, request, None);
    executor
        .state
        .store
        .dataset_put(
            &policy.loupist_id,
            &key,
            &json!({"source_id": KRX_SOURCE_ID, "data": {"bars": bars}}),
            json!({"request": request, "fetched_at": chrono::Utc::now().to_rfc3339()}),
        )
        .unwrap()
}

async fn read_krx(executor: &AppToolExecutor, policy: &ToolPolicy, mut request: Value) -> Value {
    request["max_age"] = json!(30);
    let result = executor
        .execute(policy, ToolMethod::KrxDailyQuote, request)
        .await
        .unwrap();
    assert_eq!(result["reused"], true);
    result
}

#[tokio::test(start_paused = true)]
async fn following_and_saving_a_price_in_one_reply() {
    for with_history in [false, true] {
        assert_follow_and_price_save(with_history, false, false).await;
    }
}

#[tokio::test(start_paused = true)]
async fn pending_follow_and_price_save_survive_a_replaced_dataset_revision() {
    for with_history in [false, true] {
        assert_follow_and_price_save(with_history, true, false).await;
    }
}

#[tokio::test(start_paused = true)]
async fn pending_price_save_names_the_read_it_could_not_keep_after_a_follow_conflict() {
    assert_follow_and_price_save(true, true, true).await;
}

async fn assert_follow_and_price_save(with_history: bool, wait: bool, conflict: bool) {
    use crate::commands::tests::{profile, seed_profile};

    let root = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(root.path().join("app"));
    let profile = profile("price-follow", &root.path().join("workspace"), 1);
    let loupist = profile.id.as_str();
    seed_profile(&state.store, &profile);
    let chat = crate::chat::home::ensure_home_thread(&state, loupist).unwrap();
    // The cached provider reads and replay need an executable binding.
    // Seed only its readiness projection in the test catalog.
    state
        .store
        .set_connector_credential_status(
            KRX_SOURCE_ID,
            &json!({"Known": {
                "status": crate::finance_credentials::CredentialStatus {
                    stored: true,
                    active: true,
                    active_fields: ["api_key".into()].into(),
                    verification: crate::finance_credentials::CredentialVerification::Verified,
                    ..Default::default()
                },
                "active_revision": "fixture-readiness",
            }})
            .to_string(),
        )
        .unwrap();
    state
        .store
        .save_loupist_capability(&crate::domain::LoupistCapabilityBinding {
            loupist_id: loupist.into(),
            entry_id: KRX_SOURCE_ID.into(),
            enabled: true,
            granted_permissions: vec!["execute".into()],
            config: Default::default(),
            updated_at_ms: now_ms(),
        })
        .unwrap();
    let mut capture = ToolCapture::default();
    capture.surface = Some(ChatSurface::Bot);
    let executor = AppToolExecutor {
        state: state.clone(),
        capture: Arc::new(capture),
        loupist_id: loupist.into(),
        capability_ids: crate::commands::capture_chat_connector_authority(&state, loupist)
            .unwrap()
            .capability_ids
            .into_iter()
            .collect(),
    };
    assert!(executor.capability_ids.contains(KRX_SOURCE_ID));
    let policy = ToolPolicy {
        loupist_id: loupist.into(),
        session_id: chat.id.clone(),
        use_memory: true,
        propose_memory_updates: true,
        memory_proposal_budget: 4,
        as_of: None,
        display_currency: None,
    };
    let followed = executor
        .execute(
            &policy,
            ToolMethod::LoupistSetup,
            json!({"follow":[{"name":"Noeul Paper","reference":{"ticker":"990184"}}]}),
        )
        .await
        .unwrap();
    let asset_id = followed["follow"][0]["assetId"].as_str().unwrap();
    assert!(state
        .store
        .asset_by_id(loupist, asset_id)
        .unwrap()
        .is_none());

    let quote_request =
        json!({"symbol":"990184","market":"kospi","start":"2026-03-03","end":"2026-03-04"});
    let history_request =
        json!({"symbol":"990184","market":"kospi","start":"2026-02-03","end":"2026-03-03"});
    let quote_ref = cache_krx(
        &executor,
        &policy,
        &quote_request,
        json!([{"close":"73400","date":"2026-03-03","currency":"KRW"}]),
    );
    let history_ref = cache_krx(
        &executor,
        &policy,
        &history_request,
        json!([{"close":"73100","date":"2026-03-02","currency":"KRW"}]),
    );
    let quote = read_krx(&executor, &policy, quote_request.clone()).await;
    let history = read_krx(&executor, &policy, history_request.clone()).await;
    let fields =
        json!({"rows":"/data/bars","value":"/close","date":"/date","currency":"/currency"});
    let mut params =
        json!({"assetId":asset_id,"citation_url":quote["citation_url"],"fields":fields});
    if with_history {
        params["history"] = json!({
            "citation_url":history["citation_url"],"fields":fields,
            "rolling":{"fromArg":"start","toArg":"end","days":28},
        });
    }
    let staged = executor
        .execute(&policy, ToolMethod::PriceReadSave, params)
        .await
        .unwrap();
    assert_eq!(staged["price"]["value"], 73400.0);
    if conflict {
        executor.execute(&policy, ToolMethod::NoteSave, json!({
            "workingNote": {"title":"Price context","markdown":"The close came from a daily quote.","changeSummary":"Kept the price context"}
        })).await.unwrap();
    }

    let message = super::pending_memory::commit_test_reply(
        &state,
        &chat,
        &executor.capture,
        "I followed Noeul Paper and saved its price read.",
        wait.then_some(|| {
            if conflict {
                let competing = state
                    .store
                    .asset_follow_name(
                        loupist,
                        "Noeul Paper",
                        Some(serde_json::from_value(json!({"ticker":"990184"})).unwrap()),
                        crate::commands::assets::wire::AssetReferenceOrigin::Agent,
                        now_ms(),
                    )
                    .unwrap();
                assert_ne!(competing.asset_id, asset_id);
                let context = crate::agent_harness::followed_context(&state, loupist).unwrap();
                assert_eq!(context["assets"][0]["price_read"], "none");
            }
            // Replacing the retained revisions is stronger than aging their cache entries.
            for (request, old) in [
                (&quote_request, &quote_ref),
                (&history_request, &history_ref),
            ] {
                cache_krx(&executor, &policy, request, json!([]));
                assert!(state
                    .store
                    .dataset(loupist, &old.dataset_id, old.revision)
                    .is_err());
            }
        }),
    )
    .await;
    let update = message.memory_update.as_ref().unwrap();
    assert_eq!(update.skip_reason, None);
    if conflict {
        assert_eq!(
            update.discarded_memory_items,
            Some(vec![crate::domain::DiscardedMemoryItem {
                kind: crate::domain::DiscardedMemoryKind::PriceRead,
                title: "Noeul Paper".into(),
            }]),
        );
        assert_eq!(update.status, crate::domain::MemoryUpdateStatus::Applied);
        assert_eq!(update.changes.len(), 1);
        assert_eq!(update.changes[0].title, "Price context");
        assert!(state
            .store
            .asset_by_id(loupist, asset_id)
            .unwrap()
            .is_none());
        with_assets(&state, loupist, false, |tx| {
            assert!(saved_read::load(tx, loupist, asset_id)?.is_none());
            assert!(crate::knowledge::assets::list_pages(tx, loupist)?.is_empty());
            Ok(())
        })
        .unwrap();
        return;
    }

    let asset = state.store.asset_by_id(loupist, asset_id).unwrap().unwrap();
    assert_eq!(asset.name, "Noeul Paper");
    assert_eq!(
        asset.reference.unwrap().fields.ticker.as_deref(),
        Some("990184")
    );
    assert!(asset.followed_at_ms.is_some());
    assert_eq!(update.setup.len(), 1);
    with_assets(&state, loupist, false, |tx| {
        let stored =
            saved_read::load(tx, loupist, asset_id)?.expect("the acknowledged read must commit");
        let price = stored.price.unwrap();
        assert_eq!(price.value, 73400.0);
        assert_eq!(price.date, "2026-03-03");
        assert_eq!(price.currency.as_deref(), Some("KRW"));
        assert_eq!(stored.read.history.is_some(), with_history);
        let points = saved_read::points(tx, loupist, asset_id)?;
        assert_eq!(points.len(), if with_history { 2 } else { 1 });
        assert_eq!(points.last().unwrap().value, 73400.0);
        Ok(())
    })
    .unwrap();
    assert_eq!(update.status, crate::domain::MemoryUpdateStatus::Applied);
    assert!(update.discarded_memory_items.is_none());
    assert_eq!(update.changes.len(), 1);
    assert_eq!(update.changes[0].kind, "PriceRead");
    assert_eq!(update.changes[0].operation, "create");
    assert_eq!(update.changes[0].record_id, asset_id);
    assert_eq!(update.changes[0].title, "Noeul Paper");

    let context = crate::agent_harness::followed_context(&state, loupist).unwrap();
    assert!(context["assets"][0].get("price_read").is_none());
    cache_krx(
        &executor,
        &policy,
        &quote_request,
        json!([{"close":"73900","date":"2026-03-04","currency":"KRW"}]),
    );
    replay_current(&state, loupist, asset_id, Some(30))
        .await
        .unwrap();
    with_assets(&state, loupist, false, |tx| {
        let points = saved_read::points(tx, loupist, asset_id)?;
        let latest = points.last().unwrap();
        assert_eq!(latest.date, "2026-03-04");
        assert_eq!(latest.value, 73900.0);
        Ok(())
    })
    .unwrap();
}
