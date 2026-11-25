//! Live checks of KRX and Korea Investment against their real services.
//!
//! Both are ignored, and each returns at once when its keys are not in the
//! environment. With the test keys in a file of `NAME=value` lines:
//!
//! ```sh
//! sh -c 'set -a; . ~/.config/loupist-dev/connection-test-keys.env; set +a;
//!   exec cargo test --locked --lib -- --ignored finance_data::tests::live --nocapture'
//! ```
//!
//! They print shapes, counts and provider statuses, never a key or a token.
//! `LOUPIST_TEST_RECORD_DIR`, when set, receives one trimmed KRX daily file
//! so a recorded shape can replace the hand-written fixtures.

use super::*;

#[tokio::test]
#[ignore = "downloads a public Korea Investment symbol directory"]
async fn live_kis_symbol_lookup_reads_a_public_directory() {
    let result = FinanceDataService::new()
        .unwrap()
        .kis_symbol_lookup(json!({"query":"삼성전자", "market":"elw", "limit":1}))
        .await
        .unwrap();
    assert_eq!(result["matches"][0]["market"], "elw");
    assert_eq!(result["matches"][0]["market_code"], "W");
    eprintln!("KIS symbol lookup: public ELW directory passed");
}

const KRX_API_KEY: &str = "LOUPIST_TEST_KRX_API_KEY";
const KIS_APP_KEY: &str = "LOUPIST_TEST_KIS_APP_KEY";
const KIS_APP_SECRET: &str = "LOUPIST_TEST_KIS_APP_SECRET";
/// `real` or `demo`: the environment the app key was issued for.
const KIS_ENVIRONMENT: &str = "LOUPIST_TEST_KIS_ENVIRONMENT";
const RECORD_DIR: &str = "LOUPIST_TEST_RECORD_DIR";

fn variable(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn pacer() -> futures_util::future::BoxFuture<'static, ()> {
    fixed_request_spacing()
}

/// The keys of the first data row, which say whether the reply has the
/// documented fields without printing any value.
fn row_fields(rows: &Value) -> Vec<String> {
    rows.as_array()
        .and_then(|rows| rows.first())
        .and_then(Value::as_object)
        .map(|row| row.keys().cloned().collect())
        .unwrap_or_default()
}

#[tokio::test]
#[ignore = "calls KRX with LOUPIST_TEST_KRX_API_KEY"]
async fn live_krx_verifies_both_services_and_reads_a_recent_week() {
    let Some(api_key) = variable(KRX_API_KEY) else {
        eprintln!("skipped: {KRX_API_KEY} is not set");
        return;
    };
    let service = FinanceDataService::new().unwrap();
    let credentials = BTreeMap::from([("api_key".to_owned(), api_key.clone())]);
    let coverage = service
        .verify_credential(KRX_SOURCE_ID, &credentials, None)
        .await
        .expect("KRX verification")
        .expect("KRX reports its coverage");
    eprintln!(
        "KRX coverage: KOSPI {:?}, KOSDAQ {:?}",
        coverage.kospi, coverage.kosdaq
    );

    // The latest trading day appears at 08:00 KST the next day, so read the
    // week that ended two days ago.
    let end = krx::seoul_today() - ChronoDuration::days(2);
    let start = end - ChronoDuration::days(6);
    let mut read_any = false;
    for (symbol, market, access) in [
        ("005930", "KOSPI", coverage.kospi),
        ("035900", "KOSDAQ", coverage.kosdaq),
    ] {
        if access != crate::finance_credentials::KrxServiceAccess::Verified {
            eprintln!("KRX {market}: not checked, service access {access:?}");
            continue;
        }
        let result = service
            .krx_market_data(
                json!({"symbol": symbol, "start": start.to_string(), "end": end.to_string()}),
                &api_key,
                &pacer,
            )
            .await
            .unwrap_or_else(|error| panic!("KRX {market} {symbol}: {error}"));
        let bars = result["data"]["bars"].as_array().expect("bars");
        assert!(!bars.is_empty(), "KRX {market}: no bar in a week");
        assert_eq!(result["data"]["instrument"]["listing_market"], market);
        for bar in bars {
            for field in ["open", "high", "low", "close", "volume"] {
                assert!(
                    bar[field].is_string(),
                    "KRX {market}: {field} is not a number"
                );
            }
        }
        eprintln!(
            "KRX {market} {symbol}: {} bars, missing {}, instrument {}",
            bars.len(),
            result["data"]["missing_dates"],
            result["data"]["instrument"]
        );
        read_any = true;
    }
    assert!(read_any, "neither KRX daily trading service is approved");

    if let Some(directory) = variable(RECORD_DIR) {
        let market = if coverage.kospi == crate::finance_credentials::KrxServiceAccess::Verified {
            krx::KrxMarket::Kospi
        } else {
            krx::KrxMarket::Kosdaq
        };
        let url = market.daily_url(end).unwrap();
        let raw = service
            .fetch_provider(
                service.client.get(url.clone()).header("AUTH_KEY", &api_key),
                KRX_SOURCE_ID,
                url.to_string(),
                16 * 1024 * 1024,
            )
            .await
            .expect("KRX daily file");
        let mut root: Value = serde_json::from_slice(&raw.bytes).unwrap();
        let rows = root["OutBlock_1"].as_array().cloned().unwrap_or_default();
        eprintln!(
            "KRX {url}: top-level keys {:?}, {} rows, row fields {:?}",
            root.as_object().map(|root| root.keys().collect::<Vec<_>>()),
            rows.len(),
            row_fields(&root["OutBlock_1"])
        );
        let kept = rows
            .iter()
            .take(3)
            .chain(rows.iter().filter(|row| {
                row["ISU_CD"] == "005930" || row["ISU_CD"] == "035900" || row["ACC_TRDVOL"] == "0"
            }))
            .take(8)
            .cloned()
            .collect::<Vec<_>>();
        root["OutBlock_1"] = json!(kept);
        let path = std::path::Path::new(&directory).join(format!(
            "krx-{}-{}.json",
            url.path().rsplit('/').next().unwrap_or("daily"),
            end.format("%Y%m%d")
        ));
        std::fs::write(&path, serde_json::to_vec_pretty(&root).unwrap()).unwrap();
        eprintln!("recorded {}", path.display());
    }
}

#[tokio::test]
#[ignore = "calls Korea Investment with LOUPIST_TEST_KIS_APP_KEY and LOUPIST_TEST_KIS_APP_SECRET"]
async fn live_kis_issues_a_token_and_reads_a_quote_and_a_daily_chart() {
    let (Some(app_key), Some(app_secret)) = (variable(KIS_APP_KEY), variable(KIS_APP_SECRET))
    else {
        eprintln!("skipped: {KIS_APP_KEY} and {KIS_APP_SECRET} are not both set");
        return;
    };
    let environment = variable(KIS_ENVIRONMENT).unwrap_or_else(|| "real".to_owned());
    assert!(
        matches!(environment.as_str(), "real" | "demo"),
        "{KIS_ENVIRONMENT} must be real or demo"
    );
    let service = FinanceDataService::new().unwrap();
    let credentials = BTreeMap::from([
        ("app_key".to_owned(), app_key.clone()),
        ("app_secret".to_owned(), app_secret.clone()),
    ]);
    // Verifies the key once and keeps the token for the reads.
    service
        .verify_credential(KIS_SOURCE_ID, &credentials, Some(environment.as_str()))
        .await
        .unwrap_or_else(|error| panic!("KIS token ({environment}): {error}"));
    eprintln!("KIS {environment}: credential verified");

    let end = krx::seoul_today();
    let start = end - ChronoDuration::days(30);
    let reads = [
        (
            "domestic_stock.inquire_price",
            json!({"fid_cond_mrkt_div_code": "J", "fid_input_iscd": "005930"}),
        ),
        (
            "domestic_stock.inquire_daily_itemchartprice",
            json!({
                "fid_cond_mrkt_div_code": "J",
                "fid_input_iscd": "005930",
                "fid_input_date_1": start.format("%Y%m%d").to_string(),
                "fid_input_date_2": end.format("%Y%m%d").to_string(),
                "fid_period_div_code": "D",
                "fid_org_adj_prc": "1"
            }),
        ),
    ];
    for (operation, params) in reads {
        let result = service
            .kis_read_data(
                json!({"operation_id": operation, "params": params}),
                &environment,
                &app_key,
                &app_secret,
                None,
            )
            .await
            .unwrap_or_else(|error| panic!("KIS {operation}: {error}"));
        let response = result["data"]["response"]
            .as_object()
            .expect("KIS response");
        for (field, value) in response {
            match value {
                Value::Array(_) => eprintln!(
                    "KIS {operation} {field}: {} rows, fields {:?}",
                    value.as_array().map_or(0, Vec::len),
                    row_fields(value)
                ),
                Value::Object(row) => eprintln!(
                    "KIS {operation} {field}: fields {:?}",
                    row.keys().collect::<Vec<_>>()
                ),
                // Envelope fields: rt_cd, msg_cd, msg1.
                _ => eprintln!("KIS {operation} {field}: {value}"),
            }
        }
        eprintln!(
            "KIS {operation}: page {}, warnings {}",
            result["page"], result["warnings"]
        );
        assert!(
            result["page"]["returned"].as_u64() > Some(0),
            "KIS {operation}: no rows"
        );
        if operation == "domestic_stock.inquire_daily_itemchartprice" {
            assert!(
                response
                    .get("output2")
                    .and_then(Value::as_array)
                    .is_some_and(|bars| !bars.is_empty()),
                "KIS {operation}: no daily bars"
            );
        }
    }
    if environment == "real" {
        let calendar = service
            .kis_dividend_calendar(
                json!({"start":"2024-03-20","end":"2024-03-26","symbol":"000720"}),
                &environment,
                &app_key,
                &app_secret,
            )
            .await
            .unwrap_or_else(|error| panic!("KIS real KSD dividend calendar: {error}"));
        assert_eq!(calendar["data"]["rows"][0]["symbol"], "000720");
        eprintln!("KIS real: KSD dividend calendar passed");
    } else {
        eprintln!("KIS demo: KSD dividend calendar skipped (real connection required)");
    }
}
