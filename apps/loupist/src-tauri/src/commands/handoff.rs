use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tauri::State;

use super::{loupist_configuration, map_internal, map_store};
use crate::{
    app::{AppState, CommandError},
    chat::handoff::{self, HandoffRun, LinkOptions},
};

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ResearchHandoffRequest {
    loupist_id: String,
    connection_id: String,
    model_profile_id: Option<String>,
    thinking_level: Option<String>,
    run_options: Option<BTreeMap<String, String>>,
    use_memory: Option<bool>,
    update_memory: Option<bool>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
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
    let loupist_id = request.loupist_id.clone();
    let receipt = start_link(request, &state)?;
    crate::chat::surface::ChatSurface::Bot.after_turn(&state, &loupist_id);
    Ok(receipt)
}

pub(super) fn start_link(
    request: ResearchHandoffRequest,
    state: &AppState,
) -> Result<ResearchHandoffReceipt, CommandError> {
    let ResearchHandoffRequest {
        loupist_id,
        connection_id,
        model_profile_id,
        thinking_level,
        run_options,
        use_memory,
        update_memory,
    } = request;
    state.ensure_loupist_available(&loupist_id)?;
    let connection_id = super::require_text(&connection_id, "connection id", 128)?;
    let profile = state
        .store
        .get_loupist(&loupist_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    let options = LinkOptions {
        model_profile_id,
        thinking_level,
        run_options,
        use_memory,
        update_memory,
    };
    {
        let connection = state.store.lock_loupist(&loupist_id).map_err(map_store)?;
        if let Some(existing) =
            handoff::link_retry(&connection, &loupist_id, &connection_id, &options)
                .map_err(map_store)?
        {
            return Ok(receipt(existing));
        }
        if handoff::retained_link(&connection, &loupist_id, &connection_id)
            .map_err(map_store)?
            .is_none()
        {
            return Err(CommandError::not_found("retained link"));
        }
    }
    let configuration = loupist_configuration(state, &loupist_id)?;
    let defaults = configuration.chat;
    let mut language = configuration.appearance.language;
    crate::agent_harness::interface::resolve_language(&mut language);
    let model_profile_id = options
        .model_profile_id
        .as_deref()
        .or(defaults.model_profile_id.as_deref())
        .or(profile.last_model_profile_id.as_deref())
        .unwrap_or_default();
    let requested_level = options
        .thinking_level
        .as_deref()
        .filter(|level| !level.is_empty());
    let thinking_level = match requested_level {
        Some(level) => level.to_owned(),
        None => crate::providers::auto::research_level(state, &loupist_id, model_profile_id)?,
    };
    let run_options = match &options.run_options {
        Some(run_options) => run_options.clone(),
        None => match state.model_catalog()?.resolve(model_profile_id) {
            Ok(model) if model.validate_run_options(&defaults.run_options).is_err() => model
                .run_controls
                .into_iter()
                .map(|control| (control.id, control.default_choice))
                .collect(),
            _ => defaults.run_options,
        },
    };
    let execution = state.pi_execution(model_profile_id, &thinking_level, &run_options)?;
    if options
        .model_profile_id
        .as_deref()
        .is_some_and(|id| !id.is_empty() && id != execution.model.id)
        || requested_level.is_some_and(|level| level != execution.thinking_level)
        || options.run_options.as_ref().is_some_and(|controls| {
            controls
                .iter()
                .any(|(key, choice)| execution.run_options.get(key) != Some(choice))
        })
    {
        return Err(CommandError::invalid(
            "the selected Chat settings are unavailable",
        ));
    }
    let run = HandoffRun {
        model_profile_id: execution.model.id,
        thinking_level: execution.thinking_level,
        run_options: execution.run_options,
        use_memory: options.use_memory.unwrap_or(defaults.use_memory),
        update_memory: options.update_memory.unwrap_or(defaults.update_memory),
    };
    let mut connection = state.store.lock_loupist(&loupist_id).map_err(map_store)?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(map_internal)?;
    let handoff = if let Some(existing) =
        handoff::link_retry(&transaction, &loupist_id, &connection_id, &options)
            .map_err(map_store)?
    {
        existing
    } else {
        let link = handoff::retained_link(&transaction, &loupist_id, &connection_id)
            .map_err(map_store)?
            .ok_or_else(|| CommandError::not_found("retained link"))?;
        handoff::commit_link(
            &transaction,
            &loupist_id,
            &link,
            options,
            run,
            &language,
            super::now_ms(),
        )
        .map_err(map_store)?
    };
    transaction.commit().map_err(map_internal)?;
    Ok(receipt(handoff))
}

fn receipt(handoff: handoff::Handoff) -> ResearchHandoffReceipt {
    ResearchHandoffReceipt {
        chat_id: handoff.chat_id,
        handoff_id: handoff.id,
    }
}
