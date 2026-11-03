use super::*;
use crate::commands::tests::{chat, profile, seed_profile};
use crate::knowledge::{assets, journal, JournalCheck};

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
        capture: Arc::new(ToolCapture::for_chat("assistant-thesis-test".into())),
        loupist_id: LOUPIST.into(),
        capability_ids: BTreeSet::new(),
    }
}

/// A page for Byeolmuri with one reason and one line; returns (asset id, line id).
fn thesis(executor: &AppToolExecutor) -> (String, String) {
    crate::commands::assets::with_assets(&executor.state, LOUPIST, true, |tx| {
        let page = assets::ensure_page(
            tx,
            LOUPIST,
            &assets::AssetIdentity {
                symbol: "990308".into(),
                market: "KRX".into(),
                name: "Byeolmuri".into(),
            },
            None,
            1,
        )?;
        let entry = |role, text: &str, check| {
            journal::create(
                tx,
                LOUPIST,
                journal::JournalInput {
                    judgment: text.into(),
                    reasoning: String::new(),
                    entities: vec![],
                    check,
                    thesis: Some(assets::JournalThesis {
                        asset_id: page.asset_id.clone(),
                        role,
                    }),
                },
                2,
            )
        };
        entry(
            assets::ThesisRole::Reason,
            "The new packaging lines lift margins.",
            None,
        )?;
        let line = entry(
            assets::ThesisRole::Line,
            "If one customer passes 25% of revenue.",
            Some(JournalCheck {
                claim: "One customer above 25% of revenue".into(),
                metric: "Largest customer share".into(),
                direction: "above".into(),
                threshold: 25.0,
                unit: Some("%".into()),
                by_date: None,
                confidence: None,
            }),
        )?;
        Ok((page.asset_id, line.journal_id))
    })
    .unwrap()
}

async fn result(
    executor: &AppToolExecutor,
    origin: RunResultOrigin,
    tool: &str,
    payload: Value,
) -> String {
    let delivery = new_id("thesis-evidence");
    let reference = executor
        .capture
        .stage_run_result_with_delivery(
            &delivery,
            RunResultProducer {
                origin,
                runtime_id: "native-finance".into(),
                tool_name: tool.into(),
                provider: Some("opendart".into()),
            },
            &json!({"company": "990308"}),
            payload,
            vec![],
            None,
        )
        .await
        .unwrap();
    executor.capture.commit_delivery(&delivery).await;
    evidence::citation_url(&reference)
}

async fn update(
    executor: &AppToolExecutor,
    policy: &ToolPolicy,
    params: Value,
) -> Result<Value, String> {
    let delivery = new_id("thesis");
    let outcome = executor
        .stage_thesis_update(policy, params, &delivery)
        .await;
    match outcome {
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

fn filing() -> Value {
    json!({"data": {"filings": [{"filing_id": "20260318990308", "filed_at": "20260318", "title": "별무리반도체 주요 고객 매출 비중 27.4%"}]}})
}

#[tokio::test]
async fn thesis_updates_need_saving_today_and_dated_original_evidence() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let (asset, line) = thesis(&executor);
    let original = result(
        &executor,
        RunResultOrigin::AcquiredOriginal,
        "dart_filing_read",
        filing(),
    )
    .await;
    let search = result(
        &executor,
        RunResultOrigin::Discovery,
        "web_search",
        json!({"results": [{"title": "x", "published_at": "2026-03-18"}]}),
    )
    .await;
    let undated = result(
        &executor,
        RunResultOrigin::AcquiredOriginal,
        "dart_filing_read",
        json!({"data": {"text": "no dates here"}}),
    )
    .await;
    let down = |evidence: &[&String]| json!({"assetId": asset, "to": "weakening", "reason": "The 3월 filing shows a customer at 27.4% of revenue.", "evidence": evidence});

    let refused = update(
        &executor,
        &policy(true, Some("2026-03-31")),
        down(&[&original]),
    )
    .await
    .unwrap_err();
    assert!(refused.contains("thesis_as_of"), "{refused}");
    let refused = update(&executor, &policy(false, None), down(&[&original]))
        .await
        .unwrap_err();
    assert!(refused.contains("thesis_private"), "{refused}");
    let refused = update(&executor, &policy(true, None), down(&[&search]))
        .await
        .unwrap_err();
    assert!(refused.contains("thesis_evidence_original"), "{refused}");
    let refused = update(&executor, &policy(true, None), down(&[&undated]))
        .await
        .unwrap_err();
    assert!(refused.contains("thesis_evidence_undated"), "{refused}");
    let unknown = "#evidence-ev_00000000000000000000000000000000".to_owned();
    let refused = update(&executor, &policy(true, None), down(&[&unknown]))
        .await
        .unwrap_err();
    assert!(refused.contains("not a citation_url"), "{refused}");
    let jump = json!({"assetId": asset, "to": "challenged", "reason": "x", "evidence": [original]});
    let refused = update(&executor, &policy(true, None), jump)
        .await
        .unwrap_err();
    assert!(refused.contains("thesis_two_steps"), "{refused}");
    let missing = json!({"assetId": asset, "lineReading": {"journalId": line, "observed": 31.0}, "evidence": [original]});
    let refused = update(&executor, &policy(true, None), missing)
        .await
        .unwrap_err();
    assert!(refused.contains("thesis_line_not_found"), "{refused}");

    // A dated original with the observed number crosses the user's line.
    let crossing = json!({"assetId": asset, "lineReading": {"journalId": line, "observed": 27.4}, "evidence": [original, search]});
    let staged = update(&executor, &policy(true, None), crossing)
        .await
        .unwrap();
    assert_eq!(
        (
            staged["read"].as_str(),
            staged["lineCrossed"].as_bool(),
            staged["factDate"].as_str()
        ),
        (Some("Broken"), Some(true), Some("2026-03-18"))
    );
    let refused = update(&executor, &policy(true, None), down(&[&original]))
        .await
        .unwrap_err();
    assert!(refused.contains("thesis_twice"), "{refused}");
    let staged = executor.capture.staged_theses.lock().await.clone();
    assert_eq!(staged.len(), 1);
    assert_eq!(staged[0].evidence.len(), 2);

    // Committed with the turn: Broken, and the Thread's crossing alert exists.
    let committed = crate::commands::assets::with_assets(&executor.state, LOUPIST, true, |tx| {
        crate::commands::assets::commit_turn(tx, LOUPIST, "chat-a", "message-1", 10, &staged, &[])
    })
    .unwrap();
    assert!(committed.crossed && committed.unsaved.is_empty());
    let alerts = crate::commands::assets::with_assets(&executor.state, LOUPIST, false, |tx| {
        assets::line_alerts(tx, LOUPIST, 0)
    })
    .unwrap();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].line.observed, 27.4);
}

#[tokio::test]
async fn the_page_read_is_a_citable_memory_receipt_that_never_creates_a_page() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let absent = executor
        .execute(
            &policy(true, None),
            ToolMethod::AssetPageRead,
            json!({"symbol": "MORR", "market": "NASDAQ"}),
        )
        .await
        .unwrap();
    assert_eq!(absent["exists"], false);
    let (asset, _) = thesis(&executor);
    let read = executor
        .execute(
            &policy(true, None),
            ToolMethod::AssetPageRead,
            json!({"assetId": asset}),
        )
        .await
        .unwrap();
    assert_eq!(read["read"]["verdict"], "Holding up");
    assert_eq!(read["reasons"].as_array().unwrap().len(), 1);
    assert_eq!(read["lines"][0]["status"], "not_checked");
    assert!(read["citation_url"]
        .as_str()
        .unwrap()
        .starts_with("#evidence-"));
    // A Memory receipt cannot be the only evidence for a move.
    let citation = read["citation_url"].as_str().unwrap().to_owned();
    let refused = update(
        &executor,
        &policy(true, None),
        json!({"assetId": asset, "to": "weakening", "reason": "x", "evidence": [citation]}),
    )
    .await
    .unwrap_err();
    assert!(refused.contains("thesis_evidence"), "{refused}");
    let denied = executor
        .execute(
            &policy(false, None),
            ToolMethod::AssetPageRead,
            json!({"assetId": asset}),
        )
        .await
        .unwrap_err();
    assert!(matches!(denied, BrokerError::MethodDenied));
}

#[tokio::test]
async fn an_as_of_page_read_shows_only_what_was_dated_by_the_cutoff() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let (asset, _) = thesis(&executor);
    // A reason and a line the user wrote after the cutoff.
    let march = chrono::NaiveDate::from_ymd_opt(2026, 3, 5)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap()
        .timestamp_millis();
    crate::commands::assets::with_assets(&executor.state, LOUPIST, true, |tx| {
        for (role, text, check) in [
            (assets::ThesisRole::Reason, "Export orders doubled.", None),
            (
                assets::ThesisRole::Line,
                "If exports fall below 40% of sales.",
                Some(JournalCheck {
                    claim: "Exports below 40% of sales".into(),
                    metric: "Export share".into(),
                    direction: "below".into(),
                    threshold: 40.0,
                    unit: Some("%".into()),
                    by_date: None,
                    confidence: None,
                }),
            ),
        ] {
            journal::create(
                tx,
                LOUPIST,
                journal::JournalInput {
                    judgment: text.into(),
                    reasoning: String::new(),
                    entities: vec![],
                    check,
                    thesis: Some(assets::JournalThesis {
                        asset_id: asset.clone(),
                        role,
                    }),
                },
                march,
            )?;
        }
        Ok(())
    })
    .unwrap();
    let filings = json!({"data": {"filings": [
        {"filing_id": "20260318990308", "filed_at": "20260318", "title": "별무리반도체 포장라인 증설"},
        {"filing_id": "20260115990308", "filed_at": "20260115", "title": "분기보고서 (2025.12)"}
    ]}});
    crate::commands::assets::with_assets(&executor.state, LOUPIST, true, |tx| {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 20).unwrap();
        let read = assets::observe::ToolRead {
            tool: "dart_filing_search",
            provider: None,
            payload: &filings,
            dataset: None,
            retrieved_at: None,
            citation_url: Some("#evidence-ev_fixture".into()),
            today,
            session_day: today,
        };
        assets::record_observations(
            tx,
            LOUPIST,
            &asset,
            assets::observe::extract(&read, "990308"),
            assets::ObservationOrigin::Refresh,
            None,
            5,
        )
    })
    .unwrap();
    let read = |as_of: Option<&str>| {
        let executor = &executor;
        let asset = asset.clone();
        let policy = policy(true, as_of);
        async move {
            let mut read = executor
                .execute(
                    &policy,
                    ToolMethod::AssetPageRead,
                    json!({"assetId": asset}),
                )
                .await
                .unwrap();
            let rows = read["rows"]
                .as_str()
                .and_then(crate::custom_connections::taint::delimited_body)
                .expect("filing observation rows are delimited");
            read["rows"] = serde_json::from_str(rows).unwrap();
            read
        }
    };
    let today = read(None).await;
    assert_eq!(today["rows"].as_array().unwrap().len(), 2);
    assert_eq!(today["read"]["verdict"], "Holding up");
    let then = read(Some("2026-02-01")).await;
    let dates: Vec<_> = then["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["factDate"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(dates, ["2026-01-15"]);
    assert!(then["read"].is_null(), "today's read is not an as-of fact");
    assert_eq!(then["asOf"], "2026-02-01");
    let texts = |read: &Value, key: &str, field: &str| -> Vec<String> {
        read[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item[field].as_str().unwrap().to_owned())
            .collect()
    };
    assert!(texts(&today, "reasons", "text").contains(&"Export orders doubled.".to_owned()));
    assert_eq!(
        texts(&then, "reasons", "text"),
        ["The new packaging lines lift margins."]
    );
    assert_eq!(today["lines"].as_array().unwrap().len(), 2);
    assert_eq!(
        texts(&then, "lines", "claim"),
        ["One customer above 25% of revenue"]
    );
}

/// A second line on the fixture page, with no unit (a count).
fn count_line(executor: &AppToolExecutor, asset: &str) -> String {
    crate::commands::assets::with_assets(&executor.state, LOUPIST, true, |tx| {
        journal::create(
            tx,
            LOUPIST,
            journal::JournalInput {
                judgment: "Fewer than 10 packaging lines would change my mind.".into(),
                reasoning: String::new(),
                entities: vec![],
                check: Some(JournalCheck {
                    claim: "Packaging lines below 10".into(),
                    metric: "Packaging lines".into(),
                    direction: "below".into(),
                    threshold: 10.0,
                    unit: None,
                    by_date: None,
                    confidence: None,
                }),
                thesis: Some(assets::JournalThesis {
                    asset_id: asset.into(),
                    role: assets::ThesisRole::Line,
                }),
            },
            3,
        )
    })
    .unwrap()
    .journal_id
}

#[tokio::test]
async fn a_line_reading_needs_its_exact_number_in_an_original_in_the_line_unit() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let (asset, share_line) = thesis(&executor);
    let lines_line = count_line(&executor, &asset);
    let reading = |line: &str, observed: f64, evidence: &[&String]| json!({"assetId": asset, "lineReading": {"journalId": line, "observed": observed}, "evidence": evidence});
    let refused = |outcome: Result<Value, String>| outcome.unwrap_err();
    // Hyphens in dates and identifiers are not signs, and date parts are not readings.
    let dated = result(
        &executor,
        RunResultOrigin::AcquiredOriginal,
        "dart_filing_read",
        json!({"data": {"filed_at": "2026-03-18", "title": "Form 10-K for FY2026 Q3 (rcpNo 2026-0318-9)"}}),
    )
    .await;
    for observed in [-3.0, 3.0, -18.0, 18.0, 9.0] {
        let outcome = update(
            &executor,
            &policy(true, None),
            reading(&lines_line, observed, &[&dated]),
        )
        .await;
        assert!(
            refused(outcome).contains("thesis_line_not_found"),
            "{observed}"
        );
    }
    // A fraction is not a percent: no ×/÷100 match.
    let fraction = result(
        &executor,
        RunResultOrigin::AcquiredOriginal,
        "dart_financials",
        json!({"data": {"filed_at": "2026-03-18", "facts": [{"label": "largest customer share", "value": 0.274}]}}),
    )
    .await;
    let outcome = update(
        &executor,
        &policy(true, None),
        reading(&share_line, 27.4, &[&fraction]),
    )
    .await;
    assert!(refused(outcome).contains("thesis_line_not_found"));
    // A search hit can be cited, but the number must be printed in an original.
    let snippet = result(
        &executor,
        RunResultOrigin::Discovery,
        "web_search",
        json!({"results": [{"title": "Largest customer now 27.4% of revenue", "published_at": "2026-03-18"}]}),
    )
    .await;
    let outcome = update(
        &executor,
        &policy(true, None),
        reading(&share_line, 27.4, &[&fraction, &snippet]),
    )
    .await;
    assert!(refused(outcome).contains("thesis_line_not_found"));
    // Exact at the printed precision: 27.4% matches 27.4 (and 27.40), never 27.43.
    let original = result(
        &executor,
        RunResultOrigin::AcquiredOriginal,
        "dart_filing_read",
        filing(),
    )
    .await;
    let outcome = update(
        &executor,
        &policy(true, None),
        reading(&share_line, 27.43, &[&original]),
    )
    .await;
    assert!(refused(outcome).contains("thesis_line_not_found"));
    let staged = update(
        &executor,
        &policy(true, None),
        reading(&share_line, 27.40, &[&original]),
    )
    .await
    .unwrap();
    assert_eq!(staged["lineCrossed"], true);
}

#[tokio::test]
async fn parallel_thesis_updates_in_one_step_move_an_asset_once() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let (asset, _) = thesis(&executor);
    let original = result(
        &executor,
        RunResultOrigin::AcquiredOriginal,
        "dart_filing_read",
        filing(),
    )
    .await;
    let down = json!({"assetId": asset, "to": "weakening", "reason": "The March filing shows one customer at 27.4% of revenue.", "evidence": [original]});
    let save = policy(true, None);
    // Two calls in the same model step: neither delivery has committed yet.
    let (first, second) = tokio::join!(
        executor.stage_thesis_update(&save, down.clone(), "delivery-a"),
        executor.stage_thesis_update(&save, down.clone(), "delivery-b"),
    );
    let refusals: Vec<String> = [first, second]
        .into_iter()
        .filter_map(|outcome| outcome.err().map(|error| format!("{error:?}")))
        .collect();
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert!(refusals[0].contains("thesis_twice"), "{refusals:?}");
    executor.capture.commit_delivery("delivery-a").await;
    executor.capture.commit_delivery("delivery-b").await;
    let staged = executor.capture.staged_theses.lock().await.clone();
    assert_eq!(staged.len(), 1);
    // The commit also keeps only the first update per asset.
    let doubled = [staged[0].clone(), staged[0].clone()];
    let committed = crate::commands::assets::with_assets(&executor.state, LOUPIST, true, |tx| {
        crate::commands::assets::commit_turn(tx, LOUPIST, "chat-a", "message-1", 10, &doubled, &[])
    })
    .unwrap();
    assert_eq!(committed.unsaved.len(), 1);
    assert_eq!(committed.unsaved[0].reason, "thesis_twice");
    let history = crate::commands::assets::with_assets(&executor.state, LOUPIST, false, |tx| {
        assets::moves(tx, LOUPIST, &asset)
    })
    .unwrap();
    assert_eq!(history.len(), 1);
}

#[tokio::test]
async fn an_update_the_page_no_longer_fits_is_not_saved_and_the_receipt_says_so() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let (asset, _) = thesis(&executor);
    let original = result(
        &executor,
        RunResultOrigin::AcquiredOriginal,
        "dart_filing_read",
        filing(),
    )
    .await;
    let staged = update(
        &executor,
        &policy(true, None),
        json!({"assetId": asset, "to": "weakening", "reason": "The March filing shows one customer at 27.4% of revenue.", "evidence": [original]}),
    )
    .await
    .unwrap();
    // The tool result promises nothing the commit might not keep.
    assert!(!staged["note"]
        .as_str()
        .unwrap()
        .contains("Saved with this reply"));
    // Before the reply finishes, the user archives their only reason.
    crate::commands::assets::with_assets(&executor.state, LOUPIST, true, |tx| {
        for entry in journal::list(tx, LOUPIST)? {
            if entry.check.is_none() {
                journal::archive(tx, LOUPIST, &entry.journal_id, true, 5)?;
            }
        }
        Ok(())
    })
    .unwrap();
    let theses = executor.capture.staged_theses.lock().await.clone();
    let committed = crate::commands::assets::with_assets(&executor.state, LOUPIST, true, |tx| {
        crate::commands::assets::commit_turn(tx, LOUPIST, "chat-a", "message-1", 10, &theses, &[])
    })
    .unwrap();
    assert_eq!(committed.unsaved.len(), 1);
    assert_eq!(committed.unsaved[0].reason, "thesis_none");
    // The run log records it, and the message the renderer gets names the asset.
    let mut trace = crate::domain::ChatToolTrace::default();
    for note in committed.trace_notes(42) {
        trace.push(note);
    }
    assert_eq!(trace.entries[0].name, "thesis_update");
    let unsaved = trace.unsaved_theses();
    assert_eq!(
        (unsaved[0].asset_id.as_str(), unsaved[0].name.as_str()),
        (asset.as_str(), "Byeolmuri")
    );
    let mut message = crate::test_fixtures::chat_message(
        "assistant-unsaved",
        crate::domain::ChatRole::Assistant,
        "Moved to Weakening.",
    );
    message.tool_trace = Some(trace);
    let mut thread = chat("chat-unsaved", LOUPIST, 1);
    thread.messages = vec![message];
    let dto = crate::commands::types::chat_dto(&thread).unwrap();
    assert_eq!(
        serde_json::to_value(&dto.messages[0]).unwrap()["unsaved_theses"][0]["name"],
        "Byeolmuri"
    );
    assert!(
        crate::commands::assets::with_assets(&executor.state, LOUPIST, false, |tx| {
            assets::moves(tx, LOUPIST, &asset)
        })
        .unwrap()
        .is_empty()
    );
}
