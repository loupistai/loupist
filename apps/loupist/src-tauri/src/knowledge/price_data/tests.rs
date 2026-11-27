use super::*;
use crate::custom_connections::taint::{NUMERIC_ONLY_OPENBB_TOOLS, TRUST_THIRD_PARTY_TEXT};
use serde_json::json;

fn mcp(tool: &str, trust: &str) -> Value {
    json!({"producer": "mcp", "trust": trust,
        "request": {"server_id": "openbb", "tool_name": tool, "arguments": {"symbol": "ETH-USD", "provider": "yfinance", "interval": "1h"}}})
}

fn native(producer: &str, request: Value) -> Value {
    json!({"producer": producer, "trust": TRUST_NUMERIC, "request": request})
}

/// What `source_save` stores for a bundled market-data read: the delivered
/// payload with its retained dataset reference.
fn stored(rows: Value) -> Value {
    json!({
        "dataset_ref": {"dataset_id": "dataset-eth", "revision": 1, "digest": "a".repeat(64)},
        "fetched_at": "2026-11-23T09:00:00.000Z",
        "model_delivery": {"coverage": "complete", "retained": "complete"},
        "structuredContent": {"results": rows, "provider": "yfinance"},
    })
}

fn hourly_bars() -> Value {
    json!([
        {"close": 3010.5, "date": "2026-11-23T00:00:00+00:00", "high": 3020.1, "low": 2999.0, "open": 3005.2, "volume": 1234.5},
        {"close": 3012.0, "date": "2026-11-23T01:00:00+00:00", "high": 3015.0, "low": 3001.3, "open": 3010.5, "volume": 980.0},
    ])
}

#[test]
fn price_routes_are_reviewed_numeric_routes_and_statements_are_not_among_them() {
    for tool in PRICE_OPENBB_TOOLS {
        assert!(NUMERIC_ONLY_OPENBB_TOOLS.contains(tool), "{tool}");
    }
    for statement in [
        "equity_fundamental_income",
        "equity_fundamental_balance",
        "equity_fundamental_cash",
        "equity_fundamental_metrics",
        "equity_fundamental_ratios",
        "equity_estimates_consensus",
        "equity_estimates_forward_eps",
    ] {
        assert!(NUMERIC_ONLY_OPENBB_TOOLS.contains(&statement));
        assert!(!PRICE_OPENBB_TOOLS.contains(&statement), "{statement}");
        assert!(!price_dataset(&mcp(statement, TRUST_NUMERIC)));
    }
}

#[test]
fn kis_price_operations_are_bundled_market_reads() {
    let catalog: Value =
        serde_json::from_str(include_str!("../../../../marketplace/kis-read-api-v1.json")).unwrap();
    let operations = catalog["operations"].as_array().unwrap();
    for id in PRICE_KIS_OPERATIONS {
        let operation = operations
            .iter()
            .find(|operation| operation["id"] == *id)
            .unwrap_or_else(|| panic!("{id} is not a bundled KIS operation"));
        assert_eq!(operation["scope"], "market", "{id}");
    }
}

#[test]
fn the_producing_tool_decides_for_a_retained_dataset() {
    assert!(price_dataset(&mcp(
        "crypto_price_historical",
        TRUST_NUMERIC
    )));
    assert!(!price_dataset(&mcp(
        "fixedincome_government_treasury_rates",
        TRUST_NUMERIC
    )));
    assert!(!price_dataset(&mcp(
        "fixedincome_government_yield_curve",
        TRUST_NUMERIC
    )));
    assert!(price_dataset(&mcp(
        "fixedincome_government_treasury_prices",
        TRUST_NUMERIC
    )));
    assert!(price_dataset(&mcp(
        "currency_reference_rates",
        TRUST_NUMERIC
    )));
    // The recorded trust must still say numeric.
    assert!(!price_dataset(&mcp(
        "crypto_price_historical",
        TRUST_THIRD_PARTY_TEXT
    )));
    assert!(!price_dataset(&mcp("news_company", TRUST_THIRD_PARTY_TEXT)));
    let mut custom = mcp("crypto_price_historical", TRUST_NUMERIC);
    custom["request"]["server_id"] = json!("custom-prices");
    assert!(!price_dataset(&custom));

    assert!(price_dataset(&native(
        "KrxDailyQuote",
        json!({"symbol": "005930"})
    )));
    assert!(price_dataset(&native(
        "KisRead",
        json!({"operation_id": "domestic_stock.inquire_daily_itemchartprice", "params": {}})
    )));
    for request in [
        json!({"operation_id": "domestic_stock.news_title", "params": {}}),
        json!({"operation_id": "domestic_stock.volume_rank", "params": {}}),
        json!({}),
    ] {
        assert!(!price_dataset(&native("KisRead", request)));
    }
    assert!(!price_dataset(&native(
        "WorldbankSeries",
        json!({"indicator": "NY.GDP.MKTP.CD"})
    )));
    assert!(!price_dataset(&native(
        "DefillamaRead",
        json!({"route": "protocol"})
    )));
    assert!(!price_dataset(&native("DartFinancials", json!({}))));
}

#[test]
fn a_stored_bar_table_is_price_data_by_its_shape() {
    // The dataset row is gone; the stored rows still say what they are.
    assert!(price_payload(&stored(hourly_bars()), None));
    // One close column is not a price family.
    assert!(!price_payload(
        &stored(
            json!([{"date": "2026-11-20", "close": 3010.5}, {"date": "2026-11-21", "close": null}])
        ),
        None
    ));
    // Quotes for several symbols, strings for numbers, a field missing from one row.
    assert!(price_payload(
        &stored(json!([
            {"symbol": "AAPL", "last_price": "231.10", "bid": 231.0, "ask": 231.2, "volume": 1_000},
            {"symbol": "MSFT", "last_price": "418.45", "bid": 418.4, "ask": 418.5},
        ])),
        None
    ));
    // Native bars under `/data/rows`.
    assert!(price_payload(
        &json!({"dataset_ref": {"dataset_id": "dataset-krx", "revision": 1, "digest": "b".repeat(64)},
            "data": {"rows": [{"date": "2026-11-20", "open": "1000", "high": "1060", "low": "990", "close": "1050", "volume": "12,345"}]}}),
        None
    ));
    // The metadata alone decides for rows whose field names say nothing.
    let kis = json!({"dataset_ref": {"dataset_id": "dataset-kis", "revision": 1, "digest": "c".repeat(64)},
        "data": {"rows": [{"stck_bsop_date": "20261120", "stck_clpr": "61200"}]}});
    assert!(!price_payload(&kis, None));
    assert!(price_payload(
        &kis,
        Some(&native(
            "KisRead",
            json!({"operation_id": "domestic_stock.inquire_daily_itemchartprice"})
        ))
    ));

    for rows in [
        // An income statement keeps going to World.
        json!([{"period_ending": "2025-12-31", "fiscal_year": 2025, "revenue": 1.0e9, "net_income": 2.0e8}]),
        // A statistic is not a price.
        json!([{"date": "2026-10-01", "value": 3.1}]),
        // Text beside the prices.
        json!([{"date": "2026-11-20", "close": 3010.5, "headline": "ETH climbs"}]),
        // A price that is not a number.
        json!([{"date": "2026-11-20", "close": "about three thousand"}]),
        // Prices with nothing saying when or of what.
        json!([{"close": 3010.5, "volume": 10}]),
    ] {
        assert!(!price_payload(&stored(rows.clone()), None), "{rows}");
    }
    // A result without a retained dataset (a page, a search) never is.
    let mut page = stored(hourly_bars());
    page.as_object_mut().unwrap().remove("dataset_ref");
    assert!(!price_payload(&page, None));
}

#[test]
fn a_composed_table_is_price_data_by_its_shape() {
    let csv = "date,open,high,low,close,volume\n2026-11-20,3000,3050,2990,3010.5,120\n2026-11-21,3010.5,3080,3001,3075,98\n";
    assert!(price_file("text/csv", csv));
    assert!(price_file("text/plain", csv));
    assert!(price_file(
        "text/tab-separated-values",
        "Date\tClose\tVolume\n2026-11-20\t3010.5\t120\n2026-11-21\t3075\t98\n"
    ));
    let markdown = "# ETH-USD closes\n\n| Date | Close | Volume |\n|---|---:|---:|\n| 2026-11-20 | 3,010.50 | 120 |\n| 2026-11-21 | 3,075.00 | 98 |\n";
    assert!(price_file("text/markdown", markdown));
    assert!(price_file(
        "application/json",
        r#"[{"date":"2026-11-20","close":3010.5,"volume":120},{"date":"2026-11-21","close":3075,"volume":98}]"#
    ));

    for (media_type, text) in [
        (
            "text/csv",
            "date,close,note\n2026-11-20,3010.5,after the upgrade\n",
        ),
        ("text/csv", "date,close\n2026-11-20,n/a-ish\n"),
        (
            "text/csv",
            "\"date\",\"close\"\n\"2026-11-20\",\"3010.5\"\n",
        ),
        ("text/csv", "date,close\n"),
        (
            "text/markdown",
            "ETH rose after the upgrade.\n\n| Date | Close |\n|---|---|\n| 2026-11-20 | 3010.5 |\n",
        ),
        (
            "text/markdown",
            "| Company | Revenue |\n|---|---|\n| Samsung | 300 |\n",
        ),
        (
            "text/plain",
            "Samsung Electronics supplies memory to Apple.",
        ),
        ("application/pdf", csv),
    ] {
        assert!(!price_file(media_type, text), "{media_type}: {text}");
    }
}

#[test]
fn the_tool_decides_before_a_single_rate_or_yield_column() {
    let world = [
        ("text/csv", "date,rate\n2026-01-01,4.2\n2026-02-01,4.1\n"),
        (
            "text/csv",
            "date,policy_rate\n2026-01-01,3.50\n2026-02-01,3.25\n",
        ),
        (
            "text/csv",
            "date,dividend_yield\n2026-01-01,1.4\n2026-02-01,1.5\n",
        ),
        ("text/csv", "date,yield\n2026-01-01,4.1\n"),
        ("text/csv", "date,exchangerate\n2026-01-01,1350\n"),
        ("text/csv", "date,close\n2026-11-20,3010.5\n"),
    ];
    for (media_type, text) in world {
        assert!(!price_file(media_type, text), "{text}");
    }
    let curve = stored(json!([
        {"date": "2026-11-20", "maturity": "10Y", "rate": 4.2},
        {"date": "2026-11-21", "maturity": "10Y", "rate": 4.1},
    ]));
    assert!(!price_payload(
        &curve,
        Some(&mcp("fixedincome_government_yield_curve", TRUST_NUMERIC))
    ));

    let quoted = "\"date\",\"open\",\"high\",\"low\",\"close\",\"volume\"\n\"2026-11-20\",\"3,000\",\"3,050\",\"2,990\",\"3,010.5\",\"120\"\n\"2026-11-21\",\"3010.5\",\"3080\",\"3001\",\"3075\",\"98\"\n";
    assert!(price_file("text/csv", quoted));
    let korean = "날짜,시가,고가,저가,종가,거래량\n2026-11-20,70000,71000,69000,70500,1000\n2026-11-21,70500,72000,70000,71500,1100\n";
    assert!(price_file("text/csv", korean));
    assert!(price_payload(&stored(hourly_bars()), None));
    assert!(price_payload(
        &stored(json!([{"date": "2026-11-20", "close": 1.0, "volume": 2}])),
        Some(&mcp("equity_price_historical", TRUST_NUMERIC))
    ));
}
