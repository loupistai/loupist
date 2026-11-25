//! Host path for a check: dry run, approval, and the due evaluation.
use serde_json::{json, Map, Value};

use crate::{
    app::{AppState, CommandError},
    commands::{map_store, now_ms},
    scheduled_tasks::{
        self,
        trigger::{self, CheckOutput},
        types::{
            RunTrigger, Schedule, Task, TaskStatus, TaskSubject, TaskTrigger, TaskWatch,
            TriggerApproval, TriggerConfiguration, TriggerOutcome, TriggerPhase, TriggerScriptRef,
            WatchPreset, MIN_TRIGGER_INTERVAL_MINUTES,
        },
    },
    store::scripts::{ScriptDraft, ScriptOrigin},
};

pub(crate) struct TriggerContext {
    pub version: u32,
    pub reading: String,
    pub as_of: String,
    pub evidence: Value,
    pub summary: String,
}

pub(crate) enum DueCheck {
    /// Not a live check. The scheduler claims a model run as usual.
    Pass,
    /// The check was handled and no model run starts.
    Done,
    /// The check was met. Commit the evaluation and card with its explanation claim.
    Met(Box<TriggerOccurrence>),
}

pub(crate) struct TriggerOccurrence {
    pub task: Task,
    pub context: TriggerContext,
    pub run_trigger: RunTrigger,
    pub card: Option<crate::domain::ChatMessage>,
    pub notify: crate::scheduled_tasks::types::NotifyPolicy,
    pub start_run: bool,
    pub manual: bool,
}

pub(crate) async fn on_due(
    state: &AppState,
    loupist_id: &str,
    task_id: &str,
    now: i64,
    manual: bool,
) -> Result<DueCheck, CommandError> {
    let Some(mut task) = state
        .store
        .scheduled_task_get(loupist_id, task_id)
        .map_err(map_store)?
    else {
        return Ok(DueCheck::Pass);
    };
    let Some(trigger) = task.trigger.clone() else {
        return Ok(if task.is_trigger_task() {
            DueCheck::Done
        } else {
            DueCheck::Pass
        });
    };
    let due = task.next_run_at_ms.filter(|due| *due <= now);
    if task.status != TaskStatus::Active
        || task.awaiting_confirmation
        || !trigger.state.evaluates()
        || (!manual && due.is_none())
    {
        return Ok(DueCheck::Done);
    }
    if trigger
        .last
        .as_ref()
        .and_then(|last| trigger::backoff_until(&trigger, last.at_ms))
        .is_some_and(|retry_at| now < retry_at)
    {
        return Ok(DueCheck::Done);
    }
    let manual = due.is_none();
    let scheduled_for = due.unwrap_or(now);
    if let Some(hours) = trigger.active_hours.clone() {
        if !trigger::within_hours(&hours, now) {
            task.next_run_at_ms =
                trigger::next_open(&task.schedule, Some(&hours), now).map_err(map_store)?;
            write(state, task, now)?;
            return Ok(DueCheck::Done);
        }
    }
    let tasks = state
        .store
        .scheduled_tasks_list(loupist_id)
        .map_err(map_store)?;
    let load = trigger::load_counts(&tasks, None, now);
    let allowance = trigger::read_allowance(load.reads_today);
    if allowance == 0 {
        if let Some(trigger) = task.trigger.as_mut() {
            trigger.counters.reads_blocked = true;
        }
        task.next_run_at_ms = Some(now.saturating_add(24 * 60 * 60 * 1000));
        write(state, task, now)?;
        return Ok(DueCheck::Done);
    }
    let script = match approved_source(&state.store, loupist_id, &trigger) {
        Ok(script) => script,
        Err(error) => {
            return finish(
                state,
                task,
                now,
                scheduled_for,
                manual,
                errored(&error.to_string()),
                load.agent_runs_today,
            );
        }
    };
    let price_ids = scoped_price_ids(state, loupist_id, &task.watch)?;
    if price_ids.as_ref().is_some_and(Vec::is_empty) {
        let mut missing = errored("No matching asset is followed.");
        missing.outcome = TriggerOutcome::NoData;
        return finish(
            state,
            task,
            now,
            scheduled_for,
            manual,
            missing,
            load.agent_runs_today,
        );
    }
    // Reserve before handing the worker any reads. A crash or an uncounted failure
    // leaves the conservative reservation in the daily ledger.
    let counters = &mut task.trigger.as_mut().expect("trigger").counters;
    trigger::roll_day(counters, now);
    counters.today.reads = counters.today.reads.saturating_add(allowance);
    let mut task = write(state, task, now)?;
    let authority = super::capture_chat_connector_authority(state, loupist_id)?;
    let executor = super::tool_executor::AppToolExecutor::for_host_with_connector_authority(
        state, loupist_id, authority,
    );
    let output = match executor
        .run_check(super::tool_executor::CheckRun {
            code: &script.code,
            after_outside_text: script.summary.metadata.after_outside_text,
            use_memory: task.use_memory,
            args: scoped_price_args(&trigger.args, price_ids.as_deref()),
            state: trigger.script_state.clone(),
            now: chrono::DateTime::from_timestamp_millis(now)
                .ok_or_else(|| CommandError::invalid("Invalid check time"))?
                .to_rfc3339(),
            as_of: None,
            allowed: &trigger.tools,
            discover_providers: false,
            replay: None,
            max_rounds: crate::scheduled_tasks::types::MAX_TRIGGER_ROUNDS,
            max_calls: allowance as usize,
        })
        .await
    {
        Ok(output) => {
            let counters = &mut task.trigger.as_mut().expect("trigger").counters;
            counters.today.reads = counters.today.reads.saturating_sub(allowance);
            output
        }
        Err(error) if error.to_string().contains("timed out while waiting") => {
            if let Some(trigger) = task.trigger.as_mut() {
                trigger::note_skipped(
                    trigger,
                    now,
                    scheduled_for,
                    TriggerOutcome::SkippedBusy,
                    Some("The Python worker was busy.".into()),
                );
            }
            task.next_run_at_ms = trigger::next_open(
                &task.schedule,
                task.trigger.as_ref().and_then(|t| t.active_hours.as_ref()),
                now,
            )
            .map_err(map_store)?;
            write(state, task, now)?;
            return Ok(DueCheck::Done);
        }
        Err(error) => errored(&error.to_string()),
    };
    finish(
        state,
        task,
        now,
        scheduled_for,
        manual,
        output,
        load.agent_runs_today,
    )
}

fn finish(
    state: &AppState,
    mut task: Task,
    now: i64,
    scheduled_for: i64,
    manual: bool,
    output: CheckOutput,
    loupist_runs: u32,
) -> Result<DueCheck, CommandError> {
    let check = task.trigger.as_mut().expect("due trigger");
    let effect = trigger::apply_output(check, now, scheduled_for, output, loupist_runs);
    if !effect.post_card {
        task.next_run_at_ms =
            scheduled_tasks::store::next_trigger_slot(&task, now).map_err(map_store)?;
        if effect.stop_evaluating {
            task.next_run_at_ms = Some(now.saturating_add(365 * 24 * 60 * 60 * 1000));
        }
        write(state, task, now)?;
        return Ok(DueCheck::Done);
    }

    let last = check.last.as_ref().expect("met evaluation").clone();
    let run_trigger = trigger::run_trigger_from(&last).expect("met trigger has dated data");
    let context = TriggerContext {
        version: run_trigger.version,
        reading: run_trigger.reading.clone(),
        as_of: run_trigger.as_of.clone(),
        evidence: last.evidence.clone(),
        summary: check.summary.clone(),
    };
    let (destination, notify) = crate::briefings::desktop::resolve_trigger_delivery(state, &task);
    let card = if destination == crate::scheduled_tasks::types::DeliveryTarget::Bot {
        crate::chat::home::ensure_home_thread(state, &task.loupist_id)?;
        Some(crate::chat::home::trigger_card(
            &task,
            &last,
            effect.no_run_reason,
        )?)
    } else {
        None
    };
    if let Some(card) = &card {
        let check = task.trigger.as_mut().expect("due trigger");
        check.last.as_mut().expect("met evaluation").card_message_id = Some(card.id.clone());
        check.evaluations[0].card_message_id = Some(card.id.clone());
    }
    Ok(DueCheck::Met(Box::new(TriggerOccurrence {
        task,
        context,
        run_trigger,
        card,
        notify,
        start_run: effect.start_run,
        manual,
    })))
}

fn write(state: &AppState, task: Task, now: i64) -> Result<Task, CommandError> {
    let revision = task.revision;
    let saved = state
        .store
        .scheduled_task_write(task, revision, now)
        .map_err(map_store)?;
    state.scheduled.changed();
    Ok(saved)
}

fn errored(line: &str) -> CheckOutput {
    CheckOutput {
        outcome: TriggerOutcome::Error,
        reading: String::new(),
        as_of: None,
        evidence: Value::Null,
        state: json!({}),
        error: Some(line.to_owned()),
        reads: 0,
        tools: Vec::new(),
        ledger: Vec::new(),
        retry_after_seconds: None,
        refused_tool: None,
    }
}

pub(crate) fn approved_source(
    store: &crate::store::SqliteStore,
    loupist_id: &str,
    trigger: &TaskTrigger,
) -> Result<crate::store::scripts::ScriptVersion, CommandError> {
    let approved = trigger.approved_ref().ok_or_else(|| {
        CommandError::invalid("A pending version does not run until it is turned on.")
    })?;
    let script = store
        .script_get(loupist_id, &approved.name, Some(approved.version))
        .map_err(map_store)?;
    let digest = crate::util::sha256(script.code.as_bytes());
    if script.summary.sha256 != approved.sha256
        || digest != approved.sha256
        || script.summary.metadata.after_outside_text
    {
        return Err(CommandError::invalid(
            "The saved script does not match the approved check.",
        ));
    }
    Ok(script)
}

pub(crate) fn install_template(
    state: &AppState,
    loupist_id: &str,
    template_id: &str,
    summary: &str,
) -> Result<crate::store::scripts::ScriptVersion, CommandError> {
    let code = template_source(template_id)?;
    state
        .store
        .script_save(
            loupist_id,
            ScriptDraft {
                name: template_id.to_owned(),
                title: Some(summary.to_owned()),
                summary: summary.to_owned(),
                args: None,
                code,
            },
            ScriptOrigin::Template {
                id: template_id.to_owned(),
            },
            false,
        )
        .map_err(map_store)
}

pub(crate) fn template_source(id: &str) -> Result<String, CommandError> {
    let source = match id {
        "price-band" => include_str!("../../../agent/trigger-templates/price-band.py"),
        "volume-spike" => include_str!("../../../agent/trigger-templates/volume-spike.py"),
        "level-cross" => include_str!("../../../agent/trigger-templates/level-cross.py"),
        "new-item" => include_str!("../../../agent/trigger-templates/new-item.py"),
        _ => return Err(CommandError::invalid("Unknown check template")),
    };
    Ok(source.to_owned())
}

pub(crate) fn script_ref(version: &crate::store::scripts::ScriptVersion) -> TriggerScriptRef {
    TriggerScriptRef {
        name: version.summary.name.clone(),
        version: version.summary.version,
        sha256: version.summary.sha256.clone(),
    }
}

/// A library save of a live check becomes a pending version. The approved one keeps running.
pub(crate) fn note_saved_version(
    state: &AppState,
    loupist_id: &str,
    name: &str,
    version: u32,
    sha256: &str,
) -> Result<(), CommandError> {
    let now = now_ms();
    for task in state
        .store
        .scheduled_tasks_list(loupist_id)
        .map_err(map_store)?
    {
        let Some(trigger) = task.trigger.clone() else {
            continue;
        };
        if task.replaces.is_some() || trigger.script.name != name || trigger.approved.is_none() {
            continue;
        }
        if trigger.script.version == version {
            continue;
        }
        let mut task = task;
        let trigger = task.trigger.as_mut().expect("trigger");
        trigger.pending = Some(scheduled_tasks::types::TriggerPending {
            version,
            sha256: sha256.to_owned(),
            test: None,
            configuration: Some(TriggerConfiguration {
                summary: Some(trigger.summary.clone()),
                args: trigger.args.clone(),
                tools: Vec::new(),
                reads_per_check: 0,
                active_hours: trigger.active_hours.clone(),
                price_subjects: (task.watch.preset == Some(WatchPreset::PriceMove))
                    .then(|| task.watch.subjects.clone().unwrap_or_default()),
            }),
        });
        if trigger.state == TriggerPhase::On {
            trigger.state = TriggerPhase::UpdateProposed;
        }
        write(state, task, now)?;
    }
    Ok(())
}

pub(crate) fn approve(
    state: &AppState,
    loupist_id: &str,
    task_id: &str,
    expected_revision: u64,
    version: u32,
    sha256: &str,
) -> Result<Task, CommandError> {
    let mut task = state
        .store
        .scheduled_task_get(loupist_id, task_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("scheduled task"))?;
    if task.revision != expected_revision || task.declined_in_visit.is_some() {
        return Err(CommandError::conflict("The check proposal changed"));
    }
    if task.replaces.is_some() {
        return Err(CommandError::conflict(
            "Confirm this schedule edit from its proposal card",
        ));
    }
    let trigger = task
        .trigger
        .as_mut()
        .ok_or_else(|| CommandError::invalid("This task has no check"))?;
    let selected = if let Some(pending) = &trigger.pending {
        pending.version == version && pending.sha256 == sha256
    } else {
        task.awaiting_confirmation
            && trigger.state == TriggerPhase::Proposed
            && trigger.script.version == version
            && trigger.script.sha256 == sha256
    };
    if !selected {
        return Err(CommandError::conflict(
            "Turn on the version shown on the card",
        ));
    }
    let script = state
        .store
        .script_get(loupist_id, &trigger.script.name, Some(version))
        .map_err(map_store)?;
    if script.summary.sha256 != sha256
        || crate::util::sha256(script.code.as_bytes()) != sha256
        || script.summary.metadata.after_outside_text
    {
        return Err(CommandError::invalid(
            "The saved script does not match that version.",
        ));
    }
    let (configuration, test) = if let Some(pending) = &trigger.pending {
        (pending.configuration.clone(), pending.test.clone())
    } else {
        (Some(trigger.configuration()), trigger.proposal_test.clone())
    };
    let (mut configuration, test) = reviewed(configuration, test)?;
    configuration
        .summary
        .get_or_insert_with(|| trigger.summary.clone());
    if task.watch.preset == Some(WatchPreset::PriceMove) {
        let selected = if trigger.pending.is_some() {
            price_review_watch(&task.watch, &configuration)?
        } else {
            let mut shown_args = configuration.args.clone();
            trigger::set_price_subject_args(&mut shown_args, task.watch.subjects.as_deref());
            if shown_args != configuration.args
                || task
                    .watch
                    .terms
                    .as_ref()
                    .is_some_and(|terms| Some(terms) != configuration.summary.as_ref())
            {
                return Err(CommandError::conflict(
                    "Test the edited check before turning it on",
                ));
            }
            task.watch.clone()
        };
        configuration.price_subjects = Some(selected.subjects.unwrap_or_default());
    }
    if let Some(mut prior) = trigger.approved.clone() {
        if let Some(plan) = &mut prior.configuration {
            plan.summary.get_or_insert_with(|| trigger.summary.clone());
            if task.watch.preset == Some(WatchPreset::PriceMove) {
                plan.price_subjects
                    .get_or_insert_with(|| task.watch.subjects.clone().unwrap_or_default());
            }
            trigger::remember_approval(trigger, prior);
        }
    }
    if let Some(subjects) = &configuration.price_subjects {
        task.watch.subjects = Some(subjects.clone());
    }
    let now = now_ms();
    trigger.set_configuration(configuration.clone());
    let approval = TriggerApproval {
        version,
        sha256: sha256.to_owned(),
        at_ms: now,
        configuration: Some(configuration),
        test: Some(test.clone()),
    };
    trigger.proposal_test = Some(test);
    trigger.reset_script_state();
    trigger.script.version = version;
    trigger.script.sha256 = sha256.to_owned();
    trigger::remember_approval(trigger, approval.clone());
    trigger.approved = Some(approval);
    trigger.pending = None;
    trigger.state = TriggerPhase::On;
    task.watch.terms = Some(trigger.summary.clone());
    task.awaiting_confirmation = false;
    task.status = crate::scheduled_tasks::types::TaskStatus::Active;
    task.next_run_at_ms = trigger::next_open(
        &task.schedule,
        task.trigger.as_ref().and_then(|t| t.active_hours.as_ref()),
        now,
    )
    .map_err(map_store)?;
    if matches!(task.schedule, Schedule::Interval { every_minutes } if every_minutes < MIN_TRIGGER_INTERVAL_MINUTES)
    {
        return Err(CommandError::invalid(
            "a check repeats at least every 5 minutes",
        ));
    }
    let saved = state
        .store
        .scheduled_trigger_promote(task, expected_revision, now)
        .map_err(map_store)?;
    state.scheduled.changed();
    Ok(saved)
}

fn reviewed(
    configuration: Option<TriggerConfiguration>,
    test: Option<scheduled_tasks::types::TriggerDryRun>,
) -> Result<(TriggerConfiguration, scheduled_tasks::types::TriggerDryRun), CommandError> {
    configuration
        .filter(|plan| plan.reads_per_check > 0)
        .zip(test.filter(trigger::tested))
        .ok_or_else(|| {
            CommandError::conflict(
                "This check needs a successful dry run before it can be turned on",
            )
        })
}

pub(crate) fn revert(
    state: &AppState,
    loupist_id: &str,
    task_id: &str,
    to_version: u32,
) -> Result<Task, CommandError> {
    let mut task = state
        .store
        .scheduled_task_get(loupist_id, task_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("scheduled task"))?;
    let trigger = task
        .trigger
        .as_mut()
        .ok_or_else(|| CommandError::invalid("This task has no check"))?;
    let prior = trigger::revert_target(trigger, to_version)
        .map_err(map_store)?
        .clone();
    let script = state
        .store
        .script_get(loupist_id, &trigger.script.name, Some(prior.version))
        .map_err(map_store)?;
    if script.summary.sha256 != prior.sha256
        || crate::util::sha256(script.code.as_bytes()) != prior.sha256
        || script.summary.metadata.after_outside_text
    {
        return Err(CommandError::invalid(
            "That version is no longer in the library.",
        ));
    }
    let (configuration, test) = reviewed(prior.configuration.clone(), prior.test.clone())?;
    if let Some(subjects) = &configuration.price_subjects {
        task.watch.subjects = Some(subjects.clone());
    }
    trigger.set_configuration(configuration);
    trigger.proposal_test = Some(test);
    trigger.reset_script_state();
    trigger.script.version = prior.version;
    trigger.script.sha256 = prior.sha256.clone();
    trigger.approved = Some(prior);
    trigger.pending = None;
    if trigger.state != TriggerPhase::Proposed {
        trigger.state = TriggerPhase::On;
    }
    trigger.counters.error_streak = 0;
    task.watch.terms = Some(trigger.summary.clone());
    let now = now_ms();
    if task.status == crate::scheduled_tasks::types::TaskStatus::Active {
        task.next_run_at_ms = trigger::next_open(
            &task.schedule,
            task.trigger
                .as_ref()
                .and_then(|item| item.active_hours.as_ref()),
            now,
        )
        .map_err(map_store)?;
    }
    let revision = task.revision;
    let saved = state
        .store
        .scheduled_trigger_promote(task, revision, now)
        .map_err(map_store)?;
    state.scheduled.changed();
    Ok(saved)
}

pub(super) fn price_review_watch(
    watch: &TaskWatch,
    configuration: &TriggerConfiguration,
) -> Result<TaskWatch, CommandError> {
    let mut watch = watch.clone();
    if watch.preset != Some(WatchPreset::PriceMove) {
        return Ok(watch);
    }
    if let Some(subjects) = &configuration.price_subjects {
        watch.subjects = Some(subjects.clone());
    } else if configuration.args.contains_key("asset_ids")
        || configuration.args.contains_key("asset_names")
    {
        let mut subjects = Vec::new();
        for key in ["asset_ids", "asset_names"] {
            let Some(value) = configuration.args.get(key) else {
                continue;
            };
            let values: Vec<String> = serde_json::from_value(value.clone()).map_err(|_| {
                CommandError::invalid("Price subjects must be a list of names or asset IDs.")
            })?;
            subjects.extend(values.into_iter().map(|value| match key {
                "asset_ids" => TaskSubject::Asset { asset: value },
                _ => TaskSubject::Name { name: value },
            }));
        }
        if subjects.is_empty() {
            return Err(CommandError::invalid(
                "The price check needs a named subject or no subject filter.",
            ));
        }
        watch.subjects = Some(subjects);
    }
    watch.validate().map_err(map_store)?;
    Ok(watch)
}

pub(super) fn scoped_price_ids(
    state: &AppState,
    loupist_id: &str,
    watch: &scheduled_tasks::types::TaskWatch,
) -> Result<Option<Vec<String>>, CommandError> {
    if watch.preset != Some(scheduled_tasks::types::WatchPreset::PriceMove) {
        return Ok(None);
    }
    let followed = state
        .store
        .assets_list(loupist_id, true)
        .map_err(map_store)?;
    Ok(scheduled_tasks::presets::price_move_followed_ids(
        watch, &followed,
    ))
}

pub(super) fn scoped_price_args(
    args: &Map<String, Value>,
    ids: Option<&[String]>,
) -> Map<String, Value> {
    let mut args = args.clone();
    if let Some(ids) = ids {
        // Old pinned versions already understand asset_ids.
        args.remove("asset_names");
        args.insert("asset_ids".into(), json!(ids));
    }
    args
}

/// One evaluation on current data. State, counters and the schedule stay as they are.
pub(crate) async fn test_now(
    state: &AppState,
    loupist_id: &str,
    task_id: &str,
    expected_revision: u64,
    expected_version: u32,
    expected_sha256: &str,
) -> Result<crate::scheduled_tasks::types::TriggerEvaluation, CommandError> {
    state.ensure_loupist_available(loupist_id)?;
    let mut task = state
        .store
        .scheduled_task_get(loupist_id, task_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("scheduled task"))?;
    if task.revision != expected_revision || task.declined_in_visit.is_some() {
        return Err(CommandError::conflict("The check proposal changed"));
    }
    let trigger = task
        .trigger
        .as_mut()
        .ok_or_else(|| CommandError::invalid("This task has no check"))?;
    let (version, hash) = trigger
        .pending
        .as_ref()
        .map(|pending| (pending.version, pending.sha256.as_str()))
        .unwrap_or((trigger.script.version, trigger.script.sha256.as_str()));
    if version != expected_version || hash != expected_sha256 {
        return Err(CommandError::conflict("The check proposal changed"));
    }
    let script = state
        .store
        .script_get(loupist_id, &trigger.script.name, Some(version))
        .map_err(map_store)?;
    if script.summary.sha256 != hash
        || crate::util::sha256(script.code.as_bytes()) != hash
        || script.summary.metadata.after_outside_text
    {
        return Err(CommandError::conflict(
            "The saved script does not match the check",
        ));
    }
    let pending_configuration = trigger
        .pending
        .as_ref()
        .and_then(|pending| pending.configuration.as_ref());
    let review_watch = match pending_configuration {
        Some(configuration) => price_review_watch(&task.watch, configuration)?,
        None => task.watch.clone(),
    };
    let price_ids = scoped_price_ids(state, loupist_id, &review_watch)?;
    if price_ids.as_ref().is_some_and(Vec::is_empty) {
        return Err(CommandError::invalid("No matching asset is followed."));
    }
    let authority = super::capture_chat_connector_authority(state, loupist_id)?;
    let executor = super::tool_executor::AppToolExecutor::for_host_with_connector_authority(
        state, loupist_id, authority,
    );
    let output = if trigger.pending.is_some() || task.awaiting_confirmation {
        let mut configuration = trigger
            .pending
            .as_ref()
            .and_then(|item| item.configuration.clone())
            .unwrap_or_else(|| trigger.configuration());
        if review_watch.preset == Some(WatchPreset::PriceMove) {
            trigger::set_price_subject_args(
                &mut configuration.args,
                review_watch.subjects.as_deref(),
            );
            configuration.price_subjects = Some(review_watch.subjects.unwrap_or_default());
            if trigger.pending.is_none() {
                if let Some(terms) = &review_watch.terms {
                    configuration.summary = Some(terms.clone());
                }
            }
        }
        let review = super::tool_executor::spawn_dry_run(
            executor,
            script.code,
            scoped_price_args(&configuration.args, price_ids.as_deref()),
            if task.use_memory {
                trigger.evaluations.clone()
            } else {
                Vec::new()
            },
            trigger.approved_ref().cloned(),
            task.use_memory,
        )
        .await
        .map_err(|_| CommandError::invalid("The dry run could not finish"))?
        .map_err(|_| CommandError::invalid("The dry run could not finish"))?;
        configuration.tools = review.tools;
        configuration.reads_per_check = review.today.reads.max(1);
        if let Some(pending) = &mut trigger.pending {
            pending.configuration = Some(configuration);
            pending.test = Some(review.test);
        } else {
            trigger.set_configuration(configuration);
            trigger.proposal_test = Some(review.test);
        }
        let target = task.loupist_id.clone();
        write(state, task, now_ms())?;
        crate::briefings::desktop::changed(&target);
        review.today
    } else {
        executor
            .run_check(super::tool_executor::CheckRun {
                code: &script.code,
                after_outside_text: script.summary.metadata.after_outside_text,
                use_memory: task.use_memory,
                args: scoped_price_args(&trigger.args, price_ids.as_deref()),
                state: trigger.script_state.clone(),
                now: chrono::Utc::now().to_rfc3339(),
                as_of: None,
                allowed: &trigger.tools,
                discover_providers: false,
                replay: None,
                max_rounds: 3,
                max_calls: 24,
            })
            .await
            .unwrap_or_else(|error| errored(&error.to_string()))
    };
    Ok(crate::scheduled_tasks::types::TriggerEvaluation {
        at_ms: now_ms(),
        scheduled_for_ms: now_ms(),
        outcome: output.outcome,
        reading: output.reading,
        as_of: output.as_of,
        evidence: output.evidence,
        version: script.summary.version,
        sha256: script.summary.sha256,
        reads: output.reads,
        error: output.error,
        coalesced: false,
        ledger: output.ledger,
        card_message_id: None,
        retry_after_seconds: output.retry_after_seconds,
        replay_boundary: None,
        replay_freshness: None,
    })
}

/// `args` is the check's JSON. `template_params` is the same bag when the
/// model names a bundled template; an explicit `args` wins.
pub(crate) fn trigger_args(
    args: Option<Map<String, Value>>,
    template_params: Option<Map<String, Value>>,
) -> Map<String, Value> {
    args.or(template_params).unwrap_or_default()
}

pub(crate) fn args_ok(args: &Map<String, Value>) -> Result<(), CommandError> {
    if serde_json::to_vec(args)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX)
        > crate::scheduled_tasks::types::MAX_TRIGGER_ARGS_BYTES
    {
        return Err(CommandError::invalid("check arguments are at most 2 KiB"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::scripts::ScriptDraft;

    fn met_task(state: &AppState) -> Task {
        use crate::scheduled_tasks::types::{TaskDraft, TaskTarget};
        let workspace = state
            .store
            .get_loupist("loupist-a")
            .unwrap()
            .unwrap()
            .workspace_root;
        std::fs::create_dir_all(std::path::Path::new(&workspace).join(".loupist")).unwrap();
        let mut task = state
            .store
            .scheduled_task_create(
                "loupist-a",
                TaskDraft {
                    title: "Check prices".into(),
                    prompt: "Explain the move".into(),
                    schedule: Schedule::Interval { every_minutes: 5 },
                    target: TaskTarget::NewChat,
                    model_profile_id: String::new(),
                    thinking_level: String::new(),
                    run_options: Default::default(),
                    use_memory: false,
                    update_memory: false,
                    status: TaskStatus::Active,
                },
                1,
            )
            .unwrap();
        let sha256 = "ab".repeat(32);
        task.trigger = Some(TaskTrigger {
            script: TriggerScriptRef {
                name: "price-band".into(),
                version: 1,
                sha256: sha256.clone(),
            },
            summary: "Prices outside a band".into(),
            tools: Vec::new(),
            args: Map::new(),
            active_hours: None,
            approved: Some(TriggerApproval {
                version: 1,
                sha256,
                at_ms: 1,
                configuration: None,
                test: None,
            }),
            pending: None,
            state: TriggerPhase::On,
            script_state: json!({}),
            replay_boundary: Some(crate::scheduled_tasks::types::ReplayBoundary::INITIAL),
            last: None,
            evaluations: Vec::new(),
            counters: Default::default(),
            approved_versions: Vec::new(),
            stale_after_days: 5,
            reads_per_check: 1,
            proposal_test: None,
        });
        let revision = task.revision;
        state.store.scheduled_task_write(task, revision, 1).unwrap()
    }

    fn met_output(reading: &str) -> CheckOutput {
        trigger::output_from_worker(
            &json!({"status": "completed", "check_verdict": {
                "met": true, "reading": reading, "as_of": "2026-11-24", "state": {"sent": true}
            }}),
            vec![json!({"call":{"tool":"price"}, "response":{"status":"ok"}})],
            1,
        )
    }

    fn evaluate_met(state: &AppState, task: Task, reading: &str) -> (Box<TriggerOccurrence>, i64) {
        let due = task.next_run_at_ms.unwrap();
        let DueCheck::Met(occurrence) =
            finish(state, task, due, due, false, met_output(reading), 0).unwrap()
        else {
            panic!("expected met occurrence");
        };
        (occurrence, due)
    }

    #[tokio::test]
    async fn test_now_rejects_a_changed_revision_version_or_hash_before_loading_code() {
        use crate::scheduled_tasks::types::TriggerPending;
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let loupist = "loupist-a";
        crate::test_fixtures::create_folder_loupist(&state.store, loupist, root.path());
        let mut task = met_task(&state);
        let old_revision = task.revision;
        let pending_hash = "cd".repeat(32);
        let check = task.trigger.as_mut().unwrap();
        check.state = TriggerPhase::UpdateProposed;
        check.pending = Some(TriggerPending {
            version: 2,
            sha256: pending_hash.clone(),
            configuration: None,
            test: None,
        });
        let task = write(&state, task, now_ms()).unwrap();
        // The pending code is absent. A stale request must fail at the proposal
        // check rather than attempting to load it or acquire connector authority.
        let old_hash = "ab".repeat(32);
        for (revision, version, hash) in [
            (old_revision, 2, &pending_hash),
            (task.revision, 1, &pending_hash),
            (task.revision, 2, &old_hash),
        ] {
            let error = test_now(&state, loupist, &task.id, revision, version, hash)
                .await
                .unwrap_err();
            assert_eq!(error.message, "The check proposal changed");
        }
    }

    #[test]
    fn a_declined_claim_consumes_once_and_restart_settles_the_next_card() {
        use crate::scheduled_tasks::types::RunStatus;
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_persistent_test(root.path().join("app"));
        crate::test_fixtures::create_folder_loupist(&state.store, "loupist-a", root.path());
        let task = met_task(&state);
        let id = task.id.clone();
        let home = |state: &AppState| {
            state
                .store
                .get_chat(&crate::chat::home::home_thread_id("loupist-a"))
                .unwrap()
                .unwrap()
        };
        let saved_task = || {
            state
                .store
                .scheduled_task_get("loupist-a", &id)
                .unwrap()
                .unwrap()
        };

        let (first, due) = evaluate_met(&state, task, "Moved 7%");
        assert!(home(&state).messages.is_empty());
        assert!(state
            .store
            .scheduled_trigger_consume(&first, due, false, |_| -> crate::store::StoreResult<()> {
                Err(crate::store::StoreError::Conflict("Chat busy"))
            })
            .unwrap()
            .is_none());
        assert!(state
            .store
            .scheduled_trigger_consume(&first, due, false, |_| Ok(()))
            .is_err());
        assert!(state
            .store
            .scheduled_task_runs("loupist-a", &id)
            .unwrap()
            .is_empty());
        let saved = saved_task();
        assert_eq!(
            saved.trigger.as_ref().unwrap().script_state,
            json!({"sent": true})
        );
        assert!(saved.next_run_at_ms.unwrap() > due);
        let home_after_first = home(&state);
        assert_eq!(home_after_first.messages.len(), 1);
        let denied = home_after_first.messages[0].delivery.as_ref().unwrap();
        assert_ne!(denied.status, RunStatus::Running);
        assert!(denied.trigger.as_ref().unwrap().reason.is_some());

        let (second, next_due) = evaluate_met(&state, saved, "Moved 9%");
        let (claim, ()) = state
            .store
            .scheduled_trigger_consume(&second, next_due, false, |_| Ok(()))
            .unwrap()
            .unwrap();
        assert_eq!(claim.run.scheduled_for_ms, next_due);
        assert_eq!(claim.run.trigger.unwrap().reading, "Moved 9%");
        let before_recovery = home(&state);
        assert_eq!(before_recovery.messages.len(), 2);
        assert_eq!(
            before_recovery.messages[1]
                .delivery
                .as_ref()
                .unwrap()
                .status,
            RunStatus::Running
        );

        state.close_for_restart_test();
        let state = AppState::for_persistent_test(root.path().join("app"));
        assert_eq!(
            state
                .store
                .scheduled_tasks_recover_running(next_due + 1)
                .unwrap(),
            1
        );
        let recovered = home(&state);
        assert_eq!(recovered.messages.len(), 2);
        assert_eq!(recovered.messages[0], before_recovery.messages[0]);
        let delivery = recovered.messages[1].delivery.as_ref().unwrap();
        assert_eq!(delivery.run_id, claim.run.run_id);
        assert_eq!(delivery.status, RunStatus::Interrupted);
        assert!(delivery.trigger.as_ref().unwrap().reason.is_some());
        assert!(state
            .store
            .scheduled_trigger_consume(&second, next_due, false, |_| Ok(()))
            .is_err());
    }

    #[test]
    fn a_manual_trigger_occurrence_does_not_consume_a_future_due_slot() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        crate::test_fixtures::create_folder_loupist(&state.store, "loupist-a", root.path());
        let task = met_task(&state);
        let due = task.next_run_at_ms.unwrap();
        let task_id = task.id.clone();
        let now = due - 1;
        let DueCheck::Met(occurrence) = finish(
            &state,
            task,
            now,
            now,
            true,
            met_output("Manual reading"),
            0,
        )
        .unwrap() else {
            panic!("expected a met manual occurrence");
        };
        let (claim, ()) = state
            .store
            .scheduled_trigger_consume(&occurrence, now, false, |_| Ok(()))
            .unwrap()
            .unwrap();
        assert_eq!(claim.run.scheduled_for_ms, now);
        let saved = state
            .store
            .scheduled_task_get("loupist-a", &task_id)
            .unwrap()
            .unwrap();
        assert_eq!(saved.next_run_at_ms, Some(due));
        assert_eq!(
            state
                .store
                .scheduled_run_trigger_evaluation(&claim.run)
                .unwrap()
                .unwrap()
                .reading,
            "Manual reading"
        );
    }

    #[test]
    fn met_occurrences_route_only_to_the_selected_destination() {
        use crate::scheduled_tasks::types::{
            DeliveryTarget, NotifyPolicy, ReportPolicy, TaskDelivery,
        };
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        crate::test_fixtures::create_folder_loupist(&state.store, "loupist-a", root.path());
        for destination in [
            DeliveryTarget::Bot,
            DeliveryTarget::None,
            DeliveryTarget::Briefing,
        ] {
            let mut task = met_task(&state);
            task.watch.report = Some(ReportPolicy::WhenMet);
            task.watch.delivery = Some(TaskDelivery {
                to: destination,
                notify: None,
            });
            let revision = task.revision;
            let task = state.store.scheduled_task_write(task, revision, 1).unwrap();
            let (occurrence, now) = evaluate_met(&state, task, "Moved 7%");
            let bot = destination == DeliveryTarget::Bot;
            assert_eq!(occurrence.card.is_some(), bot);
            assert_eq!(
                occurrence.notify,
                if bot {
                    NotifyPolicy::Digest
                } else {
                    NotifyPolicy::Off
                }
            );
            let (claim, ()) = state
                .store
                .scheduled_trigger_consume(&occurrence, now, false, |_| Ok(()))
                .unwrap()
                .unwrap();
            let delivery = state
                .store
                .scheduled_run_delivery("loupist-a", &claim.run.run_id)
                .unwrap()
                .unwrap();
            assert_eq!(delivery.to, destination);
        }
        let cards = state
            .store
            .get_chat(&crate::chat::home::home_thread_id("loupist-a"))
            .unwrap()
            .unwrap()
            .messages;
        assert_eq!(cards.len(), 1);
        assert_eq!(
            cards[0].delivery.as_ref().unwrap().task_title,
            "Check prices"
        );
    }

    #[test]
    fn large_replay_history_stays_readable_with_the_latest_evidence() {
        use crate::scheduled_tasks::types::MAX_STORED_EVALUATIONS;
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let loupist = "loupist-a";
        crate::test_fixtures::create_folder_loupist(&state.store, loupist, root.path());
        let mut task = met_task(&state);
        let id = task.id.clone();
        let due = task.next_run_at_ms.unwrap();
        let check = task.trigger.as_mut().unwrap();
        check.script_state = json!({"number":0,"padding":"s".repeat(8_100)});
        for number in 1..=MAX_STORED_EVALUATIONS {
            let mut output = met_output("Price moved");
            output.evidence = json!({"source":"saved close"});
            output.state = json!({"number":number,"padding":"s".repeat(8_100)});
            output.ledger[0]["response"]["value"] = json!("p".repeat(43_000));
            let now = due + number as i64;
            trigger::apply_output(check, now, now, output, 0);
        }
        assert!(serde_json::to_vec(&task).unwrap().len() > 819_200);
        let saved = write(&state, task, due + MAX_STORED_EVALUATIONS as i64).unwrap();

        let owner = state.store.lock_loupist(loupist).unwrap();
        let bytes: i64 = owner
            .query_row(
                "SELECT length(CAST(data_json AS BLOB)) FROM scheduled_tasks WHERE id=?1",
                [&id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(bytes <= 819_200);
        drop(owner);
        let reread = state
            .store
            .scheduled_task_get(loupist, &id)
            .unwrap()
            .unwrap();
        assert_eq!(reread.trigger, saved.trigger);
        let check = reread.trigger.unwrap();
        assert!(check.evaluations.len() < MAX_STORED_EVALUATIONS);
        assert_eq!(
            check.evaluations[0].ledger[0]["response"]["value"]
                .as_str()
                .unwrap()
                .len(),
            43_000
        );
        let oldest = check.evaluations.last().unwrap().replay_boundary.unwrap();
        assert_eq!(
            oldest.offset as usize,
            MAX_STORED_EVALUATIONS - check.evaluations.len()
        );
        assert_eq!(oldest.generation, 0);
        let last = check.last.unwrap();
        assert_eq!(last.evidence, json!({"source":"saved close"}));
        assert_eq!(
            last.replay_boundary.unwrap().offset as usize,
            MAX_STORED_EVALUATIONS - 1
        );
        assert_eq!(trigger::successful_read_sources(&last.ledger), ["price"]);
        assert!(last.ledger[0]["response"].get("value").is_none());
    }

    #[test]
    fn mixed_success_keeps_the_verdict_and_defers_the_next_slot() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let loupist = "loupist-a";
        crate::test_fixtures::create_folder_loupist(&state.store, loupist, root.path());
        let mut task = met_task(&state);
        let id = task.id.clone();
        for met in [false, true] {
            let due = task.next_run_at_ms.unwrap();
            let output = trigger::output_from_worker(
                &json!({"status":"completed","check_verdict":{
                    "met":met,"reading":"Northwind volume 5.0x","as_of":"2026-11-24"
                }}),
                vec![
                    json!({"call":{"tool":"price"},"response":{"status":"ok"}}),
                    json!({"response":{"status":"unavailable","code":"rate_limited","retry_after":7200}}),
                    json!({"response":{"status":"unavailable","code":"rate_limited","retry_after":30}}),
                ],
                3,
            );
            let occurrence = finish(&state, task, due, due, false, output, 0).unwrap();
            if let DueCheck::Met(occurrence) = occurrence {
                state
                    .store
                    .scheduled_trigger_consume(&occurrence, due, false, |_| Ok(()))
                    .unwrap();
            }
            task = state
                .store
                .scheduled_task_get(loupist, &id)
                .unwrap()
                .unwrap();
            let last = task.trigger.as_ref().unwrap().last.as_ref().unwrap();
            assert_eq!(
                last.outcome,
                if met {
                    TriggerOutcome::Met
                } else {
                    TriggerOutcome::NotMet
                }
            );
            assert_eq!(last.retry_after_seconds, Some(7200));
            assert!(task.next_run_at_ms.unwrap() >= due + 7_200_000);
        }
    }

    #[test]
    fn a_delayed_card_uses_its_own_verdict_and_only_successful_reads() {
        use crate::scheduled_tasks::types::{RunStatus, TriggerToolGrant};

        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        crate::test_fixtures::create_folder_loupist(&state.store, "loupist-a", root.path());
        let mut task = met_task(&state);
        let task_id = task.id.clone();
        task.trigger.as_mut().unwrap().tools = ["price", "followed", "unused"]
            .map(|tool| TriggerToolGrant {
                name: tool.into(),
                provider: None,
            })
            .to_vec();
        let revision = task.revision;
        let task = state.store.scheduled_task_write(task, revision, 1).unwrap();
        let mut output = met_output("First move");
        output.ledger = vec![
            json!({"call":{"tool":"price"}, "response":{"status":"ok"}}),
            json!({"call":{"tool":"price"}, "response":{"status":"ok"}}),
            json!({"call":{"tool":"followed"}, "response":{"status":"unavailable"}}),
        ];
        let due = task.next_run_at_ms.unwrap();
        let now = due + 61_000;
        let DueCheck::Met(first) = finish(&state, task, now, due, false, output, 0).unwrap() else {
            panic!("expected met occurrence");
        };
        assert_eq!(
            first
                .card
                .as_ref()
                .unwrap()
                .delivery
                .as_ref()
                .unwrap()
                .trigger
                .as_ref()
                .unwrap()
                .sources,
            ["price"]
        );
        let (claim, ()) = state
            .store
            .scheduled_trigger_consume(&first, now, false, |_| Ok(()))
            .unwrap()
            .unwrap();
        let finished = state
            .store
            .scheduled_task_finish(
                "loupist-a",
                &claim.run.run_id,
                RunStatus::Succeeded,
                None,
                now + 1,
            )
            .unwrap();
        let task = state
            .store
            .scheduled_task_get("loupist-a", &task_id)
            .unwrap()
            .unwrap();
        let (later, later_due) = evaluate_met(&state, task, "Later move");
        state
            .store
            .scheduled_trigger_consume(&later, later_due, false, |_| Ok(()))
            .unwrap();

        let delayed = crate::chat::home::delivery_message(&state, &finished).unwrap();
        let reading = delayed.delivery.unwrap().trigger.unwrap();
        assert_eq!(reading.reading, "First move");
        assert_eq!(reading.checked_at_ms, now);
        assert_eq!(reading.sources, ["price"]);
        assert!(reading.coalesced);
    }

    #[test]
    fn promotion_and_revert_use_the_manifest_test_and_arguments_for_each_version() {
        use crate::scheduled_tasks::types::{
            TaskDefinition, TaskDraft, TaskKind, TaskStatus, TaskTarget, TriggerToolGrant,
        };
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let loupist = "loupist-a";
        crate::test_fixtures::create_folder_loupist(&state.store, loupist, root.path());
        let alpha = state
            .store
            .seed_followed_listing(loupist, "AAA", "US", Some("Alpha"))
            .unwrap();
        let beta = state
            .store
            .seed_followed_listing(loupist, "BBB", "US", Some("Beta"))
            .unwrap();
        let watch = TaskWatch {
            preset: Some(WatchPreset::PriceMove),
            subjects: Some(vec![TaskSubject::Name {
                name: "Alpha".into(),
            }]),
            ..Default::default()
        };
        let mut trigger = crate::commands::scheduled_tasks::preset_trigger(
            &state,
            loupist,
            "price-band",
            &watch,
            "Price check",
        )
        .unwrap();
        trigger.args.insert("band_percent".into(), json!(10));
        let first = trigger.script.clone();
        trigger.tools = vec![TriggerToolGrant {
            name: "price".into(),
            provider: Some("provider-a".into()),
        }];
        trigger.reads_per_check = 20;
        let task = state
            .store
            .scheduled_task_create_definition(
                loupist,
                "watch",
                TaskDraft {
                    title: "Price check".into(),
                    prompt: "Report the move".into(),
                    schedule: Schedule::Interval { every_minutes: 15 },
                    target: TaskTarget::NewChat,
                    model_profile_id: "".into(),
                    thinking_level: "".into(),
                    run_options: Default::default(),
                    use_memory: false,
                    update_memory: false,
                    status: TaskStatus::Paused,
                },
                TaskKind::Custom,
                TaskDefinition {
                    watch,
                    trigger: Some(trigger),
                    proposed_in_message_id: Some("reply".into()),
                    ..Default::default()
                },
                now_ms(),
            )
            .unwrap();
        assert!(approve(
            &state,
            loupist,
            &task.id,
            task.revision,
            first.version,
            &first.sha256
        )
        .is_err());
        let mut reviewed_task = task;
        reviewed_task.trigger.as_mut().unwrap().proposal_test = Some(trigger::reviewed_fixture());
        let reviewed_task = write(&state, reviewed_task, now_ms()).unwrap();
        let mut turned_on = approve(
            &state,
            loupist,
            &reviewed_task.id,
            reviewed_task.revision,
            first.version,
            &first.sha256,
        )
        .unwrap();
        let check = turned_on.trigger.as_mut().unwrap();
        let last_reset = check.replay_boundary.unwrap();
        check.script_state = json!({"seen":true});
        check.state = TriggerPhase::UpdateProposed;
        check.pending = Some(scheduled_tasks::types::TriggerPending {
            version: first.version,
            sha256: first.sha256.clone(),
            configuration: Some(check.configuration()),
            test: Some(trigger::reviewed_fixture()),
        });
        let turned_on = write(&state, turned_on, now_ms()).unwrap();
        let turned_on = approve(
            &state,
            loupist,
            &turned_on.id,
            turned_on.revision,
            first.version,
            &first.sha256,
        )
        .unwrap();
        let check = turned_on.trigger.as_ref().unwrap();
        assert_eq!(check.script_state, json!({}));
        assert_eq!(
            check.replay_boundary.unwrap().generation,
            last_reset.generation + 1
        );
        assert!(check.replay_boundary.unwrap().starts_reset());
        let second = state
            .store
            .script_save(
                loupist,
                ScriptDraft {
                    name: first.name,
                    title: None,
                    summary: "A new test".into(),
                    args: None,
                    code: "print('updated')\n".into(),
                },
                ScriptOrigin::User,
                false,
            )
            .unwrap();
        note_saved_version(
            &state,
            loupist,
            &second.summary.name,
            second.summary.version,
            &second.summary.sha256,
        )
        .unwrap();
        let mut update = state
            .store
            .scheduled_task_get(loupist, &turned_on.id)
            .unwrap()
            .unwrap();
        assert!(approve(
            &state,
            loupist,
            &update.id,
            update.revision,
            second.summary.version,
            &second.summary.sha256
        )
        .is_err());
        let pending = update.trigger.as_mut().unwrap().pending.as_mut().unwrap();
        pending.configuration = Some(TriggerConfiguration {
            summary: Some("Beta moves 20%".into()),
            args: Map::from_iter([
                ("band_percent".into(), json!(20)),
                ("asset_names".into(), json!(["Beta"])),
            ]),
            tools: vec![TriggerToolGrant {
                name: "price".into(),
                provider: Some("provider-b".into()),
            }],
            reads_per_check: 3,
            active_hours: None,
            price_subjects: None,
        });
        let candidate =
            price_review_watch(&update.watch, pending.configuration.as_ref().unwrap()).unwrap();
        assert_eq!(
            scoped_price_ids(&state, loupist, &candidate).unwrap(),
            Some(vec![beta.asset_id.clone()])
        );
        assert_eq!(
            scoped_price_ids(&state, loupist, &update.watch).unwrap(),
            Some(vec![alpha.asset_id.clone()])
        );
        pending.test = Some(trigger::reviewed_fixture());
        let update = write(&state, update, now_ms()).unwrap();
        let renewed = approve(
            &state,
            loupist,
            &update.id,
            update.revision,
            second.summary.version,
            &second.summary.sha256,
        )
        .unwrap();
        let renewed_trigger = renewed.trigger.as_ref().unwrap();
        assert_eq!(renewed_trigger.summary, "Beta moves 20%");
        assert_eq!(renewed.watch.terms.as_deref(), Some("Beta moves 20%"));
        let live_ids = scoped_price_ids(&state, loupist, &renewed.watch).unwrap();
        let live_args = scoped_price_args(&renewed_trigger.args, live_ids.as_deref());
        assert_eq!(live_args["band_percent"], 20);
        assert_eq!(live_args["asset_ids"], json!([beta.asset_id]));
        assert_eq!(
            renewed_trigger.tools[0].provider.as_deref(),
            Some("provider-b")
        );
        assert_eq!(renewed_trigger.reads_per_check, 3);
        assert!(renewed_trigger
            .approved
            .as_ref()
            .unwrap()
            .test
            .as_ref()
            .unwrap()
            .scores
            .is_some());
        let reverted = revert(&state, loupist, &renewed.id, first.version).unwrap();
        assert_eq!(
            scoped_price_ids(&state, loupist, &reverted.watch).unwrap(),
            Some(vec![alpha.asset_id])
        );
        let restored = reverted.trigger.unwrap();
        assert_eq!(restored.summary, "Price check");
        assert_eq!(restored.args["band_percent"], 10);
        assert_eq!(restored.tools[0].provider.as_deref(), Some("provider-a"));
        assert_eq!(restored.reads_per_check, 20);
        assert!(restored.replay_boundary.unwrap().starts_reset());
        assert_eq!(
            restored.replay_boundary.unwrap().generation,
            renewed_trigger.replay_boundary.unwrap().generation + 1
        );
    }

    #[test]
    fn an_evaluation_uses_only_the_approved_hash() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        crate::test_fixtures::create_folder_loupist(&state.store, "loupist-a", root.path());
        let save = |code: &str| {
            state
                .store
                .script_save(
                    "loupist-a",
                    ScriptDraft {
                        name: "price-band".into(),
                        title: None,
                        summary: "A price band".into(),
                        args: None,
                        code: code.into(),
                    },
                    ScriptOrigin::User,
                    false,
                )
                .unwrap()
        };
        let first = save("print('v1')\n");
        let second = save("print('v2')\n");
        assert_ne!(first.summary.sha256, second.summary.sha256);
        let trigger = TaskTrigger {
            script: script_ref(&first),
            summary: "Prices outside a band".into(),
            tools: Vec::new(),
            args: Map::new(),
            active_hours: None,
            approved: Some(TriggerApproval {
                version: first.summary.version,
                sha256: first.summary.sha256.clone(),
                at_ms: 1,
                configuration: None,
                test: None,
            }),
            pending: Some(crate::scheduled_tasks::types::TriggerPending {
                version: second.summary.version,
                sha256: second.summary.sha256,
                test: None,
                configuration: None,
            }),
            state: TriggerPhase::UpdateProposed,
            script_state: json!({}),
            replay_boundary: Some(crate::scheduled_tasks::types::ReplayBoundary::INITIAL),
            last: None,
            evaluations: Vec::new(),
            counters: Default::default(),
            approved_versions: Vec::new(),
            stale_after_days: 5,
            reads_per_check: 1,
            proposal_test: None,
        };
        let script = approved_source(&state.store, "loupist-a", &trigger).unwrap();
        assert_eq!(script.code, "print('v1')\n");
        let framed = crate::agent_harness::attach_trigger(
            r#"{"live_time":{}}"#,
            &TriggerContext {
                version: 1,
                reading: "Northwind -7.4%".into(),
                as_of: "2026-11-24".into(),
                evidence: json!({"moved": 1}),
                summary: "Prices outside a band".into(),
            },
        );
        let mut params = Map::new();
        params.insert("band_percent".into(), json!(5));
        let mut explicit = Map::new();
        explicit.insert("band_percent".into(), json!(8));
        assert_eq!(
            trigger_args(None, Some(params.clone())).get("band_percent"),
            Some(&json!(5))
        );
        assert_eq!(
            trigger_args(Some(explicit), Some(params))
                .get("band_percent")
                .and_then(Value::as_i64),
            Some(8)
        );
        assert!(framed.contains(crate::agent_harness::TRIGGER_FRAMING));
        let framed: Value = serde_json::from_str(&framed).unwrap();
        let body = |key| {
            crate::custom_connections::taint::delimited_body(
                framed["trigger"][key].as_str().unwrap(),
            )
        };
        assert_eq!(body("reading"), Some("Northwind -7.4%"));
        assert_eq!(body("evidence"), Some(r#"{"moved":1}"#));
        assert!(approved_source(
            &state.store,
            "loupist-a",
            &TaskTrigger {
                approved: Some(TriggerApproval {
                    version: second.summary.version,
                    sha256: "ab".repeat(32),
                    at_ms: 1,
                    configuration: None,
                    test: None,
                }),
                script: TriggerScriptRef {
                    version: second.summary.version,
                    sha256: "ab".repeat(32),
                    ..trigger.script.clone()
                },
                ..trigger
            },
        )
        .is_err());
    }
}
