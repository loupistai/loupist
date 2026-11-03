use chrono::{Datelike, LocalResult, TimeZone, Utc};
use chrono_tz::Tz;

use crate::store::StoreResult;

use super::types::{invalid, Schedule};

pub fn validate_schedule(schedule: &Schedule) -> StoreResult<()> {
    match schedule {
        Schedule::Once { at_ms } => {
            if *at_ms < 0 || Utc.timestamp_millis_opt(*at_ms).single().is_none() {
                return Err(invalid("scheduled time is invalid"));
            }
        }
        Schedule::Interval { every_minutes } => {
            if !(1..=10080).contains(every_minutes) {
                return Err(invalid(
                    "repeat interval must be between 1 and 10080 minutes",
                ));
            }
        }
        Schedule::Daily {
            hour,
            minute,
            timezone,
        }
        | Schedule::Weekly {
            hour,
            minute,
            timezone,
            ..
        } => {
            if *hour > 23 || *minute > 59 || timezone.len() > 128 || timezone.parse::<Tz>().is_err()
            {
                return Err(invalid("scheduled local time or IANA timezone is invalid"));
            }
        }
    }
    if let Schedule::Weekly { days, .. } = schedule {
        let unique: std::collections::BTreeSet<_> = days.iter().collect();
        if days.is_empty()
            || days.len() > 7
            || unique.len() != days.len()
            || days.iter().any(|day| *day > 6)
        {
            return Err(invalid(
                "weekly days must be distinct values between Sunday 0 and Saturday 6",
            ));
        }
    }
    Ok(())
}

/// The next occurrence is strictly later than `after_ms`. Calendar recurrences
/// skip nonexistent local times and use only the first instant in a DST fold.
pub fn next_run_after(schedule: &Schedule, after_ms: i64) -> StoreResult<Option<i64>> {
    validate_schedule(schedule)?;
    let after = Utc
        .timestamp_millis_opt(after_ms)
        .single()
        .filter(|_| after_ms >= 0)
        .ok_or_else(|| invalid("schedule reference time is invalid"))?;
    match schedule {
        Schedule::Once { at_ms } => Ok((*at_ms > after_ms).then_some(*at_ms)),
        Schedule::Interval { every_minutes } => {
            let next = after_ms
                .checked_add(i64::from(*every_minutes) * 60_000)
                .filter(|next| Utc.timestamp_millis_opt(*next).single().is_some())
                .ok_or_else(|| invalid("next scheduled time is outside the supported range"))?;
            Ok(Some(next))
        }
        Schedule::Daily {
            hour,
            minute,
            timezone,
        }
        | Schedule::Weekly {
            hour,
            minute,
            timezone,
            ..
        } => {
            let timezone: Tz = timezone
                .parse()
                .map_err(|_| invalid("scheduled timezone is invalid"))?;
            let mut date = after.with_timezone(&timezone).date_naive();
            // A daily/weekly wall time can disappear on a transition day. This
            // bound also covers IANA date-line changes that skip an entire day.
            for _ in 0..370 {
                let included = match schedule {
                    Schedule::Weekly { days, .. } => {
                        days.contains(&(date.weekday().num_days_from_sunday() as u8))
                    }
                    _ => true,
                };
                if included {
                    let local = date
                        .and_hms_opt(u32::from(*hour), u32::from(*minute), 0)
                        .ok_or_else(|| invalid("scheduled local time is invalid"))?;
                    let instant = match timezone.from_local_datetime(&local) {
                        LocalResult::Single(value) => Some(value),
                        LocalResult::Ambiguous(first, second) => Some(first.min(second)),
                        LocalResult::None => None,
                    };
                    if let Some(next) = instant
                        .map(|value| value.timestamp_millis())
                        .filter(|next| *next > after_ms)
                    {
                        return Ok(Some(next));
                    }
                }
                date = date
                    .succ_opt()
                    .ok_or_else(|| invalid("next scheduled date is outside the supported range"))?;
            }
            Err(invalid(
                "no next scheduled time exists in the supported window",
            ))
        }
    }
}
