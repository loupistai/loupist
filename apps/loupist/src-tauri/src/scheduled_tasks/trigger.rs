//! A trigger is a pinned script the host runs on the task's schedule.
//! The approved hash is the only code an evaluation runs. A pending version waits.
use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};

use super::{
    schedule::next_run_after,
    types::{
        invalid, RunTrigger, Schedule, Task, TaskTrigger, TriggerActiveHours, TriggerApproval,
        TriggerCounters, TriggerDayCount, TriggerDryDay, TriggerDryRun, TriggerEvaluation,
        TriggerOutcome, TriggerPhase, TriggerScoreDay, TriggerScores, TriggerScriptRef,
        TriggerToolGrant, MAX_STORED_EVALUATIONS, MAX_TRIGGERED_RUNS_PER_LOUPIST_DAY,
        MAX_TRIGGER_AGENT_RUNS_PER_DAY, MAX_TRIGGER_ARGS_BYTES, MAX_TRIGGER_EVIDENCE_BYTES,
        MAX_TRIGGER_READING_CHARS, MAX_TRIGGER_READS_PER_DAY, MAX_TRIGGER_STATE_BYTES,
        TRIGGER_FAILURE_STREAK,
    },
};
use crate::store::StoreResult;

const WEB_TOOLS: &[&str] = &["web_search", "web_fetch"];

/// What one sealed execution returned, before the host applies ceilings.
#[derive(Clone, Debug)]
pub(crate) struct CheckOutput {
    pub outcome: TriggerOutcome,
    pub reading: String,
    pub as_of: Option<String>,
    pub evidence: Value,
    pub state: Value,
    pub error: Option<String>,
    pub reads: u32,
    pub tools: Vec<TriggerToolGrant>,
    pub ledger: Vec<Value>,
    pub retry_after_seconds: Option<u64>,
    pub refused_tool: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TriggerEffect {
    pub post_card: bool,
    pub start_run: bool,
    /// Why a met check did not start the agent. The card still carries the reading.
    pub no_run_reason: Option<&'static str>,
    pub stop_evaluating: bool,
}

pub(crate) struct LoupistTriggerLoad {
    pub reads_today: u32,
    pub agent_runs_today: u32,
    pub projected_daily_reads: [u32; 7],
    pub active: usize,
}

pub(crate) fn load_counts<'a>(
    tasks: impl IntoIterator<Item = &'a Task>,
    excluding: Option<&str>,
    now: i64,
) -> LoupistTriggerLoad {
    let day = local_day(now);
    let mut load = LoupistTriggerLoad {
        reads_today: 0,
        agent_runs_today: 0,
        projected_daily_reads: [0; 7],
        active: 0,
    };
    for task in tasks {
        let Some(trigger) = &task.trigger else {
            continue;
        };
        if task.replaces.is_some() {
            continue;
        }
        if trigger.state.evaluates()
            && task.status == super::types::TaskStatus::Active
            && excluding != Some(task.id.as_str())
        {
            load.active += 1;
            // Due polling only needs actual usage. Capacity checks project the week.
            if excluding.is_some() {
                for (total, reads) in
                    load.projected_daily_reads
                        .iter_mut()
                        .zip(projected_daily_reads(
                            &task.schedule,
                            trigger.active_hours.as_ref(),
                            trigger.reads_per_check,
                            now,
                        ))
                {
                    *total = total.saturating_add(reads);
                }
            }
        }
        if trigger.counters.today.day == day {
            load.reads_today = load.reads_today.saturating_add(reads_today(trigger, now));
            load.agent_runs_today = load
                .agent_runs_today
                .saturating_add(trigger.counters.agent_runs_today);
        }
    }
    load
}

pub(crate) fn read_allowance(reads_today: u32) -> u32 {
    MAX_TRIGGER_READS_PER_DAY
        .saturating_sub(reads_today)
        .min(super::types::MAX_TRIGGER_READS_PER_EVALUATION as u32)
}

pub(crate) fn set_price_subject_args(
    args: &mut Map<String, Value>,
    subjects: Option<&[super::types::TaskSubject]>,
) {
    args.remove("asset_ids");
    args.remove("asset_names");
    let mut ids = Vec::new();
    let mut names = Vec::new();
    for subject in subjects.unwrap_or_default() {
        match subject {
            super::types::TaskSubject::Asset { asset } => ids.push(asset),
            super::types::TaskSubject::Name { name } => names.push(name),
        }
    }
    if !ids.is_empty() {
        args.insert("asset_ids".into(), serde_json::json!(ids));
    }
    if !names.is_empty() {
        args.insert("asset_names".into(), serde_json::json!(names));
    }
}

impl TaskTrigger {
    pub(crate) fn reset_script_state(&mut self) {
        self.script_state = serde_json::json!({});
        self.replay_state_known = true;
    }

    pub(super) fn compact_latest_evidence(&mut self) {
        if let Some(last) = &mut self.last {
            last.replay_seed = None;
            last.ledger = successful_read_sources(&last.ledger)
                .into_iter()
                .map(|tool| serde_json::json!({"call":{"tool":tool},"response":{"status":"ok"}}))
                .collect();
        }
    }

    pub(crate) fn configuration(&self) -> super::types::TriggerConfiguration {
        super::types::TriggerConfiguration {
            summary: Some(self.summary.clone()),
            args: self.args.clone(),
            tools: self.tools.clone(),
            reads_per_check: self.reads_per_check,
            active_hours: self.active_hours.clone(),
            price_subjects: None,
        }
    }

    pub(crate) fn set_configuration(&mut self, change: super::types::TriggerConfiguration) {
        if let Some(summary) = change.summary {
            self.summary = summary;
        }
        self.args = change.args;
        self.tools = change.tools;
        self.reads_per_check = change.reads_per_check;
        self.active_hours = change.active_hours;
    }

    pub(crate) fn set_active_hours(&mut self, hours: Option<TriggerActiveHours>) {
        self.active_hours = hours;
        if let Some(mut approval) = self.approved.clone() {
            if let Some(configuration) = &mut approval.configuration {
                configuration.active_hours = self.active_hours.clone();
                remember_approval(self, approval.clone());
                self.approved = Some(approval);
            }
        }
    }

    pub(crate) fn validate(&self) -> StoreResult<()> {
        crate::store::scripts::validate_name(&self.script.name)?;
        if self.script.version == 0
            || self.script.sha256.len() != 64
            || !self
                .script
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(invalid("a check names one saved script version"));
        }
        validate_summary(&self.summary)?;
        if serde_json::to_vec(&self.args)
            .map_err(|_| invalid("check arguments are not valid JSON"))?
            .len()
            > MAX_TRIGGER_ARGS_BYTES
        {
            return Err(invalid("check arguments are at most 2 KiB"));
        }
        if serde_json::to_vec(&self.script_state)
            .map_err(|_| invalid("check state is not valid JSON"))?
            .len()
            > MAX_TRIGGER_STATE_BYTES
        {
            return Err(invalid("check state is at most 8 KiB"));
        }
        if self.tools.len() > 32 || self.tools.iter().any(|tool| !tool_name_ok(&tool.name)) {
            return Err(invalid("a check names the tools its dry run called"));
        }
        if let Some(hours) = &self.active_hours {
            hours.validate()?;
        }
        if self.evaluations.len() > MAX_STORED_EVALUATIONS {
            return Err(invalid("a check keeps its last 20 evaluations"));
        }
        if self.stale_after_days == 0 || self.stale_after_days > 366 {
            return Err(invalid("stale_after is between 1 and 366 days"));
        }
        match self.state {
            TriggerPhase::Proposed => {
                if self.approved.is_some() {
                    return Err(invalid("a proposal has not been turned on"));
                }
            }
            TriggerPhase::On | TriggerPhase::Failing => {
                let approved = self
                    .approved
                    .as_ref()
                    .ok_or_else(|| invalid("a live check has an approved version"))?;
                if self.script.version != approved.version || self.script.sha256 != approved.sha256
                {
                    return Err(invalid("a live check runs its approved version"));
                }
            }
            TriggerPhase::UpdateProposed => {
                if self.approved.is_none() || self.pending.is_none() {
                    return Err(invalid(
                        "an update keeps the approved version and a pending one",
                    ));
                }
            }
        }
        if let Some(approved) = &self.approved {
            if let Some(configuration) = &approved.configuration {
                let mut current = self.configuration();
                if configuration.summary.is_none() {
                    current.summary = None;
                }
                current
                    .price_subjects
                    .clone_from(&configuration.price_subjects);
                if *configuration != current || !approved.test.as_ref().is_some_and(tested) {
                    return Err(invalid(
                        "the running check must use its reviewed configuration",
                    ));
                }
            }
        }
        if let Some(pending) = &self.pending {
            if pending.version == 0 || pending.sha256.len() != 64 {
                return Err(invalid("a pending check version is invalid"));
            }
            if let Some(summary) = pending
                .configuration
                .as_ref()
                .and_then(|plan| plan.summary.as_deref())
            {
                validate_summary(summary)?;
            }
        }
        Ok(())
    }

    /// The bytes an evaluation may run: the approved hash, never the pending one.
    pub(crate) fn approved_ref(&self) -> Option<&TriggerScriptRef> {
        let approved = self.approved.as_ref()?;
        (self.script.version == approved.version && self.script.sha256 == approved.sha256)
            .then_some(&self.script)
    }
}

pub(crate) fn validate_summary(summary: &str) -> StoreResult<()> {
    if summary.trim().is_empty()
        || summary.chars().count() > MAX_TRIGGER_READING_CHARS
        || summary.chars().any(char::is_control)
    {
        return Err(invalid(
            "what a check looks for is one line of at most 160 characters",
        ));
    }
    Ok(())
}

impl TriggerActiveHours {
    fn validate(&self) -> StoreResult<()> {
        let unique: std::collections::BTreeSet<_> = self.days.iter().collect();
        if self.days.is_empty()
            || unique.len() != self.days.len()
            || self.days.iter().any(|day| *day > 6)
            || !clock_ok(&self.start)
            || !clock_ok(&self.end)
            || self.start == self.end
            || self.timezone.parse::<Tz>().is_err()
        {
            return Err(invalid("active hours need distinct days and a timezone"));
        }
        Ok(())
    }
}

fn clock_ok(value: &str) -> bool {
    let Some((hour, minute)) = value.split_once(':') else {
        return false;
    };
    hour.len() == 2
        && minute.len() == 2
        && hour.parse::<u8>().ok().is_some_and(|hour| hour < 24)
        && minute.parse::<u8>().ok().is_some_and(|minute| minute < 60)
}

fn tool_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

/// A call outside the approved set, and every web call, is refused.
pub(crate) fn admit_call(allowed: &[TriggerToolGrant], tool: &str) -> Result<(), &'static str> {
    if WEB_TOOLS.contains(&tool) || !allowed.iter().any(|grant| grant.name == tool) {
        Err("not_approved")
    } else {
        Ok(())
    }
}

pub(crate) fn admit_grant(
    allowed: &[TriggerToolGrant],
    observed: &TriggerToolGrant,
    discover_providers: bool,
) -> bool {
    if discover_providers {
        admit_call(allowed, &observed.name).is_ok()
    } else {
        allowed.contains(observed)
    }
}

pub(crate) fn within_hours(hours: &TriggerActiveHours, now_ms: i64) -> bool {
    let Ok(zone) = hours.timezone.parse::<Tz>() else {
        return false;
    };
    let Some(local) = chrono::Utc
        .timestamp_millis_opt(now_ms)
        .single()
        .map(|time| time.with_timezone(&zone))
    else {
        return false;
    };
    // Sunday is 0, matching weekly schedules.
    let day = local.weekday().num_days_from_sunday() as u8;
    if !hours.days.contains(&day) {
        return false;
    }
    let minute = local.hour() * 60 + local.minute();
    let start = clock_minutes(&hours.start);
    let end = clock_minutes(&hours.end);
    if start < end {
        minute >= start && minute < end
    } else {
        minute >= start || minute < end
    }
}

fn clock_minutes(value: &str) -> u32 {
    let (hour, minute) = value.split_once(':').unwrap_or(("0", "0"));
    hour.parse::<u32>().unwrap_or(0) * 60 + minute.parse::<u32>().unwrap_or(0)
}

pub(crate) fn local_day(now_ms: i64) -> String {
    crate::local_time::at_ms(now_ms)
        .map(|time| time.date_naive().to_string())
        .unwrap_or_else(|| "1970-01-01".into())
}

/// Reads per local date over the capacity window, starting with the whole current day.
fn projected_daily_reads(
    schedule: &Schedule,
    hours: Option<&TriggerActiveHours>,
    reads_per_check: u32,
    now_ms: i64,
) -> [u32; 7] {
    const MINUTE: i64 = 60_000;
    const DAY: i64 = 24 * 60 * MINUTE;
    let mut daily = [0u32; 7];
    let Some(first) = crate::local_time::date_at_ms(now_ms) else {
        return daily;
    };
    let midnight = first
        .and_hms_opt(0, 0, 0)
        .expect("midnight")
        .and_utc()
        .timestamp_millis();
    // Scan beyond the local dates in UTC to cover every time zone and DST transition.
    let start = midnight.saturating_sub(DAY * 2).max(0);
    let end = midnight.saturating_add(DAY * 9);
    let mut next_allowed = [start; 7];
    let mut count = |instant, gap| {
        if hours.is_some_and(|hours| !within_hours(hours, instant)) {
            return;
        }
        let Some(index) = crate::local_time::date_at_ms(instant)
            .and_then(|date| usize::try_from((date - first).num_days()).ok())
            .filter(|&index| index < daily.len())
        else {
            return;
        };
        if instant >= next_allowed[index] {
            daily[index] = daily[index].saturating_add(reads_per_check.max(1));
            next_allowed[index] = instant.saturating_add(gap);
        }
    };
    if let Schedule::Interval { every_minutes } = schedule {
        // Interval runs can shift with late evaluations. Count the maximum
        // number that can fit on each date at the configured minimum spacing.
        let gap = i64::from((*every_minutes).max(1)) * MINUTE;
        for instant in (start..end).step_by(MINUTE as usize) {
            count(instant, gap);
        }
    } else {
        let mut cursor = start;
        while let Ok(Some(next)) = next_run_after(schedule, cursor) {
            if next >= end {
                break;
            }
            count(next, 0);
            cursor = next;
        }
    }
    daily
}

pub(crate) fn turn_on_allowed(
    load: &LoupistTriggerLoad,
    schedule: &Schedule,
    trigger: &TaskTrigger,
    now: i64,
) -> StoreResult<()> {
    if load.active >= super::types::MAX_ACTIVE_TRIGGERS {
        return Err(invalid("this Loupist already has 10 checks turned on"));
    }
    let projected = projected_daily_reads(
        schedule,
        trigger.active_hours.as_ref(),
        trigger.reads_per_check,
        now,
    );
    if load
        .projected_daily_reads
        .iter()
        .zip(projected)
        .any(|(existing, next)| existing.saturating_add(next) > MAX_TRIGGER_READS_PER_DAY)
    {
        return Err(invalid(
            "turning this check on would pass 2,000 data reads a day",
        ));
    }
    Ok(())
}

pub(crate) fn successful_read_sources(ledger: &[Value]) -> Vec<String> {
    ledger
        .iter()
        .filter(|entry| entry["response"]["status"] == "ok")
        .filter_map(|entry| entry["call"]["tool"].as_str())
        .filter(|tool| tool_name_ok(tool))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn contains_figure(value: &Value) -> bool {
    match value {
        Value::Number(_) => true,
        Value::String(text) => text.chars().any(char::is_numeric),
        Value::Array(items) => items.iter().any(contains_figure),
        Value::Object(fields) => fields.values().any(contains_figure),
        _ => false,
    }
}

const FIGURE_WITHOUT_SOURCE: &str = "A check with a figure needs a successful data read.";

fn unsourced_figure(
    outcome: TriggerOutcome,
    reading: &str,
    evidence: &Value,
    ledger: &[Value],
) -> bool {
    matches!(outcome, TriggerOutcome::Met | TriggerOutcome::NotMet)
        && (reading.chars().any(char::is_numeric) || contains_figure(evidence))
        && successful_read_sources(ledger).is_empty()
}

/// Record one evaluation and decide whether a card and an agent run follow.
/// The caller advances `next_run` from the returned schedule hint.
pub(crate) fn apply_output(
    trigger: &mut TaskTrigger,
    now_ms: i64,
    scheduled_for_ms: i64,
    output: CheckOutput,
    loupist_agent_runs_today: u32,
) -> TriggerEffect {
    let output = if unsourced_figure(
        output.outcome,
        &output.reading,
        &output.evidence,
        &output.ledger,
    ) {
        errored(FIGURE_WITHOUT_SOURCE, output.ledger, output.reads)
    } else {
        output
    };
    roll_day(&mut trigger.counters, now_ms);
    let mut outcome = output.outcome;
    let mut error = output.error;
    if let Some(tool) = &output.refused_tool {
        outcome = TriggerOutcome::Error;
        error = Some(format!(
            "it tried to read {tool}, which you did not approve"
        ));
    }
    if matches!(outcome, TriggerOutcome::Met | TriggerOutcome::NotMet)
        && stale_or_advance(trigger, output.as_of.as_deref(), now_ms)
    {
        outcome = TriggerOutcome::Stale;
    }
    let mut evaluation = TriggerEvaluation {
        at_ms: now_ms,
        scheduled_for_ms,
        outcome,
        reading: output
            .reading
            .chars()
            .take(MAX_TRIGGER_READING_CHARS)
            .collect(),
        as_of: output.as_of,
        evidence: output.evidence,
        version: trigger.script.version,
        sha256: trigger.script.sha256.clone(),
        reads: output.reads,
        error,
        coalesced: now_ms.saturating_sub(scheduled_for_ms) > 60_000,
        ledger: output.ledger,
        card_message_id: None,
        retry_after_seconds: output.retry_after_seconds,
        replay_seed: None,
    };
    let can_replay = evaluation.can_replay();
    if can_replay && trigger.replay_state_known {
        evaluation.replay_seed = Some(trigger.script_state.clone());
    }
    if outcome.updates_state() {
        trigger.script_state = output.state;
        // This checkpoint must outlive both count- and byte-based history pruning.
        trigger.replay_state_known &= can_replay;
    }
    trigger.counters.today.evaluations = trigger.counters.today.evaluations.saturating_add(1);
    trigger.counters.today.reads = trigger.counters.today.reads.saturating_add(output.reads);
    if outcome == TriggerOutcome::Met {
        trigger.counters.today.met = trigger.counters.today.met.saturating_add(1);
    }
    match outcome {
        TriggerOutcome::Error => {
            trigger.counters.error_streak = trigger.counters.error_streak.saturating_add(1);
            trigger.counters.no_data_streak = 0;
        }
        TriggerOutcome::NoData => {
            trigger.counters.no_data_streak = trigger.counters.no_data_streak.saturating_add(1);
            trigger.counters.error_streak = 0;
        }
        _ => {
            trigger.counters.error_streak = 0;
            trigger.counters.no_data_streak = 0;
        }
    }
    let retry_after = output
        .retry_after_seconds
        .map(|seconds| seconds.saturating_mul(1000));
    if outcome == TriggerOutcome::RateLimited {
        let previous = trigger.counters.backoff_ms;
        let doubled = previous
            .max(5 * 60 * 1000)
            .saturating_mul(2)
            .min(60 * 60 * 1000);
        trigger.counters.backoff_ms = match (previous, retry_after) {
            (0, Some(hint)) => hint.max(1),
            (_, Some(hint)) => doubled.max(previous).max(hint),
            (_, None) => doubled.max(previous),
        };
    } else {
        trigger.counters.backoff_ms = retry_after.unwrap_or_default();
    }
    let stop = trigger.counters.error_streak >= TRIGGER_FAILURE_STREAK;
    if stop {
        trigger.state = TriggerPhase::Failing;
    }
    let ceiling = trigger.counters.agent_runs_today >= MAX_TRIGGER_AGENT_RUNS_PER_DAY
        || loupist_agent_runs_today >= MAX_TRIGGERED_RUNS_PER_LOUPIST_DAY;
    let start = outcome == TriggerOutcome::Met && !ceiling && !stop;
    if start {
        trigger.counters.agent_runs_today = trigger.counters.agent_runs_today.saturating_add(1);
    }
    // A met check posts the card even when the agent does not start. Stale data does not.
    let post_card = outcome == TriggerOutcome::Met;
    let no_run_reason = if outcome == TriggerOutcome::Met && !start {
        Some(if ceiling {
            "Today's limit for this check has been reached, so there is no explanation."
        } else {
            "The check stopped after repeated errors, so there is no explanation."
        })
    } else {
        None
    };
    push_evaluation(trigger, evaluation);
    TriggerEffect {
        post_card,
        start_run: start,
        no_run_reason,
        stop_evaluating: stop,
    }
}

fn push_evaluation(trigger: &mut TaskTrigger, evaluation: TriggerEvaluation) {
    trigger.evaluations.insert(0, evaluation.clone());
    trigger.evaluations.truncate(MAX_STORED_EVALUATIONS);
    trigger.last = Some(evaluation);
}

fn data_timestamp(value: &str) -> Option<DateTime<Utc>> {
    if value.len() == 10 {
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .ok()?
            .and_hms_opt(0, 0, 0)
            .map(|date| date.and_utc())
    } else {
        DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|date| date.to_utc())
    }
}

fn stale_or_advance(trigger: &mut TaskTrigger, as_of: Option<&str>, now_ms: i64) -> bool {
    let Some((as_of, timestamp)) = as_of.and_then(|value| Some((value, data_timestamp(value)?)))
    else {
        return true;
    };
    let prior = trigger
        .counters
        .as_of_value
        .as_deref()
        .and_then(data_timestamp);
    match prior {
        Some(prior) if timestamp < prior => true,
        Some(prior) if timestamp == prior => trigger.counters.as_of_since_ms.is_some_and(|since| {
            let days = i64::from(trigger.stale_after_days.max(1));
            now_ms.saturating_sub(since) > days * 24 * 60 * 60 * 1000
        }),
        _ => {
            trigger.counters.as_of_value = Some(as_of.to_owned());
            trigger.counters.as_of_since_ms = Some(now_ms);
            false
        }
    }
}

pub(crate) fn roll_day(counters: &mut TriggerCounters, now_ms: i64) {
    let day = local_day(now_ms);
    if counters.today.day == day {
        return;
    }
    if !counters.today.day.is_empty() {
        counters.recent.insert(0, counters.today.clone());
        counters.recent.truncate(30);
    }
    counters.today = TriggerDayCount {
        day,
        evaluations: 0,
        reads: 0,
        met: 0,
    };
    counters.agent_runs_today = 0;
    counters.reads_blocked = false;
}

pub(crate) fn reads_today(trigger: &TaskTrigger, now_ms: i64) -> u32 {
    if trigger.counters.today.day == local_day(now_ms) {
        trigger.counters.today.reads
    } else {
        0
    }
}

pub(crate) fn window_totals(
    trigger: &TaskTrigger,
    from_day: &str,
    to_day: &str,
) -> (u32, u32, u32) {
    let mut evaluations = 0;
    let mut reads = 0;
    let mut met = 0;
    for day in std::iter::once(&trigger.counters.today).chain(trigger.counters.recent.iter()) {
        if day.day.as_str() >= from_day && day.day.as_str() <= to_day {
            evaluations += day.evaluations;
            reads += day.reads;
            met += day.met;
        }
    }
    (evaluations, reads, met)
}

/// The next time the schedule is inside active hours, within a week of steps.
pub(crate) fn next_open(
    schedule: &Schedule,
    hours: Option<&TriggerActiveHours>,
    after_ms: i64,
) -> StoreResult<Option<i64>> {
    let mut cursor = after_ms;
    for _ in 0..8 * 24 * 12 {
        let Some(next) = next_run_after(schedule, cursor)? else {
            return Ok(None);
        };
        if hours.is_none_or(|hours| within_hours(hours, next)) {
            return Ok(Some(next));
        }
        cursor = next;
    }
    next_run_after(schedule, after_ms)
}

/// A skipped slot is recorded and does not count as a code failure.
pub(crate) fn note_skipped(
    trigger: &mut TaskTrigger,
    now_ms: i64,
    scheduled_for_ms: i64,
    outcome: TriggerOutcome,
    error: Option<String>,
) {
    roll_day(&mut trigger.counters, now_ms);
    push_evaluation(
        trigger,
        TriggerEvaluation {
            at_ms: now_ms,
            scheduled_for_ms,
            outcome,
            reading: String::new(),
            as_of: None,
            evidence: Value::Null,
            version: trigger.script.version,
            sha256: trigger.script.sha256.clone(),
            reads: 0,
            error,
            coalesced: now_ms.saturating_sub(scheduled_for_ms) > 60_000,
            ledger: Vec::new(),
            card_message_id: None,
            retry_after_seconds: None,
            replay_seed: None,
        },
    );
}

pub(crate) fn backoff_until(trigger: &TaskTrigger, now_ms: i64) -> Option<i64> {
    let gap = i64::try_from(trigger.counters.backoff_ms).unwrap_or(i64::MAX);
    (gap > 0).then(|| now_ms.saturating_add(gap))
}

/// Classify a sealed worker result. A missing or oversized verdict is a code error.
pub(crate) fn output_from_worker(result: &Value, ledger: Vec<Value>, reads: u32) -> CheckOutput {
    let status = result
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("failed");
    let failure = result.get("check_failure");
    let refused_tool = failure
        .and_then(|failure| {
            (failure["kind"] == "refused" && failure["code"] == "not_approved").then_some(())
        })
        .and_then(|_| {
            ledger.iter().rev().find_map(|entry| {
                (entry["response"]["status"] == "refused"
                    && entry["response"]["code"] == "not_approved")
                    .then(|| {
                        entry["call"]["tool"]
                            .as_str()
                            .unwrap_or("a tool")
                            .to_owned()
                    })
            })
        });
    if status == "timed_out" {
        return errored("The check took more than 20 seconds.", ledger, reads);
    }
    if result.get("stdout_truncated") == Some(&Value::Bool(true)) {
        return errored("The check exceeded its output size.", ledger, reads);
    }
    if let Some(tool) = refused_tool {
        let mut output = errored("", ledger, reads);
        output.refused_tool = Some(tool);
        return output;
    }
    let rate_limits = || {
        ledger
            .iter()
            .map(|entry| &entry["response"])
            .filter(|response| {
                response["status"] == "unavailable" && response["code"] == "rate_limited"
            })
    };
    let retry_hint = rate_limits()
        .filter_map(|response| response["retry_after"].as_u64())
        .max();
    if let Some(failure) = failure {
        let kind = failure["kind"].as_str().unwrap_or("error");
        let code = failure["code"].as_str().unwrap_or("error");
        let line = failure["line"].as_str().unwrap_or(code).to_owned();
        if kind == "unavailable"
            && (code == "rate_limited" || (code == "no_data" && rate_limits().next().is_some()))
        {
            let retry_after_seconds = retry_hint.max(failure["retry_after"].as_u64());
            return CheckOutput {
                outcome: TriggerOutcome::RateLimited,
                reading: String::new(),
                as_of: None,
                evidence: Value::Null,
                state: Value::Object(Map::new()),
                error: Some(line),
                reads,
                tools: Vec::new(),
                ledger,
                retry_after_seconds,
                refused_tool: None,
            };
        }
        if kind == "unavailable" {
            return CheckOutput {
                outcome: TriggerOutcome::NoData,
                reading: String::new(),
                as_of: None,
                evidence: json_tool(code),
                state: Value::Object(Map::new()),
                error: Some(line),
                reads,
                tools: Vec::new(),
                ledger,
                retry_after_seconds: None,
                refused_tool: None,
            };
        }
        return errored(&line, ledger, reads);
    }
    if status != "completed" {
        return errored("The check did not complete.", ledger, reads);
    }
    let Some(verdict) = result.get("check_verdict") else {
        return errored("The check did not return a result.", ledger, reads);
    };
    let met = verdict.get("met").and_then(Value::as_bool);
    let reading = verdict.get("reading").and_then(Value::as_str).unwrap_or("");
    let as_of = verdict.get("as_of").and_then(Value::as_str).unwrap_or("");
    let evidence = verdict.get("evidence").cloned().unwrap_or(Value::Null);
    let state = verdict
        .get("state")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    let evidence_bytes = serde_json::to_vec(&evidence)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX);
    if met.is_none()
        || reading.is_empty()
        || reading.chars().count() > MAX_TRIGGER_READING_CHARS
        || as_of.len() > 32
        || data_timestamp(as_of).is_none()
        || evidence_bytes > MAX_TRIGGER_EVIDENCE_BYTES
        || !state.is_object()
    {
        return errored(
            "The check returned a result of the wrong shape.",
            ledger,
            reads,
        );
    }
    if serde_json::to_vec(&state).map_or(true, |bytes| bytes.len() > MAX_TRIGGER_STATE_BYTES) {
        return errored("The check state exceeded 8 KiB.", ledger, reads);
    }
    let outcome = if met.unwrap() {
        TriggerOutcome::Met
    } else {
        TriggerOutcome::NotMet
    };
    if unsourced_figure(outcome, reading, &evidence, &ledger) {
        return errored(FIGURE_WITHOUT_SOURCE, ledger, reads);
    }
    CheckOutput {
        outcome,
        reading: reading.to_owned(),
        as_of: Some(as_of.to_owned()),
        evidence,
        state,
        error: None,
        reads,
        tools: Vec::new(),
        ledger,
        retry_after_seconds: retry_hint,
        refused_tool: None,
    }
}

fn json_tool(code: &str) -> Value {
    serde_json::json!({"read": code})
}

pub(crate) fn errored(line: &str, ledger: Vec<Value>, reads: u32) -> CheckOutput {
    CheckOutput {
        outcome: TriggerOutcome::Error,
        reading: String::new(),
        as_of: None,
        evidence: Value::Null,
        state: Value::Object(Map::new()),
        error: Some(line.to_owned()),
        reads,
        tools: Vec::new(),
        ledger,
        retry_after_seconds: None,
        refused_tool: None,
    }
}

pub(crate) fn dry_run_from_days(
    today: &CheckOutput,
    days: &[TriggerDryDay],
    unreplayed: u32,
) -> TriggerDryRun {
    TriggerDryRun {
        today_met: today.outcome == TriggerOutcome::Met,
        today_reading: if today.reading.is_empty() {
            today
                .error
                .clone()
                .unwrap_or_else(|| "The check did not finish.".into())
        } else {
            today.reading.clone()
        },
        today_outcome: today.outcome,
        fired_on: days
            .iter()
            .filter(|day| day.outcome == TriggerOutcome::Met)
            .map(|day| day.date.clone())
            .collect(),
        replayed: days.len() as u32,
        unreplayed,
        days: days.to_vec(),
        reads: today.reads.saturating_add(days.len() as u32),
        error: today.error.clone(),
        scores: None,
    }
}

pub(crate) fn tested(dry: &TriggerDryRun) -> bool {
    matches!(
        dry.today_outcome,
        TriggerOutcome::Met | TriggerOutcome::NotMet
    ) && dry.error.is_none()
        && dry.scores.is_some()
}

impl TriggerEvaluation {
    pub(crate) fn can_replay(&self) -> bool {
        !self.ledger.is_empty() || (self.reads == 0 && self.outcome.updates_state())
    }
}

pub(crate) fn score_replays(
    previous: &[TriggerEvaluation],
    mut next: impl FnMut(&TriggerEvaluation) -> Option<CheckOutput>,
) -> TriggerScores {
    let mut scores = TriggerScores {
        previous_met: 0,
        next_met: 0,
        compared: 0,
        days: Vec::new(),
    };
    for evaluation in previous.iter().take(MAX_STORED_EVALUATIONS) {
        let replayed = next(evaluation);
        if !evaluation.outcome.updates_state() {
            continue;
        }
        // Keep a row for every successful live check. NoData here also
        // represents a replay that could not be run with a known baseline.
        let outcome = replayed.map_or(TriggerOutcome::NoData, |output| output.outcome);
        scores.days.push(TriggerScoreDay {
            date: evaluation.as_of.clone().unwrap_or_default(),
            previous: evaluation.outcome,
            next: outcome,
        });
        if !outcome.updates_state() {
            continue;
        }
        scores.compared += 1;
        if evaluation.outcome == TriggerOutcome::Met {
            scores.previous_met += 1;
        }
        if outcome == TriggerOutcome::Met {
            scores.next_met += 1;
        }
    }
    scores
}

pub(crate) fn run_trigger_from(evaluation: &TriggerEvaluation) -> Option<RunTrigger> {
    Some(RunTrigger {
        version: evaluation.version,
        reading: evaluation.reading.clone(),
        as_of: evaluation.as_of.clone()?,
    })
}

/// Re-pin only a version this user has turned on before.
pub(crate) fn revert_target(
    trigger: &TaskTrigger,
    to_version: u32,
) -> StoreResult<&TriggerApproval> {
    trigger
        .approved_versions
        .iter()
        .find(|approval| approval.version == to_version)
        .ok_or_else(|| invalid("go back only to a version that was turned on before"))
}

pub(crate) fn remember_approval(trigger: &mut TaskTrigger, approval: TriggerApproval) {
    trigger
        .approved_versions
        .retain(|prior| prior.version != approval.version || prior.sha256 != approval.sha256);
    trigger.approved_versions.push(approval);
}

#[cfg(test)]
pub(crate) fn reviewed_fixture() -> TriggerDryRun {
    TriggerDryRun {
        today_met: false,
        today_reading: "No match".into(),
        today_outcome: TriggerOutcome::NotMet,
        fired_on: Vec::new(),
        replayed: 0,
        unreplayed: 20,
        days: Vec::new(),
        reads: 2,
        error: None,
        scores: Some(score_replays(&[], |_| None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn trigger() -> TaskTrigger {
        TaskTrigger {
            script: TriggerScriptRef {
                name: "price-band".into(),
                version: 1,
                sha256: "ab".repeat(32),
            },
            summary: "Prices outside a 5 percent band".into(),
            tools: vec![TriggerToolGrant {
                name: "price".into(),
                provider: Some("saved".into()),
            }],
            args: Map::new(),
            active_hours: None,
            approved: Some(TriggerApproval {
                version: 1,
                sha256: "ab".repeat(32),
                at_ms: 1,
                configuration: None,
                test: None,
            }),
            pending: None,
            state: TriggerPhase::On,
            script_state: json!({}),
            replay_state_known: true,
            last: None,
            evaluations: Vec::new(),
            counters: TriggerCounters::default(),
            approved_versions: vec![TriggerApproval {
                version: 1,
                sha256: "ab".repeat(32),
                at_ms: 1,
                configuration: None,
                test: None,
            }],
            stale_after_days: 5,
            reads_per_check: 1,
            proposal_test: None,
        }
    }

    fn met(reading: &str) -> CheckOutput {
        CheckOutput {
            outcome: TriggerOutcome::Met,
            reading: reading.into(),
            as_of: Some("2026-11-24".into()),
            evidence: json!({"moved": 1}),
            state: json!({"a": "2026-11-24"}),
            error: None,
            reads: 2,
            tools: Vec::new(),
            ledger: vec![json!({"call":{"tool":"price"},"response":{"status":"ok"}})],
            retry_after_seconds: None,
            refused_tool: None,
        }
    }

    #[test]
    fn the_last_daily_read_is_the_entire_allowance_for_the_next_check() {
        assert_eq!(read_allowance(MAX_TRIGGER_READS_PER_DAY - 1), 1);
        assert_eq!(read_allowance(MAX_TRIGGER_READS_PER_DAY), 0);
        assert_eq!(
            read_allowance(0),
            super::super::types::MAX_TRIGGER_READS_PER_EVALUATION as u32
        );
    }

    #[test]
    fn a_live_evaluation_requires_the_reviewed_tool_and_provider_pair() {
        let reviewed = TriggerToolGrant {
            name: "price".into(),
            provider: Some("provider-a".into()),
        };
        let different = TriggerToolGrant {
            name: "price".into(),
            provider: Some("provider-b".into()),
        };
        assert!(admit_grant(
            std::slice::from_ref(&reviewed),
            &reviewed,
            false
        ));
        assert!(!admit_grant(
            std::slice::from_ref(&reviewed),
            &different,
            false
        ));
        assert!(admit_grant(&[reviewed], &different, true));
    }

    #[test]
    fn a_tool_outside_the_approved_set_is_refused_and_the_web_always_is() {
        let allowed = [TriggerToolGrant {
            name: "price".into(),
            provider: None,
        }];
        assert!(admit_call(&allowed, "price").is_ok());
        assert_eq!(admit_call(&allowed, "web_search"), Err("not_approved"));
        assert_eq!(admit_call(&allowed, "world_search"), Err("not_approved"));
    }

    #[test]
    fn not_met_starts_nothing_and_met_starts_one_run_until_the_daily_ceiling() {
        let mut check = trigger();
        let mut quiet = met("Northwind -1.2%");
        quiet.outcome = TriggerOutcome::NotMet;
        let effect = apply_output(&mut check, 1_000, 1_000, quiet, 0);
        assert!(!effect.post_card && !effect.start_run);
        assert_eq!(check.last.as_ref().unwrap().outcome, TriggerOutcome::NotMet);
        let effect = apply_output(&mut check, 2_000, 2_000, met("Northwind -7.4%"), 0);
        assert!(effect.post_card && effect.start_run);
        check.counters.agent_runs_today = 3;
        let effect = apply_output(&mut check, 3_000, 3_000, met("again"), 0);
        assert!(effect.post_card && !effect.start_run);
        assert!(effect.no_run_reason.unwrap().contains("limit"));
        let effect = apply_output(&mut check, 4_000, 4_000, met("loupist"), 6);
        assert!(!effect.start_run);
    }

    #[test]
    fn three_errors_make_the_trigger_failing_and_stop_it() {
        let mut check = trigger();
        for index in 1..=3 {
            let effect = apply_output(
                &mut check,
                index,
                index,
                errored("NameError: points", vec![], 0),
                0,
            );
            assert!(!effect.start_run);
            if index < 3 {
                assert!(!effect.stop_evaluating);
                assert_eq!(check.state, TriggerPhase::On);
            }
        }
        assert_eq!(check.state, TriggerPhase::Failing);
        assert!(!check.state.evaluates());
        assert!(check.last.unwrap().error.unwrap().contains("NameError"));
    }

    #[test]
    fn numeric_met_verdicts_need_a_successful_read_with_a_named_source() {
        let mut check = trigger();
        let result = json!({"status": "completed", "check_verdict": {
            "met": true, "reading": "Moved 7%", "as_of": "2026-11-24", "state": {"sent":true}
        }});
        for ledger in [
            vec![],
            vec![json!({"call":{"tool":"price"}, "response":{"status":"unavailable"}})],
            vec![json!({"response":{"status":"ok"}})],
        ] {
            let output = output_from_worker(&result, ledger, 1);
            assert_eq!(output.outcome, TriggerOutcome::Error);
            let effect = apply_output(&mut check, 1, 1, output, 0);
            assert!(!effect.start_run && !effect.post_card);
            assert_eq!(check.script_state, json!({}));
        }
        let ledger = vec![json!({"call":{"tool":"price"}, "response":{"status":"ok"}})];
        let output = output_from_worker(&result, ledger, 1);
        assert_eq!(output.outcome, TriggerOutcome::Met);
        assert_eq!(successful_read_sources(&output.ledger), ["price"]);
        assert!(apply_output(&mut check, 2, 2, output, 0).post_card);
        let mut result = result;
        result["check_verdict"]["reading"] = json!("Threshold crossed");
        result["check_verdict"]["evidence"] = json!({"ratio": 7});
        assert_eq!(
            output_from_worker(&result, vec![], 0).outcome,
            TriggerOutcome::Error
        );
    }

    #[test]
    fn a_numeric_not_met_verdict_without_successful_reads_is_a_code_error() {
        let output = output_from_worker(
            &json!({"status":"completed","check_verdict":{
                "met":false, "reading":"Alpha +1.0%", "as_of":"2026-11-24"
            }}),
            vec![],
            0,
        );
        assert_eq!(output.outcome, TriggerOutcome::Error);
        let dry = dry_run_from_days(&output, &[], 20);
        assert!(!dry.today_reading.contains("1.0%"));
        assert!(!tested(&dry));
        let mut check = trigger();
        apply_output(&mut check, 1, 1, output, 0);
        assert_eq!(check.last.unwrap().outcome, TriggerOutcome::Error);
    }

    #[test]
    fn failed_or_oversized_worker_verdicts_cannot_start_runs_or_replace_state() {
        let mut check = trigger();
        check.script_state = json!({"previous": "signal"});
        let mut result = json!({"status": "failed", "check_verdict": {
            "met": true, "reading": "triggered", "as_of": "2026-11-24", "state": {}
        }});
        let failed = output_from_worker(&result, vec![], 1);
        assert_eq!(failed.outcome, TriggerOutcome::Error);
        let effect = apply_output(&mut check, 1, 1, failed, 0);
        assert!(!effect.start_run);

        result["status"] = json!("completed");
        result["check_verdict"]["state"] = json!({"padding": "a".repeat(MAX_TRIGGER_STATE_BYTES)});
        for now in 2..=3 {
            let output = output_from_worker(&result, vec![], 1);
            assert_eq!(output.outcome, TriggerOutcome::Error);
            assert!(output.error.as_deref().unwrap().contains("8 KiB"));
            let effect = apply_output(&mut check, now, now, output, 0);
            assert!(!effect.start_run);
            assert_eq!(effect.stop_evaluating, now == 3);
        }
        assert_eq!(check.state, TriggerPhase::Failing);
        assert_eq!(check.script_state, json!({"previous": "signal"}));
    }

    #[test]
    fn unavailable_reads_preserve_the_last_successful_state() {
        let mut check = trigger();
        apply_output(&mut check, 1, 1, met("crossed"), 0);
        let last_good = check.script_state.clone();
        for (kind, outcome) in [
            ("no_data", TriggerOutcome::NoData),
            ("rate_limited", TriggerOutcome::RateLimited),
        ] {
            let output = output_from_worker(
                &json!({"status": "failed", "check_failure": {"kind":"unavailable","code":kind}}),
                vec![],
                1,
            );
            assert_eq!(output.outcome, outcome);
            assert!(!apply_output(&mut check, 2, 2, output, 0).start_run);
            assert_eq!(check.script_state, last_good);
        }
    }

    #[test]
    fn no_data_rate_limit_and_stale_follow_the_host_rules() {
        let mut check = trigger();
        let mut missing = errored("", vec![], 1);
        missing.outcome = TriggerOutcome::NoData;
        missing.error = Some("price".into());
        apply_output(&mut check, 1, 1, missing.clone(), 0);
        assert_eq!(check.counters.no_data_streak, 1);
        assert_eq!(check.counters.error_streak, 0);
        let mut limited = errored("", vec![], 0);
        limited.outcome = TriggerOutcome::RateLimited;
        limited.retry_after_seconds = Some(30);
        apply_output(&mut check, 2, 2, limited, 0);
        assert_eq!(check.counters.backoff_ms, 30_000);
        assert_eq!(check.counters.no_data_streak, 0);
        check.counters.as_of_value = Some("2026-11-19".into());
        check.counters.as_of_since_ms = Some(0);
        let mut old = met("still");
        old.as_of = Some("2026-11-19".into());
        let later = 6 * 24 * 60 * 60 * 1000;
        let effect = apply_output(&mut check, later, later, old, 0);
        assert_eq!(check.last.as_ref().unwrap().outcome, TriggerOutcome::Stale);
        assert!(!effect.start_run && !effect.post_card);
    }

    #[test]
    fn consecutive_rate_limits_increase_delay_and_honor_provider_hints() {
        let mut check = trigger();
        let mut limited = errored("", vec![], 1);
        limited.outcome = TriggerOutcome::RateLimited;
        for (index, hint, minutes) in [
            (1, None, 10),
            (2, None, 20),
            (3, Some(1800), 40),
            (4, Some(5400), 90),
            (5, None, 90),
        ] {
            limited.retry_after_seconds = hint;
            apply_output(&mut check, index, index, limited.clone(), 0);
            assert_eq!(backoff_until(&check, index), Some(index + minutes * 60_000));
        }
        apply_output(&mut check, 6, 6, met("resumed"), 0);
        assert_eq!(backoff_until(&check, 6), None);
    }

    #[test]
    fn a_regressing_or_invalid_timestamp_cannot_alert_or_reset_freshness() {
        let mut check = trigger();
        apply_output(&mut check, 1, 1, met("newest"), 0);
        let prior_state = check.script_state.clone();
        let later = 6 * 24 * 60 * 60 * 1000;
        for as_of in ["2026-11-23", "2026-11-31", "2026-11-24T09:00:00+09:00"] {
            let mut older = met("old");
            older.as_of = Some(as_of.to_owned());
            older.state = json!({"old": true});
            let effect = apply_output(&mut check, later, later, older, 0);
            assert!(!effect.post_card && !effect.start_run);
            assert_eq!(check.counters.as_of_since_ms, Some(1));
            assert_eq!(check.script_state, prior_state);
        }
        let mut newer = met("fresh");
        newer.as_of = Some("2026-11-25".into());
        assert!(apply_output(&mut check, later, later, newer, 0).start_run);
        assert_eq!(check.counters.as_of_since_ms, Some(later));
    }

    #[test]
    fn a_pending_version_is_not_the_one_that_runs_and_revert_keeps_to_an_approval() {
        let mut check = trigger();
        check.pending = Some(super::super::types::TriggerPending {
            version: 4,
            sha256: "cd".repeat(32),
            test: None,
            configuration: None,
        });
        check.state = TriggerPhase::UpdateProposed;
        let running = check.approved_ref().unwrap();
        assert_eq!(running.version, 1);
        assert!(revert_target(&check, 4).is_err());
        assert_eq!(revert_target(&check, 1).unwrap().version, 1);
        remember_approval(
            &mut check,
            TriggerApproval {
                version: 2,
                sha256: "ef".repeat(32),
                at_ms: 9,
                configuration: None,
                test: None,
            },
        );
        assert!(revert_target(&check, 2).is_ok());
    }

    #[test]
    fn stored_ledger_replays_score_the_previous_and_candidate_verdicts() {
        let stored = TriggerEvaluation {
            at_ms: 1,
            scheduled_for_ms: 1,
            outcome: TriggerOutcome::Met,
            reading: "Northwind -7.4%".into(),
            as_of: Some("2026-11-24".into()),
            evidence: json!({}),
            version: 1,
            sha256: "ab".repeat(32),
            reads: 1,
            error: None,
            coalesced: false,
            ledger: vec![json!({"id":"ledger"})],
            card_message_id: None,
            retry_after_seconds: None,
            replay_seed: None,
        };
        let scores = score_replays(std::slice::from_ref(&stored), |evaluation| {
            assert_eq!(evaluation.ledger[0]["id"], "ledger");
            let mut same = met("Northwind -7.4%");
            same.outcome = TriggerOutcome::NotMet;
            Some(same)
        });
        assert_eq!(scores.compared, 1);
        assert_eq!(scores.previous_met, 1);
        assert_eq!(scores.next_met, 0);
    }

    #[test]
    fn capacity_counts_weekly_occurrences_on_their_day_and_full_interval_windows() {
        let now = DateTime::parse_from_rfc3339("2026-11-25T12:00:00Z")
            .unwrap()
            .timestamp_millis();
        let zone = iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into());
        let weekly = |days| Schedule::Weekly {
            days,
            hour: 12,
            minute: 0,
            timezone: zone.clone(),
        };
        let counts = projected_daily_reads(&weekly(vec![1, 2, 3, 4, 5]), None, 30, now);
        assert_eq!(counts.iter().filter(|&&reads| reads == 30).count(), 5);
        assert_eq!(counts.iter().filter(|&&reads| reads == 0).count(), 2);
        let hours = TriggerActiveHours {
            days: vec![1],
            start: "09:00".into(),
            end: "17:00".into(),
            timezone: zone.clone(),
        };
        let interval = Schedule::Interval { every_minutes: 5 };
        let counts = projected_daily_reads(&interval, Some(&hours), 24, now);
        assert_eq!(counts.iter().sum::<u32>(), 96 * 24);
        let load = LoupistTriggerLoad {
            reads_today: 0,
            agent_runs_today: 0,
            active: 1,
            projected_daily_reads: projected_daily_reads(&weekly(vec![0]), None, 1990, now),
        };
        let mut check = trigger();
        check.reads_per_check = 24;
        assert!(turn_on_allowed(&load, &weekly(vec![1]), &check, now).is_ok());
        assert!(turn_on_allowed(&load, &weekly(vec![0]), &check, now).is_err());
        check.active_hours = Some(hours);
        assert!(turn_on_allowed(&load, &interval, &check, now).is_err());
    }

    #[test]
    fn the_daily_read_budget_refuses_turn_on_and_a_worker_timeout_is_a_code_error() {
        let check = trigger();
        let mut heavy = check.clone();
        heavy.reads_per_check = 24;
        let schedule = Schedule::Interval { every_minutes: 5 };
        let projected = projected_daily_reads(&schedule, None, 24, 1_800_000_000_000);
        assert!(projected
            .iter()
            .all(|&reads| reads > MAX_TRIGGER_READS_PER_DAY));
        let load = LoupistTriggerLoad {
            reads_today: 0,
            agent_runs_today: 0,
            projected_daily_reads: [0; 7],
            active: 0,
        };
        assert!(turn_on_allowed(&load, &schedule, &heavy, 1_800_000_000_000).is_err());
        assert!(turn_on_allowed(
            &load,
            &Schedule::Daily {
                hour: 9,
                minute: 0,
                timezone: "UTC".into(),
            },
            &check,
            1_800_000_000_000,
        )
        .is_ok());
        let output = output_from_worker(&json!({"status":"timed_out"}), vec![], 0);
        assert_eq!(output.outcome, TriggerOutcome::Error);
        assert!(output.error.unwrap().contains("20"));
        let output = output_from_worker(
            &json!({"status":"failed","check_failure":{"kind":"unavailable","code":"no_data","line":"price"}}),
            vec![],
            1,
        );
        assert_eq!(output.outcome, TriggerOutcome::NoData);
    }

    #[test]
    fn missed_time_is_one_coalesced_evaluation() {
        let mut check = trigger();
        let effect = apply_output(&mut check, 10 * 60_000, 0, met("later"), 0);
        assert!(effect.start_run);
        assert!(check.last.unwrap().coalesced);
        assert_eq!(check.evaluations.len(), 1);
    }
}
