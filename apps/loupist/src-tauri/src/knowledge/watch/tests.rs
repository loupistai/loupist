use super::*;
use crate::knowledge::KnowledgeStore;

#[test]
fn defaults_switches_revisions_and_events_are_per_loupist() {
    let root = tempfile::tempdir().unwrap();
    let store = KnowledgeStore::open(root.path().join("knowledge.sqlite")).unwrap();
    store
        .transact(|w| {
            let tx = w.tx;
            body(tx);
            Ok(())
        })
        .unwrap();
}

fn body(tx: &Transaction<'_>) {
    // Defaults: filings, results and dividends on; insiders, holders, flows off.
    let equity = class_defaults(AssetClass::Equity, None);
    assert!(apply_defaults(tx, "a", "ast_0000000000000001", &equity, 10).unwrap());
    assert!(!apply_defaults(tx, "a", "ast_0000000000000001", &equity, 11).unwrap());
    let all = switches(tx, "a").unwrap();
    let asset = &all["ast_0000000000000001"];
    assert!(
        asset.enabled(WatchKind::Filings)
            && asset.enabled(WatchKind::Results)
            && asset.enabled(WatchKind::Dividends)
    );
    assert!(
        !asset.enabled(WatchKind::Insiders)
            && !asset.enabled(WatchKind::Holders)
            && !asset.enabled(WatchKind::Flows)
    );
    assert_eq!(asset.revision, 2);
    // Another Loupist has made no decision.
    assert!(switches(tx, "b").unwrap().is_empty());
    // A stale revision is a conflict; the current one moves it on.
    assert!(matches!(
        set_kind(
            tx,
            "a",
            "ast_0000000000000001",
            WatchKind::Insiders,
            true,
            1,
            12
        ),
        Err(KnowledgeError::RevisionConflict { .. })
    ));
    assert_eq!(
        set_kind(
            tx,
            "a",
            "ast_0000000000000001",
            WatchKind::Insiders,
            true,
            2,
            12
        )
        .unwrap(),
        3
    );
    // A first explicit choice in another Loupist keeps the other kinds off.
    set_kind(
        tx,
        "b",
        "ast_0000000000000001",
        WatchKind::Flows,
        true,
        1,
        13,
    )
    .unwrap();
    let other = &switches(tx, "b").unwrap()["ast_0000000000000001"];
    assert!(other.enabled(WatchKind::Flows) && !other.enabled(WatchKind::Filings));

    let evidence = vec![AssetEvidence {
        tool: "equity_fundamental_filings".into(),
        title: "SEC EDGAR".into(),
        url: Some("https://www.sec.gov/x".into()),
        ..AssetEvidence::default()
    }];
    let event = |key: &str, baseline: bool| NewWatchEvent {
        asset_id: "ast_0000000000000001".into(),
        kind: WatchKind::Filings,
        natural_key: key.into(),
        fact_date: "2026-10-30".into(),
        facts: serde_json::json!({"form": "10-Q", "title": "Quarterly report"}),
        evidence: evidence.clone(),
        baseline,
    };
    // A baseline is stored but never shown or delivered.
    assert_eq!(
        insert_event(tx, "a", &event("filing:old", true), 20).unwrap(),
        None
    );
    let id = insert_event(tx, "a", &event("filing:new", false), 21)
        .unwrap()
        .unwrap();
    // The same filing never fires twice.
    assert_eq!(
        insert_event(tx, "a", &event("filing:new", false), 22).unwrap(),
        None
    );
    assert_eq!(list_events(tx, "a", None, 50).unwrap().len(), 1);
    assert_eq!(pending(tx, "a").unwrap().len(), 1);
    assert_eq!(unseen(tx, "a").unwrap().0, 0);
    mark_delivered(tx, "a", std::slice::from_ref(&id), "message", 23).unwrap();
    assert!(pending(tx, "a").unwrap().is_empty());
    assert_eq!(unseen(tx, "a").unwrap().0, 1);
    mark_seen(tx, "a", std::slice::from_ref(&id), 24).unwrap();
    assert_eq!(unseen(tx, "a").unwrap().0, 0);
    // An event without a source or a date is never stored.
    let mut sourceless = event("filing:x", false);
    sourceless.evidence = vec![AssetEvidence::default()];
    assert_eq!(insert_event(tx, "a", &sourceless, 25).unwrap(), None);

    // A route's first ok check completes its baseline; a failure keeps it.
    record_route(
        tx,
        "a",
        "ast_0000000000000001",
        Route::Filings,
        "failed",
        "SEC EDGAR",
        30,
    )
    .unwrap();
    assert!(
        !route_state(tx, "a", "ast_0000000000000001", Route::Filings)
            .unwrap()
            .unwrap()
            .baseline_done
    );
    record_route(
        tx,
        "a",
        "ast_0000000000000001",
        Route::Filings,
        "ok",
        "SEC EDGAR",
        31,
    )
    .unwrap();
    record_route(
        tx,
        "a",
        "ast_0000000000000001",
        Route::Filings,
        "failed",
        "SEC EDGAR",
        32,
    )
    .unwrap();
    let route = route_state(tx, "a", "ast_0000000000000001", Route::Filings)
        .unwrap()
        .unwrap();
    assert!(route.baseline_done);
    assert_eq!(route.status, "failed");

    set_paused(tx, "a", true, 40).unwrap();
    assert!(state(tx, "a").unwrap().paused);
    assert_eq!(record_check(tx, "a", "2026-10-31", 41).unwrap(), None);
    assert_eq!(
        record_check(tx, "a", "2026-11-01", 42).unwrap().as_deref(),
        Some("2026-10-31")
    );
    // Recording a check does not resume a paused Loupist.
    assert!(state(tx, "a").unwrap().paused);
}

#[test]
fn class_kinds_start_on_only_in_their_templates_loupist() {
    let crypto = class_defaults(AssetClass::Crypto, Some(AssetClass::Crypto));
    assert_eq!(crypto, WatchKind::CRYPTO.map(|kind| (kind, true)).to_vec());
    assert!(class_defaults(AssetClass::Crypto, None)
        .iter()
        .all(|(_, on)| !on));
    assert!(
        class_defaults(AssetClass::Commodity, Some(AssetClass::Crypto))
            .iter()
            .all(|(_, on)| !on)
    );
    assert!(class_defaults(AssetClass::RealAsset, Some(AssetClass::RealAsset)).is_empty());
    // A first explicit choice turns off only its own class's other kinds.
    assert_eq!(WatchKind::Tvl.class_kinds(), WatchKind::CRYPTO);
    assert_eq!(WatchKind::Flows.class_kinds(), WatchKind::EQUITY);
    // Every kind round-trips through its stored name.
    for kind in WatchKind::ALL {
        assert_eq!(WatchKind::parse(kind.as_str()), Some(kind));
        assert_eq!(serde_json::to_value(kind).unwrap(), kind.as_str());
    }
}
