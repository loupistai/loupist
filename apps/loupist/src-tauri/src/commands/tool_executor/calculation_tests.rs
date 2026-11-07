use super::*;
use crate::commands::tests::{chat, profile, seed_profile};

fn policy() -> ToolPolicy {
    ToolPolicy {
        loupist_id: "loupist-a".into(),
        session_id: "chat-a".into(),
        use_memory: false,
        propose_memory_updates: false,
        memory_proposal_budget: 0,
        as_of: None,
    }
}

fn executor(temporary: &tempfile::TempDir) -> AppToolExecutor {
    let state = AppState::for_persistent_test(temporary.path().join("app"));
    for id in ["loupist-a", "loupist-b"] {
        let workspace = temporary.path().join(id);
        std::fs::create_dir_all(&workspace).unwrap();
        seed_profile(state.store.as_ref(), &profile(id, &workspace, 1));
    }
    state
        .store
        .create_chat(&chat("chat-a", "loupist-a", 1))
        .unwrap();
    AppToolExecutor {
        state,
        capture: Arc::new(ToolCapture::for_chat("assistant-calculation-test".into())),
        loupist_id: "loupist-a".into(),
        capability_ids: BTreeSet::new(),
    }
}

fn free_cash_flow(reference: &crate::datasets::DatasetRef) -> Value {
    let cell = |pointer: &str| json!({"dataset_id": reference.dataset_id, "revision": reference.revision, "pointer": pointer});
    json!({"operation": "free_cash_flow", "unit": "KRW millions", "inputs": {
        "operating_cash_flow": cell("/rows/0/amount"), "capex": cell("/rows/1/amount")}})
}

#[tokio::test]
async fn calculation_over_a_pinned_dataset_is_a_citable_derived_result_with_lineage() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let payload = json!({"rows": [
        {"concept": "operating_cash_flow", "amount": "1240"},
        {"concept": "capex", "amount": "-310"}
    ]});
    let reference = executor
        .state
        .store
        .dataset_put("loupist-a", "statement", &payload, json!({}))
        .unwrap();
    let mut original = payload;
    original["dataset_ref"] = json!(reference);
    let upstream = executor
        .capture
        .stage_run_result_with_delivery(
            "statement",
            RunResultProducer {
                origin: RunResultOrigin::AcquiredOriginal,
                runtime_id: "native-finance".into(),
                tool_name: "dart_financials".into(),
                provider: Some("issuer".into()),
            },
            &json!({"corp_code": "00000001"}),
            original,
            vec![],
            None,
        )
        .await
        .unwrap();
    executor.capture.commit_delivery("statement").await;

    let output = executor
        .execute(&policy(), ToolMethod::Calculate, free_cash_flow(&reference))
        .await
        .unwrap();
    assert_eq!(output["title"], "Free cash flow");
    assert_eq!(output["values"][0]["value"], json!(930.0));
    assert_eq!(
        output["formulas"][0],
        "FCF = OCF − capex = 1,240 − 310 = 930"
    );
    assert_eq!(
        output["inputs"][0]["provenance"]["digest"],
        json!(reference.digest)
    );
    assert!(output.get(RESOLVED_UPSTREAM_FIELD).is_none());
    let captured = executor
        .capture
        .run_result(output["result_ref"].as_str().unwrap())
        .await
        .unwrap();
    assert_eq!(captured.producer.origin, RunResultOrigin::Derived);
    assert_eq!(captured.producer.tool_name, "calculate");
    // The reader shows it as a titled work result, not a retrieved source.
    let evidence = evidence::project(&captured);
    assert_eq!(evidence.title, "Free cash flow");
    assert_eq!(evidence.kind, crate::domain::ChatEvidenceKind::Derived);
    assert_eq!(captured.upstream_result_refs, [upstream]);
}

#[tokio::test]
async fn calculation_refuses_foreign_and_corrupted_revisions_and_reports_bad_arguments_by_field() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let payload = json!({"rows": [{"amount": "1240"}, {"amount": "310"}]});
    let foreign = executor
        .state
        .store
        .dataset_put("loupist-b", "foreign", &payload, json!({}))
        .unwrap();
    let corrupt = executor
        .state
        .store
        .dataset_put("loupist-a", "corrupt", &payload, json!({}))
        .unwrap();
    executor
        .state
        .store
        .lock()
        .unwrap()
        .execute(
            "UPDATE datasets SET digest=?1 WHERE loupist_id='loupist-a' AND id=?2",
            rusqlite::params!["0".repeat(64), corrupt.dataset_id],
        )
        .unwrap();
    for refused in [&foreign, &corrupt] {
        let error = executor
            .execute(&policy(), ToolMethod::Calculate, free_cash_flow(refused))
            .await
            .unwrap_err();
        assert!(matches!(error, BrokerError::Execution(_)), "{error}");
    }

    let error = executor
        .execute(
            &policy(),
            ToolMethod::Calculate,
            json!({"operation": "margin", "inputs": {"profit": 5, "revenue": 0}}),
        )
        .await
        .unwrap_err();
    let BrokerError::InvalidArguments { field, expected } = error else {
        panic!("expected a field error");
    };
    assert_eq!(field, "inputs.revenue");
    assert!(expected.contains("positive"));
    assert!(matches!(
        executor
            .execute(
                &policy(),
                ToolMethod::Calculate,
                json!({"operation": "ratio", "loupist_id": "loupist-b"}),
            )
            .await,
        Err(BrokerError::MethodDenied)
    ));
}

#[tokio::test]
async fn an_as_of_turn_uses_only_revisions_fetched_under_a_cutoff_no_later_than_its_own() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let payload = json!({"rows": [
        {"date": "2026-03-03", "amount": "1240"},
        {"date": "2026-03-04", "amount": "-310"}
    ]});
    let put = |key: &str, metadata: Value| {
        executor
            .state
            .store
            .dataset_put("loupist-a", key, &payload, metadata)
            .unwrap()
    };
    // Fetched earlier in the Chat without a cutoff, or under a later one.
    let unbounded = put(
        "unbounded",
        json!({"producer": "KrxDailyQuote", "as_of": null}),
    );
    let later = put(
        "later",
        json!({"producer": "KrxDailyQuote", "as_of": "2026-03-10"}),
    );
    // Fetched under this cutoff (native) or an earlier one (bundled MCP request).
    let native = put(
        "native",
        json!({"producer": "KrxDailyQuote", "as_of": "2026-03-03"}),
    );
    let mcp = put(
        "mcp",
        json!({"producer": "mcp", "request": {"tool_name": "equity_price_historical", "as_of": "2026-02-27"}}),
    );
    let as_of = ToolPolicy {
        as_of: Some("2026-03-03".into()),
        ..policy()
    };
    let calls = |reference: &crate::datasets::DatasetRef| {
        [
            (ToolMethod::Calculate, free_cash_flow(reference)),
            (
                ToolMethod::DatasetQuery,
                json!({"dataset_id": reference.dataset_id, "revision": reference.revision, "pointer": "/rows"}),
            ),
            (
                ToolMethod::DatasetDescribe,
                json!({"dataset_id": reference.dataset_id, "revision": reference.revision}),
            ),
            (
                ToolMethod::DatasetExport,
                json!({"dataset_id": reference.dataset_id, "revision": reference.revision,
                    "path": format!("exports/{}.json", reference.dataset_id), "format": "json"}),
            ),
        ]
    };
    for reference in [&unbounded, &later] {
        for (method, params) in calls(reference) {
            let error = executor.execute(&as_of, method, params).await.unwrap_err();
            let BrokerError::WithNext { source, next } = &error else {
                panic!("{method:?}: expected a typed refusal, got {error}");
            };
            assert!(
                source.to_string().contains("as_of_unsupported"),
                "{method:?}: {source}"
            );
            assert!(next.contains("Fetch the data again"), "{next}");
        }
    }
    assert!(!temporary
        .path()
        .join("loupist-a/exports")
        .join(format!("{}.json", unbounded.dataset_id))
        .exists());
    for reference in [&native, &mcp] {
        for (method, params) in calls(reference) {
            executor
                .execute(&as_of, method, params)
                .await
                .unwrap_or_else(|error| panic!("{method:?}: {error}"));
        }
    }
    // Without a cutoff every owned revision stays usable.
    for (method, params) in calls(&unbounded) {
        if method != ToolMethod::DatasetExport {
            executor.execute(&policy(), method, params).await.unwrap();
        }
    }
}

#[tokio::test]
async fn cited_inputs_are_checked_against_this_turns_evidence_and_join_the_lineage() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let dart = executor
        .capture
        .stage_run_result_with_delivery(
            "dart",
            RunResultProducer {
                origin: RunResultOrigin::AcquiredOriginal,
                runtime_id: "native-finance".into(),
                tool_name: "dart_financials".into(),
                provider: Some("OpenDART".into()),
            },
            &json!({"corp_code": "00990221"}),
            json!({"facts": [
                {"canonical_concept": "operating_income", "value": "38100000000", "unit": "KRW", "scale": "1"},
                {"canonical_concept": "revenue", "value": "452312000000", "unit": "KRW", "scale": "1"}
            ]}),
            vec![],
            None,
        )
        .await
        .unwrap();
    executor.capture.commit_delivery("dart").await;
    let url = evidence::citation_url(&dart);
    let margin = |sources: Value| {
        json!({"operation": "margin", "unit": "KRW bn",
            "inputs": {"profit": "38.1", "revenue": "452.3"}, "input_sources": sources})
    };
    let output = executor
        .execute(
            &policy(),
            ToolMethod::Calculate,
            margin(json!({"profit": url, "revenue": url})),
        )
        .await
        .unwrap();
    let provenance = |index: usize| output["inputs"][index]["provenance"].clone();
    assert_eq!(provenance(0)["kind"], "cited");
    assert_eq!(
        provenance(0)["evidence_id"],
        json!(evidence::evidence_id(&dart))
    );
    // ₩38.1bn equals the raw 38,100,000,000 only after a scale conversion.
    assert_eq!(provenance(0)["status"], "rounded");
    assert_eq!(provenance(1)["status"], "rounded");
    let captured = executor
        .capture
        .run_result(output["result_ref"].as_str().unwrap())
        .await
        .unwrap();
    assert_eq!(captured.upstream_result_refs, std::slice::from_ref(&dart));

    // A number the source does not print is recorded, not trusted.
    let output = executor
        .execute(
            &policy(),
            ToolMethod::Calculate,
            json!({"operation": "margin", "unit": "KRW bn",
                "inputs": {"profit": "40", "revenue": "452.3"}, "input_sources": {"profit": url}}),
        )
        .await
        .unwrap();
    assert_eq!(output["inputs"][0]["provenance"]["status"], "not_found");
    assert_eq!(output["inputs"][1]["provenance"]["kind"], "provided");

    // Only this turn's citation_url values, for inputs the operation takes. A
    // guessed URL is refused with the ones this turn returned.
    for guess in ["#evidence-ev_unknown", "#evidence-"] {
        let error = executor
            .execute(
                &policy(),
                ToolMethod::Calculate,
                margin(json!({"revenue": guess})),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("input_sources.revenue: not a citation_url from this turn; use the citation_url of the result that holds this value, or leave it unsourced. This turn returned: "),
            "{error}"
        );
        assert!(error.contains(&format!("{url} (")), "{error}");
    }
    let error = executor
        .execute(
            &policy(),
            ToolMethod::Calculate,
            margin(json!({"price": url})),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "input_sources.price: not an input of this calculation"
    );
}

#[tokio::test]
async fn cited_inputs_are_read_in_their_own_unit_not_the_amount_unit() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let quote = executor
        .capture
        .stage_run_result_with_delivery(
            "quote",
            RunResultProducer {
                origin: RunResultOrigin::AcquiredOriginal,
                runtime_id: "native-finance".into(),
                tool_name: "kis_read".into(),
                provider: Some("KIS".into()),
            },
            &json!({"symbol": "990221"}),
            json!({"data": {"output": {"stck_prpr": "41200", "prdy_ctrt": "5.20",
                "lstn_stcn": "12345678", "mksc_shrn_iscd": "990221"}}}),
            vec![],
            None,
        )
        .await
        .unwrap();
    executor.capture.commit_delivery("quote").await;
    let url = evidence::citation_url(&quote);
    // Amounts are in 억 won; the price is per share in won, shares a count.
    let output = executor
        .execute(
            &policy(),
            ToolMethod::Calculate,
            json!({"operation": "reverse_dcf", "unit": "KRW 억", "wacc_pct": [8], "terminal_growth_pct": [2],
                "inputs": {"cash_flow": 300, "price": 41200, "shares": 12345678},
                "input_sources": {"price": url, "shares": url}}),
        )
        .await
        .unwrap();
    let status = |output: &Value, name: &str| {
        output["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|input| input["name"] == name)
            .unwrap()["provenance"]["status"]
            .clone()
    };
    assert_eq!(status(&output, "price"), "matched");
    assert_eq!(status(&output, "shares"), "matched");
    // A growth rate is a percent: KIS prints prdy_ctrt 5.20 as 5.20 %.
    let output = executor
        .execute(
            &policy(),
            ToolMethod::Calculate,
            json!({"operation": "dcf_grid", "unit": "KRW 억", "forecast_years": 1,
                "wacc_pct": [8], "terminal_growth_pct": [2],
                "inputs": {"cash_flow": 300, "growth_pct": 5.2}, "input_sources": {"growth_pct": url}}),
        )
        .await
        .unwrap();
    assert_eq!(status(&output, "growth_pct"), "matched");
    // The ticker is a code, never a number a calculation can cite.
    let output = executor
        .execute(
            &policy(),
            ToolMethod::Calculate,
            json!({"operation": "reverse_dcf", "wacc_pct": [8], "terminal_growth_pct": [2],
                "inputs": {"cash_flow": 300, "price": 41200, "shares": 990221},
                "input_sources": {"shares": url}}),
        )
        .await
        .unwrap();
    assert_eq!(status(&output, "shares"), "not_found");
}
