use serde::Deserialize;
use tauri::{Emitter, State};

use crate::app::{AppState, CommandError};
use crate::knowledge::{
    apply_proposal, dismiss_proposal, load_settings, remove_schema_item, schema_view,
    store_settings, WorldSchemaView,
};

use super::{admit, admit_mutation, with_loupist_knowledge};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoupistRequest {
    loupist_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposalRequest {
    loupist_id: String,
    proposal_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveRequest {
    loupist_id: String,
    kind: String,
    name: String,
}

fn changed(app: &tauri::AppHandle, loupist_id: &str) {
    let _ = app.emit(
        "onWorldSchemaChanged",
        serde_json::json!({ "loupistId": loupist_id }),
    );
}

#[tauri::command(async)]
pub fn world_schema_get(
    request: LoupistRequest,
    state: State<'_, AppState>,
) -> Result<WorldSchemaView, CommandError> {
    admit(state.inner(), &request.loupist_id)?;
    with_loupist_knowledge(state.inner(), &request.loupist_id, |tx, _| {
        let settings = load_settings(tx, &request.loupist_id)?;
        schema_view(tx, &request.loupist_id, &settings.extension)
    })
}

#[tauri::command(async)]
pub fn world_schema_apply(
    app: tauri::AppHandle,
    request: ProposalRequest,
    state: State<'_, AppState>,
) -> Result<WorldSchemaView, CommandError> {
    let _activity = admit_mutation(state.inner(), &request.loupist_id)?;
    let view = with_loupist_knowledge(state.inner(), &request.loupist_id, |tx, _| {
        let mut settings = load_settings(tx, &request.loupist_id)?;
        apply_proposal(
            tx,
            &request.loupist_id,
            &mut settings.extension,
            &request.proposal_id,
            crate::commands::now_ms(),
        )?;
        store_settings(tx, &request.loupist_id, &settings)?;
        schema_view(tx, &request.loupist_id, &settings.extension)
    })?;
    changed(&app, &request.loupist_id);
    Ok(view)
}

#[tauri::command(async)]
pub fn world_schema_dismiss(
    app: tauri::AppHandle,
    request: ProposalRequest,
    state: State<'_, AppState>,
) -> Result<WorldSchemaView, CommandError> {
    let _activity = admit_mutation(state.inner(), &request.loupist_id)?;
    let view = with_loupist_knowledge(state.inner(), &request.loupist_id, |tx, _| {
        dismiss_proposal(
            tx,
            &request.loupist_id,
            &request.proposal_id,
            crate::commands::now_ms(),
        )?;
        let settings = load_settings(tx, &request.loupist_id)?;
        schema_view(tx, &request.loupist_id, &settings.extension)
    })?;
    changed(&app, &request.loupist_id);
    Ok(view)
}

#[tauri::command(async)]
pub fn world_schema_remove(
    app: tauri::AppHandle,
    request: RemoveRequest,
    state: State<'_, AppState>,
) -> Result<WorldSchemaView, CommandError> {
    let _activity = admit_mutation(state.inner(), &request.loupist_id)?;
    let view = with_loupist_knowledge(state.inner(), &request.loupist_id, |tx, _| {
        let mut settings = load_settings(tx, &request.loupist_id)?;
        remove_schema_item(
            tx,
            &request.loupist_id,
            &mut settings.extension,
            &request.kind,
            &request.name,
            crate::commands::now_ms(),
        )?;
        store_settings(tx, &request.loupist_id, &settings)?;
        schema_view(tx, &request.loupist_id, &settings.extension)
    })?;
    changed(&app, &request.loupist_id);
    Ok(view)
}
