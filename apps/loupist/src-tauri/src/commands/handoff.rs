use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use super::{loupist_configuration, map_store};
use crate::{
    app::{AppState, CommandError},
    chat::handoff::{HandoffRequest, HandoffRun, StagedHandoff},
};

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ResearchHandoffRequest {
    loupist_id: String,
    title: String,
    question: String,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    brief: String,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pins: Vec<Value>,
    request_key: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResearchHandoffReceipt {
    chat_id: String,
    handoff_id: String,
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn research_handoff_start(
    request: ResearchHandoffRequest,
    state: State<'_, AppState>,
) -> Result<ResearchHandoffReceipt, CommandError> {
    let state = state.inner();
    state.ensure_loupist_available(&request.loupist_id)?;
    let parsed = HandoffRequest::parse(&serde_json::json!({
        "title": request.title,
        "question": request.question,
        "brief": request.brief,
        "pins": request.pins,
        "requestKey": request.request_key,
    }))
    .map_err(|(field, expected)| CommandError::invalid(format!("{field}: {expected}")))?;
    let profile = state
        .store
        .get_loupist(&request.loupist_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    let chat = loupist_configuration(state, &profile.id)?.chat;
    let staged = StagedHandoff {
        request: parsed,
        run: HandoffRun {
            model_profile_id: chat
                .model_profile_id
                .or(profile.last_model_profile_id)
                .unwrap_or_default(),
            thinking_level: chat.thinking_level.unwrap_or_default(),
            run_options: chat.run_options,
            use_memory: chat.use_memory,
            update_memory: chat.update_memory,
        },
    };
    let handoff = crate::chat::handoff::commit_standalone(state, &profile.id, &staged)?;
    crate::chat::surface::ChatSurface::Bot.after_turn(state, &profile.id);
    Ok(ResearchHandoffReceipt {
        chat_id: handoff.chat_id,
        handoff_id: handoff.id,
    })
}
