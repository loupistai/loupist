//! Watchers without a model (docs/knowledge/watchers.md#local-watchers): detectors over scripted
//! structured rows, baselines, fan-out, pause, failures, delivery and the
//! briefing's watch rows.
use super::scripted::Scripted;
use super::*;
use crate::commands::tests::{profile, seed_profile};
use crate::scheduled_tasks::types::{Schedule, TaskDraft, TaskKind, TaskStatus, TaskTarget};

fn day(offset: i64) -> NaiveDate {
    chrono::Local::now().date_naive() + Duration::days(offset)
}

fn fixture(temporary: &tempfile::TempDir, loupists: &[&str]) -> AppState {
    let state = AppState::for_test(temporary.path().join("app"));
    for (index, id) in loupists.iter().enumerate() {
        seed_profile(
            state.store.as_ref(),
            &profile(id, &temporary.path().join(format!("workspace-{index}")), 1),
        );
        consent_on(&state, id);
    }
    state
}

/// The morning update is on: proactive consent.
fn consent_on(state: &AppState, loupist_id: &str) {
    state
        .store
        .scheduled_task_create_kind(
            loupist_id,
            &format!("briefing-{loupist_id}"),
            TaskDraft {
                title: "Briefings".into(),
                prompt: "brief".into(),
                schedule: Schedule::Weekly {
                    days: vec![1, 2, 3, 4, 5],
                    hour: 8,
                    minute: 0,
                    timezone: "Asia/Seoul".into(),
                },
                target: TaskTarget::Dedicated,
                model_profile_id: String::new(),
                thinking_level: String::new(),
                run_options: Default::default(),
                use_memory: true,
                update_memory: true,
                status: TaskStatus::Active,
            },
            TaskKind::Briefing,
            now_ms(),
        )
        .unwrap();
}

fn follow(state: &AppState, symbol: &str, market: &str, name: &str) {
    state
        .store
        .watchlist_add(symbol, market, Some(name))
        .unwrap();
}

fn page_id(symbol: &str, market: &str) -> String {
    assets::asset_id(symbol, market)
}

fn defaults(state: &AppState, loupist_id: &str) {
    on_consent(state, loupist_id).unwrap();
}

fn enable(state: &AppState, loupist_id: &str, asset_id: &str, kind: WatchKind) {
    let revision = with_assets(state, loupist_id, false, |tx| {
        Ok(watch::switches(tx, loupist_id)?
            .get(asset_id)
            .map_or(1, |switch| switch.revision))
    })
    .unwrap();
    with_assets(state, loupist_id, true, |tx| {
        watch::set_kind(tx, loupist_id, asset_id, kind, true, revision, now_ms())
    })
    .unwrap();
}

fn sec_row(form: &str, date: NaiveDate, accession: &str, title: &str) -> Value {
    json!({"filing_date": date.to_string(), "report_type": form, "report_url": format!("https://www.sec.gov/Archives/edgar/data/990001/{accession}.htm"), "accession_number": accession, "primary_doc_description": title})
}

fn sec(rows: Vec<Value>) -> Value {
    json!({"structuredContent": {"provider": "sec", "results": rows}})
}

fn dart(rows: &[(&str, NaiveDate, &str)]) -> Value {
    json!({"data": {"filings": rows.iter().map(|(id, date, title)| json!({"filing_id": id, "filed_at": date.format("%Y%m%d").to_string(), "title": title})).collect::<Vec<_>>()}})
}

async fn check(
    state: &AppState,
    runtime: &Runtime,
    loupist_id: &str,
    reader: &mut Scripted,
) -> usize {
    run_pass_with(
        state,
        runtime,
        Scope::Loupist(loupist_id.into()),
        Utc::now(),
        reader,
    )
    .await
    .unwrap()
    .get(loupist_id)
    .copied()
    .unwrap_or_default()
}

fn events(state: &AppState, loupist_id: &str) -> Vec<WatchEventView> {
    with_assets(state, loupist_id, false, |tx| {
        watch::list_events(tx, loupist_id, None, 100)
    })
    .unwrap()
}

#[tokio::test]
async fn a_new_filing_fires_once_after_the_baseline_and_routes_to_the_most_specific_kind() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary, &["loupist-a"]);
    follow(&state, "MORR", "NASDAQ", "Morrow Components");
    defaults(&state, "loupist-a");
    let morrow = page_id("MORR", "NASDAQ");
    enable(&state, "loupist-a", &morrow, WatchKind::Holders);
    let runtime = Runtime::default();
    let mut reader = Scripted::default();
    let tenk = sec_row("10-K", day(-60), "0000990001-26-000004", "Annual report");
    reader.script(
        "equity_fundamental_filings",
        Some("MORR"),
        sec(vec![tenk.clone()]),
    );
    // The route's first check is its starting point: nothing fires.
    assert_eq!(check(&state, &runtime, "loupist-a", &mut reader).await, 0);
    assert!(events(&state, "loupist-a").is_empty());
    let tenq = sec_row("10-Q", day(-2), "0000990001-26-000011", "Quarterly report");
    let schedule13d = sec_row(
        "SC 13D",
        day(-1),
        "0000990001-26-000012",
        "Beneficial ownership",
    );
    let form4 = sec_row("4", day(-1), "0000990001-26-000013", "Statement of changes");
    reader.script(
        "equity_fundamental_filings",
        Some("MORR"),
        sec(vec![tenq, schedule13d, form4, tenk]),
    );
    assert_eq!(check(&state, &runtime, "loupist-a", &mut reader).await, 2);
    let found = events(&state, "loupist-a");
    let kinds: Vec<(WatchKind, String)> = found
        .iter()
        .map(|event| {
            (
                event.kind,
                event.facts["form"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    assert!(
        kinds.contains(&(WatchKind::Filings, "10-Q".into())),
        "{kinds:?}"
    );
    // A 13D is a big holder, not a new filing; a Form 4 fires nothing here.
    assert!(
        kinds.contains(&(WatchKind::Holders, "SC 13D".into())),
        "{kinds:?}"
    );
    assert_eq!(found.len(), 2, "{kinds:?}");
    let filing = found
        .iter()
        .find(|event| event.kind == WatchKind::Filings)
        .unwrap();
    assert_eq!(filing.fact_date, day(-2).to_string());
    assert_eq!(filing.name, "Morrow Components");
    assert!(filing.evidence[0]
        .url
        .as_deref()
        .is_some_and(|url| url.contains("0000990001-26-000011")));
    // The same rows again fire nothing.
    assert_eq!(check(&state, &runtime, "loupist-a", &mut reader).await, 0);
    assert_eq!(events(&state, "loupist-a").len(), 2);
}

#[tokio::test]
async fn three_buyers_fire_insider_buying_and_two_do_not() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary, &["loupist-a"]);
    follow(&state, "MORR", "NASDAQ", "Morrow Components");
    defaults(&state, "loupist-a");
    let morrow = page_id("MORR", "NASDAQ");
    enable(&state, "loupist-a", &morrow, WatchKind::Insiders);
    let runtime = Runtime::default();
    let mut reader = Scripted::default();
    reader.script("equity_fundamental_filings", Some("MORR"), sec(vec![]));
    let buy = |owner: &str, offset: i64| json!({"owner_name": owner, "transaction_date": day(offset).to_string(), "transaction_code": "P", "filing_url": "https://www.sec.gov/form4"});
    reader.script_from(
        "equity_ownership_insider_trading",
        Some("MORR"),
        "sec",
        json!({"structuredContent": {"results": [buy("Old Holder", -90)]}}),
    );
    check(&state, &runtime, "loupist-a", &mut reader).await;
    reader.script_from(
        "equity_ownership_insider_trading",
        Some("MORR"),
        "sec",
        json!({"structuredContent": {"results": [buy("A", -9), buy("B", -4), buy("Old Holder", -90)]}}),
    );
    assert_eq!(check(&state, &runtime, "loupist-a", &mut reader).await, 0);
    reader.script_from(
        "equity_ownership_insider_trading",
        Some("MORR"),
        "sec",
        json!({"structuredContent": {"results": [buy("A", -9), buy("B", -4), buy("C", -1)]}}),
    );
    assert_eq!(check(&state, &runtime, "loupist-a", &mut reader).await, 1);
    let found = events(&state, "loupist-a");
    assert_eq!(found[0].kind, WatchKind::Insiders);
    assert_eq!(found[0].facts["count"], 3);
    assert_eq!(found[0].facts["to"], day(-1).to_string());
    // A fourth buyer in the same window continues the cluster: no new alert.
    reader.script_from(
        "equity_ownership_insider_trading",
        Some("MORR"),
        "sec",
        json!({"structuredContent": {"results": [buy("A", -9), buy("B", -4), buy("C", -1), buy("D", 0)]}}),
    );
    assert_eq!(check(&state, &runtime, "loupist-a", &mut reader).await, 0);
    // Insider reads come only from providers whose rows name buyers.
    let mut scraped = Scripted::default();
    scraped.script("equity_fundamental_filings", Some("MORR"), sec(vec![]));
    scraped.script_from(
        "equity_ownership_insider_trading",
        Some("MORR"),
        "yfinance",
        json!({"structuredContent": {"results": []}}),
    );
    check(&state, &runtime, "loupist-a", &mut scraped).await;
    let view = settings_view(&state, &runtime, "loupist-a").unwrap();
    let insiders = view.watched[0]
        .kinds
        .iter()
        .find(|kind| kind.kind == WatchKind::Insiders)
        .unwrap();
    assert_eq!(
        insiders
            .needs_connection
            .as_ref()
            .map(|connection| connection.id.as_str()),
        Some("fmp.market-data")
    );
}

#[tokio::test]
async fn korean_titles_route_by_kind_and_an_ir_date_becomes_a_results_alert() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary, &["loupist-a"]);
    follow(&state, "990221", "KRX", "Solsup Foods");
    defaults(&state, "loupist-a");
    let solsup = page_id("990221", "KRX");
    for kind in [WatchKind::Insiders, WatchKind::Holders] {
        enable(&state, "loupist-a", &solsup, kind);
    }
    let runtime = Runtime::default();
    let mut reader = Scripted::default();
    reader.script(
        "dart_filing_search",
        Some("990221"),
        dart(&[("20250101000001", day(-200), "사업보고서 (2024.12)")]),
    );
    check(&state, &runtime, "loupist-a", &mut reader).await;
    let ir_day = day(3);
    reader.script(
        "dart_filing_search",
        Some("990221"),
        dart(&[
            (
                "20260101000002",
                day(-12),
                "임원ㆍ주요주주특정증권등소유상황보고서",
            ),
            (
                "20260101000003",
                day(-6),
                "임원ㆍ주요주주특정증권등소유상황보고서",
            ),
            (
                "20260101000004",
                day(-2),
                "임원ㆍ주요주주특정증권등소유상황보고서",
            ),
            (
                "20260101000005",
                day(-1),
                "주식등의대량보유상황보고서(일반)",
            ),
            ("20260101000006", day(-1), "기업설명회(IR)개최(안내공시)"),
            ("20260101000007", day(-1), "주요사항보고서(유상증자결정)"),
            ("20250101000001", day(-200), "사업보고서 (2024.12)"),
        ]),
    );
    reader.script(
        "dart_filing_read",
        None,
        json!({"data": {"document": {"text": format!("1. 개최일시 : {} 16:00\n2. 장소 : 서울", ir_day.format("%Y년 %m월 %d일"))}}}),
    );
    check(&state, &runtime, "loupist-a", &mut reader).await;
    let found = events(&state, "loupist-a");
    let kind_of = |kind: WatchKind| {
        found
            .iter()
            .filter(|event| event.kind == kind)
            .collect::<Vec<_>>()
    };
    assert_eq!(kind_of(WatchKind::Insiders).len(), 1, "{found:?}");
    assert_eq!(kind_of(WatchKind::Insiders)[0].facts["buyNotChecked"], true);
    assert_eq!(kind_of(WatchKind::Holders).len(), 1, "{found:?}");
    // Only the 유상증자 report is a plain new filing; the IR notice is not.
    let filings = kind_of(WatchKind::Filings);
    assert_eq!(filings.len(), 1, "{found:?}");
    assert!(filings[0].facts["title"]
        .as_str()
        .unwrap()
        .contains("유상증자"));
    let results = kind_of(WatchKind::Results);
    assert_eq!(results.len(), 1, "{found:?}");
    assert_eq!(results[0].facts["date"], ir_day.to_string());
    let event_id = results[0].facts["eventId"].as_str().unwrap().to_owned();
    let event = with_assets(&state, "loupist-a", false, |tx| {
        earnings::get_event(tx, "loupist-a", &event_id)
    })
    .unwrap()
    .unwrap();
    assert_eq!(event.date_source, earnings::DateSource::IrNotice);
    assert_eq!(reader.calls_to("dart_filing_read"), 1);
    // The notice is read once.
    check(&state, &runtime, "loupist-a", &mut reader).await;
    assert_eq!(reader.calls_to("dart_filing_read"), 1);
}

#[tokio::test]
async fn korean_flows_turn_once_and_record_dates_alert_within_seven_days() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary, &["loupist-a"]);
    follow(&state, "990221", "KRX", "Solsup Foods");
    follow(&state, "MORR", "NASDAQ", "Morrow Components");
    defaults(&state, "loupist-a");
    let solsup = page_id("990221", "KRX");
    enable(&state, "loupist-a", &solsup, WatchKind::Flows);
    let runtime = Runtime::default();
    let mut reader = Scripted::default();
    reader.script("dart_filing_search", Some("990221"), dart(&[]));
    reader.script("equity_fundamental_filings", Some("MORR"), sec(vec![]));
    let flows = |recent: f64, prior: f64| {
        let rows: Vec<Value> = (0..20)
            .map(|index| {
                let date = day(-1 - index);
                let net = if index < 10 { recent } else { prior };
                json!({"stck_bsop_date": date.format("%Y%m%d").to_string(), "frgn_ntby_tr_pbmn": format!("{}", net / 2.0), "orgn_ntby_tr_pbmn": format!("{}", net / 2.0), "acml_tr_pbmn": "5000"})
            })
            .collect();
        json!({"data": {"response": {"output2": rows}}})
    };
    reader.script("kis_read", Some("990221"), flows(-60.0, -50.0));
    let dividends = json!({"structuredContent": {"results": [
        {"ex_dividend_date": day(-400).to_string(), "amount": 0.2, "record_date": day(-399).to_string()},
    ]}});
    reader.script_from(
        "equity_fundamental_dividends",
        Some("MORR"),
        "nasdaq",
        dividends,
    );
    check(&state, &runtime, "loupist-a", &mut reader).await;
    assert!(events(&state, "loupist-a").is_empty());
    // Buying turns positive by more than 1% of traded value.
    reader.script("kis_read", Some("990221"), flows(60.0, -50.0));
    reader.script_from(
        "equity_fundamental_dividends",
        Some("MORR"),
        "nasdaq",
        json!({"structuredContent": {"results": [
            {"ex_dividend_date": day(-400).to_string(), "amount": 0.2, "record_date": day(-399).to_string()},
            {"ex_dividend_date": day(4).to_string(), "amount": 0.25, "record_date": day(5).to_string(), "declaration_date": day(-1).to_string(), "currency": "USD"},
        ]}}),
    );
    check(&state, &runtime, "loupist-a", &mut reader).await;
    let found = events(&state, "loupist-a");
    let flows_events: Vec<_> = found
        .iter()
        .filter(|event| event.kind == WatchKind::Flows)
        .collect();
    assert_eq!(flows_events.len(), 1, "{found:?}");
    assert_eq!(flows_events[0].facts["direction"], "in");
    let dividends: Vec<_> = found
        .iter()
        .filter(|event| event.kind == WatchKind::Dividends)
        .collect();
    // The declaration and the record date five days away.
    assert_eq!(dividends.len(), 2, "{found:?}");
    assert!(dividends
        .iter()
        .any(|event| event.facts["recordDate"] == day(5).to_string()));
    // Still positive the next day: no second alert.
    check(&state, &runtime, "loupist-a", &mut reader).await;
    assert_eq!(
        events(&state, "loupist-a")
            .iter()
            .filter(|event| event.kind == WatchKind::Flows)
            .count(),
        1
    );
}

#[tokio::test]
async fn one_read_serves_two_loupists_and_a_paused_one_hears_nothing() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary, &["loupist-a", "loupist-b"]);
    follow(&state, "MORR", "NASDAQ", "Morrow Components");
    defaults(&state, "loupist-a");
    defaults(&state, "loupist-b");
    let runtime = Runtime::default();
    let mut reader = Scripted::default();
    let tenk = sec_row("10-K", day(-60), "0000990001-26-000004", "Annual report");
    reader.script(
        "equity_fundamental_filings",
        Some("MORR"),
        sec(vec![tenk.clone()]),
    );
    Box::pin(run_pass_with(
        &state,
        &runtime,
        Scope::Scheduled,
        Utc::now(),
        &mut reader,
    ))
    .await
    .unwrap();
    // One filings read for both Loupists (the calendar is a separate route).
    assert_eq!(reader.calls_to("equity_fundamental_filings"), 1);
    // Each Loupist's copy cites its own dataset.
    let tenq = sec_row("10-Q", day(-2), "0000990001-26-000011", "Quarterly report");
    reader.script(
        "equity_fundamental_filings",
        Some("MORR"),
        sec(vec![tenq, tenk]),
    );
    let alerts = Box::pin(run_pass_with(
        &state,
        &runtime,
        Scope::Scheduled,
        Utc::now() + Duration::hours(2),
        &mut reader,
    ))
    .await
    .unwrap();
    assert_eq!(reader.calls_to("equity_fundamental_filings"), 2);
    assert_eq!((alerts["loupist-a"], alerts["loupist-b"]), (1, 1));
    let a = &events(&state, "loupist-a")[0];
    let b = &events(&state, "loupist-b")[0];
    assert_ne!(
        a.evidence[0]
            .dataset
            .as_ref()
            .map(|dataset| dataset.dataset_id.clone()),
        b.evidence[0]
            .dataset
            .as_ref()
            .map(|dataset| dataset.dataset_id.clone())
    );
    // Paused: Loupist B is neither read for nor told.
    with_assets(&state, "loupist-b", true, |tx| {
        watch::set_paused(tx, "loupist-b", true, now_ms())
    })
    .unwrap();
    let earlier_calls = reader.calls.len();
    let eightk = sec_row("8-K", day(-1), "0000990001-26-000020", "Current report");
    reader.script(
        "equity_fundamental_filings",
        Some("MORR"),
        sec(vec![
            eightk,
            sec_row("10-Q", day(-2), "0000990001-26-000011", "Quarterly report"),
        ]),
    );
    let alerts = Box::pin(run_pass_with(
        &state,
        &runtime,
        Scope::Scheduled,
        Utc::now() + Duration::hours(4),
        &mut reader,
    ))
    .await
    .unwrap();
    assert_eq!(alerts.get("loupist-a"), Some(&1));
    assert!(!alerts.contains_key("loupist-b"));
    assert_eq!(events(&state, "loupist-b").len(), 1);
    assert!(reader.calls[earlier_calls..]
        .iter()
        .all(|(loupist, ..)| loupist == "loupist-a"));
    // Without consent nothing is watched.
    let view = settings_view(&state, &runtime, "loupist-b").unwrap();
    assert!(view.paused && !view.consent_off);
}

#[tokio::test]
async fn a_failed_read_keeps_the_last_good_state_and_says_which_connection() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary, &["loupist-a"]);
    follow(&state, "MORR", "NASDAQ", "Morrow Components");
    defaults(&state, "loupist-a");
    let runtime = Runtime::default();
    let mut reader = Scripted::default();
    let tenk = sec_row("10-K", day(-60), "0000990001-26-000004", "Annual report");
    reader.script("equity_fundamental_filings", Some("MORR"), sec(vec![tenk]));
    check(&state, &runtime, "loupist-a", &mut reader).await;
    reader.offline = true;
    assert_eq!(check(&state, &runtime, "loupist-a", &mut reader).await, 0);
    let view = settings_view(&state, &runtime, "loupist-a").unwrap();
    let filings = view.watched[0]
        .kinds
        .iter()
        .find(|kind| kind.kind == WatchKind::Filings)
        .unwrap();
    assert_eq!(
        filings
            .failed
            .as_ref()
            .map(|failed| failed.connection.as_str()),
        Some("Market data platform")
    );
    // The baseline survived: back online, the old 10-K still fires nothing.
    reader.offline = false;
    assert_eq!(check(&state, &runtime, "loupist-a", &mut reader).await, 0);
    let route = with_assets(&state, "loupist-a", false, |tx| {
        watch::route_state(tx, "loupist-a", &page_id("MORR", "NASDAQ"), Route::Filings)
    })
    .unwrap()
    .unwrap();
    assert!(route.baseline_done && route.status == "ok");
}

#[tokio::test]
async fn the_first_check_goes_to_the_morning_and_a_thread_message_shows_five_then_more() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary, &["loupist-a"]);
    follow(&state, "MORR", "NASDAQ", "Morrow Components");
    defaults(&state, "loupist-a");
    let runtime = Runtime::default();
    let mut reader = Scripted::default();
    let tenk = sec_row("10-K", day(-60), "0000990001-26-000004", "Annual report");
    reader.script(
        "equity_fundamental_filings",
        Some("MORR"),
        sec(vec![tenk.clone()]),
    );
    check(&state, &runtime, "loupist-a", &mut reader).await;
    // A filing right after launch: morning message, no Thread message.
    reader.script(
        "equity_fundamental_filings",
        Some("MORR"),
        sec(vec![
            sec_row("8-K", day(-3), "0000990001-26-000030", "Current report"),
            tenk.clone(),
        ]),
    );
    // The first check after launch was the baseline pass above; start a new "launch".
    let fresh = Runtime::default();
    assert_eq!(check(&state, &fresh, "loupist-a", &mut reader).await, 1);
    let morning = events(&state, "loupist-a");
    assert_eq!(morning[0].delivered_in.as_deref(), Some("morning"));
    let home = crate::chat::home::ensure_home_thread(&state, "loupist-a").unwrap();
    assert!(home.messages.iter().all(|message| message.watch.is_none()));
    // Seven more filings later the same day: one message, five shown, two more.
    let mut rows: Vec<Value> = (0..7)
        .map(|index| {
            sec_row(
                "8-K",
                day(-1),
                &format!("0000990001-26-00004{index}"),
                "Current report",
            )
        })
        .collect();
    rows.push(sec_row(
        "8-K",
        day(-3),
        "0000990001-26-000030",
        "Current report",
    ));
    rows.push(tenk);
    reader.script("equity_fundamental_filings", Some("MORR"), sec(rows));
    assert_eq!(check(&state, &fresh, "loupist-a", &mut reader).await, 7);
    let home = crate::chat::home::ensure_home_thread(&state, "loupist-a").unwrap();
    let posted: Vec<_> = home
        .messages
        .iter()
        .filter_map(|message| message.watch.as_ref())
        .collect();
    assert_eq!(posted.len(), 1);
    assert_eq!(
        (
            posted[0].events.len(),
            posted[0].more,
            posted[0].event_ids.len()
        ),
        (5, 2, 7)
    );
    assert!(home.messages.last().unwrap().host_composed);
    // Needs attention counts every delivered alert until it is seen.
    let count = |state: &AppState| {
        with_assets(state, "loupist-a", false, |tx| {
            watch::unseen(tx, "loupist-a")
        })
        .unwrap()
        .0
    };
    assert_eq!(count(&state), 8);
    let ids = posted[0].event_ids.clone();
    with_assets(&state, "loupist-a", true, |tx| {
        watch::mark_seen(tx, "loupist-a", &ids, now_ms())
    })
    .unwrap();
    assert_eq!(count(&state), 1);
    // No holdings ever reach the message.
    assert!(!serde_json::to_string(&home.messages)
        .unwrap()
        .contains("quantity"));
}

#[tokio::test]
async fn a_briefing_lists_the_watch_alert_with_its_date_and_source_without_a_model() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary, &["loupist-a"]);
    follow(&state, "990308", "KRX", "Byeolmuri Semiconductor");
    defaults(&state, "loupist-a");
    let runtime = Runtime::default();
    let mut reader = Scripted::default();
    reader.script(
        "dart_filing_search",
        Some("990308"),
        dart(&[("20250101000001", day(-200), "사업보고서 (2024.12)")]),
    );
    let before = now_ms();
    check(&state, &runtime, "loupist-a", &mut reader).await;
    reader.script(
        "dart_filing_search",
        Some("990308"),
        dart(&[
            ("20260318990308", day(-1), "별무리반도체 포장라인 증설"),
            ("20250101000001", day(-200), "사업보고서 (2024.12)"),
        ]),
    );
    check(&state, &runtime, "loupist-a", &mut reader).await;
    let (prompt, context) =
        crate::briefings::build_prompt(&state, "loupist-a", Some(before), now_ms() + 1).unwrap();
    assert_eq!(context.watch_events.len(), 1, "{:?}", context.watch_events);
    let row = &context.watch_events[0];
    assert_eq!(row["kind"], "filings");
    assert_eq!(row["factDate"], day(-1).to_string());
    assert_eq!(
        row["evidence"][0]["url"],
        "https://dart.fss.or.kr/dsaf001/main.do?rcpNo=20260318990308"
    );
    assert!(prompt.contains("watchAlerts") && prompt.contains("포장라인 증설"));
}

#[test]
fn routes_run_on_their_cadence() {
    use chrono::TimeZone;
    let morning = chrono::Local
        .with_ymd_and_hms(2026, 11, 2, 9, 0, 0)
        .unwrap()
        .with_timezone(&Utc);
    let hour = 60 * 60 * 1000;
    let at = morning.timestamp_millis();
    assert!(route_due(Route::Filings, None, morning));
    assert!(!route_due(Route::Filings, Some(at - hour / 2), morning));
    assert!(route_due(Route::Filings, Some(at - hour), morning));
    // Daily after 06:00 local, once a day.
    assert!(!route_due(Route::Calendar, Some(at - hour), morning));
    assert!(route_due(Route::Calendar, Some(at - 24 * hour), morning));
    // Korean flows: weekdays after 18:30 in Seoul.
    let seoul_evening = chrono_tz::Asia::Seoul
        .with_ymd_and_hms(2026, 11, 2, 18, 40, 0)
        .unwrap()
        .with_timezone(&Utc);
    let seoul_noon = chrono_tz::Asia::Seoul
        .with_ymd_and_hms(2026, 11, 2, 12, 0, 0)
        .unwrap()
        .with_timezone(&Utc);
    let saturday = chrono_tz::Asia::Seoul
        .with_ymd_and_hms(2026, 11, 7, 19, 0, 0)
        .unwrap()
        .with_timezone(&Utc);
    // Hangeul Day (Fri Oct 9, 2026) is a published KRX holiday: no session to read.
    let holiday = chrono_tz::Asia::Seoul
        .with_ymd_and_hms(2026, 10, 9, 19, 0, 0)
        .unwrap()
        .with_timezone(&Utc);
    assert!(route_due(Route::Flows, None, seoul_evening));
    assert!(!route_due(Route::Flows, None, seoul_noon));
    assert!(!route_due(Route::Flows, None, saturday));
    assert!(!route_due(Route::Flows, None, holiday));
    assert!(!route_due(
        Route::Flows,
        Some(seoul_evening.timestamp_millis() - 5 * 60 * 1000),
        seoul_evening
    ));
}

/// The requests the renderer sends (Watching.test.tsx checks them against the
/// same file) are accepted by the watch, earnings and follow commands, whose
/// requests deny unknown fields.
#[test]
fn the_renderers_finance_requests_are_accepted() {
    use crate::commands::earnings::{
        EarningsEventsListRequest, EarningsPrepareRequest, EarningsSnapshotGetRequest,
    };
    use crate::commands::user_context::WatchlistAddRequest;
    let examples: BTreeMap<String, Vec<Value>> = serde_json::from_str(include_str!(
        "../../../../src/contracts/financeRequests.example.json"
    ))
    .unwrap();
    fn accepts<T: serde::de::DeserializeOwned>(examples: &[Value]) {
        for example in examples {
            serde_json::from_value::<T>(example.clone())
                .unwrap_or_else(|error| panic!("{example}: {error}"));
        }
    }
    for (command, requests) in &examples {
        match command.as_str() {
            "earnings_snapshot_get" => accepts::<EarningsSnapshotGetRequest>(requests),
            "earnings_events_list" => accepts::<EarningsEventsListRequest>(requests),
            "earnings_prepare" => accepts::<EarningsPrepareRequest>(requests),
            "watch_settings_list" => accepts::<WatchSettingsListRequest>(requests),
            "watch_setting_set" => accepts::<WatchSettingSetRequest>(requests),
            "watch_pause_set" => accepts::<WatchPauseSetRequest>(requests),
            "watch_check_now" => accepts::<WatchCheckNowRequest>(requests),
            "watch_events_list" => accepts::<WatchEventsListRequest>(requests),
            "watch_events_seen" => accepts::<WatchEventsSeenRequest>(requests),
            "watchlist_add" => accepts::<WatchlistAddRequest>(requests),
            other => panic!("no finance command {other}"),
        }
    }
    assert_eq!(examples.len(), 10);
    // A field Rust does not know is refused.
    assert!(serde_json::from_value::<WatchSettingSetRequest>(json!({
        "loupistId": "loupist-quality", "assetId": "ast_0123456789abcdef",
        "kind": "insiders", "enabled": true, "revision": 2
    }))
    .is_err());
}

/// A Loupist made from an asset-class template, with the morning update on.
fn class_fixture(
    temporary: &tempfile::TempDir,
    id: &str,
    template: crate::domain::LoupistTemplate,
) -> AppState {
    let state = AppState::for_test(temporary.path().join("app"));
    let mut loupist = profile(id, &temporary.path().join("workspace"), 1);
    loupist.template = Some(template);
    seed_profile(state.store.as_ref(), &loupist);
    consent_on(&state, id);
    state
}

/// Daily closes in UTC days, the last one `last_offset` days from today.
fn coin_bars(last_offset: i64, closes: &[f64]) -> Value {
    let today = Utc::now().date_naive();
    let first = today + Duration::days(last_offset + 1 - closes.len() as i64);
    json!({"structuredContent": {"provider": "yfinance", "results": closes.iter().enumerate().map(|(index, close)| json!({"date": (first + Duration::days(index as i64)).to_string(), "close": close})).collect::<Vec<_>>()}})
}

fn kind_state(view: &WatchSettingsView, asset_id: &str, kind: WatchKind) -> KindStateView {
    view.watched
        .iter()
        .chain(&view.not_watched)
        .find(|asset| asset.asset_id == asset_id)
        .and_then(|asset| asset.kinds.iter().find(|state| state.kind == kind))
        .cloned()
        .unwrap_or_else(|| panic!("{kind:?} for {asset_id}"))
}

#[tokio::test]
async fn a_crypto_loupist_watches_its_coins_from_their_closes_after_a_starting_point() {
    let temporary = tempfile::tempdir().unwrap();
    let state = class_fixture(
        &temporary,
        "loupist-crypto",
        crate::domain::LoupistTemplate::Crypto,
    );
    follow(&state, "ETH", "CRYPTO", "Ethereum");
    follow(&state, "USDC", "CRYPTO", "USD Coin");
    follow(&state, "MORR", "NASDAQ", "Morrow Components");
    defaults(&state, "loupist-crypto");
    let (eth, usdc) = (page_id("ETH", "CRYPTO"), page_id("USDC", "CRYPTO"));
    let runtime = Runtime::default();
    let view = settings_view(&state, &runtime, "loupist-crypto").unwrap();
    // The template's class starts on: moves, the fall from the high and value
    // locked for a chain's coin, only the peg for a stablecoin.
    let eth_kinds: Vec<_> = view
        .watched
        .iter()
        .find(|asset| asset.asset_id == eth)
        .unwrap()
        .kinds
        .iter()
        .map(|state| (state.kind, state.enabled))
        .collect();
    assert_eq!(
        eth_kinds,
        [
            (WatchKind::CryptoMove, true),
            (WatchKind::Drawdown, true),
            (WatchKind::Tvl, true)
        ]
    );
    assert!(kind_state(&view, &usdc, WatchKind::Depeg).enabled);
    // Equities keep their own defaults beside them.
    assert!(kind_state(&view, &page_id("MORR", "NASDAQ"), WatchKind::Filings).enabled);

    let mut reader = Scripted::default();
    reader.script("equity_fundamental_filings", Some("MORR"), sec(vec![]));
    // The first read is the starting point, even with a 15% day in it.
    reader.script(
        "crypto_price_historical",
        Some("ETHUSD"),
        coin_bars(
            -2,
            &[
                2000.0, 2010.0, 2020.0, 2030.0, 2040.0, 2050.0, 2060.0, 2070.0, 1760.0,
            ],
        ),
    );
    reader.script(
        "crypto_price_historical",
        Some("USDCUSD"),
        coin_bars(-2, &[1.0, 1.0, 0.9995]),
    );
    assert_eq!(
        check(&state, &runtime, "loupist-crypto", &mut reader).await,
        0
    );
    assert!(events(&state, "loupist-crypto").is_empty());
    // The coin reads its USD pair; value locked asks DefiLlama for Ethereum.
    assert!(reader
        .calls
        .iter()
        .any(|(_, tool, arguments)| tool == "defillama_read"
            && arguments["route"] == "chain"
            && arguments["id"] == "Ethereum"));
    // A new day: Ethereum falls 12% and USD Coin closes at 0.97.
    reader.script(
        "crypto_price_historical",
        Some("ETHUSD"),
        coin_bars(
            -1,
            &[
                2010.0, 2020.0, 2030.0, 2040.0, 2050.0, 2060.0, 2070.0, 2080.0, 2000.0, 1760.0,
            ],
        ),
    );
    reader.script(
        "crypto_price_historical",
        Some("USDCUSD"),
        coin_bars(-1, &[1.0, 1.0, 0.9995, 0.97]),
    );
    assert_eq!(
        check(&state, &runtime, "loupist-crypto", &mut reader).await,
        2
    );
    let found = events(&state, "loupist-crypto");
    let moved = found
        .iter()
        .find(|event| event.kind == WatchKind::CryptoMove)
        .unwrap();
    let yesterday = (Utc::now().date_naive() - Duration::days(1)).to_string();
    assert_eq!(moved.asset_id, eth);
    assert_eq!(moved.fact_date, yesterday);
    assert_eq!(moved.facts["window"], "day");
    assert_eq!(moved.facts["change"], -0.12);
    assert!(moved.evidence[0].dataset.is_some());
    assert_eq!(
        moved.evidence[0].pointer.as_deref(),
        Some("/structuredContent/results/9")
    );
    let peg = found
        .iter()
        .find(|event| event.kind == WatchKind::Depeg)
        .unwrap();
    assert_eq!(
        (peg.asset_id.as_str(), peg.facts["close"].as_f64()),
        (usdc.as_str(), Some(0.97))
    );
    // The same rows again fire nothing.
    assert_eq!(
        check(&state, &runtime, "loupist-crypto", &mut reader).await,
        0
    );
    // The page keeps the closes the watcher read.
    let closes = with_assets(&state, "loupist-crypto", false, |tx| {
        assets::observations(tx, "loupist-crypto", &eth)
    })
    .unwrap()
    .into_iter()
    .filter(|item| item.kind == ObservationKind::Close)
    .count();
    assert!(closes >= 9, "{closes}");
}

#[tokio::test]
async fn a_commodities_loupist_hears_positioning_flips_and_says_the_petroleum_report_needs_eia() {
    let temporary = tempfile::tempdir().unwrap();
    let state = class_fixture(
        &temporary,
        "loupist-oil",
        crate::domain::LoupistTemplate::Commodities,
    );
    follow(&state, "CL", "NYMEX", "WTI crude");
    defaults(&state, "loupist-oil");
    let crude = page_id("CL", "NYMEX");
    let runtime = Runtime::default();
    let mut reader = Scripted::default();
    let report = |weeks: &[(i64, f64, f64)]| json!({"structuredContent": {"provider": "cftc", "results": weeks.iter().map(|(ago, long, short)| json!({"date": day(-7 * ago).to_string(), "market_and_exchange_names": "WTI-PHYSICAL - NEW YORK MERCANTILE EXCHANGE", "managed_money_positions_long_all": long, "managed_money_positions_short_all": short})).collect::<Vec<_>>()}});
    reader.script_from(
        "cftc_cot",
        Some("067651"),
        "cftc",
        report(&[(3, 200_000.0, 150_000.0), (2, 180_000.0, 170_000.0)]),
    );
    reader.script(
        "derivatives_futures_historical",
        Some("CL"),
        coin_bars(-1, &[70.0, 70.5, 71.0]),
    );
    // No EIA connection: the petroleum report says what it needs.
    reader.script(
        "commodity_petroleum_status_report",
        None,
        json!({"structuredContent": {"results": []}}),
    );
    assert_eq!(check(&state, &runtime, "loupist-oil", &mut reader).await, 0);
    let view = settings_view(&state, &runtime, "loupist-oil").unwrap();
    assert!(kind_state(&view, &crude, WatchKind::Positioning).enabled);
    assert_eq!(
        kind_state(&view, &crude, WatchKind::OilInventory)
            .needs_connection
            .map(|connection| connection.id),
        Some("eia.energy".to_owned())
    );
    // The positioning read asked the CFTC for WTI's contract code.
    assert!(reader
        .calls
        .iter()
        .any(|(_, tool, arguments)| tool == "cftc_cot"
            && arguments["code"] == "067651"
            && arguments["report_type"] == "disaggregated"));
    // Next week money managers are net short: one alert, dated by the report.
    reader.script_from(
        "cftc_cot",
        Some("067651"),
        "cftc",
        report(&[
            (3, 200_000.0, 150_000.0),
            (2, 180_000.0, 170_000.0),
            (1, 160_000.0, 175_000.0),
        ]),
    );
    assert_eq!(check(&state, &runtime, "loupist-oil", &mut reader).await, 1);
    let found = events(&state, "loupist-oil");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, WatchKind::Positioning);
    assert_eq!(found[0].fact_date, day(-7).to_string());
    assert_eq!(found[0].facts["direction"], "short");
    assert!(found[0].evidence[0]
        .url
        .as_deref()
        .is_some_and(|url| url.starts_with("https://www.cftc.gov/")));
}

#[tokio::test]
async fn class_kinds_start_off_outside_their_template_and_turn_on_one_by_one() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary, &["loupist-a"]);
    follow(&state, "BTC", "CRYPTO", "Bitcoin");
    defaults(&state, "loupist-a");
    let btc = page_id("BTC", "CRYPTO");
    let runtime = Runtime::default();
    let view = settings_view(&state, &runtime, "loupist-a").unwrap();
    // Not a Crypto Loupist: Bitcoin is followed, not watched; no value locked for it.
    assert!(view.watched.is_empty());
    let kinds: Vec<_> = view.not_watched[0]
        .kinds
        .iter()
        .map(|state| state.kind)
        .collect();
    assert_eq!(kinds, [WatchKind::CryptoMove, WatchKind::Drawdown]);
    enable(&state, "loupist-a", &btc, WatchKind::Drawdown);
    let view = settings_view(&state, &runtime, "loupist-a").unwrap();
    assert!(kind_state(&view, &btc, WatchKind::Drawdown).enabled);
    assert!(!kind_state(&view, &btc, WatchKind::CryptoMove).enabled);
}

#[tokio::test]
async fn first_read_stays_a_baseline_when_delivery_was_skipped() {
    let temporary = tempfile::tempdir().unwrap();
    let state = fixture(&temporary, &["loupist-a"]);
    follow(&state, "MORR", "NASDAQ", "Morrow Components");
    defaults(&state, "loupist-a");
    let page = followed_pages(&state, "loupist-a").unwrap().remove(0);
    let at = Utc::now() + Duration::seconds(10);
    let read = route_read(Route::Filings, &page, at.date_naive()).unwrap();
    let mut reader = Scripted::default();
    reader.ensure(&state, "loupist-a");
    // Earlier manual research already knew this observation kind.
    for (offset, accession) in [(0, "earlier"), (1, "first-watch-read")] {
        reader.script(
            "equity_fundamental_filings",
            Some("MORR"),
            sec(vec![sec_row(
                "10-Q",
                day(-1),
                accession,
                "Quarterly report",
            )]),
        );
        let ReadOutcome::Ok { payload, provider } =
            reader.read(&state, "loupist-a", &read, None).await
        else {
            panic!("scripted read")
        };
        with_assets(&state, "loupist-a", true, |tx| {
            apply_read(
                tx,
                "loupist-a",
                &page,
                Route::Filings,
                &read,
                &payload,
                provider.as_deref(),
                at.date_naive(),
                at + Duration::seconds(offset),
                offset == 1,
            )?;
            if offset == 1 {
                // Research already retained this exact row as new. The first
                // watch read sees it again and must save its own starting point.
                apply_read(
                    tx,
                    "loupist-a",
                    &page,
                    Route::Filings,
                    &read,
                    &payload,
                    provider.as_deref(),
                    at.date_naive(),
                    at + Duration::seconds(offset),
                    false,
                )?;
                // Phase 1 committed; maintenance/cancellation skipped phase 2.
                watch::record_route(
                    tx,
                    "loupist-a",
                    &page.asset_id,
                    Route::Filings,
                    "ok",
                    OPENBB_CONNECTION,
                    at.timestamp_millis(),
                )?;
            }
            Ok(())
        })
        .unwrap();
    }
    let result = run_pass_with(
        &state,
        &Runtime::default(),
        Scope::Loupist("loupist-a".into()),
        at + Duration::hours(1),
        &mut reader,
    )
    .await
    .unwrap();
    assert_eq!(result["loupist-a"], 0);
    assert!(events(&state, "loupist-a").is_empty());
    // A later genuinely new filing still alerts.
    reader.script(
        "equity_fundamental_filings",
        Some("MORR"),
        sec(vec![sec_row("8-K", day(0), "later", "Current report")]),
    );
    let result = run_pass_with(
        &state,
        &Runtime::default(),
        Scope::Loupist("loupist-a".into()),
        at + Duration::hours(2),
        &mut reader,
    )
    .await
    .unwrap();
    assert_eq!(result["loupist-a"], 1);
}

#[tokio::test]
async fn class_starting_points_survive_a_skipped_delivery_phase() {
    let temporary = tempfile::tempdir().unwrap();
    let state = class_fixture(
        &temporary,
        "loupist-crypto",
        crate::domain::LoupistTemplate::Crypto,
    );
    follow(&state, "ETH", "CRYPTO", "Ethereum");
    defaults(&state, "loupist-crypto");
    let page = followed_pages(&state, "loupist-crypto").unwrap().remove(0);
    let at = Utc::now();
    let read = route_read(Route::Price, &page, at.date_naive()).unwrap();
    let mut reader = Scripted::default();
    reader.ensure(&state, "loupist-crypto");
    reader.script(
        "crypto_price_historical",
        Some("ETHUSD"),
        coin_bars(
            -1,
            &[
                2000.0, 2010.0, 2020.0, 2030.0, 2040.0, 2050.0, 2060.0, 2070.0, 1760.0,
            ],
        ),
    );
    let ReadOutcome::Ok { payload, provider } =
        reader.read(&state, "loupist-crypto", &read, None).await
    else {
        panic!("scripted read")
    };
    with_assets(&state, "loupist-crypto", true, |tx| {
        let signals = apply_read(
            tx,
            "loupist-crypto",
            &page,
            Route::Price,
            &read,
            &payload,
            provider.as_deref(),
            at.date_naive(),
            at,
            false,
        )?;
        assert!(!signals.is_empty());
        watch::record_route(
            tx,
            "loupist-crypto",
            &page.asset_id,
            Route::Price,
            "ok",
            OPENBB_CONNECTION,
            at.timestamp_millis(),
        )
    })
    .unwrap();
    let alerts = run_pass_with(
        &state,
        &Runtime::default(),
        Scope::Loupist("loupist-crypto".into()),
        at + Duration::hours(1),
        &mut reader,
    )
    .await
    .unwrap();
    assert_eq!(alerts["loupist-crypto"], 0);
    assert!(events(&state, "loupist-crypto").is_empty());
}
