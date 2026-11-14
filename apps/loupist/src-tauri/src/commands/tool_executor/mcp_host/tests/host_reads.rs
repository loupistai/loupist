//! Real executor, receipt validation, retention and reuse against a local MCP fixture.
use super::*;
use crate::commands::tool_executor::assets::HostReadError;
use crate::knowledge::assets::{self, plan::PlannedRead, AssetIdentity, PageRecord};
use std::path::Path;

const SCRIPT: &str = r#"
IFS= read -r bootstrap
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$MCP_TEST_TRANSCRIPT"
  id=${line#*\"id\":\"}; id=${id%%\"*}
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":"%s","result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}}\n' "$id" ;;
    *'"method":"tools/list"'*)
      printf '{"jsonrpc":"2.0","id":"%s","result":{"tools":[{"name":"equity_price_historical","inputSchema":{"type":"object","properties":{"symbol":{"type":"string"},"provider":{"enum":["fmp","intrinio","tiingo","tmx","cboe","nasdaq","yfinance"]}},"required":["symbol","provider"]},"annotations":{"readOnlyHint":true}}]}}\n' "$id" ;;
    *'"method":"tools/call"'*)
      provider=${line#*\"provider\":\"}; provider=${provider%%\"*}
      if { [ "$provider" = tmx ] && [[ "$line" = *'"fixture_case":"tmx_error"'* ]]; } || { [ "$provider" != fmp ] && [[ "$line" = *'"fixture_case":"empty_then_error"'* ]]; }; then
        printf '{"jsonrpc":"2.0","id":"%s","result":{"isError":true,"content":[{"type":"text","text":"fixture unavailable"}]}}\n' "$id"
      elif { [ "$provider" = tmx ] && [[ "$line" = *'"fixture_case":"tmx_empty"'* ]]; } || { [ "$provider" = fmp ] && [[ "$line" = *'"fixture_case":"empty_then_error"'* ]]; }; then
        printf '{"jsonrpc":"2.0","id":"%s","result":{"structuredContent":{"provider":"%s","results":[]},"isError":false}}\n' "$id" "$provider"
      elif [ "$provider" = fmp ] && { [[ "$line" = *'"symbol":"FAIL"'* ]] || [[ "$line" = *'"symbol":"ALLFAIL"'* ]]; }; then
        printf '{"jsonrpc":"2.0","id":"%s","result":{"isError":true,"content":[{"type":"text","text":"fixture unavailable"}]}}\n' "$id"
      elif [[ "$line" = *'"symbol":"ALLFAIL"'* ]] || { [ "$provider" = fmp ] && [[ "$line" = *'"symbol":"EMPTY"'* ]]; }; then
        printf '{"jsonrpc":"2.0","id":"%s","result":{"structuredContent":{"provider":"%s","results":[]},"isError":false}}\n' "$id" "$provider"
      else
        printf '{"jsonrpc":"2.0","id":"%s","result":{"structuredContent":{"provider":"%s","results":[{"date":"2026-11-12","close":145}]},"isError":false}}\n' "$id" "$provider"
      fi ;;
  esac
done
"#;

async fn fixture(temporary: &tempfile::TempDir, providers: &[&str]) -> (AppToolExecutor, PathBuf) {
    let mut fixture = spawn_script_server(temporary, "host", providers[0], None, SCRIPT).await;
    fixture.server.enabled_provider_ids = providers.iter().map(|id| (*id).to_owned()).collect();
    fixture
        .server
        .tools
        .get_mut("equity_price_historical")
        .unwrap()
        .input_schema["properties"]["provider"]["enum"] = json!(providers);
    let state = AppState::for_test(temporary.path().join("app"));
    let capture = capture(temporary, "host", None);
    install(&capture, fixture.server).await;
    let executor = turn_executor(temporary, &state, "loupist-a", capture);
    state
        .store
        .create_chat(&crate::commands::tests::chat("mcp-test", "loupist-a", 1))
        .unwrap();
    (executor, fixture.transcript)
}

fn page_with_market(executor: &AppToolExecutor, symbol: &str, market: &str) -> PageRecord {
    crate::commands::assets::with_assets(&executor.state, "loupist-a", true, |tx| {
        assets::ensure_page(
            tx,
            "loupist-a",
            &AssetIdentity {
                symbol: symbol.into(),
                market: market.into(),
                name: symbol.into(),
            },
            None,
            1,
        )
    })
    .unwrap()
}

fn read(symbol: &str) -> PlannedRead {
    PlannedRead {
        kind: "price",
        tool: "equity_price_historical",
        server: Some("openbb"),
        connection: "openbb.platform",
        arguments: json!({"symbol": symbol}),
    }
}

// Exercise both host entry points through the same fixture and policy.
async fn asset_read(
    executor: &AppToolExecutor,
    page: &PageRecord,
    read: &PlannedRead,
    allowed: Option<&[&str]>,
    max_age: Option<u64>,
) -> Result<(Value, Option<String>), HostReadError> {
    match allowed {
        Some(allowed) => {
            executor
                .host_read_among(&policy("loupist-a", None), page, read, max_age, allowed)
                .await
        }
        None => {
            executor
                .host_read(&policy("loupist-a", None), page, read, max_age)
                .await
        }
    }
}

fn providers_called(transcript: &Path) -> Vec<String> {
    fs::read_to_string(transcript)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let call: Value = serde_json::from_str(line).unwrap();
            (call["method"] == "tools/call").then(|| {
                call["params"]["arguments"]["provider"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
        })
        .collect()
}

#[tokio::test]
async fn host_market_routes_us_unknown_and_canadian_equities() {
    let temporary = tempfile::tempdir().unwrap();
    let (executor, transcript) = fixture(&temporary, &["tmx", "cboe", "yfinance"]).await;
    for (symbol, market, expected) in [
        ("SHOP", "TSX", "tmx"),
        ("SHOP.TO", "unknown", "tmx"),
        ("NVDA", "NASDAQ", "cboe"),
        ("NVDA", "unknown", "yfinance"),
        ("SHOP", "TSXV", "tmx"),
        ("SHOP.V", "unknown", "tmx"),
    ] {
        let page = page_with_market(&executor, symbol, market);
        let (_, provider) = asset_read(&executor, &page, &read(symbol), None, None)
            .await
            .unwrap();
        assert_eq!(provider.as_deref(), Some(expected), "{symbol}:{market}");
    }
    assert_eq!(
        providers_called(&transcript),
        ["tmx", "tmx", "cboe", "yfinance", "tmx", "tmx"]
    );
    executor.host_shutdown().await;
}

#[tokio::test]
async fn host_market_excludes_tmx_even_in_restricted_provider_lists() {
    let temporary = tempfile::tempdir().unwrap();
    let (executor, _) = fixture(&temporary, &["tmx", "nasdaq", "fmp"]).await;
    let page = page_with_market(&executor, "NVDA", "NASDAQ");
    for allowed in [&["tmx", "fmp"][..], &["tmx", "nasdaq"][..]] {
        let (_, provider) = asset_read(&executor, &page, &read("NVDA"), Some(allowed), None)
            .await
            .unwrap();
        assert_ne!(provider.as_deref(), Some("tmx"));
    }
    assert!(matches!(
        asset_read(&executor, &page, &read("NVDA"), Some(&["tmx"]), None).await,
        Err(HostReadError::NoProvider)
    ));
    let canadian = page_with_market(&executor, "SHOP", "TSX");
    let (_, provider) = asset_read(
        &executor,
        &canadian,
        &read("SHOP"),
        Some(&["nasdaq", "tmx"]),
        None,
    )
    .await
    .unwrap();
    assert_eq!(provider.as_deref(), Some("tmx"));
    executor.host_shutdown().await;
}

async fn fallback_and_reuse(symbol: &str) {
    let temporary = tempfile::tempdir().unwrap();
    let (executor, transcript) =
        fixture(&temporary, &["fmp", "intrinio", "tiingo", "yfinance"]).await;
    {
        let page = page_with_market(&executor, symbol, "NASDAQ");
        let (payload, provider) = asset_read(&executor, &page, &read(symbol), None, Some(1800))
            .await
            .unwrap();
        assert_eq!(provider.as_deref(), Some("intrinio"));
        assert!(payload.get("dataset_ref").is_some(), "{payload}");
        let count = providers_called(&transcript).len();
        let (reused, reused_provider) = asset_read(
            &executor,
            &page,
            &read(symbol),
            Some(&["intrinio"]),
            Some(1800),
        )
        .await
        .unwrap();
        assert_eq!(reused_provider, provider);
        assert_eq!(reused["dataset_ref"], payload["dataset_ref"]);
        assert_eq!(providers_called(&transcript).len(), count);
        let (_, among_provider) = asset_read(
            &executor,
            &page,
            &read(symbol),
            Some(&["fmp", "intrinio"]),
            Some(1800),
        )
        .await
        .unwrap();
        assert_eq!(among_provider, provider);
        assert_eq!(
            providers_called(&transcript).len(),
            count + usize::from(symbol == "FAIL"),
            "failures retry; retained empty and successful results reuse max_age"
        );
    }
    executor.host_shutdown().await;
}

#[tokio::test]
async fn host_market_falls_back_after_failure_and_reuses_the_serving_provider() {
    fallback_and_reuse("FAIL").await;
}

#[tokio::test]
async fn host_market_falls_back_after_empty_rows_and_reuses_the_serving_provider() {
    fallback_and_reuse("EMPTY").await;
}

#[tokio::test]
async fn host_market_retries_at_most_three_and_never_falls_back_when_pinned() {
    let temporary = tempfile::tempdir().unwrap();
    let (executor, transcript) =
        fixture(&temporary, &["fmp", "intrinio", "tiingo", "yfinance"]).await;
    let page = page_with_market(&executor, "ALLFAIL", "NASDAQ");
    let (payload, provider) = asset_read(&executor, &page, &read("ALLFAIL"), None, None)
        .await
        .unwrap();
    assert_eq!(payload["structuredContent"]["results"], json!([]));
    assert_eq!(provider.as_deref(), Some("tiingo"));
    assert_eq!(providers_called(&transcript), ["fmp", "intrinio", "tiingo"]);
    for symbol in ["FAIL", "EMPTY"] {
        let before = providers_called(&transcript).len();
        let _ = asset_read(&executor, &page, &read(symbol), Some(&["fmp"]), None).await;
        let mut pinned = read(symbol);
        pinned.arguments["provider"] = json!("fmp");
        let _ = asset_read(&executor, &page, &pinned, None, None).await;
        assert_eq!(&providers_called(&transcript)[before..], ["fmp", "fmp"]);
    }
    executor.host_shutdown().await;
}

async fn canadian_bare_failure(case: &str) {
    let temporary = tempfile::tempdir().unwrap();
    let (executor, transcript) = fixture(&temporary, &["tmx", "yfinance"]).await;
    let page = page_with_market(&executor, "SHOP", "TSX");
    let mut request = read("SHOP");
    request.arguments["fixture_case"] = json!(case);
    let outcome = asset_read(&executor, &page, &request, None, None).await;
    assert_eq!(
        providers_called(&transcript),
        ["tmx"],
        "a bare Canadian symbol must never reach a US-default vendor"
    );
    if case == "tmx_error" {
        assert!(outcome.is_err());
    } else {
        assert_eq!(
            outcome.unwrap().0["structuredContent"]["results"],
            json!([])
        );
    }
    executor.host_shutdown().await;
}

#[tokio::test]
async fn listing_identity_canadian_bare_never_falls_back_after_error() {
    canadian_bare_failure("tmx_error").await;
}

#[tokio::test]
async fn listing_identity_canadian_bare_never_falls_back_after_empty() {
    canadian_bare_failure("tmx_empty").await;
}

#[tokio::test]
async fn listing_identity_canadian_vendor_order_requires_a_verified_suffix() {
    let temporary = tempfile::tempdir().unwrap();
    let (executor, transcript) = fixture(
        &temporary,
        &["fmp", "intrinio", "tiingo", "tmx", "cboe", "yfinance"],
    )
    .await;
    for (symbol, market) in [("RY", "TSX"), ("TD.TO", "TSX"), ("SHOP.V", "TSXV")] {
        let page = page_with_market(&executor, symbol, market);
        let (_, provider) = asset_read(&executor, &page, &read(symbol), None, None)
            .await
            .unwrap();
        assert_eq!(provider.as_deref(), Some("tmx"));
    }
    for symbol in ["TD.TO", "SHOP.V"] {
        let page = page_with_market(&executor, symbol, "TSX");
        for case in ["tmx_error", "tmx_empty"] {
            let mut request = read(symbol);
            request.arguments["fixture_case"] = json!(case);
            let (_, provider) = asset_read(&executor, &page, &request, None, None)
                .await
                .unwrap();
            assert_eq!(provider.as_deref(), Some("yfinance"));
        }
    }
    let calls = providers_called(&transcript);
    assert!(calls
        .iter()
        .all(|provider| ["tmx", "yfinance"].contains(&provider.as_str())));
    executor.host_shutdown().await;
}

#[tokio::test]
async fn listing_identity_known_foreign_bare_symbols_have_no_candidate() {
    let temporary = tempfile::tempdir().unwrap();
    let (executor, transcript) = fixture(
        &temporary,
        &["fmp", "intrinio", "tiingo", "tmx", "cboe", "yfinance"],
    )
    .await;
    for (symbol, market) in [
        ("BP", "LSE"),
        ("SAP", "XETRA"),
        ("AIR", "EURONEXT"),
        ("7203", "JPX"),
        ("0700", "HKEX"),
        ("BHP", "ASX"),
        ("600519", "SSE"),
        ("000001", "SZSE"),
    ] {
        let page = page_with_market(&executor, symbol, market);
        assert!(
            matches!(
                asset_read(&executor, &page, &read(symbol), None, None).await,
                Err(HostReadError::NoProvider)
            ),
            "{symbol}:{market}"
        );
    }
    assert!(providers_called(&transcript).is_empty());
    executor.host_shutdown().await;
}

#[tokio::test]
async fn listing_identity_foreign_suffixes_use_only_the_verified_vendor() {
    let temporary = tempfile::tempdir().unwrap();
    let (executor, transcript) = fixture(
        &temporary,
        &["fmp", "intrinio", "tiingo", "tmx", "cboe", "yfinance"],
    )
    .await;
    for (symbol, market) in [
        ("BP.L", "LSE"),
        ("SAP.DE", "XETRA"),
        ("AIR.PA", "EURONEXT"),
        ("7203.T", "TSE"),
        ("0700.HK", "HKEX"),
        ("BHP.AX", "ASX"),
        ("600519.SS", "SSE"),
        ("000001.SZ", "SZSE"),
    ] {
        let page = page_with_market(&executor, symbol, market);
        let (_, provider) = asset_read(&executor, &page, &read(symbol), None, None)
            .await
            .unwrap();
        assert_eq!(provider.as_deref(), Some("yfinance"), "{symbol}:{market}");
    }
    let wrong_suffix = page_with_market(&executor, "SAP.T", "XETRA");
    assert!(matches!(
        asset_read(&executor, &wrong_suffix, &read("SAP.T"), None, None).await,
        Err(HostReadError::NoProvider)
    ));
    assert_eq!(providers_called(&transcript), vec!["yfinance"; 8]);
    executor.host_shutdown().await;
}

#[tokio::test]
async fn listing_identity_unknown_us_venue_codes_keep_the_us_default() {
    let temporary = tempfile::tempdir().unwrap();
    let (executor, _) = fixture(&temporary, &["tmx", "cboe", "yfinance"]).await;
    for market in ["NGM", "PCX", "OTC", "", "unrecognised", "unknown"] {
        let mut page = page_with_market(&executor, "NVDA", "unknown");
        page.market = market.to_owned();
        let (_, provider) = asset_read(&executor, &page, &read("NVDA"), None, None)
            .await
            .unwrap();
        assert_eq!(provider.as_deref(), Some("yfinance"), "{market}");
    }
    executor.host_shutdown().await;
}

#[tokio::test]
async fn listing_identity_empty_rows_survive_a_later_provider_error() {
    let temporary = tempfile::tempdir().unwrap();
    let (executor, transcript) = fixture(&temporary, &["fmp", "intrinio", "tiingo"]).await;
    let page = page_with_market(&executor, "NVDA", "NASDAQ");
    let mut request = read("NVDA");
    request.arguments["fixture_case"] = json!("empty_then_error");
    let (payload, provider) = asset_read(&executor, &page, &request, None, None)
        .await
        .unwrap();
    assert_eq!(provider.as_deref(), Some("fmp"));
    assert_eq!(payload["structuredContent"]["results"], json!([]));
    assert_eq!(providers_called(&transcript), ["fmp", "intrinio", "tiingo"]);
    executor.host_shutdown().await;
}

#[tokio::test]
async fn listing_identity_no_provider_is_a_connection_outcome_on_refresh() {
    use crate::commands::assets::{refresh_with, with_assets, HostReads};
    let temporary = tempfile::tempdir().unwrap();
    let (executor, transcript) = fixture(&temporary, &["yfinance"]).await;
    let page = page_with_market(&executor, "SAP", "XETRA");
    let grants = BTreeSet::from(["openbb.platform".into()]);
    refresh_with(
        &executor.state,
        "loupist-a",
        &page.asset_id,
        assets::ObservationOrigin::Refresh,
        HostReads {
            executor: &executor,
            capabilities: &grants,
            max_age: None,
        },
        None,
    )
    .await
    .unwrap();
    let context = crate::commands::assets::view_context(&executor.state).unwrap();
    let view = with_assets(&executor.state, "loupist-a", false, |tx| {
        assets::page_view(tx, "loupist-a", &page.asset_id, 0, &context)
    })
    .unwrap();
    let price = view
        .checks
        .failures
        .iter()
        .find(|failure| failure.kind == "price")
        .expect("a bare foreign symbol must be unavailable instead of recording a US price");
    assert_eq!(price.reason, "needs_connection");
    let filings = view
        .checks
        .failures
        .iter()
        .find(|failure| failure.kind == "filings")
        .unwrap();
    assert_eq!(filings.reason, "needs_connection");
    assert!(providers_called(&transcript).is_empty());
    executor.host_shutdown().await;
}
