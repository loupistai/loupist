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
        display_currency: None,
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
        Err(BrokerError::InvalidArguments { ref field, .. }) if field == "loupist_id"
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
    let refusal = error.to_string();
    assert!(refusal.starts_with("input_sources.price: One of this call's own input names ("));
    assert!(refusal.contains("profit") && refusal.contains("revenue"));
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

/// A price divided by an assumed multiple is a price: the cited close matches
/// in the per-share unit and a cited multiple reads as a pure number.
#[tokio::test]
async fn a_ratio_per_share_reads_its_numerator_in_unit_and_its_denominator_as_a_number() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let close = stage_original(
        &executor,
        "close",
        "equity_price_historical",
        json!({"structuredContent": {"results": [{"date": "2026-11-20", "close": 345.82000732421875}]}}),
    )
    .await;
    let multiple = stage_original(
        &executor,
        "multiple",
        "research_note",
        json!({"data": {"text": "The scenario applies a 100x earnings multiple."}}),
    )
    .await;
    let output = executor
        .execute(
            &policy(),
            ToolMethod::Calculate,
            json!({"operation": "ratio", "unit": "USD per share",
                "inputs": {"numerator": 345.82000732421875, "denominator": 100},
                "input_sources": {"numerator": close, "denominator": multiple}}),
        )
        .await
        .unwrap();
    assert_eq!(input_status(&output, "numerator"), "matched");
    assert_eq!(input_status(&output, "denominator"), "matched");
    assert_eq!(output["values"][0]["unit"], "USD per share");
    assert_eq!(
        output["formulas"][0],
        "numerator / denominator = 345.82 / 100 = 3.4582 USD per share"
    );
}

async fn stage_original(
    executor: &AppToolExecutor,
    delivery: &str,
    tool: &str,
    payload: Value,
) -> String {
    let reference = executor
        .capture
        .stage_run_result_with_delivery(
            delivery,
            RunResultProducer {
                origin: RunResultOrigin::AcquiredOriginal,
                runtime_id: "native-finance".into(),
                tool_name: tool.into(),
                provider: Some("OpenDART".into()),
            },
            &json!({"delivery": delivery}),
            payload,
            vec![],
            None,
        )
        .await
        .unwrap();
    executor.capture.commit_delivery(delivery).await;
    evidence::citation_url(&reference)
}

fn input_status(output: &Value, name: &str) -> Value {
    output["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|input| input["name"] == name)
        .unwrap()["provenance"]["status"]
        .clone()
}

/// rc1 RP-002: a margin over a quarterly report typed 52 and 675 with no
/// `unit`; the report prints `52억원` and `675억원`, and a unitless input never
/// matched an amount printed with a scale.
#[tokio::test]
async fn rc1_rp002_unitless_inputs_match_the_numbers_their_filing_prints() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let report = stage_original(&executor, "report", "dart_filing_read", json!({
        "dataset_ref": {"dataset_id": "dataset-17f19041a98041d4bfaee6eb43260a3b", "revision": 1},
        "data": {"document": {"text": "분기보고서 (2026.09) 가상의 한빛화학 제18기 3분기(2026.07.01~2026.09.30) 분기보고서입니다. 제출일 2026-11-12. II. 사업의 내용 - 영업실적: 당분기 연결 매출액은 675억원(전년동기 723억원), 영업이익은 52억원(전년동기 80억원)입니다. 여수공장은 2026년 8월 24일부터 정기보수에 들어가 당분기 중 38일간 가동을 멈추었고, 이로 인해 판매량이 줄었습니다."}}
    })).await;
    let margin = |profit: Value, revenue: Value, unit: Option<&str>| {
        let mut request = json!({"operation": "margin", "inputs": {"profit": profit, "revenue": revenue},
            "input_sources": {"profit": report, "revenue": report}});
        if let Some(unit) = unit {
            request["unit"] = json!(unit);
        }
        request
    };
    for (profit, revenue, unit, expected) in [
        (json!(52), json!(675), None, ["matched", "matched"]),
        (json!(80), json!(723), None, ["matched", "matched"]),
        (
            json!(52),
            json!(675),
            Some("KRW 억원"),
            ["matched", "matched"],
        ),
        (
            json!(5.2),
            json!(67.5),
            Some("KRW bn"),
            ["rounded", "rounded"],
        ),
        // A different number, or the same digits in another scale, is not printed.
        (json!(53), json!(675), None, ["not_found", "matched"]),
        (json!(52), json!(676), None, ["matched", "not_found"]),
        (
            json!(52),
            json!(675),
            Some("KRW"),
            ["not_found", "not_found"],
        ),
        (
            json!(52),
            json!(675),
            Some("KRW millions"),
            ["not_found", "not_found"],
        ),
    ] {
        let output = executor
            .execute(
                &policy(),
                ToolMethod::Calculate,
                margin(profit.clone(), revenue.clone(), unit),
            )
            .await
            .unwrap();
        assert_eq!(
            [
                input_status(&output, "profit"),
                input_status(&output, "revenue")
            ],
            expected,
            "{profit} / {revenue} in {unit:?}"
        );
    }
}

/// rc1 T-047: the dividend total is printed under the label `배당금총액(원)`;
/// earnings come from a statement row in KRW. Both runs cited them, one with
/// `unit` "KRW" and one without, and each lost one input to `not_found`.
#[tokio::test]
async fn rc1_t047_cited_payout_inputs_match_row_labels_and_rows() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let notice = stage_original(&executor, "notice", "dart_filing_read", json!({
        "dataset_ref": {"dataset_id": "dataset-9ef0049d758f4b248d9fdd6c22912160", "revision": 1},
        "data": {"document": {"text": "현금ㆍ현물배당결정 1. 배당구분 결산배당 2. 배당종류 현금배당 3. 1주당 배당금(원) 보통주식 300 4. 시가배당율(%) 보통주식 1.4 5. 배당금총액(원) 6,000,000,000 6. 배당기준일 2025-12-31"}}
    })).await;
    let statements = stage_original(&executor, "statements", "dart_financials", json!({
        "dataset_ref": {"dataset_id": "dataset-statements", "revision": 1},
        "data": {"facts": [
            {"label": "지배기업 소유주지분 당기순이익", "value": "21000000000", "unit": "KRW", "scale": "1"},
            {"label": "영업활동현금흐름", "value": "52600000000", "unit": "KRW", "scale": "1"}
        ]}
    })).await;
    for unit in [Some("KRW"), None] {
        let mut request = json!({"operation": "payout_ratio",
            "inputs": {"dividends": 6000000000_u64, "earnings": 21000000000_u64, "free_cash_flow": 33700000000_u64},
            "input_sources": {"dividends": notice, "earnings": statements}});
        if let Some(unit) = unit {
            request["unit"] = json!(unit);
        }
        let output = executor
            .execute(&policy(), ToolMethod::Calculate, request.clone())
            .await
            .unwrap();
        assert_eq!(input_status(&output, "dividends"), "matched", "{unit:?}");
        assert_eq!(input_status(&output, "earnings"), "matched", "{unit:?}");
        // A wrong dividend total is still not found.
        request["inputs"]["dividends"] = json!(6500000000_u64);
        let output = executor
            .execute(&policy(), ToolMethod::Calculate, request)
            .await
            .unwrap();
        assert_eq!(input_status(&output, "dividends"), "not_found", "{unit:?}");
    }
}

/// rc2 RP-002: the article states the fall in the word after the number
/// (`약 9% 줄었고`). A negative input reads that sign; a positive one still
/// matches the magnitude, and an input written with `+` does not.
#[tokio::test]
async fn a_negative_cited_input_matches_a_fall_its_source_states_in_words() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let article = stage_original(&executor, "article", "web_fetch", json!({
        "content": "회사는 같은 날 열린 실적 설명회에서 여수공장 정기보수로 3분기 판매량이 2분기보다 약 9% 줄었고, 정기보수와 관련한 일회성 비용 약 14억원이 영업이익에 반영됐다고 설명했다."
    })).await;
    for (growth, expected) in [
        (json!(-9), "matched"),
        (json!(9), "matched"),
        (json!("+9"), "not_found"),
        (json!(-8), "not_found"),
    ] {
        let output = executor
            .execute(
                &policy(),
                ToolMethod::Calculate,
                json!({"operation": "dcf_grid", "unit": "KRW billions",
                    "wacc_pct": [7, 8], "terminal_growth_pct": [1, 2],
                    "inputs": {"cash_flow": 100, "growth_pct": growth},
                    "input_sources": {"growth_pct": article}}),
            )
            .await
            .unwrap();
        assert_eq!(input_status(&output, "growth_pct"), expected, "{growth}");
    }
}

async fn stage_conversion_source(
    executor: &AppToolExecutor,
    method: ToolMethod,
    delivery: &str,
    request: Value,
    payload: Value,
) -> String {
    let producer = run_result_producer(method, &request, &payload).unwrap();
    let reference = executor
        .capture
        .stage_run_result_with_delivery(delivery, producer, &request, payload, vec![], None)
        .await
        .unwrap();
    executor.capture.commit_delivery(delivery).await;
    evidence::citation_url(&reference)
}

fn fx_table() -> crate::finance_data::fx::Table {
    crate::finance_data::fx::Table::parse(
        &json!({"date":"2026-11-16","USD":1.1014,"KRW":1470.23}),
        chrono::Utc::now(),
    )
    .unwrap()
}

async fn stage_conversion_rate(executor: &AppToolExecutor, from: &str, to: &str) -> String {
    stage_conversion_source(
        executor,
        ToolMethod::FxRate,
        &format!("{from}-{to}"),
        json!({"from":from,"to":to}),
        fx_table().pair(from, to).unwrap(),
    )
    .await
}

fn conversion(rate: &str) -> Value {
    json!({"operation":"convert","unit":"USD billions","to":"KRW",
        "inputs":{"amount":3930,"rate":rate}})
}

async fn run_conversion(executor: &AppToolExecutor, request: Value) -> Result<Value, BrokerError> {
    executor
        .execute(&policy(), ToolMethod::Calculate, request)
        .await
}

#[tokio::test]
async fn conversion_records_the_cited_rate_and_declared_amount_currency() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let rate = stage_conversion_rate(&executor, "USD", "KRW").await;
    let amount = stage_conversion_source(
        &executor,
        ToolMethod::KrxDailyQuote,
        "snapshot",
        json!({"symbol":"EXAMPLE"}),
        json!({"valuation":{"market_cap":3_930_000_000_000_u64}}),
    )
    .await;
    let mut request = conversion("1334.87");
    request["input_sources"] = json!({"amount":amount,"rate":rate});
    let result = run_conversion(&executor, request).await.unwrap();
    let pair = fx_table().pair("USD", "KRW").unwrap();
    assert_eq!(pair["to_per_from"], 1334.87);
    assert_eq!(result["inputs"][0]["currency_basis"], "declared");
    assert_eq!(result["inputs"][1]["provenance"]["kind"], "cited");
    assert_eq!(result["inputs"][1]["provenance"]["status"], "matched");
    assert_eq!(result["inputs"][1]["unit"]["kind"], "number");
    assert_eq!(result["inputs"][1]["unit"]["text"], "KRW per USD");
    assert_eq!(
        result["conversion"],
        json!({"from":"USD","to":"KRW","rate_date":pair["date"],
            "rate_source":pair["source"],"revision":pair["revision"]}),
    );
    assert_eq!(result["formulas"][0],
        "Amount × Rate = 3,930 USD billions × 1,334.87 KRW per USD (ECB, 2026-11-16) = 5,246,039.1 KRW billions");
    let retained = executor
        .capture
        .run_result(result["result_ref"].as_str().unwrap())
        .await
        .unwrap();
    let rate_refs = retained
        .upstream_result_refs
        .iter()
        .filter(|reference| evidence::citation_url(reference) == rate);
    assert_eq!(rate_refs.count(), 1);

    let assumed = run_conversion(&executor, conversion("1334.87"))
        .await
        .unwrap();
    assert_eq!(
        assumed["inputs"][1]["provenance"],
        json!({"kind":"provided"})
    );
    assert!(assumed["formulas"][0]
        .as_str()
        .unwrap()
        .contains("(assumed rate)"));
}

#[tokio::test]
async fn conversion_verifies_the_rate_producer_direction_and_pointer() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let rate = stage_conversion_rate(&executor, "USD", "KRW").await;
    let other = stage_conversion_rate(&executor, "EUR", "KRW").await;
    let reverse = stage_conversion_rate(&executor, "KRW", "USD").await;
    let forged = stage_conversion_source(
        &executor,
        ToolMethod::KrxDailyQuote,
        "not-fx",
        json!({"symbol":"EXAMPLE"}),
        fx_table().pair("USD", "KRW").unwrap(),
    )
    .await;
    for (typed, citation, expected_field) in [
        ("1470.23", &rate, "inputs.rate"),
        ("1334.8700001", &rate, "inputs.rate"),
        ("1.334876e3", &rate, "inputs.rate"),
        ("1334.9", &rate, "inputs.rate"),
        ("1.3349e3", &rate, "inputs.rate"),
        ("1470.23", &other, "input_sources.rate"),
        ("0.000749136", &reverse, "input_sources.rate"),
        ("1334.87", &forged, "input_sources.rate"),
    ] {
        let mut request = conversion(typed);
        request["input_sources"] = json!({"rate":citation});
        let error = run_conversion(&executor, request).await.unwrap_err();
        assert!(
            matches!(&error, BrokerError::InvalidArguments { field, .. } if field == expected_field),
            "{typed}: {error}",
        );
        let detail = if citation == &rate {
            "the cited fx_rate result states 1334.87 for to_per_from"
        } else if citation == &other {
            "the cited fx_rate is from EUR to KRW. This conversion needs an fx_rate result from USD to KRW"
        } else if citation == &reverse {
            "the cited fx_rate is from KRW to USD. This conversion needs an fx_rate result from USD to KRW"
        } else {
            "expected the citation_url of an fx_rate result from USD to KRW in this turn"
        };
        assert_eq!(error.to_string(), format!("{expected_field}: {detail}"));
    }
}

#[tokio::test]
async fn conversion_checks_the_currency_of_the_matched_amount_cell() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    for (index, row) in [
        json!({"market_cap":3_930_000_000_000_u64,"market_cap_currency":"EUR","currency":"USD"}),
        json!({"market_cap":3_930_000_000_000_u64,"currency":"EUR"}),
        json!({"market_cap":3_930_000_000_000_u64,"reported_currency":"EUR"}),
        json!({"market_cap":3_930_000_000_000_u64,"currency_code":"EUR"}),
    ]
    .into_iter()
    .enumerate()
    {
        let amount = stage_conversion_source(
            &executor,
            ToolMethod::KrxDailyQuote,
            &format!("snapshot-{index}"),
            json!({"symbol":format!("EXAMPLE{index}")}),
            json!({"valuation":row}),
        )
        .await;
        let mut request = conversion("1334.87");
        request["input_sources"] = json!({"amount":amount});
        let error = run_conversion(&executor, request).await.unwrap_err();
        assert!(
            matches!(&error, BrokerError::InvalidArguments { field, .. } if field == "input_sources.amount")
        );
        assert_eq!(
            error.to_string(),
            "input_sources.amount: the source states this value in EUR; unit says USD"
        );
    }

    let amount = stage_conversion_source(
        &executor,
        ToolMethod::KrxDailyQuote,
        "reported-in-usd",
        json!({"symbol":"EXAMPLE"}),
        json!({"valuation":{"market_cap":3_930_000_000_000_u64,
            "market_cap_currency":"USD","currency":"EUR"}}),
    )
    .await;
    let mut request = conversion("1334.87");
    request["input_sources"] = json!({"amount":amount});
    let result = run_conversion(&executor, request).await.unwrap();
    assert_eq!(result["inputs"][0]["provenance"]["status"], "rounded");
    assert!(result["inputs"][0].get("currency_basis").is_none());
}

#[tokio::test]
async fn conversion_currency_basis_is_omitted_for_an_unmatched_amount() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let source = stage_conversion_source(
        &executor,
        ToolMethod::KrxDailyQuote,
        "unmatched-amount",
        json!({"symbol":"EXAMPLE"}),
        json!({"valuation":{"market_cap":100,"market_cap_currency":"EUR"}}),
    )
    .await;
    let mut request = conversion("1334.87");
    request["input_sources"] = json!({"amount":source});
    let result = run_conversion(&executor, request).await.unwrap();
    let amount = &result["inputs"][0];
    assert_eq!(amount["provenance"]["status"], "not_found");
    assert_eq!(amount.get("currency_basis"), None);
}

#[tokio::test]
async fn conversion_currency_basis_is_declared_for_a_matched_passage() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let url = "https://example.test/market-cap";
    let source = stage_conversion_source(
        &executor,
        ToolMethod::WebFetch,
        "amount-passage",
        json!({"url":url}),
        json!({"url":url,"content":"The report puts market capitalization at USD 3,930 billion."}),
    )
    .await;
    let mut request = conversion("1334.87");
    request["input_sources"] = json!({"amount":source});
    let result = run_conversion(&executor, request).await.unwrap();
    let amount = &result["inputs"][0];
    assert_eq!(amount["provenance"]["status"], "matched");
    assert_eq!(amount["currency_basis"], "declared");
}

#[tokio::test]
async fn review_cited_conversion_rates_require_published_precision() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let small = stage_conversion_rate(&executor, "KRW", "USD").await;
    let near_one = stage_conversion_rate(&executor, "EUR", "USD").await;
    let large = stage_conversion_rate(&executor, "USD", "KRW").await;
    let mut refusals = Vec::new();
    for (from, to, typed, citation) in [
        ("KRW", "USD", "0", &small),
        ("KRW", "USD", "0.001", &small),
        ("KRW", "USD", "0.00075", &small),
        ("EUR", "USD", "1", &near_one),
        ("USD", "KRW", "1.3e3", &large),
    ] {
        let mut request = conversion(typed);
        request["unit"] = json!(from);
        request["to"] = json!(to);
        request["input_sources"] = json!({"rate": citation});
        refusals.push(
            run_conversion(&executor, request)
                .await
                .err()
                .map(|error| error.to_string()),
        );
    }
    let small_rate = fx_table().pair("KRW", "USD").unwrap()["to_per_from"]
        .as_f64()
        .unwrap();
    let small_refusal =
        format!("inputs.rate: the cited fx_rate result states {small_rate} for to_per_from");
    assert_eq!(
        refusals,
        [
            Some("inputs.rate: a positive number".to_owned()),
            Some(small_refusal.clone()),
            Some(small_refusal),
            Some("inputs.rate: the cited fx_rate result states 1.1014 for to_per_from".to_owned()),
            Some("inputs.rate: the cited fx_rate result states 1334.87 for to_per_from".to_owned()),
        ]
    );

    for (from, to, typed, citation) in [
        ("KRW", "USD", format!("{small_rate:.10}"), &small),
        ("KRW", "USD", format!("{small_rate:.8e}"), &small),
        ("EUR", "USD", "1.101400".into(), &near_one),
        ("EUR", "USD", "1.101400e0".into(), &near_one),
        ("USD", "KRW", "1334.8700".into(), &large),
        ("USD", "KRW", "1.3348700e3".into(), &large),
    ] {
        let mut request = conversion(&typed);
        request["unit"] = json!(from);
        request["to"] = json!(to);
        request["input_sources"] = json!({"rate": citation});
        let result = run_conversion(&executor, request).await.unwrap();
        assert_eq!(
            result["inputs"][1]["provenance"]["status"], "matched",
            "{typed}"
        );
    }
}

#[tokio::test]
async fn review_assumed_conversion_rates_must_be_positive() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    for rate in ["0", "-1"] {
        let error = run_conversion(&executor, conversion(rate))
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "inputs.rate: a positive number");
    }
}

#[tokio::test]
async fn review_existing_citation_checks_keep_legacy_exponent_precision() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let source = stage_conversion_source(
        &executor,
        ToolMethod::KrxDailyQuote,
        "ratio-source",
        json!({"symbol":"EXAMPLE"}),
        json!({"fundamentals":{"count":1400}}),
    )
    .await;
    let result = executor
        .execute(
            &policy(),
            ToolMethod::Calculate,
            json!({"operation":"ratio","inputs":{"numerator":"1e3","denominator":1},
            "input_sources":{"numerator":source}}),
        )
        .await
        .unwrap();
    assert_eq!(result["inputs"][0]["provenance"]["status"], "not_found");
}

fn dataset_conversion(reference: &crate::datasets::DatasetRef, pointer: &str) -> Value {
    let mut request = conversion("1334.87");
    request["inputs"]["amount"] = json!({"dataset_id":reference.dataset_id,
        "revision":reference.revision,"pointer":pointer});
    request
}

#[tokio::test]
async fn review_conversion_checks_the_pinned_amount_row_with_or_without_a_citation() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let mut payload = json!({"rows":[
        {"revenue":3930,"reported_currency":"JPY"},
        {"revenue":3930,"reported_currency":"USD"},
        {"revenue":3930}
    ]});
    let reference = executor
        .state
        .store
        .dataset_put("loupist-a", "revenue", &payload, json!({}))
        .unwrap();
    payload["dataset_ref"] = json!(reference);
    let source = stage_conversion_source(
        &executor,
        ToolMethod::KrxDailyQuote,
        "dataset-source",
        json!({"symbol":"EXAMPLE"}),
        payload,
    )
    .await;
    let mut checks = Vec::new();
    for cite in [false, true] {
        let mut outcomes = Vec::new();
        for index in 0..3 {
            let pointer = format!("/rows/{index}/revenue");
            let mut request = dataset_conversion(&reference, &pointer);
            if cite {
                request["input_sources"] = json!({"amount":source});
            }
            outcomes.push(
                run_conversion(&executor, request)
                    .await
                    .map_err(|error| error.to_string())
                    .map(|result| {
                        let amount = &result["inputs"][0];
                        assert_eq!(amount["provenance"]["kind"], "dataset");
                        assert_eq!(amount["provenance"]["pointer"], pointer);
                        amount.get("currency_basis").cloned()
                    }),
            );
        }
        checks.push(outcomes);
    }
    let expected = vec![
        Err("input_sources.amount: the source states this value in JPY; unit says USD".to_owned()),
        Ok(None),
        Ok(Some(json!("declared"))),
    ];
    assert_eq!(checks, [expected.clone(), expected]);
}

#[tokio::test]
async fn review_conversion_rejects_minor_unit_codes_in_cited_and_pinned_rows() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary);
    let mut observed = Vec::new();
    let mut expected = Vec::new();
    for (index, (stated, major, allowed)) in [
        ("GBp", "GBP", false),
        ("ZAc", "ZAR", false),
        ("ILA", "ILS", false),
        ("GBP", "GBP", true),
        ("gbp", "GBP", true),
    ]
    .into_iter()
    .enumerate()
    {
        let row = json!({"close":3930,"currency":stated});
        let payload = json!({"price":row});
        let source = stage_conversion_source(
            &executor,
            ToolMethod::KrxDailyQuote,
            &format!("case-{index}"),
            json!({"symbol":format!("EXAMPLE{index}")}),
            payload.clone(),
        )
        .await;
        let reference = executor
            .state
            .store
            .dataset_put("loupist-a", &format!("case-{index}"), &payload, json!({}))
            .unwrap();
        let mut cited = conversion("1334.87");
        cited["input_sources"] = json!({"amount":source});
        let pinned = dataset_conversion(&reference, "/price/close");
        for (kind, mut request) in [("cited", cited), ("pinned", pinned)] {
            request["unit"] = json!(major);
            let result = run_conversion(&executor, request)
                .await
                .map(|result| result["inputs"][0].get("currency_basis").cloned())
                .map_err(|error| error.to_string());
            observed.push((stated, kind, result));
            let outcome = if allowed {
                Ok(None)
            } else {
                Err(format!("input_sources.amount: the source states this value in {stated}; unit says {major}"))
            };
            expected.push((stated, kind, outcome));
        }
    }
    assert_eq!(observed, expected);
}
