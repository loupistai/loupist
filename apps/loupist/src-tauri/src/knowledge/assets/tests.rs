use super::*;
use crate::knowledge::{journal, JournalCheck, KnowledgeStore};
use observe::ToolRead;
use serde_json::json;

const LOUPIST: &str = "loupist-a";
const DAY: i64 = 86_400_000;

fn store() -> (tempfile::TempDir, KnowledgeStore) {
    let root = tempfile::tempdir().unwrap();
    let store = KnowledgeStore::open(root.path().join("knowledge.sqlite")).unwrap();
    (root, store)
}

fn day(value: &str) -> NaiveDate {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
}

fn ms(value: &str) -> i64 {
    day(value)
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap()
        .timestamp_millis()
}

fn hanul() -> AssetIdentity {
    AssetIdentity {
        symbol: "990308".into(),
        market: "KRX".into(),
        name: "Byeolmuri Semiconductor".into(),
    }
}

fn page(store: &KnowledgeStore) -> PageRecord {
    store
        .transact(|w| ensure_page(w.tx, LOUPIST, &hanul(), None, ms("2026-09-30")))
        .unwrap()
}

fn dart(filings: &[(&str, &str, &str)]) -> Value {
    json!({"data": {"filings": filings.iter().map(|(id, date, title)| json!({"filing_id": id, "filed_at": date, "title": title})).collect::<Vec<_>>()}})
}

fn read<'a>(tool: &'a str, payload: &'a Value, today: &str) -> ToolRead<'a> {
    ToolRead {
        tool,
        provider: None,
        payload,
        dataset: Some(DatasetRef {
            dataset_id: "dataset-1".into(),
            revision: 1,
            digest: "d".repeat(64),
        }),
        retrieved_at: None,
        citation_url: None,
        today: day(today),
        session_day: day(today),
    }
}

fn context(today: &str) -> ViewContext {
    ViewContext {
        today: Some(day(today)),
        ..ViewContext::default()
    }
}

fn write_entry(
    store: &KnowledgeStore,
    asset: &str,
    role: ThesisRole,
    text: &str,
    check: Option<JournalCheck>,
    at: i64,
) -> JournalRecord {
    store
        .transact(|w| {
            journal::create(
                w.tx,
                LOUPIST,
                journal::JournalInput {
                    judgment: text.into(),
                    reasoning: String::new(),
                    entities: vec![],
                    check,
                    thesis: Some(JournalThesis {
                        asset_id: asset.into(),
                        role,
                    }),
                },
                at,
            )
        })
        .unwrap()
}

fn line_check(threshold: f64) -> JournalCheck {
    JournalCheck {
        claim: "One customer above 25% of revenue".into(),
        metric: "Largest customer share of revenue".into(),
        direction: "above".into(),
        threshold,
        unit: Some("%".into()),
        by_date: None,
        confidence: None,
    }
}

fn filing_evidence(date: &str) -> AssetEvidence {
    AssetEvidence {
        tool: "dart_filing_read".into(),
        title: "분기보고서".into(),
        citation_url: Some("#evidence-ev_1".into()),
        date: Some(date.into()),
        ..AssetEvidence::default()
    }
}

fn update(
    asset: &str,
    to: Option<Verdict>,
    line: Option<(String, f64)>,
    date: &str,
) -> ThesisUpdate {
    ThesisUpdate {
        asset_id: asset.into(),
        to,
        reason: to.map(|_| {
            "The 10-Q shows operating margin 14.8%, below the 16% the first reason needs.".into()
        }),
        reason_id: None,
        line,
        evidence: vec![filing_evidence(date)],
        fact_date: day(date),
    }
}

#[test]
fn class_resolution_follows_user_market_lookup_world_then_equity() {
    assert_eq!(
        class::initial_class("BTC", "UPBIT", None),
        (AssetClass::Crypto, ClassSource::Market)
    );
    assert_eq!(
        class::initial_class("USDKRW", "", None),
        (AssetClass::RatesFx, ClassSource::Market)
    );
    assert_eq!(
        class::initial_class("GC", "COMEX", Some("equity")),
        (AssetClass::Commodity, ClassSource::Market)
    );
    assert_eq!(
        class::initial_class("HANU", "KRX", Some("Exchange-traded fund")),
        (AssetClass::Fund, ClassSource::World)
    );
    assert_eq!(
        class::initial_class("MORR", "NASDAQ", None),
        (AssetClass::Equity, ClassSource::Default)
    );
    assert_eq!(
        class::initial_class("VILLA", "REAL", None).0,
        AssetClass::RealAsset
    );
    let (_root, store) = store();
    let identity = AssetIdentity {
        symbol: "MORR".into(),
        market: "NASDAQ".into(),
        name: "Morrow Components".into(),
    };
    let created = store
        .transact(|w| ensure_page(w.tx, LOUPIST, &identity, Some("Exchange-traded fund"), 1))
        .unwrap();
    assert_eq!(
        (created.class, created.class_source),
        (AssetClass::Fund, ClassSource::World)
    );
    // A refresh lookup outranks the World class, a user choice outranks everything,
    // and nothing else replaces the user's choice.
    let looked = store
        .transact(|w| {
            set_class(
                w.tx,
                LOUPIST,
                &created.asset_id,
                AssetClass::Equity,
                ClassSource::Lookup,
            )
        })
        .unwrap();
    assert_eq!(
        (looked.class, looked.class_source),
        (AssetClass::Equity, ClassSource::Lookup)
    );
    let chosen = store
        .transact(|w| {
            set_class(
                w.tx,
                LOUPIST,
                &created.asset_id,
                AssetClass::Crypto,
                ClassSource::User,
            )
        })
        .unwrap();
    assert_eq!(chosen.class_source, ClassSource::User);
    let kept = store
        .transact(|w| {
            set_class(
                w.tx,
                LOUPIST,
                &created.asset_id,
                AssetClass::Fund,
                ClassSource::World,
            )
        })
        .unwrap();
    assert_eq!(
        (kept.class, kept.class_source),
        (AssetClass::Crypto, ClassSource::User)
    );
    assert_eq!(created.asset_id, asset_id("morr", "nasdaq"));
    assert!(valid_asset_id(&created.asset_id));
}

#[test]
fn the_first_refresh_is_a_baseline_and_later_filings_are_new_dated_sourced_rows() {
    let (_root, store) = store();
    let page = page(&store);
    let first = dart(&[("20260115990308", "20260115", "분기보고서 (2025.12)")]);
    let added = store
        .transact(|w| {
            record_observations(
                w.tx,
                LOUPIST,
                &page.asset_id,
                observe::extract(&read("dart_filing_search", &first, "2026-03-01"), "990308"),
                ObservationOrigin::Refresh,
                None,
                ms("2026-03-01"),
            )
        })
        .unwrap();
    assert_eq!(added, 0, "a first refresh records a baseline");
    let second = dart(&[
        ("20260318990308", "20260318", "별무리반도체 포장라인 증설"),
        ("20260115990308", "20260115", "분기보고서 (2025.12)"),
    ]);
    let refresh = |at: &str| {
        store
            .transact(|w| {
                record_observations(
                    w.tx,
                    LOUPIST,
                    &page.asset_id,
                    observe::extract(&read("dart_filing_search", &second, "2026-03-20"), "990308"),
                    ObservationOrigin::Refresh,
                    None,
                    ms(at),
                )
            })
            .unwrap()
    };
    assert_eq!(refresh("2026-03-20"), 1);
    assert_eq!(
        refresh("2026-03-21"),
        0,
        "an identical refresh adds nothing"
    );
    let delta = store
        .transact(|w| deltas(w.tx, LOUPIST, Some(&page.asset_id), ms("2026-03-01")))
        .unwrap()
        .remove(0);
    assert_eq!(delta.rows.len(), 1);
    let row = &delta.rows[0];
    assert_eq!(
        (row.kind.as_str(), row.fact_date.as_str(), row.new),
        ("filing", "2026-03-18", true)
    );
    assert_eq!(
        row.evidence[0].url.as_deref(),
        Some("https://dart.fss.or.kr/dsaf001/main.do?rcpNo=20260318990308")
    );
    assert_eq!(row.evidence[0].pointer.as_deref(), Some("/data/filings/0"));
    assert!(row.evidence[0].dataset.is_some());
    // A first look over 30 days shows the baseline filing whose fact falls inside it.
    let first_look = store
        .transact(|w| deltas(w.tx, LOUPIST, Some(&page.asset_id), ms("2026-01-10")))
        .unwrap()
        .remove(0);
    assert_eq!(first_look.rows.len(), 2);
    assert!(
        !first_look.rows[1].new,
        "baseline rows are never marked new"
    );
}

#[test]
fn rows_without_a_date_or_a_source_are_never_recorded() {
    let undated = json!({"data": {"filings": [{"filing_id": "20260318990308", "title": "x"}]}});
    assert!(observe::extract(
        &read("dart_filing_search", &undated, "2026-03-20"),
        "990308"
    )
    .is_empty());
    let future = dart(&[("20260330990308", "20260330", "later")]);
    assert!(
        observe::extract(&read("dart_filing_search", &future, "2026-03-20"), "990308").is_empty()
    );
    let (_root, store) = store();
    let page = page(&store);
    let sourceless = NewObservation {
        kind: ObservationKind::Filing,
        fact_date: "2026-03-18".into(),
        dedupe_key: "filing:x".into(),
        facts: json!({"title": "x"}),
        evidence: AssetEvidence {
            tool: "model".into(),
            title: "x".into(),
            ..AssetEvidence::default()
        },
    };
    store
        .transact(|w| {
            record_observations(
                w.tx,
                LOUPIST,
                &page.asset_id,
                vec![sourceless],
                ObservationOrigin::Chat,
                None,
                ms("2026-03-20"),
            )
        })
        .unwrap();
    assert!(store
        .read(|w| observations(w.tx, LOUPIST, &page.asset_id))
        .unwrap()
        .is_empty());
}

#[test]
fn the_price_row_compares_the_latest_close_with_the_last_close_on_or_before_the_date() {
    let (_root, store) = store();
    let page = page(&store);
    let bars = json!({"data": {"instrument": {"currency": "KRW"}, "bars": [
        {"date": "2026-10-26", "close": "41000"},
        {"date": "2026-10-27", "close": "41200"},
        {"date": "2026-10-28", "close": "39920"}
    ]}});
    store
        .transact(|w| {
            record_observations(
                w.tx,
                LOUPIST,
                &page.asset_id,
                observe::extract(&read("krx_daily_quote", &bars, "2026-10-29"), "990308"),
                ObservationOrigin::Refresh,
                None,
                ms("2026-10-29"),
            )
        })
        .unwrap();
    let delta = store
        .transact(|w| deltas(w.tx, LOUPIST, Some(&page.asset_id), ms("2026-10-27")))
        .unwrap()
        .remove(0);
    let price = delta.rows.iter().find(|row| row.kind == "price").unwrap();
    assert_eq!(price.facts["change"], json!(-3.1));
    assert_eq!(
        (price.facts["from"].as_f64(), price.facts["to"].as_f64()),
        (Some(41200.0), Some(39920.0))
    );
    assert_eq!(price.evidence.len(), 2, "both closes are linked");
    assert!(!price.material, "−3.1% is below the 5% equity threshold");
    let view = store
        .transact(|w| {
            page_view(
                w.tx,
                LOUPIST,
                &page.asset_id,
                ms("2026-10-27"),
                &context("2026-10-29"),
            )
        })
        .unwrap();
    assert!(view.glance.iter().any(|row| row.key == "price"));
}

#[test]
fn closes_are_completed_sessions_that_a_later_read_restates() {
    let (_root, store) = store();
    let page = page(&store);
    let record = |payload: &Value, today: &str| {
        store
            .transact(|w| {
                record_observations(
                    w.tx,
                    LOUPIST,
                    &page.asset_id,
                    observe::extract(&read("krx_daily_quote", payload, today), "990308"),
                    ObservationOrigin::Refresh,
                    None,
                    ms(today),
                )
            })
            .unwrap()
    };
    let closes = || {
        store
            .read(|w| observations(w.tx, LOUPIST, &page.asset_id))
            .unwrap()
            .into_iter()
            .filter(|item| item.kind == ObservationKind::Close)
            .map(|item| (item.fact_date, item.facts["close"].as_f64().unwrap()))
            .collect::<Vec<_>>()
    };
    // The exchange's today may still be trading: only earlier sessions count.
    record(
        &json!({"data": {"bars": [{"date": "2026-10-27", "close": "41200"}, {"date": "2026-10-28", "close": "40100"}]}}),
        "2026-10-28",
    );
    assert_eq!(closes(), [("2026-10-27".to_owned(), 41200.0)]);
    // The next read adds the finished session and restates a corrected one.
    record(
        &json!({"data": {"bars": [{"date": "2026-10-27", "close": "41250"}, {"date": "2026-10-28", "close": "39920"}]}}),
        "2026-10-29",
    );
    assert_eq!(
        closes(),
        [
            ("2026-10-27".to_owned(), 41250.0),
            ("2026-10-28".to_owned(), 39920.0)
        ]
    );
    // A row marked final is a completed session even on the exchange's day.
    record(
        &json!({"data": {"bars": [{"date": "2026-10-29", "close": "40000", "final": true}]}}),
        "2026-10-29",
    );
    assert_eq!(closes().len(), 3);
}

#[test]
fn adjusted_and_raw_closes_stay_apart_and_a_split_is_not_a_move() {
    let (_root, store) = store();
    let morrow = store
        .transact(|w| {
            ensure_page(
                w.tx,
                LOUPIST,
                &AssetIdentity {
                    symbol: "MORR".into(),
                    market: "NASDAQ".into(),
                    name: "Morrow Components".into(),
                },
                None,
                1,
            )
        })
        .unwrap();
    let record = |payload: Value| {
        store
            .transact(|w| {
                record_observations(
                    w.tx,
                    LOUPIST,
                    &morrow.asset_id,
                    observe::extract(
                        &read("equity_price_historical", &payload, "2026-10-25"),
                        "MORR",
                    ),
                    ObservationOrigin::Refresh,
                    None,
                    ms("2026-10-25"),
                )
            })
            .unwrap()
    };
    let price = || {
        store
            .transact(|w| deltas(w.tx, LOUPIST, Some(&morrow.asset_id), ms("2026-10-19")))
            .unwrap()
            .remove(0)
            .rows
            .into_iter()
            .find(|row| row.kind == "price")
            .unwrap()
    };
    // A raw history across a 2:1 split halves; it is shown, never flagged.
    record(json!({"query": {"adjustment": "unadjusted"}, "results": [
        {"date": "2026-10-19", "close": 100.0},
        {"date": "2026-10-21", "close": 50.0, "split_ratio": 2.0},
        {"date": "2026-10-22", "close": 51.0}
    ]}));
    let raw = price();
    assert_eq!(raw.facts["series"], observe::PRICE_RAW);
    assert_eq!(raw.facts["change"], json!(-49.0));
    assert_eq!(raw.facts["splitBetween"], true);
    assert!(!raw.material, "a split is not a move");
    // An adjusted history is its own series, preferred for the page's price row.
    record(json!({"results": [
        {"date": "2026-10-19", "close": 100.0, "adj_close": 50.0},
        {"date": "2026-10-21", "close": 50.0, "adj_close": 50.0},
        {"date": "2026-10-22", "close": 51.0, "adj_close": 51.0}
    ]}));
    let adjusted = price();
    assert_eq!(adjusted.facts["series"], observe::PRICE_ADJUSTED);
    assert_eq!(adjusted.facts["change"], json!(2.0));
    let series: BTreeSet<String> = store
        .read(|w| observations(w.tx, LOUPIST, &morrow.asset_id))
        .unwrap()
        .into_iter()
        .map(|item| item.facts["series"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(series.len(), 2, "{series:?}");
}

#[test]
fn a_read_moves_one_step_with_dated_evidence_and_only_once_a_thesis_exists() {
    let (_root, store) = store();
    let page = page(&store);
    let refused = store
        .transact(|w| {
            apply_update(
                w.tx,
                LOUPIST,
                &update(&page.asset_id, Some(Verdict::Weakening), None, "2026-10-30"),
                None,
                ms("2026-10-31"),
            )
        })
        .unwrap_err();
    assert_eq!(refused.code(), "thesis_none");
    let reason = write_entry(
        &store,
        &page.asset_id,
        ThesisRole::Reason,
        "Margins recover once the new line qualifies.",
        None,
        ms("2026-09-30"),
    );
    let view = store
        .transact(|w| page_view(w.tx, LOUPIST, &page.asset_id, 0, &context("2026-10-31")))
        .unwrap();
    let read = view.read.unwrap();
    assert_eq!(
        (read.verdict, read.since.as_str(), read.started),
        (Verdict::Holding, "2026-09-30", true)
    );
    for (to, code) in [
        (Verdict::Challenged, "thesis_two_steps"),
        (Verdict::Holding, "thesis_same_step"),
    ] {
        let refused = store
            .transact(|w| {
                apply_update(
                    w.tx,
                    LOUPIST,
                    &update(&page.asset_id, Some(to), None, "2026-10-30"),
                    None,
                    ms("2026-10-31"),
                )
            })
            .unwrap_err();
        assert_eq!(refused.code(), code);
    }
    let mut long = update(&page.asset_id, Some(Verdict::Weakening), None, "2026-10-30");
    long.reason = Some("x".repeat(281));
    assert_eq!(
        store
            .transact(|w| apply_update(w.tx, LOUPIST, &long, None, 1))
            .unwrap_err()
            .code(),
        "thesis_reason"
    );
    let mut bare = update(&page.asset_id, Some(Verdict::Weakening), None, "2026-10-30");
    bare.evidence.clear();
    assert_eq!(
        store
            .transact(|w| apply_update(w.tx, LOUPIST, &bare, None, 1))
            .unwrap_err()
            .code(),
        "thesis_evidence"
    );
    let mut named = update(&page.asset_id, Some(Verdict::Weakening), None, "2026-10-30");
    named.reason_id = Some(reason.journal_id.clone());
    let applied = store
        .transact(|w| {
            apply_update(
                w.tx,
                LOUPIST,
                &named,
                Some(("chat-1", "message-1")),
                ms("2026-10-31"),
            )
        })
        .unwrap();
    let moved = applied.moved.unwrap();
    assert_eq!(
        (moved.from, moved.to, moved.fact_date.as_str()),
        (Some(Verdict::Holding), Verdict::Weakening, "2026-10-30")
    );
    let view = store
        .transact(|w| page_view(w.tx, LOUPIST, &page.asset_id, 0, &context("2026-10-31")))
        .unwrap();
    assert_eq!(view.read.as_ref().unwrap().verdict, Verdict::Weakening);
    assert_eq!(
        view.read.as_ref().unwrap().undo_move_id.as_deref(),
        Some(moved.move_id.as_str())
    );
    assert_eq!(
        view.reasons[0].last_move.as_ref().unwrap().to,
        Verdict::Weakening
    );
    let events = store
        .transact(|w| {
            w.growth_events(&crate::knowledge::GrowthEventsQuery {
                loupist_id: LOUPIST.into(),
                since_ms: None,
                until_ms: None,
                limit: None,
                cursor: None,
            })
        })
        .unwrap();
    let event = events
        .events
        .iter()
        .find(|event| event.kind == "thesis_moved")
        .unwrap();
    assert_eq!(
        (
            event.after_summary.as_deref(),
            event.origin_message_id.as_deref()
        ),
        (Some("weakening"), Some("message-1"))
    );
    // Archiving the only reason removes the read.
    store
        .transact(|w| journal::archive(w.tx, LOUPIST, &reason.journal_id, true, ms("2026-11-01")))
        .unwrap();
    assert!(store
        .transact(|w| page_view(w.tx, LOUPIST, &page.asset_id, 0, &context("2026-11-01")))
        .unwrap()
        .read
        .is_none());
}

#[test]
fn a_crossed_line_breaks_the_read_until_the_user_undoes_it() {
    let (_root, store) = store();
    let page = page(&store);
    write_entry(
        &store,
        &page.asset_id,
        ThesisRole::Reason,
        "Morrow stays the only HBM substrate supplier.",
        None,
        ms("2026-09-30"),
    );
    let line = write_entry(
        &store,
        &page.asset_id,
        ThesisRole::Line,
        "If one customer passes 25% of revenue.",
        Some(line_check(25.0)),
        ms("2026-09-30"),
    );
    // A reading under the line is recorded without moving the read.
    let below = store
        .transact(|w| {
            apply_update(
                w.tx,
                LOUPIST,
                &update(
                    &page.asset_id,
                    None,
                    Some((line.journal_id.clone(), 21.0)),
                    "2026-10-30",
                ),
                None,
                ms("2026-10-30"),
            )
        })
        .unwrap();
    assert!(below.moved.is_none() && !below.reading.unwrap().crossed);
    let view = store
        .transact(|w| page_view(w.tx, LOUPIST, &page.asset_id, 0, &context("2026-10-30")))
        .unwrap();
    assert_eq!(view.lines[0].status, "not_crossed");
    assert_eq!(view.lines[0].last.as_ref().unwrap().observed, 21.0);
    // Crossing jumps straight to Broken from Holding up.
    let crossed = store
        .transact(|w| {
            apply_update(
                w.tx,
                LOUPIST,
                &update(
                    &page.asset_id,
                    None,
                    Some((line.journal_id.clone(), 27.0)),
                    "2026-10-31",
                ),
                Some(("chat-1", "message-2")),
                ms("2026-10-31"),
            )
        })
        .unwrap();
    let moved = crossed.moved.unwrap();
    assert_eq!(
        (moved.from, moved.to),
        (Some(Verdict::Holding), Verdict::Broken)
    );
    assert!(moved
        .reason
        .starts_with("Crossed your line: One customer above 25% of revenue (27%, 2026-10-31)"));
    assert_eq!(
        store
            .read(|w| unseen_crossings(w.tx, LOUPIST))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store.read(|w| line_alerts(w.tx, LOUPIST, 0)).unwrap()[0].move_id,
        moved.move_id
    );
    let view = store
        .transact(|w| page_view(w.tx, LOUPIST, &page.asset_id, 0, &context("2026-10-31")))
        .unwrap();
    assert_eq!(
        (
            view.lines[0].status.as_str(),
            view.read.as_ref().unwrap().verdict
        ),
        ("crossed", Verdict::Broken)
    );
    // Opening the page clears the attention row.
    store
        .transact(|w| mark_seen(w.tx, LOUPIST, &page.asset_id, ms("2026-10-31")))
        .unwrap();
    assert!(store
        .read(|w| unseen_crossings(w.tx, LOUPIST))
        .unwrap()
        .is_empty());
    // Undo: only the latest Loupist move. A refresh that bumps the page's
    // revision does not stale it; a newer move does.
    let bars = json!({"data": {"bars": [{"date": "2026-10-30", "close": "39920"}]}});
    store
        .transact(|w| {
            record_observations(
                w.tx,
                LOUPIST,
                &page.asset_id,
                observe::extract(&read("krx_daily_quote", &bars, "2026-10-31"), "990308"),
                ObservationOrigin::Refresh,
                None,
                ms("2026-10-31"),
            )?;
            record_refresh(
                w.tx,
                LOUPIST,
                &page.asset_id,
                BTreeMap::new(),
                ms("2026-10-31"),
            )
        })
        .unwrap();
    assert!(
        store
            .transact(|w| get_page(w.tx, LOUPIST, &page.asset_id))
            .unwrap()
            .unwrap()
            .revision
            > view.revision
    );
    let stale = store
        .transact(|w| {
            undo(
                w.tx,
                LOUPIST,
                &page.asset_id,
                "amv-an-earlier-move",
                ms("2026-11-01"),
            )
        })
        .unwrap_err();
    assert_eq!(stale.code(), "revision_conflict");
    let restored = store
        .transact(|w| {
            undo(
                w.tx,
                LOUPIST,
                &page.asset_id,
                &moved.move_id,
                ms("2026-11-01"),
            )
        })
        .unwrap();
    assert_eq!(restored.verdict, Some(Verdict::Holding));
    let again = store
        .transact(|w| {
            undo(
                w.tx,
                LOUPIST,
                &page.asset_id,
                &moved.move_id,
                ms("2026-11-01"),
            )
        })
        .unwrap_err();
    assert_eq!(
        again.code(),
        "revision_conflict",
        "the Undo is now the latest move"
    );
    let history = store
        .transact(|w| moves(w.tx, LOUPIST, &page.asset_id))
        .unwrap();
    assert_eq!(history.last().unwrap().actor, "user");
    assert!(history
        .iter()
        .any(|item| item.move_id == moved.move_id && item.undone));
    // After an Undo the same direction needs a fact dated after the undone one.
    let refused = store
        .transact(|w| {
            apply_update(
                w.tx,
                LOUPIST,
                &update(&page.asset_id, Some(Verdict::Weakening), None, "2026-10-31"),
                None,
                ms("2026-11-01"),
            )
        })
        .unwrap_err();
    assert_eq!(refused.code(), "thesis_after_undo");
    assert!(store
        .transact(|w| apply_update(
            w.tx,
            LOUPIST,
            &update(&page.asset_id, Some(Verdict::Weakening), None, "2026-11-01"),
            None,
            ms("2026-11-01")
        ))
        .is_ok());
}

#[test]
fn sources_name_an_asset_by_whole_words_and_short_tickers_need_their_exchange() {
    let (_root, store) = store();
    let identity = |symbol: &str, market: &str, name: &str| AssetIdentity {
        symbol: symbol.into(),
        market: market.into(),
        name: name.into(),
    };
    let at_t = store
        .transact(|w| ensure_page(w.tx, LOUPIST, &identity("T", "NYSE", "T"), None, 1))
        .unwrap();
    let morrow = store
        .transact(|w| {
            ensure_page(
                w.tx,
                LOUPIST,
                &identity("MORR", "NASDAQ", "Morrow Components"),
                None,
                1,
            )
        })
        .unwrap();
    let korean = page(&store);
    // A one-letter ticker is not found inside words or alone.
    assert!(!names_asset("Tesla quarterly report", &at_t));
    assert!(!names_asset("Form T filed", &at_t));
    assert!(names_asset("T (NYSE) 10-Q for Q3", &at_t));
    // Substrings of longer words never match a symbol or a name.
    assert!(!names_asset("Morrowind modding notes", &morrow));
    assert!(!names_asset("MORRIS county bonds", &morrow));
    assert!(names_asset("Morrow Components 10-Q", &morrow));
    assert!(names_asset("MORR: margins slip", &morrow));
    // Korean particles may follow a name or a code.
    assert!(names_asset(
        "Byeolmuri Semiconductor's capacity plan",
        &korean
    ));
    assert!(names_asset("990308의 분기보고서", &korean));
    assert!(!names_asset("1990308 lot numbers", &korean));
}

#[test]
fn after_an_undo_a_line_crossing_needs_a_later_fact_too() {
    let (_root, store) = store();
    let page = page(&store);
    write_entry(
        &store,
        &page.asset_id,
        ThesisRole::Reason,
        "Morrow stays the only HBM substrate supplier.",
        None,
        ms("2026-09-30"),
    );
    let line = write_entry(
        &store,
        &page.asset_id,
        ThesisRole::Line,
        "If one customer passes 25% of revenue.",
        Some(line_check(25.0)),
        ms("2026-09-30"),
    );
    let reading = |observed: f64, date: &str| {
        update(
            &page.asset_id,
            None,
            Some((line.journal_id.clone(), observed)),
            date,
        )
    };
    let crossed = store
        .transact(|w| {
            apply_update(
                w.tx,
                LOUPIST,
                &reading(27.0, "2026-10-30"),
                None,
                ms("2026-10-30"),
            )
        })
        .unwrap()
        .moved
        .unwrap();
    assert_eq!(
        store.read(|w| line_alerts(w.tx, LOUPIST, 0)).unwrap().len(),
        1
    );
    store
        .transact(|w| {
            undo(
                w.tx,
                LOUPIST,
                &page.asset_id,
                &crossed.move_id,
                ms("2026-10-31"),
            )
        })
        .unwrap();
    // An undone crossing is not announced in the Thread (one not yet posted).
    assert!(store
        .read(|w| line_alerts(w.tx, LOUPIST, 0))
        .unwrap()
        .is_empty());
    // The same reading again cannot re-break the read the user restored.
    for date in ["2026-10-29", "2026-10-30"] {
        let refused = store
            .transact(|w| apply_update(w.tx, LOUPIST, &reading(27.0, date), None, ms("2026-10-31")))
            .unwrap_err();
        assert_eq!(refused.code(), "thesis_after_undo", "{date}");
    }
    // A reading under the line moves nothing, so it is still recorded.
    assert!(store
        .transact(|w| apply_update(
            w.tx,
            LOUPIST,
            &reading(21.0, "2026-10-30"),
            None,
            ms("2026-10-31")
        ))
        .unwrap()
        .moved
        .is_none());
    let again = store
        .transact(|w| {
            apply_update(
                w.tx,
                LOUPIST,
                &reading(28.0, "2026-11-01"),
                None,
                ms("2026-11-01"),
            )
        })
        .unwrap();
    assert_eq!(again.moved.unwrap().to, Verdict::Broken);
}

#[test]
fn a_line_needs_a_number_and_only_lines_may_skip_the_date() {
    let (_root, store) = store();
    let page = page(&store);
    let undated = line_check(25.0);
    let create = |thesis: Option<JournalThesis>, check: Option<JournalCheck>| {
        store.transact(|w| {
            journal::create(
                w.tx,
                LOUPIST,
                journal::JournalInput {
                    judgment: "A line.".into(),
                    reasoning: String::new(),
                    entities: vec![],
                    check,
                    thesis,
                },
                1,
            )
        })
    };
    assert_eq!(
        create(None, Some(undated.clone())).unwrap_err().code(),
        "journal_check_date"
    );
    let line = JournalThesis {
        asset_id: page.asset_id.clone(),
        role: ThesisRole::Line,
    };
    assert!(create(Some(line.clone()), Some(undated)).is_ok());
    assert_eq!(
        create(Some(line), None).unwrap_err().code(),
        "journal_thesis_line"
    );
    let unknown = JournalThesis {
        asset_id: asset_id("NOPE", "NYSE"),
        role: ThesisRole::Reason,
    };
    assert_eq!(
        create(Some(unknown), None).unwrap_err().code(),
        "journal_thesis"
    );
    // Undated lines never appear among due Journal checks.
    assert!(store
        .read(|w| journal::due(w.tx, LOUPIST, "2030-01-01"))
        .unwrap()
        .is_empty());
    let _ = DAY;
}

#[test]
fn capital_decisions_and_korean_dividends_become_sourced_rows_and_glance_facts() {
    let (_root, store) = store();
    let page = page(&store);
    let decisions = json!({"dataset_ref": {"dataset_id": "ds_major", "revision": 1}, "data": {
    "kind": "convertible_bonds",
    "rows": [
        {"filing_id": "20261030990308", "filed_at": "2026-10-30", "kind": "convertible_bonds",
         "url": "https://dart.fss.or.kr/dsaf001/main.do?rcpNo=20261030990308",
         "face_amount": "30000000000", "conversion_price": "9100", "refix_floor": "6370",
         "payment_date": "2026-11-05", "dilution_pct": "8.71", "dilution_basis": "calculated", "dilution_at": "floor",
         "dilution_calculation": {"title": "Most new shares at the floor", "formulas": ["New shares at the floor / Shares outstanding = 4,709,576 / 54,044,311 = 8.71%"],
            "inputs": [], "display": "8.71%", "value": 8.7143, "unit": "%"}},
        {"filing_id": "20261020990308", "filed_at": "2026-10-20", "kind": "exchangeable_bonds",
         "url": "https://dart.fss.or.kr/dsaf001/main.do?rcpNo=20261020990308"}
    ]}});
    let rights = json!({"data": {"kind": "rights_offering", "rows": [
        {"filing_id": "20260915990308", "filed_at": "2026-09-15", "kind": "rights_offering",
         "url": "https://dart.fss.or.kr/dsaf001/main.do?rcpNo=20260915990308",
         "new_shares_common": "4000000", "method": "shareholders", "issue_price": null, "payment_date": null,
         "dilution_pct": "8", "dilution_basis": "calculated",
         "dilution_calculation": {"title": "Share count increase", "formulas": ["New shares / Shares before = 4,000,000 / 50,000,000 = 8.00%"],
            "inputs": [], "display": "8.00%", "value": 8.0, "unit": "%"}}
    ]}});
    let capital = observe::extract(
        &read("dart_major_report", &decisions, "2026-10-31"),
        "990308",
    );
    assert_eq!(capital.len(), 1, "an exchangeable bond stays a filing row");
    let bond = &capital[0];
    assert_eq!(bond.kind, ObservationKind::Capital);
    assert_eq!(bond.fact_date, "2026-10-30");
    assert_eq!(bond.facts["conversionPrice"], 9100.0);
    assert_eq!(bond.facts["floor"], 6370.0);
    assert_eq!(bond.facts["dilution"]["basis"], "calculated");
    assert_eq!(bond.facts["dilution"]["at"], "floor");
    assert_eq!(bond.facts["dilution"]["display"], "8.71%");
    assert_eq!(bond.facts["dilution"]["calc"]["value"], 8.7143);
    assert!(bond
        .evidence
        .url
        .as_deref()
        .unwrap()
        .ends_with("20261030990308"));
    let offering = observe::extract(&read("dart_major_report", &rights, "2026-10-31"), "990308");
    assert_eq!(offering[0].facts["shares"], 4_000_000.0);
    assert!(offering[0].facts["price"].is_null() && offering[0].facts["paymentDate"].is_null());
    assert_eq!(offering[0].facts["dilution"]["calc"]["value"], 8.0);

    let dividends = json!({"dataset_ref": {"dataset_id": "ds_div", "revision": 1}, "data": {"year": 2025, "rows": [
        {"filing_id": "20260310990308", "filed_at": "2026-03-10", "item": "dps_cash", "share_class": "common", "fiscal_year": 2025, "value": "300"},
        {"filing_id": "20260310990308", "filed_at": "2026-03-10", "item": "dps_cash", "share_class": "common", "fiscal_year": 2024, "value": "250"}
    ], "record_date": {"status": "found", "date": "2025-12-31", "filing_id": "20260205990308", "filed_at": "2026-02-05",
        "url": "https://dart.fss.or.kr/dsaf001/main.do?rcpNo=20260205990308"}}});
    let dividend = observe::extract(&read("dart_dividends", &dividends, "2026-10-31"), "990308");
    assert_eq!(dividend.len(), 1);
    assert_eq!(dividend[0].facts["amount"], 300.0);
    assert_eq!(dividend[0].facts["recordDate"], "2025-12-31");
    assert_eq!(dividend[0].fact_date, "2026-02-05");
    let mut in_notice = dividends.clone();
    in_notice["data"]["record_date"] = json!({"status": "in_notice", "date": null, "filing_id": "20260205990308", "filed_at": "2026-02-05"});
    let unread = observe::extract(&read("dart_dividends", &in_notice, "2026-10-31"), "990308");
    assert_eq!(unread[0].facts["recordInNotice"], true);
    assert_eq!(unread[0].fact_date, "2026-03-10");

    store
        .transact(|w| {
            let mut found = capital.clone();
            found.extend(offering.clone());
            found.extend(dividend.clone());
            record_observations(
                w.tx,
                LOUPIST,
                &page.asset_id,
                found,
                ObservationOrigin::Refresh,
                None,
                ms("2026-10-31"),
            )
        })
        .unwrap();
    let delta = store
        .transact(|w| delta_for(w.tx, LOUPIST, &page, None, ms("2026-09-01")))
        .unwrap();
    let kinds: Vec<_> = delta.rows.iter().map(|row| row.kind.as_str()).collect();
    assert!(kinds.contains(&"capital"));
    let view = store
        .transact(|w| page_view(w.tx, LOUPIST, &page.asset_id, 0, &context("2026-10-31")))
        .unwrap();
    let glance = view
        .glance
        .iter()
        .find(|row| row.key == "dividend")
        .unwrap();
    assert_eq!(glance.facts["recordDate"], "2025-12-31");
    assert!(view.flows.is_none());
}

#[test]
fn checks_name_only_what_the_last_check_read() {
    let (_root, store) = store();
    let page = page(&store);
    let context = ViewContext {
        connection_names: [
            ("opendart.disclosures", "OpenDART"),
            ("krx.market-data", "KRX"),
            ("koreainvestment.market-data", "Korea Investment"),
        ]
        .into_iter()
        .map(|(id, name)| (id.to_owned(), name.to_owned()))
        .collect(),
        ..context("2026-10-31")
    };
    let checks = |store: &KnowledgeStore| {
        store
            .transact(|w| page_view(w.tx, LOUPIST, &page.asset_id, 0, &context))
            .unwrap()
            .checks
    };
    // Before the first check the page says what it will read.
    let planned = checks(&store);
    assert_eq!(
        planned.kinds,
        ["filings", "results", "price", "dividends", "flows"]
    );
    assert_eq!(planned.connections, ["OpenDART", "KRX", "Korea Investment"]);
    assert_eq!(
        planned.not_yet,
        ["creditBalances", "shortSaleBalances", "lockUps"]
    );
    let status = |status: &str, connection: &str| RefreshStatus {
        status: status.into(),
        connection: connection.into(),
        at_ms: ms("2026-10-31"),
    };
    // OpenDART answered for filings but not results; KRX is switched off.
    store
        .transact(|w| {
            record_refresh(
                w.tx,
                LOUPIST,
                &page.asset_id,
                BTreeMap::from([
                    ("filings".to_owned(), status("ok", "OpenDART")),
                    ("results".to_owned(), status("failed", "OpenDART")),
                    ("price".to_owned(), status("needs_connection", "KRX")),
                    ("dividends".to_owned(), status("ok", "OpenDART")),
                    (
                        "flows".to_owned(),
                        status("needs_connection", "Korea Investment"),
                    ),
                ]),
                ms("2026-10-31"),
            )
        })
        .unwrap();
    let partial = checks(&store);
    assert_eq!(partial.kinds, ["filings", "dividends"]);
    assert_eq!(partial.connections, ["OpenDART"]);
    assert_eq!(
        partial
            .failures
            .iter()
            .map(|failure| (failure.kind.as_str(), failure.reason.as_str()))
            .collect::<Vec<_>>(),
        [
            ("flows", "needs_connection"),
            ("price", "needs_connection"),
            ("results", "failed")
        ]
    );
}

/// The morning message's event strip: followed assets' results and dividend
/// dates and open Journal checks in the next 14 days, soonest first, at most five.
#[test]
fn the_event_strip_lists_what_is_ahead_for_followed_assets_soonest_first() {
    use crate::knowledge::earnings::{self, DateSource};
    use crate::store::user_context::MapContextItem;
    use upcoming::{upcoming, UpcomingKind};

    let (_root, store) = store();
    let ensure = |symbol: &str, market: &str, name: &str| {
        store
            .transact(|w| {
                ensure_page(
                    w.tx,
                    LOUPIST,
                    &AssetIdentity {
                        symbol: symbol.into(),
                        market: market.into(),
                        name: name.into(),
                    },
                    None,
                    ms("2026-09-30"),
                )
            })
            .unwrap()
    };
    let byeolmuri = page(&store);
    let morrow = ensure("MORR", "NASDAQ", "Morrow Components");
    let solsup = ensure("SOLS", "NYSE", "Solsup Foods");
    // Opened once from an answer, never followed.
    let stranger = ensure("OPEN", "NYSE", "Opened Once");
    let results = |page: &PageRecord, date: &str, source: DateSource| {
        store
            .transact(|w| {
                earnings::upsert_event(
                    w.tx,
                    LOUPIST,
                    &page.asset_id,
                    "Q3 2026",
                    None,
                    day(date),
                    source,
                    ms("2026-10-20"),
                )
            })
            .unwrap();
    };
    results(&byeolmuri, "2026-11-14", DateSource::LegalDeadline);
    results(&morrow, "2026-11-04", DateSource::Calendar);
    results(&solsup, "2026-11-03", DateSource::Calendar);
    results(&stranger, "2026-11-03", DateSource::Calendar);
    // Solsup's results are already out: nothing to look ahead to.
    store
        .transact(|w| {
            w.tx.execute(
                "UPDATE knowledge_earnings_events SET status='reported' WHERE asset_id=?1",
                [&solsup.asset_id],
            )?;
            Ok(())
        })
        .unwrap();
    let dividend = |page: &PageRecord, ex: Option<&str>, record: Option<&str>| {
        store
            .transact(|w| {
                record_observations(
                    w.tx,
                    LOUPIST,
                    &page.asset_id,
                    vec![NewObservation {
                        kind: ObservationKind::Dividend,
                        fact_date: "2026-10-20".into(),
                        dedupe_key: format!("dividend:{}", ex.or(record).unwrap()),
                        facts: json!({"amount": 0.26, "currency": "USD", "exDate": ex, "recordDate": record}),
                        evidence: AssetEvidence {
                            tool: "equity_fundamental_dividends".into(),
                            title: "Dividends".into(),
                            url: Some("https://example.test/dividends".into()),
                            date: Some("2026-10-20".into()),
                            ..AssetEvidence::default()
                        },
                    }],
                    ObservationOrigin::Refresh,
                    None,
                    ms("2026-10-20"),
                )
            })
            .unwrap();
    };
    dividend(&morrow, Some("2026-11-06"), Some("2026-11-07"));
    dividend(&byeolmuri, None, Some("2026-11-20"));
    let check = |claim: &str, by_date: &str| JournalCheck {
        claim: claim.into(),
        metric: "gross margin".into(),
        direction: "above".into(),
        threshold: 55.0,
        unit: Some("%".into()),
        by_date: Some(by_date.into()),
        confidence: None,
    };
    let entry = |claim: &str, by_date: &str| {
        store
            .transact(|w| {
                journal::create(
                    w.tx,
                    LOUPIST,
                    journal::JournalInput {
                        judgment: claim.into(),
                        reasoning: String::new(),
                        entities: vec![],
                        check: Some(check(claim, by_date)),
                        thesis: None,
                    },
                    ms("2026-10-01"),
                )
            })
            .unwrap()
    };
    let soon = entry("Inventory days fall below 50", "2026-11-02");
    let thesis = write_entry(
        &store,
        &byeolmuri.asset_id,
        ThesisRole::Line,
        "Gross margin stays above 55%",
        Some(check("Gross margin stays above 55%", "2026-11-04")),
        ms("2026-10-01"),
    );
    let last_day = entry("Orders recover", "2026-11-15");
    entry("Overdue already", "2026-10-31");
    entry("Too far ahead", "2026-11-30");
    let archived = entry("Archived check", "2026-11-03");
    store
        .transact(|w| journal::archive(w.tx, LOUPIST, &archived.journal_id, true, ms("2026-10-02")))
        .unwrap();

    let follow = |symbol: &str, market: &str| MapContextItem {
        symbol: symbol.into(),
        market: market.into(),
        display_name: None,
    };
    let followed = |today: &str| ViewContext {
        watchlist: vec![follow("990308", "KRX"), follow("SOLS", "NYSE")],
        holdings: vec![follow("MORR", "NASDAQ")],
        ..context(today)
    };
    let strip = |today: &str| {
        store
            .transact(|w| upcoming(w.tx, LOUPIST, &followed(today)))
            .unwrap()
    };
    let rows = |events: &[upcoming::UpcomingEvent]| {
        events
            .iter()
            .map(|event| {
                (
                    event.date.clone(),
                    event.kind,
                    event
                        .asset
                        .as_ref()
                        .map(|asset| asset.name.clone())
                        .or(event.claim.clone())
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>()
    };
    let row = |date: &str, kind: UpcomingKind, name: &str| (date.to_owned(), kind, name.to_owned());

    let today = strip("2026-11-01");
    assert_eq!(
        rows(&today),
        [
            row(
                "2026-11-02",
                UpcomingKind::Check,
                "Inventory days fall below 50"
            ),
            row("2026-11-04", UpcomingKind::Results, "Morrow Components"),
            row("2026-11-04", UpcomingKind::Check, "Byeolmuri Semiconductor"),
            row("2026-11-06", UpcomingKind::ExDividend, "Morrow Components"),
            row(
                "2026-11-14",
                UpcomingKind::Results,
                "Byeolmuri Semiconductor"
            ),
        ],
        "the check on the window's last day is the sixth and is cut"
    );
    // A results row says where its date comes from and opens its event.
    assert_eq!(
        (
            today[1].date_source,
            today[1].event_id.as_deref(),
            today[1].period.as_deref()
        ),
        (
            Some(DateSource::Calendar),
            Some(earnings::event_id(&morrow.asset_id, "Q3 2026").as_str()),
            Some("Q3 2026")
        )
    );
    assert_eq!(today[4].date_source, Some(DateSource::LegalDeadline));
    // A check opens its Journal entry and names the asset its thesis belongs to.
    assert_eq!(
        today[0].journal_id.as_deref(),
        Some(soon.journal_id.as_str())
    );
    assert!(today[0].asset.is_none());
    assert_eq!(
        (
            today[2].journal_id.as_deref(),
            today[2].claim.as_deref(),
            today[2].asset.as_ref().map(|asset| asset.asset_id.as_str())
        ),
        (
            Some(thesis.journal_id.as_str()),
            Some("Gross margin stays above 55%"),
            Some(byeolmuri.asset_id.as_str())
        )
    );
    // Holdings never travel with an event: the asset is named, nothing more.
    let wire = serde_json::to_value(&today[3]).unwrap();
    assert_eq!(
        wire,
        json!({"kind": "exDividend", "date": "2026-11-06", "asset": {"assetId": morrow.asset_id, "symbol": "MORR", "market": "NASDAQ", "name": "Morrow Components"}})
    );

    // Five days on: today counts, the window's last day counts, the past does not.
    assert_eq!(
        rows(&strip("2026-11-06")),
        [
            row("2026-11-06", UpcomingKind::ExDividend, "Morrow Components"),
            row(
                "2026-11-14",
                UpcomingKind::Results,
                "Byeolmuri Semiconductor"
            ),
            row("2026-11-15", UpcomingKind::Check, "Orders recover"),
            row(
                "2026-11-20",
                UpcomingKind::RecordDate,
                "Byeolmuri Semiconductor"
            ),
        ]
    );
    assert_eq!(strip("2026-11-06")[2].journal_id, Some(last_day.journal_id));
    // Nothing followed and no checks ahead: no strip.
    assert!(store
        .transact(|w| upcoming(w.tx, LOUPIST, &context("2026-12-01")))
        .unwrap()
        .is_empty());
}

#[test]
fn match_entity_returns_none_when_two_nodes_share_the_symbol() {
    let (_root, store) = store();
    let page = page(&store);
    let node = |id: &str, name: &str| {
        json!({"id": id, "name": name, "aliases": [], "description": "", "kind": "Organization",
               "sourceRole": "entity", "properties": {"ticker": "990308"}})
    };
    let body = |nodes: Vec<Value>| -> WorldSnapshotBody {
        serde_json::from_value(json!({"nodes": nodes, "claims": [], "annotations": []})).unwrap()
    };
    assert!(match_entity(&body(vec![node("n1", "One"), node("n2", "Two")]), &page).is_none());
    assert_eq!(
        match_entity(&body(vec![node("n1", "One")]), &page).map(|(id, _)| id),
        Some("n1".to_owned())
    );
}
