//! Eval-only model clock; host observation windows continue using their own clock.
use chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    sync::{OnceLock, RwLock},
};
type Scope = Option<tokio::runtime::Id>;
static CLOCKS: OnceLock<RwLock<HashMap<Scope, DateTime<Utc>>>> = OnceLock::new();
fn clocks() -> &'static RwLock<HashMap<Scope, DateTime<Utc>>> {
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
pub(crate) struct Guard(Scope, Option<DateTime<Utc>>, bool);
pub(crate) fn install(value: Option<&str>) -> Result<Guard, String> {
    let scope = scope();
    if let Some(value) = value {
        let value = DateTime::parse_from_rfc3339(value)
            .map_err(|_| "scenario clock must be RFC 3339")?
            .with_timezone(&Utc);
        let previous = clocks().write().unwrap().insert(scope, value);
        return Ok(Guard(scope, previous, true));
    }
    Ok(Guard(scope, None, false))
}
pub(crate) fn now() -> DateTime<Utc> {
    clocks()
        .read()
        .unwrap()
        .get(&scope())
        .copied()
        .unwrap_or_else(Utc::now)
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
        drop(guard);
        assert!((now() - Utc::now()).num_seconds().abs() < 2);
    }
}
