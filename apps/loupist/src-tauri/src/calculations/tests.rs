use super::*;
use std::collections::BTreeMap;

/// In-memory pinned revisions; `refuse` simulates a store integrity failure.
#[derive(Default)]
struct Memory {
    datasets: BTreeMap<(String, u32), Arc<Value>>,
    refuse: Option<String>,
}

impl Memory {
    fn with(id: &str, payload: Value) -> Self {
        let mut memory = Self::default();
        memory.datasets.insert((id.into(), 1), Arc::new(payload));
        memory
    }
}

impl DatasetSource for Memory {
    fn dataset(
        &mut self,
        dataset_id: &str,
        revision: u32,
    ) -> Result<(DatasetRef, Arc<Value>), String> {
        if let Some(reason) = &self.refuse {
            return Err(reason.clone());
        }
        let payload = self
            .datasets
            .get(&(dataset_id.to_owned(), revision))
            .cloned()
            .ok_or_else(|| "not found: dataset revision".to_owned())?;
        Ok((
            DatasetRef {
                dataset_id: dataset_id.into(),
                revision,
                digest: "d".repeat(64),
            },
            payload,
        ))
    }
}

fn run(request: Value) -> Result<Value, CalcError> {
    calculate(&request, &mut Memory::default())
}

fn value(result: &Value, name: &str) -> f64 {
    result["values"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["name"] == name)
        .unwrap_or_else(|| panic!("{name} missing in {result}"))["value"]
        .as_f64()
        .unwrap()
}

fn display(result: &Value, name: &str) -> String {
    result["values"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["name"] == name)
        .unwrap()["display"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn formulas(result: &Value) -> Vec<String> {
    serde_json::from_value(result["formulas"].clone()).unwrap()
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

#[test]
fn free_cash_flow_substitutes_inputs_and_treats_negative_capex_as_an_outflow() {
    let result = run(
        json!({"operation": "free_cash_flow", "unit": "USD millions",
        "inputs": {"operating_cash_flow": 1240, "capex": "310"}}),
    )
    .unwrap();
    assert_eq!(value(&result, "free_cash_flow"), 930.0);
    assert_eq!(result["values"][0]["unit"], "USD millions");
    assert_eq!(formulas(&result), ["FCF = OCF − capex = 1,240 − 310 = 930"]);
    assert_eq!(
        result["inputs"][0]["provenance"],
        json!({"kind": "provided"})
    );
    assert!(result.get("notes").is_none());

    let negative = run(json!({"operation": "free_cash_flow",
        "inputs": {"operating_cash_flow": 1240, "capex": -310}}))
    .unwrap();
    assert_eq!(value(&negative, "free_cash_flow"), 930.0);
    assert_eq!(negative["values"][0]["unit"], "input units");
    assert!(negative["notes"][0]
        .as_str()
        .unwrap()
        .contains("outflow of 310"));
}

#[test]
fn reported_values_drop_binary_noise_but_keep_large_amounts_exact() {
    let small = run(json!({"operation": "free_cash_flow",
        "inputs": {"operating_cash_flow": 0.3, "capex": 0.1}}))
    .unwrap();
    assert_eq!(value(&small, "free_cash_flow"), 0.2);
    let large = run(json!({"operation": "free_cash_flow",
        "inputs": {"operating_cash_flow": "302231360123456", "capex": "1"}}))
    .unwrap();
    assert_eq!(value(&large, "free_cash_flow"), 302_231_360_123_455.0);
    assert_eq!(display(&large, "free_cash_flow"), "302,231,360,123,455");
}

#[test]
fn percent_change_and_cagr_match_hand_values() {
    let change =
        run(json!({"operation": "percent_change", "inputs": {"from": 1000, "to": 1100}})).unwrap();
    assert_eq!(value(&change, "change"), 100.0);
    assert_eq!(value(&change, "percent_change"), 10.0);
    assert_eq!(
        formulas(&change)[1],
        "(to − from) / from = 100 / 1,000 = 10.00%"
    );

    let cagr = run(
        json!({"operation": "cagr", "inputs": {"start": 1000, "end": 1500, "years": 3},
        "labels": {"start": "FY2021 revenue", "end": "FY2024 revenue"}}),
    )
    .unwrap();
    // 1.5^(1/3) = 1.1447142425533319
    close(value(&cagr, "cagr"), 14.471424255333);
    assert_eq!(display(&cagr, "cagr"), "14.47%");
    assert_eq!(
        formulas(&cagr)[0],
        "(FY2024 revenue / FY2021 revenue)^(1 / years) − 1 = (1,500 / 1,000)^(1 / 3) − 1 = 14.47%"
    );
    let halved =
        run(json!({"operation": "cagr", "inputs": {"start": 100, "end": 25, "years": 2}})).unwrap();
    assert_eq!(value(&halved, "cagr"), -50.0);
}

#[test]
fn margins_ratios_payout_and_leverage_match_hand_values() {
    let margin = run(
        json!({"operation": "margin", "inputs": {"profit": 120, "revenue": 1000},
        "labels": {"profit": "Operating income"}}),
    )
    .unwrap();
    assert_eq!(value(&margin, "margin"), 12.0);
    assert_eq!(
        formulas(&margin),
        ["Operating income / Revenue = 120 / 1,000 = 12.00%"]
    );

    let multiple = run(
        json!({"operation": "ratio", "inputs": {"numerator": 50, "denominator": 4},
        "labels": {"numerator": "Price", "denominator": "EPS"}}),
    )
    .unwrap();
    assert_eq!(value(&multiple, "ratio"), 12.5);
    assert_eq!(formulas(&multiple), ["Price / EPS = 50 / 4 = 12.50x"]);
    let share = run(json!({"operation": "ratio", "as_percent": true,
        "inputs": {"numerator": 25, "denominator": 200}}))
    .unwrap();
    assert_eq!(display(&share, "ratio"), "12.50%");

    let payout = run(json!({"operation": "payout_ratio",
        "inputs": {"dividends": -40, "earnings": 100, "free_cash_flow": 80}}))
    .unwrap();
    assert_eq!(value(&payout, "payout_on_earnings"), 40.0);
    assert_eq!(value(&payout, "payout_on_free_cash_flow"), 50.0);
    assert_eq!(formulas(&payout)[1], "Dividends / FCF = 40 / 80 = 50.00%");

    let leverage = run(json!({"operation": "net_debt_to_ebitda",
        "inputs": {"total_debt": 500, "cash": 200, "ebitda": 150}}))
    .unwrap();
    assert_eq!(value(&leverage, "net_debt"), 300.0);
    assert_eq!(value(&leverage, "net_debt_to_ebitda"), 2.0);
    assert_eq!(
        formulas(&leverage),
        [
            "Net debt = Total debt − Cash = 500 − 200 = 300",
            "Net debt / EBITDA = 300 / 150 = 2.00x"
        ]
    );
    let net_cash = run(json!({"operation": "net_debt_to_ebitda",
        "inputs": {"total_debt": 100, "cash": 400, "ebitda": 150}}))
    .unwrap();
    assert_eq!(value(&net_cash, "net_debt_to_ebitda"), -2.0);
    assert!(net_cash["notes"][0].as_str().unwrap().contains("net cash"));
}

#[test]
fn undefined_denominators_and_bases_are_typed_errors() {
    for (request, field) in [
        (
            json!({"operation": "ratio", "inputs": {"numerator": 1, "denominator": 0}}),
            "inputs.denominator",
        ),
        (
            json!({"operation": "percent_change", "inputs": {"from": 0, "to": 5}}),
            "inputs.from",
        ),
        (
            json!({"operation": "percent_change", "inputs": {"from": -5, "to": 5}}),
            "inputs.from",
        ),
        (
            json!({"operation": "cagr", "inputs": {"start": 100, "end": -1, "years": 2}}),
            "inputs.end",
        ),
        (
            json!({"operation": "cagr", "inputs": {"start": 100, "end": 120, "years": 0}}),
            "inputs.years",
        ),
        (
            json!({"operation": "margin", "inputs": {"profit": 5, "revenue": 0}}),
            "inputs.revenue",
        ),
        (
            json!({"operation": "payout_ratio", "inputs": {"dividends": 5, "earnings": -2}}),
            "inputs.earnings",
        ),
        (
            json!({"operation": "net_debt_to_ebitda", "inputs": {"total_debt": 5, "cash": 1, "ebitda": 0}}),
            "inputs.ebitda",
        ),
    ] {
        let error = run(request.clone()).unwrap_err();
        assert_eq!(error.field(), field, "{request}");
        assert!(matches!(
            error,
            CalcError::ZeroDenominator { .. } | CalcError::OutOfDomain { .. }
        ));
    }
    let missing =
        run(json!({"operation": "payout_ratio", "inputs": {"dividends": 5}})).unwrap_err();
    assert!(matches!(missing, CalcError::Missing { .. }));
}

#[test]
fn malformed_requests_name_the_field_and_the_expected_shape() {
    for (request, field) in [
        (json!({"operation": "average"}), "operation"),
        (json!({"inputs": {}}), "operation"),
        (
            json!({"operation": "cagr", "inputs": {"start": 1, "end": 2}}),
            "inputs.years",
        ),
        (
            json!({"operation": "cagr", "inputs": {"start": 1, "end": 2, "years": 1, "from": 3}}),
            "inputs.from",
        ),
        (
            json!({"operation": "cagr", "window": 5, "inputs": {"start": 1, "end": 2, "years": 1}}),
            "window",
        ),
        (
            json!({"operation": "ratio", "inputs": {"numerator": "NaN", "denominator": 1}}),
            "inputs.numerator",
        ),
        (
            json!({"operation": "ratio", "inputs": {"numerator": "1,234", "denominator": 1}}),
            "inputs.numerator",
        ),
        (
            json!({"operation": "ratio", "inputs": {"numerator": "inf", "denominator": 1}}),
            "inputs.numerator",
        ),
        (
            json!({"operation": "ratio", "inputs": {"numerator": true, "denominator": 1}}),
            "inputs.numerator",
        ),
        (
            json!({"operation": "ratio", "inputs": {"numerator": 1e19, "denominator": 1}}),
            "inputs.numerator",
        ),
        (
            json!({"operation": "ratio", "inputs": {"numerator": 1, "denominator": 1}, "labels": {"numerator": ""}}),
            "labels.numerator",
        ),
        (
            json!({"operation": "ratio", "inputs": {"numerator": 1, "denominator": 1}, "as_percent": "yes"}),
            "as_percent",
        ),
    ] {
        let error = run(request.clone()).unwrap_err();
        assert_eq!(error.field(), field, "{request}");
        assert!(!error.detail().is_empty());
    }
    let scientific =
        run(json!({"operation": "ratio", "inputs": {"numerator": "1.5e3", "denominator": " 3 "}}))
            .unwrap();
    assert_eq!(value(&scientific, "ratio"), 500.0);
}

#[test]
fn growth_series_lists_each_period_and_reorders_newest_first_dates() {
    let ascending = run(
        json!({"operation": "growth_series", "series": [100, 110, 121],
        "dates": ["2022", "2023", "2024"]}),
    )
    .unwrap();
    let descending = run(
        json!({"operation": "growth_series", "series": ["121", "110", "100"],
        "dates": ["2024", "2023", "2022"]}),
    )
    .unwrap();
    for result in [&ascending, &descending] {
        close(value(result, "total_change"), 21.0);
        close(value(result, "compound_growth_per_period"), 10.0);
        let periods = result["periods"].as_array().unwrap();
        assert_eq!(periods.len(), 2);
        assert_eq!(periods[0]["label"], "2023");
        close(periods[0]["growth_pct"].as_f64().unwrap(), 10.0);
        assert_eq!(periods[1]["display"], "10.00%");
    }
    assert!(ascending.get("notes").is_none());
    assert!(descending["notes"][0]
        .as_str()
        .unwrap()
        .contains("reordered"));

    let unordered = run(json!({"operation": "growth_series", "series": [1, 2, 3],
        "dates": ["2023", "2022", "2024"]}))
    .unwrap_err();
    assert_eq!(unordered.field(), "dates");
    let nonpositive = run(json!({"operation": "growth_series", "series": [0, 10, 20]})).unwrap();
    assert!(nonpositive["periods"][0]["growth_pct"].is_null());
    close(
        nonpositive["periods"][1]["growth_pct"].as_f64().unwrap(),
        100.0,
    );
    assert_eq!(nonpositive["notes"].as_array().unwrap().len(), 2);
}

#[test]
fn returns_summary_matches_hand_computed_return_volatility_and_drawdown() {
    // Returns +10%, −10%, +10%: total 1.089 − 1, sample stdev 0.11547 × √3 = 20%.
    let dates = ["2026-01-02", "2026-01-03", "2026-01-04", "2026-01-05"];
    let prices = run(
        json!({"operation": "returns_summary", "periods_per_year": 3,
        "series": [100, 110, 99, 108.9], "dates": dates}),
    )
    .unwrap();
    close(value(&prices, "total_return"), 8.9);
    close(value(&prices, "annualized_return"), 8.9);
    close(value(&prices, "annualized_volatility"), 20.0);
    close(value(&prices, "max_drawdown"), -10.0);
    assert_eq!(
        prices["drawdown"],
        json!({"peak": "2026-01-03", "trough": "2026-01-04", "recovered": null})
    );
    assert_eq!(
        formulas(&prices)[0],
        "Total return = last / first − 1 = 108.9 / 100 − 1 = 8.90%"
    );

    let returns = run(
        json!({"operation": "returns_summary", "periods_per_year": 3,
        "series_kind": "returns_pct", "series": [10, -10, 10], "dates": &dates[1..]}),
    )
    .unwrap();
    close(value(&returns, "total_return"), 8.9);
    close(value(&returns, "annualized_volatility"), 20.0);
    close(value(&returns, "max_drawdown"), -10.0);
    assert_eq!(returns["drawdown"]["peak"], "2026-01-03");

    // Daily annualization: 252 / 2 periods.
    let recovered = run(json!({"operation": "returns_summary", "series": [100, 50, 100]})).unwrap();
    close(value(&recovered, "max_drawdown"), -50.0);
    assert_eq!(recovered["drawdown"]["recovered"], "#3");
    close(value(&recovered, "annualized_return"), 0.0);

    for request in [
        json!({"operation": "returns_summary", "series": [100, 0, 50]}),
        json!({"operation": "returns_summary", "series_kind": "returns_pct", "series": [10, -101]}),
        json!({"operation": "returns_summary", "series": [100]}),
    ] {
        assert_eq!(run(request).unwrap_err().field(), "series");
    }
}

#[test]
fn moving_average_compares_the_latest_value_with_its_window() {
    let result =
        run(json!({"operation": "moving_average", "window": 3, "series": [1, 2, 3, 4, 5, 6]}))
            .unwrap();
    assert_eq!(value(&result, "moving_average"), 5.0);
    assert_eq!(value(&result, "latest"), 6.0);
    close(value(&result, "latest_vs_average"), 20.0);
    assert_eq!(
        formulas(&result)[0],
        "SMA(3) = sum of the latest 3 values / 3 = 15 / 3 = 5"
    );
    assert_eq!(
        run(json!({"operation": "moving_average", "window": 7, "series": [1, 2, 3, 4, 5, 6]}))
            .unwrap_err()
            .field(),
        "window"
    );
    assert!(matches!(
        run(json!({"operation": "moving_average", "series": [1, 2]})).unwrap_err(),
        CalcError::Missing { .. }
    ));
    assert_eq!(
        run(json!({"operation": "moving_average", "window": 2, "series": [1, null, 3]}))
            .unwrap_err()
            .field(),
        "series[1]"
    );
    let skipped = run(
        json!({"operation": "moving_average", "window": 2, "skip_missing": true,
        "series": [1, null, 3]}),
    )
    .unwrap();
    assert_eq!(value(&skipped, "moving_average"), 2.0);
    assert!(skipped["notes"][0].as_str().unwrap().contains("Dropped 1"));
}

#[test]
fn dcf_grid_matches_a_perpetuity_and_reports_terminal_share() {
    // One forecast year at 10% with zero growth is a 100 perpetuity: value 1,000.
    let result = run(json!({"operation": "dcf_grid", "forecast_years": 1,
        "wacc_pct": [10], "terminal_growth_pct": [0],
        "inputs": {"cash_flow": 100, "growth_pct": 0, "net_debt": 200, "shares": 10}}))
    .unwrap();
    close(result["grid"]["values"][0][0].as_f64().unwrap(), 80.0);
    close(
        result["grid"]["terminal_value_share_pct"][0][0]
            .as_f64()
            .unwrap(),
        90.909090909091,
    );
    assert_eq!(result["grid"]["measure"], "equity value per share");
    close(value(&result, "value_low"), 80.0);
    let example = formulas(&result)[1].clone();
    assert!(example.contains("equity = 1,000 − 200 = 800"), "{example}");
    assert!(example.ends_with("per share = 800 / 10 = 80"), "{example}");

    // Growth equal to terminal growth reduces to Gordon: 100 × 1.02 / (8% − 2%) = 1,700.
    let gordon = run(
        json!({"operation": "dcf_grid", "wacc_pct": [7, 8, 9], "terminal_growth_pct": [1, 2],
        "inputs": {"cash_flow": 100, "growth_pct": 2}}),
    )
    .unwrap();
    close(gordon["grid"]["values"][1][1].as_f64().unwrap(), 1700.0);
    assert_eq!(gordon["grid"]["values"].as_array().unwrap().len(), 3);
    assert!(value(&gordon, "value_low") < value(&gordon, "value_high"));
}

#[test]
fn reverse_dcf_recovers_the_growth_a_price_implies() {
    let result = run(
        json!({"operation": "reverse_dcf", "wacc_pct": [8, 12], "terminal_growth_pct": [2],
        "inputs": {"cash_flow": 100, "price": 17, "shares": 100}}),
    )
    .unwrap();
    // At 8% WACC a 1,700 value is the 2% Gordon case.
    assert!((result["grid"]["values"][0][0].as_f64().unwrap() - 2.0).abs() < 1e-6);
    assert!(result["grid"]["values"][1][0].as_f64().unwrap() > 2.0);
    assert_eq!(result["grid"]["unit"], "%");

    let unreachable = run(
        json!({"operation": "reverse_dcf", "wacc_pct": [8], "terminal_growth_pct": [2],
        "inputs": {"cash_flow": 1, "price": 1000000, "shares": 1000000}}),
    )
    .unwrap();
    assert!(unreachable["grid"]["values"][0][0].is_null());
    assert!(unreachable["notes"][0]
        .as_str()
        .unwrap()
        .contains("outside"));

    for (request, field) in [
        (
            json!({"operation": "reverse_dcf", "wacc_pct": [2], "terminal_growth_pct": [2],
            "inputs": {"cash_flow": 100, "price": 17, "shares": 100}}),
            "wacc_pct",
        ),
        (
            json!({"operation": "dcf_grid", "wacc_pct": [8], "terminal_growth_pct": [2],
            "inputs": {"cash_flow": -5, "growth_pct": 2}}),
            "inputs.cash_flow",
        ),
        (
            json!({"operation": "dcf_grid", "wacc_pct": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10], "terminal_growth_pct": [0],
            "inputs": {"cash_flow": 5, "growth_pct": 2}}),
            "wacc_pct",
        ),
        (
            json!({"operation": "dcf_grid", "terminal_growth_pct": [0],
            "inputs": {"cash_flow": 5, "growth_pct": 2}}),
            "wacc_pct",
        ),
        (
            json!({"operation": "reverse_dcf", "wacc_pct": [8], "terminal_growth_pct": [2],
            "inputs": {"cash_flow": 100, "price": 1, "shares": 1, "net_debt": -5}}),
            "inputs.net_debt",
        ),
    ] {
        assert_eq!(
            run(request.clone()).unwrap_err().field(),
            field,
            "{request}"
        );
    }
}

#[test]
fn dataset_operands_carry_revision_provenance_and_missing_values_stay_missing() {
    let payload = json!({"statement": {"ocf": "1240", "capex": "-310", "blank": null},
    "rows": [
        {"date": "2026-01-05", "close": "108.9"},
        {"date": "2026-01-04", "close": 99},
        {"date": "2026-01-03", "close": "110"},
        {"date": "2026-01-02", "close": 100, "note": "first"}
    ]});
    let cell = |pointer: &str| json!({"dataset_id": "ds-1", "revision": 1, "pointer": pointer});
    let mut memory = Memory::with("ds-1", payload);
    let flow = calculate(
        &json!({"operation": "free_cash_flow",
            "inputs": {"operating_cash_flow": cell("/statement/ocf"), "capex": cell("/statement/capex")}}),
        &mut memory,
    )
    .unwrap();
    assert_eq!(value(&flow, "free_cash_flow"), 930.0);
    assert_eq!(
        flow["inputs"][1]["provenance"],
        json!({"kind": "dataset", "dataset_id": "ds-1", "revision": 1, "digest": "d".repeat(64), "pointer": "/statement/capex"})
    );

    let column = json!({"dataset_id": "ds-1", "revision": 1, "rows_pointer": "/rows", "column": "close", "date_column": "date"});
    let summary = calculate(
        &json!({"operation": "returns_summary", "periods_per_year": 3, "series": column}),
        &mut memory,
    )
    .unwrap();
    close(value(&summary, "total_return"), 8.9);
    assert_eq!(summary["drawdown"]["trough"], "2026-01-04");
    assert_eq!(summary["inputs"][0]["first"]["label"], "2026-01-02");
    assert_eq!(summary["inputs"][0]["provenance"]["column"], "close");
    assert_eq!(summary["inputs"][0]["provenance"]["rows"], 4);
    let notes = |result: &Value| {
        result["notes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert!(notes(&summary)
        .iter()
        .any(|note| note.contains("reordered")));
    assert!(!notes(&summary)
        .iter()
        .any(|note| note.contains("stored order")));
    // Without a date column the same newest-first rows run in stored order,
    // and the result says which order was assumed.
    let undated = calculate(
        &json!({"operation": "returns_summary", "periods_per_year": 3,
            "series": {"dataset_id": "ds-1", "revision": 1, "rows_pointer": "/rows", "column": "close"}}),
        &mut memory,
    )
    .unwrap();
    assert!(
        notes(&undated)
            .iter()
            .any(|note| note.contains("stored order") && note.contains("oldest first")),
        "{undated}"
    );
    assert_eq!(undated["inputs"][0]["first"]["label"], "#1");

    for (request, field) in [
        (
            json!({"operation": "ratio", "inputs": {"numerator": cell("/statement/blank"), "denominator": 1}}),
            "inputs.numerator.pointer",
        ),
        (
            json!({"operation": "ratio", "inputs": {"numerator": cell("/statement/none"), "denominator": 1}}),
            "inputs.numerator.pointer",
        ),
        (
            json!({"operation": "moving_average", "window": 2,
            "series": {"dataset_id": "ds-1", "revision": 1, "rows_pointer": "/rows", "column": "note"}}),
            "series.column (note) in row 0",
        ),
        (
            json!({"operation": "moving_average", "window": 2,
            "series": {"dataset_id": "ds-1", "revision": 1, "rows_pointer": "/statement", "column": "ocf"}}),
            "series.rows_pointer",
        ),
    ] {
        let error = calculate(&request, &mut memory).unwrap_err();
        assert_eq!(error.field(), field, "{request}");
        assert!(matches!(error, CalcError::Dataset { .. }));
    }

    let mut refused = Memory::with("ds-1", json!({}));
    refused.refuse = Some("conflict: dataset integrity check failed".into());
    let error = calculate(
        &json!({"operation": "ratio", "inputs": {"numerator": cell("/x"), "denominator": 1}}),
        &mut refused,
    )
    .unwrap_err();
    assert!(matches!(error, CalcError::Unavailable { .. }));
    assert!(error.to_string().contains("integrity"));
}

#[test]
fn display_text_groups_thousands_and_keeps_signs_readable() {
    assert_eq!(format::amount(1240.0), "1,240");
    assert_eq!(format::amount(-310.5), "-310.5");
    assert_eq!(format::amount(1234567.891), "1,234,567.89");
    assert_eq!(format::amount(0.0123456789), "0.0123457");
    assert_eq!(format::amount(0.0), "0");
    assert_eq!(format::percent(-0.001), "0.00%");
    assert_eq!(format::percent(-12.346), "-12.35%");
    assert_eq!(format::multiple(2.0), "2.00x");
    assert_eq!(format::rate(2.5), "2.5%");
}

#[test]
fn sum_totals_a_series_and_drops_missing_values_only_when_asked() {
    let result = run(
        json!({"operation": "sum", "series": [120, null, -30.5, 10.5],
        "dates": ["2026-10-27", "2026-10-28", "2026-10-29", "2026-10-30"], "skip_missing": true,
        "unit": "KRW millions"}),
    )
    .unwrap();
    close(value(&result, "total"), 100.0);
    close(value(&result, "count"), 3.0);
    close(value(&result, "mean"), 100.0 / 3.0);
    assert!(formulas(&result)[0].contains("2026-10-27 to 2026-10-30"));
    assert!(result["notes"][0].as_str().unwrap().contains("Dropped 1"));
    let missing = run(json!({"operation": "sum", "series": [1, null]})).unwrap_err();
    assert_eq!(missing.field(), "series[1]");
}

#[test]
fn holdings_overlap_sums_the_smaller_weight_of_each_shared_holding() {
    let result = run(json!({"operation": "holdings_overlap",
        "weights_a": {"AAPL": 7.0, "msft": 6.5, "NVDA": 5.0},
        "weights_b": {"MSFT": 8.0, "AAPL": 3.0, "XOM": 2.0},
        "labels": {"weights_a": "Fund A", "weights_b": "Fund B"}}))
    .unwrap();
    close(value(&result, "overlap"), 9.5);
    close(value(&result, "shared_holdings"), 2.0);
    assert_eq!(result["top_shared"][0]["id"], "MSFT");
    assert!(formulas(&result)[0].contains("Fund A"));

    let many: Map<String, Value> = (0..501).map(|i| (format!("H{i}"), json!(0.1))).collect();
    for (request, field) in [
        (
            json!({"operation": "holdings_overlap", "weights_a": many, "weights_b": {"A": 1}}),
            "weights_a",
        ),
        (
            json!({"operation": "holdings_overlap", "weights_a": {"A": 60, "B": 41}, "weights_b": {"A": 1}}),
            "weights_a",
        ),
        (
            json!({"operation": "holdings_overlap", "weights_a": {"A": 101}, "weights_b": {"A": 1}}),
            "weights_a.A",
        ),
        (
            json!({"operation": "holdings_overlap", "weights_a": {"A": 1, "a": 2}, "weights_b": {"A": 1}}),
            "weights_a",
        ),
        (
            json!({"operation": "holdings_overlap", "weights_a": {"A": 1}}),
            "weights_b",
        ),
    ] {
        assert_eq!(
            run(request.clone()).unwrap_err().field(),
            field,
            "{request}"
        );
    }
    // Rounding in fund reports may take a set just over 100%.
    assert!(run(json!({"operation": "holdings_overlap",
        "weights_a": {"A": 60, "B": 40.4}, "weights_b": {"A": 1}}))
    .is_ok());
}

#[test]
fn goal_grid_projects_and_solves_across_rates_with_end_of_period_deposits() {
    let forward = run(
        json!({"operation": "goal_grid", "inputs": {"contribution": 300, "years": 15},
        "periods_per_year": 12, "return_pct": [3, 5, 7], "unit": "USD"}),
    )
    .unwrap();
    let rows = forward["grid"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    for row in rows {
        let rate = row["return_pct"].as_f64().unwrap();
        let i = (1.0 + rate / 100.0).powf(1.0 / 12.0) - 1.0;
        let expected = 300.0 * ((1.0 + i).powi(180) - 1.0) / i;
        assert!(
            (row["value"].as_f64().unwrap() - expected).abs() < 1e-6,
            "{row}"
        );
    }
    close(value(&forward, "total_contributed"), 54_000.0);
    assert!(value(&forward, "value_low") < value(&forward, "value_high"));
    // A zero return: the deposits and the start only.
    let flat = run(
        json!({"operation": "goal_grid", "inputs": {"contribution": 100, "years": 2, "start": 50},
        "periods_per_year": 1, "return_pct": [0]}),
    )
    .unwrap();
    close(flat["grid"]["rows"][0]["value"].as_f64().unwrap(), 250.0);

    let backward = run(
        json!({"operation": "goal_grid", "inputs": {"target": 100000, "start": 10000, "years": 10},
        "periods_per_year": 4, "return_pct": [2, 6]}),
    )
    .unwrap();
    for row in backward["grid"]["rows"].as_array().unwrap() {
        let rate = row["return_pct"].as_f64().unwrap();
        let i = (1.0 + rate / 100.0).powf(0.25) - 1.0;
        let growth = (1.0 + i).powi(40);
        let deposit = row["contribution"].as_f64().unwrap();
        assert!((10000.0 * growth + deposit * (growth - 1.0) / i - 100000.0).abs() < 1e-6);
    }
    // The start alone reaches a small target: nothing more to save.
    let reached = run(
        json!({"operation": "goal_grid", "inputs": {"target": 1000, "start": 5000, "years": 5},
        "return_pct": [4]}),
    )
    .unwrap();
    close(
        reached["grid"]["rows"][0]["contribution"].as_f64().unwrap(),
        0.0,
    );

    for (request, field) in [
        (
            json!({"operation": "goal_grid", "inputs": {"contribution": 1, "target": 2, "years": 5}, "return_pct": [4]}),
            "inputs.contribution",
        ),
        (
            json!({"operation": "goal_grid", "inputs": {"years": 5}, "return_pct": [4]}),
            "inputs.contribution",
        ),
        (
            json!({"operation": "goal_grid", "inputs": {"contribution": 1, "years": 61}, "return_pct": [4]}),
            "inputs.years",
        ),
        (
            json!({"operation": "goal_grid", "inputs": {"contribution": 1, "years": 2.5}, "return_pct": [4]}),
            "inputs.years",
        ),
        (
            json!({"operation": "goal_grid", "inputs": {"contribution": 1, "years": 5}, "periods_per_year": 7, "return_pct": [4]}),
            "periods_per_year",
        ),
        (
            json!({"operation": "goal_grid", "inputs": {"contribution": 1, "years": 5}, "return_pct": [31]}),
            "return_pct[0]",
        ),
        (
            json!({"operation": "goal_grid", "inputs": {"contribution": 1, "years": 5}, "return_pct": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]}),
            "return_pct",
        ),
        (
            json!({"operation": "goal_grid", "inputs": {"contribution": 1, "years": 5}}),
            "return_pct",
        ),
    ] {
        assert_eq!(
            run(request.clone()).unwrap_err().field(),
            field,
            "{request}"
        );
    }
}

#[test]
fn dca_history_buys_the_first_trading_row_of_each_month_in_a_daily_series() {
    let dates = [
        "2026-01-02",
        "2026-01-05",
        "2026-01-30",
        "2026-02-02",
        "2026-02-27",
        "2026-03-03",
        "2026-03-31",
    ];
    let prices = [100.0, 90.0, 80.0, 50.0, 70.0, 100.0, 120.0];
    let result = run(
        json!({"operation": "dca_history", "series": prices, "dates": dates,
        "inputs": {"contribution": 1000}, "unit": "KRW"}),
    )
    .unwrap();
    // Buys at 100 (Jan 2), 50 (Feb 2) and 100 (Mar 3).
    let units = 10.0 + 20.0 + 10.0;
    close(value(&result, "units"), units);
    close(value(&result, "invested"), 3000.0);
    close(value(&result, "last_price"), 120.0);
    close(value(&result, "value"), units * 120.0);
    close(value(&result, "average_cost"), 3000.0 / units);
    close(
        value(&result, "return"),
        (units * 120.0 / 3000.0 - 1.0) * 100.0,
    );
    assert_eq!(result["purchases"]["count"], 3);
    assert!(formulas(&result)[0].contains("2026-01-02 to 2026-03-03"));

    // Every row here falls in a different ISO week; a 1% fee buys 990 a time.
    let weekly = run(
        json!({"operation": "dca_history", "series": prices, "dates": dates,
        "inputs": {"contribution": 1000, "fee_pct": 1}, "schedule": "weekly"}),
    )
    .unwrap();
    assert_eq!(weekly["purchases"]["count"], 7);
    let expected: f64 = prices.iter().map(|price| 990.0 / price).sum();
    close(value(&weekly, "units"), expected);

    for (request, field) in [
        (
            json!({"operation": "dca_history", "series": [1, 2], "inputs": {"contribution": 10}}),
            "dates",
        ),
        (
            json!({"operation": "dca_history", "series": [1, 2], "dates": ["2026-01", "2026-02"], "inputs": {"contribution": 10}, "schedule": "weekly"}),
            "dates[0]",
        ),
        (
            json!({"operation": "dca_history", "series": [1, 0], "dates": ["2026-01-02", "2026-02-02"], "inputs": {"contribution": 10}}),
            "series",
        ),
        (
            json!({"operation": "dca_history", "series": [1], "dates": ["2026-01-02"], "inputs": {"contribution": 10}, "schedule": "daily"}),
            "schedule",
        ),
        (
            json!({"operation": "dca_history", "series": [1], "dates": ["2026-01-02"], "inputs": {"contribution": 10, "fee_pct": 100}}),
            "inputs.fee_pct",
        ),
    ] {
        assert_eq!(
            run(request.clone()).unwrap_err().field(),
            field,
            "{request}"
        );
    }
}

#[test]
fn reverse_dcf_marks_cells_that_need_more_growth_than_the_reference() {
    let result = run(
        json!({"operation": "reverse_dcf", "wacc_pct": [8, 12], "terminal_growth_pct": [2, 3],
        "inputs": {"cash_flow": 100, "price": 17, "shares": 100, "reference_growth_pct": 5},
        "labels": {"reference_growth_pct": "FY2020–FY2025", "cash_flow": "free cash flow FY2025"}}),
    )
    .unwrap();
    let grid = &result["grid"];
    assert_eq!(grid["reference"], 5.0);
    for (row, cells) in grid["values"].as_array().unwrap().iter().enumerate() {
        for (column, cell) in cells.as_array().unwrap().iter().enumerate() {
            let above = grid["above_reference"][row][column].as_bool().unwrap();
            assert_eq!(
                above,
                cell.as_f64().is_some_and(|growth| growth > 5.0),
                "{row} {column}"
            );
        }
    }
    // 8% / 2% is the Gordon case at 2%, below the reference; 12% needs more.
    assert_eq!(grid["above_reference"][0][0], false);
    assert_eq!(grid["above_reference"][1][0], true);
    let inputs = result["inputs"].as_array().unwrap();
    let named = |name: &str| inputs.iter().find(|input| input["name"] == name).unwrap();
    // The reference's label is its period, which it keeps apart from its name.
    assert_eq!(named("reference_growth_pct")["label"], "reference growth");
    assert_eq!(named("reference_growth_pct")["period"], "FY2020–FY2025");
    assert_eq!(named("cash_flow")["label"], "free cash flow FY2025");

    // An unsolvable cell is never marked; without a reference nothing is.
    let unreachable = run(json!({"operation": "reverse_dcf", "wacc_pct": [8], "terminal_growth_pct": [2],
        "inputs": {"cash_flow": 1, "price": 1000000, "shares": 1000000, "reference_growth_pct": 5}}))
    .unwrap();
    assert_eq!(unreachable["grid"]["above_reference"][0][0], false);
    let plain = run(
        json!({"operation": "reverse_dcf", "wacc_pct": [8], "terminal_growth_pct": [2],
        "inputs": {"cash_flow": 100, "price": 17, "shares": 100}}),
    )
    .unwrap();
    assert!(plain["grid"].get("above_reference").is_none());
    for (request, field) in [
        (
            json!({"operation": "reverse_dcf", "wacc_pct": [8], "terminal_growth_pct": [2],
            "inputs": {"cash_flow": 100, "price": 17, "shares": 100, "reference_growth_pct": 150}}),
            "inputs.reference_growth_pct",
        ),
        (
            json!({"operation": "dcf_grid", "wacc_pct": [8], "terminal_growth_pct": [2],
            "inputs": {"cash_flow": 100, "growth_pct": 2, "reference_growth_pct": 5}}),
            "inputs.reference_growth_pct",
        ),
    ] {
        assert_eq!(
            run(request.clone()).unwrap_err().field(),
            field,
            "{request}"
        );
    }
}

/// Every input carries the name its formula prints and how it is printed, so a
/// popover never reads either back from the formula or the evidence (B98).
#[test]
fn every_input_carries_its_formula_label_and_unit() {
    let inputs = |result: &Value| -> Vec<(String, String, Value)> {
        result["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| {
                (
                    input["name"].as_str().unwrap().to_owned(),
                    input["label"].as_str().unwrap().to_owned(),
                    input["unit"].clone(),
                )
            })
            .collect()
    };
    let amount =
        |currency: &str, scale: u8| json!({"kind": "amount", "currency": currency, "scale": scale});
    let margin = run(json!({"operation": "margin", "unit": "KRW billions",
        "inputs": {"profit": "38.1", "revenue": "452.3"}, "labels": {"profit": "Operating income"}}))
    .unwrap();
    assert_eq!(
        inputs(&margin),
        [
            ("profit".into(), "Operating income".into(), amount("KRW", 9)),
            ("revenue".into(), "Revenue".into(), amount("KRW", 9)),
        ]
    );
    // The labels are the words the formula printed.
    assert_eq!(
        formulas(&margin),
        ["Operating income / Revenue = 38.1 / 452.3 = 8.42%"]
    );

    let growth = run(json!({"operation": "cagr", "unit": "억원",
        "inputs": {"start": 3000, "end": 4523, "years": 3}}))
    .unwrap();
    assert_eq!(
        inputs(&growth)[1..],
        [
            ("end".into(), "end".into(), amount("KRW", 8)),
            ("years".into(), "years".into(), json!({"kind": "number"})),
        ]
    );

    let grid = run(json!({"operation": "reverse_dcf", "unit": "USD millions",
        "wacc_pct": [9], "terminal_growth_pct": [2],
        "inputs": {"cash_flow": 412, "price": 41.2, "shares": 98.2, "reference_growth_pct": 8}}))
    .unwrap();
    assert_eq!(
        inputs(&grid),
        [
            ("cash_flow".into(), "cash flow".into(), amount("USD", 6)),
            (
                "price".into(),
                "price".into(),
                json!({"kind": "price", "currency": "USD"})
            ),
            ("shares".into(), "shares".into(), json!({"kind": "number"})),
            (
                "reference_growth_pct".into(),
                "reference growth".into(),
                json!({"kind": "percent"})
            ),
        ]
    );

    // No unit is a bare amount; a unit Rust cannot place stays as text; "%" is a percent.
    let unit_of = |request: Value, index: usize| inputs(&run(request).unwrap())[index].2.clone();
    assert_eq!(
        unit_of(
            json!({"operation": "percent_change", "inputs": {"from": 10, "to": 12}}),
            0
        ),
        json!({"kind": "amount"})
    );
    assert_eq!(
        unit_of(
            json!({"operation": "percent_change", "unit": "stores", "inputs": {"from": 10, "to": 12}}),
            0
        ),
        json!({"kind": "number", "text": "stores"})
    );
    assert_eq!(
        unit_of(
            json!({"operation": "percent_change", "unit": "%", "inputs": {"from": 3.5, "to": 4.2}}),
            1
        ),
        json!({"kind": "percent"})
    );
    let returns = run(
        json!({"operation": "returns_summary", "series_kind": "returns_pct",
        "series": [1, -2, 3]}),
    )
    .unwrap();
    assert_eq!(
        inputs(&returns)[0],
        ("series".into(), "series".into(), json!({"kind": "percent"}))
    );
}

#[test]
fn a_unit_names_its_currency_only_when_it_names_exactly_one() {
    use crate::knowledge::numbers::currency_code;
    for (unit, code) in [
        ("USD millions", Some("USD")),
        ("$m", Some("USD")),
        ("US$ bn", Some("USD")),
        ("HK$ millions", Some("HKD")),
        ("₩", Some("KRW")),
        ("억원", Some("KRW")),
        ("백만달러", Some("USD")),
        ("KRW bn", Some("KRW")),
        ("euros", Some("EUR")),
        ("millions", None),
        ("stores", None),
        ("USD and KRW", None),
    ] {
        assert_eq!(currency_code(unit).as_deref(), code, "{unit}");
    }
}
