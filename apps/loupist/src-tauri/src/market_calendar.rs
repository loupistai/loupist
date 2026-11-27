//! Exchange trading days: weekdays minus the holidays an exchange has
//! published. The table is `market-calendar.json` at the app root, shared with
//! the header's market status (`chat/marketStatus.ts`, which also shows early
//! closes) and maintained by hand from each exchange's own calendar (NYSE
//! nyse.com/markets/hours-calendars, Nasdaq nasdaqtrader.com holiday calendar,
//! KRX global.krx.co.kr market holidays; URLs and notes live beside the dates).
//! A year an exchange has not published is a regular year: every weekday trades.
//! `scripts/check-market-calendar.mjs` (web checks and the release workflow)
//! fails when the current year is missing, so that default never ships silently.
use std::collections::BTreeMap;
use std::sync::OnceLock;

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate};
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
    time_zone: String,
    years: Vec<i32>,
    #[allow(dead_code)]
    sources: Vec<String>,
    #[allow(dead_code)]
    note: String,
    closed: BTreeMap<String, String>,
    /// Shortened sessions; only the header's market status shows them.
    #[allow(dead_code)]
    early_close: BTreeMap<String, String>,
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str(include_str!("../../market-calendar.json"))
            .expect("market-calendar.json is valid (checked by its test)")
    })
}

const KRX_MARKETS: &[&str] = &["KRX", "XKRX", "KOSPI", "KOSDAQ", "KONEX", "KS", "KQ"];
const US_MARKETS: &[&str] = &[
    "US", "USA", "NYSE", "NASDAQ", "AMEX", "NYSEARCA", "ARCA", "BATS", "CBOE", "NMS", "NYQ",
    "XNAS", "XNYS",
];

fn calendar_for(market: &str) -> Option<&'static MarketCalendar> {
    let market = market.trim().to_uppercase();
    let id = if KRX_MARKETS.contains(&market.as_str()) {
        "KRX"
    } else if is_us_session_market(&market) {
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

pub(crate) fn is_us_session_market(market: &str) -> bool {
    US_MARKETS.contains(&market)
}

// The prior session is usable before the current daily bar has arrived. On a
// closed day the latest completed session is the only eligible observation.
pub(crate) fn daily_bar_dates(
    market: &str,
    now: DateTime<FixedOffset>,
    cutoff: Option<NaiveDate>,
) -> [NaiveDate; 3] {
    let today = calendar_for(market)
        .and_then(|calendar| calendar.time_zone.parse::<chrono_tz::Tz>().ok())
        .map_or(now.date_naive(), |zone| {
            now.with_timezone(&zone).date_naive()
        });
    let today = cutoff.map_or(today, |cutoff| cutoff.min(today));
    let previous = previous_trading_day(market, today);
    if is_trading_day(market, today) {
        [today, previous, previous_trading_day(market, previous)]
    } else {
        [previous, previous, previous_trading_day(market, previous)]
    }
}

pub(crate) fn check_bar_dates(
    now: DateTime<FixedOffset>,
    cutoff: Option<NaiveDate>,
) -> BTreeMap<&'static str, [NaiveDate; 3]> {
    [("", &[""][..]), ("KRX", KRX_MARKETS), ("US", US_MARKETS)]
        .into_iter()
        .flat_map(|(market, aliases)| {
            let dates = daily_bar_dates(market, now, cutoff);
            aliases.iter().map(move |alias| (*alias, dates))
        })
        .collect()
}

fn previous_trading_day(market: &str, day: NaiveDate) -> NaiveDate {
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
    fn daily_bar_windows_follow_the_exchanges_own_holidays_and_local_dates() {
        let windows = [
            (
                "NASDAQ",
                "2026-11-26T14:00:00Z",
                ["2026-11-25", "2026-11-25", "2026-11-24"],
            ),
            (
                "NASDAQ",
                "2026-11-27T02:00:00Z",
                ["2026-11-25", "2026-11-25", "2026-11-24"],
            ),
            (
                "NASDAQ",
                "2026-11-27T14:00:00Z",
                ["2026-11-27", "2026-11-25", "2026-11-24"],
            ),
            (
                "KOSDAQ",
                "2026-10-08T18:00:00Z",
                ["2026-10-08", "2026-10-08", "2026-10-07"],
            ),
            (
                "KOSDAQ",
                "2026-10-11T23:00:00Z",
                ["2026-10-12", "2026-10-08", "2026-10-07"],
            ),
        ];
        for (market, now, expected) in windows {
            let now = DateTime::parse_from_rfc3339(now).unwrap();
            assert_eq!(daily_bar_dates(market, now, None), expected.map(day));
        }
        let now = DateTime::parse_from_rfc3339("2026-11-30T08:00:00Z").unwrap();
        assert_eq!(
            daily_bar_dates("KRX", now, Some(day("2026-11-26"))),
            ["2026-11-26", "2026-11-25", "2026-11-24"].map(day)
        );
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
