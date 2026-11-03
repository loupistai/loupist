//! What the host reads for each kind of asset (v1: tools that exist today), and
//! what the page says it does not check yet. At most four reads per asset.
use chrono::{Datelike, Duration, NaiveDate};
use serde_json::{json, Value};

use super::class::{is_korean_market, AssetClass};

pub const MAX_READS: usize = 4;
/// OpenBB runs inside the bundled market-data runtime.
pub const OPENBB: &str = "openbb";

/// One read in a refresh. `connection` is the capability that grants it.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedRead {
    /// The check kind the page lists ("filings", "results", …).
    pub kind: &'static str,
    pub tool: &'static str,
    /// `None` for native tools; the MCP server id otherwise.
    pub server: Option<&'static str>,
    pub connection: &'static str,
    pub arguments: Value,
}

/// The latest OpenDART periodic report that should be filed by `today`.
fn dart_period(today: NaiveDate) -> (i32, &'static str) {
    let year = today.year();
    let after = |month: u32, day: u32| (today.month(), today.day()) >= (month, day);
    if after(11, 15) {
        (year, "q3")
    } else if after(8, 15) {
        (year, "half_year")
    } else if after(5, 16) {
        (year, "q1")
    } else if after(3, 31) {
        (year - 1, "annual")
    } else {
        (year - 1, "q3")
    }
}

fn mcp(
    kind: &'static str,
    tool: &'static str,
    connection: &'static str,
    arguments: Value,
) -> PlannedRead {
    PlannedRead {
        kind,
        tool,
        server: Some(OPENBB),
        connection,
        arguments,
    }
}

/// The reads for one refresh. `lookup` asks whether an equity is really a fund
/// (`etf_info` succeeds) on its first refresh.
pub fn reads(
    class: AssetClass,
    symbol: &str,
    market: &str,
    today: NaiveDate,
    lookup: bool,
) -> Vec<PlannedRead> {
    let start = |days: i64| (today - Duration::days(days)).to_string();
    let symbol = symbol.trim();
    let mut reads = match class {
        AssetClass::Equity if is_korean_market(market) => {
            let (year, period) = dart_period(today);
            vec![
                PlannedRead {
                    kind: "filings",
                    tool: "dart_filing_search",
                    server: None,
                    connection: "opendart.disclosures",
                    arguments: json!({"company": symbol, "start": start(365), "end": today.to_string(), "limit": 20}),
                },
                PlannedRead {
                    kind: "results",
                    tool: "dart_financials",
                    server: None,
                    connection: "opendart.disclosures",
                    arguments: json!({"company": symbol, "fiscal_year": year, "report_period": period, "basis": "consolidated"}),
                },
                PlannedRead {
                    kind: "price",
                    tool: "krx_daily_quote",
                    server: None,
                    connection: "krx.market-data",
                    arguments: json!({"symbol": symbol, "start": start(28), "end": today.to_string()}),
                },
                PlannedRead {
                    kind: "dividends",
                    tool: "dart_dividends",
                    server: None,
                    connection: "opendart.disclosures",
                    arguments: json!({"company": symbol, "year": dividend_year(today)}),
                },
            ]
        }
        AssetClass::Equity => {
            let mut reads = Vec::new();
            if lookup {
                reads.push(mcp(
                    "fees",
                    "etf_info",
                    "openbb.platform",
                    json!({"symbol": symbol}),
                ));
            }
            reads.push(mcp(
                "filings",
                "equity_fundamental_filings",
                "sec.edgar",
                json!({"symbol": symbol, "provider": "sec", "limit": 20}),
            ));
            reads.push(mcp(
                "results",
                "equity_fundamental_income",
                "openbb.platform",
                json!({"symbol": symbol, "period": "quarter", "limit": 4}),
            ));
            reads.push(mcp(
                "price",
                "equity_price_historical",
                "openbb.platform",
                json!({"symbol": symbol, "start_date": start(120)}),
            ));
            reads.push(mcp(
                "dividends",
                "equity_fundamental_dividends",
                "openbb.platform",
                json!({"symbol": symbol}),
            ));
            reads
        }
        // Providers want a pair: "BTC" asks for "BTCUSD" (yfinance reads it as BTC-USD).
        AssetClass::Crypto => vec![mcp(
            "price",
            "crypto_price_historical",
            "openbb.platform",
            json!({"symbol": crate::knowledge::watch::class::crypto_pair(symbol), "start_date": start(120)}),
        )],
        AssetClass::Fund => vec![
            mcp(
                "price",
                "etf_historical",
                "openbb.platform",
                json!({"symbol": symbol, "start_date": start(120)}),
            ),
            mcp(
                "fees",
                "etf_info",
                "openbb.platform",
                json!({"symbol": symbol}),
            ),
            mcp(
                "holdings",
                "etf_holdings",
                "openbb.platform",
                json!({"symbol": symbol}),
            ),
        ],
        AssetClass::Commodity => vec![mcp(
            "price",
            "derivatives_futures_historical",
            "openbb.platform",
            json!({"symbol": symbol, "start_date": start(120)}),
        )],
        AssetClass::RatesFx if matches!(market.trim().to_uppercase().as_str(), "RATES" | "UST") => {
            vec![mcp(
                "level",
                "fixedincome_government_treasury_rates",
                "openbb.platform",
                json!({"start_date": start(120)}),
            )]
        }
        AssetClass::RatesFx => vec![mcp(
            "level",
            "currency_price_historical",
            "openbb.platform",
            json!({"symbol": symbol, "start_date": start(120)}),
        )],
        AssetClass::RealAsset | AssetClass::Prediction | AssetClass::Private => Vec::new(),
    };
    reads.truncate(MAX_READS);
    reads
}

/// The kinds the page says it checks, in the order the plan reads them.
pub fn checked_kinds(class: AssetClass, market: &str) -> Vec<&'static str> {
    let mut kinds: Vec<&'static str> = reads(
        class,
        "X",
        market,
        NaiveDate::from_ymd_opt(2026, 1, 2).expect("fixed date"),
        false,
    )
    .into_iter()
    .map(|read| read.kind)
    .collect();
    if class == AssetClass::Crypto {
        kinds.push("drawdown");
    }
    // Korean investor flows are read once a day, outside the four reads.
    if class == AssetClass::Equity && is_korean_market(market) {
        kinds.push("flows");
    }
    kinds.dedup();
    kinds
}

/// The fiscal year whose annual report (and dividend) should be filed by
/// `today`: the year before from April, otherwise the one before that.
pub fn dividend_year(today: NaiveDate) -> i32 {
    if (today.month(), today.day()) >= (4, 1) {
        today.year() - 1
    } else {
        today.year() - 2
    }
}

/// Korea Investment's investor flows, short selling and lending balance for
/// a KRX equity (`flows`), once a day after the close.
pub fn flows_reads(symbol: &str, today: NaiveDate) -> [PlannedRead; 3] {
    let symbol = symbol.trim();
    let (from, to) = (
        (today - Duration::days(40)).format("%Y%m%d").to_string(),
        today.format("%Y%m%d").to_string(),
    );
    let kis = |operation: &str, params: Value| PlannedRead {
        kind: "flows",
        tool: "kis_read",
        server: None,
        connection: crate::finance_data::KIS_SOURCE_ID,
        arguments: json!({"operation_id": operation, "params": params}),
    };
    [
        kis(
            "domestic_stock.investor_trade_by_stock_daily",
            json!({"fid_cond_mrkt_div_code": "J", "fid_input_iscd": symbol, "fid_input_date_1": to, "fid_org_adj_prc": "", "fid_etc_cls_code": ""}),
        ),
        kis(
            "domestic_stock.daily_short_sale",
            json!({"fid_cond_mrkt_div_code": "J", "fid_input_iscd": symbol, "fid_input_date_1": from, "fid_input_date_2": to}),
        ),
        kis(
            "domestic_stock.daily_loan_trans",
            json!({"mrkt_div_cls_code": "3", "mksc_shrn_iscd": symbol, "start_date": from, "end_date": to}),
        ),
    ]
}

/// One capital decision named by a 주요사항보고서 the filings read listed.
pub fn capital_read(symbol: &str, kind: &str, filed: NaiveDate) -> PlannedRead {
    PlannedRead {
        kind: "filings",
        tool: "dart_major_report",
        server: None,
        connection: "opendart.disclosures",
        arguments: json!({"company": symbol.trim(), "kind": kind, "start": filed.to_string(), "end": filed.to_string()}),
    }
}

/// What this kind of asset would need that v1 does not read (spec §3.2).
pub fn not_checked_yet(class: AssetClass, market: &str) -> Vec<&'static str> {
    match class {
        AssetClass::Equity if is_korean_market(market) => {
            vec!["creditBalances", "shortSaleBalances", "lockUps"]
        }
        AssetClass::Equity => vec!["estimateRevisions", "insiderTrades", "ownershipChanges"],
        AssetClass::Crypto => vec!["tokenUnlocks", "tvl", "governance", "exchangeFlows"],
        AssetClass::Fund => vec!["fundFlows", "premiumDiscount"],
        AssetClass::Commodity => vec!["positioning", "inventories"],
        AssetClass::RatesFx => vec!["centralBankCalendar", "auctions"],
        AssetClass::RealAsset | AssetClass::Private => vec!["valuationFeeds"],
        AssetClass::Prediction => vec!["marketOdds"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_stay_within_four_reads_and_route_by_market() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let korea = reads(AssetClass::Equity, "990308", "KRX", today, false);
        assert_eq!(
            korea.iter().map(|r| r.tool).collect::<Vec<_>>(),
            [
                "dart_filing_search",
                "dart_financials",
                "krx_daily_quote",
                "dart_dividends"
            ]
        );
        assert_eq!(korea[1].arguments["report_period"], "half_year");
        assert_eq!(korea[3].arguments["year"], 2025);
        assert_eq!(
            dividend_year(NaiveDate::from_ymd_opt(2026, 3, 31).unwrap()),
            2024
        );
        assert_eq!(
            checked_kinds(AssetClass::Equity, "KOSDAQ"),
            ["filings", "results", "price", "dividends", "flows"]
        );
        let flows = flows_reads("990308", today);
        assert!(flows
            .iter()
            .all(|read| read.kind == "flows"
                && read.connection == crate::finance_data::KIS_SOURCE_ID));
        assert_eq!(flows[2].arguments["params"]["end_date"], "20261101");
        assert_eq!(
            not_checked_yet(AssetClass::Equity, "KRX"),
            ["creditBalances", "shortSaleBalances", "lockUps"]
        );
        let first = reads(AssetClass::Equity, "MORR", "NASDAQ", today, true);
        assert_eq!(first.len(), MAX_READS);
        assert_eq!(first[0].tool, "etf_info");
        let later = reads(AssetClass::Equity, "MORR", "NASDAQ", today, false);
        assert_eq!(later.last().unwrap().tool, "equity_fundamental_dividends");
        assert!(reads(AssetClass::Private, "X", "PRIVATE", today, true).is_empty());
        assert_eq!(
            dart_period(NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()),
            (2025, "q3")
        );
        assert_eq!(
            dart_period(NaiveDate::from_ymd_opt(2026, 4, 2).unwrap()),
            (2025, "annual")
        );
        assert_eq!(
            checked_kinds(AssetClass::Crypto, "CRYPTO"),
            ["price", "drawdown"]
        );
        let coin = reads(AssetClass::Crypto, "BTC", "CRYPTO", today, false);
        assert_eq!(coin[0].arguments["symbol"], "BTCUSD");
    }
}
