//! Results dates (finance-earnings-snapshot.md §3.1): period labels, the Korean
//! legal deadlines, the listing time zone captures run in, and which filings
//! close the chain.
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};

/// The last calendar quarter end strictly before `date`.
pub fn quarter_end_before(date: NaiveDate) -> NaiveDate {
    let mut year = date.year();
    let ends = |year: i32| {
        [
            NaiveDate::from_ymd_opt(year, 12, 31),
            NaiveDate::from_ymd_opt(year, 9, 30),
            NaiveDate::from_ymd_opt(year, 6, 30),
            NaiveDate::from_ymd_opt(year, 3, 31),
        ]
    };
    loop {
        for end in ends(year).into_iter().flatten() {
            if end < date {
                return end;
            }
        }
        year -= 1;
    }
}

/// The calendar quarter end on or after `date`.
pub fn quarter_end_on_or_after(date: NaiveDate) -> NaiveDate {
    let month = (date.month() - 1) / 3 * 3 + 3;
    let day = if matches!(month, 6 | 9) { 30 } else { 31 };
    NaiveDate::from_ymd_opt(date.year(), month, day).expect("a quarter's last day")
}

/// "Q3 2026" for the quarter that ends on `end`.
pub fn period_label(end: NaiveDate) -> String {
    format!("Q{} {}", (end.month() - 1) / 3 + 1, end.year())
}

/// The period end a "Q3 2026" label names (calendar quarters).
pub fn label_end(label: &str) -> Option<NaiveDate> {
    let (quarter, year) = label.trim().split_once(' ')?;
    let quarter: u32 = quarter.strip_prefix('Q')?.parse().ok()?;
    let year: i32 = year.parse().ok()?;
    let (month, day) = match quarter {
        1 => (3, 31),
        2 => (6, 30),
        3 => (9, 30),
        4 => (12, 31),
        _ => return None,
    };
    NaiveDate::from_ymd_opt(year, month, day)
}

pub fn is_year_end(end: NaiveDate) -> bool {
    end.month() == 12
}

/// The Korean legal deadline for a period: 45 days after a quarter or half,
/// 90 days after the fiscal year.
pub fn korea_deadline(end: NaiveDate) -> NaiveDate {
    end + Duration::days(if is_year_end(end) { 90 } else { 45 })
}

/// The period whose Korean deadline is next on or after `today`.
pub fn korea_next_period(today: NaiveDate) -> NaiveDate {
    let mut end = quarter_end_before(today);
    // Walk back while an earlier period's deadline is still ahead (the annual
    // report's 90 days can outlast the next quarter's end).
    loop {
        let earlier = quarter_end_before(end);
        if korea_deadline(earlier) >= today {
            end = earlier;
        } else {
            break;
        }
    }
    while korea_deadline(end) < today {
        end = quarter_end_on_or_after(end + Duration::days(1));
    }
    end
}

/// The listing's time zone; New York for US and unknown listings.
pub fn listing_zone(market: &str) -> chrono_tz::Tz {
    crate::knowledge::assets::class::exchange_time_zone(market)
        .unwrap_or(chrono_tz::America::New_York)
}

/// 18:00 on `day` in the listing's time zone.
pub fn evening(day: NaiveDate, market: &str) -> DateTime<Utc> {
    let zone = listing_zone(market);
    let local = day.and_hms_opt(18, 0, 0).expect("18:00 exists");
    zone.from_local_datetime(&local)
        .earliest()
        .map(|time| time.with_timezone(&Utc))
        .unwrap_or_else(|| Utc.from_utc_datetime(&local))
}

/// The listing's current day.
pub fn listing_today(market: &str, now: DateTime<Utc>) -> NaiveDate {
    now.with_timezone(&listing_zone(market)).date_naive()
}

/// The year and month a Korean periodic report names ("분기보고서 (2026.03)").
pub fn korean_report_month(title: &str) -> Option<(i32, u32)> {
    let open = title.rfind('(')?;
    let inner = title[open + 1..].split(')').next()?;
    let (year, month) = inner.trim().split_once('.')?;
    Some((year.trim().parse().ok()?, month.trim().parse().ok()?))
}

/// Which Korean periodic report a title is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KoreanReport {
    Quarterly,
    Half,
    Annual,
    Audit,
}

pub fn korean_report(title: &str) -> Option<KoreanReport> {
    let compact: String = title.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.contains("감사보고서제출") {
        Some(KoreanReport::Audit)
    } else if compact.contains("분기보고서") {
        Some(KoreanReport::Quarterly)
    } else if compact.contains("반기보고서") {
        Some(KoreanReport::Half)
    } else if compact.contains("사업보고서") {
        Some(KoreanReport::Annual)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn korean_dates_come_from_the_legal_deadline_when_nothing_else_says() {
        assert_eq!(korea_deadline(day("2026-03-31")), day("2026-05-15"));
        assert_eq!(korea_deadline(day("2026-09-30")), day("2026-11-14"));
        assert_eq!(korea_deadline(day("2025-12-31")), day("2026-03-31"));
        assert_eq!(korea_next_period(day("2026-11-01")), day("2026-09-30"));
        assert_eq!(korea_next_period(day("2026-11-20")), day("2026-12-31"));
        // The annual deadline (Mar 31) outlasts Q1's end.
        assert_eq!(korea_next_period(day("2026-03-20")), day("2025-12-31"));
        assert_eq!(korea_next_period(day("2026-04-02")), day("2026-03-31"));
        assert_eq!(period_label(day("2026-09-30")), "Q3 2026");
        assert_eq!(label_end("Q1 2026"), Some(day("2026-03-31")));
        assert_eq!(quarter_end_before(day("2026-04-28")), day("2026-03-31"));
        assert_eq!(quarter_end_before(day("2026-01-15")), day("2025-12-31"));
        assert_eq!(
            quarter_end_on_or_after(day("2026-10-01")),
            day("2026-12-31")
        );
        assert_eq!(korean_report_month("분기보고서 (2026.03)"), Some((2026, 3)));
        assert_eq!(
            korean_report("[기재정정]반기보고서 (2026.06)"),
            Some(KoreanReport::Half)
        );
        assert_eq!(korean_report("감사보고서제출"), Some(KoreanReport::Audit));
    }

    #[test]
    fn captures_run_at_six_in_the_evening_in_the_listing_zone() {
        let seoul = evening(day("2026-04-27"), "KRX");
        assert_eq!(seoul.to_rfc3339(), "2026-04-27T09:00:00+00:00");
        let new_york = evening(day("2026-10-29"), "NASDAQ");
        assert_eq!(new_york.to_rfc3339(), "2026-10-29T22:00:00+00:00");
    }
}
