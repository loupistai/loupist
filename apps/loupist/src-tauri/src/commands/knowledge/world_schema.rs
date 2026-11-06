use serde::Deserialize;
use tauri::{Emitter, State};

use crate::app::{AppState, CommandError};
use crate::knowledge::{remove_schema_item, schema_view, undo_change, WorldSchemaView};

use super::{admit, admit_mutation, with_loupist_knowledge};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoupistRequest {
    loupist_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UndoRequest {
    loupist_id: String,
    change_id: String,
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
        schema_view(tx, &request.loupist_id)
    })
}

#[tauri::command(async)]
pub fn world_schema_undo(
    app: tauri::AppHandle,
    request: UndoRequest,
    state: State<'_, AppState>,
) -> Result<WorldSchemaView, CommandError> {
    let _activity = admit_mutation(state.inner(), &request.loupist_id)?;
    let view = with_loupist_knowledge(state.inner(), &request.loupist_id, |tx, _| {
        undo_change(
            tx,
            &request.loupist_id,
            &request.change_id,
            crate::commands::now_ms(),
        )?;
        schema_view(tx, &request.loupist_id)
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
        remove_schema_item(
            tx,
            &request.loupist_id,
            &request.kind,
            &request.name,
            crate::commands::now_ms(),
        )?;
        schema_view(tx, &request.loupist_id)
    })?;
    changed(&app, &request.loupist_id);
    Ok(view)
}
