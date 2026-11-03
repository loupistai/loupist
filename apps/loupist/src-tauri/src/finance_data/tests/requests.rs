use super::*;

#[test]
fn canonical_decimal_preserves_exact_values_and_rejects_non_decimals() {
    // JSON text, so numbers keep their source spelling before normalization.
    for (raw, expected) in [
        ("1e-7", "0.0000001"),
        ("9007199254740993", "9007199254740993"),
        ("-0.0", "0"),
        (r#""001,234.500""#, "1234.5"),
        (
            r#""9007199254740993.01234567890123456789""#,
            "9007199254740993.01234567890123456789",
        ),
    ] {
        let value: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(
            canonical_decimal(&value).as_deref(),
            Some(expected),
            "{raw}"
        );
    }
    let largest: Value = serde_json::from_str("1.7976931348623157e308").unwrap();
    let normalized = canonical_decimal(&largest).unwrap();
    assert!(!normalized.contains(['e', 'E']));
    assert_eq!(normalized.parse::<f64>().unwrap(), f64::MAX);
    for raw in ["1e1000000000", "NaN", "Infinity", "1.2.3", "--1"] {
        assert_eq!(canonical_decimal(&json!(raw)), None, "{raw}");
    }
}

#[test]
fn world_bank_small_observations_reach_pinned_data_and_chart_without_loss() {
    use crate::chat::chart::{dataset_from_dataset_selection, ChartFromDatasetInput};
    use crate::store::{LoupistStore, SqliteStore};

    let query = ValidatedMacroDataQuery {
        economy: "XQ".into(),
        indicator: "GT.FICT.INTENSITY".into(),
        start_year: 2021,
        end_year: 2023,
    };
    // Raw provider JSON includes exponent notation and an explicit missing value.
    let response = br#"[{"pages":1,"lastupdated":"2026-10-01"},[
      {"indicator":{"id":"GT.FICT.INTENSITY","value":"Fictional intensity"},"country":{"id":"XQ","value":"Fictional economy"},"countryiso3code":"XQQ","date":"2023","value":1e-7,"unit":"tonnes per unit"},
      {"indicator":{"id":"GT.FICT.INTENSITY","value":"Fictional intensity"},"country":{"id":"XQ","value":"Fictional economy"},"countryiso3code":"XQQ","date":"2022","value":null,"unit":"tonnes per unit"},
      {"indicator":{"id":"GT.FICT.INTENSITY","value":"Fictional intensity"},"country":{"id":"XQ","value":"Fictional economy"},"countryiso3code":"XQQ","date":"2021","value":-2.5e-9,"unit":"tonnes per unit"}
    ]]"#;
    let result = normalize_world_bank_response(
        &query,
        "https://api.worldbank.org/v2/country/XQ/indicator/GT.FICT.INTENSITY",
        "2026-10-26T00:00:00Z",
        response,
    )
    .unwrap();

    let store = SqliteStore::open_in_memory().unwrap();
    store
        .create_loupist(&crate::test_fixtures::loupist_profile("numeric"))
        .unwrap();
    let reference = store
        .dataset_put(
            "numeric",
            "intensity",
            &result,
            json!({"provider":"world-bank.indicators"}),
        )
        .unwrap();
    let queried = store
        .dataset_query(
            "numeric",
            &reference.dataset_id,
            reference.revision,
            Some("/data/observations"),
            0,
            3,
            &["period".into(), "value".into()],
        )
        .unwrap();
    assert_eq!(queried["rows"][0]["value"], "-0.0000000025");
    let (_, payload, _) = store
        .dataset("numeric", &reference.dataset_id, reference.revision)
        .unwrap();
    let selection: ChartFromDatasetInput = serde_json::from_value(json!({
        "dataset_id": reference.dataset_id, "revision": reference.revision,
        "rows_pointer": "/data/observations",
        "columns": [
            {"id":"period","label":"Year","kind":"string","pointer":"/period"},
            {"id":"value","label":"Intensity (tonnes per unit)","kind":"number","pointer":"/value"}
        ]
    }))
    .unwrap();
    let chart = dataset_from_dataset_selection(
        "numeric-chart".into(),
        selection,
        &payload,
        reference.digest,
    )
    .unwrap();
    assert_eq!(
        chart.rows,
        vec![
            vec![json!("2021"), json!("-0.0000000025")],
            vec![json!("2022"), Value::Null],
            vec![json!("2023"), json!("0.0000001")],
        ]
    );
}

#[test]
fn query_validation_rejects_extra_fields_and_path_injection() {
    assert!(validate_macro_query(json!({
        "provider": WORLD_BANK_SOURCE_ID,
        "economy": "../../all",
        "indicator": "NY.GDP.MKTP.CD",
        "start_year": 2020,
        "end_year": 2021
    }))
    .is_err());
    assert!(validate_macro_query(json!({
        "provider": WORLD_BANK_SOURCE_ID,
        "economy": "USA",
        "indicator": "NY.GDP.MKTP.CD",
        "start_year": 2020,
        "end_year": 2021,
        "url": "https://example.com"
    }))
    .is_err());
}

#[tokio::test]
async fn credential_verification_rejects_unknown_or_malformed_inputs_before_network() {
    let service = FinanceDataService::new().unwrap();
    let fields = |values: &[(&str, &str)]| {
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect::<BTreeMap<_, _>>()
    };
    for (provider, credentials) in [
        ("unknown.provider", fields(&[("api_key", "not-a-secret")])),
        (OPENDART_SOURCE_ID, fields(&[("api_key", "short")])),
        (
            KIS_SOURCE_ID,
            fields(&[("app_key", "test-kis-application-key")]),
        ),
        (
            KIS_SOURCE_ID,
            fields(&[
                ("app_key", "test-kis-application-key"),
                ("app_secret", "test-kis-application-secret"),
                ("api_key", "unexpected-secret"),
            ]),
        ),
    ] {
        assert!(
            matches!(
                service
                    .verify_credential(provider, &credentials, None)
                    .await,
                Err(FinanceDataError::InvalidQuery(_))
            ),
            "{provider}"
        );
    }
}
