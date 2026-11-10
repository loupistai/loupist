use super::*;
use crate::knowledge::learning::{self, LearningSettings, Review, ReviewQuery};
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "MemoryLearningSettingsRequest"))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsRequest {
    pub loupist_id: String,
    pub learn_methods: bool,
}
#[tauri::command]
pub fn learning_review_list(
    state: State<'_, AppState>,
    request: ReviewQuery,
) -> Result<Vec<Review>, CommandError> {
    admit(&state, &request.loupist_id)?;
    with_loupist_knowledge(&state, &request.loupist_id, |tx, _| {
        learning::list(tx, &request)
    })
}
#[tauri::command]
pub fn memory_learning_settings_get(
    state: State<'_, AppState>,
    request: KnowledgeCatalogRequest,
) -> Result<LearningSettings, CommandError> {
    admit(&state, &request.loupist_id)?;
    with_loupist_knowledge(&state, &request.loupist_id, |tx, _| {
        learning::settings(tx, &request.loupist_id)
    })
}
#[tauri::command]
pub fn memory_learning_settings_set(
    state: State<'_, AppState>,
    request: SettingsRequest,
) -> Result<LearningSettings, CommandError> {
    let _activity = admit_mutation(&state, &request.loupist_id)?;
    let settings = with_loupist_knowledge(&state, &request.loupist_id, |tx, _| {
        learning::save_settings(tx, &request.loupist_id, request.learn_methods)
    })?;
    state.graph.wake();
    Ok(settings)
}
