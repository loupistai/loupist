use super::*;
use crate::commands::knowledge::world_setup::tests::connect_fixture;
use crate::knowledge::{GraphJobView, GraphProcessingSettings, OwnedRecord, RecordKind};

/// A fresh app with a model connected. The graph supervisor is stopped so no
/// background run moves a job while the test reads it.
async fn connected(root: &std::path::Path) -> AppState {
    let state = AppState::for_folder_test(root.join("app"));
    state.graph.shutdown().await.unwrap();
    connect_fixture(&state);
    state
}

fn follow_and_hold(state: &AppState) {
    state
        .store
        .watchlist_add("hanul", "krx", Some(" Hanul Precision "))
        .unwrap();
    state
        .store
        .watchlist_add("MORROW", "NASDAQ", Some("Morrow Components"))
        .unwrap();
    let preview = state
        .store
        .preview_csv_bytes(b"symbol,market,name,quantity,cost,value,currency\nBLUEH,NYSE,Bluehaven Memory,1234.5,77.25,98765.43,USD\nMORROW,NASDAQ,Morrow Components,10,3,30,USD\n")
        .unwrap();
    state
        .store
        .holdings_commit_csv(&preview.preview_id)
        .unwrap();
}

async fn create_managed(state: &AppState) -> String {
    crate::commands::loupist::create_managed_loupist(
        state,
        crate::commands::LoupistCreateRequest {
            name: "Memory desk".into(),
            focus: None,
            avatar: None,
            template: None,
            tone: None,
        },
    )
    .await
    .unwrap()
    .id
}

type Followed = (Vec<OwnedRecord>, Vec<GraphJobView>, GraphProcessingSettings);

fn followed_state(state: &AppState, loupist_id: &str) -> Followed {
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        let sources = crate::knowledge::KnowledgeWrite::bind(tx, blobs)
            .list_records(loupist_id)?
            .into_iter()
            .filter(|record| record.kind == RecordKind::Source)
            .collect();
        Ok((
            sources,
            crate::knowledge::list_jobs(tx, loupist_id)?,
            crate::knowledge::load_settings(tx, loupist_id)?,
        ))
    })
    .unwrap()
}

fn bodies(jobs: &[GraphJobView]) -> Vec<&str> {
    jobs.iter().map(|job| job.document.body.as_str()).collect()
}

/// The status from the jobs alone, before World processing readiness.
fn job_status(state: &AppState, loupist_id: &str) -> FollowedStatus {
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs).followed_status(loupist_id)
    })
    .unwrap()
}

fn set_job_status(state: &AppState, loupist_id: &str, status: &str) {
    with_loupist_knowledge(state, loupist_id, |tx, _| {
        tx.execute(
            "UPDATE knowledge_graph_outbox SET status = ?2 WHERE loupist_id = ?1",
            rusqlite::params![loupist_id, status],
        )?;
        Ok(())
    })
    .unwrap();
}

fn library_sources(
    state: &AppState,
    loupist_id: &str,
) -> Vec<super::super::KnowledgeRecordSummaryDto> {
    super::super::catalog(
        state,
        super::super::KnowledgeCatalogRequest {
            loupist_id: loupist_id.into(),
        },
    )
    .unwrap()
    .sources
}

#[tokio::test]
async fn followed_library_preview_contains_asset_lines_without_the_model_lead() {
    let root = tempfile::tempdir().unwrap();
    let state = connected(root.path()).await;
    state
        .store
        .watchlist_add("NVDA", "NASDAQ", Some("NVIDIA"))
        .unwrap();
    let loupist_id = create_managed(&state).await;
    let sources = library_sources(&state, &loupist_id);
    assert_eq!(sources.len(), 1);
    assert_eq!(
        sources[0].objects[0].summary.as_deref(),
        Some("- NVIDIA (NVDA:NASDAQ)")
    );
    // Presentation must not alter the exact material the World worker reads.
    assert!(bodies(&followed_state(&state, &loupist_id).1)[0]
        .starts_with("Assets the user follows. Each line is one asset"));
}

#[tokio::test]
async fn followed_library_previews_each_addition_with_only_its_own_asset_lines() {
    let root = tempfile::tempdir().unwrap();
    let state = connected(root.path()).await;
    state
        .store
        .watchlist_add("NVDA", "NASDAQ", Some("NVIDIA"))
        .unwrap();
    let loupist_id = create_managed(&state).await;
    state
        .store
        .watchlist_add("005930", "KRX", Some("삼성전자"))
        .unwrap();
    assert_eq!(sync_followed_assets(&state, &loupist_id).unwrap(), 1);
    let sources = library_sources(&state, &loupist_id);
    assert_eq!(sources.len(), 2);
    let previews = sources
        .iter()
        .map(|source| {
            assert_eq!(source.title_key, Some("followed"));
            source.objects[0].summary.as_deref().unwrap()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        previews,
        std::collections::BTreeSet::from(["- NVIDIA (NVDA:NASDAQ)", "- 삼성전자 (005930:KRX)",])
    );
    // These are incremental captures, not superseded versions: keep both exact
    // Sources and ordinary jobs available to evidence reads and processing.
    let (retained, jobs, _) = followed_state(&state, &loupist_id);
    assert_eq!((retained.len(), jobs.len()), (2, 2));
    assert!(retained
        .iter()
        .any(|record| record.record_id == sources[0].record_id));
}

#[tokio::test]
async fn a_new_loupist_reads_what_the_user_follows_by_name_only() {
    let root = tempfile::tempdir().unwrap();
    let state = connected(root.path()).await;
    follow_and_hold(&state);
    let loupist_id = create_managed(&state).await;
    let (sources, jobs, settings) = followed_state(&state, &loupist_id);
    assert_eq!(
        sources
            .iter()
            .map(|record| record.title.as_str())
            .collect::<Vec<_>>(),
        ["What you follow"]
    );
    // One ordinary World job reads it; watched first, then held, each asset once.
    assert_eq!(jobs.len(), 1);
    let job = &jobs[0];
    assert_eq!(job.document.episode_kind, "source_span");
    assert_eq!(
        job.document.body,
        "Assets the user follows. Each line is one asset (a company's shares, a fund, a crypto asset, a commodity or another instrument) with its ticker in parentheses, written TICKER:MARKET when the market is known.\n- Hanul Precision (HANUL:KRX)\n- Morrow Components (MORROW:NASDAQ)\n- Bluehaven Memory (BLUEH:NYSE)\n"
    );
    for private in ["1234", "98765", "77.25", "USD", "quantity", "hold"] {
        assert!(!job.document.body.contains(private), "{private}");
    }
    assert!(job.document.source_binding.is_some());
    // Each asset is an anchor, so the extracted names resolve to one node each.
    let anchors: Vec<_> = settings
        .anchors
        .iter()
        .map(|anchor| (anchor.name.as_str(), anchor.identifiers.clone()))
        .collect();
    assert_eq!(
        anchors,
        [
            (
                "Hanul Precision",
                vec!["HANUL".to_owned(), "HANUL:KRX".to_owned()]
            ),
            (
                "Morrow Components",
                vec!["MORROW".to_owned(), "MORROW:NASDAQ".to_owned()]
            ),
            (
                "Bluehaven Memory",
                vec!["BLUEH".to_owned(), "BLUEH:NYSE".to_owned()]
            ),
        ]
    );
    assert_eq!(
        job_status(&state, &loupist_id),
        FollowedStatus {
            state: FollowedState::Queued,
            assets: 3,
            queued_at_ms: Some(job.created_at_ms),
        }
    );
}

#[tokio::test]
async fn what_the_user_follows_waits_for_a_model_and_is_skipped_when_nothing_is_followed() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(root.path().join("app"));
    state.graph.shutdown().await.unwrap();
    let empty = create_managed(&state).await;
    let (sources, jobs, settings) = followed_state(&state, &empty);
    assert!(sources.is_empty() && jobs.is_empty() && settings.anchors.is_empty());
    assert_eq!(
        followed_status(&state, &empty).unwrap().state,
        FollowedState::None
    );
    follow_and_hold(&state);
    let loupist_id = create_managed(&state).await;
    let (sources, jobs, _) = followed_state(&state, &loupist_id);
    assert_eq!(sources.len(), 1);
    assert!(jobs.is_empty(), "no model is connected yet");
    assert_eq!(
        followed_status(&state, &loupist_id).unwrap(),
        FollowedStatus {
            state: FollowedState::Waiting,
            assets: 3,
            queued_at_ms: None,
        }
    );
    connect_fixture(&state);
    crate::commands::knowledge::complete_pending_world_models(&state).unwrap();
    let (_, jobs, _) = followed_state(&state, &loupist_id);
    assert_eq!(jobs.len(), 1);
    assert!(jobs[0]
        .document
        .body
        .contains("- Hanul Precision (HANUL:KRX)\n"));
}

#[tokio::test]
async fn assets_followed_later_get_a_new_source_listing_only_them() {
    let root = tempfile::tempdir().unwrap();
    let state = connected(root.path()).await;
    follow_and_hold(&state);
    let loupist_id = create_managed(&state).await;
    // Unfollowing removes nothing, and following again adds nothing new.
    state.store.watchlist_remove("HANUL", "KRX").unwrap();
    assert_eq!(sync_followed_assets(&state, &loupist_id).unwrap(), 0);
    state.store.watchlist_add("HANUL", "KRX", None).unwrap();
    state
        .store
        .watchlist_add("BTC", "CRYPTO", Some("Bitcoin"))
        .unwrap();
    state
        .store
        .watchlist_add("005930", "KRX", Some("삼성전자"))
        .unwrap();
    assert_eq!(sync_followed_assets(&state, &loupist_id).unwrap(), 1);
    let (sources, jobs, settings) = followed_state(&state, &loupist_id);
    assert_eq!(sources.len(), 2);
    assert!(sources
        .iter()
        .all(|record| record.title == "What you follow"));
    assert_eq!(
        bodies(&jobs)[1],
        "Assets the user follows. Each line is one asset (a company's shares, a fund, a crypto asset, a commodity or another instrument) with its ticker in parentheses, written TICKER:MARKET when the market is known.\n- 삼성전자 (005930:KRX)\n- Bitcoin (BTC:CRYPTO)\n"
    );
    assert!(settings
        .anchors
        .iter()
        .any(|anchor| anchor.name == "삼성전자"
            && anchor.identifiers == ["005930".to_owned(), "005930:KRX".to_owned()]));
    // Nothing new: no Source, no job.
    assert_eq!(sync_followed_assets(&state, &loupist_id).unwrap(), 0);
    let (sources, jobs, _) = followed_state(&state, &loupist_id);
    assert_eq!((sources.len(), jobs.len()), (2, 2));
    // The status covers every "What you follow" Source.
    let status = job_status(&state, &loupist_id);
    assert_eq!((status.state, status.assets), (FollowedState::Queued, 5));
    assert_eq!(status.queued_at_ms, Some(jobs[0].created_at_ms));
}

#[tokio::test]
async fn the_status_follows_the_jobs() {
    let root = tempfile::tempdir().unwrap();
    let state = connected(root.path()).await;
    follow_and_hold(&state);
    let loupist_id = create_managed(&state).await;
    for (job, expected) in [
        ("running", FollowedState::Running),
        ("completed", FollowedState::Done),
        ("failed_before_mutation", FollowedState::Failed),
        ("reconcile_required", FollowedState::Failed),
        ("paused_budget", FollowedState::Off),
    ] {
        set_job_status(&state, &loupist_id, job);
        let status = job_status(&state, &loupist_id);
        assert_eq!(status.state, expected, "{job}");
        assert_eq!(status.queued_at_ms.is_some(), job == "running", "{job}");
    }
    // A running read wins over a finished one.
    state
        .store
        .watchlist_add("BTC", "CRYPTO", Some("Bitcoin"))
        .unwrap();
    sync_followed_assets(&state, &loupist_id).unwrap();
    assert_eq!(job_status(&state, &loupist_id).state, FollowedState::Queued);
    // A queued read that World processing cannot start now is waiting.
    with_loupist_knowledge(&state, &loupist_id, |tx, _| {
        let mut settings = crate::knowledge::load_settings(tx, &loupist_id)?;
        settings.helper_model_profile_id = Some("google/gemini-flash".into());
        crate::knowledge::store_settings(tx, &loupist_id, &settings)
    })
    .unwrap();
    assert_eq!(
        followed_status(&state, &loupist_id).unwrap().state,
        FollowedState::Waiting
    );
}

#[test]
fn a_queued_read_waits_only_on_what_the_user_can_fix() {
    for (readiness, expected) in [
        ("ready", FollowedState::Queued),
        ("runtime_unavailable", FollowedState::Queued),
        ("disabled", FollowedState::Off),
        ("paused", FollowedState::Off),
        ("model_required", FollowedState::Waiting),
        ("model_unavailable", FollowedState::Waiting),
        ("provider_disconnected", FollowedState::Waiting),
        ("reconciliation_required", FollowedState::Waiting),
    ] {
        assert_eq!(queued_state(readiness), expected, "{readiness}");
    }
}

#[tokio::test(start_paused = true)]
async fn a_burst_of_follows_makes_one_source_and_one_job_per_loupist() {
    let root = tempfile::tempdir().unwrap();
    let state = connected(root.path()).await;
    follow_and_hold(&state);
    let first = create_managed(&state).await;
    let second = create_managed(&state).await;
    for (symbol, name) in [("TSLA", "Tesla"), ("ETH", "Ethereum"), ("GC", "Gold")] {
        crate::commands::user_context::follow(&state, symbol, "NASDAQ", Some(name), None).unwrap();
        tokio::time::advance(Duration::from_secs(1)).await;
    }
    // Nothing is written while the burst goes on.
    assert_eq!(followed_state(&state, &first).1.len(), 1);
    tokio::time::sleep(QUIET).await;
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    for loupist_id in [&first, &second] {
        let (sources, jobs, _) = followed_state(&state, loupist_id);
        assert_eq!((sources.len(), jobs.len()), (2, 2), "{loupist_id}");
        assert_eq!(
            bodies(&jobs)[1],
            "Assets the user follows. Each line is one asset (a company's shares, a fund, a crypto asset, a commodity or another instrument) with its ticker in parentheses, written TICKER:MARKET when the market is known.\n- Ethereum (ETH:NASDAQ)\n- Gold (GC:NASDAQ)\n- Tesla (TSLA:NASDAQ)\n"
        );
    }
}
