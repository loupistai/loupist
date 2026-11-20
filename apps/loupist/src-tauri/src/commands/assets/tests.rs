use serde_json::json;
use tauri::Manager;

use super::*;

const A: &str = "loupist-a";
const B: &str = "loupist-b";

/// A stored price for this Loupist's page of the asset, as a finished replay leaves it.
fn store_price(state: &AppState, loupist: &str, asset: &AssetRow) {
    with_assets(state, loupist, true, |tx| {
        assets::ensure_asset_page(tx, loupist, asset, now_ms())?;
        saved_read::save(
            tx,
            loupist,
            &saved_read::StagedPriceRead {
                asset_id: asset.asset_id.clone(),
                read: serde_json::from_value(json!({
                    "state":"ready",
                    "current":{
                        "tool":"quote",
                        "arguments":{"symbol":"NVDA"},
                        "fields":{"value":"/value","date":"/date","currency":"/currency"}
                    }
                }))?,
                price: serde_json::from_value(json!({
                    "value":159.0,
                    "date":"2026-11-18",
                    "currency":"USD",
                    "provider":"Yahoo Finance",
                    "dataset":{"dataset_id":"fixture","revision":1,"digest":"fixture"}
                }))?,
                history: Vec::new(),
            },
        )
    })
    .unwrap();
}

#[tokio::test]
async fn following_commands_and_bot_tiles_act_on_the_requesting_loupist_alone() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState::for_test(root.path().join("app"));
    for id in [A, B] {
        crate::commands::tests::seed_profile(
            &state.store,
            &crate::commands::tests::profile(id, &root.path().join(id), 1),
        );
    }
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    app.manage(state.clone());
    let request = |loupist: &str| AssetsListRequest {
        loupist_id: loupist.into(),
    };
    let names = |loupist: &str| {
        assets_list(app.state(), request(loupist))
            .unwrap()
            .into_iter()
            .map(|row| row.name)
            .collect::<Vec<_>>()
    };
    let by_id = |loupist: &str, asset: &AssetRow| AssetIdRequest {
        loupist_id: loupist.into(),
        asset_id: asset.asset_id.clone(),
    };
    let page = |loupist: &str, asset: &AssetRow| {
        page_for(
            &state,
            AssetOpenRequest {
                loupist_id: loupist.into(),
                asset_id: Some(asset.asset_id.clone()),
                ticker: None,
                entity_id: None,
                stored_only: Some(true),
            },
        )
        .unwrap()
    };

    let nvidia = asset_follow_name(
        app.state(),
        AssetFollowNameRequest {
            loupist_id: A.into(),
            name: "Nvidia".into(),
            reference: None,
        },
    )
    .unwrap();
    assert_eq!(names(A), ["Nvidia"]);
    assert!(names(B).is_empty());
    // B opens the same asset's page unfollowed; its Follow follows it for B.
    assert_eq!(page(B, &nvidia).asset.followed_at_ms, None);
    assert!(page(A, &nvidia).asset.followed_at_ms.is_some());
    asset_follow(app.state(), by_id(B, &nvidia)).unwrap();
    assert_eq!(names(B), ["Nvidia"]);
    store_price(&state, A, &nvidia);
    store_price(&state, B, &nvidia);

    // A stops following: B's follow, page and Bot tiles stay.
    asset_unfollow(app.state(), by_id(A, &nvidia)).unwrap();
    assert!(names(A).is_empty());
    assert_eq!(names(B), ["Nvidia"]);
    assert_eq!(page(A, &nvidia).asset.followed_at_ms, None);
    assert!(watchlist_quotes(app.state(), request(A))
        .unwrap()
        .is_empty());
    let tiles = watchlist_quotes(app.state(), request(B)).unwrap();
    assert_eq!(tiles.len(), 1);
    assert_eq!(tiles[0].asset_id, nvidia.asset_id);
}
