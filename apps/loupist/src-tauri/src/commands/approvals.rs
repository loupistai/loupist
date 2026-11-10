//! The user's answer to an approval card in a running Chat (composer spec D3).

use serde::Deserialize;
use tauri::State;

use crate::{
    app::{AppState, CommandError},
    chat::approvals::{self, ApprovalDecision},
};

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct ApprovalDecideRequest {
    loupist_id: String,
    request_id: String,
    decision: ApprovalDecision,
}

/// Resumes the waiting tool call. "Always" is stored by the connection service for
/// this Loupist, connection and tool, and stays revocable in Settings › Connections.
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn chat_approval_decide(
    request: ApprovalDecideRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state.ensure_loupist_available(&request.loupist_id)?;
    approvals::decide(&request.loupist_id, &request.request_id, request.decision)
        .map_err(|_| CommandError::conflict("This approval is no longer waiting"))
}
