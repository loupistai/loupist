//! Local usage summaries. The ledger supplies measures and the catalog supplies names.
use std::collections::BTreeMap;

use chrono::NaiveDate;
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    app::{AppState, CommandError},
    domain::ChatCostEstimate,
    providers::status::{live_credential_kind, CredentialKind},
    runs::log::UsageSurface,
    scheduled_tasks::allowance::{
        local_date_in_zone as local_date, Admission, AllowanceState, Limits,
    },
    store::{
        model_usage::{ModelUsageMeasures, ModelUsageRow, USAGE_RETENTION_MS},
        SqliteStore,
    },
};

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageSummaryRequest {
    pub from_ms: i64,
    pub to_ms: i64,
    pub time_zone: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub from_ms: i64,
    pub to_ms: i64,
    pub totals: UsageTotals,
    pub accounts: Vec<UsageAccount>,
    pub days: Vec<UsageDay>,
    pub surfaces: Vec<UsageBySurface>,
    pub loupists: Vec<UsageByLoupist>,
    pub programs: Vec<UsageProgram>,
    pub allowance: AllowanceState,
}

#[derive(Debug, Default, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UsageTotals {
    pub runs: u32,
    pub unknown_runs: u32,
    pub requests: u64,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub cost_estimate: Option<ChatCostEstimate>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UsageAccount {
    pub provider: String,
    pub credential_kind: Option<CredentialKind>,
    pub totals: UsageTotals,
}

#[derive(Debug, Default, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UsageDay {
    pub date: String,
    pub you: u64,
    pub background: u64,
    pub world: u64,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UsageBySurface {
    pub surface: UsageSurface,
    pub totals: UsageTotals,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UsageByLoupist {
    pub loupist_id: Option<String>,
    pub name: Option<String>,
    pub totals: UsageTotals,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UsageProgram {
    pub loupist_id: String,
    pub task_id: String,
    pub title: Option<String>,
    pub runs: u32,
    pub totals: UsageTotals,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub checks: Option<UsageChecks>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UsageChecks {
    pub evaluations: u32,
    pub reads: u32,
    pub met: u32,
    pub last_check_at_ms: Option<i64>,
}

impl UsageSummaryRequest {
    fn dates(&self) -> Result<(Tz, NaiveDate, NaiveDate), CommandError> {
        let valid = self
            .to_ms
            .checked_sub(self.from_ms)
            .is_some_and(|span| (1..=USAGE_RETENTION_MS).contains(&span));
        let invalid_range = || CommandError::invalid("Choose a usage period of up to 400 days.");
        if !valid {
            return Err(invalid_range());
        }
        let zone = self
            .time_zone
            .parse::<Tz>()
            .map_err(|_| CommandError::invalid("Choose a valid time zone."))?;
        let first = local_date(self.from_ms, zone).ok_or_else(invalid_range)?;
        let last = local_date(self.to_ms - 1, zone).ok_or_else(invalid_range)?;
        Ok((zone, first, last))
    }
}

impl UsageTotals {
    fn add(&mut self, data: &ModelUsageMeasures, cost: Option<ChatCostEstimate>) {
        self.runs += 1;
        if data.tokens_partial
            || [data.requests, data.input, data.cached_input, data.output].contains(&None)
        {
            self.unknown_runs += 1;
        }
        self.requests = self.requests.saturating_add(data.requests.unwrap_or(0));
        self.input_tokens = self.input_tokens.saturating_add(data.input.unwrap_or(0));
        self.cached_input_tokens = self
            .cached_input_tokens
            .saturating_add(data.cached_input.unwrap_or(0));
        self.output_tokens = self.output_tokens.saturating_add(data.output.unwrap_or(0));
        add_cost(&mut self.cost_estimate, cost);
    }
}

fn add_cost(total: &mut Option<ChatCostEstimate>, cost: Option<ChatCostEstimate>) {
    if let Some(cost) = cost {
        let sum = total.get_or_insert(ChatCostEstimate {
            microusd: 0,
            from_provider: true,
        });
        sum.microusd = sum.microusd.saturating_add(cost.microusd);
        sum.from_provider &= cost.from_provider;
    }
}

impl UsageDay {
    fn add(&mut self, surface: UsageSurface, requests: u64) {
        let bucket = match surface {
            UsageSurface::Bot
            | UsageSurface::Research
            | UsageSurface::Handoff
            | UsageSurface::DeepResearch => &mut self.you,
            UsageSurface::Briefing | UsageSurface::Scheduled | UsageSurface::Learning => {
                &mut self.background
            }
            UsageSurface::World | UsageSurface::WorldSchema | UsageSurface::MethodLearning => {
                &mut self.world
            }
        };
        *bucket = bucket.saturating_add(requests);
    }
}

#[cfg(test)]
pub(super) fn summarize(
    request: UsageSummaryRequest,
    store: &SqliteStore,
    lookup_credential: impl Fn(&str) -> CredentialKind,
) -> Result<UsageSummary, CommandError> {
    summarize_with_limits(request, store, Limits::default(), lookup_credential)
}

fn summarize_with_limits(
    request: UsageSummaryRequest,
    store: &SqliteStore,
    limits: Limits,
    lookup_credential: impl Fn(&str) -> CredentialKind,
) -> Result<UsageSummary, CommandError> {
    let (zone, first, last) = request.dates()?;
    let allowance = Admission::prepare(store, None)
        .and_then(|snapshot| snapshot.state(None, limits, crate::commands::now_ms()))
        .map_err(super::map_store)?;
    let names: BTreeMap<_, _> = store
        .list_loupists()
        .map_err(super::map_store)?
        .into_iter()
        .map(|profile| (profile.id, profile.name))
        .collect();
    let rows = store
        .model_usage_in_range(request.from_ms, request.to_ms)
        .map_err(super::map_store)?;
    let mut days: BTreeMap<_, _> = first
        .iter_days()
        .take_while(|day| *day <= last)
        .map(|day| {
            (
                day,
                UsageDay {
                    date: day.format("%Y-%m-%d").to_string(),
                    ..Default::default()
                },
            )
        })
        .collect();
    let mut live = BTreeMap::new();
    let mut totals = UsageTotals::default();
    let mut accounts = BTreeMap::<_, UsageAccount>::new();
    let mut surfaces = BTreeMap::<_, UsageTotals>::new();
    let mut loupists = BTreeMap::<_, UsageTotals>::new();
    let (from_day, to_day) = (first.to_string(), last.to_string());
    let mut programs = BTreeMap::new();
    for loupist_id in names.keys() {
        // A workspace can be unavailable while its model ledger is still readable.
        for task in store.scheduled_tasks_list(loupist_id).unwrap_or_default() {
            let checks = task.trigger.as_ref().map(|trigger| {
                let (evaluations, reads, met) =
                    crate::scheduled_tasks::trigger::window_totals(trigger, &from_day, &to_day);
                UsageChecks {
                    evaluations,
                    reads,
                    met,
                    last_check_at_ms: trigger.last.as_ref().map(|check| check.at_ms),
                }
            });
            programs.insert(
                (loupist_id.clone(), task.id),
                (Some(task.title), checks, UsageTotals::default()),
            );
        }
    }
    for ModelUsageRow {
        finished_at_ms,
        loupist_id,
        task_id,
        data,
    } in rows
    {
        let mut add_account =
            |provider: String, kind: Option<CredentialKind>, measures: &ModelUsageMeasures| {
                let credential_kind = kind.or_else(|| {
                    let current = *live
                        .entry(provider.clone())
                        .or_insert_with(|| lookup_credential(&provider));
                    (current != CredentialKind::None).then_some(current)
                });
                let cost = measures
                    .cost_microusd
                    .filter(|_| {
                        matches!(
                            credential_kind,
                            Some(CredentialKind::ApiKey | CredentialKind::Environment)
                        )
                    })
                    .map(|microusd| ChatCostEstimate {
                        microusd,
                        from_provider: measures.cost_from_provider == Some(true),
                    });
                let account = accounts
                    .entry(provider.clone())
                    .or_insert_with(|| UsageAccount {
                        provider,
                        credential_kind,
                        totals: UsageTotals::default(),
                    });
                // Reads are ordered by finish time. Use the latest known account type.
                if credential_kind.is_some() {
                    account.credential_kind = credential_kind;
                }
                account.totals.add(measures, cost);
                cost
            };
        let cost = if data.accounts.is_empty() {
            add_account(data.provider, data.credential_kind, &data.measures)
        } else {
            let mut cost = None;
            for account in data.accounts {
                add_cost(
                    &mut cost,
                    add_account(account.provider, account.credential_kind, &account.measures),
                );
            }
            cost
        };
        totals.add(&data.measures, cost);
        if let Some(day) = local_date(finished_at_ms, zone).and_then(|date| days.get_mut(&date)) {
            day.add(data.surface, data.measures.requests.unwrap_or(0));
        }
        surfaces
            .entry(data.surface)
            .or_default()
            .add(&data.measures, cost);
        loupists
            .entry(loupist_id.clone())
            .or_default()
            .add(&data.measures, cost);
        if let (Some(loupist_id), Some(task_id)) = (loupist_id, task_id) {
            let (_, _, program_usage) = programs.entry((loupist_id, task_id)).or_default();
            program_usage.add(&data.measures, cost);
        }
    }
    Ok(UsageSummary {
        from_ms: request.from_ms,
        to_ms: request.to_ms,
        totals,
        allowance,
        accounts: accounts.into_values().collect(),
        days: days.into_values().collect(),
        surfaces: surfaces
            .into_iter()
            .map(|(surface, totals)| UsageBySurface { surface, totals })
            .collect(),
        loupists: loupists
            .into_iter()
            .map(|(loupist_id, totals)| UsageByLoupist {
                name: loupist_id.as_ref().and_then(|id| names.get(id)).cloned(),
                loupist_id,
                totals,
            })
            .collect(),
        programs: programs
            .into_iter()
            .map(
                |((loupist_id, task_id), (title, checks, totals))| UsageProgram {
                    title,
                    loupist_id,
                    task_id,
                    runs: totals.runs,
                    totals,
                    checks,
                },
            )
            .collect(),
    })
}

#[tauri::command(async, rename_all = "snake_case")]
pub fn usage_summary(
    request: UsageSummaryRequest,
    state: State<'_, AppState>,
) -> Result<UsageSummary, CommandError> {
    let limits = Limits::load(&state.artifacts.app_data_dir)?;
    summarize_with_limits(request, &state.store, limits, |provider| {
        live_credential_kind(&state, provider)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::model_usage::fixture_row;
    use chrono::{DateTime, Datelike, Utc, Weekday};

    #[test]
    fn allowance_view_reports_when_another_loupists_learning_run_occupies_the_slot() {
        use crate::scheduled_tasks::types::*;

        let root = tempfile::tempdir().unwrap();
        let store = SqliteStore::open_catalog(root.path().join("catalog.sqlite3")).unwrap();
        let owner = "running-owner";
        for id in [owner, "other-owner"] {
            crate::test_fixtures::create_folder_loupist(&store, id, &root.path().join(id));
        }
        let now = crate::commands::now_ms();
        let task = store
            .scheduled_task_create_definition(
                owner,
                "learning-task",
                TaskDraft {
                    title: "Daily research".into(),
                    prompt: "Read the latest evidence.".into(),
                    schedule: Schedule::Interval { every_minutes: 60 },
                    target: TaskTarget::NewChat,
                    model_profile_id: "fixture".into(),
                    thinking_level: "medium".into(),
                    run_options: Default::default(),
                    use_memory: true,
                    update_memory: true,
                    status: TaskStatus::Active,
                },
                TaskKind::Learning,
                TaskDefinition {
                    program: Some(LearningProgram {
                        depth: LearningDepth::Standard,
                        scope: LearningScope {
                            connections: LearningConnections::Selected(vec![]),
                            web: false,
                        },
                        keeps: LearningKeeps {
                            sources: true,
                            lens: false,
                            asset_reads: false,
                        },
                        preset: None,
                    }),
                    ..Default::default()
                },
                now,
            )
            .unwrap();
        let learning_running = || {
            let summary = summarize(
                UsageSummaryRequest {
                    from_ms: now - 60_000,
                    to_ms: now + 1,
                    time_zone: "UTC".into(),
                },
                &store,
                |_| CredentialKind::Plan,
            )
            .unwrap();
            serde_json::to_value(summary.allowance).unwrap()["learningRunning"].clone()
        };
        assert_eq!(learning_running(), false);
        let (claim, ()) = store
            .scheduled_task_claim_with(owner, &task.id, now, true, |_| Ok(()))
            .unwrap()
            .unwrap();
        assert!(store
            .scheduled_tasks_activity("other-owner")
            .unwrap()
            .is_empty());
        assert_eq!(learning_running(), true);
        store
            .scheduled_task_finish(owner, &claim.run.run_id, RunStatus::Succeeded, None, now)
            .unwrap();
        assert_eq!(learning_running(), false);
    }

    #[test]
    fn usage_groups_local_dates_across_dst_and_uses_only_key_costs() {
        let store = SqliteStore::open_in_memory().unwrap();
        // Future fixtures stay inside the retention policy as this test ages.
        let transition = NaiveDate::from_ymd_opt(Utc::now().year() + 1, 3, 1)
            .unwrap()
            .iter_days()
            .filter(|day| day.weekday() == Weekday::Sun)
            .nth(1)
            .unwrap();
        let previous = transition.pred_opt().unwrap();
        let next = transition.succ_opt().unwrap();
        let ms = |date: NaiveDate, hour, minute| {
            date.and_hms_opt(hour, minute, 0)
                .unwrap()
                .and_utc()
                .timestamp_millis()
        };
        let first = ms(previous, 5, 0);
        let last = ms(next.succ_opt().unwrap(), 4, 0);
        let mut rows = [
            fixture_row(ms(transition, 4, 59)),
            fixture_row(ms(transition, 5, 0)),
            fixture_row(ms(next, 3, 59)),
            fixture_row(ms(next, 4, 0)),
        ];
        for (index, row) in rows.iter_mut().enumerate() {
            row.data.surface = [
                UsageSurface::Bot,
                UsageSurface::Scheduled,
                UsageSurface::World,
                UsageSurface::DeepResearch,
            ][index];
            row.data.credential_kind = [
                Some(CredentialKind::Plan),
                None,
                Some(CredentialKind::Environment),
                None,
            ][index];
            if index == 3 {
                row.data.provider = "plan".into();
            }
            row.data.measures.requests = Some(index as u64 + 1);
            row.data.measures.cost_microusd = Some(10);
            row.data.measures.cost_from_provider = Some(index != 1);
            store.put_model_usage(&index.to_string(), row).unwrap();
        }
        let usage = summarize(
            UsageSummaryRequest {
                from_ms: first,
                to_ms: last,
                time_zone: "America/New_York".into(),
            },
            &store,
            |provider| {
                if provider == "openrouter" {
                    CredentialKind::ApiKey
                } else {
                    CredentialKind::Plan
                }
            },
        )
        .unwrap();
        assert_eq!(usage.accounts.len(), 2);
        assert_eq!(usage.surfaces.len(), 4);
        assert_eq!(usage.totals.requests, 10);
        assert_eq!(usage.totals.unknown_runs, 4);
        assert_eq!(
            usage.totals.cost_estimate,
            Some(ChatCostEstimate {
                microusd: 20,
                from_provider: false
            })
        );
        assert_eq!(
            usage
                .days
                .iter()
                .map(|day| (day.date.clone(), day.you, day.background, day.world))
                .collect::<Vec<_>>(),
            [
                (previous.to_string(), 1, 0, 0),
                (transition.to_string(), 0, 2, 3),
                (next.to_string(), 4, 0, 0)
            ]
        );
    }

    #[test]
    fn usage_keeps_a_deleted_program_until_its_loupist_is_removed() {
        use crate::scheduled_tasks::types::{Schedule, TaskDraft, TaskStatus, TaskTarget};
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open_catalog(dir.path().join("catalog.sqlite3")).unwrap();
        crate::test_fixtures::create_folder_loupist(&store, "usage", &dir.path().join("workspace"));
        let profile = store.get_loupist("usage").unwrap().unwrap();
        let now = crate::commands::now_ms();
        let task = store
            .scheduled_task_create(
                &profile.id,
                TaskDraft {
                    title: "Weekly review".into(),
                    prompt: "Review the week's results.".into(),
                    schedule: Schedule::Interval { every_minutes: 60 },
                    target: TaskTarget::NewChat,
                    model_profile_id: String::new(),
                    thinking_level: String::new(),
                    run_options: BTreeMap::new(),
                    use_memory: false,
                    update_memory: false,
                    status: TaskStatus::Paused,
                },
                now,
            )
            .unwrap();
        let mut row = fixture_row(now);
        row.loupist_id = Some(profile.id.clone());
        row.task_id = Some(task.id.clone());
        store.put_model_usage("run", &row).unwrap();
        let summary = || {
            summarize(
                UsageSummaryRequest {
                    from_ms: now,
                    to_ms: now + 1,
                    time_zone: "UTC".into(),
                },
                &store,
                |_| CredentialKind::None,
            )
            .unwrap()
        };
        let before = summary();
        assert_eq!(
            before.loupists[0].name.as_deref(),
            Some(profile.name.as_str())
        );
        assert_eq!(before.programs[0].task_id, task.id);
        assert_eq!(before.programs[0].title.as_deref(), Some("Weekly review"));
        assert_eq!(before.programs[0].runs, 1);
        store
            .scheduled_task_delete(&profile.id, &task.id, task.revision)
            .unwrap();
        assert!(summary().programs[0].title.is_none());
        store.delete_loupist(&profile).unwrap();
        let after = summary();
        assert!(after.programs.is_empty());
        assert_eq!(after.loupists[0].loupist_id, None);
        assert_eq!(after.totals.requests, 1);
    }

    #[test]
    fn programs_include_check_only_tasks_and_count_the_same_local_days_as_model_usage() {
        use crate::scheduled_tasks::types::{
            Schedule, TaskDraft, TaskStatus, TaskTarget, TriggerDayCount,
        };
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_test(dir.path().join("app"));
        let store = &state.store;
        crate::test_fixtures::create_folder_loupist(store, "usage", &dir.path().join("workspace"));
        let tick = |text: &str| {
            DateTime::parse_from_rfc3339(text)
                .unwrap()
                .timestamp_millis()
        };
        // Next year keeps the model fixture within the catalog retention window.
        let year = chrono::Datelike::year(&chrono::Utc::now()) + 1;
        let start = tick(&format!("{year}-01-02T00:00:00+09:00"));
        let end = tick(&format!("{year}-01-02T08:00:00+09:00"));
        let mut task = store
            .scheduled_task_create(
                "usage",
                TaskDraft {
                    title: "Check prices".into(),
                    prompt: "Explain the move.".into(),
                    schedule: Schedule::Interval { every_minutes: 60 },
                    target: TaskTarget::NewChat,
                    model_profile_id: String::new(),
                    thinking_level: String::new(),
                    run_options: BTreeMap::new(),
                    use_memory: false,
                    update_memory: false,
                    status: TaskStatus::Paused,
                },
                start,
            )
            .unwrap();
        let mut check = crate::commands::scheduled_tasks::preset_trigger(
            &state,
            "usage",
            "price-band",
            &Default::default(),
            "Check prices",
        )
        .unwrap();
        let mut output = crate::scheduled_tasks::trigger::errored("Unavailable", vec![], 0);
        output.outcome = crate::scheduled_tasks::types::TriggerOutcome::NoData;
        crate::scheduled_tasks::trigger::apply_output(&mut check, end - 1, end - 1, output, 0);
        // The clock in apply_output is device-local. Force the KST persisted
        // bucket for this fixture even when the test machine is in another zone.
        check.counters.today = TriggerDayCount {
            day: format!("{year}-01-02"),
            evaluations: 40,
            reads: 80,
            met: 0,
        };
        check.counters.recent = vec![TriggerDayCount {
            day: format!("{year}-01-01"),
            evaluations: 11,
            reads: 22,
            met: 1,
        }];
        task.trigger = Some(check);
        let id = task.id.clone();
        let revision = task.revision;
        store.scheduled_task_write(task, revision, end).unwrap();
        let summary = |from_ms, to_ms, time_zone: &str| {
            summarize(
                UsageSummaryRequest {
                    from_ms,
                    to_ms,
                    time_zone: time_zone.into(),
                },
                store,
                |_| CredentialKind::None,
            )
            .unwrap()
        };

        for (from, to, zone, expected) in [
            (start, end, "Asia/Seoul", (40, 80, 0)),
            (start - 86_400_000, start, "Asia/Seoul", (11, 22, 1)),
            (start - 86_400_000, end, "Asia/Seoul", (51, 102, 1)),
            (start, end, "UTC", (11, 22, 1)),
        ] {
            let usage = summary(from, to, zone);
            let [program] = usage.programs.as_slice() else {
                panic!("expected the check-only program")
            };
            let checks = program.checks.as_ref().unwrap();
            assert_eq!(program.task_id, id);
            assert_eq!(program.totals.requests, 0);
            assert_eq!((checks.evaluations, checks.reads, checks.met), expected);
            assert_eq!(checks.last_check_at_ms, Some(end - 1));
        }
        let mut row = fixture_row(start);
        row.loupist_id = Some("usage".into());
        row.task_id = Some(id);
        row.data.measures.requests = Some(3);
        store.put_model_usage("model-run", &row).unwrap();
        let usage = summary(start, end, "Asia/Seoul");
        assert_eq!(usage.programs[0].checks.as_ref().unwrap().evaluations, 40);
        assert_eq!(usage.programs[0].totals.requests, 3);
        assert_eq!(usage.days[0].date, format!("{year}-01-02"));
        assert_eq!(
            usage.days[0].you + usage.days[0].background + usage.days[0].world,
            3
        );
    }

    #[test]
    fn usage_rejects_overflows_invalid_ranges_and_non_iana_zones() {
        for (from_ms, to_ms, time_zone) in [
            (0, 0, "UTC"),
            (1, 0, "UTC"),
            (0, USAGE_RETENTION_MS + 1, "UTC"),
            (i64::MIN, i64::MAX, "UTC"),
            (i64::MAX - 1, i64::MAX, "UTC"),
            (0, 1, "+09:00"),
        ] {
            assert!(UsageSummaryRequest {
                from_ms,
                to_ms,
                time_zone: time_zone.into()
            }
            .dates()
            .is_err());
        }
        assert!(
            serde_json::from_value::<UsageSummaryRequest>(serde_json::json!({
                "fromMs": 0, "toMs": 1, "timeZone": "UTC", "other": true,
            }))
            .is_err()
        );
    }
}
