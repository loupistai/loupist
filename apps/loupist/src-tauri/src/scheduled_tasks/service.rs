//! Shared authority for the Scheduled UI and Chat tools.
use super::types::*;
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

pub(crate) fn validate_draft(state: &AppState, draft: &TaskDraft) -> Result<(), CommandError> {
    draft.validate().map_err(map_store)?;
    state.pi_execution(
        &draft.model_profile_id,
        &draft.thinking_level,
        &draft.run_options,
    )?;
    Ok(())
}

pub(crate) fn saved_task(state: &AppState, task: Task) -> Result<Task, CommandError> {
    state.scheduled.changed();
    crate::briefings::desktop::changed(&task.loupist_id);
    if task.status == TaskStatus::Active {
        crate::briefings::desktop::enable(state)?;
    }
    Ok(present(state, task))
}

pub(crate) fn present(state: &AppState, mut task: Task) -> Task {
    let level = if task.thinking_level.is_empty() {
        preferred_level(state, &task.loupist_id)
    } else {
        None
    };
    task.resolve_selection(state.selection_catalog().ok().as_ref(), level.as_deref());
    task
}

// The run inherits this setting when its task has no saved thinking level.
fn preferred_level(state: &AppState, loupist_id: &str) -> Option<String> {
    crate::commands::loupist_configuration(state, loupist_id)
        .ok()?
        .chat
        .thinking_level
}

pub(crate) fn list(state: &AppState, loupist_id: &str) -> Result<Vec<TaskListItem>, CommandError> {
    ensure_visible(state, loupist_id)?;
    let mut items = state
        .store
        .scheduled_tasks_list_with_latest(loupist_id)
        .map_err(map_store)?;
    if !items.is_empty() {
        let catalog = state.selection_catalog().ok();
        let level = if items.iter().any(|item| item.task.thinking_level.is_empty()) {
            preferred_level(state, loupist_id)
        } else {
            None
        };
        for item in &mut items {
            if item.task.is_unreadable_list_item() {
                continue;
            }
            item.task
                .resolve_selection(catalog.as_ref(), level.as_deref());
        }
    }
    Ok(items)
}

pub(crate) fn get(state: &AppState, loupist_id: &str, id: &str) -> Result<Task, CommandError> {
    find(state, loupist_id, id)?.ok_or_else(|| CommandError::not_found("scheduled task"))
}

#[cfg(test)]
pub(crate) fn create(
    state: &AppState,
    loupist_id: &str,
    draft: TaskDraft,
    request_id: Option<&str>,
) -> Result<Task, CommandError> {
    let generated;
    let id = match request_id {
        Some(id) => id,
        None => {
            generated = crate::commands::new_id("scheduled");
            &generated
        }
    };
    create_definition(state, loupist_id, draft, id, Default::default())
}

pub(crate) fn create_definition(
    state: &AppState,
    loupist_id: &str,
    draft: TaskDraft,
    request_id: &str,
    definition: TaskDefinition,
) -> Result<Task, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::ChatMutation)?;
    state.ensure_loupist_available(loupist_id)?;
    validate_draft(state, &draft)?;
    state
        .store
        .scheduled_task_create_definition(
            loupist_id,
            request_id,
            draft,
            TaskKind::Custom,
            definition,
            now_ms(),
        )
        .map_err(map_store)
        .and_then(|task| saved_task(state, task))
}

pub(crate) fn set_delivery(
    state: &AppState,
    loupist_id: &str,
    id: &str,
    revision: u64,
    to: Option<DeliveryTarget>,
    notify: Option<NotifyPolicy>,
) -> Result<Task, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::ChatMutation)?;
    state.ensure_loupist_available(loupist_id)?;
    if to.is_none() && notify.is_none() {
        return Err(CommandError::invalid(
            "Choose a destination or notification policy",
        ));
    }
    let task = state
        .store
        .scheduled_task_set_delivery(loupist_id, id, revision, to, notify, now_ms())
        .map_err(map_store)?;
    state.scheduled.changed();
    crate::briefings::desktop::changed(loupist_id);
    Ok(present(state, task))
}

pub(crate) fn update(
    state: &AppState,
    loupist_id: &str,
    id: &str,
    revision: u64,
    draft: TaskDraft,
    watch: TaskWatch,
    active_hours: Option<TriggerActiveHours>,
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
        // The store refuses a kind another version wrote.
        super::types::TaskKind::Custom | super::types::TaskKind::Unknown(_) => {}
    }
    validate_draft(state, &draft)?;
    state
        .store
        .scheduled_task_update_definition(
            loupist_id,
            id,
            revision,
            draft,
            Some((watch, active_hours)),
            now_ms(),
        )
        .map_err(map_store)
        .and_then(|task| saved_task(state, task))
}

/// Keep the existing definition runnable while its replacement waits for a tap.
pub(crate) fn propose_change(
    state: &AppState,
    current: &Task,
    expected_revision: u64,
    mut draft: TaskDraft,
    watch: TaskWatch,
    message_id: &str,
) -> Result<Task, CommandError> {
    draft.status = TaskStatus::Paused;
    if current.awaiting_confirmation {
        return update(
            state,
            &current.loupist_id,
            &current.id,
            expected_revision,
            draft,
            watch,
            current
                .trigger
                .as_ref()
                .and_then(|check| check.active_hours.clone()),
        );
    }
    let key = crate::util::sha256(
        format!("{}\0{expected_revision}\0{message_id}", current.id).as_bytes(),
    );
    create_definition(
        state,
        &current.loupist_id,
        draft,
        &format!("schedule-proposal-{}", &key[..32]),
        TaskDefinition {
            watch,
            proposed_in_message_id: Some(message_id.to_owned()),
            trigger: current.trigger.clone(),
            replaces: Some(ProposalTarget {
                id: current.id.clone(),
                revision: expected_revision,
            }),
            ..Default::default()
        },
    )
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
        let mut draft = get(state, loupist_id, id)?.draft();
        draft.status = status;
        validate_draft(state, &draft)?;
    }
    state
        .store
        .scheduled_task_set_status(loupist_id, id, revision, status, now_ms())
        .map_err(map_store)
        .and_then(|task| saved_task(state, task))
}

/// The user's one-tap OK in the Chat card for a task the agent proposed.
pub(crate) fn confirm(
    state: &AppState,
    loupist_id: &str,
    id: &str,
    revision: u64,
) -> Result<Task, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::ChatMutation)?;
    state.ensure_loupist_available(loupist_id)?;
    let mut draft = get(state, loupist_id, id)?.draft();
    draft.status = TaskStatus::Active;
    validate_draft(state, &draft)?;
    state
        .store
        .scheduled_task_confirm(loupist_id, id, revision, now_ms())
        .map_err(map_store)
        .and_then(|task| saved_task(state, task))
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
        .inspect(|_| {
            state.scheduled.changed();
            crate::briefings::desktop::changed(loupist_id);
        })
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
        .map(|task| task.map(|task| present(state, task)))
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
    // Checked here so that the agent's `schedule_delete` meets the same rule
    // as the app: only a custom task is deleted.
    if get(state, loupist_id, id)?.kind == super::types::TaskKind::Briefing {
        return Err(CommandError::invalid(
            "Turn the briefing off instead of deleting it",
        ));
    }
    let sessions = state
        .store
        .scheduled_task_delete(loupist_id, id, revision)
        .map_err(map_store)?;
    crate::chat::questions::forget_task(loupist_id, id);
    state.scheduled.changed();
    crate::briefings::desktop::changed(loupist_id);
    release_sessions(state, loupist_id, Some(&sessions));
    Ok(())
}

/// Deletes the scheduler-only run sessions among `candidates` (every session
/// the scheduler made, when `None`) that nothing in Schedule points at any
/// more (`scheduled_released_sessions`), through the same path as the user's
/// Delete. One that cannot go now, such as a session with a turn running in
/// it, is left for a later release or the next startup. Returns how many went.
pub(crate) fn release_sessions(
    state: &AppState,
    loupist_id: &str,
    candidates: Option<&[String]>,
) -> usize {
    if candidates.is_some_and(<[String]>::is_empty) {
        return 0;
    }
    let released = match state
        .store
        .scheduled_released_sessions(loupist_id, candidates)
    {
        Ok(released) => released,
        Err(_) => {
            eprintln!("Scheduled run sessions could not be read for release");
            return 0;
        }
    };
    let mut count = 0;
    for thread_id in released {
        let result =
            crate::commands::records::delete_chat_session(state, loupist_id, &thread_id, |_| {
                // Checked again while the session is held: the user may have
                // written in it, or a task may point at it now.
                let still = state
                    .store
                    .scheduled_released_sessions(loupist_id, Some(std::slice::from_ref(&thread_id)))
                    .map_err(map_store)?;
                if still.is_empty() {
                    Err(CommandError::conflict("the run session is in use again"))
                } else {
                    Ok(())
                }
            });
        match result {
            Ok(()) => count += 1,
            Err(error) => eprintln!("A scheduled run session was kept for now: {}", error.code),
        }
    }
    count
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
    if get(state, loupist_id, id)?.awaiting_confirmation {
        return Err(CommandError::conflict(
            "This task needs your approval before it can run",
        ));
    }
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
