use chrono::{Datelike, NaiveDate, Timelike};

use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{TimePoint, TimePrecision};

pub(super) fn normalize_time_point(point: &TimePoint) -> KnowledgeResult<TimePoint> {
    match point.precision {
        TimePrecision::Unknown => {
            if point.value.is_some() || point.approximate {
                return Err(KnowledgeError::Invalid("unknown time has a value"));
            }
            Ok(TimePoint {
                precision: TimePrecision::Unknown,
                value: None,
                approximate: false,
            })
        }
        TimePrecision::Year => Ok(TimePoint {
            precision: TimePrecision::Year,
            value: Some(canonical_year(required(point)?)?),
            approximate: point.approximate,
        }),
        TimePrecision::Month => Ok(TimePoint {
            precision: TimePrecision::Month,
            value: Some(canonical_month(required(point)?)?),
            approximate: point.approximate,
        }),
        TimePrecision::Date => Ok(TimePoint {
            precision: TimePrecision::Date,
            value: Some(canonical_date(required(point)?)?),
            approximate: point.approximate,
        }),
        TimePrecision::Instant => Ok(TimePoint {
            precision: TimePrecision::Instant,
            value: Some(canonical_instant(required(point)?)?),
            approximate: point.approximate,
        }),
    }
}

fn required(point: &TimePoint) -> KnowledgeResult<&str> {
    point
        .value
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or(KnowledgeError::Invalid("time value is missing"))
}

fn canonical_year(value: &str) -> KnowledgeResult<String> {
    let year = parse_year(value)?;
    Ok(format!("{year:04}"))
}

fn canonical_month(value: &str) -> KnowledgeResult<String> {
    let (year, month) = split_two(value, "month is invalid")?;
    let year = parse_year(year)?;
    let month = parse_month(month)?;
    Ok(format!("{year:04}-{month:02}"))
}

fn canonical_date(value: &str) -> KnowledgeResult<String> {
    let mut parts = value.split('-');
    let year = parts
        .next()
        .ok_or(KnowledgeError::Invalid("date is invalid"))?;
    let month = parts
        .next()
        .ok_or(KnowledgeError::Invalid("date is invalid"))?;
    let day = parts
        .next()
        .ok_or(KnowledgeError::Invalid("date is invalid"))?;
    if parts.next().is_some() || day.len() != 2 {
        return Err(KnowledgeError::Invalid("date is invalid"));
    }
    let year = parse_year(year)?;
    let month = parse_month(month)?;
    let day: u32 = day
        .parse()
        .map_err(|_| KnowledgeError::Invalid("date is invalid"))?;
    let date = NaiveDate::from_ymd_opt(year, month, day)
        .ok_or(KnowledgeError::Invalid("date is invalid"))?;
    Ok(format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        date.month(),
        date.day()
    ))
}

fn canonical_instant(value: &str) -> KnowledgeResult<String> {
    if !value.contains('T')
        || !(value.ends_with('Z')
            || value.contains('+')
            || value.rfind('-').is_some_and(|index| index > 10))
    {
        return Err(KnowledgeError::Invalid(
            "instant requires a timezone offset",
        ));
    }
    let parsed = chrono::DateTime::parse_from_rfc3339(value)
        .map_err(|_| KnowledgeError::Invalid("instant is invalid"))?;
    let offset = parsed.offset().local_minus_utc();
    let sign = if offset < 0 { '-' } else { '+' };
    let absolute = offset.unsigned_abs();
    let hours = absolute / 3600;
    let minutes = (absolute % 3600) / 60;
    if minutes > 59 || hours > 23 {
        return Err(KnowledgeError::Invalid("instant is invalid"));
    }
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{sign}{hours:02}:{minutes:02}",
        parsed.year(),
        parsed.month(),
        parsed.day(),
        parsed.hour(),
        parsed.minute(),
        parsed.second()
    ))
}

fn parse_year(value: &str) -> KnowledgeResult<i32> {
    if value.len() != 4 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(KnowledgeError::Invalid("year is invalid"));
    }
    let year: i32 = value
        .parse()
        .map_err(|_| KnowledgeError::Invalid("year is invalid"))?;
    if (1..=9999).contains(&year) {
        Ok(year)
    } else {
        Err(KnowledgeError::Invalid("year is invalid"))
    }
}

fn parse_month(value: &str) -> KnowledgeResult<u32> {
    if value.len() != 2 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(KnowledgeError::Invalid("month is invalid"));
    }
    let month: u32 = value
        .parse()
        .map_err(|_| KnowledgeError::Invalid("month is invalid"))?;
    if (1..=12).contains(&month) {
        Ok(month)
    } else {
        Err(KnowledgeError::Invalid("month is invalid"))
    }
}

fn split_two<'a>(value: &'a str, label: &'static str) -> KnowledgeResult<(&'a str, &'a str)> {
    let mut parts = value.split('-');
    let year = parts.next().ok_or(KnowledgeError::Invalid(label))?;
    let month = parts.next().ok_or(KnowledgeError::Invalid(label))?;
    if parts.next().is_some() {
        return Err(KnowledgeError::Invalid(label));
    }
    Ok((year, month))
}
