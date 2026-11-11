//! Shared authority for the Scheduled UI and Chat tools.
use super::types::{Run, RunStatus, Task, TaskDraft, TaskListItem, TaskStatus};
use crate::{
    app::{AppState, CommandError, LoupistAccess},
    commands::{map_store, now_ms},
    runs::coordinator::RunKind,
    runs::maintenance::MaintenanceActivityKind,
};

pub(crate) fn ensure_visible(state: &AppState, loupist_id: &str) -> Result<(), CommandError> {
    if matches!(state.loupist_access(loupist_id), LoupistAccess::Hidden) {
        return Err(CommandError::new(
            "loupist_storage_unavailable",
            "Loupist storage is unavailable",
        ));
    }
    Ok(())
}

fn validate_draft(state: &AppState, draft: &TaskDraft) -> Result<(), CommandError> {
    draft.validate().map_err(map_store)?;
    state.pi_execution(
        &draft.model_profile_id,
        &draft.thinking_level,
        &draft.run_options,
    )?;
    Ok(())
}

pub(crate) fn list(state: &AppState, loupist_id: &str) -> Result<Vec<TaskListItem>, CommandError> {
    ensure_visible(state, loupist_id)?;
    // The automatic results task appears with the first followed equity.
    if state.ensure_loupist_available(loupist_id).is_ok() {
        let _ = crate::commands::earnings::ensure_task(state, loupist_id, false);
    }
    state
        .store
        .scheduled_tasks_list_with_latest(loupist_id)
        .map_err(map_store)
}

pub(crate) fn get(state: &AppState, loupist_id: &str, id: &str) -> Result<Task, CommandError> {
    ensure_visible(state, loupist_id)?;
    state
        .store
        .scheduled_task_get(loupist_id, id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("scheduled task"))
}

pub(crate) fn create(
    state: &AppState,
    loupist_id: &str,
    draft: TaskDraft,
    request_id: Option<&str>,
) -> Result<Task, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::ChatMutation)?;
    state.ensure_loupist_available(loupist_id)?;
    validate_draft(state, &draft)?;
    match request_id {
        Some(id) => state
            .store
            .scheduled_task_create_idempotent(loupist_id, id, draft, now_ms()),
        None => state
            .store
            .scheduled_task_create(loupist_id, draft, now_ms()),
    }
    .map_err(map_store)
}

pub(crate) fn update(
    state: &AppState,
    loupist_id: &str,
    id: &str,
    revision: u64,
    draft: TaskDraft,
) -> Result<Task, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::ChatMutation)?;
    state.ensure_loupist_available(loupist_id)?;
    match get(state, loupist_id, id)?.kind {
        super::types::TaskKind::Briefing => {
            return Err(CommandError::invalid(
                "Use briefing settings to edit the host-authored briefing",
            ))
        }
        super::types::TaskKind::Earnings => {
            return Err(CommandError::invalid(
                "The results task is automatic; pause or resume it instead",
            ))
        }
        // The store refuses a kind another version wrote.
        super::types::TaskKind::Custom | super::types::TaskKind::Unknown(_) => {}
    }
    validate_draft(state, &draft)?;
    state
        .store
        .scheduled_task_update(loupist_id, id, revision, draft, now_ms())
        .map_err(map_store)
}

pub(crate) fn set_status(
    state: &AppState,
    loupist_id: &str,
    id: &str,
    revision: u64,
    status: TaskStatus,
) -> Result<Task, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::ChatMutation)?;
    ensure_visible(state, loupist_id)?;
    if status == TaskStatus::Active {
        state.ensure_loupist_available(loupist_id)?;
        let task = get(state, loupist_id, id)?;
        // The earnings task makes no model call, so no model must be ready.
        if task.kind != super::types::TaskKind::Earnings {
            let mut draft = task.draft();
            draft.status = status;
            validate_draft(state, &draft)?;
        }
    }
    state
        .store
        .scheduled_task_set_status(loupist_id, id, revision, status, now_ms())
        .map_err(map_store)
}

/// The user's one-tap OK in the Chat card for a task the agent proposed.
pub(crate) fn confirm(state: &AppState, loupist_id: &str, id: &str) -> Result<Task, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::ChatMutation)?;
    state.ensure_loupist_available(loupist_id)?;
    let mut draft = get(state, loupist_id, id)?.draft();
    draft.status = TaskStatus::Active;
    validate_draft(state, &draft)?;
    state
        .store
        .scheduled_task_confirm(loupist_id, id, now_ms())
        .map_err(map_store)
}

/// Removes a proposed task the user declined; confirmed tasks are managed elsewhere.
pub(crate) fn discard(state: &AppState, loupist_id: &str, id: &str) -> Result<(), CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::ChatMutation)?;
    ensure_visible(state, loupist_id)?;
    state
        .store
        .scheduled_task_discard(loupist_id, id)
        .map_err(map_store)
}

/// The current state of one task, or `None` once it is gone.
pub(crate) fn find(
    state: &AppState,
    loupist_id: &str,
    id: &str,
) -> Result<Option<Task>, CommandError> {
    ensure_visible(state, loupist_id)?;
    state
        .store
        .scheduled_task_get(loupist_id, id)
        .map_err(map_store)
}

pub(crate) fn delete(
    state: &AppState,
    loupist_id: &str,
    id: &str,
    revision: u64,
) -> Result<(), CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::ChatMutation)?;
    ensure_visible(state, loupist_id)?;
    if get(state, loupist_id, id)?.kind == super::types::TaskKind::Earnings {
        return Err(CommandError::invalid(
            "The results task is automatic; pause it instead of deleting it",
        ));
    }
    state
        .store
        .scheduled_task_delete(loupist_id, id, revision)
        .map_err(map_store)
}

pub(crate) fn runs(state: &AppState, loupist_id: &str, id: &str) -> Result<Vec<Run>, CommandError> {
    ensure_visible(state, loupist_id)?;
    state
        .store
        .scheduled_task_runs(loupist_id, id)
        .map_err(map_store)
}

pub(crate) async fn run_now(
    state: &AppState,
    loupist_id: &str,
    id: &str,
) -> Result<Run, CommandError> {
    state.ensure_loupist_available(loupist_id)?;
    state
        .scheduled
        .dispatch(state, loupist_id, id, true)
        .await?
        .ok_or_else(|| CommandError::conflict("This scheduled task is already running"))
}

pub(crate) async fn stop(
    state: &AppState,
    loupist_id: &str,
    task_id: &str,
    run_id: &str,
) -> Result<(), CommandError> {
    ensure_visible(state, loupist_id)?;
    let run = state
        .store
        .scheduled_task_run_get(loupist_id, run_id)
        .map_err(map_store)?
        .filter(|run| run.task_id == task_id)
        .ok_or_else(|| CommandError::not_found("scheduled run"))?;
    if run.status == RunStatus::Running {
        state.cancel_run(&run.run_id, RunKind::Chat).await?;
    }
    Ok(())
}
