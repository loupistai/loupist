pub(crate) mod app_settings;
pub(crate) mod approvals;
mod artifact_context;
pub(crate) mod assets;
pub(crate) mod attachments;
pub(crate) mod attention;
pub(crate) mod chat_runtime;
pub(crate) mod configuration;
pub(crate) mod dialog;
pub(crate) mod earnings;
pub(crate) mod export_file;
pub(crate) mod export_trust;
pub(crate) mod handoff;
pub(crate) mod knowledge;
pub(crate) mod loupist;
mod memory_updates;
pub(crate) mod power;
pub(crate) mod questions;
pub(crate) mod records;
pub(crate) mod scheduled_tasks;
pub(crate) mod skills;
mod tool_executor;
pub mod types;
pub(crate) mod user_context;
pub(crate) mod watch;
pub(crate) mod workspace;
pub(crate) mod workspace_ui;

#[cfg(feature = "e2e")]
pub(crate) use loupist::seed_e2e_workspace;
pub use types::*;

use std::collections::BTreeSet;

use chrono::{SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use tauri::State;
use uuid::Uuid;

use crate::{
    app::{AppState, CommandError, LoupistAccess, LoupistAvailability},
    domain::{LoupistCapabilityBinding, LoupistProfile},
    fs::loupist_root::BoundLoupistRoot,
    providers::catalog::ModelCatalogView,
};

pub(crate) const MAX_PROMPT_BYTES: usize = 128 * 1024;
pub(crate) const MAX_CHAT_OUTPUT_BYTES: usize = 512 * 1024;
pub(crate) const MAX_CHAT_CONTEXT_BYTES: usize = 256 * 1024;
pub(crate) const MAX_CHAT_TITLE_BYTES: usize = 200;

#[tauri::command(rename_all = "snake_case")]
pub fn model_catalog_get(state: State<'_, AppState>) -> Result<ModelCatalogView, CommandError> {
    state.model_catalog_view()
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ModelVisibilityUpdateRequest {
    pub model_profile_id: String,
    pub visible_in_chat: bool,
}

#[tauri::command(rename_all = "snake_case")]
pub fn model_visibility_update(
    request: ModelVisibilityUpdateRequest,
    state: State<'_, AppState>,
) -> Result<ModelCatalogView, CommandError> {
    state.set_model_visible(&request.model_profile_id, request.visible_in_chat)?;
    state.model_catalog_view()
}

#[tauri::command(rename_all = "snake_case")]
pub fn run_activity_list(
    state: State<'_, AppState>,
) -> Result<Vec<crate::runs::coordinator::RunActivity>, CommandError> {
    state.run_coordinator.activities()
}

#[tauri::command(rename_all = "snake_case")]
pub fn open_external_url(url: String) -> Result<(), CommandError> {
    let url = crate::web::browser::validated_http_url(&url)?;
    crate::web::external_browser::open(url.as_str())
        .map_err(|error| CommandError::internal(format!("could not open external link: {error}")))
}

pub(crate) fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

pub(crate) fn iso_time(timestamp_ms: i64) -> Result<String, CommandError> {
    Utc.timestamp_millis_opt(timestamp_ms)
        .single()
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true))
        .ok_or_else(|| CommandError::internal("stored timestamp is invalid"))
}

pub(crate) fn require_text(
    value: &str,
    label: &str,
    maximum: usize,
) -> Result<String, CommandError> {
    let value = value.trim();
    if value.is_empty() || value.len() > maximum || value.contains('\0') {
        return Err(CommandError::invalid(format!(
            "{label} must contain between 1 and {maximum} bytes"
        )));
    }
    Ok(value.to_owned())
}

pub(crate) fn fallback_chat_title(prompt: &str) -> String {
    let title = prompt
        .lines()
        .next()
        .unwrap_or(prompt)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    title.chars().take(40).collect()
}

pub(crate) fn map_internal(error: impl std::fmt::Display) -> CommandError {
    CommandError::internal(error.to_string())
}

/// The typed refusal for a folder in a pre-ladder development format.
pub(crate) const LOUPIST_EARLIER_VERSION: &str = "loupist_earlier_version";
/// The typed refusal for a folder a newer Loupist saved (never touched).
pub(crate) const LOUPIST_NEWER_VERSION: &str = "loupist_newer_version";

/// Preserve not-found and conflict when mapping a store error to a command error.
pub(crate) fn map_store(error: crate::store::StoreError) -> CommandError {
    use crate::store::StoreError;
    match error {
        StoreError::Conflict(message) => CommandError::conflict(message),
        StoreError::EarlierVersion => CommandError::new(
            LOUPIST_EARLIER_VERSION,
            "This Loupist was made by an earlier version and can't be opened. Create a new Loupist.",
        ),
        StoreError::NewerVersion => CommandError::new(
            LOUPIST_NEWER_VERSION,
            "This Loupist was saved by a newer version of Loupist. Update Loupist to open it.",
        ),
        StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows) => {
            CommandError::not_found("record")
        }
        StoreError::Sqlite(error) => {
            eprintln!("Could not access saved data: {error}");
            CommandError::internal("Could not access saved data.")
        }
        other => CommandError::internal(other.to_string()),
    }
}

pub(crate) fn new_id(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

pub(crate) fn loupist_config_directory(
    state: &AppState,
    loupist_id: &str,
) -> Result<std::path::PathBuf, CommandError> {
    let profile = state
        .store
        .get_loupist(loupist_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    Ok(crate::fs::loupist_root::profile_workspace(&profile)?
        .path()
        .join(".loupist"))
}

pub(crate) fn loupist_configuration(
    state: &AppState,
    loupist_id: &str,
) -> Result<crate::scoped_config::ResolvedConfig, CommandError> {
    let directory = loupist_config_directory(state, loupist_id)?;
    crate::scoped_config::resolve(&state.artifacts.app_data_dir, Some(&directory))
}

/// Non-secret authority snapshot for retaining Pi JSONL tool-result context.
///
/// Configuration and credential values are deliberately excluded. Their
/// opaque, independently-rotated revisions bind cache reuse instead.
#[derive(Clone, Debug)]
pub(crate) struct ChatConnectorAuthority {
    pub(crate) capability_ids: Vec<String>,
    pub(crate) sha256: String,
    pub(crate) cacheable: bool,
}

#[derive(Clone, Serialize)]
struct ChatConnectorAuthoritySeal {
    version: &'static str,
    bindings: Vec<ChatConnectorBindingSeal>,
    connectors: Vec<ChatConnectorSeal>,
}

#[derive(Clone, Serialize)]
struct ChatConnectorBindingSeal {
    entry_id: String,
    enabled: bool,
    execute: bool,
    updated_at_ms: i64,
}

#[derive(Clone, Serialize)]
struct ChatConnectorSeal {
    entry_id: String,
    config_revision: crate::marketplace::connector_config::ConnectorConfigRevision,
    active_credential_revision: Option<String>,
}

const CHAT_CONNECTOR_AUTHORITY_SEAL_VERSION: &str = "chat-connector-authority/v1";

fn canonicalize_chat_connector_authority_seal(seal: &mut ChatConnectorAuthoritySeal) {
    seal.bindings
        .sort_by(|left, right| left.entry_id.cmp(&right.entry_id));
    seal.connectors
        .sort_by(|left, right| left.entry_id.cmp(&right.entry_id));
}

fn chat_connector_authority_sha256(
    seal: &ChatConnectorAuthoritySeal,
) -> Result<String, CommandError> {
    let serialized = serde_json::to_vec(seal).map_err(map_internal)?;
    Ok(crate::util::sha256(&serialized))
}

fn binding_grants_execute(binding: &LoupistCapabilityBinding) -> bool {
    binding
        .granted_permissions
        .iter()
        .any(|permission| permission == "execute")
}

fn enabled_execute_capability_ids_from_bindings(
    state: &AppState,
    loupist_id: &str,
    bindings: &[LoupistCapabilityBinding],
) -> Result<Vec<String>, CommandError> {
    let mut ready = BTreeSet::new();
    for binding in bindings {
        if binding.enabled
            && binding_grants_execute(binding)
            && crate::marketplace::execute_binding_ready(binding, state)?
        {
            ready.insert(binding.entry_id.clone());
        }
    }
    for connection in
        crate::custom_connections::service::records_for_state(state).map_err(map_internal)?
    {
        if !connection.pending && connection.enabled_loupists.contains(loupist_id) {
            ready.insert(connection.id);
        }
    }
    Ok(ready.into_iter().collect())
}

#[cfg(test)]
pub(crate) fn enabled_execute_capability_ids(
    state: &AppState,
    loupist_id: &str,
) -> Result<Vec<String>, CommandError> {
    crate::marketplace::with_connector_lifecycle(state, || {
        let bindings = state
            .store
            .list_loupist_capabilities(loupist_id)
            .map_err(map_store)?;
        enabled_execute_capability_ids_from_bindings(state, loupist_id, &bindings)
    })
}

/// Captures every connector authority which may influence a Pi session. The
/// whole catalog is included: a disabled connector's global config can be a
/// runtime dependency of another enabled connector.
pub(crate) fn capture_chat_connector_authority(
    state: &AppState,
    loupist_id: &str,
) -> Result<ChatConnectorAuthority, CommandError> {
    crate::marketplace::with_connector_lifecycle(state, || {
        let bindings = state
            .store
            .list_loupist_capabilities(loupist_id)
            .map_err(map_store)?;
        let capability_ids =
            enabled_execute_capability_ids_from_bindings(state, loupist_id, &bindings)?;
        let binding_seals = bindings
            .into_iter()
            .map(|binding| {
                let execute = binding_grants_execute(&binding);
                ChatConnectorBindingSeal {
                    entry_id: binding.entry_id,
                    enabled: binding.enabled,
                    execute,
                    updated_at_ms: binding.updated_at_ms,
                }
            })
            .collect::<Vec<_>>();
        let catalog = crate::marketplace::bundled_catalog()?;
        let mut cacheable = !capability_ids
            .iter()
            .any(|id| crate::custom_connections::service::valid_id(id));
        let connector_seals = catalog
            .entries
            .into_iter()
            .map(|entry| {
                let config_revision =
                    match crate::marketplace::connector_config::connector_config_revision(
                        state, &entry.id,
                    ) {
                        Ok(revision) => revision,
                        // A malformed or unreadable dormant connector must never
                        // keep an old Pi cache alive, but it need not prevent a
                        // Chat that does not execute that connector.
                        Err(_) => {
                            cacheable = false;
                            crate::marketplace::connector_config::ConnectorConfigRevision::Unreadable
                        }
                    };
                if !config_revision.is_cacheable() {
                    cacheable = false;
                }
                let active_credential_revision =
                    match crate::finance_credentials::active_revision(&state.credential_environment, &entry.id) {
                        Ok(revision) => revision,
                        Err(_) => {
                            cacheable = false;
                            None
                        }
                    };
                ChatConnectorSeal {
                    entry_id: entry.id,
                    config_revision,
                    active_credential_revision,
                }
            })
            .collect::<Vec<_>>();
        let mut seal = ChatConnectorAuthoritySeal {
            version: CHAT_CONNECTOR_AUTHORITY_SEAL_VERSION,
            bindings: binding_seals,
            connectors: connector_seals,
        };
        canonicalize_chat_connector_authority_seal(&mut seal);
        Ok(ChatConnectorAuthority {
            capability_ids,
            sha256: chat_connector_authority_sha256(&seal)?,
            cacheable,
        })
    })
}

pub(crate) async fn profile_summary(
    state: &AppState,
    profile: &LoupistProfile,
) -> Result<LoupistSummary, CommandError> {
    match state.loupist_access(&profile.id) {
        LoupistAccess::Visible(availability) => {
            profile_summary_with_availability(state, profile, availability).await
        }
        LoupistAccess::Hidden => Err(CommandError::new(
            "loupist_storage_unavailable",
            "Loupist storage is unavailable",
        )),
    }
}

pub(crate) async fn profile_summary_at(
    state: &AppState,
    profile: &LoupistProfile,
    _workspace: &BoundLoupistRoot,
) -> Result<LoupistSummary, CommandError> {
    state.ensure_loupist_available(&profile.id)?;
    profile_summary_from_parts(
        state,
        profile,
        host_record_count(state, &profile.id),
        LoupistAvailability::Available,
    )
}

async fn profile_summary_with_availability(
    state: &AppState,
    profile: &LoupistProfile,
    availability: LoupistAvailability,
) -> Result<LoupistSummary, CommandError> {
    let availability = match availability {
        LoupistAvailability::Available => {
            state.ensure_loupist_available(&profile.id)?;
            if crate::fs::loupist_root::profile_workspace(profile).is_err() {
                LoupistAvailability::WorkspaceUnavailable
            } else if !state.store.is_folder_catalog() {
                LoupistAvailability::Available
            } else {
                match state.store.lock_loupist(&profile.id) {
                    Ok(_) => LoupistAvailability::Available,
                    Err(crate::store::StoreError::EarlierVersion) => {
                        LoupistAvailability::EarlierVersion
                    }
                    Err(crate::store::StoreError::NewerVersion) => {
                        LoupistAvailability::NewerVersion
                    }
                    Err(_) => LoupistAvailability::WorkspaceUnavailable,
                }
            }
        }
        other => other,
    };
    profile_summary_from_parts(
        state,
        profile,
        host_record_count(state, &profile.id),
        availability,
    )
}

fn host_record_count(state: &AppState, loupist_id: &str) -> usize {
    crate::commands::knowledge::host_module_count(state, loupist_id)
}

fn profile_summary_from_parts(
    state: &AppState,
    profile: &LoupistProfile,
    record_count: usize,
    availability: LoupistAvailability,
) -> Result<LoupistSummary, CommandError> {
    let palette = ["#a4530c", "#4d5f8e", "#76508d", "#2f6f83", "#5f6b7a"];
    let accent_index = profile
        .id
        .bytes()
        .fold(0_usize, |total, byte| total.wrapping_add(byte as usize))
        % palette.len();
    Ok(LoupistSummary {
        id: profile.id.clone(),
        name: profile.name.clone(),
        workspace_path: profile.workspace_root.clone(),
        philosophy: profile.focus.clone(),
        instructions: profile.instructions.clone(),
        avatar: profile.avatar.clone(),
        tone: profile.tone,
        template: profile.template,
        record_count,
        updated_at: iso_time(profile.updated_at_ms)?,
        accent: palette[accent_index].into(),
        last_model_profile_id: profile.last_model_profile_id.clone(),
        enabled_skill_ids: if matches!(
            availability,
            LoupistAvailability::WorkspaceUnavailable
                | LoupistAvailability::EarlierVersion
                | LoupistAvailability::NewerVersion
        ) {
            Vec::new()
        } else {
            crate::scoped_config::read_scope(
                &state.artifacts.app_data_dir,
                Some(&loupist_config_directory(state, &profile.id)?),
                crate::scoped_config::ConfigScope::Project {
                    loupist_id: profile.id.clone(),
                },
            )?
            .effective
            .enabled_skill_ids()
        },
        availability,
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod loupist_folder_tests;
