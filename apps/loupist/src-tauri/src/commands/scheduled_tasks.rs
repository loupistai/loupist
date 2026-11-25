use serde::{Deserialize, Serialize};
use tauri::{ipc::Channel, State};

use super::{map_store, types::ChatStreamEvent};
use crate::{
    app::{AppState, CommandError},
    scheduled_tasks::{
        service,
        types::{Run, Task, TaskListItem, TaskStatus, TaskSubject},
        WatchSnapshot,
    },
};

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "ScheduledTasksLoupistRequest"))]
#[serde(deny_unknown_fields)]
pub(crate) struct LoupistRequest {
    loupist_id: String,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskRequest {
    loupist_id: String,
    task_id: String,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusRequest {
    loupist_id: String,
    task_id: String,
    expected_revision: u64,
    status: TaskStatus,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct RevisionRequest {
    loupist_id: String,
    task_id: String,
    expected_revision: u64,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct RunRequest {
    loupist_id: String,
    task_id: String,
    run_id: String,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FromPresetRequest {
    pub loupist_id: String,
    pub preset: crate::scheduled_tasks::types::WatchPreset,
    pub subjects: Option<Vec<TaskSubject>>,
    pub params: Option<crate::scheduled_tasks::presets::PresetParams>,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeliveryRequest {
    loupist_id: String,
    task_id: String,
    expected_revision: u64,
    to: Option<crate::scheduled_tasks::types::DeliveryTarget>,
    notify: Option<crate::scheduled_tasks::types::NotifyPolicy>,
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn scheduled_task_from_preset(
    request: FromPresetRequest,
    state: State<'_, AppState>,
) -> Result<Task, CommandError> {
    let task = from_preset(request, &state)?;
    if task.status == TaskStatus::Active
        && task.last_run_at_ms.is_none()
        && task
            .next_run_at_ms
            .is_some_and(|due| due <= super::now_ms())
        && state
            .scheduled
            .dispatch(&state, &task.loupist_id, &task.id, false)
            .await
            .is_ok()
    {
        return Ok(service::find(&state, &task.loupist_id, &task.id)?.unwrap_or(task));
    }
    Ok(task)
}

pub(crate) fn from_preset(
    request: FromPresetRequest,
    state: &AppState,
) -> Result<Task, CommandError> {
    let loupist_id = request.loupist_id.clone();
    let mut prepared = preset_definition(request, state, None)?;
    if let Some(existing) = service::find(state, &loupist_id, &prepared.id)? {
        return Ok(existing);
    }
    let trigger = prepared
        .trigger_template
        .as_deref()
        .map(|template| {
            preset_trigger(
                state,
                &loupist_id,
                template,
                &prepared.definition.watch,
                &prepared.draft.title,
            )
        })
        .transpose()?;
    if trigger.is_some() {
        prepared.draft.status = crate::scheduled_tasks::types::TaskStatus::Paused;
        prepared.definition.proposed_in_message_id = Some(format!("preset-{}", prepared.id));
        prepared.definition.trigger = trigger;
    }
    service::create_definition(
        state,
        &loupist_id,
        prepared.draft,
        &prepared.id,
        prepared.definition,
    )
}

pub(crate) fn preset_trigger(
    state: &AppState,
    loupist_id: &str,
    template: &str,
    watch: &crate::scheduled_tasks::types::TaskWatch,
    title: &str,
) -> Result<crate::scheduled_tasks::types::TaskTrigger, CommandError> {
    let saved = crate::commands::triggers::install_template(state, loupist_id, template, title)?;
    let mut trigger = crate::scheduled_tasks::types::TaskTrigger {
        script: crate::commands::triggers::script_ref(&saved),
        summary: watch.terms.clone().unwrap_or_else(|| title.to_owned()),
        tools: Vec::new(),
        args: serde_json::json!({"band_percent": 5})
            .as_object()
            .cloned()
            .unwrap_or_default(),
        active_hours: None,
        approved: None,
        pending: None,
        state: crate::scheduled_tasks::types::TriggerPhase::Proposed,
        script_state: serde_json::json!({}),
        replay_state_known: true,
        last: None,
        evaluations: Vec::new(),
        counters: Default::default(),
        approved_versions: Vec::new(),
        stale_after_days: 5,
        reads_per_check: 0,
        proposal_test: None,
    };
    crate::scheduled_tasks::trigger::set_price_subject_args(
        &mut trigger.args,
        watch.subjects.as_deref(),
    );
    Ok(trigger)
}

pub(crate) struct PreparedPreset {
    pub(crate) id: String,
    pub(crate) draft: crate::scheduled_tasks::types::TaskDraft,
    pub(crate) definition: crate::scheduled_tasks::types::TaskDefinition,
    trigger_template: Option<String>,
}

pub(crate) fn preset_definition(
    request: FromPresetRequest,
    state: &AppState,
    run_at_ms: Option<i64>,
) -> Result<PreparedPreset, CommandError> {
    use crate::scheduled_tasks::{presets, types::*};
    let loupist = &request.loupist_id;
    state.ensure_loupist_available(loupist)?;
    let config = super::loupist_configuration(state, loupist)?.chat;
    let filled = presets::fill(
        state,
        loupist,
        request.preset,
        request.subjects,
        request.params.unwrap_or_default(),
        run_at_ms,
    )?;
    let key = serde_json::to_vec(&(
        request.preset,
        &filled.watch.subjects,
        matches!(filled.schedule, Schedule::Once { .. }).then_some(&filled.schedule),
    ))
    .map_err(super::map_internal)?;
    let id = format!("watch-{}", &crate::util::sha256(&key)[..32]);
    let draft = TaskDraft {
        title: filled.title,
        prompt: filled.prompt,
        schedule: filled.schedule,
        target: TaskTarget::NewChat,
        model_profile_id: config.model_profile_id.unwrap_or_default(),
        thinking_level: config.thinking_level.unwrap_or_default(),
        run_options: config.run_options,
        use_memory: config.use_memory,
        update_memory: config.update_memory,
        status: TaskStatus::Active,
    };
    Ok(PreparedPreset {
        id,
        draft,
        definition: TaskDefinition {
            watch: filled.watch,
            ..Default::default()
        },
        trigger_template: filled.trigger_template,
    })
}

#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn scheduled_task_set_delivery(
    request: DeliveryRequest,
    state: State<'_, AppState>,
) -> Result<Task, CommandError> {
    service::set_delivery(
        &state,
        &request.loupist_id,
        &request.task_id,
        request.expected_revision,
        request.to,
        request.notify,
    )
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn scheduled_task_run_now(
    request: TaskRequest,
    state: State<'_, AppState>,
) -> Result<Run, CommandError> {
    service::run_now(&state, &request.loupist_id, &request.task_id).await
}

#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn scheduled_tasks_activity(
    request: LoupistRequest,
    state: State<'_, AppState>,
) -> Result<Vec<Run>, CommandError> {
    service::ensure_visible(&state, &request.loupist_id)?;
    state
        .store
        .scheduled_tasks_activity(&request.loupist_id)
        .map_err(map_store)
}

#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn scheduled_task_watch(
    request: RunRequest,
    on_event: Channel<ChatStreamEvent>,
    state: State<'_, AppState>,
) -> Result<Option<WatchSnapshot>, CommandError> {
    service::ensure_visible(&state, &request.loupist_id)?;
    state.scheduled.watch(
        &request.loupist_id,
        &request.task_id,
        &request.run_id,
        on_event,
    )
}

#[cfg(test)]
mod tests;

/// The current state of a task a Chat card shows; `None` once it was deleted.
#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn scheduled_task_get(
    request: TaskRequest,
    state: State<'_, AppState>,
) -> Result<Option<Task>, CommandError> {
    service::find(&state, &request.loupist_id, &request.task_id)
}

/// The one-tap OK for a task the Loupist proposed from a conversation.
#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn scheduled_task_confirm(
    request: RevisionRequest,
    state: State<'_, AppState>,
) -> Result<Task, CommandError> {
    service::confirm(
        &state,
        &request.loupist_id,
        &request.task_id,
        request.expected_revision,
    )
}

/// Declines a proposed task; only unconfirmed tasks can be discarded this way.
#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn scheduled_task_discard(
    request: TaskRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    service::discard(&state, &request.loupist_id, &request.task_id)
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct ProposalDeclineRequest {
    loupist_id: String,
    task_id: String,
    visit: i64,
    declined: bool,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct ProposalVisitRequest {
    loupist_id: String,
    visit: i64,
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn scheduled_proposal_set_declined(
    request: ProposalDeclineRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(crate::runs::maintenance::MaintenanceActivityKind::ChatMutation)?;
    service::ensure_visible(&state, &request.loupist_id)?;
    state
        .store
        .scheduled_proposal_set_declined(
            &request.loupist_id,
            &request.task_id,
            request.visit,
            request.declined,
        )
        .map_err(map_store)?;
    crate::briefings::desktop::changed(&request.loupist_id);
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn scheduled_proposal_visit_end(
    request: ProposalVisitRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(crate::runs::maintenance::MaintenanceActivityKind::ChatMutation)?;
    service::ensure_visible(&state, &request.loupist_id)?;
    let removed = state
        .store
        .scheduled_proposal_visit_end(&request.loupist_id, request.visit)
        .map_err(map_store)?;
    if removed > 0 {
        crate::briefings::desktop::changed(&request.loupist_id);
    }
    Ok(())
}

/// Every scheduled task of the Loupist with its latest run, for the Schedule tab.
#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn scheduled_tasks_list(
    request: LoupistRequest,
    state: State<'_, AppState>,
) -> Result<Vec<TaskListItem>, CommandError> {
    service::list(&state, &request.loupist_id)
}

/// Pause or resume one task; the revision guards against a concurrent edit.
#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn scheduled_task_set_status(
    request: StatusRequest,
    state: State<'_, AppState>,
) -> Result<Task, CommandError> {
    if request.status == TaskStatus::Completed {
        return Err(CommandError::invalid("only a run can complete a task"));
    }
    service::set_status(
        &state,
        &request.loupist_id,
        &request.task_id,
        request.expected_revision,
        request.status,
    )
}

/// Deletes a custom task. The briefing is turned off from its own settings.
#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn scheduled_task_delete(
    request: RevisionRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    service::delete(
        &state,
        &request.loupist_id,
        &request.task_id,
        request.expected_revision,
    )
}

/// The task's run history, newest first.
#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn scheduled_task_runs(
    request: TaskRequest,
    state: State<'_, AppState>,
) -> Result<Vec<Run>, CommandError> {
    service::runs(&state, &request.loupist_id, &request.task_id)
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct TriggerGetRequest {
    pub loupist_id: String,
    pub task_id: String,
    pub version: Option<u32>,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct TriggerReviewRequest {
    pub loupist_id: String,
    pub task_id: String,
    pub expected_revision: u64,
    pub version: u32,
    pub sha256: String,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TriggerRevertRequest {
    pub loupist_id: String,
    pub task_id: String,
    pub to_version: u32,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TriggerView {
    pub code: String,
    pub summary: String,
    pub version: u32,
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub dry_run: Option<crate::scheduled_tasks::types::TriggerDryRun>,
    pub task: Task,
}

#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn scheduled_trigger_get(
    request: TriggerGetRequest,
    state: State<'_, AppState>,
) -> Result<TriggerView, CommandError> {
    let task = state
        .store
        .scheduled_task_get(&request.loupist_id, &request.task_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("scheduled task"))?;
    let trigger = task
        .trigger
        .as_ref()
        .ok_or_else(|| CommandError::invalid("This task has no check"))?;
    let version = request
        .version
        .or(trigger.pending.as_ref().map(|pending| pending.version))
        .unwrap_or(trigger.script.version);
    let script = state
        .store
        .script_get(&request.loupist_id, &trigger.script.name, Some(version))
        .map_err(map_store)?;
    let dry_run = if let Some(pending) = trigger
        .pending
        .as_ref()
        .filter(|pending| pending.version == version)
    {
        pending.test.clone()
    } else if version == trigger.script.version {
        trigger.proposal_test.clone()
    } else {
        trigger
            .approved_versions
            .iter()
            .find(|item| item.version == version)
            .and_then(|item| item.test.clone())
    };
    Ok(TriggerView {
        code: script.code,
        summary: trigger.summary.clone(),
        version: script.summary.version,
        sha256: script.summary.sha256,
        dry_run,
        task,
    })
}

#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn scheduled_trigger_approve(
    request: TriggerReviewRequest,
    state: State<'_, AppState>,
) -> Result<Task, CommandError> {
    crate::commands::triggers::approve(
        &state,
        &request.loupist_id,
        &request.task_id,
        request.expected_revision,
        request.version,
        &request.sha256,
    )
}

#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn scheduled_trigger_revert(
    request: TriggerRevertRequest,
    state: State<'_, AppState>,
) -> Result<Task, CommandError> {
    crate::commands::triggers::revert(
        &state,
        &request.loupist_id,
        &request.task_id,
        request.to_version,
    )
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn scheduled_trigger_test(
    request: TriggerReviewRequest,
    state: State<'_, AppState>,
) -> Result<crate::scheduled_tasks::types::TriggerEvaluation, CommandError> {
    crate::commands::triggers::test_now(
        &state,
        &request.loupist_id,
        &request.task_id,
        request.expected_revision,
        request.version,
        &request.sha256,
    )
    .await
}

#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn scheduled_trigger_evaluations(
    request: TaskRequest,
    state: State<'_, AppState>,
) -> Result<Vec<crate::scheduled_tasks::types::TriggerEvaluation>, CommandError> {
    let task = state
        .store
        .scheduled_task_get(&request.loupist_id, &request.task_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("scheduled task"))?;
    Ok(task
        .trigger
        .map(|trigger| trigger.evaluations)
        .unwrap_or_default())
}
