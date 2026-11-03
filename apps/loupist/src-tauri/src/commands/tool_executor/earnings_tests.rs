//! `earnings_snapshot_save` refusals and commits (finance-earnings-snapshot.md
//! §3.3), with this turn's evidence staged as a real tool would stage it.
use super::*;
use crate::commands::tests::{chat, profile, seed_profile};
use crate::knowledge::{assets, earnings};

const LOUPIST: &str = "loupist-a";

fn policy(save: bool, as_of: Option<&str>) -> ToolPolicy {
    ToolPolicy {
        loupist_id: LOUPIST.into(),
        session_id: "chat-a".into(),
        use_memory: save,
        propose_memory_updates: save,
        memory_proposal_budget: 1,
        as_of: as_of.map(str::to_owned),
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
    AppToolExecutor {
        state,
        capture: Arc::new(ToolCapture::for_chat("assistant-earnings-test".into())),
        loupist_id: LOUPIST.into(),
        capability_ids: BTreeSet::new(),
    }
}

fn day(offset: i64) -> chrono::NaiveDate {
    chrono::Local::now().date_naive() + chrono::Duration::days(offset)
}

/// Morrow Components with a Q3 event `offset` days from today.
fn asset(executor: &AppToolExecutor, offset: i64) -> (String, String) {
    crate::commands::assets::with_assets(&executor.state, LOUPIST, true, |tx| {
        let page = assets::ensure_page(
            tx,
            LOUPIST,
            &assets::AssetIdentity {
                symbol: "MORR".into(),
                market: "NASDAQ".into(),
                name: "Morrow Components".into(),
            },
            None,
            1,
        )?;
        let (event, _) = earnings::upsert_event(
            tx,
            LOUPIST,
            &page.asset_id,
            "Q3 2026",
            Some(day(offset) - chrono::Duration::days(30)),
            day(offset),
            earnings::DateSource::Calendar,
            1,
        )?;
        Ok((page.asset_id, event.event_id))
    })
    .unwrap()
}

async fn result(
    executor: &AppToolExecutor,
    tool: &str,
    provider: Option<&str>,
    payload: Value,
) -> String {
    let delivery = new_id("earnings-evidence");
    let reference = executor
        .capture
        .stage_run_result_with_delivery(
            &delivery,
            RunResultProducer {
                origin: RunResultOrigin::AcquiredOriginal,
                runtime_id: "test".into(),
                tool_name: tool.into(),
                provider: provider.map(str::to_owned),
            },
            &json!({"symbol": "MORR"}),
            payload,
            vec![],
            None,
        )
        .await
        .unwrap();
    executor.capture.commit_delivery(&delivery).await;
    evidence::citation_url(&reference)
}

async fn save(
    executor: &AppToolExecutor,
    policy: &ToolPolicy,
    params: Value,
) -> Result<Value, String> {
    let delivery = new_id("earnings-save");
    match executor
        .stage_earnings_save(policy, params, &delivery)
        .await
    {
        Ok(value) => {
            executor.capture.commit_delivery(&delivery).await;
            Ok(value)
        }
        Err(error) => {
            executor.capture.discard_delivery(&delivery).await;
            Err(format!("{error:?}"))
        }
    }
}

fn guidance_row(evidence: &str) -> Value {
    json!({"measure": "revenue", "low": "1.20", "high": "1.25", "scale": 1e9, "unit": "currency", "currency": "USD", "kind": "guidance", "evidence": [evidence]})
}

#[tokio::test]
async fn expectations_need_dated_earlier_evidence_that_prints_each_value() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let (asset, event) = asset(&executor, 5);
    let release = result(
        &executor,
        "web_fetch",
        None,
        json!({"title": "Q2 results", "published_at": day(-60).to_string(), "content": "We expect third-quarter revenue of $1.20 billion to $1.25 billion."}),
    )
    .await;
    let expected = |rows: Vec<Value>| json!({"assetId": asset, "eventId": event, "side": "expected", "rows": rows});
    // Not in an as-of answer, not with saving off.
    let refused = save(
        &executor,
        &policy(true, Some(&day(-1).to_string())),
        expected(vec![guidance_row(&release)]),
    )
    .await
    .unwrap_err();
    assert!(refused.contains("earnings_as_of"), "{refused}");
    let refused = save(
        &executor,
        &policy(false, None),
        expected(vec![guidance_row(&release)]),
    )
    .await
    .unwrap_err();
    assert!(refused.contains("earnings_private"), "{refused}");
    // A value the source does not print is refused.
    let wrong = json!({"measure": "revenue", "point": "1.30", "scale": 1e9, "unit": "currency", "kind": "guidance", "evidence": [release]});
    let refused = save(&executor, &policy(true, None), expected(vec![wrong]))
        .await
        .unwrap_err();
    assert!(refused.contains("earnings_value_not_found"), "{refused}");
    // The model never sends a difference.
    let mut with_difference = guidance_row(&release);
    with_difference["difference"] = json!("+1.6% beat");
    assert!(save(
        &executor,
        &policy(true, None),
        expected(vec![with_difference])
    )
    .await
    .is_err());
    // Implied ranges come from the host's reverse DCF.
    let implied = json!({"measure": "label:Price implied growth", "low": "6.1", "high": "9.8", "unit": "percent", "kind": "implied", "evidence": [release]});
    let refused = save(&executor, &policy(true, None), expected(vec![implied]))
        .await
        .unwrap_err();
    assert!(refused.contains("earnings_implied_host"), "{refused}");
    // Consensus only from FMP or Intrinio: a scraped estimate is refused.
    let scraped = result(
        &executor,
        "equity_estimates_forward_sales",
        Some("yfinance"),
        json!({"results": [{"date": day(-1).to_string(), "mean": 1230000000.0}]}),
    )
    .await;
    let consensus = |evidence: &str| json!({"measure": "revenue", "point": "1230000000", "unit": "currency", "kind": "consensus", "evidence": [evidence]});
    let refused = save(
        &executor,
        &policy(true, None),
        expected(vec![consensus(&scraped)]),
    )
    .await
    .unwrap_err();
    assert!(refused.contains("earnings_consensus_provider"), "{refused}");
    let fmp = result(
        &executor,
        "equity_estimates_forward_sales",
        Some("fmp"),
        json!({"results": [{"date": day(-1).to_string(), "mean": 1230000000.0}]}),
    )
    .await;
    // (Evidence on or after the report date never meets an open event: expectations lock then.)
    let saved = save(
        &executor,
        &policy(true, None),
        expected(vec![guidance_row(&release), consensus(&fmp)]),
    )
    .await
    .unwrap();
    assert_eq!(saved["rows"], 2);
    // Committed with the reply: Rust stores values only, never a difference.
    let staged = executor.capture.staged_earnings.lock().await.clone();
    let stored = crate::commands::assets::with_assets(&executor.state, LOUPIST, true, |tx| {
        commit_earnings(tx, LOUPIST, "answer-1", 100, &staged, false)
    })
    .unwrap();
    assert!(stored >= 2, "{stored}");
    let rows = crate::commands::assets::with_assets(&executor.state, LOUPIST, false, |tx| {
        earnings::rows(tx, LOUPIST, &event)
    })
    .unwrap();
    let guidance = rows
        .iter()
        .find(|row| row.kind == earnings::ValueKind::Guidance)
        .unwrap();
    assert_eq!((guidance.low, guidance.high), (Some(1.2e9), Some(1.25e9)));
    assert_eq!(guidance.origin, earnings::Origin::Chat);
    assert!(rows
        .iter()
        .all(|row| !row.extra.to_string().contains("beat")));
}

#[tokio::test]
async fn reported_rows_need_evidence_dated_on_or_after_the_report_date_and_lock_expectations() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let (asset, event) = asset(&executor, -1);
    let early = result(
        &executor,
        "web_fetch",
        None,
        json!({"title": "Preview", "published_at": day(-3).to_string(), "content": "Revenue was $1.27 billion."}),
    )
    .await;
    let release = result(
        &executor,
        "web_fetch",
        None,
        json!({"title": "Q3 results", "published_at": day(-1).to_string(), "content": "Third-quarter revenue was $1.27 billion; operating margin 14.8%. We expect fourth-quarter revenue of $1.30 billion to $1.35 billion."}),
    )
    .await;
    let undated = result(
        &executor,
        "web_fetch",
        None,
        json!({"title": "Summary", "content": "Revenue was $1.27 billion."}),
    )
    .await;
    let reported = |evidence: &str| json!({"assetId": asset, "side": "reported", "rows": [{"measure": "revenue", "point": "1.27", "scale": 1e9, "unit": "currency", "kind": "reported", "evidence": [evidence]}]});
    let refused = save(&executor, &policy(true, None), reported(&early))
        .await
        .unwrap_err();
    assert!(refused.contains("earnings_reported_early"), "{refused}");
    let refused = save(&executor, &policy(true, None), reported(&undated))
        .await
        .unwrap_err();
    assert!(refused.contains("earnings_evidence_undated"), "{refused}");
    // After the report date expectations are locked.
    let refused = save(
        &executor,
        &policy(true, None),
        json!({"assetId": asset, "side": "expected", "rows": [guidance_row(&release)]}),
    )
    .await
    .unwrap_err();
    assert!(refused.contains("earnings_expected_locked"), "{refused}");
    let saved = save(&executor, &policy(true, None), reported(&release))
        .await
        .unwrap();
    assert_eq!(
        (saved["eventId"].as_str(), saved["rows"].as_i64()),
        (Some(event.as_str()), Some(1))
    );
    // New guidance names its period.
    let missing_period = json!({"assetId": asset, "side": "guidance_new", "rows": [{"measure": "revenue", "low": "1.30", "high": "1.35", "scale": 1e9, "unit": "currency", "kind": "guidance", "evidence": [release]}]});
    assert!(save(&executor, &policy(true, None), missing_period)
        .await
        .unwrap_err()
        .contains("period"));
    let guidance = json!({"assetId": asset, "side": "guidance_new", "rows": [{"measure": "revenue", "period": "Q4 2026", "low": "1.30", "high": "1.35", "scale": 1e9, "unit": "currency", "kind": "guidance", "evidence": [release]}]});
    save(&executor, &policy(true, None), guidance)
        .await
        .unwrap();
    let staged = executor.capture.staged_earnings.lock().await.clone();
    crate::commands::assets::with_assets(&executor.state, LOUPIST, true, |tx| {
        commit_earnings(tx, LOUPIST, "answer-1", 100, &staged, false)
    })
    .unwrap();
    // One reported row per measure, basis, period and stage.
    let refused = save(&executor, &policy(true, None), reported(&release))
        .await
        .unwrap_err();
    assert!(refused.contains("earnings_reported_exists"), "{refused}");
    // The call's tone comes only from a transcript.
    let tone = json!({"assetId": asset, "side": "reported", "tone": {"text": "Cautious on margins: “pricing pressure persisted”.", "spans": ["pricing pressure persisted"], "evidence": [release]}});
    let refused = save(&executor, &policy(true, None), tone)
        .await
        .unwrap_err();
    assert!(refused.contains("earnings_tone_source"), "{refused}");
    let transcript = result(
        &executor,
        "equity_fundamental_transcript",
        Some("fmp"),
        json!({"results": [{"date": day(-1).to_string(), "content": "Our CFO said pricing pressure persisted in the quarter."}]}),
    )
    .await;
    let tone = json!({"assetId": asset, "side": "reported", "tone": {"text": "Cautious on margins: “pricing pressure persisted”.", "spans": ["pricing pressure persisted"], "evidence": [transcript]}});
    save(&executor, &policy(true, None), tone).await.unwrap();
}

/// Account and balance reads stay on this computer: never a Source (D9).
#[tokio::test]
async fn an_account_read_cannot_become_a_source() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let delivery = new_id("account-read");
    let reference = executor
        .capture
        .stage_run_result_with_delivery(
            &delivery,
            RunResultProducer {
                origin: RunResultOrigin::AcquiredOriginal,
                runtime_id: "native-finance".into(),
                tool_name: "kis_read".into(),
                provider: Some("koreainvestment.market-data".into()),
            },
            &json!({"operation_id": "domestic_stock.inquire_balance"}),
            json!({"schema_version": "loupist-kis-account-result/1", "operation": "account.read", "query": {"operation_id": "domestic_stock.inquire_balance"}, "data": {"response": {"output1": [{"pdno": "990417", "hldg_qty": crate::privacy::holdings::HIDDEN}]}}}),
            vec![],
            None,
        )
        .await
        .unwrap();
    executor.capture.commit_delivery(&delivery).await;
    let error = executor
        .stage_source_save(
            &policy(true, None),
            json!({"result_ref": reference, "title": "My balance", "coverage": "included_full"}),
            "save-account",
        )
        .await
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("account_data_not_saved"),
        "{error:?}"
    );
}

/// Every account read in the shipped catalog was reviewed for holdings: the
/// listed fields hide, identity and the market price stay.
#[test]
fn every_account_read_has_a_reviewed_list_of_hidden_fields() {
    use crate::privacy::holdings::AccountReview;
    let service = crate::finance_data::FinanceDataService::new().unwrap();
    let Some(AccountReview::Account(fields)) =
        service.kis_account_review("domestic_stock.inquire_balance")
    else {
        panic!("the balance read is an account read with a review");
    };
    for hidden in [
        "hldg_qty",
        "ord_psbl_qty",
        "thdt_buyqty",
        "evlu_amt",
        "pchs_amt",
        "evlu_pfls_amt",
        "dnca_tot_amt",
        "tot_evlu_amt",
        "nass_amt",
        "pchs_avg_pric",
        "evlu_pfls_rt",
        "evlu_erng_rt",
    ] {
        assert!(
            fields.iter().any(|field| field == hidden),
            "{hidden} must be hidden"
        );
    }
    for shown in ["pdno", "prdt_name", "prpr"] {
        assert!(!fields.iter().any(|field| field == shown), "{shown} stays");
    }
    assert_eq!(
        service.kis_account_review("domestic_stock.inquire_price"),
        Some(AccountReview::Market)
    );
    let manifest: Value =
        serde_json::from_str(include_str!("../../../../marketplace/kis-read-api-v1.json")).unwrap();
    for operation in manifest["operations"].as_array().unwrap() {
        if operation["scope"] == "account" {
            let id = operation["id"].as_str().unwrap();
            assert!(
                matches!(
                    service.kis_account_review(id),
                    Some(AccountReview::Account(_))
                ),
                "{id} has no reviewed holdings list"
            );
        }
    }
}
