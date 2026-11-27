//! A followed asset on an exchange `market-calendar.json` does not list trades
//! every weekday, and its bars stop counting as fresh as the days pass.
use chrono::{Datelike, Duration, FixedOffset, NaiveDate, TimeZone};

use crate::market_calendar::{check_bar_dates, daily_bar_dates, is_trading_day};

/// Exchanges the calendar does not list, from several regions.
const UNLISTED: [&str; 6] = ["XETR", "XTKS", "BVMF", "XJSE", "XBOM", "XLIS"];

#[test]
fn an_unlisted_exchange_falls_back_to_weekdays_and_its_bars_age_out() {
    // (user's UTC offset in hours, first day) across two weeks each.
    let starts = [(9, "2027-03-01"), (-3, "2027-06-14"), (1, "2027-12-20")];
    for market in UNLISTED {
        for (offset, start) in starts {
            let zone = FixedOffset::east_opt(offset * 3600).unwrap();
            let start = NaiveDate::parse_from_str(start, "%Y-%m-%d").unwrap();
            let mut fresh_days = std::collections::BTreeMap::<NaiveDate, u32>::new();
            for step in 0..14 {
                let today = start + Duration::days(step);
                let now = zone
                    .from_local_datetime(&today.and_hms_opt(12, 0, 0).unwrap())
                    .unwrap();
                assert_eq!(
                    is_trading_day(market, today),
                    today.weekday().number_from_monday() <= 5,
                    "{market} {today}"
                );
                let [latest, previous, prior] = daily_bar_dates(market, now, None);
                assert!(latest <= today, "{market} {today}: {latest} is not past");
                assert!(prior < previous && previous < latest || latest == previous);
                for day in [latest, previous, prior] {
                    assert!(day.weekday().number_from_monday() <= 5, "{market} {day}");
                }
                // An unlisted market reads the user's own date, as `""` does,
                // which the Python ledger uses for any market it has no entry for.
                assert_eq!(daily_bar_dates("", now, None), [latest, previous, prior]);
                assert!(!check_bar_dates(now, None).contains_key(market));
                for day in std::collections::BTreeSet::from([latest, previous]) {
                    *fresh_days.entry(day).or_default() += 1;
                }
            }
            for (day, count) in fresh_days {
                // A Friday stays fresh across the weekend and the Monday: four days.
                assert!(
                    count <= 4,
                    "{market}: a bar from {day} stayed fresh {count} days"
                );
            }
        }
    }
}
