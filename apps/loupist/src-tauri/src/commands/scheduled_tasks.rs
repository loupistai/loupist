use serde::Deserialize;
use tauri::{ipc::Channel, State};

use super::{map_store, types::ChatStreamEvent};
use crate::{
    app::{AppState, CommandError},
    scheduled_tasks::{
        service,
        types::{Run, Task, TaskKind, TaskListItem, TaskStatus},
        WatchSnapshot,
    },
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoupistRequest {
    loupist_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskRequest {
    loupist_id: String,
    task_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusRequest {
    loupist_id: String,
    task_id: String,
    expected_revision: u64,
    status: TaskStatus,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RevisionRequest {
    loupist_id: String,
    task_id: String,
    expected_revision: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunRequest {
    loupist_id: String,
    task_id: String,
    run_id: String,
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

#[tauri::command(rename_all = "snake_case")]
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
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn scheduled_task_get(
    request: TaskRequest,
    state: State<'_, AppState>,
) -> Result<Option<Task>, CommandError> {
    service::find(&state, &request.loupist_id, &request.task_id)
}

/// The one-tap OK for a task the Loupist proposed from a conversation.
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn scheduled_task_confirm(
    request: TaskRequest,
    state: State<'_, AppState>,
) -> Result<Task, CommandError> {
    service::confirm(&state, &request.loupist_id, &request.task_id)
}

/// Declines a proposed task; only unconfirmed tasks can be discarded this way.
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn scheduled_task_discard(
    request: TaskRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    service::discard(&state, &request.loupist_id, &request.task_id)
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
    if service::get(&state, &request.loupist_id, &request.task_id)?.kind == TaskKind::Briefing {
        return Err(CommandError::invalid(
            "Turn the briefing off instead of deleting it",
        ));
    }
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
