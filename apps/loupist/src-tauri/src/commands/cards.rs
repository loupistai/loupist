//! Turn a stored offer on or off, or answer a question a run left behind.

use serde::Deserialize;
use tauri::State;

use crate::{
    app::{AppState, CommandError},
    scheduled_tasks::types::Task,
};

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OfferRequest {
    pub loupist_id: String,
    pub offer_id: String,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AskAnswerRequest {
    pub loupist_id: String,
    pub ask_id: String,
    pub choice: u8,
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn bot_offer_accept(
    request: OfferRequest,
    state: State<'_, AppState>,
) -> Result<Task, CommandError> {
    let task = accept(&state, &request)?;
    if task.status == crate::scheduled_tasks::types::TaskStatus::Active
        && task.last_run_at_ms.is_none()
        && task
            .next_run_at_ms
            .is_some_and(|due| due <= crate::commands::now_ms())
    {
        let _ = state
            .scheduled
            .dispatch(&state, &task.loupist_id, &task.id, false)
            .await;
    }
    Ok(crate::scheduled_tasks::service::find(&state, &task.loupist_id, &task.id)?.unwrap_or(task))
}

pub(crate) fn accept(state: &AppState, request: &OfferRequest) -> Result<Task, CommandError> {
    state.ensure_loupist_available(&request.loupist_id)?;
    crate::chat::cards::accept(state, &request.loupist_id, &request.offer_id)
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn bot_offer_dismiss(
    request: OfferRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state.ensure_loupist_available(&request.loupist_id)?;
    crate::chat::cards::dismiss(&state, &request.loupist_id, &request.offer_id)
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn bot_ask_answer(
    request: AskAnswerRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state.ensure_loupist_available(&request.loupist_id)?;
    crate::chat::cards::answer(&state, &request.loupist_id, &request.ask_id, request.choice)
}
