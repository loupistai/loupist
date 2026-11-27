//! One local-day calculation for scheduled Chat, World claims and Usage.
use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard},
};

use chrono::{DateTime, NaiveDate, Offset};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::{
    domain::ChatFailureKind,
    runs::log::UsageSurface,
    store::{
        model_usage::{model_usage_in_range, ModelUsageFailure},
        SqliteStore, StoreError, StoreResult,
    },
};

const DAY_MS: i64 = 86_400_000;
pub(crate) const START_FLOOR: u32 = 5;
pub(crate) const DEFAULT_BACKGROUND: u32 = 400;
pub(crate) const DEFAULT_WORLD_CHAT: u32 = 300;

/// Ended runs older than this cannot fall on any local day the allowance reads.
fn ended_runs_since_ms() -> i64 {
    crate::local_time::now()
        .timestamp_millis()
        .saturating_sub(3 * DAY_MS)
}

// Each registered Loupist holds its running rows in its own database. All
// admission paths take this lock before opening their write transaction.
static CLAIMS: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorldOrigin {
    #[default]
    Background,
    Chat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub(crate) enum HoldReason {
    Allowance,
    PlanLimit,
    OutOfCredit,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Hold {
    pub(crate) reason: HoldReason,
    pub(crate) local_day: String,
    pub(crate) provider: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub background: u32,
    pub world_chat: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            background: DEFAULT_BACKGROUND,
            world_chat: DEFAULT_WORLD_CHAT,
        }
    }
}

impl Limits {
    pub(crate) fn load(app_data: &std::path::Path) -> Result<Self, crate::app::CommandError> {
        let config = crate::scoped_config::resolve(app_data, None)?;
        Ok(Self {
            background: config.background_daily_requests,
            world_chat: config.world_chat_daily_requests,
        })
    }
}

#[derive(Debug, Default, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
enum Level {
    #[default]
    Ok,
    Warning,
    Reached,
    Off,
}

#[derive(Debug, Default, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct Bucket {
    limit: u32,
    used: u64,
    reserved: u64,
    state: Level,
}

impl Bucket {
    fn remaining(&self) -> u32 {
        u64::from(self.limit)
            .saturating_sub(self.used)
            .saturating_sub(self.reserved) as u32
    }

    fn update_level(&mut self) {
        self.state = if self.limit == 0 {
            Level::Off
        } else if self.used >= u64::from(self.limit) {
            Level::Reached
        } else if self.used.saturating_mul(5) >= u64::from(self.limit) * 4 {
            Level::Warning
        } else {
            Level::Ok
        };
    }
}

#[derive(Debug, Default, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct WorldShare {
    used: u64,
    limit: u32,
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    reserved: u64,
}

impl WorldShare {
    #[allow(dead_code, reason = "used by the World claim lane")]
    fn remaining(&self) -> u32 {
        u64::from(self.limit)
            .saturating_sub(self.used)
            .saturating_sub(self.reserved) as u32
    }
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct AllowanceState {
    pub background: Bucket,
    pub world_chat: Bucket,
    pub world_share: WorldShare,
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    day: NaiveDate,
    /// Whether the single app-wide learning slot is occupied.
    pub(crate) learning_running: bool,
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    learning_used: BTreeMap<String, u64>,
    /// The admitted Loupist's daily learning allowance, from its config.
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    learning_limit: u32,
    pub provider_holds: BTreeMap<String, Hold>,
}

impl AllowanceState {
    pub(crate) fn allowance_hold(&self) -> Hold {
        Hold {
            reason: HoldReason::Allowance,
            local_day: self.day.to_string(),
            provider: None,
        }
    }

    pub(crate) fn background_remaining(&self) -> u32 {
        self.background.remaining()
    }

    /// What is left of the learning allowance of the Loupist this state was
    /// admitted for. At 0 every learning run is held.
    pub(crate) fn learning_remaining(&self, loupist_id: &str) -> u32 {
        u64::from(self.learning_limit)
            .saturating_sub(self.learning_used.get(loupist_id).copied().unwrap_or(0)) as u32
    }
}

pub(crate) fn admit_run(
    state: &AllowanceState,
    provider: Option<&str>,
    requests: u32,
) -> Result<u32, Hold> {
    if let Some(hold) = provider.and_then(|id| state.provider_holds.get(id)) {
        return Err(hold.clone());
    }
    let requests = requests.min(state.background_remaining());
    if requests < START_FLOOR {
        return Err(state.allowance_hold());
    }
    Ok(requests)
}

#[allow(dead_code, reason = "used by the World claim lane")]
pub(crate) fn admit_world_attempt(
    state: &AllowanceState,
    origin: WorldOrigin,
    calls: u32,
    provider: Option<&str>,
) -> Result<(), Hold> {
    let remaining = match origin {
        WorldOrigin::Background => {
            if let Some(hold) = provider.and_then(|id| state.provider_holds.get(id)) {
                return Err(hold.clone());
            }
            state
                .background_remaining()
                .min(state.world_share.remaining())
        }
        WorldOrigin::Chat => state.world_chat.remaining(),
    };
    // A World chunk spends its own remaining calls. The scheduled-run floor
    // stays in `admit_run`; three calls with four left must start.
    if remaining < calls {
        return Err(state.allowance_hold());
    }
    Ok(())
}

#[derive(Clone)]
struct EndedRun {
    run_id: String,
    budget: u64,
    finished_at_ms: i64,
    /// A plan-limit or out-of-credit failure and the provider it was admitted for.
    hold: Option<(String, HoldReason)>,
}

#[derive(Default)]
struct Reserved {
    run_budgets: BTreeMap<String, u64>,
    /// Background runs that have ended. One whose usage never reached the
    /// ledger (a crash, or a refused write) counts its frozen budget, the most
    /// it can have spent, and keeps the provider hold its failure recorded.
    ended_runs: Vec<EndedRun>,
    learning_running: bool,
    world_background: u64,
    world_chat: u64,
}

impl Reserved {
    fn add_rows(&mut self, connection: &Connection) -> StoreResult<()> {
        let mut runs = connection.prepare(
            "SELECT run_id, coalesce(json_extract(data_json, '$.budget.requests'), 0),
                    coalesce(json_extract(data_json, '$.kind') = 'learning', 0)
             FROM scheduled_task_runs WHERE status = 'running' AND json_valid(data_json)",
        )?;
        for row in runs.query_map([], |row| {
            Ok((row.get(0)?, row.get::<_, u32>(1)?, row.get::<_, bool>(2)?))
        })? {
            let (id, cap, learning) = row?;
            self.run_budgets.insert(id, u64::from(cap));
            self.learning_running |= learning;
        }
        let mut ended = connection.prepare(
            "SELECT run_id, coalesce(json_extract(data_json, '$.budget.requests'), 0), finished_at_ms,
                    CASE WHEN status = 'failed' THEN json_extract(data_json, '$.error') END,
                    json_extract(data_json, '$.provider')
             FROM scheduled_task_runs
             WHERE status <> 'running' AND finished_at_ms >= ?1 AND json_valid(data_json)",
        )?;
        for row in ended.query_map([ended_runs_since_ms()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })? {
            let (run_id, cap, finished_at_ms, error, provider) = row?;
            let reason = match error.as_deref() {
                Some(crate::pi::provider_failure::PLAN_LIMIT) => Some(HoldReason::PlanLimit),
                Some(crate::pi::provider_failure::OUT_OF_CREDIT) => Some(HoldReason::OutOfCredit),
                _ => None,
            };
            let hold = provider.zip(reason);
            if cap > 0 || hold.is_some() {
                self.ended_runs.push(EndedRun {
                    run_id,
                    budget: u64::from(cap),
                    finished_at_ms,
                    hold,
                });
            }
        }
        let mut attempts = connection.prepare(
            "SELECT json_extract(payload_json, '$.origin'),
                    coalesce(json_extract(payload_json, '$.extraction.maxLlmCalls'), 0)
             FROM knowledge_graph_outbox WHERE status = 'running' AND json_valid(payload_json)",
        )?;
        for row in attempts.query_map([], |row| {
            Ok((row.get::<_, Option<String>>(0)?, row.get::<_, u32>(1)?))
        })? {
            let (origin, calls) = row?;
            let calls = u64::from(calls);
            if origin.as_deref() == Some("chat") {
                self.world_chat = self.world_chat.saturating_add(calls);
            } else {
                self.world_background = self.world_background.saturating_add(calls);
            }
        }
        Ok(())
    }
}

/// Take before the target write transaction and retain through its commit.
/// The transaction supplies the target's own rows. Existing claims in other
/// databases can only release their reservations during this sequence.
pub(crate) struct Admission<'a> {
    store: &'a SqliteStore,
    reserved: Reserved,
    _claims: MutexGuard<'static, ()>,
}

impl<'a> Admission<'a> {
    pub(crate) fn prepare(store: &'a SqliteStore, target: Option<&str>) -> StoreResult<Self> {
        let claims = CLAIMS.lock().map_err(|_| StoreError::LockPoisoned)?;
        // Create the shared ledger before opening a folder write transaction.
        store.initialize_model_usage()?;
        let mut reserved = Reserved::default();
        if target.is_none() || store.is_folder_catalog() {
            store.for_each_readable_database(|id, connection| {
                if Some(id) != target {
                    reserved.add_rows(connection)?;
                }
                Ok(())
            })?;
        }
        Ok(Self {
            store,
            reserved,
            _claims: claims,
        })
    }

    pub(crate) fn state(
        &self,
        target: Option<&Connection>,
        limits: Limits,
        now_ms: i64,
    ) -> StoreResult<AllowanceState> {
        self.state_with_clock(target, limits, now_ms, crate::local_time::date_at_ms)
    }

    pub(crate) fn state_with_clock(
        &self,
        target: Option<&Connection>,
        limits: Limits,
        now_ms: i64,
        date_at: impl Fn(i64) -> Option<NaiveDate>,
    ) -> StoreResult<AllowanceState> {
        let day =
            date_at(now_ms).ok_or_else(|| StoreError::Invalid("invalid allowance day".into()))?;
        let middle = day
            .and_hms_opt(0, 0, 0)
            .expect("midnight")
            .and_utc()
            .timestamp_millis();
        // Filter a padded UTC range using each timestamp's own local offset.
        let (from, to) = (middle - DAY_MS, middle + 2 * DAY_MS);
        let mut current = Reserved::default();
        if let Some(connection) = target {
            current.add_rows(connection)?;
        }
        let mut running: BTreeMap<_, _> = self.reserved.run_budgets.clone();
        running.extend(current.run_budgets);
        let mut ended = self.reserved.ended_runs.clone();
        ended.extend(current.ended_runs);
        let rows = match target {
            Some(connection) if !self.store.is_folder_catalog() => {
                model_usage_in_range(connection, from, to)?
            }
            Some(_) => {
                // The catalog lives outside the Loupist folder. A read-only
                // handle avoids acquiring the catalog mutex under a folder lock.
                let path = self
                    .store
                    .database_path()
                    .expect("folder catalog has a path");
                let catalog = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                model_usage_in_range(&catalog, from, to)?
            }
            None => self.store.model_usage_in_range(from, to)?,
        };
        let learning_limit = match target {
            Some(connection) => {
                crate::scheduled_tasks::learning::settings_beside(connection)?.daily_requests
            }
            None => crate::scheduled_tasks::types::DEFAULT_LEARNING_DAILY_REQUESTS,
        };
        let mut state = AllowanceState {
            day,
            learning_running: self.reserved.learning_running || current.learning_running,
            learning_used: BTreeMap::new(),
            learning_limit,
            background: Bucket {
                limit: limits.background,
                ..Default::default()
            },
            world_chat: Bucket {
                limit: limits.world_chat,
                reserved: self.reserved.world_chat.saturating_add(current.world_chat),
                ..Default::default()
            },
            world_share: WorldShare {
                limit: limits.background / 2,
                used: 0,
                reserved: self
                    .reserved
                    .world_background
                    .saturating_add(current.world_background),
            },
            provider_holds: BTreeMap::new(),
        };
        let recorded: std::collections::BTreeSet<String> =
            rows.iter().map(|row| row.data.run_id.clone()).collect();
        // Provider holds today, from ledger rows and from failed runs whose row
        // was lost; the latest one on a provider gives its reason.
        let mut holds: Vec<(i64, String, HoldReason)> = Vec::new();
        for run in &ended {
            if date_at(run.finished_at_ms) != Some(day) || recorded.contains(&run.run_id) {
                continue;
            }
            state.background.used = state.background.used.saturating_add(run.budget);
            if let Some((provider, reason)) = &run.hold {
                holds.push((run.finished_at_ms, provider.clone(), *reason));
            }
        }
        for row in rows
            .into_iter()
            .filter(|row| date_at(row.finished_at_ms) == Some(day))
        {
            let data = row.data;
            let requests = data.measures.requests.unwrap_or(0);
            if data.surface == UsageSurface::Learning {
                if let Some(owner) = row.loupist_id {
                    let total = state.learning_used.entry(owner).or_default();
                    *total = total.saturating_add(requests);
                }
            }
            // Master plan 3.8 and 4.8: a plan-limit or out-of-credit failure on
            // a provider holds background work on it for the rest of the local
            // day, whichever surface failed, an interactive turn included.
            let hold = match data.failure_kind {
                Some(ModelUsageFailure::Chat(ChatFailureKind::PlanLimit)) => {
                    Some(HoldReason::PlanLimit)
                }
                Some(ModelUsageFailure::Chat(ChatFailureKind::OutOfCredit)) => {
                    Some(HoldReason::OutOfCredit)
                }
                _ => None,
            };
            if let Some(reason) = hold {
                holds.push((row.finished_at_ms, data.provider.clone(), reason));
            }
            let counts_background = match data.surface {
                UsageSurface::Scheduled | UsageSurface::Briefing | UsageSurface::Learning => {
                    if let Some(cap) = running.get_mut(&data.run_id) {
                        *cap = cap.saturating_sub(requests);
                    }
                    true
                }
                UsageSurface::World if data.world_origin == Some(WorldOrigin::Chat) => {
                    state.world_chat.used = state.world_chat.used.saturating_add(requests);
                    false
                }
                UsageSurface::World | UsageSurface::WorldSchema | UsageSurface::MethodLearning => {
                    state.world_share.used = state.world_share.used.saturating_add(requests);
                    true
                }
                _ => false,
            };
            if counts_background {
                state.background.used = state.background.used.saturating_add(requests);
            }
        }
        for lost in self.store.lost_holds() {
            if date_at(lost.at_ms) == Some(day) {
                let reason = match lost.failure {
                    ChatFailureKind::OutOfCredit => HoldReason::OutOfCredit,
                    _ => HoldReason::PlanLimit,
                };
                holds.push((lost.at_ms, lost.provider, reason));
            }
        }
        holds.sort_by_key(|(at_ms, ..)| *at_ms);
        for (_, provider, reason) in holds {
            state.provider_holds.insert(
                provider.clone(),
                Hold {
                    reason,
                    local_day: day.to_string(),
                    provider: Some(provider),
                },
            );
        }
        state.background.reserved = running
            .into_values()
            .fold(state.world_share.reserved, u64::saturating_add);
        state.background.update_level();
        state.world_chat.update_level();
        Ok(state)
    }
}

pub(crate) fn local_date_in_zone(at_ms: i64, zone: chrono_tz::Tz) -> Option<NaiveDate> {
    let time = DateTime::from_timestamp_millis(at_ms)?.with_timezone(&zone);
    time.naive_utc()
        .checked_add_offset(time.offset().fix())
        .map(|time| time.date())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::model_usage::fixture_row;

    #[test]
    fn admission_respects_both_allowances_and_dst_local_midnight() {
        let store = SqliteStore::open_in_memory().unwrap();
        use chrono::{Datelike, Utc, Weekday};
        let next_year = Utc::now().year() + 1;
        let transition = NaiveDate::from_ymd_opt(next_year, 3, 1)
            .unwrap()
            .iter_days()
            .filter(|date| date.weekday() == Weekday::Sun)
            .nth(1)
            .unwrap();
        let after = transition
            .succ_opt()
            .unwrap()
            .and_hms_opt(4, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        let before = after - 1;
        let zone = chrono_tz::America::New_York;
        let date = |at| local_date_in_zone(at, zone);
        let mut run = fixture_row(before);
        run.data.surface = UsageSurface::Scheduled;
        run.data.measures.requests = Some(8);
        run.data.failure_kind = Some(ModelUsageFailure::Chat(ChatFailureKind::PlanLimit));
        store.put_model_usage("scheduled", &run).unwrap();
        let mut world = fixture_row(before);
        world.data.surface = UsageSurface::World;
        world.data.measures.requests = Some(12);
        store.put_model_usage("background-world", &world).unwrap();
        world.data.world_origin = Some(WorldOrigin::Chat);
        world.data.measures.requests = Some(20);
        store.put_model_usage("chat-world", &world).unwrap();
        let mut admission = Admission::prepare(&store, None).unwrap();
        admission.reserved.world_background = 3;
        admission.reserved.run_budgets.insert(run.data.run_id, 10);
        let limits = Limits {
            background: 40,
            world_chat: 32,
        };
        let state = admission
            .state_with_clock(None, limits, before, date)
            .unwrap();
        assert_eq!((state.background.used, state.background.reserved), (20, 5));
        assert_eq!(admit_run(&state, None, 30), Ok(15));
        assert_eq!(
            admit_run(&state, None, 4).unwrap_err().reason,
            HoldReason::Allowance
        );
        for (origin, calls, allowed) in [
            (WorldOrigin::Background, 5, true),
            (WorldOrigin::Background, 6, false),
            (WorldOrigin::Chat, 12, true),
            (WorldOrigin::Chat, 13, false),
        ] {
            assert_eq!(
                admit_world_attempt(&state, origin, calls, None).is_ok(),
                allowed
            );
        }
        assert_eq!(
            admit_run(&state, Some(&run.data.provider), 5)
                .unwrap_err()
                .reason,
            HoldReason::PlanLimit
        );
        assert!(
            admit_world_attempt(&state, WorldOrigin::Chat, 12, Some(&run.data.provider)).is_ok()
        );
        assert_ne!(date(before), date(after));
        let next = admission
            .state_with_clock(None, limits, after, date)
            .unwrap();
        assert_eq!(next.background.used, 0);
        assert_eq!(next.world_chat.used, 0);
        assert!(next.provider_holds.is_empty());
    }

    #[test]
    fn a_world_chunk_uses_its_own_call_count_not_the_scheduled_run_floor() {
        let store = SqliteStore::open_in_memory().unwrap();
        let admission = Admission::prepare(&store, None).unwrap();
        let state = admission
            .state(
                None,
                Limits {
                    background: 8,
                    world_chat: 4,
                },
                1_800_000_000_000,
            )
            .unwrap();
        assert!(admit_world_attempt(&state, WorldOrigin::Chat, 3, None).is_ok());
        assert!(admit_world_attempt(&state, WorldOrigin::Background, 3, None).is_ok());
        assert!(admit_world_attempt(&state, WorldOrigin::Chat, 5, None).is_err());
        assert!(admit_run(&state, None, 4).is_err());
    }
}
