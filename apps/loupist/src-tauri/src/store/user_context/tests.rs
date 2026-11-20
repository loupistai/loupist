use super::*;

const FIGI: &str = "BBG000BBJQV0";
const A: &str = "loupist-a";
const B: &str = "loupist-b";

/// A catalog with two registered Loupists, A and B.
fn store() -> SqliteStore {
    let store = SqliteStore::open_in_memory().unwrap();
    for id in [A, B] {
        store
            .create_loupist(&crate::test_fixtures::loupist_profile(id))
            .unwrap();
    }
    store
}

fn listing(figi: &str) -> AssetReferenceFields {
    AssetReferenceFields {
        figi: Some(figi.into()),
        ticker: Some("PERS".into()),
        exchange: Some("US".into()),
        ..Default::default()
    }
}

#[test]
fn a_follow_keeps_the_listing_exchange_name_when_supplied() {
    let store = store();
    let follow = |fields| {
        store
            .asset_follow_name(
                A,
                "Example",
                Some(fields),
                AssetReferenceOrigin::Openfigi,
                1,
            )
            .unwrap()
    };
    let first = follow(listing(FIGI));
    assert!(first
        .reference
        .as_ref()
        .unwrap()
        .fields
        .exchange_name
        .is_none());
    let named = follow(AssetReferenceFields {
        exchange_name: Some("Example Exchange".into()),
        ..listing(FIGI)
    });
    assert_eq!(named.asset_id, first.asset_id);
    assert_eq!(
        store.assets_list(A, false).unwrap()[0]
            .reference
            .as_ref()
            .unwrap()
            .fields
            .exchange_name
            .as_deref(),
        Some("Example Exchange")
    );
}

#[test]
fn a_staged_follow_can_add_the_name_to_an_existing_venue() {
    let store = store();
    let original = store
        .asset_follow_name(
            A,
            "Example",
            Some(listing(FIGI)),
            AssetReferenceOrigin::User,
            1,
        )
        .unwrap();
    let reference = AssetReferenceFields {
        exchange_name: Some("Example Exchange".into()),
        ..listing(FIGI)
    };
    let planned = plan_follow(
        std::slice::from_ref(&original),
        "Example",
        Some(reference),
        AssetReferenceOrigin::Agent,
        2,
    )
    .unwrap();
    let written = store.apply_asset_follow(A, &planned, 2).unwrap().unwrap();
    assert_eq!(written, planned);
    assert!(store.apply_asset_follow(A, &planned, 3).unwrap().is_none());
}

#[test]
fn asset_follow_name_as_typed_reuses_and_refollows_a_listing() {
    let store = store();
    let asset = store
        .seed_followed_listing(A, "LMPST", "US", Some("Étoile Test Works"))
        .unwrap();
    let follow = |at_ms| {
        store
            .asset_follow_name(
                A,
                "  étoile TEST WORKS  ",
                None,
                AssetReferenceOrigin::User,
                at_ms,
            )
            .unwrap()
    };
    assert_eq!(follow(asset.created_at_ms + 1), asset);

    store.set_asset_followed(A, &asset.asset_id, None).unwrap();
    let restored = follow(asset.created_at_ms + 2);
    assert_eq!(
        restored,
        AssetRow {
            followed_at_ms: Some(asset.created_at_ms + 2),
            ..asset
        }
    );
    assert_eq!(store.assets_list(A, false).unwrap(), vec![restored]);
}

#[test]
fn asset_follow_name_as_typed_reuses_a_name_only_asset() {
    let store = store();
    let asset = store
        .asset_follow_name(A, "Persistence Works", None, AssetReferenceOrigin::User, 1)
        .unwrap();
    let repeated = store
        .asset_follow_name(
            A,
            " persistence WORKS ",
            None,
            AssetReferenceOrigin::User,
            2,
        )
        .unwrap();
    assert_eq!(repeated, asset);
    assert_eq!(store.assets_list(A, false).unwrap(), vec![asset]);
}

#[test]
fn asset_follow_name_with_a_figi_prefers_the_listing_over_a_name_match() {
    let store = store();
    let named = store
        .asset_follow_name(A, "Persistence Fund", None, AssetReferenceOrigin::User, 1)
        .unwrap();
    let original = store
        .asset_follow_name(
            A,
            "Previous Label",
            Some(listing(FIGI)),
            AssetReferenceOrigin::Openfigi,
            2,
        )
        .unwrap();
    store
        .set_asset_followed(A, &original.asset_id, None)
        .unwrap();

    let mut updated_fields = listing(FIGI);
    updated_fields.ticker = Some("NEW".into());
    let restored = store
        .asset_follow_name(
            A,
            "Persistence Fund",
            Some(updated_fields),
            AssetReferenceOrigin::Openfigi,
            3,
        )
        .unwrap();
    assert_eq!(
        restored,
        AssetRow {
            followed_at_ms: Some(3),
            ..original
        }
    );
    assert_eq!(store.assets_list(A, false).unwrap(), vec![named, restored]);
}

#[test]
fn asset_follow_name_with_a_figi_adds_a_reference_to_the_same_name() {
    let store = store();
    let named = store
        .asset_follow_name(A, "Persistence Fund", None, AssetReferenceOrigin::User, 1)
        .unwrap();
    let fields = listing(FIGI);
    let followed = store
        .asset_follow_name(
            A,
            "  persistence FUND  ",
            Some(fields.clone()),
            AssetReferenceOrigin::Openfigi,
            2,
        )
        .unwrap();
    assert_eq!(
        followed,
        AssetRow {
            reference: Some(AssetReference {
                fields,
                origin: AssetReferenceOrigin::Openfigi
            }),
            ..named
        }
    );
    assert_eq!(store.assets_list(A, false).unwrap(), vec![followed]);
}

#[test]
fn asset_follow_name_with_a_figi_creates_a_separate_listing_when_it_differs() {
    let store = store();
    let name = "Persistence Fund";
    let legacy = store
        .seed_followed_listing(A, "PERS", "US", Some(name))
        .unwrap();
    let first = store
        .asset_follow_name(
            A,
            name,
            Some(listing(FIGI)),
            AssetReferenceOrigin::Openfigi,
            2,
        )
        .unwrap();
    let second = store
        .asset_follow_name(
            A,
            name,
            Some(listing("BBG000BBJR79")),
            AssetReferenceOrigin::Openfigi,
            3,
        )
        .unwrap();
    assert_ne!(first.asset_id, legacy.asset_id);
    assert_ne!(second.asset_id, first.asset_id);
    assert_eq!(store.assets_list(A, false).unwrap().len(), 3);
    // Name-only follows still pick an asset already followed when several listings have that name.
    store.set_asset_followed(A, &first.asset_id, None).unwrap();
    store.set_asset_followed(A, &legacy.asset_id, None).unwrap();
    assert_eq!(
        store
            .asset_follow_name(A, name, None, AssetReferenceOrigin::User, 4)
            .unwrap(),
        second
    );
}

#[test]
fn staged_asset_follow_adds_a_reference_without_replacing_a_concurrent_listing() {
    let store = store();
    let name = "Persistence Fund";
    let named = store
        .asset_follow_name(A, name, None, AssetReferenceOrigin::User, 1)
        .unwrap();
    let plan = |figi| {
        plan_follow(
            std::slice::from_ref(&named),
            name,
            Some(listing(figi)),
            AssetReferenceOrigin::Agent,
            2,
        )
        .unwrap()
    };
    let accepted = plan(FIGI);
    let stale = plan("BBG000BBJR79");
    assert_eq!(
        store.apply_asset_follow(A, &accepted, 3).unwrap(),
        Some(accepted.clone())
    );
    assert!(matches!(
        store.apply_asset_follow(A, &stale, 4),
        Err(StoreError::Conflict(_))
    ));
    assert_eq!(store.assets_list(A, false).unwrap(), vec![accepted]);
}

#[test]
fn unfollow_preserves_the_asset_and_exact_lookup_uses_its_reference() {
    let store = store();
    let asset = store
        .seed_followed_listing(A, "NVDA", "US", Some("Nvidia"))
        .unwrap();
    let typed = store
        .asset_follow_name(A, "nvda", None, AssetReferenceOrigin::User, 2)
        .unwrap();
    assert_ne!(asset.asset_id, typed.asset_id);
    let rows = store.assets_list(A, false).unwrap();
    assert_eq!(matching(&rows, "nvda"), vec![&asset]);
    assert_eq!(matching(&rows, "Nvid"), vec![&asset]);
    store.set_asset_followed(A, &asset.asset_id, None).unwrap();
    assert_eq!(store.assets_list(A, true).unwrap(), vec![typed]);
    let retained = store.asset_by_id(A, &asset.asset_id).unwrap().unwrap();
    assert_eq!(retained.followed_at_ms, None);
    assert_eq!(retained.reference, asset.reference);
    assert_eq!(retained.name, asset.name);
}

#[test]
fn each_loupist_follows_its_own_assets_over_one_catalog() {
    let store = store();
    let nvidia = store
        .asset_follow_name(
            A,
            "Nvidia",
            Some(listing(FIGI)),
            AssetReferenceOrigin::Openfigi,
            1,
        )
        .unwrap();
    let fund = store
        .asset_follow_name(B, "Harbor Fund", None, AssetReferenceOrigin::User, 2)
        .unwrap();
    let followed = |loupist| {
        let mut ids: Vec<_> = store
            .assets_list(loupist, true)
            .unwrap()
            .into_iter()
            .map(|row| row.asset_id)
            .collect();
        ids.sort();
        ids
    };
    assert_eq!(followed(A), std::slice::from_ref(&nvidia.asset_id));
    assert_eq!(followed(B), std::slice::from_ref(&fund.asset_id));
    // The catalog is shared: B reads A's asset as one it does not follow.
    assert_eq!(
        store.asset_by_id(B, &nvidia.asset_id).unwrap(),
        Some(AssetRow {
            followed_at_ms: None,
            ..nvidia.clone()
        })
    );
    assert_eq!(store.map_context(A).unwrap().watchlist.len(), 1);
    assert_eq!(store.map_context(B).unwrap().updated_at_ms, 2);

    // B following the same listing lands on the same asset, at its own time.
    let shared = store
        .asset_follow_name(
            B,
            "NVIDIA CORP",
            Some(listing(FIGI)),
            AssetReferenceOrigin::Openfigi,
            5,
        )
        .unwrap();
    assert_eq!(shared.asset_id, nvidia.asset_id);
    assert_eq!(shared.followed_at_ms, Some(5));
    assert_eq!(
        store
            .asset_by_id(A, &nvidia.asset_id)
            .unwrap()
            .unwrap()
            .followed_at_ms,
        Some(1)
    );

    // Unfollowing and following by ID change only that Loupist's list.
    store.set_asset_followed(A, &nvidia.asset_id, None).unwrap();
    assert!(followed(A).is_empty());
    assert!(followed(B).contains(&nvidia.asset_id));
    store
        .set_asset_followed(A, &fund.asset_id, Some(7))
        .unwrap();
    assert_eq!(followed(A), std::slice::from_ref(&fund.asset_id));
    assert_eq!(
        store
            .asset_by_id(B, &fund.asset_id)
            .unwrap()
            .unwrap()
            .followed_at_ms,
        Some(2)
    );
    // Following an asset the catalog does not have changes nothing.
    assert_eq!(
        store
            .set_asset_followed(A, "ast_0123456789abcdef", Some(8))
            .unwrap(),
        None
    );
    assert_eq!(followed(A), [fund.asset_id]);
}

#[test]
fn removing_a_loupist_removes_its_follows_and_keeps_the_catalog() {
    let store = store();
    let asset = store
        .asset_follow_name(A, "Harbor Fund", None, AssetReferenceOrigin::User, 1)
        .unwrap();
    store
        .set_asset_followed(B, &asset.asset_id, Some(2))
        .unwrap();
    let profile = store.get_loupist(A).unwrap().unwrap();
    store.delete_loupist(&profile).unwrap();
    let follows: i64 = store
        .lock()
        .unwrap()
        .query_row(
            "SELECT count(*) FROM asset_follows WHERE loupist_id=?1",
            [A],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(follows, 0);
    assert_eq!(store.assets_list(A, false).unwrap().len(), 1);
    assert_eq!(
        store.assets_list(B, true).unwrap()[0].followed_at_ms,
        Some(2)
    );
    // A Loupist that is not registered cannot follow.
    assert!(store
        .asset_follow_name(A, "Harbor Fund", None, AssetReferenceOrigin::User, 3)
        .is_err());
}
