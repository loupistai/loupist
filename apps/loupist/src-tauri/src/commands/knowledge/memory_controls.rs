use super::*;
use crate::knowledge::{
    memory_controls as storage, memory_controls::MemoryControls, memory_reset,
    memory_reset::ResetCounts,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PauseRequest {
    pub loupist_id: String,
    pub record_id: String,
    pub paused: bool,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResetRequest {
    pub loupist_id: String,
    pub confirm: bool,
    pub expected: ResetCounts,
}

#[tauri::command]
pub fn knowledge_memory_controls(
    state: State<'_, AppState>,
    request: KnowledgeCatalogRequest,
) -> Result<MemoryControls, CommandError> {
    admit(&state, &request.loupist_id)?;
    with_loupist_knowledge(&state, &request.loupist_id, |tx, _| {
        storage::load(tx, &request.loupist_id)
    })
}
#[tauri::command]
pub fn knowledge_lens_pause(
    state: State<'_, AppState>,
    request: PauseRequest,
) -> Result<MemoryControls, CommandError> {
    let _activity = admit_mutation(&state, &request.loupist_id)?;
    with_loupist_knowledge(&state, &request.loupist_id, |tx, _| {
        storage::pause(tx, &request.loupist_id, &request.record_id, request.paused)
    })
}
#[tauri::command]
pub fn knowledge_memory_reset_preview(
    state: State<'_, AppState>,
    request: KnowledgeCatalogRequest,
) -> Result<ResetCounts, CommandError> {
    admit(&state, &request.loupist_id)?;
    with_loupist_knowledge(&state, &request.loupist_id, |tx, _| {
        memory_reset::preview(tx, &request.loupist_id)
    })
}
#[tauri::command]
pub fn knowledge_memory_reset(
    state: State<'_, AppState>,
    request: ResetRequest,
) -> Result<ResetCounts, CommandError> {
    let _activity = admit_mutation(&state, &request.loupist_id)?;
    if !request.confirm {
        return Err(CommandError::invalid("Reset needs confirmation"));
    }
    let counts = with_loupist_knowledge(&state, &request.loupist_id, |tx, blobs| {
        memory_reset::reset(
            tx,
            blobs,
            &request.loupist_id,
            &request.expected,
            crate::commands::now_ms(),
        )
    })?;
    state.graph.wake();
    Ok(counts)
}
