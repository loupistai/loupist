mod catalog;
pub(crate) mod connector_config;
pub(crate) mod credentials;
pub(crate) mod entry_lookup;

pub(crate) use catalog::progress_names;
pub use catalog::*;
use connector_config::{config_state, read_connector_config};
pub(crate) use connector_config::{connector_config_value, web_research_policy};
use credentials::credential_statuses;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard, OnceLock};

use chrono::Utc;
use tauri::State;

use crate::{
    app::{AppState, CommandError},
    domain::LoupistCapabilityBinding,
    runs::maintenance::MaintenanceActivityKind,
    store::LoupistStore,
};

fn connector_lifecycle_lock() -> Result<MutexGuard<'static, ()>, CommandError> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Default::default)
        .lock()
        .map_err(|_| CommandError::internal("connector lifecycle lock was poisoned"))
}

pub(crate) fn with_connector_lifecycle<T>(
    state: &AppState,
    operation: impl FnOnce() -> Result<T, CommandError>,
) -> Result<T, CommandError> {
    let guard = connector_lifecycle_lock()?;
    connector_config::reconcile_connector_configs_locked(state, &guard)?;
    operation()
}

#[tauri::command(rename_all = "snake_case")]
pub fn marketplace_snapshot(
    state: State<'_, AppState>,
) -> Result<MarketplaceSnapshotDto, CommandError> {
    with_connector_lifecycle(&state, || marketplace_snapshot_for_state(&state))
}

#[tauri::command(rename_all = "snake_case")]
pub fn loupist_capability_list(
    loupist_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<LoupistCapabilityBindingDto>, CommandError> {
    state.ensure_loupist_available(&loupist_id)?;
    with_connector_lifecycle(&state, || {
        loupist_capability_list_for_state(&loupist_id, &state)
    })
}

fn installed_entries(
    catalog: &MarketplaceCatalogDto,
    connectors: &[MarketplaceConnectorStatusDto],
) -> Vec<MarketplaceInstalledDto> {
    catalog
        .entries
        .iter()
        .map(|entry| {
            let entry_id = entry.id.as_str();
            let readiness = connectors
                .iter()
                .find(|connector| connector.entry_id == entry_id)
                .map(|connector| connector.readiness);
            let configured = readiness == Some(MarketplaceConnectorReadiness::Ready);
            MarketplaceInstalledDto {
                entry_id: entry.id.clone(),
                configured,
                health: match readiness {
                    Some(MarketplaceConnectorReadiness::Ready) => MarketplaceHealth::Ready,
                    Some(MarketplaceConnectorReadiness::RuntimeUnavailable) => {
                        MarketplaceHealth::Error
                    }
                    _ => MarketplaceHealth::NeedsConfiguration,
                },
            }
        })
        .collect()
}

fn marketplace_snapshot_for_state(
    state: &AppState,
) -> Result<MarketplaceSnapshotDto, CommandError> {
    let bundled = bundled_marketplace()?;
    let catalog = bundled.catalog.clone();
    let connectors = catalog
        .entries
        .iter()
        .map(|entry| connector_status(entry, state))
        .collect::<Result<Vec<_>, _>>()?;
    let installed = installed_entries(&catalog, &connectors);
    let installed_ids = installed
        .iter()
        .map(|entry| entry.entry_id.as_str())
        .collect::<BTreeSet<_>>();
    let connectors = connectors
        .into_iter()
        .filter(|connector| installed_ids.contains(connector.entry_id.as_str()))
        .collect();
    let snapshot = MarketplaceSnapshotDto {
        schema_version: SNAPSHOT_SCHEMA_VERSION.to_owned(),
        plugins: bundled.plugins.clone(),
        installed,
        connectors,
        catalog,
    };
    validate_snapshot(&snapshot).map_err(|message| {
        CommandError::internal(format!("Marketplace snapshot is invalid: {message}"))
    })?;
    Ok(snapshot)
}

fn loupist_capability_list_for_state(
    loupist_id: &str,
    state: &AppState,
) -> Result<Vec<LoupistCapabilityBindingDto>, CommandError> {
    let catalog = bundled_catalog()?;
    let installed = installed_entries(
        &catalog,
        &catalog
            .entries
            .iter()
            .map(|entry| connector_status(entry, state))
            .collect::<Result<Vec<_>, _>>()?,
    );
    let stored = state
        .store
        .list_loupist_capabilities(loupist_id)
        .map_err(|error| CommandError::internal(error.to_string()))?;
    bindings_for_installed_entries(loupist_id, installed, stored)
        .into_iter()
        .map(|binding| binding_dto(binding, &catalog, state))
        .collect()
}

fn bindings_for_installed_entries(
    loupist_id: &str,
    installed: Vec<MarketplaceInstalledDto>,
    stored: Vec<LoupistCapabilityBinding>,
) -> Vec<LoupistCapabilityBinding> {
    let mut stored = stored
        .into_iter()
        .map(|binding| (binding.entry_id.clone(), binding))
        .collect::<BTreeMap<_, _>>();
    installed
        .into_iter()
        .map(|entry| {
            stored
                .remove(&entry.entry_id)
                .unwrap_or(LoupistCapabilityBinding {
                    loupist_id: loupist_id.to_owned(),
                    entry_id: entry.entry_id,
                    enabled: false,
                    granted_permissions: Vec::new(),
                    config: BTreeMap::new(),
                    updated_at_ms: 0,
                })
        })
        .collect()
}

fn binding_dto(
    binding: LoupistCapabilityBinding,
    catalog: &MarketplaceCatalogDto,
    state: &AppState,
) -> Result<LoupistCapabilityBindingDto, CommandError> {
    let entry = catalog
        .entries
        .iter()
        .find(|entry| entry.id == binding.entry_id)
        .ok_or_else(|| CommandError::internal("capability is absent from the catalog"))?;
    let available = connector_ready(entry, state)?;
    Ok(LoupistCapabilityBindingDto {
        entry_id: binding.entry_id,
        enabled: binding.enabled,
        granted_permissions: binding.granted_permissions,
        available,
    })
}

fn connector_status(
    entry: &MarketplaceEntryDto,
    state: &AppState,
) -> Result<MarketplaceConnectorStatusDto, CommandError> {
    let config = read_connector_config(state, &entry.id)?;
    let config_state = config_state(entry, &config);
    let credentials = credential_statuses(state, entry)?;
    let credentials_ready = entry
        .setup
        .as_ref()
        .map(|setup| {
            setup.credential_fields.iter().all(|field| {
                !field.required
                    || credentials
                        .iter()
                        .any(|credential| credential.credential_id == field.id && credential.active)
            })
        })
        .unwrap_or(true);
    let configuration_ready = !matches!(config_state, MarketplaceConfigState::Missing);
    let runtime_ready = runtime_ready(entry, state);
    let readiness = if !runtime_ready {
        MarketplaceConnectorReadiness::RuntimeUnavailable
    } else if configuration_ready && credentials_ready {
        MarketplaceConnectorReadiness::Ready
    } else {
        MarketplaceConnectorReadiness::NeedsConfiguration
    };
    Ok(MarketplaceConnectorStatusDto {
        entry_id: entry.id.clone(),
        config,
        config_state,
        credentials,
        readiness,
    })
}

fn runtime_ready(entry: &MarketplaceEntryDto, state: &AppState) -> bool {
    match entry.runtime.kind {
        MarketplaceRuntimeKind::BundledMcp => entry
            .runtime
            .server_id
            .as_deref()
            .is_some_and(|id| state.artifacts.mcp_runtimes.contains_key(id)),
        MarketplaceRuntimeKind::Native => true,
    }
}

pub(super) fn connector_ready(
    entry: &MarketplaceEntryDto,
    state: &AppState,
) -> Result<bool, CommandError> {
    Ok(connector_readiness(entry, state)? == MarketplaceConnectorReadiness::Ready)
}

pub(crate) fn connector_readiness(
    entry: &MarketplaceEntryDto,
    state: &AppState,
) -> Result<MarketplaceConnectorReadiness, CommandError> {
    Ok(connector_status(entry, state)?.readiness)
}

/// When Marketplace setup first makes a connector ready, enable it for every
/// Loupist so Chat can use it without a separate Agents toggle.
pub(super) fn enable_ready_bindings_locked(
    state: &AppState,
    entry_id: &str,
    _lifecycle_guard: &MutexGuard<'static, ()>,
) -> Result<(), CommandError> {
    let catalog = bundled_catalog()?;
    let Some(entry) = catalog.entries.iter().find(|entry| entry.id == entry_id) else {
        return Ok(());
    };
    if !connector_ready(entry, state)? {
        return Ok(());
    }
    for loupist in state
        .store
        .list_loupists()
        .map_err(|error| CommandError::internal(error.to_string()))?
    {
        if state.ensure_loupist_available(&loupist.id).is_err() {
            continue;
        }
        let existing = state
            .store
            .get_loupist_capability(&loupist.id, entry_id)
            .map_err(|error| CommandError::internal(error.to_string()))?;
        if existing.as_ref().is_some_and(|binding| binding.enabled) {
            continue;
        }
        let updated_at_ms = next_binding_timestamp(existing.as_ref());
        let binding = LoupistCapabilityBinding {
            loupist_id: loupist.id,
            entry_id: entry_id.to_owned(),
            enabled: true,
            granted_permissions: vec!["execute".to_owned()],
            config: existing.map(|binding| binding.config).unwrap_or_default(),
            updated_at_ms,
        };
        state
            .store
            .save_loupist_capability(&binding)
            .map_err(|error| CommandError::internal(error.to_string()))?;
    }
    Ok(())
}

/// Turns a host-only connection on for every Loupist when the user first uses
/// it (the first wallet address for `onchain.wallets`); opt-outs made later stay.
pub(crate) fn enable_on_first_use(state: &AppState, entry_id: &str) -> Result<(), CommandError> {
    let guard = connector_lifecycle_lock()?;
    enable_ready_bindings_locked(state, entry_id, &guard)
}

/// Rechecks mutable connector setup at the moment a run captures authority.
/// Stored-but-unverified candidates never make a capability executable.
pub(crate) fn execute_binding_ready(
    binding: &LoupistCapabilityBinding,
    state: &AppState,
) -> Result<bool, CommandError> {
    let catalog = bundled_catalog()?;
    let Some(entry) = catalog
        .entries
        .iter()
        .find(|entry| entry.id == binding.entry_id)
    else {
        return Ok(false);
    };
    connector_ready(entry, state)
}

fn ensure_installable(entry_id: &str) -> Result<(), CommandError> {
    let catalog = bundled_catalog()?;
    if !catalog.entries.iter().any(|entry| entry.id == entry_id) {
        return Err(CommandError::not_found("Marketplace entry"));
    }
    Ok(())
}

fn next_binding_timestamp(existing: Option<&LoupistCapabilityBinding>) -> i64 {
    Utc::now().timestamp_millis().max(
        existing
            .map(|binding| binding.updated_at_ms.saturating_add(1))
            .unwrap_or_default(),
    )
}

fn save_binding(
    request: LoupistCapabilityRequest,
    state: &AppState,
    enabled: bool,
) -> Result<LoupistCapabilityBindingDto, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::MarketplaceConfiguration)?;
    state.ensure_loupist_available(&request.loupist_id)?;
    ensure_installable(&request.entry_id)?;
    let _lifecycle_guard = connector_lifecycle_lock()?;
    connector_config::reconcile_connector_configs_locked(state, &_lifecycle_guard)?;
    state.ensure_loupist_available(&request.loupist_id)?;
    let catalog = bundled_catalog()?;
    let existing = state
        .store
        .get_loupist_capability(&request.loupist_id, &request.entry_id)
        .map_err(|error| CommandError::internal(error.to_string()))?;
    let updated_at_ms = next_binding_timestamp(existing.as_ref());
    let binding = LoupistCapabilityBinding {
        loupist_id: request.loupist_id,
        entry_id: request.entry_id,
        enabled,
        granted_permissions: if enabled {
            vec!["execute".to_owned()]
        } else {
            Vec::new()
        },
        config: existing.map(|binding| binding.config).unwrap_or_default(),
        updated_at_ms,
    };
    if enabled && !binding_dto(binding.clone(), &catalog, state)?.available {
        return Err(CommandError::conflict(
            "Marketplace connector must be configured before it is enabled",
        ));
    }
    state
        .store
        .save_loupist_capability(&binding)
        .map_err(|error| CommandError::internal(error.to_string()))?;
    binding_dto(binding, &catalog, state)
}

#[tauri::command(rename_all = "snake_case")]
pub fn loupist_capability_enable(
    request: LoupistCapabilityRequest,
    state: State<'_, AppState>,
) -> Result<LoupistCapabilityBindingDto, CommandError> {
    save_binding(request, &state, true)
}

#[tauri::command(rename_all = "snake_case")]
pub fn loupist_capability_disable(
    request: LoupistCapabilityRequest,
    state: State<'_, AppState>,
) -> Result<LoupistCapabilityBindingDto, CommandError> {
    save_binding(request, &state, false)
}

#[cfg(test)]
mod tests;
