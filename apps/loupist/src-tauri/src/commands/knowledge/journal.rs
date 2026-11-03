use super::*;
use crate::knowledge::{journal as storage, JournalRecord};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalListRequest {
    pub loupist_id: String,
    pub include_archived: Option<bool>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalCreateRequest {
    pub loupist_id: String,
    pub judgment: String,
    pub reasoning: Option<String>,
    pub entities: Option<Vec<String>>,
    pub check: Option<crate::knowledge::JournalCheck>,
    /// A reason or line on an existing asset page.
    pub thesis: Option<crate::knowledge::assets::JournalThesis>,
}
impl JournalCreateRequest {
    fn input(self) -> storage::JournalInput {
        storage::JournalInput {
            judgment: self.judgment,
            reasoning: self.reasoning.unwrap_or_default(),
            entities: self.entities.unwrap_or_default(),
            check: self.check,
            thesis: self.thesis,
        }
    }
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalUpdateRequest {
    pub loupist_id: String,
    pub journal_id: String,
    pub expected_updated_at_ms: i64,
    pub judgment: String,
    pub reasoning: Option<String>,
    pub entities: Option<Vec<String>>,
    pub check: Option<crate::knowledge::JournalCheck>,
    pub thesis: Option<crate::knowledge::assets::JournalThesis>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalArchiveRequest {
    pub loupist_id: String,
    pub journal_id: String,
    pub archived: bool,
}

#[tauri::command]
pub fn journal_list(
    state: State<'_, AppState>,
    request: JournalListRequest,
) -> Result<Vec<JournalRecord>, CommandError> {
    admit(&state, &request.loupist_id)?;
    with_loupist_knowledge(&state, &request.loupist_id, |tx, _| {
        Ok(storage::list(tx, &request.loupist_id)?
            .into_iter()
            .filter(|j| request.include_archived.unwrap_or(false) || !j.archived)
            .collect())
    })
}
#[tauri::command]
pub fn journal_create(
    state: State<'_, AppState>,
    request: JournalCreateRequest,
) -> Result<JournalRecord, CommandError> {
    let loupist = request.loupist_id.clone();
    let _activity = admit_mutation(&state, &loupist)?;
    with_loupist_knowledge(&state, &loupist, |tx, _| {
        storage::create(tx, &loupist, request.input(), crate::commands::now_ms())
    })
}
#[tauri::command]
pub fn journal_update(
    state: State<'_, AppState>,
    request: JournalUpdateRequest,
) -> Result<JournalRecord, CommandError> {
    let _activity = admit_mutation(&state, &request.loupist_id)?;
    with_loupist_knowledge(&state, &request.loupist_id, |tx, _| {
        storage::update(
            tx,
            &request.loupist_id,
            &request.journal_id,
            request.expected_updated_at_ms,
            storage::JournalInput {
                judgment: request.judgment,
                reasoning: request.reasoning.unwrap_or_default(),
                entities: request.entities.unwrap_or_default(),
                check: request.check,
                thesis: request.thesis,
            },
            crate::commands::now_ms(),
        )
    })
}
#[tauri::command]
pub fn journal_archive(
    state: State<'_, AppState>,
    request: JournalArchiveRequest,
) -> Result<JournalRecord, CommandError> {
    let _activity = admit_mutation(&state, &request.loupist_id)?;
    with_loupist_knowledge(&state, &request.loupist_id, |tx, _| {
        storage::archive(
            tx,
            &request.loupist_id,
            &request.journal_id,
            request.archived,
            crate::commands::now_ms(),
        )
    })
}

/// Journal records with a check and no outcome yet, for the attention list.
pub(crate) fn open_checks(
    state: &AppState,
    loupist_id: &str,
) -> Result<Vec<JournalRecord>, CommandError> {
    with_loupist_knowledge(state, loupist_id, |tx, _| {
        Ok(storage::list(tx, loupist_id)?
            .into_iter()
            .filter(|j| !j.archived && j.outcome.is_none() && j.check.is_some())
            .collect())
    })
}
