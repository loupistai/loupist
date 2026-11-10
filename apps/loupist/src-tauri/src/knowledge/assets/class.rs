//! Which kind of asset a page is about, and where that answer came from.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AssetClass {
    Equity,
    Crypto,
    Fund,
    Commodity,
    RatesFx,
    RealAsset,
    Prediction,
    Private,
}

impl AssetClass {
    pub const ALL: [AssetClass; 8] = [
        Self::Equity,
        Self::Crypto,
        Self::Fund,
        Self::Commodity,
        Self::RatesFx,
        Self::RealAsset,
        Self::Prediction,
        Self::Private,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Equity => "equity",
            Self::Crypto => "crypto",
            Self::Fund => "fund",
            Self::Commodity => "commodity",
            Self::RatesFx => "rates_fx",
            Self::RealAsset => "real_asset",
            Self::Prediction => "prediction",
            Self::Private => "private",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(value.to_owned())).ok()
    }

    /// Classes with no automatic data source in v1: only Sources and World growth.
    pub fn sources_only(self) -> bool {
        matches!(self, Self::RealAsset | Self::Prediction | Self::Private)
    }
}

/// First match wins: the user's choice, the market code, a first-refresh
/// lookup, the World entity's class, then equity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ClassSource {
    User,
    Market,
    Lookup,
    World,
    Default,
}

impl ClassSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Market => "market",
            Self::Lookup => "lookup",
            Self::World => "world",
            Self::Default => "default",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(value.to_owned())).ok()
    }

    /// Whether a source may replace a class set by `self`.
    pub fn yields_to(self, other: Self) -> bool {
        let rank = |source: Self| match source {
            Self::User => 0,
            Self::Market => 1,
            Self::Lookup => 2,
            Self::World => 3,
            Self::Default => 4,
        };
        rank(other) <= rank(self)
    }
}

/// Crypto venues, as the renderer's market chip knows them (`chat/marketStatus.ts`).
const CRYPTO_MARKETS: &[&str] = &[
    "CRYPTO", "UPBIT", "BITHUMB", "BINANCE", "COINBASE", "KRAKEN", "BYBIT", "OKX",
];
const KOREAN_MARKETS: &[&str] = &["KRX", "KOSPI", "KOSDAQ", "KONEX", "KS", "KQ"];
const CURRENCIES: &[&str] = &[
    "USD", "EUR", "JPY", "GBP", "KRW", "CNY", "CNH", "CHF", "AUD", "CAD", "HKD", "SGD", "NZD",
    "SEK", "NOK", "DKK", "INR", "TWD", "MXN", "BRL", "ZAR",
];

fn upper(value: &str) -> String {
    value.trim().to_uppercase()
}

pub fn is_korean_market(market: &str) -> bool {
    KOREAN_MARKETS.contains(&upper(market).as_str())
}

/// The exchange's time zone (codes as `chat/marketStatus.ts` knows them). A
/// daily crypto bar closes at midnight UTC.
pub fn exchange_time_zone(market: &str) -> Option<chrono_tz::Tz> {
    use chrono_tz::{America, Asia, Australia, Europe, UTC};
    let market = upper(market);
    if CRYPTO_MARKETS.contains(&market.as_str()) {
        return Some(UTC);
    }
    Some(match market.as_str() {
        "KRX" | "KOSPI" | "KOSDAQ" | "KONEX" | "KS" | "KQ" => Asia::Seoul,
        "US" | "NYSE" | "NASDAQ" | "AMEX" | "NYSEARCA" | "ARCA" | "BATS" | "CBOE" | "NMS"
        | "FX" | "FOREX" | "RATES" | "UST" | "NYMEX" | "COMEX" | "ICE" => America::New_York,
        "CME" => America::Chicago,
        "TSE" | "JPX" | "TYO" | "T" => Asia::Tokyo,
        "HKEX" | "HKG" | "HK" | "SEHK" => Asia::Hong_Kong,
        "SSE" | "SHA" | "SS" | "SZSE" | "SHE" | "SZ" => Asia::Shanghai,
        "LSE" | "LON" | "L" => Europe::London,
        "XETRA" | "ETR" | "FRA" | "DE" => Europe::Berlin,
        "EURONEXT" | "EPA" | "AMS" | "PA" | "AS" => Europe::Paris,
        "TSX" | "TO" => America::Toronto,
        "ASX" | "AX" => Australia::Sydney,
        _ => return None,
    })
}

/// The exchange's current day at `now`: a daily close dated before it is a
/// completed session. Unknown markets use the computer's own day.
pub fn session_day(market: &str, now: chrono::DateTime<chrono::Utc>) -> chrono::NaiveDate {
    match exchange_time_zone(market) {
        Some(zone) => now.with_timezone(&zone).date_naive(),
        None => now.with_timezone(&chrono::Local).date_naive(),
    }
}

/// "USDKRW", "EURUSD": two known currency codes back to back.
fn currency_pair(symbol: &str) -> bool {
    let symbol = upper(symbol).replace(['/', '-', '='], "");
    symbol.len() == 6
        && symbol.is_ascii()
        && CURRENCIES.contains(&&symbol[..3])
        && CURRENCIES.contains(&&symbol[3..])
}

/// The class a market code (or an FX pair symbol) decides on its own.
pub fn class_from_market(symbol: &str, market: &str) -> Option<AssetClass> {
    let market = upper(market);
    if CRYPTO_MARKETS.contains(&market.as_str()) {
        return Some(AssetClass::Crypto);
    }
    match market.as_str() {
        "CME" | "COMEX" | "NYMEX" | "ICE" => return Some(AssetClass::Commodity),
        "FX" | "FOREX" | "RATES" | "UST" => return Some(AssetClass::RatesFx),
        "REAL" => return Some(AssetClass::RealAsset),
        "POLYMARKET" | "KALSHI" => return Some(AssetClass::Prediction),
        "PRIVATE" => return Some(AssetClass::Private),
        _ => {}
    }
    currency_pair(symbol).then_some(AssetClass::RatesFx)
}

/// A World Asset's stated class, mapped onto the page classes. Whole words
/// and phrases only: "corporate" is not a rate and "bitcoin miner" is not a coin.
pub fn class_from_world(value: &str) -> Option<AssetClass> {
    let value = value.trim().to_lowercase();
    let words: Vec<&str> = value
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let has = |needles: &[&str]| {
        needles.iter().any(|needle| {
            let parts: Vec<&str> = needle.split(' ').collect();
            words
                .windows(parts.len())
                .any(|window| window == parts.as_slice())
        })
    };
    if has(&[
        "crypto",
        "cryptocurrency",
        "cryptocurrencies",
        "token",
        "tokens",
        "coin",
        "coins",
        "stablecoin",
        "stablecoins",
        "가상자산",
        "암호화폐",
        "코인",
    ]) {
        Some(AssetClass::Crypto)
    } else if has(&[
        "etf", "etfs", "etn", "fund", "funds", "trust", "reit", "reits", "펀드",
    ]) {
        Some(AssetClass::Fund)
    } else if has(&[
        "commodity",
        "commodities",
        "future",
        "futures",
        "metal",
        "metals",
        "energy",
        "agricultural",
        "agriculture",
        "원자재",
        "선물",
    ]) {
        Some(AssetClass::Commodity)
    } else if has(&[
        "currency",
        "currencies",
        "fx",
        "forex",
        "rate",
        "rates",
        "bond",
        "bonds",
        "treasury",
        "treasuries",
        "채권",
        "금리",
        "환율",
        "통화",
    ]) {
        Some(AssetClass::RatesFx)
    } else if has(&[
        "real estate",
        "real asset",
        "real assets",
        "property",
        "properties",
        "land",
        "부동산",
    ]) {
        Some(AssetClass::RealAsset)
    } else if has(&["prediction", "event contract", "event contracts"]) {
        Some(AssetClass::Prediction)
    } else if has(&["private", "비상장"]) {
        Some(AssetClass::Private)
    } else if has(&[
        "equity", "equities", "stock", "stocks", "share", "shares", "주식",
    ]) {
        Some(AssetClass::Equity)
    } else {
        None
    }
}

/// Resolution without a refresh lookup: market code, then the World class, then equity.
pub fn initial_class(
    symbol: &str,
    market: &str,
    world_class: Option<&str>,
) -> (AssetClass, ClassSource) {
    if let Some(class) = class_from_market(symbol, market) {
        return (class, ClassSource::Market);
    }
    if let Some(class) = world_class.and_then(class_from_world) {
        return (class, ClassSource::World);
    }
    (AssetClass::Equity, ClassSource::Default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_world_class_matches_whole_words_only() {
        assert_eq!(class_from_world("Corporate"), None);
        assert_eq!(
            class_from_world("Corporate bond"),
            Some(AssetClass::RatesFx)
        );
        assert_eq!(
            class_from_world("Bitcoin miner equity"),
            Some(AssetClass::Equity)
        );
        assert_eq!(
            class_from_world("Exchange-traded fund"),
            Some(AssetClass::Fund)
        );
        assert_eq!(class_from_world("Generate"), None);
        assert_eq!(
            class_from_world("Landmark stocks"),
            Some(AssetClass::Equity)
        );
        assert_eq!(class_from_world("Real estate"), Some(AssetClass::RealAsset));
        assert_eq!(
            class_from_world("Private equity"),
            Some(AssetClass::Private)
        );
        assert_eq!(
            class_from_world("Fixed-rate notes"),
            Some(AssetClass::RatesFx)
        );
    }
}
