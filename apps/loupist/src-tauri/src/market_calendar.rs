//! Exchange trading days: weekdays minus the holidays an exchange has
//! published. The table is `market-calendar.json` at the app root, shared with
//! the header's market status (`chat/marketStatus.ts`, which also shows early
//! closes) and maintained by hand from each exchange's own calendar (NYSE
//! nyse.com/markets/hours-calendars, Nasdaq nasdaqtrader.com holiday calendar,
//! KRX global.krx.co.kr market holidays; URLs and notes live beside the dates).
//! A year an exchange has not published is a regular year: every weekday trades.
use std::collections::BTreeMap;
use std::sync::OnceLock;

use chrono::{Datelike, NaiveDate};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Table {
    #[allow(dead_code)]
    about: String,
    markets: BTreeMap<String, MarketCalendar>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MarketCalendar {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    time_zone: String,
    years: Vec<i32>,
    #[cfg_attr(not(test), allow(dead_code))]
    sources: Vec<String>,
    #[allow(dead_code)]
    note: String,
    closed: BTreeMap<String, String>,
    /// Shortened sessions; only the header's market status shows them.
    #[cfg_attr(not(test), allow(dead_code))]
    early_close: BTreeMap<String, String>,
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str(include_str!("../../market-calendar.json"))
            .expect("market-calendar.json is valid (checked by its test)")
    })
}

/// The calendar a market code uses (`KOSDAQ` → KRX, `NASDAQ` → US), if any.
fn calendar_for(market: &str) -> Option<&'static MarketCalendar> {
    let market = market.trim().to_uppercase();
    let id = if crate::knowledge::assets::class::is_korean_market(&market) {
        "KRX"
    } else if matches!(
        market.as_str(),
        "US" | "NYSE" | "NASDAQ" | "AMEX" | "NYSEARCA" | "ARCA" | "BATS" | "CBOE" | "NMS"
    ) {
        "US"
    } else {
        return None;
    };
    table().markets.get(id)
}

/// Whether `market` trades on `day`: a weekday that is not a published holiday.
/// Markets without a calendar, and years not yet published, trade every weekday.
pub fn is_trading_day(market: &str, day: NaiveDate) -> bool {
    if day.weekday().number_from_monday() > 5 {
        return false;
    }
    calendar_for(market)
        .filter(|calendar| calendar.years.contains(&day.year()))
        .is_none_or(|calendar| !calendar.closed.contains_key(&day.to_string()))
}

/// The last trading day strictly before `day`.
pub fn previous_trading_day(market: &str, day: NaiveDate) -> NaiveDate {
    let mut day = day.pred_opt().expect("a day before");
    while !is_trading_day(market, day) {
        day = day.pred_opt().expect("a day before");
    }
    day
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn the_table_lists_only_weekdays_inside_published_years() {
        let table = table();
        assert!(table.markets.contains_key("US") && table.markets.contains_key("KRX"));
        for (id, calendar) in &table.markets {
            assert!(!calendar.sources.is_empty(), "{id} cites its source");
            assert!(calendar
                .sources
                .iter()
                .all(|url| url.starts_with("https://")));
            for text in calendar.closed.keys().chain(calendar.early_close.keys()) {
                let date = day(text);
                assert_eq!(&date.to_string(), text, "{id} writes dates as YYYY-MM-DD");
                assert!(calendar.years.contains(&date.year()), "{id} {date}");
                assert!(date.weekday().number_from_monday() <= 5, "{id} {date}");
            }
            for (date, time) in &calendar.early_close {
                assert!(!calendar.closed.contains_key(date), "{id} {date}");
                assert!(NaiveTime::parse_from_str(time, "%H:%M").is_ok());
            }
        }
    }

    #[test]
    fn trading_days_skip_published_holidays_and_nothing_else() {
        // A US holiday (Thanksgiving), a KRX holiday (Hangeul Day) and a weekend.
        assert!(!is_trading_day("NASDAQ", day("2026-11-26")));
        assert!(!is_trading_day("NYSE", day("2026-11-26")));
        assert!(!is_trading_day("KOSPI", day("2026-10-09")));
        assert!(!is_trading_day("KRX", day("2026-10-10")));
        // Each calendar is its own: Thanksgiving trades in Seoul.
        assert!(is_trading_day("KRX", day("2026-11-26")));
        // A normal day, and a market without a calendar on Tokyo's Culture Day.
        assert!(is_trading_day("KRX", day("2026-11-02")));
        assert!(is_trading_day("TSE", day("2026-11-03")));
        // Outside the published years nothing is a holiday: KRX has not published
        // 2027 (Seollal is Feb 8), NYSE has not published 2028.
        assert!(is_trading_day("KRX", day("2027-02-08")));
        assert!(is_trading_day("NYSE", day("2028-01-17")));
        // A half day still trades.
        assert!(is_trading_day("NYSE", day("2026-11-27")));
        // Monday after Hangeul Day's long weekend.
        assert_eq!(
            previous_trading_day("KRX", day("2026-10-12")),
            day("2026-10-08")
        );
    }
}
