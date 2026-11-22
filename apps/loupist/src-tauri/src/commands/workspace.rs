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

fn document_workspace(
    state: &AppState,
    id: &str,
    path: &str,
) -> Result<BoundLoupistRoot, CommandError> {
    use crate::agent_harness::native_policy::{classify, NativeRoots, PathClass};
    let root = workspace(state, id)?;
    let relative = workspace_files::relative_path(path)?;
    let private = &state.artifacts.app_data_dir;
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(std::path::PathBuf::from);
    // There is no run or attachment grant on a desktop document request.
    let data = state.store.loupist_data_dir(id).map_err(super::map_store)?;
    let mut roots = NativeRoots::new(
        root.path(),
        private,
        private,
        &data,
        &data,
        &data,
        home.as_deref(),
    );
    if let Some(agent) = state
        .artifacts
        .pi
        .as_ref()
        .and_then(|pi| pi.system_prompt.parent())
    {
        roots = roots.with_agent_skills(&agent.join("skills"));
    }
    if classify(&root.path().join(relative), &roots).class != PathClass::Workspace {
        return Err(CommandError::invalid(
            "select a document inside the workspace",
        ));
    }
    Ok(root)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn workspace_workbook_read(
    request: crate::python_run::workbook::WorkbookRequest,
    on_event: tauri::ipc::Channel<crate::python_run::workbook::WorkbookProgress>,
    state: State<'_, AppState>,
) -> Result<crate::python_run::workbook::WorkbookRead, CommandError> {
    let root = document_workspace(&state, &request.loupist_id, &request.path)?;
    crate::python_run::workbook::read(
        root,
        request,
        &state.workbook_cache,
        &state.artifacts.python_cache_root,
        |event| {
            let _ = on_event.send(event);
        },
    )
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn workspace_document_handler(
    request: crate::fs::workspace_document::WorkspaceDocumentRequest,
    state: State<'_, AppState>,
) -> Result<crate::fs::workspace_document::WorkspaceDocumentHandler, CommandError> {
    use crate::fs::workspace_document::{self, WorkspaceDocumentHandler};
    with_workspace_document(
        &state,
        request,
        |reason| WorkspaceDocumentHandler::Refused { reason },
        workspace_document::handler,
    )
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn workspace_document_open(
    request: crate::fs::workspace_document::WorkspaceDocumentRequest,
    state: State<'_, AppState>,
) -> Result<crate::fs::workspace_document::WorkspaceDocumentOpen, CommandError> {
    use crate::fs::workspace_document::{self, WorkspaceDocumentOpen};
    with_workspace_document(
        &state,
        request,
        |reason| WorkspaceDocumentOpen::Refused { reason },
        workspace_document::open,
    )
    .await
}

async fn with_workspace_document<T: Send + 'static>(
    state: &AppState,
    request: crate::fs::workspace_document::WorkspaceDocumentRequest,
    refused: impl FnOnce(crate::fs::workspace_document::DocumentRefusal) -> T,
    action: fn(&BoundLoupistRoot, &str) -> T,
) -> Result<T, CommandError> {
    use crate::fs::workspace_document::DocumentRefusal;
    let root = match document_workspace(state, &request.loupist_id, &request.path) {
        Ok(root) => root,
        Err(error) if error.code == crate::app::CommandErrorCode::InvalidRequest => {
            return Ok(refused(DocumentRefusal::OutsideWorkspace));
        }
        Err(error) => return Err(error),
    };
    tauri::async_runtime::spawn_blocking(move || action(&root, &request.path))
        .await
        .map_err(super::map_internal)
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
        let mut detail = crate::chat::activity::read(
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
        )?;
        crate::chat::local_files::FileLabels::for_chat(&chat).project_activity(&mut detail);
        Ok(detail)
    })
    .await
    .map_err(super::map_internal)?
}
