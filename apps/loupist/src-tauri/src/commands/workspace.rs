use serde::Deserialize;
use tauri::State;

use crate::{
    app::{AppState, CommandError},
    chat::activity::ChatActivityDetail,
    fs::loupist_root::{profile_workspace, BoundLoupistRoot},
    fs::workspace_files::{self, WorkspaceDirectory, WorkspaceFile},
};

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(deny_unknown_fields)]
pub struct WorkspacePathRequest {
    pub loupist_id: String,
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ChatActivityRequest {
    pub loupist_id: String,
    pub thread_id: String,
    pub detail_id: String,
}

fn workspace(state: &AppState, loupist_id: &str) -> Result<BoundLoupistRoot, CommandError> {
    state.ensure_loupist_available(loupist_id)?;
    let profile = state
        .store
        .get_loupist(loupist_id)
        .map_err(super::map_store)?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    profile_workspace(&profile)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn workspace_list(
    request: WorkspacePathRequest,
    state: State<'_, AppState>,
) -> Result<WorkspaceDirectory, CommandError> {
    let root = workspace(&state, &request.loupist_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        workspace_files::list(&root, request.path.as_deref().unwrap_or("."))
    })
    .await
    .map_err(super::map_internal)?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn workspace_read(
    request: WorkspacePathRequest,
    state: State<'_, AppState>,
) -> Result<WorkspaceFile, CommandError> {
    let root = workspace(&state, &request.loupist_id)?;
    let path = request
        .path
        .ok_or_else(|| CommandError::invalid("select a workspace file"))?;
    tauri::async_runtime::spawn_blocking(move || workspace_files::read(&root, &path))
        .await
        .map_err(super::map_internal)?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn workspace_reveal(
    request: WorkspacePathRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let root = workspace(&state, &request.loupist_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let relative =
            workspace_files::check_existing(&root, request.path.as_deref().unwrap_or("."))?;
        let path = root.path().join(relative);
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("/usr/bin/open")
                .arg("-R")
                .arg(path)
                .spawn()
                .map_err(super::map_internal)?;
        }
        #[cfg(windows)]
        {
            std::process::Command::new("explorer.exe")
                .arg(format!("/select,{}", path.display()))
                .spawn()
                .map_err(super::map_internal)?;
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let directory = if path.is_dir() {
                path.as_path()
            } else {
                path.parent().unwrap_or(root.path())
            };
            open::that_detached(directory).map_err(super::map_internal)?;
        }
        Ok(())
    })
    .await
    .map_err(super::map_internal)?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn chat_activity_read(
    request: ChatActivityRequest,
    state: State<'_, AppState>,
) -> Result<ChatActivityDetail, CommandError> {
    state.ensure_loupist_available(&request.loupist_id)?;
    let chat = state
        .store
        .get_chat(&request.thread_id)
        .map_err(super::map_store)?
        .ok_or_else(|| CommandError::not_found("chat thread"))?;
    if chat.loupist_id != request.loupist_id {
        return Err(CommandError::conflict(
            "chat thread does not belong to this Loupist",
        ));
    }
    let active = state.run_coordinator.activities()?;
    let root = state.loupist_files_root(&request.loupist_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::chat::activity::read(
            &root,
            &request.loupist_id,
            &request.thread_id,
            &request.detail_id,
            |run_id| {
                active.iter().any(|run| {
                    run.run_id == run_id
                        && run.loupist_id == request.loupist_id
                        && run.target == request.thread_id
                })
            },
        )
    })
    .await
    .map_err(super::map_internal)?
}
