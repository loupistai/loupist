//! Results events without a model (finance-earnings-snapshot.md §8): dates,
//! releases, final reports, locks, Rust's differences and revisions, and when
//! each capture is due.
use super::*;
use crate::knowledge::{
    assets::{self, AssetIdentity, NewObservation, ObservationOrigin},
    KnowledgeStore,
};
use chrono::TimeZone;
use serde_json::json;

const LOUPIST: &str = "loupist-a";

fn day(value: &str) -> NaiveDate {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
}

fn store() -> (tempfile::TempDir, KnowledgeStore) {
    let root = tempfile::tempdir().unwrap();
    let store = KnowledgeStore::open(root.path().join("knowledge.sqlite")).unwrap();
    (root, store)
}

fn page(store: &KnowledgeStore, symbol: &str, market: &str) -> PageRecord {
    store
        .transact(|w| {
            assets::ensure_page(
                w.tx,
                LOUPIST,
                &AssetIdentity {
                    symbol: symbol.into(),
                    market: market.into(),
                    name: format!("{symbol} Holdings"),
                },
                None,
                1,
            )
        })
        .unwrap()
}

fn evidence(url: &str, date: &str) -> AssetEvidence {
    AssetEvidence {
        tool: "equity_fundamental_filings".into(),
        provider: Some("sec".into()),
        title: "SEC EDGAR".into(),
        url: Some(url.into()),
        date: Some(date.into()),
        ..AssetEvidence::default()
    }
}

fn filing(
    id: &str,
    date: &str,
    form: Option<&str>,
    title: &str,
    items: Option<&str>,
) -> NewObservation {
    let mut facts = json!({"form": form, "title": title, "filingId": id});
    if let Some(items) = items {
        facts["items"] = json!(items);
    }
    NewObservation {
        kind: ObservationKind::Filing,
        fact_date: date.into(),
        dedupe_key: format!("filing:{id}"),
        facts,
        evidence: evidence(&format!("https://example.test/{id}"), date),
    }
}

fn observe(store: &KnowledgeStore, page: &PageRecord, observations: Vec<NewObservation>) {
    store
        .transact(|w| {
            assets::record_observations(
                w.tx,
                LOUPIST,
                &page.asset_id,
                observations,
                ObservationOrigin::Refresh,
                None,
                10,
            )
        })
        .unwrap();
}

fn sync_at(store: &KnowledgeStore, page: &PageRecord, today: &str) -> SyncOutcome {
    store
        .transact(|w| sync(w.tx, LOUPIST, page, day(today), 100))
        .unwrap()
}

fn events_of(store: &KnowledgeStore, page: &PageRecord) -> Vec<EventRecord> {
    store
        .transact(|w| events_for(w.tx, LOUPIST, &page.asset_id))
        .unwrap()
}

#[test]
fn one_sync_keeps_the_first_audit_with_or_without_a_period_in_its_title() {
    for title in ["감사보고서제출 (2025.12)", "감사보고서"] {
        let (_root, store) = store();
        let page = page(&store, "005930", "KRX");
        store
            .transact(|w| {
                upsert_event(
                    w.tx,
                    LOUPIST,
                    &page.asset_id,
                    "Q4 2025",
                    Some(day("2025-12-31")),
                    day("2026-03-31"),
                    DateSource::LegalDeadline,
                    1,
                )
            })
            .unwrap();
        observe(
            &store,
            &page,
            vec![
                filing("first-audit", "2026-03-12", None, title, None),
                filing("later-audit", "2026-03-13", None, title, None),
            ],
        );
        sync_at(&store, &page, "2026-03-14");
        let audit = events_of(&store, &page)
            .into_iter()
            .find(|event| event.period == "Q4 2025")
            .unwrap()
            .flags
            .audit
            .unwrap();
        assert_eq!(audit.filing_id, "first-audit", "{title}");
        sync_at(&store, &page, "2026-03-14");
        let audit = events_of(&store, &page)
            .into_iter()
            .find(|event| event.period == "Q4 2025")
            .unwrap()
            .flags
            .audit
            .unwrap();
        assert_eq!(audit.filing_id, "first-audit", "{title}");
    }
}

#[test]
fn one_sync_keeps_the_first_matching_release_and_final_report() {
    let (_root, store) = store();
    let page = page(&store, "MORR", "NASDAQ");
    store
        .transact(|w| {
            upsert_event(
                w.tx,
                LOUPIST,
                &page.asset_id,
                "Q3 2026",
                Some(day("2026-09-30")),
                day("2026-10-30"),
                DateSource::Calendar,
                1,
            )
        })
        .unwrap();
    observe(
        &store,
        &page,
        vec![
            filing(
                "first-release",
                "2026-10-29",
                Some("8-K"),
                "Results",
                Some("2.02"),
            ),
            filing(
                "later-release",
                "2026-10-30",
                Some("8-K"),
                "Results",
                Some("2.02"),
            ),
            filing(
                "first-final",
                "2026-10-31",
                Some("10-Q"),
                "Quarterly report",
                None,
            ),
            filing(
                "later-final",
                "2026-11-01",
                Some("10-Q"),
                "Quarterly report",
                None,
            ),
        ],
    );
    let outcome = sync_at(&store, &page, "2026-11-02");
    assert_eq!(outcome.released.len(), 1);
    let event = events_of(&store, &page).remove(0);
    assert_eq!(event.release.unwrap().filing_id, "first-release");
    assert_eq!(event.final_report.unwrap().filing_id, "first-final");
    assert!(sync_at(&store, &page, "2026-11-02").released.is_empty());
}

#[test]
fn stale_filing_snapshots_preserve_the_first_release_final_and_current_flags() {
    let (_root, store) = store();
    let page = page(&store, "MORR", "NASDAQ");
    store
        .transact(|w| {
            let (event, _) = upsert_event(
                w.tx,
                LOUPIST,
                &page.asset_id,
                "Q3 2026",
                Some(day("2026-09-30")),
                day("2026-10-30"),
                DateSource::Calendar,
                1,
            )?;
            let report = |id: &str| FilingRef {
                filing_id: id.into(),
                date: "2026-10-30".into(),
                form: Some("8-K".into()),
                title: "Results".into(),
                evidence: evidence("https://example.test/results", "2026-10-30"),
            };
            let audit = report("audit");
            edit_flags(w.tx, LOUPIST, &event.event_id, 2, |flags| {
                flags.audit = Some(audit.clone())
            })?;
            assert!(set_final(w.tx, LOUPIST, &event, &report("first-final"), 3)?);
            assert!(!set_final(
                w.tx,
                LOUPIST,
                &event,
                &report("later-final"),
                4
            )?);
            assert!(set_release(
                w.tx,
                LOUPIST,
                &event,
                &report("first-release"),
                5
            )?);
            assert!(!set_release(
                w.tx,
                LOUPIST,
                &event,
                &report("later-release"),
                6
            )?);
            let current = get_event(w.tx, LOUPIST, &event.event_id)?.unwrap();
            assert_eq!(current.status, Status::Final);
            assert_eq!(current.flags.audit, Some(audit));
            assert_eq!(current.flags.release_seen_ms, Some(5));
            assert_eq!(current.release.unwrap().filing_id, "first-release");
            assert_eq!(current.final_report.unwrap().filing_id, "first-final");
            Ok(())
        })
        .unwrap();
}

#[test]
fn us_dates_releases_and_final_reports_follow_the_calendar_and_filings() {
    let (_root, store) = store();
    let morrow = page(&store, "MORR", "NASDAQ");
    // The calendar announces a date; the baseline filing is old.
    observe(
        &store,
        &morrow,
        vec![
            NewObservation {
                kind: ObservationKind::Earnings,
                fact_date: "2026-10-20".into(),
                dedupe_key: "earnings:2026-10-30".into(),
                facts: json!({"date": "2026-10-30", "periodEnd": "2026-09-30"}),
                evidence: evidence("https://example.test/calendar", "2026-10-20"),
            },
            filing(
                "old-10q",
                "2026-08-01",
                Some("10-Q"),
                "Quarterly report",
                None,
            ),
        ],
    );
    sync_at(&store, &morrow, "2026-10-20");
    let events = events_of(&store, &morrow);
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(
        (event.period.as_str(), event.report_date),
        ("Q3 2026", day("2026-10-30"))
    );
    assert_eq!(event.date_source, DateSource::Calendar);
    // A moved date updates the same event.
    observe(
        &store,
        &morrow,
        vec![NewObservation {
            kind: ObservationKind::Earnings,
            fact_date: "2026-10-22".into(),
            dedupe_key: "earnings:2026-10-29".into(),
            facts: json!({"date": "2026-10-29", "periodEnd": "2026-09-30"}),
            evidence: evidence("https://example.test/calendar2", "2026-10-22"),
        }],
    );
    sync_at(&store, &morrow, "2026-10-22");
    let events = events_of(&store, &morrow);
    assert_eq!(
        (events.len(), events[0].report_date),
        (1, day("2026-10-29"))
    );
    // Shown 14 days ahead; a passed date with no release seen yet stays
    // current ("Not reported yet") for 30 days.
    let current = |today: &str| current_event(&events, day(today)).map(|event| event.report_date);
    assert_eq!(current("2026-10-14"), None);
    assert_eq!(current("2026-10-15"), Some(day("2026-10-29")));
    assert_eq!(current("2026-10-31"), Some(day("2026-10-29")));
    assert_eq!(current("2026-11-29"), None);
    // An 8-K on the date (items 2.02) is the release; a 10-Q after it is final.
    observe(
        &store,
        &morrow,
        vec![filing(
            "8k",
            "2026-10-29",
            Some("8-K"),
            "Current report",
            Some("2.02,9.01"),
        )],
    );
    let outcome = sync_at(&store, &morrow, "2026-10-29");
    assert_eq!(outcome.released, vec![events[0].event_id.clone()]);
    let event = &events_of(&store, &morrow)[0];
    assert_eq!(event.status, Status::Reported);
    assert_eq!(event.release.as_ref().unwrap().filing_id, "8k");
    observe(
        &store,
        &morrow,
        vec![filing(
            "10q",
            "2026-11-06",
            Some("10-Q"),
            "Quarterly report",
            None,
        )],
    );
    sync_at(&store, &morrow, "2026-11-06");
    let event = &events_of(&store, &morrow)[0];
    assert_eq!(event.status, Status::Final);
    assert_eq!(event.final_report.as_ref().unwrap().filing_id, "10q");
}

#[test]
fn korean_dates_come_from_the_ir_notice_else_the_legal_deadline_and_the_chain_follows_dart() {
    let (_root, store) = store();
    let solsup = page(&store, "990221", "KRX");
    observe(
        &store,
        &solsup,
        vec![
            filing(
                "20260101000001",
                "2026-01-20",
                None,
                "사업보고서 (2025.12)",
                None,
            ),
            filing(
                "20260415000001",
                "2026-04-15",
                None,
                "기업설명회(IR)개최(안내공시)",
                None,
            ),
        ],
    );
    let outcome = sync_at(&store, &solsup, "2026-04-16");
    // Q1's legal deadline (45 days) until the IR notice is read.
    let event = events_of(&store, &solsup)
        .into_iter()
        .find(|event| event.period == "Q1 2026")
        .unwrap();
    assert_eq!(
        (event.report_date, event.date_source),
        (day("2026-05-15"), DateSource::LegalDeadline)
    );
    assert_eq!(outcome.ir_notices.len(), 1);
    let notice = outcome.ir_notices[0].clone();
    let read_date =
        crate::knowledge::watch::detect::ir_date("개최일시: 2026-04-28 16:00", notice.filed);
    store
        .transact(|w| apply_ir_date(w.tx, LOUPIST, &solsup, &notice, read_date, 200))
        .unwrap();
    let event = events_of(&store, &solsup)
        .into_iter()
        .find(|event| event.period == "Q1 2026")
        .unwrap();
    assert_eq!(
        (event.report_date, event.date_source),
        (day("2026-04-28"), DateSource::IrNotice)
    );
    // Read once: the next sync does not ask again.
    assert!(sync_at(&store, &solsup, "2026-04-17").ir_notices.is_empty());
    // The 잠정실적 disclosure is the release; the 분기보고서 closes the chain.
    observe(
        &store,
        &solsup,
        vec![filing(
            "20260428000009",
            "2026-04-28",
            None,
            "연결재무제표기준영업(잠정)실적(공정공시)",
            None,
        )],
    );
    sync_at(&store, &solsup, "2026-04-28");
    let event = events_of(&store, &solsup)
        .into_iter()
        .find(|event| event.period == "Q1 2026")
        .unwrap();
    assert_eq!(event.status, Status::Reported);
    let context = SnapshotContext {
        preparing: false,
        consensus_connected: false,
    };
    let view = store
        .transact(|w| snapshot(w.tx, LOUPIST, &event, &solsup, &context))
        .unwrap();
    assert_eq!(view.chain.len(), 2);
    assert_eq!(
        (view.chain[0].stage.as_str(), view.chain[0].filed),
        ("preliminary", true)
    );
    assert_eq!(
        (
            view.chain[1].stage.as_str(),
            view.chain[1].filed,
            view.chain[1].due_by.as_deref()
        ),
        ("quarterly", false, Some("2026-05-15"))
    );
    // A Korean card never asks for an FMP key.
    assert!(!view.consensus_needs_key);
    observe(
        &store,
        &solsup,
        vec![filing(
            "20260514000001",
            "2026-05-14",
            None,
            "분기보고서 (2026.03)",
            None,
        )],
    );
    sync_at(&store, &solsup, "2026-05-14");
    let event = events_of(&store, &solsup)
        .into_iter()
        .find(|event| event.period == "Q1 2026")
        .unwrap();
    assert_eq!(event.status, Status::Final);
    // An annual event adds the audit report to its chain.
    let annual = events_of(&store, &solsup)
        .into_iter()
        .find(|event| event.period == "Q4 2025");
    assert!(annual.is_none(), "an old annual deadline is not recreated");
}

#[allow(clippy::too_many_arguments)]
fn row(
    side: Side,
    measure: &str,
    kind: ValueKind,
    point: Option<&str>,
    range: Option<(&str, &str)>,
    unit: Unit,
    stage: Option<Stage>,
    fact_date: &str,
) -> NewRow {
    NewRow {
        side,
        measure: measure.into(),
        basis: String::new(),
        period: "Q3 2026".into(),
        low: range.map(|(low, _)| low.into()),
        high: range.map(|(_, high)| high.into()),
        point: point.map(str::to_owned),
        unit,
        currency: (unit == Unit::Currency).then(|| "USD".into()),
        kind,
        stage,
        provider: (kind == ValueKind::Consensus).then(|| "FMP".into()),
        evidence: vec![evidence("https://example.test/source", fact_date)],
        extra: json!({}),
        fact_date: day(fact_date),
        captured_as_of: None,
    }
}

fn reported_event(store: &KnowledgeStore, page: &PageRecord) -> EventRecord {
    store
        .transact(|w| {
            let (event, _) = upsert_event(
                w.tx,
                LOUPIST,
                &page.asset_id,
                "Q3 2026",
                Some(day("2026-09-30")),
                day("2026-10-30"),
                DateSource::Calendar,
                1,
            )?;
            Ok(event)
        })
        .unwrap()
}

#[test]
fn differences_ranges_points_and_revisions_are_computed_by_rust() {
    let (_root, store) = store();
    let morrow = page(&store, "MORR", "NASDAQ");
    let event = reported_event(&store, &morrow);
    let today = day("2026-10-20");
    // Before the report date expectations can be written.
    assert!(expected_writable(&event, &[], today, None).is_ok());
    store
        .transact(|w| {
            insert_rows(
                w.tx,
                LOUPIST,
                &event.event_id,
                &[
                    row(
                        Side::Expected,
                        "revenue",
                        ValueKind::Guidance,
                        None,
                        Some(("1200000000", "1250000000")),
                        Unit::Currency,
                        None,
                        "2026-07-30",
                    ),
                    row(
                        Side::Expected,
                        "revenue",
                        ValueKind::Consensus,
                        Some("1230000000"),
                        None,
                        Unit::Currency,
                        None,
                        "2026-10-29",
                    ),
                    row(
                        Side::Expected,
                        "operatingMargin",
                        ValueKind::Guidance,
                        Some("16.0"),
                        None,
                        Unit::Percent,
                        None,
                        "2026-07-30",
                    ),
                ],
                Origin::Host,
                None,
                10,
            )?;
            insert_rows(
                w.tx,
                LOUPIST,
                &event.event_id,
                &[
                    row(
                        Side::Reported,
                        "revenue",
                        ValueKind::Reported,
                        Some("1270000000"),
                        None,
                        Unit::Currency,
                        Some(Stage::Preliminary),
                        "2026-10-30",
                    ),
                    row(
                        Side::Reported,
                        "operatingMargin",
                        ValueKind::Reported,
                        Some("14.8"),
                        None,
                        Unit::Percent,
                        Some(Stage::Preliminary),
                        "2026-10-30",
                    ),
                    NewRow {
                        period: "Q4 2026".into(),
                        ..row(
                            Side::GuidanceNew,
                            "revenue",
                            ValueKind::Guidance,
                            None,
                            Some(("1300000000", "1350000000")),
                            Unit::Currency,
                            None,
                            "2026-10-30",
                        )
                    },
                ],
                Origin::Chat,
                Some("answer-1"),
                20,
            )
        })
        .unwrap();
    let context = SnapshotContext {
        preparing: false,
        consensus_connected: true,
    };
    let view = store
        .transact(|w| snapshot(w.tx, LOUPIST, &event, &morrow, &context))
        .unwrap();
    let revenue = view
        .measures
        .iter()
        .find(|measure| measure.key.as_deref() == Some("revenue") && !measure.guidance)
        .unwrap();
    assert_eq!(revenue.expected.len(), 2);
    assert_eq!(revenue.expected[0].kind, ValueKind::Guidance);
    let DifferenceView::Range { position, calc } = &revenue.differences[0] else {
        panic!("{:?}", revenue.differences);
    };
    assert_eq!(*position, diff::Position::Above);
    assert_eq!(calc.as_ref().unwrap().display, "+1.6%");
    let DifferenceView::Change { calc } = &revenue.differences[1] else {
        panic!("{:?}", revenue.differences);
    };
    assert_eq!(calc.display, "+3.3%");
    let margin = view
        .measures
        .iter()
        .find(|measure| measure.key.as_deref() == Some("operatingMargin"))
        .unwrap();
    let DifferenceView::Change { calc } = &margin.differences[0] else {
        panic!("{:?}", margin.differences);
    };
    assert_eq!(
        (calc.display.as_str(), calc.unit.as_str()),
        ("−1.2 pp", "pp")
    );
    let guidance = view
        .measures
        .iter()
        .find(|measure| measure.guidance)
        .unwrap();
    assert_eq!(guidance.period, "Q4 2026");
    assert!(matches!(
        guidance.differences[0],
        DifferenceView::Guidance {
            change: diff::GuidanceChange::New,
            ..
        }
    ));
    // No difference text was ever stored: the rows hold values only.
    // Preliminary → final: a changed figure is a revision, an unchanged one is not.
    store
        .transact(|w| {
            insert_rows(
                w.tx,
                LOUPIST,
                &event.event_id,
                &[
                    row(
                        Side::Reported,
                        "revenue",
                        ValueKind::Reported,
                        Some("1268000000"),
                        None,
                        Unit::Currency,
                        Some(Stage::Final),
                        "2026-11-06",
                    ),
                    row(
                        Side::Reported,
                        "operatingMargin",
                        ValueKind::Reported,
                        Some("14.8"),
                        None,
                        Unit::Percent,
                        Some(Stage::Final),
                        "2026-11-06",
                    ),
                ],
                Origin::Host,
                None,
                30,
            )
        })
        .unwrap();
    let final_event = EventRecord {
        status: Status::Final,
        ..event.clone()
    };
    let view = store
        .transact(|w| snapshot(w.tx, LOUPIST, &final_event, &morrow, &context))
        .unwrap();
    assert_eq!(view.revisions.len(), 1, "{:?}", view.revisions);
    assert_eq!(view.revisions[0].key.as_deref(), Some("revenue"));
    assert!(!view.unchanged);
    // The final figure leads the reported column.
    let revenue = view
        .measures
        .iter()
        .find(|measure| measure.key.as_deref() == Some("revenue") && !measure.guidance)
        .unwrap();
    assert_eq!(revenue.reported[0].stage, Some(Stage::Final));
}

#[test]
fn another_basis_is_not_comparable_and_a_raised_guidance_says_so() {
    let (_root, store) = store();
    let morrow = page(&store, "MORR", "NASDAQ");
    let event = reported_event(&store, &morrow);
    store
        .transact(|w| {
            insert_rows(
                w.tx,
                LOUPIST,
                &event.event_id,
                &[
                    NewRow {
                        basis: "adjusted".into(),
                        ..row(
                            Side::Expected,
                            "eps",
                            ValueKind::Consensus,
                            Some("0.42"),
                            None,
                            Unit::PerShare,
                            None,
                            "2026-10-29",
                        )
                    },
                    NewRow {
                        basis: "GAAP".into(),
                        ..row(
                            Side::Reported,
                            "eps",
                            ValueKind::Reported,
                            Some("0.40"),
                            None,
                            Unit::PerShare,
                            Some(Stage::Preliminary),
                            "2026-10-30",
                        )
                    },
                    NewRow {
                        period: "Q4 2026".into(),
                        ..row(
                            Side::Expected,
                            "revenue",
                            ValueKind::Guidance,
                            None,
                            Some(("1250000000", "1300000000")),
                            Unit::Currency,
                            None,
                            "2026-07-30",
                        )
                    },
                ],
                Origin::Chat,
                None,
                10,
            )?;
            insert_rows(
                w.tx,
                LOUPIST,
                &event.event_id,
                &[NewRow {
                    period: "Q4 2026".into(),
                    ..row(
                        Side::GuidanceNew,
                        "revenue",
                        ValueKind::Guidance,
                        None,
                        Some(("1300000000", "1350000000")),
                        Unit::Currency,
                        None,
                        "2026-10-30",
                    )
                }],
                Origin::Chat,
                None,
                20,
            )
        })
        .unwrap();
    let view = store
        .transact(|w| {
            snapshot(
                w.tx,
                LOUPIST,
                &event,
                &morrow,
                &SnapshotContext {
                    preparing: false,
                    consensus_connected: true,
                },
            )
        })
        .unwrap();
    let eps = view
        .measures
        .iter()
        .find(|measure| measure.key.as_deref() == Some("eps") && measure.expected.len() == 1)
        .unwrap();
    assert_eq!(
        eps.differences[0],
        DifferenceView::NotComparable {
            reason: diff::NotComparable::Basis
        }
    );
    let guidance = view
        .measures
        .iter()
        .find(|measure| measure.guidance)
        .unwrap();
    assert!(matches!(
        guidance.differences[0],
        DifferenceView::Guidance {
            change: diff::GuidanceChange::Raised,
            calc: Some(_)
        }
    ));
}

#[test]
fn expectations_lock_at_the_report_date_and_a_late_capture_writes_them_once() {
    let (_root, store) = store();
    let morrow = page(&store, "MORR", "NASDAQ");
    let event = reported_event(&store, &morrow);
    assert!(expected_writable(&event, &[], day("2026-10-29"), None).is_ok());
    assert_eq!(
        expected_writable(&event, &[], day("2026-10-30"), None),
        Err("earnings_expected_locked")
    );
    // A late capture under its host cutoff, once.
    assert!(expected_writable(&event, &[], day("2026-10-31"), Some(day("2026-10-29"))).is_ok());
    store
        .transact(|w| {
            insert_rows(
                w.tx,
                LOUPIST,
                &event.event_id,
                &[row(
                    Side::Expected,
                    "revenue",
                    ValueKind::Guidance,
                    Some("1230000000"),
                    None,
                    Unit::Currency,
                    None,
                    "2026-07-30",
                )],
                Origin::Host,
                None,
                10,
            )
        })
        .unwrap();
    let existing = store
        .transact(|w| rows(w.tx, LOUPIST, &event.event_id))
        .unwrap();
    assert_eq!(
        expected_writable(
            &event,
            &existing,
            day("2026-10-31"),
            Some(day("2026-10-29"))
        ),
        Err("earnings_expected_locked")
    );
    // A row needs a source, and a range needs both ends.
    let bad = NewRow {
        evidence: vec![AssetEvidence::default()],
        ..row(
            Side::Expected,
            "revenue",
            ValueKind::Guidance,
            Some("1"),
            None,
            Unit::Currency,
            None,
            "2026-07-30",
        )
    };
    assert!(store
        .transact(|w| insert_rows(
            w.tx,
            LOUPIST,
            &event.event_id,
            &[bad],
            Origin::Host,
            None,
            11
        ))
        .is_err());
}

#[test]
fn captures_are_due_the_evening_before_late_after_the_date_and_half_an_hour_after_a_release() {
    let base = EventRecord {
        event_id: "ern_0000000000000001".into(),
        asset_id: "ast_0000000000000001".into(),
        period: "Q3 2026".into(),
        fiscal_period_end: Some(day("2026-09-30")),
        report_date: day("2026-10-30"),
        date_source: DateSource::Calendar,
        status: Status::Upcoming,
        release: None,
        final_report: None,
        before_at_ms: None,
        before_as_of: None,
        after_at_ms: None,
        final_at_ms: None,
        flags: Flags::default(),
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    let new_york = |y, m, d, h, min| {
        chrono_tz::America::New_York
            .with_ymd_and_hms(y, m, d, h, min, 0)
            .unwrap()
            .with_timezone(&Utc)
    };
    // 17:59 the day before: not yet, and the next look is at 18:00.
    let (due_now, next) = due(
        std::slice::from_ref(&base),
        "NASDAQ",
        new_york(2026, 10, 29, 17, 59),
    );
    assert!(due_now.is_empty());
    assert_eq!(next, Some(new_york(2026, 10, 29, 18, 0).timestamp_millis()));
    let (due_now, _) = due(
        std::slice::from_ref(&base),
        "NASDAQ",
        new_york(2026, 10, 29, 18, 0),
    );
    assert_eq!(
        (
            due_now.len(),
            due_now[0].side.clone(),
            due_now[0].late_as_of
        ),
        (1, CaptureSide::Before, None)
    );
    // The app was closed: on the report date the capture is late, cut off the day before.
    let (due_now, _) = due(
        std::slice::from_ref(&base),
        "NASDAQ",
        new_york(2026, 10, 30, 9, 0),
    );
    assert_eq!(due_now[0].late_as_of, Some(day("2026-10-29")));
    // A release seen at 16:05: the after-capture waits 30 minutes.
    let released = EventRecord {
        status: Status::Reported,
        before_at_ms: Some(1),
        release: Some(FilingRef {
            form: Some("8-K".into()),
            title: "Current report".into(),
            date: "2026-10-30".into(),
            filing_id: "8k".into(),
            evidence: evidence("https://example.test/8k", "2026-10-30"),
        }),
        flags: Flags {
            release_seen_ms: Some(new_york(2026, 10, 30, 16, 5).timestamp_millis()),
            ..Flags::default()
        },
        ..base.clone()
    };
    let (due_now, next) = due(
        std::slice::from_ref(&released),
        "NASDAQ",
        new_york(2026, 10, 30, 16, 20),
    );
    assert!(due_now.is_empty());
    assert_eq!(
        next,
        Some(new_york(2026, 10, 30, 16, 35).timestamp_millis())
    );
    let (due_now, _) = due(
        std::slice::from_ref(&released),
        "NASDAQ",
        new_york(2026, 10, 30, 16, 40),
    );
    assert_eq!(due_now[0].side, CaptureSide::After);
    // A legal deadline is only a bound: captured two weeks before it.
    let deadline = EventRecord {
        date_source: DateSource::LegalDeadline,
        report_date: day("2026-11-14"),
        ..base
    };
    let seoul = chrono_tz::Asia::Seoul
        .with_ymd_and_hms(2026, 10, 31, 18, 0, 0)
        .unwrap()
        .with_timezone(&Utc);
    let (due_now, _) = due(std::slice::from_ref(&deadline), "KRX", seoul);
    assert_eq!(due_now.len(), 1);
}
