use std::collections::BTreeMap;
use std::sync::MutexGuard;

use serde::Serialize;
use tauri::State;

use crate::{
    app::{AppState, CommandError},
    runs::maintenance::MaintenanceActivityKind,
    store::AppliedConnectorConfig,
};

use super::{
    catalog::{
        bundled_catalog, MarketplaceConfigState, MarketplaceConnectorConfigureRequest,
        MarketplaceConnectorReadiness, MarketplaceConnectorStatusDto, MarketplaceEntryDto,
        MarketplaceSetupFieldDto, MarketplaceSetupFieldKind,
    },
    connector_lifecycle_lock, connector_ready, connector_status,
    credentials::disable_bindings_and_delete_credentials_locked,
    enable_ready_bindings_locked, ensure_installable,
};

const WEB_RESEARCH_ID: &str = "community.web-research";
const WEB_RESEARCH_POLICY_FIELD: &str = "search_policy";

/// Semantic, secret-free configuration identity for execution/cache seals.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
#[serde(tag = "state", content = "revision", rename_all = "snake_case")]
pub(crate) enum ConnectorConfigRevision {
    Absent,
    Unreadable,
    Revision(String),
}

impl ConnectorConfigRevision {
    pub(crate) const fn is_cacheable(&self) -> bool {
        !matches!(self, Self::Unreadable)
    }
}

pub(super) fn config_state(
    entry: &MarketplaceEntryDto,
    config: &BTreeMap<String, String>,
) -> MarketplaceConfigState {
    let fields = entry
        .setup
        .as_ref()
        .map(|setup| setup.config_fields.as_slice())
        .unwrap_or_default();
    if fields.is_empty() {
        return MarketplaceConfigState::NotRequired;
    }
    let has_unknown_field = config
        .keys()
        .any(|key| !fields.iter().any(|field| field.id == *key));
    let has_missing_or_invalid_field = fields.iter().any(|field| match config.get(&field.id) {
        Some(value) => !valid_setup_value(field, value),
        None => field.required,
    });
    if has_unknown_field || has_missing_or_invalid_field {
        MarketplaceConfigState::Missing
    } else {
        MarketplaceConfigState::Valid
    }
}

/// TOML can contain only the non-secret setup fields declared by bundled
/// connectors. Partial setup remains editable, but a credential field or an
/// invalid value of a declared field does not. A connector or field this
/// catalog does not declare is not an error here: `ignored_config_entries`
/// reports it, and it takes no effect.
pub(crate) fn validate_config_entries(
    configs: &BTreeMap<String, BTreeMap<String, String>>,
) -> Result<(), CommandError> {
    let catalog = bundled_catalog()?;
    for (id, config) in configs {
        let Some(setup) = catalog
            .entries
            .iter()
            .find(|entry| &entry.id == id)
            .and_then(|entry| entry.setup.as_ref())
        else {
            continue;
        };
        for (key, value) in config {
            if let Some(field) = setup.config_fields.iter().find(|field| field.id == *key) {
                if !valid_setup_value(field, value) {
                    return Err(CommandError::invalid(format!(
                        "invalid configuration field: connectors.{id}.{key}"
                    )));
                }
            } else if setup.credential_fields.iter().any(|field| field.id == *key) {
                return Err(CommandError::invalid(format!(
                    "unsupported non-secret configuration field: connectors.{id}.{key}"
                )));
            }
        }
    }
    Ok(())
}

/// The connector tables and fields in `config.toml` that the bundled catalog
/// does not declare, because a later or earlier catalog removed or renamed
/// them: `(id, None)` for a whole connector, `(id, Some(field))` for one field.
/// They stay in the file, are reported as warnings and take no effect.
pub(crate) fn ignored_config_entries(
    configs: &BTreeMap<String, BTreeMap<String, String>>,
) -> Result<Vec<(String, Option<String>)>, CommandError> {
    let catalog = bundled_catalog()?;
    let mut ignored = Vec::new();
    for (id, config) in configs {
        let Some(entry) = catalog.entries.iter().find(|entry| &entry.id == id) else {
            ignored.push((id.clone(), None));
            continue;
        };
        let setup = entry.setup.as_ref();
        for key in config.keys() {
            let declared = setup.is_some_and(|setup| {
                setup
                    .config_fields
                    .iter()
                    .chain(&setup.credential_fields)
                    .any(|field| field.id == *key)
            });
            if !declared {
                ignored.push((id.clone(), Some(key.clone())));
            }
        }
    }
    Ok(ignored)
}

fn digests(
    entry: &MarketplaceEntryDto,
    config: &BTreeMap<String, String>,
) -> Result<AppliedConnectorConfig, CommandError> {
    let scope = entry
        .setup
        .as_ref()
        .map(|setup| setup.credential_scope_fields.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|key| (key, config.get(key)))
        .collect::<BTreeMap<_, _>>();
    Ok(AppliedConnectorConfig {
        config_digest: crate::util::sha256(
            &serde_json::to_vec(config)
                .map_err(|_| CommandError::internal("configuration digest failed"))?,
        ),
        scope_digest: crate::util::sha256(
            &serde_json::to_vec(&scope)
                .map_err(|_| CommandError::internal("configuration scope digest failed"))?,
        ),
    })
}

fn raw_config(state: &AppState, entry_id: &str) -> Result<BTreeMap<String, String>, CommandError> {
    ensure_installable(entry_id)?;
    crate::scoped_config::read_global_connector_settings(&state.artifacts.app_data_dir, entry_id)
}

pub(super) fn read_connector_config(
    state: &AppState,
    entry_id: &str,
) -> Result<BTreeMap<String, String>, CommandError> {
    let config = raw_config(state, entry_id)?;
    let catalog = bundled_catalog()?;
    let entry = catalog
        .entries
        .iter()
        .find(|entry| entry.id == entry_id)
        .ok_or_else(|| CommandError::not_found("Marketplace entry"))?;
    let current = digests(entry, &config)?;
    let applied = state
        .store
        .applied_connector_config(entry_id)
        .map_err(crate::commands::map_store)?;
    if applied.as_ref() != Some(&current) {
        return Err(CommandError::new(
            "config_apply_pending",
            "Connector configuration changed and must be reconciled before use",
        ));
    }
    Ok(config)
}

pub(crate) fn connector_config_revision(
    state: &AppState,
    entry_id: &str,
) -> Result<ConnectorConfigRevision, CommandError> {
    let config = raw_config(state, entry_id)?;
    if config.is_empty() {
        return Ok(ConnectorConfigRevision::Absent);
    }
    Ok(ConnectorConfigRevision::Revision(crate::util::sha256(
        &serde_json::to_vec(&config)
            .map_err(|_| CommandError::internal("configuration digest failed"))?,
    )))
}

pub(crate) fn reconcile_connector_configs(state: &AppState) -> Result<(), CommandError> {
    let guard = connector_lifecycle_lock()?;
    reconcile_connector_configs_locked(state, &guard)
}

pub(super) fn reconcile_connector_configs_locked(
    state: &AppState,
    guard: &MutexGuard<'static, ()>,
) -> Result<(), CommandError> {
    let resolved = crate::scoped_config::resolve(&state.artifacts.app_data_dir, None)?;
    let catalog = bundled_catalog()?;
    for entry in &catalog.entries {
        let config = resolved
            .connectors
            .get(&entry.id)
            .cloned()
            .unwrap_or_default();
        let next = digests(entry, &config)?;
        let previous = state
            .store
            .applied_connector_config(&entry.id)
            .map_err(crate::commands::map_store)?;
        if previous.as_ref() == Some(&next) {
            continue;
        }
        let previous_scope = match &previous {
            Some(applied) => applied.scope_digest.clone(),
            None => digests(entry, &BTreeMap::new())?.scope_digest,
        };
        if previous_scope != next.scope_digest {
            // The on-disk digest mismatch independently closes admission even
            // if an unavailable Loupist prevents binding cleanup from finishing.
            if entry.id == crate::finance_data::KIS_SOURCE_ID {
                state.finance_data.invalidate_kis_token_cache();
            }
            disable_bindings_and_delete_credentials_locked(state, &entry.id, guard)?;
        }
        // External editors do not hold our mutex. Never attest stale bytes.
        if digests(entry, &raw_config(state, &entry.id)?)? != next {
            return Err(CommandError::conflict(
                "connector configuration changed while it was being applied",
            ));
        }
        state
            .store
            .set_applied_connector_config(&entry.id, &next)
            .map_err(crate::commands::map_store)?;
        // Match the existing connection setup behavior for keyless connectors
        // when configuration first completes setup. Rechecks keep opt-outs.
        if config_state(entry, &config) == MarketplaceConfigState::Valid && previous.is_some() {
            // Credentialed connectors become ready through verification; a
            // plain text edit must never authorize a new credential bundle.
            let no_credentials = entry
                .setup
                .as_ref()
                .is_none_or(|setup| setup.credential_fields.is_empty());
            let empty_config = BTreeMap::new();
            let newly_configured = config_state(entry, &empty_config)
                == MarketplaceConfigState::Missing
                && previous.as_ref().is_some_and(|old| {
                    old.config_digest
                        == digests(entry, &empty_config)
                            .map(|empty| empty.config_digest)
                            .unwrap_or_default()
                });
            if no_credentials && newly_configured {
                enable_ready_bindings_locked(state, &entry.id, guard)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn connector_config_value(
    state: &AppState,
    entry_id: &str,
    field_id: &str,
) -> Result<Option<String>, CommandError> {
    Ok(read_connector_config(state, entry_id)?.remove(field_id))
}

pub(crate) fn web_research_policy(
    state: &AppState,
) -> Result<crate::web::WebSearchPolicy, CommandError> {
    let config = read_connector_config(state, WEB_RESEARCH_ID)?;
    crate::web::WebSearchPolicy::from_config_value(
        config.get(WEB_RESEARCH_POLICY_FIELD).map(String::as_str),
    )
    .ok_or_else(|| CommandError::internal("Web Research routing configuration is invalid"))
}

pub(super) fn valid_setup_value(field: &MarketplaceSetupFieldDto, value: &str) -> bool {
    if !(field.min_length..=field.max_length).contains(&value.len())
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        return false;
    }
    match field.kind {
        MarketplaceSetupFieldKind::ApiKey => !value.chars().any(char::is_whitespace),
        MarketplaceSetupFieldKind::Email => {
            let mut parts = value.split('@');
            parts.next().is_some_and(|part| !part.is_empty())
                && parts.next().is_some_and(|part| part.contains('.'))
                && parts.next().is_none()
                && !value.chars().any(char::is_whitespace)
        }
        MarketplaceSetupFieldKind::Select => field.options.iter().any(|option| option == value),
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn marketplace_connector_configure(
    request: MarketplaceConnectorConfigureRequest,
    state: State<'_, AppState>,
) -> Result<MarketplaceConnectorStatusDto, CommandError> {
    configure_connector(request, &state)
}

pub(crate) fn configure_connector(
    request: MarketplaceConnectorConfigureRequest,
    state: &AppState,
) -> Result<MarketplaceConnectorStatusDto, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::MarketplaceConfiguration)?;
    ensure_installable(&request.entry_id)?;
    let guard = connector_lifecycle_lock()?;
    reconcile_connector_configs_locked(state, &guard)?;
    let catalog = bundled_catalog()?;
    let entry = catalog
        .entries
        .iter()
        .find(|entry| entry.id == request.entry_id)
        .ok_or_else(|| CommandError::not_found("Marketplace entry"))?;
    validate_config_entries(&BTreeMap::from([(
        request.entry_id.clone(),
        request.config.clone(),
    )]))?;
    if config_state(entry, &request.config) == MarketplaceConfigState::Missing {
        return Err(CommandError::invalid(
            "connector configuration does not match its setup contract",
        ));
    }
    let was_ready = connector_ready(entry, state)?;
    crate::scoped_config::write_global_connector_settings(
        &state.artifacts.app_data_dir,
        &request.entry_id,
        &request.config,
    )?;
    reconcile_connector_configs_locked(state, &guard).map_err(|error| {
        CommandError::new(
            "config_apply_pending",
            format!(
                "Configuration was saved, but connection reconciliation is blocked: {}",
                error.message
            ),
        )
    })?;
    let status = connector_status(entry, state)?;
    if !was_ready && status.readiness == MarketplaceConnectorReadiness::Ready {
        enable_ready_bindings_locked(state, &request.entry_id, &guard)?;
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::artifact_trust::ensure_private_regular_file;
    use std::fs;

    const TEST_ENTRY_ID: &str = "sec.edgar";

    fn configure(state: &AppState, entry_id: &str, field: &str, value: &str) {
        configure_connector(
            MarketplaceConnectorConfigureRequest {
                entry_id: entry_id.to_owned(),
                config: BTreeMap::from([(field.to_owned(), value.to_owned())]),
            },
            state,
        )
        .unwrap();
    }

    fn kis_config(environment: &str) -> BTreeMap<String, String> {
        BTreeMap::from([("environment".into(), environment.into())])
    }

    fn verify_fixture_credential(state: &AppState) {
        let environment = &state.credential_environment;
        let id = crate::finance_data::KIS_SOURCE_ID;
        crate::finance_credentials::stage(
            environment,
            id,
            &BTreeMap::from([
                ("app_key".into(), "fixture-key".into()),
                ("app_secret".into(), "fixture-secret".into()),
            ]),
        )
        .unwrap();
        let candidate = crate::finance_credentials::candidate(environment, id)
            .unwrap()
            .unwrap();
        crate::finance_credentials::finish_verification(
            environment,
            id,
            candidate.revision(),
            crate::finance_credentials::VerificationOutcome::Verified,
            1,
            None,
        )
        .unwrap();
    }

    fn has_active_credential(environment: &str, id: &str) -> bool {
        crate::finance_credentials::status(environment, id)
            .unwrap()
            .active
    }

    #[test]
    fn unreadable_config_is_rejected_without_rewriting_the_file() {
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let path = state.artifacts.app_data_dir.join("config.toml");
        let bytes = b"[connectors.unknown\n";
        fs::write(&path, bytes).unwrap();
        ensure_private_regular_file(&path).unwrap();

        assert!(read_connector_config(&state, TEST_ENTRY_ID).is_err());
        assert!(connector_config_revision(&state, TEST_ENTRY_ID).is_err());
        assert!(configure_connector(
            MarketplaceConnectorConfigureRequest {
                entry_id: TEST_ENTRY_ID.to_owned(),
                config: BTreeMap::from([(
                    "contact_email".to_owned(),
                    "research@example.invalid".to_owned(),
                )]),
            },
            &state,
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn config_revision_changes_only_with_semantic_content() {
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let id = crate::finance_data::KIS_SOURCE_ID;
        configure(&state, id, "environment", "real");
        verify_fixture_credential(&state);
        let before = connector_config_revision(&state, id).unwrap();
        configure(&state, id, "environment", "real");
        assert_eq!(connector_config_revision(&state, id).unwrap(), before);

        // An external editor adding only a comment keeps the revision and key.
        let path = state.artifacts.app_data_dir.join("config.toml");
        let original = fs::read_to_string(&path).unwrap();
        fs::write(
            &path,
            format!("# An external editor only added a comment.\n{original}\n"),
        )
        .unwrap();
        reconcile_connector_configs(&state).unwrap();
        assert_eq!(connector_config_revision(&state, id).unwrap(), before);
        assert!(has_active_credential(&state.credential_environment, id));

        configure(
            &state,
            TEST_ENTRY_ID,
            "contact_email",
            "first@example.invalid",
        );
        let first = connector_config_revision(&state, TEST_ENTRY_ID).unwrap();
        configure(
            &state,
            TEST_ENTRY_ID,
            "contact_email",
            "updated@example.invalid",
        );
        assert_ne!(
            connector_config_revision(&state, TEST_ENTRY_ID).unwrap(),
            first
        );
    }

    #[test]
    fn changed_scope_is_denied_before_reconciliation_and_recovered_on_restart() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("app");
        let state = AppState::for_persistent_test(root.clone());
        let id = crate::finance_data::KIS_SOURCE_ID;
        configure(&state, id, "environment", "real");
        verify_fixture_credential(&state);
        let environment = state.credential_environment.clone();
        crate::scoped_config::write_global_connector_settings(&root, id, &kis_config("demo"))
            .unwrap();
        assert_eq!(
            read_connector_config(&state, id).unwrap_err().code,
            "config_apply_pending"
        );
        state.close_for_restart_test();
        let restarted = AppState::for_persistent_test(root);
        assert_eq!(restarted.credential_environment, environment);
        assert_eq!(
            read_connector_config(&restarted, id).unwrap(),
            kis_config("demo")
        );
        assert!(!has_active_credential(&environment, id));
    }

    #[test]
    fn non_secret_setup_contract_rejects_credential_fields_and_preserves_source() {
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let path = state.artifacts.app_data_dir.join("config.toml");
        let source = "[connectors.\"koreainvestment.market-data\"]\napp_key = \"fixture\"\n";
        fs::write(&path, source).unwrap();
        assert!(reconcile_connector_configs(&state).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), source);
    }
}
