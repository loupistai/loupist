//! Typed renderer access to the Rust-owned configuration service.
use std::path::PathBuf;

use tauri::{Emitter, State};

use crate::{
    app::{AppState, CommandError},
    scoped_config::{self, ConfigReadRequest, ConfigScope, ConfigSnapshot, ConfigUpdateRequest},
};

fn project_directory(
    state: &AppState,
    scope: &ConfigScope,
) -> Result<Option<PathBuf>, CommandError> {
    match scope {
        ConfigScope::Global => Ok(None),
        ConfigScope::Project { loupist_id } => {
            state.ensure_loupist_available(loupist_id)?;
            super::loupist_config_directory(state, loupist_id).map(Some)
        }
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn config_read(
    request: ConfigReadRequest,
    state: State<'_, AppState>,
) -> Result<ConfigSnapshot, CommandError> {
    let project = project_directory(&state, &request.scope)?;
    scoped_config::read_scope(
        &state.artifacts.app_data_dir,
        project.as_deref(),
        request.scope,
    )
}

#[tauri::command(rename_all = "snake_case")]
pub fn config_update(
    request: ConfigUpdateRequest,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<ConfigSnapshot, CommandError> {
    let project = project_directory(&state, &request.scope)?;
    let snapshot =
        scoped_config::update_scope(&state.artifacts.app_data_dir, project.as_deref(), &request)?;
    // Saving is complete before reconciliation. If invalidation fails the new
    // file remains the source of truth and the connection stays blocked.
    if matches!(request.scope, ConfigScope::Global) {
        crate::marketplace::connector_config::reconcile_connector_configs(&state).map_err(
            |error| {
                CommandError::new(
                    "config_apply_pending",
                    format!(
                        "Configuration was saved, but connection reconciliation is blocked: {}",
                        error.message
                    ),
                )
            },
        )?;
    }
    app.emit("configuration:changed", &request.scope)
        .map_err(super::map_internal)?;
    Ok(snapshot)
}

#[tauri::command(rename_all = "snake_case")]
pub fn config_reveal(
    request: ConfigReadRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let project = project_directory(&state, &request.scope)?;
    let snapshot = scoped_config::read_scope(
        &state.artifacts.app_data_dir,
        project.as_deref(),
        request.scope,
    )?;
    let path = PathBuf::from(snapshot.path);
    // Revealing never creates or overwrites a configuration file. Missing files
    // reveal their already registered containing folder.
    let target = if path.exists() {
        path.as_path()
    } else {
        path.parent()
            .ok_or_else(|| CommandError::internal("Configuration directory is missing"))?
    };
    #[cfg(target_os = "macos")]
    std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(target)
        .spawn()
        .map_err(super::map_internal)?;
    #[cfg(windows)]
    std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", target.display()))
        .spawn()
        .map_err(super::map_internal)?;
    #[cfg(not(any(target_os = "macos", windows)))]
    open::that_detached(if target.is_dir() {
        target
    } else {
        target.parent().unwrap_or(target)
    })
    .map_err(super::map_internal)?;
    Ok(())
}
