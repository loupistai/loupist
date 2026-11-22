//! Typed renderer access to the Rust-owned configuration service.
use std::path::PathBuf;

use tauri::State;

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
    let snapshot = scoped_config::read_scope(
        &state.artifacts.app_data_dir,
        project.as_deref(),
        request.scope,
    )?;
    present(&state, snapshot)
}

pub(crate) fn present(
    state: &AppState,
    mut snapshot: ConfigSnapshot,
) -> Result<ConfigSnapshot, CommandError> {
    let chat = &snapshot.effective.chat;
    let requested = chat.model_profile_id.as_deref().unwrap_or_default();
    if !snapshot.diagnostics.is_empty() {
        snapshot.model_selection = Some(
            crate::providers::selection::ResolvedModelSelection::unavailable(
                crate::providers::selection::ModelUnavailableReason::InvalidConfiguration,
                crate::providers::auto::uses_default(requested),
            ),
        );
        return Ok(snapshot);
    }
    let Ok(catalog) = state.selection_catalog() else {
        snapshot.model_selection = Some(
            crate::providers::selection::ResolvedModelSelection::catalog_unavailable(requested),
        );
        return Ok(snapshot);
    };
    let selection = catalog.selection(
        requested,
        chat.thinking_level.as_deref().unwrap_or_default(),
        &chat.run_options,
    );
    if let ConfigScope::Project { loupist_id } = &snapshot.scope {
        snapshot.default_model_change = state
            .store
            .observe_default_model(loupist_id, &selection)
            .map_err(super::map_store)?;
    }
    snapshot.model_selection = Some(selection);
    Ok(snapshot)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn config_update(
    request: ConfigUpdateRequest,
    state: State<'_, AppState>,
) -> Result<ConfigSnapshot, CommandError> {
    // Scope changes leave their saved item pending for its own connection action.
    crate::app::off_main_thread(&state, move |state| update_config(request, state)).await
}

fn update_config(
    request: ConfigUpdateRequest,
    state: &AppState,
) -> Result<ConfigSnapshot, CommandError> {
    let project = project_directory(state, &request.scope)?;
    let (snapshot, display_changed) = scoped_config::update_scope_with_display_change(
        &state.artifacts.app_data_dir,
        project.as_deref(),
        &request,
    )?;
    if display_changed {
        state.fx_reference.invalidate_display();
    }
    // Saving is complete before reconciliation. If invalidation fails the new
    // file remains the source of truth and the connection stays blocked.
    if matches!(request.scope, ConfigScope::Global) {
        crate::marketplace::connector_config::reconcile_connector_configs(state).map_err(
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
    present(state, snapshot)
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
    crate::process_lease::macos_commands::spawn(
        std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(target),
    )
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
