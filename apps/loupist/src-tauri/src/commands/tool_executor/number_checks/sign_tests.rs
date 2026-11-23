//! Outflow rows, per-share rows, candidate order and per-share ratios, from
//! two stored Tesla answers and their false-positive boundaries.
use super::*;
use crate::domain::answer_checks::{ComparisonReason, SourceSign};
use NumberCheckStatus::{Differs, Matched, NotFound, Rounded};

pub(super) fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/tesla-q3-number-checks.json"
    ))
    .unwrap()
}

/// The saved Source a `world_read` of a fixture passage returns.
pub(super) fn envelope(source: &Value) -> String {
    json!({"charset": null, "content": source["content"]}).to_string()
}

fn run_result(result_ref: &str, tool: &str, origin: RunResultOrigin, payload: Value) -> RunResult {
    RunResult {
        result_ref: result_ref.into(),
        producer: RunResultProducer {
            origin,
            runtime_id: "native".into(),
            tool_name: tool.into(),
            provider: None,
        },
        origin: None,
        request_digest: "a".repeat(64),
        response_digest: "b".repeat(64),
        retrieved_at: "2026-11-22T19:06:39Z".into(),
        payload,
        model_delivery: None,
        warnings: vec![],
        upstream_result_refs: vec![],
    }
}

/// Checks `answer` against retained results, rewriting each fixture id to the
/// id its result projects.
async fn checked(answer: &str, results: Vec<(&str, RunResult)>) -> Vec<NumberCheck> {
    let capture = ToolCapture::default();
    let mut answer = answer.to_owned();
    let mut evidence = Vec::new();
    for (fixture_id, result) in results {
        let projected = evidence::project(&result);
        answer = answer.replace(fixture_id, &projected.id);
        capture
            .run_results
            .lock()
            .await
            .results
            .insert(result.result_ref.clone(), result);
        evidence.push(projected);
    }
    capture.number_checks(&answer, &evidence).await.checks
}

fn passage(name: &str, content: &str) -> RunResult {
    run_result(
        &format!("result:{name}"),
        "web_fetch",
        RunResultOrigin::AcquiredOriginal,
        json!({"content": content, "content_bytes": content.len(), "status": 200}),
    )
}

async fn one(passage_text: &str, answer: &str) -> NumberCheck {
    let read = passage("boundary", passage_text);
    let id = evidence::project(&read).id;
    let answer = answer.replace("{id}", &id);
    let checks = checked(&answer, vec![(id.as_str(), read)]).await;
    assert_eq!(checks.len(), 1, "{answer}");
    checks.into_iter().next().unwrap()
}

#[tokio::test]
async fn tesla_answers_match_outflows_per_share_rows_and_vendor_fields() {
    let fixture = fixture();
    let mut results = Vec::new();
    for name in ["highlights", "statements"] {
        let source = &fixture["sources"][name];
        results.push((
            source["id"].as_str().unwrap(),
            run_result(
                &format!("result:{name}"),
                "world_read",
                RunResultOrigin::SavedContext,
                json!({"text": envelope(source), "scope": "source"}),
            ),
        ));
    }
    results.push((
        fixture["dataset"]["id"].as_str().unwrap(),
        run_result(
            "result:dataset",
            "dataset_query",
            RunResultOrigin::Derived,
            fixture["dataset"]["payload"].clone(),
        ),
    ));
    let checks = checked(fixture["answers"]["grok"].as_str().unwrap(), results).await;
    let statuses: Vec<_> = checks.iter().map(|check| check.status).collect();
    assert_eq!(
        statuses,
        [
            // 희석 EPS row: the per-share row ignores the table's billions.
            Matched, Matched, Matched, Matched, Matched,
            // 잉여현금흐름 row, signed where the source signs it.
            Rounded, Rounded, Rounded, Rounded, Rounded,
            // Operating cash flow, capex, free cash flow.
            Rounded, Rounded, Rounded,
            // TTM capex now and a year earlier, Q2 capex, vendor fields.
            Rounded, Rounded, Rounded, Rounded, Rounded,
            // Signed free cash flow from the issuer and the vendor.
            Rounded, Rounded, // TTM capex again, TTM free cash flow, Q3 capex again.
            Rounded, Rounded, Rounded,
        ],
        "{checks:#?}"
    );
    let outflows: Vec<_> = checks
        .iter()
        .filter(|check| check.source_sign == Some(SourceSign::Outflow))
        .map(|check| check.link_index)
        .collect();
    assert_eq!(outflows, [11, 13, 14, 15, 16, 17, 20, 22]);
    let printed = |check: &NumberCheck| {
        let context = check.context.as_ref().expect("a match in the saved Source");
        context.text[context.start..context.end].to_owned()
    };
    for (index, source) in [
        (4, "0.22"),
        (11, "(4,732)"),
        (13, "(15,407)"),
        (14, "(8,914)"),
        (15, "(5,789)"),
    ] {
        assert_eq!(printed(&checks[index]), source, "{:?}", checks[index]);
    }
    for (index, pointer) in [
        (16, "/rows/1/capital_expenditure"),
        (17, "/rows/1/investments_in_property_plant_and_equipment"),
    ] {
        assert_eq!(checks[index].pointer.as_deref(), Some(pointer));
    }
    // Every figure of the reproduced EPS row comes from the diluted GAAP row.
    for check in &checks[..5] {
        let context = check.context.as_ref().unwrap();
        let before = &context.text[..context.start];
        let label = &before[before.rfind("EPS").unwrap()..];
        assert!(
            label.starts_with("EPS\tattributable\tto\tcommon\tstockholders,\tdiluted\t(GAAP)"),
            "{check:?}"
        );
    }
}

#[tokio::test]
async fn sign_carrying_rows_need_the_answer_to_state_the_sign() {
    for row in [
        "Net income (loss)",
        "Operating income (loss)",
        "Free cash flow",
        "Net cash provided by operating activities",
        "Net cash provided by (used in) investing activities",
        "Change in capital expenditures",
        "Net loss",
    ] {
        let source = format!("| In millions of USD | FY2025 |\n| {row} | (1,234) |");
        for (answer, expected) in [
            ("That line was [$1.23bn](#evidence-{id}).", NotFound),
            ("Net income was [$1.23bn](#evidence-{id}).", NotFound),
            ("Net income was [−$1.23bn](#evidence-{id}).", Rounded),
            (
                "The company reported a loss of [$1.23bn](#evidence-{id}).",
                Rounded,
            ),
            ("Net loss was [$1.23bn](#evidence-{id}).", Rounded),
            ("| 순손실 | [$1.23bn](#evidence-{id}) |", Rounded),
            ("The cash outflow was [$1.23bn](#evidence-{id}).", Rounded),
        ] {
            let check = one(&source, answer).await;
            assert_eq!(check.status, expected, "{row} / {answer}: {check:?}");
            assert!(check.source_sign.is_none(), "{check:?}");
            if expected == NotFound {
                assert_eq!(
                    check.comparison_reason,
                    Some(ComparisonReason::SignConflict)
                );
            }
        }
    }
    let growth = one(
        "| Metric | FY2025 |\n| Revenue growth | (4.8)% |",
        "Revenue declined [4.8%](#evidence-{id}) last year.",
    )
    .await;
    assert_eq!(growth.status, Matched, "{growth:?}");
    let unsigned = one(
        "| Metric | FY2025 |\n| Revenue growth | (4.8)% |",
        "Revenue growth was [4.8%](#evidence-{id}).",
    )
    .await;
    assert_eq!(unsigned.status, NotFound, "{unsigned:?}");
}

#[tokio::test]
async fn an_outflow_row_supports_only_figures_whose_clause_names_it_or_nothing() {
    let source = "| In millions of USD | Q3-2026 |\n| Purchases of property and equipment | (4,732) |\n| Repayments of debt | (1,500) |\n| Dividends paid | (750) |";
    for (answer, expected) in [
        ("설비투자는 [$4.73bn](#evidence-{id})입니다.", Rounded),
        ("In the quarter it was [$4.73bn](#evidence-{id}).", Rounded),
        ("Revenue was [$4.73bn](#evidence-{id}).", NotFound),
        (
            "Operating cash flow was [$4.73bn](#evidence-{id}).",
            NotFound,
        ),
        ("| 매출 | [$4.73bn](#evidence-{id}) |", NotFound),
        ("| 희석 EPS | [$4.73bn](#evidence-{id}) |", NotFound),
        ("Capex rose to [$4.73bn](#evidence-{id}).", Rounded),
        ("Capex increased by [$4.73bn](#evidence-{id}).", NotFound),
        ("Capex was [−$4.73bn](#evidence-{id}).", Rounded),
        ("Capex was [+$4.73bn](#evidence-{id}).", NotFound),
        ("Debt repayments were [$1.50bn](#evidence-{id}).", Matched),
        ("배당금 지급은 [$750m](#evidence-{id})입니다.", Matched),
    ] {
        let check = one(source, answer).await;
        assert_eq!(check.status, expected, "{answer}: {check:?}");
        let flipped = expected != NotFound && !answer.contains('−');
        assert_eq!(
            check.source_sign == Some(SourceSign::Outflow),
            flipped,
            "{answer}: {check:?}"
        );
    }
}

#[tokio::test]
async fn the_reproduced_row_picks_among_exact_candidates_of_other_periods() {
    let source = "($ in millions, except per share data)\tQ3-2025\tQ4-2025\tQ1-2026\tQ2-2026\tQ3-2026\tEPS\tbasic\t0.22\t0.25\t0.14\t0.33\t0.23\tEPS\tdiluted\t0.39\t0.24\t0.13\t0.32\t0.22";
    let read = passage("eps", source);
    let id = evidence::project(&read).id;
    let row = format!(
        "| Item | Q3 2025 | Q4 2025 | Q1 2026 | Q2 2026 | Q3 2026 |\n|---|---|---|---|---|---|\n| Diluted EPS | [$0.39](#evidence-{id}) | [$0.24](#evidence-{id}) | [$0.13](#evidence-{id}) | [$0.32](#evidence-{id}) | [$0.22](#evidence-{id}) |\n\nBasic EPS was [$0.22](#evidence-{id}) in Q3 2025 and [$0.25](#evidence-{id}) in Q4 2025."
    );
    let checks = checked(&row, vec![(id.as_str(), read)]).await;
    let at = |check: &NumberCheck| check.span.as_ref().unwrap().start;
    let diluted = source.find("0.39").unwrap();
    for check in &checks[..5] {
        assert_eq!(check.status, Matched, "{check:?}");
        assert!(at(check) >= diluted, "{check:?}");
    }
    assert_eq!(at(&checks[4]), source.rfind("0.22").unwrap());
    // Outside a reproduced row, the first exact candidate stands.
    assert_eq!(checks[5].status, Matched);
    assert_eq!(at(&checks[5]), source.find("0.22").unwrap());
    // An exact value of another period is still in the cited material.
    assert_eq!(checks[6].status, Matched);
    assert_eq!(at(&checks[6]), source.find("0.25").unwrap());
}

fn ratio(request: Value) -> Value {
    crate::calculations::calculate(&request, &mut NoDatasets).unwrap()
}

struct NoDatasets;
impl crate::calculations::DatasetSource for NoDatasets {
    fn dataset(
        &mut self,
        _: &str,
        _: u32,
    ) -> Result<(crate::datasets::DatasetRef, std::sync::Arc<Value>), String> {
        Err("no datasets".into())
    }
}

fn calculated(text: &str, payload: Value) -> NumberCheck {
    let link = EvidenceLink {
        index: 0,
        text: text.into(),
        evidence_id: "ev".into(),
    };
    let result = run_result(
        "result:ratio",
        "calculate",
        RunResultOrigin::Derived,
        payload,
    );
    check_link(&link, &result, None, &RunResultRegistry::default()).unwrap()
}

#[test]
fn a_ratio_stated_per_share_is_an_amount_and_two_amounts_stay_a_multiple() {
    let fixture = fixture();
    for (case, answer) in
        fixture["ratios"]
            .as_array()
            .unwrap()
            .iter()
            .zip(["3.46달러", "4.61달러", "6.92달러"])
    {
        let mut request = case["request"].clone();
        request.as_object_mut().unwrap().remove("input_sources");
        let payload = ratio(request);
        assert_eq!(payload["values"][0]["unit"], "USD per share");
        assert!(
            payload["formulas"][0]
                .as_str()
                .unwrap()
                .ends_with(" USD per share"),
            "{payload}"
        );
        assert_eq!(payload["inputs"][1]["unit"], json!({"kind": "number"}));
        let check = calculated(answer, payload.clone());
        assert_eq!(check.status, Matched, "{answer}: {check:?}");
        let check = calculated(&format!("{} USD per share", &answer[..4]), payload);
        assert_eq!(check.status, Matched, "{answer}: {check:?}");
    }
    let multiple = ratio(json!({"operation": "ratio", "unit": "USD",
        "inputs": {"numerator": 345.82, "denominator": 0.91}}));
    assert_eq!(multiple["values"][0]["unit"], "x");
    assert_eq!(calculated("380.02배", multiple.clone()).status, Matched);
    assert_eq!(calculated("$380.02", multiple).status, Differs);
    let share = ratio(json!({"operation": "ratio", "as_percent": true,
        "inputs": {"numerator": 20560, "denominator": 28046}}));
    assert_eq!(calculated("73.3%", share.clone()).status, Matched);
    assert_eq!(calculated("$73.31", share).status, Differs);
}

pub(super) fn research_fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/research-negatives-number-checks.json"
    ))
    .unwrap()
}

#[tokio::test]
async fn japanese_and_korean_negative_notations_meet_the_same_outflow_rule() {
    let fixture = research_fixture();
    let results = fixture["sources"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, source)| {
            (
                source["id"].as_str().unwrap(),
                passage(name, source["excerpt"].as_str().unwrap()),
            )
        })
        .collect();
    let checks = checked(fixture["answer"].as_str().unwrap(), results).await;
    let summary: Vec<_> = checks
        .iter()
        .map(|check| (check.status, check.source_sign.is_some()))
        .collect();
    assert_eq!(
        summary,
        [
            // Operating cash flow, signed where the source signs it.
            (Matched, false),
            (Matched, false),
            (Matched, false),
            // `(1,906,811)`, `△510,803`, `(8,366,764)` in acquisition rows.
            (Matched, true),
            (Matched, true),
            (Matched, true),
            // Leased equipment and intangible assets in three notations.
            (Matched, true),
            (Matched, true),
            (Matched, true),
            (Matched, true),
            // A signed operating cash flow the answer calls an outflow.
            (Matched, false),
            // Working-capital changes: the answer states the increase.
            (NotFound, false),
            (NotFound, false),
        ],
        "{checks:#?}"
    );
    for check in &checks[3..11] {
        assert!(check.span.is_some(), "{check:?}");
    }
    assert_eq!(
        checks[11].comparison_reason,
        Some(ComparisonReason::SignConflict)
    );
}

#[tokio::test]
async fn a_reference_number_in_a_table_cell_is_a_citation_not_a_figure() {
    let fixture = research_fixture();
    let marker = &fixture["citation_marker"];
    let read = run_result(
        "result:dataset",
        "dataset_query",
        RunResultOrigin::Derived,
        serde_json::from_str(marker["evidence"]["excerpt"].as_str().unwrap()).unwrap(),
    );
    let id = marker["evidence"]["id"].as_str().unwrap();
    let table = format!(
        "{}\n\n단위는 억 달러입니다.\n\n| 회계연도 | 매출 |\n|---|---:|\n| 2021 | 숫자 없음 [2](#evidence-{id}) |\n| 2022 | [2](#evidence-{id}) |\n\n근거는 다른 표입니다 [3](#evidence-{id}).",
        marker["answer"].as_str().unwrap()
    );
    let checks = checked(&table, vec![(id, read)]).await;
    let texts: Vec<_> = checks
        .iter()
        .map(|check| (check.link_index, check.text.as_str()))
        .collect();
    // The `[2]` and `[3]` that trail words are citations; a whole cell
    // under a unit is a figure.
    assert_eq!(texts, [(2, "2")], "{checks:#?}");
}
