use std::collections::{BTreeMap, BTreeSet};
use std::sync::MutexGuard;
use std::time::Duration;

use chrono::Utc;
use serde_json::{json, Value};
use tauri::State;

use crate::{
    app::{off_main_thread, AppState, CommandError},
    mcp::{
        contains_protected_value, filter_tool_providers, validate_result_provider, McpCallResult,
        McpError, McpLaunchConfig, McpSession,
    },
    runs::maintenance::MaintenanceActivityKind,
    runs::scratch::RunScratch,
};

use super::{
    catalog::{
        bundled_catalog, MarketplaceCatalogDto, MarketplaceCredentialRequest,
        MarketplaceCredentialSaveRequest, MarketplaceCredentialStatusDto,
        MarketplaceCredentialVerification, MarketplaceEntryDto, MarketplaceRuntimeKind,
        MarketplaceSetupFieldKind,
    },
    connector_config::{
        connector_config_value, reconcile_connector_configs_locked, valid_setup_value,
    },
    connector_lifecycle_lock, connector_ready, enable_ready_bindings_locked, ensure_installable,
};

const MCP_CREDENTIAL_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const MCP_CREDENTIAL_SHUTDOWN_GRACE: Duration = Duration::from_secs(1);

/// Why a bundled connection's key was not verified. Only an answer with data
/// from the provider verifies a key; everything else is reported as rejected
/// (the provider refused it) or unavailable (Loupist could not tell).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum McpCredentialProbeError {
    /// The provider refused the key.
    Rejected,
    /// The provider refused the key because it is not in the format it issues.
    RejectedFormat,
    /// The provider could not be reached, or the probe could not run.
    Unavailable,
    /// The provider answered without any data, which proves nothing about the
    /// key: some OpenBB providers turn a refusal into an empty result.
    NoData,
}

impl McpCredentialProbeError {
    fn outcome(self) -> crate::finance_credentials::VerificationOutcome {
        use crate::finance_credentials::VerificationOutcome;
        match self {
            Self::Rejected | Self::RejectedFormat => VerificationOutcome::Rejected,
            Self::Unavailable | Self::NoData => VerificationOutcome::TemporarilyUnavailable,
        }
    }

    /// Fixed, key-free text. The renderer words these by code in both languages.
    fn command_error(self) -> CommandError {
        match self {
            Self::Rejected => CommandError::new(
                "credential_rejected",
                "The provider rejected this key. Check that you copied the whole key.",
            ),
            Self::RejectedFormat => CommandError::new(
                "credential_format_rejected",
                "The provider rejected this key because it is not in the format the provider issues.",
            ),
            Self::Unavailable => CommandError::new(
                "provider_unavailable",
                "Loupist could not check this key with the provider. It is saved but not verified.",
            ),
            Self::NoData => CommandError::new(
                "provider_unavailable",
                "The provider answered without data, so Loupist could not confirm this key. It is saved but not verified.",
            ),
        }
    }
}

/// Reads a probe's answer. An error the helper classified as offline is
/// "could not check"; a classified key refusal keeps its reason; any other
/// tool error is a refusal. A success counts only when it carries records.
fn classify_probe_response(response: &McpCallResult) -> Result<(), McpCredentialProbeError> {
    if response.is_error {
        let marker = response
            .structured_content
            .as_ref()
            .and_then(|value| value.get("loupist_error"));
        let kind = marker
            .and_then(|marker| marker.get("kind"))
            .and_then(Value::as_str);
        let reason = marker
            .and_then(|marker| marker.get("reason"))
            .and_then(Value::as_str);
        return Err(match (kind, reason) {
            (Some("offline"), _) => McpCredentialProbeError::Unavailable,
            (Some("credential_rejected"), Some("key_format")) => {
                McpCredentialProbeError::RejectedFormat
            }
            _ => McpCredentialProbeError::Rejected,
        });
    }
    let has_records = response
        .structured_content
        .as_ref()
        .and_then(|value| value.get("results"))
        .is_some_and(|results| match results {
            Value::Array(records) => !records.is_empty(),
            Value::Object(record) => !record.is_empty(),
            _ => false,
        });
    if has_records {
        Ok(())
    } else {
        Err(McpCredentialProbeError::NoData)
    }
}

pub(super) fn credential_statuses(
    state: &AppState,
    entry: &MarketplaceEntryDto,
) -> Result<Vec<MarketplaceCredentialStatusDto>, CommandError> {
    let fields = entry
        .setup
        .as_ref()
        .map(|setup| setup.credential_fields.as_slice())
        .unwrap_or_default();
    if fields.is_empty() {
        return Ok(Vec::new());
    }
    let status = match crate::finance_credentials::status(&state.credential_environment, &entry.id)
    {
        Ok(status) => status,
        // One unreadable key never fails the list: only its own connection
        // shows it, as locked (macOS refused access) or as a read error.
        Err(error) => {
            let locked = matches!(
                error,
                crate::finance_credentials::FinanceCredentialError::Locked
            );
            return Ok(fields
                .iter()
                .map(|field| MarketplaceCredentialStatusDto {
                    entry_id: entry.id.clone(),
                    credential_id: field.id.clone(),
                    stored: true,
                    active: false,
                    pending: false,
                    verification: MarketplaceCredentialVerification::Never,
                    verified_at: None,
                    last_error: (!locked)
                        .then(|| "Loupist could not read the saved credentials.".to_owned()),
                    krx_coverage: None,
                    locked,
                })
                .collect());
        }
    };
    Ok(fields
        .iter()
        .map(|field| {
            let active = status.active_fields.contains(&field.id);
            let pending = status.candidate_fields.contains(&field.id);
            let verification = if pending {
                match status.verification {
                    crate::finance_credentials::CredentialVerification::Never => {
                        MarketplaceCredentialVerification::Never
                    }
                    crate::finance_credentials::CredentialVerification::Verified => {
                        MarketplaceCredentialVerification::Verified
                    }
                    crate::finance_credentials::CredentialVerification::Rejected => {
                        MarketplaceCredentialVerification::Rejected
                    }
                    crate::finance_credentials::CredentialVerification::TemporarilyUnavailable => {
                        MarketplaceCredentialVerification::TemporarilyUnavailable
                    }
                }
            } else if active {
                MarketplaceCredentialVerification::Verified
            } else {
                MarketplaceCredentialVerification::Never
            };
            let last_error = match verification {
                MarketplaceCredentialVerification::Rejected => {
                    Some("The provider rejected these credentials.".to_owned())
                }
                MarketplaceCredentialVerification::TemporarilyUnavailable => {
                    Some("The provider could not verify these credentials right now.".to_owned())
                }
                MarketplaceCredentialVerification::Never
                | MarketplaceCredentialVerification::Verified => None,
            };
            MarketplaceCredentialStatusDto {
                entry_id: entry.id.clone(),
                credential_id: field.id.clone(),
                stored: active || pending,
                active,
                pending,
                verification,
                verified_at: active.then_some(status.verified_at).flatten(),
                last_error,
                krx_coverage: status.krx_coverage,
                locked: false,
            }
        })
        .collect())
}

/// A credential store failure as a command error. A refused macOS prompt has
/// its own code so the renderer can say which connection is locked.
pub(crate) fn credential_store_error(
    error: crate::finance_credentials::FinanceCredentialError,
) -> CommandError {
    match error {
        crate::finance_credentials::FinanceCredentialError::Locked => CommandError::new(
            crate::app::CommandErrorCode::CredentialLocked,
            "macOS did not allow Loupist to read this saved key. Choose Try again and allow access.",
        ),
        error => CommandError::internal(error.to_string()),
    }
}

pub(super) fn credential_provider_context(
    state: &AppState,
    entry_id: &str,
) -> Result<Option<String>, CommandError> {
    match entry_id {
        "sec.edgar" => connector_config_value(state, entry_id, "contact_email"),
        crate::finance_data::KIS_SOURCE_ID => {
            connector_config_value(state, entry_id, "environment")
        }
        _ => Ok(None),
    }
}

pub(super) fn credential_entry<'a>(
    catalog: &'a MarketplaceCatalogDto,
    entry_id: &str,
) -> Result<&'a MarketplaceEntryDto, CommandError> {
    let entry = catalog
        .entries
        .iter()
        .find(|entry| entry.id == entry_id)
        .ok_or_else(|| CommandError::not_found("Marketplace entry"))?;
    let fields = entry
        .setup
        .as_ref()
        .map(|setup| setup.credential_fields.as_slice())
        .unwrap_or_default();
    if fields.is_empty() {
        return Err(CommandError::invalid(
            "connector credentials are not declared",
        ));
    }
    Ok(entry)
}

pub(super) fn validate_credential_secrets(
    entry: &MarketplaceEntryDto,
    secrets: &BTreeMap<String, String>,
) -> Result<(), CommandError> {
    let fields = entry
        .setup
        .as_ref()
        .map(|setup| setup.credential_fields.as_slice())
        .unwrap_or_default();
    if secrets.is_empty()
        || secrets
            .keys()
            .any(|credential_id| !fields.iter().any(|field| field.id == *credential_id))
    {
        return Err(CommandError::invalid(
            "Enter at least one declared credential or profile field.",
        ));
    }
    for (credential_id, secret) in secrets {
        let field = fields
            .iter()
            .find(|field| field.id == *credential_id)
            .expect("submitted credential ids were checked above");
        if !(field.min_length..=field.max_length).contains(&secret.len()) {
            return Err(CommandError::invalid(format!(
                "{} must be between {} and {} characters.",
                field.label, field.min_length, field.max_length
            )));
        }
        if secret.contains('\0') || secret.chars().any(char::is_control) {
            return Err(CommandError::invalid(format!(
                "{} contains unsupported characters.",
                field.label
            )));
        }
        if matches!(field.kind, MarketplaceSetupFieldKind::ApiKey)
            && secret.chars().any(char::is_whitespace)
        {
            return Err(CommandError::invalid(format!(
                "{} cannot contain whitespace.",
                field.label
            )));
        }
        if !valid_setup_value(field, secret) {
            return Err(CommandError::invalid(format!(
                "Enter a valid {}.",
                field.label.to_lowercase()
            )));
        }
        if entry.id == crate::finance_data::KIS_SOURCE_ID
            && field.id == "account_number"
            && !secret.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(CommandError::invalid(
                "KIS account number must contain exactly 8 digits.",
            ));
        }
        if entry.id == crate::finance_data::KIS_SOURCE_ID
            && field.id == "account_product_code"
            && !secret.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(CommandError::invalid(
                "KIS account product code must be alphanumeric.",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_required_credential_fields(
    entry: &MarketplaceEntryDto,
    patch: &BTreeMap<String, String>,
    current: &crate::finance_credentials::CredentialStatus,
) -> Result<(), CommandError> {
    let fields = entry
        .setup
        .as_ref()
        .map(|setup| setup.credential_fields.as_slice())
        .unwrap_or_default();
    if let Some(missing) = fields.iter().find(|field| {
        field.required
            && !patch.contains_key(&field.id)
            && !current.active_fields.contains(&field.id)
            && !current.candidate_fields.contains(&field.id)
    }) {
        return Err(CommandError::invalid(format!(
            "{} is required.",
            missing.label
        )));
    }
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn marketplace_credential_save(
    request: MarketplaceCredentialSaveRequest,
    state: State<'_, AppState>,
) -> Result<Vec<MarketplaceCredentialStatusDto>, CommandError> {
    off_main_thread(&state, move |state| {
        save_credential_for_state(request, state)
    })
    .await
}

/// Try again on a connection whose saved key macOS refused: asks macOS once
/// more (the user chose to) and returns the connection's credentials, still
/// `locked` when access was refused again.
#[tauri::command(rename_all = "snake_case")]
pub async fn marketplace_credential_unlock(
    request: MarketplaceCredentialRequest,
    state: State<'_, AppState>,
) -> Result<Vec<MarketplaceCredentialStatusDto>, CommandError> {
    off_main_thread(&state, move |state| {
        unlock_credential_for_state(request, state)
    })
    .await
}

pub(super) fn unlock_credential_for_state(
    request: MarketplaceCredentialRequest,
    state: &AppState,
) -> Result<Vec<MarketplaceCredentialStatusDto>, CommandError> {
    let catalog = bundled_catalog()?;
    ensure_installable(&request.entry_id)?;
    let entry = credential_entry(&catalog, &request.entry_id)?;
    match crate::finance_credentials::unlock(&state.credential_environment, &request.entry_id) {
        Ok(_) | Err(crate::finance_credentials::FinanceCredentialError::Locked) => {}
        Err(error) => return Err(CommandError::internal(error.to_string())),
    }
    credential_statuses(state, entry)
}

pub(super) fn save_credential_for_state(
    request: MarketplaceCredentialSaveRequest,
    state: &AppState,
) -> Result<Vec<MarketplaceCredentialStatusDto>, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::MarketplaceCredential)?;
    let catalog = bundled_catalog()?;
    ensure_installable(&request.entry_id)?;
    let _lifecycle_guard = connector_lifecycle_lock()?;
    reconcile_connector_configs_locked(state, &_lifecycle_guard)?;
    let entry = credential_entry(&catalog, &request.entry_id)?;
    validate_credential_secrets(entry, &request.secrets)?;
    // Saving is the user's own action: a key refused earlier this session is
    // read again here (macOS may ask) rather than reported as locked.
    let current =
        crate::finance_credentials::unlock(&state.credential_environment, &request.entry_id)
            .map_err(credential_store_error)?;
    validate_required_credential_fields(entry, &request.secrets, &current)?;
    crate::finance_credentials::stage(
        &state.credential_environment,
        &request.entry_id,
        &request.secrets,
    )
    .map_err(credential_store_error)?;
    credential_statuses(state, entry)
}

pub(super) fn credential_verification_outcome<T>(
    result: &Result<T, crate::finance_data::FinanceDataError>,
) -> crate::finance_credentials::VerificationOutcome {
    use crate::finance_data::FinanceDataError;

    match result {
        Ok(_) => crate::finance_credentials::VerificationOutcome::Verified,
        Err(FinanceDataError::CredentialRejected(_))
        | Err(FinanceDataError::KisCredentialRejected(_))
        | Err(FinanceDataError::ServiceApprovalRequired { .. })
        | Err(FinanceDataError::InvalidQuery(_)) => {
            crate::finance_credentials::VerificationOutcome::Rejected
        }
        Err(_) => crate::finance_credentials::VerificationOutcome::TemporarilyUnavailable,
    }
}

pub(super) fn credential_verification_error(
    error: &crate::finance_data::FinanceDataError,
) -> CommandError {
    use crate::finance_data::FinanceDataError;

    match error {
        FinanceDataError::KisCredentialRejected(diagnostic) => CommandError::new(
            "credential_rejected",
            format!("KIS error · {}", diagnostic.summary()),
        ),
        FinanceDataError::KisRateLimited(diagnostic) => CommandError::new(
            "provider_unavailable",
            format!(
                "KIS token verification is temporarily limited. KIS error · {}",
                diagnostic.summary()
            ),
        ),
        FinanceDataError::CredentialRejected(_)
        | FinanceDataError::InvalidQuery(_)
        | FinanceDataError::ServiceApprovalRequired { .. } => CommandError::new(
            "credential_rejected",
            "The provider rejected these credentials.",
        ),
        _ => CommandError::new(
            "provider_unavailable",
            "The provider could not verify these credentials right now.",
        ),
    }
}

fn credential_scope_snapshot(
    state: &AppState,
    entry: &MarketplaceEntryDto,
) -> Result<BTreeMap<String, String>, CommandError> {
    let Some(setup) = &entry.setup else {
        return Ok(BTreeMap::new());
    };
    setup
        .credential_scope_fields
        .iter()
        .map(|field_id| {
            let value = connector_config_value(state, &entry.id, field_id)?.ok_or_else(|| {
                CommandError::conflict("connector configuration must be completed first")
            })?;
            Ok((field_id.clone(), value))
        })
        .collect()
}

async fn verify_bundled_mcp_credential(
    state: &AppState,
    entry: &MarketplaceEntryDto,
    candidate_secrets: &BTreeMap<String, String>,
) -> Result<(), McpCredentialProbeError> {
    let server_id = entry
        .runtime
        .server_id
        .as_deref()
        .ok_or(McpCredentialProbeError::Unavailable)?;
    let runtime = state
        .artifacts
        .mcp_runtimes
        .get(server_id)
        .cloned()
        .ok_or(McpCredentialProbeError::Unavailable)?;
    let probe = entry
        .runtime
        .verification_probe
        .as_ref()
        .ok_or(McpCredentialProbeError::Unavailable)?;
    let enabled_provider_ids = entry
        .runtime
        .provider_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if enabled_provider_ids.is_empty() || !enabled_provider_ids.is_subset(&runtime.provider_ids) {
        return Err(McpCredentialProbeError::Unavailable);
    }
    let possible_network_hosts = enabled_provider_ids
        .iter()
        .filter_map(|provider| runtime.provider_network_hosts.get(provider))
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>();
    let allowed_network_hosts = entry
        .permissions
        .network_hosts
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if allowed_network_hosts.is_empty() || allowed_network_hosts != possible_network_hosts {
        return Err(McpCredentialProbeError::Unavailable);
    }
    let provider_receipt_pointer = runtime.provider_receipt_pointer.clone();
    let tool_activation = runtime.tool_activation.clone();
    let control_tool_names = runtime.control_tool_names.clone();

    let mut credentials = BTreeMap::new();
    for (field_id, target_id) in &entry.runtime.credential_mapping {
        let secret = candidate_secrets
            .get(field_id)
            .ok_or(McpCredentialProbeError::Unavailable)?
            .clone();
        if credentials.insert(target_id.clone(), secret).is_some() {
            return Err(McpCredentialProbeError::Unavailable);
        }
    }
    let sensitive_values = credentials.values().cloned().collect::<Vec<_>>();
    let mut provider_config = BTreeMap::<String, BTreeMap<String, String>>::new();
    if !entry.runtime.config_mapping.is_empty() {
        let [provider_id] = entry.runtime.provider_ids.as_slice() else {
            return Err(McpCredentialProbeError::Unavailable);
        };
        let values = provider_config.entry(provider_id.clone()).or_default();
        for field_id in entry.runtime.config_mapping.keys() {
            let value = connector_config_value(state, &entry.id, field_id)
                .map_err(|_| McpCredentialProbeError::Unavailable)?
                .ok_or(McpCredentialProbeError::Unavailable)?;
            values.insert(field_id.clone(), value);
        }
    }

    let scratch = RunScratch::create(
        state.artifacts.deletion_root.clone(),
        "credential-probe",
        &format!("probe-{}", uuid::Uuid::new_v4().simple()),
    )
    .map_err(|_| McpCredentialProbeError::Unavailable)?;
    let launch = McpLaunchConfig {
        server_id: server_id.to_owned(),
        executable: runtime.executable,
        arguments: Vec::new(),
        private_working_dir: scratch.path().to_path_buf(),
        lease_dir: runtime.lease_dir,
        environment: BTreeMap::new(),
        bootstrap: json!({
            "type": "loupist.bootstrap",
            "protocol_version": 1,
            "run_id": format!("credential-probe:{}", uuid::Uuid::new_v4().simple()),
            "scratch_dir": scratch.path().to_string_lossy(),
            "credentials": credentials,
            "settings": {
                "allowed_categories": runtime.allowed_categories,
                "enabled_provider_ids": enabled_provider_ids,
                "allowed_network_hosts": allowed_network_hosts,
                "provider_config": provider_config,
            }
        }),
    };
    let (session, initial_tools) = McpSession::spawn(launch)
        .await
        .map_err(|_| McpCredentialProbeError::Unavailable)?;
    let result = async {
        if initial_tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<BTreeSet<_>>()
            != control_tool_names
        {
            return Err(McpCredentialProbeError::Unavailable);
        }
        if !initial_tools
            .iter()
            .any(|tool| tool.name == probe.tool_name)
        {
            let activation = tool_activation
                .as_ref()
                .ok_or(McpCredentialProbeError::Unavailable)?;
            if !initial_tools
                .iter()
                .any(|tool| tool.name == activation.tool_name)
            {
                return Err(McpCredentialProbeError::Unavailable);
            }
            let activation_arguments = Value::Object(serde_json::Map::from_iter([(
                activation.argument_name.clone(),
                json!([probe.tool_name.clone()]),
            )]));
            let activation_result = session
                .call_tool(
                    &activation.tool_name,
                    activation_arguments,
                    MCP_CREDENTIAL_PROBE_TIMEOUT,
                )
                .await
                .map_err(|_| McpCredentialProbeError::Unavailable)?;
            if activation_result.is_error {
                return Err(McpCredentialProbeError::Unavailable);
            }
        }
        let listed = session
            .list_tools()
            .await
            .map_err(|_| McpCredentialProbeError::Unavailable)?;
        let tool = listed
            .iter()
            .find(|tool| tool.name == probe.tool_name)
            .ok_or(McpCredentialProbeError::Unavailable)?;
        let filtered = filter_tool_providers(tool, &enabled_provider_ids, false)
            .map_err(|_| McpCredentialProbeError::Unavailable)?
            .ok_or(McpCredentialProbeError::Unavailable)?;
        let requested_provider = probe
            .arguments
            .get("provider")
            .and_then(|value| value.as_str());
        if requested_provider.is_some_and(|provider| !enabled_provider_ids.contains(provider)) {
            return Err(McpCredentialProbeError::Unavailable);
        }
        if filtered
            .input_schema
            .get("properties")
            .and_then(|value| value.get("provider"))
            .is_some()
            && requested_provider.is_none()
        {
            return Err(McpCredentialProbeError::Unavailable);
        }
        let response = session
            .call_tool(
                &probe.tool_name,
                probe.arguments.clone(),
                MCP_CREDENTIAL_PROBE_TIMEOUT,
            )
            .await
            .map_err(|error| match error {
                McpError::Remote(_) => McpCredentialProbeError::Rejected,
                _ => McpCredentialProbeError::Unavailable,
            })?;
        let encoded =
            serde_json::to_value(&response).map_err(|_| McpCredentialProbeError::Unavailable)?;
        if contains_protected_value(&encoded, &sensitive_values) {
            return Err(McpCredentialProbeError::Unavailable);
        }
        if !response.is_error {
            validate_result_provider(
                &response,
                &provider_receipt_pointer,
                requested_provider,
                &enabled_provider_ids,
            )
            .map_err(|_| McpCredentialProbeError::Unavailable)?;
        }
        classify_probe_response(&response)
    }
    .await;
    let shutdown = session.shutdown(MCP_CREDENTIAL_SHUTDOWN_GRACE).await;
    if result.is_ok() && shutdown.is_err() {
        return Err(McpCredentialProbeError::Unavailable);
    }
    result
}

#[tauri::command(rename_all = "snake_case")]
pub async fn marketplace_credential_verify(
    request: MarketplaceCredentialRequest,
    state: State<'_, AppState>,
) -> Result<Vec<MarketplaceCredentialStatusDto>, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::MarketplaceCredential)?;
    let catalog = bundled_catalog()?;
    ensure_installable(&request.entry_id)?;
    let entry = credential_entry(&catalog, &request.entry_id)?;
    let (
        candidate,
        candidate_secrets,
        verification_secrets,
        provider_context,
        scope_snapshot,
        was_ready,
    ) = {
        let _lifecycle_guard = connector_lifecycle_lock()?;
        reconcile_connector_configs_locked(&state, &_lifecycle_guard)?;
        let was_ready = connector_ready(entry, &state)?;
        let Some(candidate) = crate::finance_credentials::candidate_for_verification(
            &state.credential_environment,
            &request.entry_id,
        )
        .map_err(credential_store_error)?
        else {
            return Err(CommandError::conflict(
                "connector credentials must be saved first",
            ));
        };
        let required_ids = entry
            .setup
            .as_ref()
            .map(|setup| {
                setup
                    .credential_fields
                    .iter()
                    .filter(|field| field.required)
                    .map(|field| field.id.as_str())
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let candidate_secrets = candidate.secrets().clone();
        let verification_secrets = candidate
            .secrets()
            .iter()
            .filter(|(credential_id, _)| required_ids.contains(credential_id.as_str()))
            .map(|(credential_id, secret)| (credential_id.clone(), secret.clone()))
            .collect::<BTreeMap<_, _>>();
        let provider_context = credential_provider_context(&state, &request.entry_id)?;
        let scope_snapshot = credential_scope_snapshot(&state, entry)?;
        (
            candidate,
            candidate_secrets,
            verification_secrets,
            provider_context,
            scope_snapshot,
            was_ready,
        )
    };
    let revision = candidate.revision().to_owned();
    if entry.runtime.kind == MarketplaceRuntimeKind::BundledMcp {
        let result = verify_bundled_mcp_credential(&state, entry, &candidate_secrets).await;
        drop(candidate);
        let outcome = match result {
            Ok(()) => crate::finance_credentials::VerificationOutcome::Verified,
            Err(error) => error.outcome(),
        };
        let _lifecycle_guard = connector_lifecycle_lock()?;
        reconcile_connector_configs_locked(&state, &_lifecycle_guard)?;
        if credential_scope_snapshot(&state, entry)? != scope_snapshot {
            return Err(CommandError::conflict(
                "connector configuration changed while credential verification was in progress",
            ));
        }
        let finished = crate::finance_credentials::finish_verification(
            &state.credential_environment,
            &request.entry_id,
            &revision,
            outcome,
            Utc::now().timestamp_millis(),
            None,
        )
        .map_err(credential_store_error)?;
        if finished == crate::finance_credentials::FinishVerification::Stale {
            return Err(CommandError::conflict(
                "connector credential changed while verification was in progress",
            ));
        }
        return match result {
            Ok(()) => {
                if !was_ready {
                    enable_ready_bindings_locked(&state, &request.entry_id, &_lifecycle_guard)?;
                }
                credential_statuses(&state, entry)
            }
            Err(error) => Err(error.command_error()),
        };
    }
    let result = state
        .finance_data
        .verify_credential(
            &request.entry_id,
            &verification_secrets,
            provider_context.as_deref(),
        )
        .await;
    drop(candidate);
    let outcome = credential_verification_outcome(&result);
    let _lifecycle_guard = connector_lifecycle_lock()?;
    reconcile_connector_configs_locked(&state, &_lifecycle_guard)?;
    if credential_provider_context(&state, &request.entry_id)? != provider_context
        || credential_scope_snapshot(&state, entry)? != scope_snapshot
    {
        return Err(CommandError::conflict(
            "connector configuration changed while credential verification was in progress",
        ));
    }
    let finished = crate::finance_credentials::finish_verification(
        &state.credential_environment,
        &request.entry_id,
        &revision,
        outcome,
        Utc::now().timestamp_millis(),
        result.as_ref().ok().copied().flatten(),
    )
    .map_err(credential_store_error)?;
    if finished == crate::finance_credentials::FinishVerification::Stale {
        return Err(CommandError::conflict(
            "connector credential changed while verification was in progress",
        ));
    }
    if let Err(error) = result {
        return Err(credential_verification_error(&error));
    }
    if !was_ready {
        enable_ready_bindings_locked(&state, &request.entry_id, &_lifecycle_guard)?;
    }
    credential_statuses(&state, entry)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn marketplace_credential_delete(
    request: MarketplaceCredentialRequest,
    state: State<'_, AppState>,
) -> Result<Vec<MarketplaceCredentialStatusDto>, CommandError> {
    let entry_id = request.entry_id.clone();
    let result = off_main_thread(&state, move |state| {
        delete_credential_for_state(request, state)
    })
    .await;
    if entry_id == crate::finance_data::KIS_SOURCE_ID {
        state.finance_data.clear_kis_token_cache().await;
    }
    result
}

pub(super) fn delete_credential_for_state(
    request: MarketplaceCredentialRequest,
    state: &AppState,
) -> Result<Vec<MarketplaceCredentialStatusDto>, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::MarketplaceCredential)?;
    let catalog = bundled_catalog()?;
    ensure_installable(&request.entry_id)?;
    let entry = credential_entry(&catalog, &request.entry_id)?;
    let lifecycle_guard = connector_lifecycle_lock()?;
    disable_bindings_and_delete_credentials_locked(state, &request.entry_id, &lifecycle_guard)?;
    credential_statuses(state, entry)
}

pub(super) fn disable_bindings_and_delete_credentials_locked(
    state: &AppState,
    entry_id: &str,
    _lifecycle_guard: &MutexGuard<'static, ()>,
) -> Result<(), CommandError> {
    // Visit every reachable folder even if another folder is offline. Until
    // offline revocations can be reconciled on folder open, retain the secret
    // and refuse success rather than let an old grant authorize a replacement.
    let mut unavailable = Vec::new();
    for loupist in state
        .store
        .list_loupists()
        .map_err(|error| CommandError::internal(error.to_string()))?
    {
        let mut binding = match state.store.get_loupist_capability(&loupist.id, entry_id) {
            Ok(Some(binding)) => binding,
            Ok(None) => continue,
            Err(_) => {
                unavailable.push(format!("{} ({})", loupist.name, loupist.workspace_root));
                continue;
            }
        };
        if binding.enabled || !binding.granted_permissions.is_empty() {
            binding.enabled = false;
            binding.granted_permissions.clear();
            binding.updated_at_ms = Utc::now()
                .timestamp_millis()
                .max(binding.updated_at_ms.saturating_add(1));
            if state.store.save_loupist_capability(&binding).is_err() {
                unavailable.push(format!("{} ({})", loupist.name, loupist.workspace_root));
            }
        }
    }
    if !unavailable.is_empty() {
        return Err(CommandError::conflict(format!(
            "Reconnect these Loupists and retry deleting the credential: {}. Access was removed from reachable Loupists; the credential is still stored.",
            unavailable.join(", ")
        )));
    }
    crate::finance_credentials::delete(&state.credential_environment, entry_id)
        .map_err(credential_store_error)
}

#[cfg(test)]
mod probe_tests {
    use super::*;
    use crate::finance_credentials::VerificationOutcome;

    fn answer(is_error: bool, structured: Option<Value>, text: &str) -> McpCallResult {
        McpCallResult {
            content: vec![json!({"type": "text", "text": text})],
            structured_content: structured,
            is_error,
            metadata: None,
        }
    }

    fn outcome(response: &McpCallResult) -> VerificationOutcome {
        match classify_probe_response(response) {
            Ok(()) => VerificationOutcome::Verified,
            Err(error) => error.outcome(),
        }
    }

    #[test]
    fn only_an_answer_with_records_verifies_a_key() {
        let accepted = answer(
            false,
            Some(json!({"provider": "fred", "results": [{"date": "1947-01-01", "GDP": 243.164}]})),
            "",
        );
        assert_eq!(classify_probe_response(&accepted), Ok(()));
        assert_eq!(outcome(&accepted), VerificationOutcome::Verified);

        // OpenBB FRED turned FRED's HTTP 400 for a rejected key into this.
        for empty in [
            json!({"provider": "fred", "results": []}),
            json!({"provider": "fred", "results": {}}),
            json!({"provider": "fred", "results": null}),
            json!({"provider": "fred"}),
            json!({"provider": "fred", "results": "GDP"}),
            Value::Null,
        ] {
            let response = answer(false, Some(empty.clone()), "");
            assert_eq!(
                classify_probe_response(&response),
                Err(McpCredentialProbeError::NoData),
                "{empty}"
            );
            assert_eq!(
                outcome(&response),
                VerificationOutcome::TemporarilyUnavailable
            );
        }
        assert_eq!(
            classify_probe_response(&answer(false, None, "{\"results\": [1]}")),
            Err(McpCredentialProbeError::NoData)
        );
    }

    #[test]
    fn provider_refusals_are_rejected_with_their_reason() {
        let format = answer(
            true,
            Some(json!({"loupist_error": {"kind": "credential_rejected", "reason": "key_format"}})),
            "The data provider rejected the configured key.",
        );
        assert_eq!(
            classify_probe_response(&format),
            Err(McpCredentialProbeError::RejectedFormat)
        );
        let not_accepted = answer(
            true,
            Some(
                json!({"loupist_error": {"kind": "credential_rejected", "reason": "key_not_accepted"}}),
            ),
            "The data provider rejected the configured key.",
        );
        assert_eq!(
            classify_probe_response(&not_accepted),
            Err(McpCredentialProbeError::Rejected)
        );
        // Providers whose OpenBB fetcher raises on HTTP 400, 401 or 403 reach
        // Loupist as an unclassified tool error.
        for status in [400, 401, 403, 502] {
            let refused = answer(true, None, &format!("Error calling tool (HTTP {status})"));
            assert_eq!(
                classify_probe_response(&refused),
                Err(McpCredentialProbeError::Rejected)
            );
            assert_eq!(outcome(&refused), VerificationOutcome::Rejected);
        }
        let unknown_reason = answer(
            true,
            Some(json!({"loupist_error": {"kind": "credential_rejected", "reason": "other"}})),
            "",
        );
        assert_eq!(
            classify_probe_response(&unknown_reason),
            Err(McpCredentialProbeError::Rejected)
        );
    }

    #[test]
    fn a_network_failure_is_could_not_check_not_rejected() {
        let offline = answer(
            true,
            Some(json!({"loupist_error": {"kind": "offline"}})),
            "The data provider could not be reached.",
        );
        assert_eq!(
            classify_probe_response(&offline),
            Err(McpCredentialProbeError::Unavailable)
        );
        assert_eq!(
            outcome(&offline),
            VerificationOutcome::TemporarilyUnavailable
        );
    }

    #[test]
    fn verification_errors_have_fixed_codes_and_key_free_text() {
        for (error, code) in [
            (McpCredentialProbeError::Rejected, "credential_rejected"),
            (
                McpCredentialProbeError::RejectedFormat,
                "credential_format_rejected",
            ),
            (McpCredentialProbeError::Unavailable, "provider_unavailable"),
            (McpCredentialProbeError::NoData, "provider_unavailable"),
        ] {
            let command = error.command_error();
            assert_eq!(command.code, code);
            assert!(!command.message.is_empty());
        }
    }
}
