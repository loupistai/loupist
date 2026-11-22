use super::*;
use crate::knowledge::{journal, JournalCheck, KnowledgeStore};

const LOUPIST: &str = "loupist-a";

fn asset() -> AssetRow {
    AssetRow {
        asset_id: asset_id("990308", "KRX"),
        name: "Byeolmuri Semiconductor".into(),
        reference: None,
        created_at_ms: ms("2026-09-30"),
        followed_at_ms: Some(ms("2026-09-30")),
    }
}
fn page(store: &KnowledgeStore) -> PageRecord {
    let asset = asset();
    store
        .transact(|w| ensure_asset_page(w.tx, LOUPIST, &asset, asset.created_at_ms))
        .unwrap()
}
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
fn move_listing_skips_unreadable_rows_but_undo_stays_strict() {
    for (column, value) in [
        ("from_verdict", "future"),
        ("to_verdict", "future"),
        ("evidence_json", "{"),
        ("line_json", "{"),
    ] {
        let (_root, store) = store();
        let page = page(&store);
        store.transact(|w| {
            for (id, at) in [("good-before", 1), ("odd", 2), ("good-after", 3)] {
                w.tx.execute("INSERT INTO knowledge_asset_moves
                    (move_id,loupist_id,asset_id,to_verdict,fact_date,reason,evidence_json,actor,at_ms)
                    VALUES (?1,?2,?3,'holding','2026-10-01','Reason','[]','loupist',?4)",
                    params![id, LOUPIST, page.asset_id, at])?;
            }
            w.tx.execute(&format!("UPDATE knowledge_asset_moves SET {column}=?1 WHERE move_id='odd'"), [value])?;
            Ok(())
        }).unwrap();
        let listed = store
            .read(|w| moves(w.tx, LOUPIST, &page.asset_id))
            .unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|m| m.move_id.as_str())
                .collect::<Vec<_>>(),
            ["good-before", "good-after"],
            "{column}"
        );
        assert!(store
            .transact(|w| undo(w.tx, LOUPIST, &page.asset_id, "good-after", 4))
            .is_err());
    }
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
        .transact(|w| page_view(w.tx, LOUPIST, asset()))
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
        .transact(|w| page_view(w.tx, LOUPIST, asset()))
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
                world_revision_id: None,
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
        .transact(|w| page_view(w.tx, LOUPIST, asset()))
        .unwrap()
        .read
        .is_none());
}

#[test]
fn as_of_page_uses_the_latest_visible_line_reading() {
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
    for (observed, fact, recorded) in [
        (20.0, "2026-10-01", "2026-10-01"),
        (21.0, "2026-10-02", "2026-10-05"),
        (27.0, "2026-10-05", "2026-10-05"),
    ] {
        store
            .transact(|w| {
                apply_update(
                    w.tx,
                    LOUPIST,
                    &update(
                        &page.asset_id,
                        None,
                        Some((line.journal_id.clone(), observed)),
                        fact,
                    ),
                    None,
                    ms(recorded),
                )
            })
            .unwrap();
    }
    let historical = store
        .read(|w| model_read(w.tx, LOUPIST, asset(), Some(day("2026-10-03"))))
        .unwrap();
    assert_eq!(historical["lines"][0]["status"], "not_crossed");
    assert_eq!(historical["lines"][0]["last"]["observed"], 20.0);
    assert_eq!(historical["history"], json!([]));
    assert!(historical["read"].is_null());
    assert!(historical["savedRead"].is_null());
    assert!(historical.get("currentPrice").is_none());
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
}
