//! The automatic earnings task and its captures, without a model
//! (finance-earnings-snapshot.md §8): consensus only from FMP or Intrinio, a
//! late capture without consensus, reported rows and one Thread message, a
//! failed capture's Try again, and a task that can be paused but not edited.
use super::*;
use crate::commands::{
    tests::{profile, seed_profile},
    watch::scripted::Scripted,
};
use crate::knowledge::earnings::{DateSource, EventRecord, Status};
use crate::scheduled_tasks::types::TaskStatus;

fn day(offset: i64) -> NaiveDate {
    chrono::Local::now().date_naive() + Duration::days(offset)
}

fn fixture(temporary: &tempfile::TempDir) -> AppState {
    let state = AppState::for_test(temporary.path().join("app"));
    seed_profile(
        state.store.as_ref(),
        &profile("loupist-a", &temporary.path().join("workspace"), 1),
    );
    state
        .store
        .watchlist_add("MORR", "NASDAQ", Some("Morrow Components"))
        .unwrap();
    state
}

fn morrow(state: &AppState) -> PageRecord {
    watch_commands::followed_pages(state, "loupist-a")
        .unwrap()
        .into_iter()
        .find(|page| page.symbol == "MORR")
        .unwrap()
}

/// An event `offset` days from today; reported events carry their release.
fn event(state: &AppState, page: &PageRecord, offset: i64, released: bool) -> EventRecord {
    with_assets(state, "loupist-a", true, |tx| {
        let (event, _) = earnings::upsert_event(
            tx,
            "loupist-a",
            &page.asset_id,
            "Q3 2026",
            Some(day(offset) - Duration::days(30)),
            day(offset),
            DateSource::Calendar,
            1,
        )?;
        if released {
            earnings::set_release(
                tx,
                "loupist-a",
                &event,
                &earnings::FilingRef {
                    form: Some("8-K".into()),
                    title: "Current report".into(),
                    date: day(offset).to_string(),
                    filing_id: "8k".into(),
                    evidence: AssetEvidence {
                        tool: "equity_fundamental_filings".into(),
                        title: "SEC EDGAR".into(),
                        url: Some("https://www.sec.gov/8k".into()),
                        ..AssetEvidence::default()
                    },
                },
                1,
            )?;
        }
        Ok(earnings::get_event(tx, "loupist-a", &event.event_id)?.unwrap())
    })
    .unwrap()
}

fn closes(state: &AppState, page: &PageRecord, offsets: &[i64]) {
    let rows: Vec<Value> = offsets
        .iter()
        .map(|offset| json!({"date": day(*offset).to_string(), "close": 40.0 + *offset as f64 / 10.0}))
        .collect();
    let payload = json!({"structuredContent": {"results": rows}});
    let reference = state
        .store
        .dataset_put("loupist-a", "closes", &payload, json!({}))
        .unwrap();
    let mut payload = payload;
    payload["dataset_ref"] = json!(reference);
    let found = observe::extract(
        &tool_read("equity_price_historical", None, &payload, page, Utc::now()),
        &page.symbol,
    );
    with_assets(state, "loupist-a", true, |tx| {
        assets::record_observations(
            tx,
            "loupist-a",
            &page.asset_id,
            found,
            ObservationOrigin::Refresh,
            None,
            1,
        )
    })
    .unwrap();
}

fn estimates(mean: f64, period_end: NaiveDate) -> Value {
    json!({"structuredContent": {"results": [{"date": period_end.to_string(), "fiscal_period": "Q3", "fiscal_year": 2026, "mean": mean}]}})
}

fn statements(reader: &mut Scripted) {
    reader.script_from(
        "equity_fundamental_cash",
        Some("MORR"),
        "fmp",
        json!({"structuredContent": {"results": [{"fiscal_year": 2025, "operating_cash_flow": 900000000.0, "capital_expenditure": -300000000.0}]}}),
    );
    reader.script_from(
        "equity_fundamental_balance",
        Some("MORR"),
        "fmp",
        json!({"structuredContent": {"results": [{"total_debt": 500000000.0, "cash_and_cash_equivalents": 200000000.0}]}}),
    );
    reader.script_from(
        "equity_fundamental_income",
        Some("MORR"),
        "fmp",
        json!({"structuredContent": {"results": [{"weighted_average_diluted_shares_outstanding": 250000000.0}]}}),
    );
}

fn snapshot(state: &AppState, page: &PageRecord, event: &EventRecord) -> SnapshotView {
    snapshot_for(state, "loupist-a", &page.asset_id, Some(&event.event_id))
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn a_before_capture_reads_consensus_only_from_fmp_and_prices_the_implied_growth() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary);
    let page = morrow(&state);
    let upcoming = event(&state, &page, 1, false);
    closes(&state, &page, &[-3, -2, -1]);
    let mut reader = Scripted::default();
    reader.capabilities.insert("fmp.market-data".into());
    let end = upcoming.period_end().unwrap();
    // A scraped provider never serves expectations.
    reader.script_from(
        "equity_estimates_forward_sales",
        Some("MORR"),
        "seeking_alpha",
        estimates(1.23e9, end),
    );
    reader.script_from(
        "equity_estimates_forward_eps",
        Some("MORR"),
        "fmp",
        estimates(0.42, end),
    );
    statements(&mut reader);
    let due = earnings::Due {
        event: upcoming.clone(),
        side: CaptureSide::Before,
        late_as_of: None,
    };
    capture(&state, &mut reader, "loupist-a", &page, &due, Utc::now())
        .await
        .unwrap();
    let view = snapshot(&state, &page, &upcoming);
    assert!(view.before.is_some());
    let eps = view
        .measures
        .iter()
        .find(|measure| measure.key.as_deref() == Some("eps"))
        .unwrap();
    assert_eq!(
        (eps.expected[0].kind, eps.expected[0].provider.as_deref()),
        (ValueKind::Consensus, Some("FMP"))
    );
    assert!(view
        .measures
        .iter()
        .all(|measure| measure.key.as_deref() != Some("revenue")));
    let implied = view.implied.expect("implied range");
    // The newest completed close on or before the day before the report.
    let newest = with_assets(&state, "loupist-a", false, |tx| {
        earnings::closes(tx, "loupist-a", &page.asset_id)
    })
    .unwrap()
    .last()
    .unwrap()
    .date;
    assert_eq!(implied.close_date, newest.to_string());
    assert!(newest < upcoming.report_date);
    assert!(implied.low_pct < implied.high_pct, "{implied:?}");
    assert!(implied
        .calc
        .formulas
        .iter()
        .any(|line| line.contains("solve g")));
    // Open the grid names the year of the cash flow, as an answer's card does.
    let grid = implied.grid.as_ref().expect("the whole grid");
    let cash_flow = grid
        .inputs
        .iter()
        .find(|input| input.name == "cash_flow")
        .unwrap();
    assert_eq!(cash_flow.label.as_deref(), Some("free cash flow FY2025"));
    // A successful before-capture posts nothing in the Thread.
    crate::chat::home::deliver_finished_runs(&state, "loupist-a").unwrap();
    let home = crate::chat::home::ensure_home_thread(&state, "loupist-a").unwrap();
    assert!(home
        .messages
        .iter()
        .all(|message| message.earnings.is_none()));
}

#[tokio::test]
async fn a_late_capture_leaves_consensus_out_and_without_a_key_the_card_says_what_it_needs() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary);
    let page = morrow(&state);
    let reported = event(&state, &page, -1, true);
    closes(&state, &page, &[-4, -3, -2]);
    let mut reader = Scripted::default();
    reader.capabilities.insert("fmp.market-data".into());
    let end = reported.period_end().unwrap();
    reader.script_from(
        "equity_estimates_forward_sales",
        Some("MORR"),
        "fmp",
        estimates(1.23e9, end),
    );
    reader.script_from(
        "equity_estimates_forward_eps",
        Some("MORR"),
        "fmp",
        estimates(0.42, end),
    );
    statements(&mut reader);
    let due = earnings::Due {
        event: reported.clone(),
        side: CaptureSide::Before,
        late_as_of: Some(day(-2)),
    };
    capture(&state, &mut reader, "loupist-a", &page, &due, Utc::now())
        .await
        .unwrap();
    assert_eq!(reader.calls_to("equity_estimates_forward_sales"), 0);
    let view = snapshot(&state, &page, &reported);
    assert!(view.consensus_late);
    assert_eq!(
        view.before.as_ref().unwrap().late_as_of.as_deref(),
        Some(day(-2).to_string().as_str())
    );
    // The implied range uses a close on or before the day before the report.
    assert!(view.implied.as_ref().unwrap().close_date.as_str() <= day(-2).to_string().as_str());
    // Written once: a second late capture finds them locked.
    let again = capture(&state, &mut reader, "loupist-a", &page, &due, Utc::now())
        .await
        .unwrap();
    assert!(matches!(again, RunSummaryItem::Expected { count: 0, .. }));

    // Without FMP or Intrinio the card says consensus needs a key.
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary);
    let page = morrow(&state);
    let upcoming = event(&state, &page, 3, false);
    let mut reader = Scripted::default();
    let due = earnings::Due {
        event: upcoming.clone(),
        side: CaptureSide::Before,
        late_as_of: None,
    };
    capture(&state, &mut reader, "loupist-a", &page, &due, Utc::now())
        .await
        .unwrap();
    let view = snapshot(&state, &page, &upcoming);
    assert!(view.consensus_needs_key && view.no_guidance && view.measures.is_empty());
}

#[tokio::test]
async fn reported_figures_arrive_after_the_release_and_the_thread_hears_once() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary);
    let page = morrow(&state);
    let reported = event(&state, &page, -1, true);
    with_assets(&state, "loupist-a", true, |tx| {
        earnings::insert_rows(
            tx,
            "loupist-a",
            &reported.event_id,
            &[NewRow {
                side: Side::Expected,
                measure: "revenue".into(),
                basis: String::new(),
                period: "Q3 2026".into(),
                low: Some("1200000000".into()),
                high: Some("1250000000".into()),
                point: None,
                unit: Unit::Currency,
                currency: Some("USD".into()),
                kind: ValueKind::Guidance,
                stage: None,
                provider: None,
                evidence: vec![AssetEvidence {
                    tool: "web_fetch".into(),
                    title: "Q2 release".into(),
                    url: Some("https://example.test/q2".into()),
                    ..AssetEvidence::default()
                }],
                extra: json!({}),
                fact_date: day(-90),
                captured_as_of: None,
            }],
            Origin::Chat,
            None,
            1,
        )
    })
    .unwrap();
    let mut reader = Scripted::default();
    let end = reported.period_end().unwrap();
    reader.script_from(
        "equity_fundamental_income",
        Some("MORR"),
        "fmp",
        json!({"structuredContent": {"results": [{"period_ending": end.to_string(), "revenue": 1270000000.0, "operating_income": 188000000.0, "net_income": 120000000.0, "diluted_earnings_per_share": 0.48, "reported_currency": "USD", "filing_date": day(-1).to_string()}]}}),
    );
    let due = earnings::Due {
        event: reported.clone(),
        side: CaptureSide::After,
        late_as_of: None,
    };
    capture(&state, &mut reader, "loupist-a", &page, &due, Utc::now())
        .await
        .unwrap();
    let view = snapshot(&state, &page, &reported);
    let revenue = view
        .measures
        .iter()
        .find(|measure| measure.key.as_deref() == Some("revenue"))
        .unwrap();
    assert_eq!(revenue.reported[0].stage, Some(Stage::Preliminary));
    assert!(matches!(
        revenue.differences[0],
        diff::DifferenceView::Range {
            position: diff::Position::Above,
            ..
        }
    ));
    assert!(view.after.is_some());
    crate::chat::home::deliver_finished_runs(&state, "loupist-a").unwrap();
    let home = crate::chat::home::ensure_home_thread(&state, "loupist-a").unwrap();
    let posted: Vec<_> = home
        .messages
        .iter()
        .filter_map(|message| message.earnings.as_ref())
        .collect();
    assert_eq!(posted.len(), 1);
    assert_eq!(
        (posted[0].outcome.as_str(), posted[0].period.as_str()),
        ("reported", "Q3 2026")
    );
    assert!(!posted[0].measures.is_empty() && posted[0].measures.len() <= 2);
    // The final report's capture adds rows but no second message.
    let final_event = with_assets(&state, "loupist-a", true, |tx| {
        let current = earnings::get_event(tx, "loupist-a", &reported.event_id)?.unwrap();
        earnings::set_final(
            tx,
            "loupist-a",
            &current,
            &earnings::FilingRef {
                form: Some("10-Q".into()),
                title: "Quarterly report".into(),
                date: day(0).to_string(),
                filing_id: "10q".into(),
                evidence: AssetEvidence {
                    tool: "equity_fundamental_filings".into(),
                    title: "SEC EDGAR".into(),
                    url: Some("https://www.sec.gov/10q".into()),
                    ..AssetEvidence::default()
                },
            },
            2,
        )?;
        Ok(earnings::get_event(tx, "loupist-a", &reported.event_id)?.unwrap())
    })
    .unwrap();
    let due = earnings::Due {
        event: final_event.clone(),
        side: CaptureSide::Final,
        late_as_of: None,
    };
    capture(&state, &mut reader, "loupist-a", &page, &due, Utc::now())
        .await
        .unwrap();
    let view = snapshot(&state, &page, &final_event);
    assert!(view.unchanged, "{:?}", view.revisions);
    crate::chat::home::deliver_finished_runs(&state, "loupist-a").unwrap();
    let home = crate::chat::home::ensure_home_thread(&state, "loupist-a").unwrap();
    assert_eq!(
        home.messages
            .iter()
            .filter(|message| message.earnings.is_some())
            .count(),
        1
    );
}

#[tokio::test]
async fn a_failed_before_capture_keeps_the_card_and_offers_try_again() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary);
    let page = morrow(&state);
    let upcoming = event(&state, &page, 1, false);
    closes(&state, &page, &[-1]);
    let mut reader = Scripted {
        offline: true,
        ..Scripted::default()
    };
    reader.capabilities.insert("fmp.market-data".into());
    let due = earnings::Due {
        event: upcoming.clone(),
        side: CaptureSide::Before,
        late_as_of: None,
    };
    assert!(
        capture(&state, &mut reader, "loupist-a", &page, &due, Utc::now())
            .await
            .is_err()
    );
    let view = snapshot(&state, &page, &upcoming);
    assert_eq!(
        view.failed.as_ref().map(|failed| failed.side.as_str()),
        Some("before")
    );
    assert!(view.before.is_none());
    crate::chat::home::deliver_finished_runs(&state, "loupist-a").unwrap();
    let home = crate::chat::home::ensure_home_thread(&state, "loupist-a").unwrap();
    let failed: Vec<_> = home
        .messages
        .iter()
        .filter_map(|message| message.earnings.as_ref())
        .collect();
    assert_eq!(
        (
            failed.len(),
            failed[0].outcome.as_str(),
            failed[0].side.as_deref()
        ),
        (1, "failed", Some("before"))
    );
    // Due again after an hour, not at once.
    let current = with_assets(&state, "loupist-a", false, |tx| {
        earnings::get_event(tx, "loupist-a", &upcoming.event_id)
    })
    .unwrap()
    .unwrap();
    let (due_now, _) = earnings::due(std::slice::from_ref(&current), "NASDAQ", Utc::now());
    assert!(due_now.is_empty());
    let later = std::cmp::max(
        Utc::now() + Duration::minutes(61),
        earnings::capture_at(&current, "NASDAQ") + Duration::minutes(1),
    );
    let (due_later, _) = earnings::due(std::slice::from_ref(&current), "NASDAQ", later);
    assert_eq!(due_later.len(), 1);
}

#[tokio::test]
async fn the_results_task_is_automatic_and_can_be_paused_but_not_edited_or_deleted() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary);
    // Without proactive consent it waits, paused.
    ensure_task(&state, "loupist-a", false).unwrap();
    let task = state
        .store
        .scheduled_task_get("loupist-a", &task_id("loupist-a"))
        .unwrap()
        .unwrap();
    assert_eq!(
        (task.kind, task.status),
        (TaskKind::Earnings, TaskStatus::Paused)
    );
    // Consent turns the untouched task on; a second ensure changes nothing.
    ensure_task(&state, "loupist-a", true).unwrap();
    ensure_task(&state, "loupist-a", false).unwrap();
    let tasks = state.store.scheduled_tasks_list("loupist-a").unwrap();
    let earnings_tasks: Vec<_> = tasks
        .iter()
        .filter(|task| task.kind == TaskKind::Earnings)
        .collect();
    assert_eq!(earnings_tasks.len(), 1);
    assert_eq!(earnings_tasks[0].status, TaskStatus::Active);
    let task = earnings_tasks[0].clone();
    assert!(
        crate::scheduled_tasks::service::delete(&state, "loupist-a", &task.id, task.revision)
            .is_err()
    );
    assert!(crate::scheduled_tasks::service::update(
        &state,
        "loupist-a",
        &task.id,
        task.revision,
        task.draft()
    )
    .is_err());
    let paused = crate::scheduled_tasks::service::set_status(
        &state,
        "loupist-a",
        &task.id,
        task.revision,
        TaskStatus::Paused,
    )
    .unwrap();
    assert_eq!(paused.status, TaskStatus::Paused);
    // Resuming needs no model: the task makes no model call.
    let resumed = crate::scheduled_tasks::service::set_status(
        &state,
        "loupist-a",
        &task.id,
        paused.revision,
        TaskStatus::Active,
    )
    .unwrap();
    assert_eq!(resumed.status, TaskStatus::Active);
    // A failed run never pauses it.
    let (claim, ()) = state
        .store
        .scheduled_task_claim_with("loupist-a", &task.id, now_ms(), true, |_| Ok(()))
        .unwrap()
        .unwrap();
    // Its history is fields the renderer words, never English lines to parse (B99).
    let items = vec![
        RunSummaryItem::Reported {
            name: "Morrow Components".into(),
            period: "Q3 2026".into(),
            count: 2,
        },
        RunSummaryItem::Failed {
            name: "Solsup Foods".into(),
            period: "Q3 2026".into(),
        },
    ];
    state
        .store
        .scheduled_run_summary("loupist-a", &claim.run.run_id, items.clone())
        .unwrap();
    let stored = state.store.inbox_runs("loupist-a", None, 10).unwrap();
    let run = stored
        .iter()
        .find(|run| run.run_id == claim.run.run_id)
        .unwrap();
    assert_eq!(run.summary_items, items);
    assert!(run.summary.is_empty());
    assert_eq!(
        serde_json::to_value(&run.summary_items).unwrap(),
        json!([
            {"kind": "earnings_reported", "name": "Morrow Components", "period": "Q3 2026", "count": 2},
            {"kind": "earnings_failed", "name": "Solsup Foods", "period": "Q3 2026"},
        ])
    );
    // Items past their bounds are refused rather than stored.
    let unnamed = vec![RunSummaryItem::Failed {
        name: " ".into(),
        period: "Q3 2026".into(),
    }];
    assert!(state
        .store
        .scheduled_run_summary("loupist-a", &claim.run.run_id, unnamed)
        .is_err());
    assert!(state
        .store
        .scheduled_run_summary(
            "loupist-a",
            &claim.run.run_id,
            vec![RunSummaryItem::Nothing; 17]
        )
        .is_err());
    state
        .store
        .scheduled_task_finish(
            "loupist-a",
            &claim.run.run_id,
            RunStatus::Failed,
            Some("no source answered"),
            now_ms(),
        )
        .unwrap();
    let after = state
        .store
        .scheduled_task_get("loupist-a", &task.id)
        .unwrap()
        .unwrap();
    assert_eq!(after.status, TaskStatus::Active);
    // Its run lives in the home Thread and is never delivered as a generic result.
    assert_eq!(
        claim.run.thread_id.as_deref(),
        Some(crate::chat::home::home_thread_id("loupist-a").as_str())
    );
    let _ = Status::Upcoming;
}
