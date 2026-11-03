//! Class watchers for crypto and commodities (finance-crypto-and-templates.md
//! §3.3): which kinds apply to an asset, what each one reads, and detectors
//! over the rows a read returned. Thresholds are the spec's. Every signal is
//! dated by its own row and cites that row; only the latest point can fire,
//! and a level that was already crossed at the point before does not fire again.
use chrono::{Duration, NaiveDate};
use serde_json::{json, Value};

use super::WatchKind;
use crate::knowledge::assets::observe::{self, ToolRead};

/// A coin's close-to-close move worth an alert in a day, and over 7 days.
pub const CRYPTO_DAY_MOVE: f64 = 0.10;
pub const CRYPTO_WEEK_MOVE: f64 = 0.20;
/// Falls below the 90-day high that alert, deepest first.
pub const DRAWDOWN_LEVELS: [f64; 3] = [-0.50, -0.30, -0.20];
const HIGH_DAYS: i64 = 90;
/// A stablecoin closing outside this band is off its $1 peg.
pub const PEG_BAND: (f64, f64) = (0.98, 1.02);
/// Value locked falling this far over 7 days.
pub const TVL_DROP: f64 = -0.20;
/// A front futures contract moving this far over 7 days.
pub const FUTURES_WEEK_MOVE: f64 = 0.05;
/// EIA series: U.S. ending stocks of crude oil excluding the SPR (thousand barrels).
pub const COMMERCIAL_CRUDE: &str = "WCESTUS1";

/// USD stablecoins the peg watcher applies to.
const STABLECOINS: &[&str] = &[
    "USDT", "USDC", "DAI", "USDE", "USDS", "FDUSD", "PYUSD", "TUSD", "USDP", "FRAX", "GUSD",
    "USD1", "RLUSD",
];

/// DefiLlama chain names for chain tokens (`route: chain`), checked against
/// api.llama.fi/v2/historicalChainTvl on 2026-11-01.
const LLAMA_CHAINS: &[(&str, &str)] = &[
    ("ETH", "Ethereum"),
    ("SOL", "Solana"),
    ("BNB", "BSC"),
    ("TRX", "Tron"),
    ("AVAX", "Avalanche"),
    ("ARB", "Arbitrum"),
    ("OP", "OP Mainnet"),
    ("POL", "Polygon"),
    ("MATIC", "Polygon"),
    ("SUI", "Sui"),
    ("APT", "Aptos"),
    ("ADA", "Cardano"),
    ("NEAR", "Near"),
    ("TON", "TON"),
    ("SEI", "Sei"),
];

/// DefiLlama protocol slugs for protocol tokens (`route: protocol`), checked
/// against api.llama.fi/protocol on 2026-11-01.
const LLAMA_PROTOCOLS: &[(&str, &str)] = &[
    ("AAVE", "aave"),
    ("UNI", "uniswap"),
    ("LDO", "lido"),
    ("SKY", "sky"),
    ("MKR", "sky"),
    ("CRV", "curve-finance"),
    ("COMP", "compound-finance"),
    ("PENDLE", "pendle"),
    ("ENA", "ethena"),
    ("EIGEN", "eigenlayer"),
    ("GMX", "gmx"),
    ("MORPHO", "morpho"),
    ("RAY", "raydium"),
    ("JUP", "jupiter"),
    ("HYPE", "hyperliquid"),
    ("JTO", "jito"),
];

/// CFTC contract market codes by futures root, checked against
/// publicreporting.cftc.gov (disaggregated report, 2026-10-20).
const CFTC_CODES: &[(&str, &str)] = &[
    ("GC", "088691"),
    ("SI", "084691"),
    ("HG", "085692"),
    ("PL", "076651"),
    ("PA", "075651"),
    ("CL", "067651"),
    ("NG", "023651"),
    ("HO", "022651"),
    ("RB", "111659"),
    ("ZC", "002602"),
    ("ZS", "005602"),
    ("ZW", "001602"),
    ("KC", "083731"),
    ("SB", "080732"),
    ("CT", "033661"),
    ("CC", "073732"),
    ("LE", "057642"),
];

/// Crude futures the EIA weekly petroleum report is about.
const OIL: &[&str] = &["CL", "BZ", "MCL"];

/// "BTC" from "BTC", "BTCUSD", "BTC-USD" or "btc/usdt".
pub fn crypto_base(symbol: &str) -> String {
    let symbol = symbol.trim().to_uppercase();
    if let Some((base, _)) = symbol.split_once(['-', '/']) {
        return base.to_owned();
    }
    for quote in ["USDT", "USDC", "USD", "KRW", "EUR"] {
        if let Some(base) = symbol.strip_suffix(quote) {
            if base.len() >= 2 {
                return base.to_owned();
            }
        }
    }
    symbol
}

/// The pair a price read asks for: the coin against the US dollar.
pub fn crypto_pair(symbol: &str) -> String {
    format!("{}USD", crypto_base(symbol))
}

/// "CL" from "CL", "CL=F", "CLZ26" or "clz2026".
pub fn futures_root(symbol: &str) -> String {
    let symbol = symbol.trim().to_uppercase();
    let symbol = symbol.trim_end_matches("=F");
    let letters = symbol.trim_end_matches(|c: char| c.is_ascii_digit());
    if letters.len() < symbol.len() && letters.len() >= 3 {
        // A dated contract: the letter before the year is its month code.
        return letters[..letters.len() - 1].to_owned();
    }
    letters.to_owned()
}

pub fn is_stablecoin(symbol: &str) -> bool {
    STABLECOINS.contains(&crypto_base(symbol).as_str())
}

/// Where DefiLlama keeps a coin's value locked: `("chain" | "protocol", id)`.
pub fn llama_target(symbol: &str) -> Option<(&'static str, &'static str)> {
    let base = crypto_base(symbol);
    LLAMA_CHAINS
        .iter()
        .find(|(token, _)| *token == base)
        .map(|(_, chain)| ("chain", *chain))
        .or_else(|| {
            LLAMA_PROTOCOLS
                .iter()
                .find(|(token, _)| *token == base)
                .map(|(_, slug)| ("protocol", *slug))
        })
}

pub fn cftc_code(symbol: &str) -> Option<&'static str> {
    let root = futures_root(symbol);
    CFTC_CODES
        .iter()
        .find(|(known, _)| *known == root)
        .map(|(_, code)| *code)
}

pub fn is_oil(symbol: &str) -> bool {
    OIL.contains(&futures_root(symbol).as_str())
}

/// The class kinds that apply to a coin: a peg for stablecoins; moves and the
/// fall from the high for the rest, and value locked when DefiLlama has it.
pub fn crypto_kinds(symbol: &str) -> Vec<WatchKind> {
    if is_stablecoin(symbol) {
        return vec![WatchKind::Depeg];
    }
    let mut kinds = vec![WatchKind::CryptoMove, WatchKind::Drawdown];
    if llama_target(symbol).is_some() {
        kinds.push(WatchKind::Tvl);
    }
    kinds
}

/// The class kinds that apply to a futures market.
pub fn commodity_kinds(symbol: &str) -> Vec<WatchKind> {
    let mut kinds = vec![WatchKind::FuturesMove];
    if cftc_code(symbol).is_some() {
        kinds.push(WatchKind::Positioning);
    }
    if is_oil(symbol) {
        kinds.push(WatchKind::OilInventory);
    }
    kinds
}

/// What a read says for one kind: the row's date, a natural key, sentence
/// facts and the row it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Signal {
    pub kind: WatchKind,
    pub date: NaiveDate,
    pub key: String,
    pub facts: Value,
    pub pointer: String,
    pub url: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct Point {
    date: NaiveDate,
    value: f64,
    pointer: String,
}

fn round(value: f64, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    (value * scale).round() / scale
}

fn sorted(mut points: Vec<Point>) -> Vec<Point> {
    points.sort_by_key(|point| point.date);
    points.dedup_by_key(|point| point.date);
    points
}

/// Completed daily closes (dated before the exchange's current day).
fn closes(read: &ToolRead<'_>) -> Vec<Point> {
    let Some((rows, prefix)) = observe::rows(read.payload, "/data/bars") else {
        return Vec::new();
    };
    sorted(
        rows.iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let date = observe::day(row, &["date"])?;
                let value = observe::number(row, &["close"])?;
                (date < read.session_day && value > 0.0).then(|| Point {
                    date,
                    value,
                    pointer: format!("{prefix}/{index}"),
                })
            })
            .collect(),
    )
}

/// The latest point on or before `days` before point `index`.
fn back(points: &[Point], index: usize, days: i64) -> Option<&Point> {
    let cutoff = points[index].date - Duration::days(days);
    points[..index]
        .iter()
        .rev()
        .find(|point| point.date <= cutoff)
}

/// The change over 7 days to point `index`, and where it started.
fn week(points: &[Point], index: usize) -> Option<(f64, &Point)> {
    let start = back(points, index, 7)?;
    Some((points[index].value / start.value - 1.0, start))
}

/// A 7-day move past `threshold` at the latest point that was not already past
/// it, in the same direction, at the point before.
fn week_move(points: &[Point], threshold: f64) -> Option<(f64, &Point)> {
    let last = points.len().checked_sub(1)?;
    let (change, start) = week(points, last).filter(|(change, _)| change.abs() >= threshold)?;
    let continuing = last
        .checked_sub(1)
        .and_then(|before| week(points, before))
        .is_some_and(|(before, _)| before.abs() >= threshold && before.signum() == change.signum());
    (!continuing).then_some((change, start))
}

/// The deepest level the close at `index` sits under, against the highest
/// close of the 90 days to it.
fn drawdown_at(points: &[Point], index: usize) -> Option<(f64, f64, &Point)> {
    let from = points[index].date - Duration::days(HIGH_DAYS);
    let high = points[..=index]
        .iter()
        .filter(|point| point.date > from)
        .max_by(|a, b| a.value.total_cmp(&b.value))?;
    let drawdown = points[index].value / high.value - 1.0;
    DRAWDOWN_LEVELS
        .into_iter()
        .find(|level| drawdown <= *level)
        .map(|level| (level, drawdown, high))
}

/// Price kinds over a coin's or a futures market's daily closes.
pub fn price_signals(read: &ToolRead<'_>, kinds: &[WatchKind]) -> Vec<Signal> {
    let points = closes(read);
    let Some(last_index) = points.len().checked_sub(1).filter(|index| *index >= 1) else {
        return Vec::new();
    };
    let (last, prev) = (&points[last_index], &points[last_index - 1]);
    let mut out = Vec::new();
    let signal = |kind: WatchKind, key: String, facts: Value| Signal {
        kind,
        date: last.date,
        key,
        facts,
        pointer: last.pointer.clone(),
        url: None,
    };
    if kinds.contains(&WatchKind::CryptoMove) {
        let day = (last.date - prev.date == Duration::days(1))
            .then(|| last.value / prev.value - 1.0)
            .filter(|change| change.abs() >= CRYPTO_DAY_MOVE);
        let moved = match day {
            Some(change) => Some(("day", change, prev)),
            None => {
                week_move(&points, CRYPTO_WEEK_MOVE).map(|(change, start)| ("week", change, start))
            }
        };
        if let Some((window, change, start)) = moved {
            out.push(signal(
                WatchKind::CryptoMove,
                format!("move:{}", last.date),
                json!({"window": window, "change": round(change, 4), "from": start.date.to_string(), "to": last.date.to_string(), "close": last.value}),
            ));
        }
    }
    if kinds.contains(&WatchKind::FuturesMove) {
        if let Some((change, start)) = week_move(&points, FUTURES_WEEK_MOVE) {
            out.push(signal(
                WatchKind::FuturesMove,
                format!("move:{}", last.date),
                json!({"window": "week", "change": round(change, 4), "from": start.date.to_string(), "to": last.date.to_string(), "close": last.value}),
            ));
        }
    }
    if kinds.contains(&WatchKind::Drawdown) {
        if let Some((level, drawdown, high)) = drawdown_at(&points, last_index) {
            let before = drawdown_at(&points, last_index - 1).map(|(level, ..)| level);
            if before.is_none_or(|before| level < before) {
                out.push(signal(
                    WatchKind::Drawdown,
                    format!("drawdown:{}:{}", (level * 100.0).round() as i64, last.date),
                    json!({"level": level, "drawdown": round(drawdown, 4), "high": high.value, "highDate": high.date.to_string(), "close": last.value, "to": last.date.to_string()}),
                ));
            }
        }
    }
    if kinds.contains(&WatchKind::Depeg) {
        let off = |value: f64| value < PEG_BAND.0 || value > PEG_BAND.1;
        if off(last.value) && !off(prev.value) {
            out.push(signal(
                WatchKind::Depeg,
                format!("depeg:{}", last.date),
                json!({"close": last.value, "to": last.date.to_string()}),
            ));
        }
    }
    out
}

/// Value locked down 20% or more over 7 days to the latest complete day.
pub fn tvl_signal(read: &ToolRead<'_>) -> Option<Signal> {
    let (rows, prefix) = observe::rows(read.payload, "/data/rows")?;
    let points = sorted(
        rows.iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let date = observe::day(row, &["date"])?;
                let value = observe::number(row, &["value"])?;
                // Today's value is still moving; a day is known at its end.
                (date < read.session_day && value > 0.0).then(|| Point {
                    date,
                    value,
                    pointer: format!("{prefix}/{index}"),
                })
            })
            .collect(),
    );
    let last = points.len().checked_sub(1)?;
    let (change, start) = week(&points, last).filter(|(change, _)| *change <= TVL_DROP)?;
    let already = last
        .checked_sub(1)
        .and_then(|before| week(&points, before))
        .is_some_and(|(before, _)| before <= TVL_DROP);
    if already {
        return None;
    }
    let text = |pointer: &str| read.payload.pointer(pointer).and_then(Value::as_str);
    let target = text("/data/series/name")
        .or_else(|| text("/data/series/id"))
        .unwrap_or_default();
    let (route, id) = (text("/query/route"), text("/query/id"));
    let url = match (route, id) {
        (Some("chain"), Some(id)) => Some(format!(
            "https://defillama.com/chain/{}",
            id.replace(' ', "%20")
        )),
        (Some("protocol"), Some(id)) => Some(format!("https://defillama.com/protocol/{id}")),
        _ => None,
    };
    let point = &points[last];
    Some(Signal {
        kind: WatchKind::Tvl,
        date: point.date,
        key: format!("tvl:{}", point.date),
        facts: json!({"change": round(change, 4), "from": start.date.to_string(), "to": point.date.to_string(), "value": point.value, "target": target}),
        pointer: point.pointer.clone(),
        url,
    })
}

/// CFTC managed money's net position (long − short) changing sign between the
/// two latest weekly reports.
pub fn positioning_signal(read: &ToolRead<'_>) -> Option<Signal> {
    let (rows, prefix) = observe::rows(read.payload, "")?;
    let mut reports: Vec<(NaiveDate, f64, String, usize)> = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let date = observe::day(row, &["date", "report_date_as_yyyy_mm_dd"])?;
            let long = observe::number(
                row,
                &[
                    "managed_money_positions_long_all",
                    "m_money_positions_long_all",
                ],
            )?;
            let short = observe::number(
                row,
                &[
                    "managed_money_positions_short_all",
                    "m_money_positions_short_all",
                ],
            )?;
            (date <= read.today).then(|| (date, long - short, format!("{prefix}/{index}"), index))
        })
        .collect();
    reports.sort_by_key(|(date, ..)| *date);
    reports.dedup_by_key(|(date, ..)| *date);
    let [.., (_, before, ..), (date, net, pointer, index)] = reports.as_slice() else {
        return None;
    };
    if *net == 0.0 || *before == 0.0 || net.signum() == before.signum() {
        return None;
    }
    let market = rows[*index]
        .get("market_and_exchange_names")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(Signal {
        kind: WatchKind::Positioning,
        date: *date,
        key: format!("positioning:{date}"),
        facts: json!({"direction": if *net > 0.0 { "long" } else { "short" }, "net": net, "before": before, "to": date.to_string(), "market": market}),
        pointer: pointer.clone(),
        url: Some("https://www.cftc.gov/MarketReports/CommitmentsofTraders/index.htm".into()),
    })
}

/// The latest EIA weekly report: its week and, when the report has it, the
/// change in U.S. commercial crude stocks from the week before.
pub fn inventory_signal(read: &ToolRead<'_>) -> Option<Signal> {
    let (rows, prefix) = observe::rows(read.payload, "")?;
    let dated = |row: &Value| observe::day(row, &["date"]).filter(|date| *date <= read.today);
    let crude = sorted(
        rows.iter()
            .enumerate()
            .filter(|(_, row)| row.get("symbol").and_then(Value::as_str) == Some(COMMERCIAL_CRUDE))
            .filter_map(|(index, row)| {
                Some(Point {
                    date: dated(row)?,
                    value: observe::number(row, &["value"])?,
                    pointer: format!("{prefix}/{index}"),
                })
            })
            .collect(),
    );
    let url = Some("https://www.eia.gov/petroleum/supply/weekly/".to_owned());
    if let [.., before, last] = crude.as_slice() {
        return Some(Signal {
            kind: WatchKind::OilInventory,
            date: last.date,
            key: format!("inventory:{}", last.date),
            facts: json!({"to": last.date.to_string(), "stocks": last.value, "change": last.value - before.value, "unit": "thousand barrels"}),
            pointer: last.pointer.clone(),
            url,
        });
    }
    let (index, date) = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| Some((index, dated(row)?)))
        .max_by_key(|(_, date)| *date)?;
    Some(Signal {
        kind: WatchKind::OilInventory,
        date,
        key: format!("inventory:{date}"),
        facts: json!({"to": date.to_string()}),
        pointer: format!("{prefix}/{index}"),
        url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    fn read<'a>(tool: &'a str, payload: &'a Value, session_day: &str) -> ToolRead<'a> {
        ToolRead {
            tool,
            provider: None,
            payload,
            dataset: None,
            retrieved_at: None,
            citation_url: None,
            today: day(session_day),
            session_day: day(session_day),
        }
    }

    /// Daily closes from `start`, one per day.
    fn bars(start: &str, closes: &[f64]) -> Value {
        let first = day(start);
        json!({"structuredContent": {"results": closes.iter().enumerate().map(|(index, close)| json!({"date": (first + Duration::days(index as i64)).to_string(), "close": close})).collect::<Vec<_>>()}})
    }

    fn kinds(signals: &[Signal]) -> Vec<WatchKind> {
        signals.iter().map(|signal| signal.kind).collect()
    }

    #[test]
    fn coins_and_futures_map_to_their_sources() {
        assert_eq!(crypto_base("btc-usd"), "BTC");
        assert_eq!(crypto_base("ETHUSDT"), "ETH");
        assert_eq!(crypto_base("USDC"), "USDC");
        assert_eq!(crypto_pair("SOL"), "SOLUSD");
        assert_eq!(futures_root("CLZ26"), "CL");
        assert_eq!(futures_root("GC=F"), "GC");
        assert_eq!(futures_root("ZC"), "ZC");
        assert_eq!(crypto_kinds("USDC"), [WatchKind::Depeg]);
        assert_eq!(
            crypto_kinds("ETH"),
            [WatchKind::CryptoMove, WatchKind::Drawdown, WatchKind::Tvl]
        );
        assert_eq!(
            crypto_kinds("BTC"),
            [WatchKind::CryptoMove, WatchKind::Drawdown]
        );
        assert_eq!(llama_target("OP"), Some(("chain", "OP Mainnet")));
        assert_eq!(llama_target("AAVE"), Some(("protocol", "aave")));
        assert_eq!(
            commodity_kinds("CL"),
            [
                WatchKind::FuturesMove,
                WatchKind::Positioning,
                WatchKind::OilInventory
            ]
        );
        assert_eq!(cftc_code("GC"), Some("088691"));
        assert_eq!(commodity_kinds("XYZ"), [WatchKind::FuturesMove]);
    }

    #[test]
    fn a_coin_fires_on_a_big_day_or_week_and_a_fall_from_its_high_once() {
        let all = [WatchKind::CryptoMove, WatchKind::Drawdown];
        // A 12% fall on the last complete day (today's partial bar is ignored).
        let mut series = vec![100.0; 10];
        series.extend([88.0, 50.0]);
        let payload = bars("2026-10-20", &series);
        let signals = price_signals(
            &read("crypto_price_historical", &payload, "2026-10-31"),
            &all,
        );
        assert_eq!(kinds(&signals), [WatchKind::CryptoMove]);
        assert_eq!(signals[0].date, day("2026-10-30"));
        assert_eq!(signals[0].facts["window"], "day");
        assert_eq!(signals[0].facts["change"], -0.12);
        assert_eq!(signals[0].pointer, "/structuredContent/results/10");
        // 9% a day is not enough; 7 days of small steps up 21% are, once.
        let steps = [
            100.0, 100.0, 103.0, 106.0, 109.0, 112.0, 115.0, 118.0, 121.0,
        ];
        let payload = bars("2026-10-21", &steps);
        let signals = price_signals(
            &read("crypto_price_historical", &payload, "2026-10-30"),
            &all,
        );
        assert_eq!(kinds(&signals), [WatchKind::CryptoMove]);
        assert_eq!(signals[0].facts["window"], "week");
        let mut longer = steps.to_vec();
        longer.push(125.0);
        let payload = bars("2026-10-21", &longer);
        assert!(price_signals(
            &read("crypto_price_historical", &payload, "2026-10-31"),
            &all
        )
        .is_empty());
        // Down 31% from the 90-day high: one alert at −30%, none the day after at −32%.
        let mut fall = vec![100.0; 20];
        fall.extend([75.0, 72.0, 69.0]);
        let payload = bars("2026-10-01", &fall);
        let signals = price_signals(
            &read("crypto_price_historical", &payload, "2026-10-24"),
            &all,
        );
        assert_eq!(kinds(&signals), [WatchKind::Drawdown]);
        assert_eq!(signals[0].facts["level"], -0.3);
        assert_eq!(signals[0].key, "drawdown:-30:2026-10-23");
        fall.push(68.0);
        let payload = bars("2026-10-01", &fall);
        assert!(price_signals(
            &read("crypto_price_historical", &payload, "2026-10-25"),
            &all
        )
        .is_empty());
    }

    #[test]
    fn a_stablecoin_fires_when_it_leaves_the_band_and_futures_on_a_weekly_move() {
        let payload = bars("2026-10-28", &[1.0, 0.999, 0.973]);
        let signals = price_signals(
            &read("crypto_price_historical", &payload, "2026-10-31"),
            &[WatchKind::Depeg],
        );
        assert_eq!(kinds(&signals), [WatchKind::Depeg]);
        assert_eq!(signals[0].facts["close"], 0.973);
        let payload = bars("2026-10-28", &[1.0, 0.97, 0.973]);
        assert!(price_signals(
            &read("crypto_price_historical", &payload, "2026-10-31"),
            &[WatchKind::Depeg]
        )
        .is_empty());
        // Futures: up 6% over a week fires; 4% does not.
        let payload = bars(
            "2026-10-20",
            &[80.0, 80.0, 81.0, 81.0, 82.0, 83.0, 83.5, 83.9, 84.8],
        );
        let signals = price_signals(
            &read("derivatives_futures_historical", &payload, "2026-10-29"),
            &[WatchKind::FuturesMove],
        );
        assert_eq!(kinds(&signals), [WatchKind::FuturesMove]);
        assert_eq!(signals[0].facts["change"], 0.06);
        let payload = bars(
            "2026-10-20",
            &[80.0, 80.0, 81.0, 81.0, 82.0, 83.0, 83.0, 83.0, 83.2],
        );
        assert!(price_signals(
            &read("derivatives_futures_historical", &payload, "2026-10-29"),
            &[WatchKind::FuturesMove]
        )
        .is_empty());
    }

    #[test]
    fn value_locked_positioning_and_inventory_read_their_own_rows() {
        let rows = |values: &[f64]| json!({"query": {"route": "chain", "id": "Ethereum"}, "data": {"series": {"id": "Ethereum", "name": null}, "rows": values.iter().enumerate().map(|(index, value)| json!({"date": (day("2026-10-20") + Duration::days(index as i64)).to_string(), "value": value.to_string()})).collect::<Vec<_>>()}});
        let payload = rows(&[100.0, 100.0, 98.0, 95.0, 90.0, 85.0, 82.0, 81.0, 78.0, 70.0]);
        // The row dated today (Oct 29, 70) is still moving; Oct 28 is 22% below Oct 21.
        let signal = tvl_signal(&read("defillama_read", &payload, "2026-10-29")).unwrap();
        assert_eq!(signal.date, day("2026-10-28"));
        assert_eq!(signal.facts["change"], -0.22);
        assert_eq!(signal.facts["target"], "Ethereum");
        assert_eq!(
            signal.url.as_deref(),
            Some("https://defillama.com/chain/Ethereum")
        );
        // Already past −20% the day before: nothing new.
        assert!(tvl_signal(&read("defillama_read", &payload, "2026-10-30")).is_none());

        let cot = |nets: &[(f64, f64)]| json!({"structuredContent": {"results": nets.iter().enumerate().map(|(index, (long, short))| json!({"date": (day("2026-10-06") + Duration::weeks(index as i64)).to_string(), "market_and_exchange_names": "GOLD - COMMODITY EXCHANGE INC.", "managed_money_positions_long_all": long, "managed_money_positions_short_all": short})).collect::<Vec<_>>()}});
        let payload = cot(&[
            (120_000.0, 90_000.0),
            (100_000.0, 95_000.0),
            (90_000.0, 101_000.0),
        ]);
        let signal = positioning_signal(&read("cftc_cot", &payload, "2026-10-31")).unwrap();
        assert_eq!(signal.date, day("2026-10-20"));
        assert_eq!(signal.facts["direction"], "short");
        assert_eq!(signal.facts["net"], -11_000.0);
        let payload = cot(&[(120_000.0, 90_000.0), (100_000.0, 95_000.0)]);
        assert!(positioning_signal(&read("cftc_cot", &payload, "2026-10-31")).is_none());

        let payload = json!({"structuredContent": {"results": [
            {"date": "2026-10-16", "symbol": "WCESTUS1", "value": 421_300.0, "unit": "Thousand Barrels"},
            {"date": "2026-10-16", "symbol": "WCSSTUS1", "value": 405_000.0},
            {"date": "2026-10-23", "symbol": "WCESTUS1", "value": 419_200.0, "unit": "Thousand Barrels"}
        ]}});
        let signal = inventory_signal(&read(
            "commodity_petroleum_status_report",
            &payload,
            "2026-10-29",
        ))
        .unwrap();
        assert_eq!(signal.key, "inventory:2026-10-23");
        assert_eq!(signal.facts["change"], -2_100.0);
        assert_eq!(signal.pointer, "/structuredContent/results/2");
    }
}
