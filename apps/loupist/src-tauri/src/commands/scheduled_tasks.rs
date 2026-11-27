use serde::{Deserialize, Serialize};
use tauri::{ipc::Channel, State};

use super::{map_store, types::ChatStreamEvent};
use crate::{
    app::{AppState, CommandError},
    scheduled_tasks::{
        service,
        types::{
            LearningPresetId, PresetId, Run, Task, TaskListItem, TaskStatus, TaskSubject,
            WatchPreset,
        },
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
pub(crate) struct RunNowRequest {
    loupist_id: String,
    task_id: String,
    #[cfg_attr(test, ts(optional))]
    confirm: Option<bool>,
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
    pub preset: WatchPreset,
    pub subjects: Option<Vec<TaskSubject>>,
    pub params: Option<crate::scheduled_tasks::presets::PresetParams>,
}

/// Watch presets and learning presets share this command. `preset` is the watch
/// id or a learning preset id such as `asset-daily` or `market`. `subject` is
/// the one learning subject; watch presets keep using `subjects`. `slots` fills
/// a learning preset's text slots, such as the market's name.
#[derive(Clone, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PresetCommand {
    pub loupist_id: String,
    pub preset: PresetId,
    pub subjects: Option<Vec<TaskSubject>>,
    pub subject: Option<TaskSubject>,
    pub params: Option<crate::scheduled_tasks::presets::PresetParams>,
    /// A learning preset's text slots by name, such as the market's name.
    /// Watch presets take none.
    pub slots: Option<std::collections::BTreeMap<String, String>>,
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
    request: PresetCommand,
    state: State<'_, AppState>,
) -> Result<Task, CommandError> {
    open_preset(request, &state).await
}

pub(crate) async fn open_preset(
    request: PresetCommand,
    state: &AppState,
) -> Result<Task, CommandError> {
    let subjects = preset_subjects(&request)?;
    let task = open_definition(
        &request.loupist_id,
        request.preset,
        subjects,
        request.params,
        request.slots,
        state,
    )?;
    finish_preset(state, task).await
}

fn preset_subjects(request: &PresetCommand) -> Result<Option<Vec<TaskSubject>>, CommandError> {
    match (&request.subjects, &request.subject) {
        (Some(subjects), Some(subject)) if subjects.as_slice() != std::slice::from_ref(subject) => {
            Err(CommandError::invalid("Use either subject or subjects"))
        }
        (Some(subjects), _) => Ok(Some(subjects.clone())),
        (None, Some(subject)) => Ok(Some(vec![subject.clone()])),
        (None, None) => Ok(None),
    }
}

async fn finish_preset(state: &AppState, task: Task) -> Result<Task, CommandError> {
    if task.kind == crate::scheduled_tasks::types::TaskKind::Learning {
        return start_learning_run(state, task).await;
    }
    if task.status == TaskStatus::Active
        && task.last_run_at_ms.is_none()
        && task
            .next_run_at_ms
            .is_some_and(|due| due <= super::now_ms())
        && state
            .scheduled
            .dispatch(state, &task.loupist_id, &task.id, false)
            .await
            .is_ok()
    {
        return Ok(service::find(state, &task.loupist_id, &task.id)?.unwrap_or(task));
    }
    Ok(task)
}

async fn start_learning_run(state: &AppState, task: Task) -> Result<Task, CommandError> {
    let pending = task.status == TaskStatus::Active
        && task.last_run_at_ms.is_none()
        && service::runs(state, &task.loupist_id, &task.id)?.is_empty();
    if pending {
        if let Err(error) = service::run_now(state, &task.loupist_id, &task.id, false).await {
            if service::runs(state, &task.loupist_id, &task.id)?.is_empty() {
                return Err(error);
            }
        }
    }
    Ok(service::find(state, &task.loupist_id, &task.id)?.unwrap_or(task))
}

fn preset_thinking_level(
    state: &AppState,
    model_profile_id: &str,
    saved_level: Option<&str>,
) -> Result<String, CommandError> {
    let level = saved_level.unwrap_or_default();
    if level.is_empty() && !crate::providers::auto::uses_default(model_profile_id) {
        // Chat may save the model on its own. A task with a concrete model
        // needs that model's resolved level even when Chat leaves it implicit.
        return Ok(crate::providers::auto::admit(
            state,
            model_profile_id,
            "",
            &Default::default(),
        )?
        .thinking_level);
    }
    Ok(level.to_owned())
}

#[test]
fn scheduled_presets_resolve_an_unspecified_level_for_a_concrete_model() {
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temp.path().join("app"));
    let level = preset_thinking_level(&state, "fixture", None).unwrap();
    assert!(state.model_catalog().unwrap().models[0]
        .thinking_levels
        .contains(&level));
    assert_eq!(
        preset_thinking_level(&state, "fixture", Some("off")).unwrap(),
        "off"
    );
    assert_eq!(preset_thinking_level(&state, "", None).unwrap(), "");
}

/// Fill a learning preset for a Loupist: its asset subject must be followed,
/// and every other preset takes its subject from its slots. Every entry
/// point that turns a learning preset into a program fills it here.
pub(crate) fn fill_learning_preset(
    state: &AppState,
    loupist_id: &str,
    preset_id: LearningPresetId,
    subjects: &[TaskSubject],
    slots: &std::collections::BTreeMap<String, String>,
    local_timezone: chrono_tz::Tz,
) -> Result<crate::scheduled_tasks::learning::FilledProgram, CommandError> {
    use crate::scheduled_tasks::learning::{PresetAsset, SubjectKind};
    let agent_root = state
        .artifacts
        .pi
        .as_ref()
        .and_then(|pi| pi.system_prompt.parent())
        .ok_or_else(|| CommandError::invalid("Unknown learning preset"))?;
    let preset = crate::scheduled_tasks::learning::load_preset(agent_root, preset_id)?;
    let followed;
    let asset = match (preset.subject_kind, subjects) {
        (SubjectKind::Asset, [TaskSubject::Asset { asset }]) => {
            followed = state
                .store
                .assets_list(loupist_id, true)
                .map_err(map_store)?;
            let row = followed
                .iter()
                .find(|row| row.asset_id == *asset)
                .ok_or_else(|| {
                    CommandError::invalid("This learning preset needs a followed asset")
                })?;
            Some(PresetAsset {
                id: asset,
                name: &row.name,
                reference: row
                    .reference
                    .as_ref()
                    .and_then(|reference| reference.fields.ticker.as_deref())
                    .unwrap_or(asset),
            })
        }
        (SubjectKind::Asset, _) => {
            return Err(CommandError::invalid(
                "This learning preset needs one asset subject",
            ))
        }
        (SubjectKind::Name | SubjectKind::None, []) => None,
        (SubjectKind::Name | SubjectKind::None, _) => {
            return Err(CommandError::invalid(
                "This learning preset takes its subject from its slots",
            ))
        }
    };
    preset.fill(asset, slots, local_timezone)
}

fn learning_from_preset(
    loupist_id: &str,
    preset_id: LearningPresetId,
    subjects: Option<Vec<TaskSubject>>,
    slots: std::collections::BTreeMap<String, String>,
    state: &AppState,
    local_timezone: chrono_tz::Tz,
) -> Result<Task, CommandError> {
    use crate::scheduled_tasks::types::{TaskDefinition, TaskDraft, TaskTarget, TaskWatch};
    let filled = fill_learning_preset(
        state,
        loupist_id,
        preset_id,
        &subjects.unwrap_or_default(),
        &slots,
        local_timezone,
    )?;
    // The same preset, subject and slots open the program they made before.
    // A slotless asset preset keeps Track daily's original key.
    let key = match filled.subjects.as_slice() {
        [TaskSubject::Asset { asset }] if slots.is_empty() => {
            serde_json::to_vec(&(preset_id.as_str(), asset))
        }
        subjects => serde_json::to_vec(&(preset_id.as_str(), subjects, &slots)),
    }
    .map_err(super::map_internal)?;
    let id = format!("learning-{}", &crate::util::sha256(&key)[..32]);
    if let Some(existing) = service::find(state, loupist_id, &id)? {
        return Ok(existing);
    }
    state.ensure_loupist_available(loupist_id)?;
    let config = super::loupist_configuration(state, loupist_id)?.chat;
    let model_profile_id = config.model_profile_id.unwrap_or_default();
    let thinking_level =
        preset_thinking_level(state, &model_profile_id, config.thinking_level.as_deref())?;
    let draft = TaskDraft {
        title: filled.title,
        prompt: filled.prompt,
        schedule: filled.schedule,
        target: TaskTarget::NewChat,
        model_profile_id,
        thinking_level,
        run_options: config.run_options,
        use_memory: true,
        update_memory: true,
        status: TaskStatus::Active,
    };
    service::create_definition(
        state,
        loupist_id,
        draft,
        &id,
        TaskDefinition {
            program: Some(filled.program),
            watch: TaskWatch {
                report: Some(filled.report),
                delivery: Some(filled.delivery),
                subjects: Some(filled.subjects),
                // Watch runs scope themselves by `watch.preset`; of the
                // learning presets only Track daily is recognised there.
                preset: (preset_id == LearningPresetId::AssetDaily)
                    .then_some(WatchPreset::AssetDaily),
                ..TaskWatch::default()
            },
            ..TaskDefinition::default()
        },
    )
}

#[cfg(test)]
pub(crate) fn from_preset(
    request: FromPresetRequest,
    state: &AppState,
) -> Result<Task, CommandError> {
    open_definition(
        &request.loupist_id.clone(),
        PresetId::Watch(request.preset),
        request.subjects,
        request.params,
        None,
        state,
    )
}

/// The one place a preset becomes a task, for every watch and learning preset.
fn open_definition(
    loupist_id: &str,
    preset: PresetId,
    subjects: Option<Vec<TaskSubject>>,
    params: Option<crate::scheduled_tasks::presets::PresetParams>,
    slots: Option<std::collections::BTreeMap<String, String>>,
    state: &AppState,
) -> Result<Task, CommandError> {
    let watch = match preset.normalized() {
        PresetId::Learning(learning) => {
            if params.is_some_and(|params| params.results_date.is_some()) {
                return Err(CommandError::invalid(
                    "Only a results follow-up takes a results date",
                ));
            }
            return learning_from_preset(
                loupist_id,
                learning,
                subjects,
                slots.unwrap_or_default(),
                state,
                crate::scheduled_tasks::presets::local_timezone(),
            );
        }
        PresetId::Watch(watch) => watch,
    };
    if let Some(slot) = slots.as_ref().and_then(|slots| slots.keys().next()) {
        return Err(CommandError::with_detail(
            crate::app::CommandErrorCode::PresetSlot,
            "This preset has no slot of that name",
            serde_json::json!({"preset": watch, "slot": slot, "reason": "unknown"}),
        ));
    }
    let request = FromPresetRequest {
        loupist_id: loupist_id.to_owned(),
        preset: watch,
        subjects,
        params,
    };
    let loupist_id = request.loupist_id.clone();
    let mut prepared = preset_definition(request, state, None)?;
    if let Some(existing) = service::find(state, &loupist_id, &prepared.id)? {
        return Ok(existing);
    }
    attach_preset_check(state, &loupist_id, &mut prepared)?;
    service::create_definition(
        state,
        &loupist_id,
        prepared.draft,
        &prepared.id,
        prepared.definition,
    )
}

/// A check preset is stored paused, with its trigger. It does not become an
/// active model task when the trigger cannot be attached.
pub(crate) fn attach_preset_check(
    state: &AppState,
    loupist_id: &str,
    prepared: &mut PreparedPreset,
) -> Result<(), CommandError> {
    let Some(template) = prepared.trigger_template.clone() else {
        return Ok(());
    };
    let trigger = preset_trigger(
        state,
        loupist_id,
        &template,
        &prepared.definition.watch,
        &prepared.draft.title,
    )?;
    prepared.draft.status = crate::scheduled_tasks::types::TaskStatus::Paused;
    prepared.definition.proposed_in_message_id = Some(format!("preset-{}", prepared.id));
    prepared.definition.trigger = Some(trigger);
    Ok(())
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
        when_met_summary: watch
            .preset
            .and_then(crate::scheduled_tasks::presets::preset_when_met_key),
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
        replay_boundary: Some(crate::scheduled_tasks::types::ReplayBoundary::INITIAL),
        last: None,
        evaluations: Vec::new(),
        counters: Default::default(),
        approved_versions: Vec::new(),
        stale_after_days: crate::scheduled_tasks::trigger::DEFAULT_STALE_AFTER_DAYS,
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
    let model_profile_id = config.model_profile_id.unwrap_or_default();
    let thinking_level =
        preset_thinking_level(state, &model_profile_id, config.thinking_level.as_deref())?;
    let draft = TaskDraft {
        title: filled.title,
        prompt: filled.prompt,
        schedule: filled.schedule,
        target: TaskTarget::NewChat,
        model_profile_id,
        thinking_level,
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
    request: RunNowRequest,
    state: State<'_, AppState>,
) -> Result<Run, CommandError> {
    service::run_now(
        &state,
        &request.loupist_id,
        &request.task_id,
        request.confirm.unwrap_or(false),
    )
    .await
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
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LearningProgramUpdateRequest {
    loupist_id: String,
    task_id: String,
    revision: u64,
    patch: crate::scheduled_tasks::program_edit::ProgramPatch,
}

/// The user's own edit of a learning program from Schedule: no proposal.
#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn learning_program_update(
    request: LearningProgramUpdateRequest,
    state: State<'_, AppState>,
) -> Result<Task, CommandError> {
    crate::scheduled_tasks::program_edit::update(
        &state,
        &request.loupist_id,
        &request.task_id,
        request.revision,
        request.patch,
    )
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LearningProgramUndoAllRequest {
    loupist_id: String,
    task_id: String,
    /// Counts what Undo all would do and changes nothing.
    #[cfg_attr(test, ts(optional))]
    preview: Option<bool>,
}

/// Undoes every run of a learning program in one transaction, newest first.
#[tauri::command(async, rename_all = "snake_case")]
pub(crate) fn learning_program_undo_all(
    request: LearningProgramUndoAllRequest,
    state: State<'_, AppState>,
) -> Result<crate::knowledge::ProgramUndo, CommandError> {
    let _activity = super::knowledge::admit_mutation(&state, &request.loupist_id)?;
    let task = service::find(&state, &request.loupist_id, &request.task_id)?
        .ok_or_else(|| CommandError::not_found("learning program"))?;
    if task.kind != crate::scheduled_tasks::types::TaskKind::Learning {
        return Err(CommandError::invalid(
            "Only a learning program is undone here",
        ));
    }
    super::knowledge::with_loupist_knowledge(&state, &request.loupist_id, |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs).undo_program(
            &request.loupist_id,
            &request.task_id,
            request.preview.unwrap_or(false),
            crate::commands::now_ms(),
        )
    })
}

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
