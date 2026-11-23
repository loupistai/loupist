//! Eval-only model clock; host observation windows continue using their own clock.
//! The offset the scenario's RFC 3339 clock states is the user's local offset
//! (`crate::local_time`), so a recorded scenario sees the same local date on any host.
use chrono::{DateTime, FixedOffset, Utc};
use std::{
    collections::HashMap,
    sync::{OnceLock, RwLock},
};
type Scope = Option<tokio::runtime::Id>;
static CLOCKS: OnceLock<RwLock<HashMap<Scope, DateTime<FixedOffset>>>> = OnceLock::new();
fn clocks() -> &'static RwLock<HashMap<Scope, DateTime<FixedOffset>>> {
    CLOCKS.get_or_init(Default::default)
}
fn scope() -> Scope {
    if cfg!(test) {
        tokio::runtime::Handle::try_current()
            .ok()
            .map(|runtime| runtime.id())
    } else {
        None
    }
}
pub(crate) struct Guard(Scope, Option<DateTime<FixedOffset>>, bool);
pub(crate) fn install(value: Option<&str>) -> Result<Guard, String> {
    let scope = scope();
    if let Some(value) = value {
        let value =
            DateTime::parse_from_rfc3339(value).map_err(|_| "scenario clock must be RFC 3339")?;
        let previous = clocks().write().unwrap().insert(scope, value);
        return Ok(Guard(scope, previous, true));
    }
    Ok(Guard(scope, None, false))
}
pub(crate) fn now() -> DateTime<Utc> {
    local_now().map_or_else(Utc::now, |now| now.with_timezone(&Utc))
}
/// The scenario's instant at its stated offset, when a scenario clock is installed.
pub(crate) fn local_now() -> Option<DateTime<FixedOffset>> {
    clocks().read().unwrap().get(&scope()).copied()
}
/// The scenario clock's stated offset, which stands for the user's time zone.
pub(crate) fn offset() -> Option<FixedOffset> {
    local_now().map(|now| *now.offset())
}
impl Drop for Guard {
    fn drop(&mut self) {
        if self.2 {
            let mut clocks = clocks().write().unwrap();
            if let Some(previous) = self.1 {
                clocks.insert(self.0, previous);
            } else {
                clocks.remove(&self.0);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn model_clock_is_scoped_validated_and_restored() {
        assert!(install(Some("not a date")).is_err());
        let guard = install(Some("2026-04-15T09:00:00+09:00")).unwrap();
        assert_eq!(now().to_rfc3339(), "2026-04-15T00:00:00+00:00");
        assert_eq!(tokio::spawn(async { now() }).await.unwrap(), now());
        assert_eq!(
            local_now().unwrap().to_rfc3339(),
            "2026-04-15T09:00:00+09:00"
        );
        assert_eq!(offset().unwrap().local_minus_utc(), 9 * 3600);
        drop(guard);
        assert!((now() - Utc::now()).num_seconds().abs() < 2);
        assert_eq!((local_now(), offset()), (None, None));
    }
    #[tokio::test]
    async fn the_live_time_envelope_reads_the_scenario_clock_at_its_stated_offset() {
        let live_time = || crate::agent_harness::live_time_envelope(crate::local_time::now());
        let seoul = install(Some("2026-11-23T02:00:00+09:00")).unwrap();
        let block = live_time();
        assert_eq!(block["current_utc"], "2026-11-22T17:00:00.000Z");
        assert_eq!(block["current_date_utc"], "2026-11-22");
        assert_eq!(block["current_local"], "2026-11-23T02:00:00.000+09:00");
        assert_eq!(block["current_date_local"], "2026-11-23");
        assert_eq!(block["current_weekday_local"], "Monday");
        assert_eq!(block["utc_offset"], "+09:00");
        // Every other local date the host derives uses the same offset.
        let evening = DateTime::parse_from_rfc3339("2026-11-20T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            crate::local_time::at(evening).to_rfc3339(),
            "2026-11-21T05:00:00+09:00"
        );
        assert_eq!(
            crate::local_time::date_at_ms(evening.timestamp_millis()).map(|day| day.to_string()),
            Some("2026-11-21".into())
        );
        assert_eq!(crate::local_time::today().to_string(), "2026-11-23");
        {
            // A clock written in UTC is a user at UTC.
            let _utc = install(Some("2026-04-15T08:00:00Z")).unwrap();
            let block = live_time();
            assert_eq!(block["current_local"], "2026-04-15T08:00:00.000+00:00");
            assert_eq!(block["current_date_local"], block["current_date_utc"]);
            assert_eq!(block["utc_offset"], "+00:00");
        }
        assert_eq!(live_time()["current_date_local"], "2026-11-23");
        drop(seoul);
        let mac = chrono::Local::now();
        assert_eq!(live_time()["utc_offset"], mac.format("%:z").to_string());
    }
}
