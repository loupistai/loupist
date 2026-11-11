//! Skills UI uses registered scopes and discovered identities, never arbitrary write paths.
use crate::{
    app::{AppState, CommandError},
    scoped_config::{self, ConfigReadRequest, ConfigScope},
    skills::{self, ManagedSkill, SkillCatalog, SkillDocument},
};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tauri::State;

fn directory(state: &AppState, scope: &ConfigScope) -> Result<PathBuf, CommandError> {
    match scope {
        ConfigScope::Global => Ok(state.artifacts.app_data_dir.clone()),
        ConfigScope::Project { loupist_id } => {
            state.ensure_loupist_available(loupist_id)?;
            super::loupist_config_directory(state, loupist_id)
        }
    }
}
pub(crate) fn list(state: &AppState, scope: &ConfigScope) -> Result<SkillCatalog, CommandError> {
    let directory = directory(state, scope)?;
    let project = matches!(scope, ConfigScope::Project { .. }).then_some(directory.as_path());
    let config = scoped_config::read_scope(&state.artifacts.app_data_dir, project, scope.clone())?;
    skills::catalog(
        &state.artifacts.app_data_dir,
        project.and_then(Path::parent),
        config,
    )
}
fn find(state: &AppState, scope: &ConfigScope, key: &str) -> Result<ManagedSkill, CommandError> {
    list(state, scope)?
        .skills
        .into_iter()
        .find(|s| s.path.as_deref().unwrap_or(&s.id) == key)
        .ok_or_else(|| CommandError::not_found("Skill"))
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillReadRequest {
    pub scope: ConfigScope,
    pub key: String,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillCreateRequest {
    pub scope: ConfigScope,
    pub name: String,
    pub description: String,
    pub instructions: String,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillSaveRequest {
    pub scope: ConfigScope,
    pub key: String,
    pub expected_revision: String,
    pub description: String,
    pub instructions: String,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillDeleteRequest {
    pub scope: ConfigScope,
    pub key: String,
    pub expected_revision: String,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillImportRequest {
    pub scope: ConfigScope,
    pub kind: ImportKind,
    pub url: Option<String>,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ImportKind {
    Folder,
    Zip,
    Github,
}

#[tauri::command]
pub fn skills_list(
    request: ConfigReadRequest,
    state: State<'_, AppState>,
) -> Result<SkillCatalog, CommandError> {
    list(&state, &request.scope)
}
#[tauri::command]
pub fn skills_read(
    request: SkillReadRequest,
    state: State<'_, AppState>,
) -> Result<SkillDocument, CommandError> {
    skills::document(find(&state, &request.scope, &request.key)?)
}
#[tauri::command]
pub fn skills_create(
    request: SkillCreateRequest,
    state: State<'_, AppState>,
) -> Result<SkillCatalog, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(crate::runs::maintenance::MaintenanceActivityKind::LoupistMutation)?;
    skills::create(
        &directory(&state, &request.scope)?,
        &request.name,
        &request.description,
        &request.instructions,
    )?;
    list(&state, &request.scope)
}
#[tauri::command]
pub fn skills_save(
    request: SkillSaveRequest,
    state: State<'_, AppState>,
) -> Result<SkillCatalog, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(crate::runs::maintenance::MaintenanceActivityKind::LoupistMutation)?;
    skills::save(
        &find(&state, &request.scope, &request.key)?,
        &request.expected_revision,
        &request.description,
        &request.instructions,
    )?;
    list(&state, &request.scope)
}
#[tauri::command]
pub fn skills_delete(
    request: SkillDeleteRequest,
    state: State<'_, AppState>,
) -> Result<SkillCatalog, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(crate::runs::maintenance::MaintenanceActivityKind::LoupistMutation)?;
    skills::remove(
        &find(&state, &request.scope, &request.key)?,
        &request.expected_revision,
    )?;
    list(&state, &request.scope)
}
#[tauri::command]
pub fn skills_reveal(
    request: SkillReadRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let skill = find(&state, &request.scope, &request.key)?;
    let path = skill
        .path
        .ok_or_else(|| CommandError::invalid("Included skills are managed by the app"))?;
    open::that_detached(
        Path::new(&path)
            .parent()
            .ok_or_else(|| CommandError::invalid("Skill folder is missing"))?,
    )
    .map_err(super::map_internal)
}
#[tauri::command]
pub async fn skills_import(
    request: SkillImportRequest,
    state: State<'_, AppState>,
) -> Result<Option<SkillCatalog>, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(crate::runs::maintenance::MaintenanceActivityKind::LoupistMutation)?;
    let root = directory(&state, &request.scope)?;
    let package = match request.kind {
        ImportKind::Github => {
            skills::from_github(
                request
                    .url
                    .as_deref()
                    .ok_or_else(|| CommandError::invalid("Enter a GitHub folder URL"))?,
            )
            .await?
        }
        ImportKind::Folder => {
            let Some(folder) = rfd::AsyncFileDialog::new()
                .set_title("Choose a skill folder containing SKILL.md")
                .pick_folder()
                .await
            else {
                return Ok(None);
            };
            skills::from_folder(&folder.path().canonicalize().map_err(super::map_internal)?)?
        }
        ImportKind::Zip => {
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_title("Choose a skill ZIP package")
                .add_filter("Skill package", &["zip", "skill"])
                .pick_file()
                .await
            else {
                return Ok(None);
            };
            let bytes = crate::fs::artifact_trust::read_private_regular_file_bounded(
                file.path(),
                16 * 1024 * 1024,
            )
            .map_err(super::map_internal)?;
            skills::from_zip(bytes, None)?
        }
    };
    // Folder selection or download can yield; revalidate the registered scope.
    if directory(&state, &request.scope)? != root {
        return Err(CommandError::conflict(
            "Skill destination changed; try again",
        ));
    }
    skills::install(&root, package)?;
    list(&state, &request.scope).map(Some)
}
