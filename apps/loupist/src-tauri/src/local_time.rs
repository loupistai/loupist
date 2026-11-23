//! The user's own clock. Whatever tells the model or an unattended run what
//! "today" is reads it here: the Mac's time zone, or, under an eval scenario
//! clock, that clock's frozen instant at the offset its RFC 3339 text states.
//! Only the offset ever leaves the Mac, never a zone name or a place.
use chrono::{DateTime, FixedOffset, Local, NaiveDate, Utc};

/// This moment at the user's offset.
pub(crate) fn now() -> DateTime<FixedOffset> {
    #[cfg(feature = "eval-harness")]
    if let Some(now) = crate::eval_harness::clock::local_now() {
        return now;
    }
    Local::now().fixed_offset()
}

/// `instant` at the offset the user's time zone had then, so a date across a
/// daylight-saving change keeps the offset of its own day.
pub(crate) fn at(instant: DateTime<Utc>) -> DateTime<FixedOffset> {
    #[cfg(feature = "eval-harness")]
    if let Some(offset) = crate::eval_harness::clock::offset() {
        return instant.with_timezone(&offset);
    }
    instant.with_timezone(&Local).fixed_offset()
}

/// A host timestamp at the user's offset then.
pub(crate) fn at_ms(at_ms: i64) -> Option<DateTime<FixedOffset>> {
    DateTime::from_timestamp_millis(at_ms).map(at)
}

/// The user's calendar date now.
pub(crate) fn today() -> NaiveDate {
    now().date_naive()
}

/// The user's calendar date at a host timestamp.
pub(crate) fn date_at_ms(at_ms: i64) -> Option<NaiveDate> {
    self::at_ms(at_ms).map(|local| local.date_naive())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_dates_follow_the_time_zone_of_this_mac() {
        let instant = DateTime::parse_from_rfc3339("2026-11-22T17:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let local = at(instant);
        assert_eq!(local, instant);
        assert_eq!(*local.offset(), *instant.with_timezone(&Local).offset());
        assert_eq!(
            date_at_ms(instant.timestamp_millis()),
            Some(instant.with_timezone(&Local).date_naive())
        );
        assert!((now() - Local::now().fixed_offset()).num_seconds().abs() < 2);
        assert_eq!(date_at_ms(i64::MAX), None);
    }
}
