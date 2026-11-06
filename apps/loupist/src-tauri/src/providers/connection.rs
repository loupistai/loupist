use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tauri::{ipc::Channel, State};
use uuid::Uuid;

use crate::{
    app::{AppState, CommandError},
    fs::artifact_trust::{
        create_private_directory, ensure_private_directory, ensure_private_regular_file,
        read_private_regular_file_bounded, ArtifactTrustError,
    },
    pi::support::{self as pi_support, SupportFailure},
    pi::PiSupportLaunchConfig,
    providers::catalog::{
        catalog_allows_authorization, provider_credential_from_environment,
        provider_credential_generation, provider_options, ConfiguredModel, ModelCatalogView,
        ModelProviderOption, ModelRunControl,
    },
    providers::custom::{self, CustomProviderRequest},
    providers::status::{self, CredentialKind, Trigger},
    providers::support::{ManualEntry, PasteRule, ProviderSupportLease},
    web::{SearchCancel, SearchHits, SearchProviderId, WebError, WebSearchQuery, WebSource},
};

const PROVIDER_PROTOCOL: &str = "loupist-provider/1";
const MAX_RESULT_BYTES: u64 = 512 * 1024;
const MAX_API_KEY_BYTES: usize = 8 * 1024;
const SUPPORT_TIMEOUT: Duration = Duration::from_secs(30);
const OAUTH_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The extension's paste prompt carries this title; any other UI request is refused.
const MANUAL_CODE_TITLE: &str = "loupist-provider/1:manual_code";
const MAX_USER_CODE_CHARS: usize = 32;
const PROVIDER_MODEL_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_PROVIDER_MODEL_CACHE_ENTRIES: usize = 32;

#[derive(Clone)]
pub(crate) struct ProviderModelDiscoveryCache {
    inner: Arc<Mutex<VecDeque<ProviderModelCacheEntry>>>,
    ttl: Duration,
    max_entries: usize,
}

struct ProviderModelCacheEntry {
    provider: String,
    credential_generation: String,
    refreshed_at: Instant,
}

impl Default for ProviderModelDiscoveryCache {
    fn default() -> Self {
        Self::with_limits(PROVIDER_MODEL_CACHE_TTL, MAX_PROVIDER_MODEL_CACHE_ENTRIES)
    }
}

impl ProviderModelDiscoveryCache {
    fn with_limits(ttl: Duration, max_entries: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
            ttl,
            max_entries: max_entries.max(1),
        }
    }

    fn is_fresh(&self, provider: &str, credential_generation: &str) -> bool {
        self.is_fresh_at(provider, credential_generation, Instant::now())
    }

    fn is_fresh_at(&self, provider: &str, credential_generation: &str, now: Instant) -> bool {
        let mut entries = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.retain(|entry| now.saturating_duration_since(entry.refreshed_at) < self.ttl);
        let Some(index) = entries.iter().position(|entry| entry.provider == provider) else {
            return false;
        };
        let entry = entries
            .remove(index)
            .expect("provider model cache index disappeared");
        if entry.credential_generation != credential_generation {
            return false;
        }
        entries.push_back(entry);
        true
    }

    fn record(&self, provider: &str, credential_generation: String) {
        self.record_at(provider, credential_generation, Instant::now());
    }

    fn record_at(&self, provider: &str, credential_generation: String, now: Instant) {
        let mut entries = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.retain(|entry| {
            entry.provider != provider
                && now.saturating_duration_since(entry.refreshed_at) < self.ttl
        });
        while entries.len() >= self.max_entries {
            entries.pop_front();
        }
        entries.push_back(ProviderModelCacheEntry {
            provider: provider.to_owned(),
            credential_generation,
            refreshed_at: now,
        });
    }

    pub(crate) fn invalidate(&self, provider: &str) {
        let mut entries = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.retain(|entry| entry.provider != provider);
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderModelsRequest {
    pub provider: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConnectionRequest {
    pub provider: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfigureRequest {
    pub provider: String,
    pub api_key: Option<String>,
    #[serde(default)]
    pub clear_saved_key: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderModelOption {
    pub id: String,
    pub name: String,
    pub reasoning: bool,
    pub context_window: u64,
    pub max_tokens: u64,
    pub input: Vec<String>,
    pub thinking_levels: Vec<String>,
    pub thinking_level_map: std::collections::BTreeMap<String, Option<String>>,
    pub run_controls: Vec<ModelRunControl>,
}

/// Exactly `ProviderConnectionEvent` in `src/contracts/model.ts`.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderConnectionEvent {
    /// `url` is the validated sign-in link, sent only when the browser could not open.
    OpeningBrowser {
        message: String,
        opened: bool,
        url: Option<String>,
    },
    Waiting {
        message: String,
    },
    Connected {
        message: String,
    },
    DeviceCode {
        user_code: String,
        verification_uri: String,
        expires_in_s: Option<u64>,
    },
    /// Pi waits for the address the browser shows ("Browser didn't come back?").
    ManualEntry,
    Failed {
        reason: ConnectFailure,
    },
}

/// Why a sign-in ended without a credential; the renderer maps each to copy.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectFailure {
    PlanPermissionDeclined,
    PlanNotEligible,
    PortInUse,
    TimedOut,
    BrowserFailed,
    Offline,
    Denied,
    Other,
}

impl ConnectFailure {
    fn from_extension(reason: &str) -> Self {
        serde_json::from_value(Value::String(reason.to_owned())).unwrap_or(Self::Other)
    }

    fn code(self) -> &'static str {
        match self {
            Self::PlanPermissionDeclined => "plan_permission_declined",
            Self::PlanNotEligible => "plan_not_eligible",
            Self::PortInUse => "port_in_use",
            Self::TimedOut => "timed_out",
            Self::BrowserFailed => "browser_failed",
            Self::Offline => "offline",
            Self::Denied => "denied",
            Self::Other => "other",
        }
    }
}

fn connect_failed(reason: ConnectFailure) -> CommandError {
    CommandError::new("provider_connect_failed", reason.code())
}

/// One running sign-in: its event channel, the paste channel into Pi and the
/// failure reason the extension reported.
struct OAuthSession<'a> {
    provider: &'a str,
    events: &'a Channel<ProviderConnectionEvent>,
    answers: tokio::sync::mpsc::UnboundedSender<crate::pi::support::UiAnswer>,
    failure: std::sync::Mutex<Option<ConnectFailure>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderResult {
    protocol: String,
    #[serde(rename = "type")]
    result_type: String,
    provider: String,
    models: Vec<ProviderModelOption>,
}

pub(crate) struct SupportFiles {
    pub(crate) directory: PathBuf,
    pub(crate) result: PathBuf,
    pub(crate) request: Option<PathBuf>,
}

impl Drop for SupportFiles {
    fn drop(&mut self) {
        if let Some(request) = &self.request {
            let _ = std::fs::remove_file(request);
        }
        let _ = std::fs::remove_file(&self.result);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn provider_models(
    request: ProviderModelsRequest,
    state: State<'_, AppState>,
) -> Result<ModelCatalogView, CommandError> {
    validate_connectable(&state, &request.provider)?;
    if let Some(catalog) = cached_provider_models(&state, &request.provider)? {
        return Ok(catalog);
    }
    let admission = state.provider_support.try_acquire_discovery()?;
    let result = discover_provider_models(&state, &request.provider, &admission).await;
    drop(admission);
    state.graph.wake();
    result
}

#[tauri::command(rename_all = "snake_case")]
pub async fn provider_configure(
    request: ProviderConfigureRequest,
    state: State<'_, AppState>,
) -> Result<ModelCatalogView, CommandError> {
    let provider = provider_option(&request.provider)?;
    if !provider.api_key {
        return Err(CommandError::invalid(
            "this provider must be connected through its browser sign-in",
        ));
    }
    let api_key = request
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(api_key) = api_key {
        validate_api_key(api_key)?;
    }
    let mutates_credentials = api_key.is_some() || request.clear_saved_key;
    let admission = if mutates_credentials {
        state.provider_support.acquire_mutation().await?
    } else {
        state.provider_support.try_acquire_discovery()?
    };
    if mutates_credentials {
        // Invalidate before mutation: a support command can change its durable
        // credential authority even when its response or shutdown later fails.
        state.provider_model_cache.invalidate(&request.provider);
        let operation = if api_key.is_some() { "set" } else { "clear" };
        let result = run_support_command(
            &state,
            format!("/loupist-provider-api-key {} {operation}", request.provider),
            None,
            None,
            api_key.map(str::to_owned),
            &admission,
            None,
        )
        .await?;
        if result.result_type != "credential_updated" || result.provider != request.provider {
            return Err(CommandError::internal(
                "Pi returned the wrong credential update result",
            ));
        }
        record_status(
            &state,
            &request.provider,
            if api_key.is_some() {
                Trigger::Connected(CredentialKind::ApiKey)
            } else {
                Trigger::Disconnected(status::used_by(&state, &request.provider))
            },
        );
    }
    if request.clear_saved_key && api_key.is_none() {
        state.replace_provider_models(&request.provider, Vec::new())?;
        drop(admission);
        state.graph.wake();
        return state.model_catalog_view();
    }
    let result = discover_provider_models(&state, &request.provider, &admission).await;
    drop(admission);
    state.graph.wake();
    result
}

#[tauri::command(rename_all = "snake_case")]
pub async fn provider_connect(
    request: ProviderConnectionRequest,
    on_event: Channel<ProviderConnectionEvent>,
    state: State<'_, AppState>,
) -> Result<ModelCatalogView, CommandError> {
    let provider = provider_option(&request.provider)?;
    if provider.oauth.is_none() {
        return Err(CommandError::invalid(
            "provider does not support OAuth connection",
        ));
    }
    let admission = state.provider_support.acquire_oauth().await?;

    // OAuth may replace durable authority before the support command reports
    // completion, so stale model discovery must become unreachable first.
    state.provider_model_cache.invalidate(&request.provider);

    let (answers, pasted) = tokio::sync::mpsc::unbounded_channel();
    let session = OAuthSession {
        provider: &request.provider,
        events: &on_event,
        answers,
        failure: std::sync::Mutex::new(None),
    };
    let result = run_support_command(
        &state,
        format!("/loupist-provider-login {}", request.provider),
        Some(&session),
        None,
        None,
        &admission,
        Some(pasted),
    )
    .await;
    let reported = *session
        .failure
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let result = match result {
        Ok(result) => result,
        // Cancel and Escape end quietly: no error row.
        Err(error) if error.code == "cancelled" => return Err(error),
        Err(error) => {
            let reason = reported.or_else(|| {
                (error.message == "Pi provider setup timed out").then_some(ConnectFailure::TimedOut)
            });
            return match reason {
                Some(reason) => {
                    let _ = send_event(&on_event, ProviderConnectionEvent::Failed { reason });
                    Err(connect_failed(reason))
                }
                None => Err(error),
            };
        }
    };
    if result.result_type != "credential_updated" || result.provider != request.provider {
        return Err(CommandError::internal("Pi returned the wrong OAuth result"));
    }
    record_status(
        &state,
        &request.provider,
        Trigger::Connected(CredentialKind::Plan),
    );
    send_event(
        &on_event,
        ProviderConnectionEvent::Connected {
            message: format!(
                "{} is connected. Pi's available models are ready for Loupist runs.",
                provider.label
            ),
        },
    )?;
    let result = discover_provider_models(&state, &request.provider, &admission).await;
    drop(admission);
    state.graph.wake();
    result
}

#[tauri::command(rename_all = "snake_case")]
pub fn provider_connect_cancel(state: State<'_, AppState>) {
    state.provider_support.cancel_oauth();
}

#[tauri::command(rename_all = "snake_case")]
pub fn provider_connect_open_browser(state: State<'_, AppState>) -> Result<(), CommandError> {
    let url = state
        .provider_support
        .oauth_authorization_url()
        .ok_or_else(|| CommandError::conflict("no active sign-in page is ready"))?;
    open_authorization_url(&url)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderSubmitCodeRequest {
    pub value: String,
}

/// The paste fallback: Rust validates the address or code, then answers Pi's
/// waiting prompt. The value is never logged or stored.
#[tauri::command(rename_all = "snake_case")]
pub fn provider_connect_submit_code(
    request: ProviderSubmitCodeRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state.provider_support.submit_oauth_code(&request.value)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderPageRequest {
    pub provider: String,
}

/// The provider's key page; the renderer sends a provider ID, never a URL.
pub(crate) fn console_url(provider: &str) -> Option<&'static str> {
    if provider == crate::providers::local::UPSTAGE_PRESET {
        return Some("https://console.upstage.ai/api-keys");
    }
    provider_options()
        .into_iter()
        .find(|option| option.id == provider)
        .and_then(|option| option.console_url)
}

/// Where a plan's usage is managed: the ChatGPT plan and Claude extra usage.
pub(crate) fn usage_url(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" | "openai-codex" => Some("https://chatgpt.com/settings/usage"),
        "anthropic" => Some("https://claude.ai/settings/usage"),
        _ => None,
    }
}

fn open_fixed_page(url: Option<&'static str>) -> Result<(), CommandError> {
    let url = url.ok_or_else(|| CommandError::invalid("this provider has no page to open"))?;
    crate::web::external_browser::open(url)
        .map_err(|error| CommandError::internal(format!("The page could not open: {error}")))
}

#[tauri::command(rename_all = "snake_case")]
pub fn provider_open_console(request: ProviderPageRequest) -> Result<(), CommandError> {
    open_fixed_page(console_url(&request.provider))
}

#[tauri::command(rename_all = "snake_case")]
pub fn provider_open_usage(request: ProviderPageRequest) -> Result<(), CommandError> {
    open_fixed_page(usage_url(&request.provider))
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelNoticeAckRequest {
    pub id: String,
}

/// The "Now using …" line was shown (sent or dismissed); it does not return.
#[tauri::command(rename_all = "snake_case")]
pub fn model_notice_ack(
    request: ModelNoticeAckRequest,
    state: State<'_, AppState>,
) -> Result<ModelCatalogView, CommandError> {
    if request.id.is_empty() || request.id.len() > 64 {
        return Err(CommandError::invalid("model notice ID is invalid"));
    }
    state
        .store
        .model_notice_ack(&request.id)
        .map_err(|error| CommandError::internal(error.to_string()))?;
    state.model_catalog_view()
}

/// Status writes never fail the connection change that caused them.
fn record_status(state: &AppState, provider: &str, trigger: Trigger) {
    if status::record(state, provider, trigger).is_err() {
        eprintln!("Loupist could not record a provider status change");
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn provider_disconnect(
    request: ProviderConnectionRequest,
    state: State<'_, AppState>,
) -> Result<ModelCatalogView, CommandError> {
    validate_connectable(&state, &request.provider)?;
    let agent_dir = state.pi_agent_data_dir()?;
    // A Disconnected card's Remove: nothing is signed in, only the record stays.
    if provider_credential_generation(&agent_dir, &request.provider)?.is_none()
        && status::load(&state, &request.provider)?.is_some()
    {
        state
            .store
            .provider_status_delete(&request.provider)
            .map_err(|error| CommandError::internal(error.to_string()))?;
        return state.model_catalog_view();
    }
    let admission = state.provider_support.acquire_mutation().await?;
    state.provider_model_cache.invalidate(&request.provider);
    let result = run_support_command(
        &state,
        format!("/loupist-provider-logout {}", request.provider),
        None,
        None,
        None,
        &admission,
        None,
    )
    .await?;
    if result.result_type != "credential_updated" || result.provider != request.provider {
        return Err(CommandError::internal(
            "Pi returned the wrong logout result",
        ));
    }
    record_status(
        &state,
        &request.provider,
        Trigger::Disconnected(status::used_by(&state, &request.provider)),
    );
    state.replace_provider_models(&request.provider, Vec::new())?;
    drop(admission);
    state.graph.wake();
    state.model_catalog_view()
}

/// Adds or edits a custom OpenAI- or Anthropic-compatible provider. A new
/// provider always stores a credential (a placeholder for a keyless local
/// server); an edit keeps the saved key unless a new one is given or the
/// address changed.
#[tauri::command(rename_all = "snake_case")]
pub async fn provider_custom_save(
    request: CustomProviderRequest,
    state: State<'_, AppState>,
) -> Result<ModelCatalogView, CommandError> {
    save_custom_provider(&state, request).await
}

/// The one save path for custom endpoints, local engines and the Upstage preset.
pub(crate) async fn save_custom_provider(
    state: &AppState,
    request: CustomProviderRequest,
) -> Result<ModelCatalogView, CommandError> {
    let agent_dir = state.pi_agent_data_dir()?;
    let provider = custom::prepare(&agent_dir, &request)?;
    let api_key = request
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(api_key) = api_key {
        validate_api_key(api_key)?;
    }
    let admission = state.provider_support.acquire_mutation().await?;
    state.provider_model_cache.invalidate(&provider.id);
    let previous = custom::find(&agent_dir, &provider.id)?;
    let key = custom_key_to_store(
        api_key,
        previous.as_ref().map(|previous| previous.base_url.as_str()),
        &provider.base_url,
        provider_credential_generation(&agent_dir, &provider.id)?.is_some(),
    );
    custom::write(&agent_dir, &provider)?;
    let needs_key = key.is_some();
    if let Some(key) = key {
        let stored = run_support_command(
            state,
            format!("/loupist-provider-api-key {} set", provider.id),
            None,
            None,
            Some(key.to_owned()),
            &admission,
            None,
        )
        .await
        .and_then(|result| {
            if result.result_type == "credential_updated" && result.provider == provider.id {
                Ok(())
            } else {
                Err(CommandError::internal(
                    "Pi returned the wrong credential update result",
                ))
            }
        });
        if let Err(error) = stored {
            // Leave no half-added provider behind.
            match previous {
                Some(previous) => custom::write(&agent_dir, &previous)?,
                None => custom::remove(&agent_dir, &provider.id)?,
            }
            return Err(error);
        }
    }
    if needs_key || previous.is_none() {
        record_status(
            state,
            &provider.id,
            Trigger::Connected(status::custom_kind(&provider.base_url)),
        );
    }
    let result = discover_provider_models(state, &provider.id, &admission).await;
    drop(admission);
    state.graph.wake();
    result
}

/// The key a custom endpoint save stores, or `None` to keep the saved one. A
/// key never follows the endpoint to a new address: a changed address without
/// a new key stores the keyless placeholder, so the user enters it again.
fn custom_key_to_store<'a>(
    api_key: Option<&'a str>,
    previous_base_url: Option<&str>,
    base_url: &str,
    has_saved_key: bool,
) -> Option<&'a str> {
    let moved = previous_base_url.is_some_and(|previous| previous != base_url);
    match api_key {
        Some(key) => Some(key),
        None if moved || !has_saved_key => Some(custom::NO_KEY),
        None => None,
    }
}

/// Signs a custom provider out of Pi, then deletes its entry and models.
#[tauri::command(rename_all = "snake_case")]
pub async fn provider_custom_remove(
    request: ProviderConnectionRequest,
    state: State<'_, AppState>,
) -> Result<ModelCatalogView, CommandError> {
    let agent_dir = state.pi_agent_data_dir()?;
    if custom::find(&agent_dir, &request.provider)?.is_none() {
        return Err(CommandError::not_found("Custom provider"));
    }
    let admission = state.provider_support.acquire_mutation().await?;
    state.provider_model_cache.invalidate(&request.provider);
    if provider_credential_generation(&agent_dir, &request.provider)?.is_some() {
        let result = run_support_command(
            &state,
            format!("/loupist-provider-logout {}", request.provider),
            None,
            None,
            None,
            &admission,
            None,
        )
        .await?;
        if result.result_type != "credential_updated" || result.provider != request.provider {
            return Err(CommandError::internal(
                "Pi returned the wrong logout result",
            ));
        }
    }
    record_status(
        &state,
        &request.provider,
        Trigger::Disconnected(status::used_by(&state, &request.provider)),
    );
    custom::remove(&agent_dir, &request.provider)?;
    state.replace_provider_models(&request.provider, Vec::new())?;
    drop(admission);
    state.graph.wake();
    state.model_catalog_view()
}

fn provider_option(provider: &str) -> Result<ModelProviderOption, CommandError> {
    provider_options()
        .into_iter()
        .find(|option| option.id == provider)
        .ok_or_else(|| CommandError::invalid("unsupported Pi model provider"))
}

async fn discover_provider_models(
    state: &AppState,
    provider: &str,
    admission: &ProviderSupportLease,
) -> Result<ModelCatalogView, CommandError> {
    let agent_data_dir = state.pi_agent_data_dir()?;
    let credential_generation = provider_credential_generation(&agent_data_dir, provider)?
        .ok_or_else(|| {
            CommandError::invalid("connect the provider before loading its Pi models")
        })?;
    // Recheck after admission. A preceding request may have completed while
    // this caller was entering the support path.
    if state
        .provider_model_cache
        .is_fresh(provider, &credential_generation)
    {
        return state.model_catalog_view();
    }
    // A ChatGPT sign-in lists what the plan offers (`GET /v1/models`); the
    // extension falls back to Pi's catalog when that call fails.
    let live = provider == "openai"
        && crate::providers::catalog::provider_credential_kind(&agent_data_dir, provider)?
            .as_deref()
            == Some("oauth");
    let result = run_support_command(
        state,
        format!(
            "/loupist-provider-models {provider}{}",
            if live { " --live" } else { "" }
        ),
        None,
        provider_credential_from_environment(provider),
        None,
        admission,
        None,
    )
    .await?;
    if result.result_type != "models" || result.provider != provider {
        return Err(CommandError::internal(
            "Pi returned the wrong provider catalog",
        ));
    }
    let models = result
        .models
        .into_iter()
        .map(|model| configured_model(provider, model))
        .collect::<Result<Vec<_>, _>>()?;
    let current_generation = provider_credential_generation(&agent_data_dir, provider)?;
    publish_provider_models(
        state,
        provider,
        models,
        &credential_generation,
        current_generation.as_deref(),
        admission,
    )?;
    state.model_catalog_view()
}

fn publish_provider_models(
    state: &AppState,
    provider: &str,
    models: Vec<ConfiguredModel>,
    expected_generation: &str,
    current_generation: Option<&str>,
    admission: &ProviderSupportLease,
) -> Result<(), CommandError> {
    if admission.is_cancelled() {
        return Err(provider_operation_cancelled());
    }
    if current_generation != Some(expected_generation) {
        return Err(CommandError::conflict(
            "Provider credentials changed while loading models. Reload the provider's models.",
        ));
    }
    // A model the provider answered 404 for stays out, even when Pi's bundled
    // catalog still lists it, until the provider is connected again.
    let gone = state
        .store
        .gone_models(provider)
        .map_err(|error| CommandError::internal(error.to_string()))?;
    let models = models
        .into_iter()
        .filter(|model| !gone.contains(&model.id))
        .collect();
    state.replace_provider_models(provider, models)?;
    crate::commands::knowledge::complete_pending_world_models(state)?;
    state
        .provider_model_cache
        .record(provider, expected_generation.to_owned());
    Ok(())
}

fn cached_provider_models(
    state: &AppState,
    provider: &str,
) -> Result<Option<ModelCatalogView>, CommandError> {
    let agent_data_dir = state.pi_agent_data_dir()?;
    let Some(credential_generation) = provider_credential_generation(&agent_data_dir, provider)?
    else {
        state.provider_model_cache.invalidate(provider);
        return Ok(None);
    };
    if state
        .provider_model_cache
        .is_fresh(provider, &credential_generation)
    {
        return state.model_catalog_view().map(Some);
    }
    Ok(None)
}

fn configured_model(
    provider: &str,
    model: ProviderModelOption,
) -> Result<ConfiguredModel, CommandError> {
    let configured = ConfiguredModel {
        id: format!("{provider}/{}", model.id),
        name: model.name,
        provider: provider.to_owned(),
        model: model.id,
        input: model.input,
        reasoning: model.reasoning,
        context_window: model.context_window,
        max_tokens: model.max_tokens,
        thinking_levels: model.thinking_levels,
        thinking_level_map: model.thinking_level_map,
        run_controls: model.run_controls,
    };
    configured.validate()?;
    Ok(configured)
}

fn validate_provider(provider: &str) -> Result<(), CommandError> {
    if provider_options()
        .iter()
        .any(|option| option.id == provider)
    {
        Ok(())
    } else {
        Err(CommandError::invalid("unsupported Pi model provider"))
    }
}

/// A built-in provider, or a custom provider the user has added.
fn validate_connectable(state: &AppState, provider: &str) -> Result<(), CommandError> {
    if validate_provider(provider).is_ok()
        || (custom::is_custom_id(provider)
            && custom::find(&state.pi_agent_data_dir()?, provider)?.is_some())
    {
        Ok(())
    } else {
        Err(CommandError::invalid("unsupported Pi model provider"))
    }
}

fn validate_api_key(api_key: &str) -> Result<(), CommandError> {
    if api_key.is_empty()
        || api_key.len() > MAX_API_KEY_BYTES
        || api_key.chars().any(char::is_control)
    {
        return Err(CommandError::invalid("API key is invalid"));
    }
    Ok(())
}

async fn run_support_command(
    state: &AppState,
    command: String,
    oauth: Option<&OAuthSession<'_>>,
    provider_credential: Option<(String, String)>,
    mutation_api_key: Option<String>,
    admission: &ProviderSupportLease,
    answers: Option<tokio::sync::mpsc::UnboundedReceiver<crate::pi::support::UiAnswer>>,
) -> Result<ProviderResult, CommandError> {
    if admission.is_cancelled() {
        return Err(provider_operation_cancelled());
    }
    let pi = state
        .artifacts
        .pi
        .as_ref()
        .ok_or_else(|| CommandError::unavailable("pi"))?;
    let files = create_support_files(&state.artifacts.app_data_dir)?;
    let launch = PiSupportLaunchConfig {
        executable: pi.executable.clone(),
        runtime_dir: pi.runtime_dir.clone(),
        extension: pi.provider_extension.clone(),
        agent_data_dir: state.pi_agent_data_dir()?,
        private_working_dir: files.directory.clone(),
        lease_dir: state.artifacts.process_lease_dir.clone(),
        result_file: files.result.clone(),
        request_file: None,
        provider_credential,
        mutation_api_key,
    };
    let outcome = pi_support::run(
        launch,
        &command,
        if oauth.is_some() {
            OAUTH_TIMEOUT
        } else {
            SUPPORT_TIMEOUT
        },
        admission.cancelled(),
        |payload| {
            if let Some(request) = manual_code_request(payload) {
                let session = oauth.ok_or_else(|| {
                    CommandError::internal("unexpected sign-in prompt outside a sign-in")
                })?;
                return accept_manual_code_request(session, admission, request);
            }
            if let Some(event) = provider_event(payload)? {
                handle_provider_event(event, oauth, admission)?;
            }
            Ok(())
        },
        || {
            pi_support::result_ready(&files.result, None)
                .map_err(|_| CommandError::internal("Pi provider result is invalid"))
        },
        answers,
    )
    .await;
    if outcome.shutdown.is_err() {
        admission.mark_shutdown_failed();
    }
    outcome.result.map_err(|error| match error {
        SupportFailure::Operation(error) => error,
        SupportFailure::Launch(error) | SupportFailure::Dispatch(error) => {
            CommandError::internal(error.to_string())
        }
        SupportFailure::CancelledBeforeDispatch | SupportFailure::Cancelled => {
            provider_operation_cancelled()
        }
        SupportFailure::Timeout => CommandError::internal("Pi provider setup timed out"),
        _ => CommandError::internal("Pi provider setup failed"),
    })?;
    outcome
        .shutdown
        .map_err(|error| CommandError::internal(error.to_string()))?;
    if admission.is_cancelled() {
        return Err(provider_operation_cancelled());
    }
    read_support_result(&files.result)
}

/// Pi's `manual_code` prompt, raised by the extension as `ctx.ui.input`.
fn manual_code_request(payload: &Value) -> Option<String> {
    (payload.get("type").and_then(Value::as_str) == Some("extension_ui_request")
        && payload.get("method").and_then(Value::as_str) == Some("input")
        && payload.get("title").and_then(Value::as_str) == Some(MANUAL_CODE_TITLE))
    .then(|| payload.get("id").and_then(Value::as_str).map(str::to_owned))
    .flatten()
    .filter(|id| !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control))
}

fn accept_manual_code_request(
    session: &OAuthSession<'_>,
    admission: &ProviderSupportLease,
    request_id: String,
) -> Result<(), CommandError> {
    // A flow without a paste rule (a device code) waits for its own completion.
    let Some(rule) = PasteRule::for_provider(session.provider) else {
        return Ok(());
    };
    admission.set_manual_entry(ManualEntry {
        request_id,
        rule,
        responder: session.answers.clone(),
    })?;
    send_event(session.events, ProviderConnectionEvent::ManualEntry)
}

fn provider_operation_cancelled() -> CommandError {
    CommandError::new("cancelled", "Provider operation was cancelled.")
}

fn create_support_files(app_data_dir: &Path) -> Result<SupportFiles, CommandError> {
    let root = app_data_dir.join("provider-support");
    ensure_private_directory(&root).map_err(|error| CommandError::internal(error.to_string()))?;
    let directory = root.join(Uuid::new_v4().simple().to_string());
    create_private_directory(&directory)
        .map_err(|error| CommandError::internal(error.to_string()))?;
    let result = directory.join("result.json");
    ensure_private_regular_file(&result)
        .map_err(|error| CommandError::internal(error.to_string()))?;
    Ok(SupportFiles {
        directory,
        result,
        request: None,
    })
}

pub(crate) fn create_search_support_files(
    app_data_dir: &Path,
) -> Result<SupportFiles, CommandError> {
    let mut files = create_support_files(app_data_dir)?;
    let request = files.directory.join("request.json");
    ensure_private_regular_file(&request)
        .map_err(|error| CommandError::internal(error.to_string()))?;
    files.request = Some(request);
    Ok(files)
}

fn read_support_result(path: &Path) -> Result<ProviderResult, CommandError> {
    ensure_private_regular_file(path).map_err(|error| CommandError::internal(error.to_string()))?;
    let encoded =
        read_private_regular_file_bounded(path, MAX_RESULT_BYTES).map_err(|error| match error {
            ArtifactTrustError::Oversized => {
                CommandError::internal("Pi provider result is too large")
            }
            ArtifactTrustError::Untrusted => {
                CommandError::internal("Pi provider result is invalid")
            }
            ArtifactTrustError::Io(error) => CommandError::internal(error.to_string()),
        })?;
    let result: ProviderResult = serde_json::from_slice(&encoded)
        .map_err(|_| CommandError::internal("Pi provider result is invalid"))?;
    if result.protocol != PROVIDER_PROTOCOL {
        return Err(CommandError::internal("Pi provider protocol is invalid"));
    }
    Ok(result)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeSearchFileResult {
    protocol: String,
    #[serde(rename = "type")]
    result_type: String,
    provider: String,
    status: String,
    #[serde(default)]
    sources: Vec<NativeSearchSource>,
    model: Option<String>,
    #[serde(rename = "requestId")]
    _request_id: Option<String>,
    usage: Option<NativeSearchUsage>,
    #[serde(rename = "searchRequestCount")]
    search_request_count: Option<u32>,
    error_kind: Option<String>,
    retry_after_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeSearchSource {
    title: String,
    url: String,
    snippet: Option<String>,
    #[serde(rename = "publishedAt")]
    published_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeSearchUsage {
    #[serde(rename = "inputTokens")]
    _input_tokens: Option<u32>,
    #[serde(rename = "outputTokens")]
    _output_tokens: Option<u32>,
    #[serde(rename = "totalTokens")]
    _total_tokens: Option<u32>,
    #[serde(rename = "searchRequests")]
    search_requests: Option<u32>,
}

fn write_search_request(
    path: &Path,
    provider: SearchProviderId,
    query: &WebSearchQuery,
) -> Result<(), WebError> {
    let mut body = serde_json::json!({
        "protocol": PROVIDER_PROTOCOL,
        "type": "search",
        "provider": provider.as_str(),
        "query": query.query,
        "limit": query.limit,
        "include_domains": query.include_domains,
        "exclude_domains": query.exclude_domains,
    });
    if let Some(recency) = query.recency {
        body["recency"] = Value::String(
            match recency {
                crate::web::WebRecency::Day => "day",
                crate::web::WebRecency::Week => "week",
                crate::web::WebRecency::Month => "month",
                crate::web::WebRecency::Year => "year",
            }
            .into(),
        );
    }
    let encoded = serde_json::to_vec(&body).map_err(|_| WebError::InvalidProviderResponse)?;
    if encoded.len() as u64 > MAX_RESULT_BYTES {
        return Err(WebError::InvalidProviderResponse);
    }
    std::fs::write(path, encoded).map_err(|_| WebError::Provider)?;
    Ok(())
}

fn read_native_search_result(path: &Path) -> Result<NativeSearchFileResult, WebError> {
    let encoded = read_native_search_result_bytes(path)?;
    let result: NativeSearchFileResult = serde_json::from_slice(&encoded).map_err(|error| {
        WebError::ProviderMessage(format!(
            "web provider result {}",
            native_search_schema_failure(&encoded, &error)
        ))
    })?;
    if result.protocol != PROVIDER_PROTOCOL || result.result_type != "search" {
        return Err(WebError::ProviderMessage(
            "web provider result used an unsupported protocol".into(),
        ));
    }
    Ok(result)
}

fn read_native_search_result_bytes(path: &Path) -> Result<Vec<u8>, WebError> {
    read_private_regular_file_bounded(path, MAX_RESULT_BYTES).map_err(|error| {
        WebError::ProviderMessage(
            match error {
                ArtifactTrustError::Oversized => {
                    "web provider result exceeded its protocol size limit"
                }
                ArtifactTrustError::Untrusted => "web provider result file was invalid",
                ArtifactTrustError::Io(_) => "web provider result file was unavailable",
            }
            .into(),
        )
    })
}

fn native_search_result_to_hits(
    provider: SearchProviderId,
    result: NativeSearchFileResult,
) -> Result<SearchHits, WebError> {
    if result.protocol != PROVIDER_PROTOCOL || result.result_type != "search" {
        return Err(WebError::InvalidProviderResponse);
    }
    if result.provider != provider.as_str() {
        return Err(WebError::ProviderMessage(
            "web provider result named a different provider".into(),
        ));
    }
    if result.status != "ok" {
        return Err(native_search_error(&result));
    }
    let search_request_count = result.search_request_count.or(result
        .usage
        .as_ref()
        .and_then(|usage| usage.search_requests));
    let sources = result
        .sources
        .into_iter()
        .map(|source| {
            crate::web::web_source(
                if source.title.trim().is_empty() {
                    "Untitled source".into()
                } else {
                    source.title
                },
                source.url,
                source.snippet.unwrap_or_default(),
                source.published_at,
            )
        })
        .collect::<Vec<WebSource>>();
    Ok(SearchHits {
        sources,
        model: result.model,
        search_request_count,
    })
}

fn native_search_schema_failure(encoded: &[u8], error: &serde_json::Error) -> &'static str {
    if encoded.is_empty() {
        return "file was empty";
    }
    if error.is_eof() {
        return "JSON ended before the result was complete";
    }
    if std::str::from_utf8(encoded).is_err() {
        return "was not UTF-8 protocol JSON";
    }
    let Ok(Value::Object(root)) = serde_json::from_slice::<Value>(encoded) else {
        return "was not valid protocol JSON";
    };
    let root_fields = [
        "protocol",
        "type",
        "provider",
        "status",
        "sources",
        "model",
        "requestId",
        "usage",
        "searchRequestCount",
        "error_kind",
        "retry_after_ms",
    ];
    if root.keys().any(|key| !root_fields.contains(&key.as_str())) {
        return "contained an unexpected top-level field";
    }
    if root
        .get("sources")
        .and_then(Value::as_array)
        .is_some_and(|sources| {
            sources.iter().any(|source| {
                source.as_object().is_some_and(|source| {
                    source.keys().any(|key| {
                        !["title", "url", "snippet", "publishedAt"].contains(&key.as_str())
                    })
                })
            })
        })
    {
        return "contained an unexpected source field";
    }
    if root
        .get("usage")
        .and_then(Value::as_object)
        .is_some_and(|usage| {
            usage.keys().any(|key| {
                ![
                    "inputTokens",
                    "outputTokens",
                    "totalTokens",
                    "searchRequests",
                ]
                .contains(&key.as_str())
            })
        })
    {
        return "contained an unexpected usage field";
    }
    let detail = error.to_string();
    if detail.contains("missing field") {
        "was missing a required protocol field"
    } else if detail.contains("invalid type") {
        "contained an invalid protocol field type"
    } else if detail.contains("invalid value") {
        "contained an invalid protocol field value"
    } else {
        "did not match its protocol schema"
    }
}

fn native_search_error(result: &NativeSearchFileResult) -> WebError {
    match result.error_kind.as_deref() {
        Some("unavailable") => WebError::ProviderUnavailable,
        Some("rate_limited") => WebError::RateLimited {
            retry_after: result.retry_after_ms.map(Duration::from_millis),
        },
        Some("timeout") => WebError::Timeout,
        Some("cancelled") => WebError::Cancelled,
        Some("malformed") => {
            WebError::ProviderMessage("web provider returned malformed search data".into())
        }
        Some("no_search_tool") => {
            WebError::ProviderMessage("web provider completed without running hosted search".into())
        }
        Some("transport") => WebError::Transport,
        _ => WebError::Provider,
    }
}

fn search_support_result_ready(result: &Path) -> Result<bool, WebError> {
    let encoded = read_native_search_result_bytes(result)?;
    if encoded.is_empty() {
        return Ok(false);
    }
    let Ok(result) = serde_json::from_slice::<NativeSearchFileResult>(&encoded) else {
        return Ok(false);
    };
    Ok(result.protocol == PROVIDER_PROTOCOL && result.result_type == "search")
}

pub async fn run_native_web_search(
    state: &AppState,
    provider: SearchProviderId,
    query: &WebSearchQuery,
    remaining: Duration,
    cancel: &SearchCancel,
) -> Result<SearchHits, WebError> {
    if !provider.is_native() {
        return Err(WebError::ProviderUnavailable);
    }
    if cancel.is_cancelled() {
        return Err(WebError::Cancelled);
    }
    let _admission = state
        .provider_support
        .try_acquire()
        .map_err(|_| WebError::ProviderUnavailable)?;
    let pi = state
        .artifacts
        .pi
        .as_ref()
        .ok_or(WebError::ProviderUnavailable)?;
    let files = create_search_support_files(&state.artifacts.app_data_dir)
        .map_err(|_| WebError::Provider)?;
    let request = files.request.as_ref().ok_or(WebError::Provider)?;
    write_search_request(request, provider, query)?;
    let launch = PiSupportLaunchConfig {
        executable: pi.executable.clone(),
        runtime_dir: pi.runtime_dir.clone(),
        extension: pi.provider_extension.clone(),
        agent_data_dir: state.pi_agent_data_dir().map_err(|_| WebError::Provider)?,
        private_working_dir: files.directory.clone(),
        lease_dir: state.artifacts.process_lease_dir.clone(),
        result_file: files.result.clone(),
        request_file: files.request.clone(),
        provider_credential: None,
        mutation_api_key: None,
    };
    let outcome = pi_support::run(
        launch,
        "/loupist-provider-search",
        remaining,
        cancel.cancelled(),
        |_| Ok::<_, WebError>(()),
        || search_support_result_ready(&files.result),
        None,
    )
    .await;
    if outcome.shutdown.is_err() {
        _admission.mark_shutdown_failed();
    }
    outcome.result.map_err(|error| match error {
        SupportFailure::CancelledBeforeDispatch | SupportFailure::Cancelled => WebError::Cancelled,
        SupportFailure::Timeout => WebError::Timeout,
        SupportFailure::ExtensionFailed => WebError::ProviderMessage(
            "web search extension failed before producing a result".into(),
        ),
        SupportFailure::Operation(error) => error,
        _ => WebError::Provider,
    })?;
    outcome.shutdown.map_err(|_| WebError::Provider)?;
    native_search_result_to_hits(provider, read_native_search_result(&files.result)?)
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ExtensionProviderEvent {
    AuthorizationUrl {
        url: String,
        #[serde(default)]
        instructions: Option<String>,
    },
    DeviceCode {
        user_code: String,
        verification_uri: String,
        #[serde(default)]
        expires_in_s: Option<u64>,
    },
    Connected {
        message: String,
    },
    Progress {
        message: String,
    },
    Failed {
        reason: String,
    },
}

fn provider_event(payload: &Value) -> Result<Option<ExtensionProviderEvent>, CommandError> {
    if payload.get("type").and_then(Value::as_str) != Some("extension_ui_request")
        || payload.get("method").and_then(Value::as_str) != Some("notify")
    {
        return Ok(None);
    }
    let Some(message) = payload.get("message").and_then(Value::as_str) else {
        return Ok(None);
    };
    let Some(encoded) = message.strip_prefix(&format!("{PROVIDER_PROTOCOL}:")) else {
        return Ok(None);
    };
    serde_json::from_str(encoded)
        .map(Some)
        .map_err(|_| CommandError::internal("Pi provider event is invalid"))
}

fn valid_user_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= MAX_USER_CODE_CHARS
        && code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Records the sign-in page on the lease and opens it, exactly as Pi wrote it
/// (Pi names Loupist on OpenAI's page from `agentName`). A browser that cannot
/// open does not end the sign-in: the renderer offers the link to copy.
fn present_sign_in_page(admission: &ProviderSupportLease, url: &str) -> Result<bool, CommandError> {
    validate_authorization_url(url)?;
    admission.set_oauth_authorization_url(url.to_owned())?;
    // Unit tests exercise the event path without launching a browser.
    Ok(cfg!(test) || crate::web::external_browser::open(url).is_ok())
}

fn handle_provider_event(
    event: ExtensionProviderEvent,
    oauth: Option<&OAuthSession<'_>>,
    admission: &ProviderSupportLease,
) -> Result<(), CommandError> {
    match event {
        ExtensionProviderEvent::AuthorizationUrl { url, instructions } => {
            let _ = instructions;
            let session = oauth
                .ok_or_else(|| CommandError::internal("unexpected OAuth authorization event"))?;
            let opened = present_sign_in_page(admission, &url)?;
            send_event(
                session.events,
                ProviderConnectionEvent::OpeningBrowser {
                    message: "Continue in your browser to finish signing in.".into(),
                    opened,
                    url: (!opened).then_some(url),
                },
            )
        }
        ExtensionProviderEvent::DeviceCode {
            user_code,
            verification_uri,
            expires_in_s,
        } => {
            let session =
                oauth.ok_or_else(|| CommandError::internal("unexpected device sign-in event"))?;
            if !valid_user_code(&user_code) {
                return Err(CommandError::internal("Pi returned an invalid device code"));
            }
            present_sign_in_page(admission, &verification_uri)?;
            send_event(
                session.events,
                ProviderConnectionEvent::DeviceCode {
                    user_code,
                    verification_uri,
                    expires_in_s: expires_in_s.filter(|seconds| *seconds <= 24 * 60 * 60),
                },
            )
        }
        ExtensionProviderEvent::Failed { reason } => {
            if let Some(session) = oauth {
                *session
                    .failure
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                    Some(ConnectFailure::from_extension(&reason));
            }
            Ok(())
        }
        ExtensionProviderEvent::Progress { message } => {
            if let Some(session) = oauth {
                send_event(session.events, ProviderConnectionEvent::Waiting { message })?;
            }
            Ok(())
        }
        ExtensionProviderEvent::Connected { message } => {
            if let Some(session) = oauth {
                send_event(
                    session.events,
                    ProviderConnectionEvent::Connected { message },
                )?;
            }
            Ok(())
        }
    }
}

fn open_authorization_url(url: &str) -> Result<(), CommandError> {
    validate_authorization_url(url)?;
    crate::web::external_browser::open(url).map_err(|error| {
        CommandError::internal(format!(
            "Provider sign-in could not open the trusted authorization page: {error}"
        ))
    })
}

fn untrusted_authorization_url() -> CommandError {
    CommandError::internal("Pi returned an untrusted authorization URL")
}

fn validate_authorization_url(url: &str) -> Result<(), CommandError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| CommandError::internal("Pi returned an invalid authorization URL"))?;
    let host = parsed.host_str().ok_or_else(untrusted_authorization_url)?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.fragment().is_some()
        || !catalog_allows_authorization(host, parsed.path())
    {
        return Err(untrusted_authorization_url());
    }
    Ok(())
}

fn send_event(
    channel: &Channel<ProviderConnectionEvent>,
    event: ProviderConnectionEvent,
) -> Result<(), CommandError> {
    channel
        .send(event)
        .map_err(|_| CommandError::internal("provider setup event delivery failed"))
}

/// Discover through Pi's existing provider extension; the harness never opens auth files.
#[cfg(feature = "eval-harness")]
pub(crate) async fn eval_models(
    state: &AppState,
    provider: &str,
) -> Result<Vec<ConfiguredModel>, CommandError> {
    validate_provider(provider)?;
    let admission = state.provider_support.try_acquire_discovery()?;
    let result = run_support_command(
        state,
        format!("/loupist-provider-models {provider}"),
        None,
        None,
        None,
        &admission,
        None,
    )
    .await?;
    if result.result_type != "models" || result.provider != provider {
        return Err(CommandError::internal(
            "Pi returned the wrong provider catalog",
        ));
    }
    result
        .models
        .into_iter()
        .map(|model| configured_model(provider, model))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_urls_are_locked_to_cataloged_oauth_hosts() {
        for allowed in [
            "https://auth.openai.com/api/accounts/authorize?client_id=dynamic_agent_client",
            "https://claude.ai/oauth/authorize?client_id=test",
            "https://accounts.x.ai/oauth2/device?user_code=ABCD",
        ] {
            assert!(validate_authorization_url(allowed).is_ok(), "{allowed}");
        }
        for denied in [
            "https://example.com/oauth/authorize",
            "http://auth.openai.com/api/accounts/authorize",
            "https://openrouter.ai/auth?callback_url=http://127.0.0.1/cb",
            "https://auth.openai.com/other",
            "https://user@auth.openai.com/oauth/authorize",
            "https://auth.openai.com:444/oauth/authorize",
            "https://accounts.x.ai.evil.example/oauth2/device",
        ] {
            assert!(validate_authorization_url(denied).is_err(), "{denied}");
        }
    }

    #[tokio::test]
    async fn chatgpt_sign_in_page_is_stored_exactly_as_pi_wrote_it() {
        let coordinator = crate::providers::support::ProviderSupportCoordinator::default();
        let admission = coordinator.acquire_oauth().await.unwrap();
        let (channel, _events) = capture_channel();
        let (answers, _pasted) = tokio::sync::mpsc::unbounded_channel();
        let session = OAuthSession {
            provider: "openai",
            events: &channel,
            answers,
            failure: std::sync::Mutex::new(None),
        };
        let original = "https://auth.openai.com/api/accounts/authorize?client_id=dynamic_agent_client&agent_name_hint=Loupist&ext_agent_host_id=urn%3Auuid%3Aabc&state=state-value";
        let event = provider_event(&notify(serde_json::json!({
            "type": "authorization_url",
            "url": original,
            "instructions": "Complete sign-in in your browser.",
        })))
        .unwrap()
        .unwrap();
        handle_provider_event(event, Some(&session), &admission).unwrap();
        let stored = coordinator.oauth_authorization_url().unwrap();
        assert_eq!(stored, original);
        assert!(validate_authorization_url(&stored).is_ok(), "{stored}");
    }

    fn capture_channel() -> (
        Channel<ProviderConnectionEvent>,
        std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
    ) {
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        let channel = Channel::new(move |body| {
            sink.lock()
                .unwrap()
                .push(body.deserialize::<Value>().unwrap());
            Ok(())
        });
        (channel, events)
    }

    fn notify(event: Value) -> Value {
        serde_json::json!({
            "type": "extension_ui_request",
            "id": "notice",
            "method": "notify",
            "message": format!("{PROVIDER_PROTOCOL}:{event}"),
        })
    }

    #[tokio::test]
    async fn sign_in_events_reach_the_renderer_typed_and_the_paste_prompt_reaches_the_lease() {
        let coordinator = crate::providers::support::ProviderSupportCoordinator::default();
        let admission = coordinator.acquire_oauth().await.unwrap();
        let (channel, events) = capture_channel();
        let (answers, mut pasted) = tokio::sync::mpsc::unbounded_channel();
        let session = OAuthSession {
            provider: "anthropic",
            events: &channel,
            answers,
            failure: std::sync::Mutex::new(None),
        };

        // Pi's paste prompt: only the extension's titled input request is accepted.
        let prompt = serde_json::json!({
            "type": "extension_ui_request", "id": "prompt-1", "method": "input",
            "title": MANUAL_CODE_TITLE, "placeholder": "",
        });
        let request = manual_code_request(&prompt).unwrap();
        accept_manual_code_request(&session, &admission, request).unwrap();
        let mut other = prompt.clone();
        other["title"] = Value::String("Anything else".into());
        assert_eq!(manual_code_request(&other), None);
        // Claude's browser shows its own callback address; Pi gets code#state.
        coordinator
            .submit_oauth_code("http://localhost:53692/callback?code=code&state=state")
            .unwrap();
        assert_eq!(
            pasted.recv().await,
            Some(("prompt-1".to_owned(), "code#state".to_owned()))
        );

        // A device code is shown with its page; a malformed code is refused.
        let device = provider_event(&notify(serde_json::json!({
            "type": "device_code", "user_code": "WDJB-MJHT",
            "verification_uri": "https://accounts.x.ai/oauth2/device?user_code=WDJB-MJHT",
            "expires_in_s": 900,
        })))
        .unwrap()
        .unwrap();
        handle_provider_event(device, Some(&session), &admission).unwrap();
        assert_eq!(
            coordinator.oauth_authorization_url().as_deref(),
            Some("https://accounts.x.ai/oauth2/device?user_code=WDJB-MJHT")
        );
        let forged = provider_event(&notify(serde_json::json!({
            "type": "device_code", "user_code": "<script>",
            "verification_uri": "https://accounts.x.ai/oauth2/device",
        })))
        .unwrap()
        .unwrap();
        assert!(handle_provider_event(forged, Some(&session), &admission).is_err());
        let elsewhere = provider_event(&notify(serde_json::json!({
            "type": "device_code", "user_code": "ABCD",
            "verification_uri": "https://evil.example/device",
        })))
        .unwrap()
        .unwrap();
        assert!(handle_provider_event(elsewhere, Some(&session), &admission).is_err());

        // A failure reason is held for the command result; unknown text reads as other.
        for (reason, expected) in [
            (
                "plan_permission_declined",
                ConnectFailure::PlanPermissionDeclined,
            ),
            ("some provider text", ConnectFailure::Other),
        ] {
            let failed = provider_event(&notify(
                serde_json::json!({"type": "failed", "reason": reason}),
            ))
            .unwrap()
            .unwrap();
            handle_provider_event(failed, Some(&session), &admission).unwrap();
            assert_eq!(*session.failure.lock().unwrap(), Some(expected));
        }
        assert_eq!(
            connect_failed(ConnectFailure::PortInUse).code,
            "provider_connect_failed"
        );
        assert_eq!(
            connect_failed(ConnectFailure::PortInUse).message,
            "port_in_use"
        );

        let seen = events.lock().unwrap().clone();
        assert_eq!(seen[0], serde_json::json!({"type": "manual_entry"}));
        assert_eq!(
            seen[1],
            serde_json::json!({
                "type": "device_code", "user_code": "WDJB-MJHT",
                "verification_uri": "https://accounts.x.ai/oauth2/device?user_code=WDJB-MJHT",
                "expires_in_s": 900,
            })
        );
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn a_saved_endpoint_key_never_follows_the_endpoint_to_a_new_address() {
        let old = "https://models.example.com/v1";
        let new = "https://other.example.net/v1";
        // Same address, no new key: the saved key stays.
        assert_eq!(custom_key_to_store(None, Some(old), old, true), None);
        // A new address without a key drops the saved one.
        assert_eq!(
            custom_key_to_store(None, Some(old), new, true),
            Some(custom::NO_KEY)
        );
        // A key typed with the new address is stored for it.
        assert_eq!(
            custom_key_to_store(Some("typed"), Some(old), new, true),
            Some("typed")
        );
        // A new endpoint or one with nothing saved gets the keyless placeholder.
        assert_eq!(
            custom_key_to_store(None, None, new, false),
            Some(custom::NO_KEY)
        );
        assert_eq!(
            custom_key_to_store(None, Some(old), old, false),
            Some(custom::NO_KEY)
        );
    }

    #[test]
    fn fixed_pages_are_chosen_by_provider_never_by_the_renderer() {
        assert_eq!(
            usage_url("openai"),
            Some("https://chatgpt.com/settings/usage")
        );
        assert_eq!(
            usage_url("anthropic"),
            Some("https://claude.ai/settings/usage")
        );
        assert_eq!(usage_url("groq"), None);
        assert_eq!(
            console_url("openai"),
            Some("https://platform.openai.com/api-keys")
        );
        assert_eq!(
            console_url("upstage"),
            Some("https://console.upstage.ai/api-keys")
        );
        assert_eq!(console_url("https://evil.example"), None);
    }

    #[test]
    fn provider_mutation_inputs_and_results_never_accept_secret_payloads() {
        assert!(validate_api_key("valid-api-key").is_ok());
        assert!(validate_api_key("").is_err());
        assert!(validate_api_key("line\nbreak").is_err());
        assert!(validate_api_key(&"x".repeat(MAX_API_KEY_BYTES + 1)).is_err());

        let leaked_result = serde_json::json!({
            "protocol": PROVIDER_PROTOCOL,
            "type": "credential_updated",
            "provider": "anthropic",
            "models": [],
            "credential": { "type": "api_key", "key": "must-not-cross-result-boundary" }
        });
        assert!(serde_json::from_value::<ProviderResult>(leaked_result).is_err());
    }

    #[test]
    fn provider_model_cache_is_lru_bounded_expiring_and_authority_scoped() {
        let cache = ProviderModelDiscoveryCache::with_limits(Duration::from_secs(60), 2);
        let now = Instant::now();
        cache.record_at("anthropic", "authority-a".into(), now);
        cache.record_at("openai", "authority-b".into(), now);
        assert!(cache.is_fresh_at("anthropic", "authority-a", now));

        cache.record_at("google", "authority-c".into(), now);
        assert!(cache.is_fresh_at("anthropic", "authority-a", now));
        assert!(!cache.is_fresh_at("openai", "authority-b", now));
        assert!(!cache.is_fresh_at("google", "authority-c", now + Duration::from_secs(60)));

        // A rotated credential is a miss and evicts the old authority.
        assert!(!cache.is_fresh_at("anthropic", "rotated", now));
        assert!(!cache.is_fresh_at("anthropic", "authority-a", now));
    }

    fn discovery_model(name: &str) -> ConfiguredModel {
        ConfiguredModel {
            id: "xai/test-model".into(),
            name: name.into(),
            provider: "xai".into(),
            model: "test-model".into(),
            input: vec!["text".into()],
            reasoning: false,
            context_window: 128_000,
            max_tokens: 4_096,
            thinking_levels: vec!["off".into()],
            thinking_level_map: Default::default(),
            run_controls: Vec::new(),
        }
    }

    #[test]
    fn model_discovery_does_not_publish_after_credentials_change_or_disconnect() {
        let directory = tempfile::tempdir().unwrap();
        let state = AppState::for_test(directory.path().join("app"));
        state
            .replace_provider_models("xai", vec![discovery_model("Existing")])
            .unwrap();
        let existing = state.model_catalog().unwrap();
        let admission = state.provider_support.try_acquire_discovery().unwrap();
        for generation in [Some("changed"), None] {
            let error = publish_provider_models(
                &state,
                "xai",
                vec![discovery_model("Stale")],
                "original",
                generation,
                &admission,
            )
            .unwrap_err();
            assert_eq!(error.code, "conflict");
            assert_eq!(state.model_catalog().unwrap(), existing);
            assert!(!state.provider_model_cache.is_fresh("xai", "original"));
        }
        publish_provider_models(
            &state,
            "xai",
            vec![discovery_model("Fresh")],
            "original",
            Some("original"),
            &admission,
        )
        .unwrap();
        assert_eq!(
            state
                .model_catalog()
                .unwrap()
                .resolve("xai/test-model")
                .unwrap(),
            discovery_model("Fresh")
        );
        assert!(state.provider_model_cache.is_fresh("xai", "original"));
    }

    #[test]
    fn a_gone_model_stays_out_of_model_refreshes_until_the_provider_reconnects() {
        let directory = tempfile::tempdir().unwrap();
        let state = AppState::for_test(directory.path().join("app"));
        let gone = ConfiguredModel {
            id: "xai/retired".into(),
            model: "retired".into(),
            ..discovery_model("Retired")
        };
        state
            .replace_provider_models("xai", vec![discovery_model("Kept"), gone.clone()])
            .unwrap();
        crate::providers::auto::prune_gone(&state, "xai/retired");
        let admission = state.provider_support.try_acquire_discovery().unwrap();
        let refresh = || {
            publish_provider_models(
                &state,
                "xai",
                vec![discovery_model("Kept"), gone.clone()],
                "original",
                Some("original"),
                &admission,
            )
            .unwrap();
            state
                .model_catalog()
                .unwrap()
                .resolve("xai/retired")
                .is_ok()
        };
        // Pi's bundled catalog still lists it; the refresh leaves it out.
        assert!(!refresh());
        assert!(state
            .model_catalog()
            .unwrap()
            .resolve("xai/test-model")
            .is_ok());
        record_status(&state, "xai", Trigger::Connected(CredentialKind::ApiKey));
        assert!(refresh());
    }

    #[tokio::test]
    async fn preempted_model_discovery_cannot_publish_or_populate_the_cache() {
        let directory = tempfile::tempdir().unwrap();
        let state = AppState::for_test(directory.path().join("app"));
        state
            .replace_provider_models("xai", vec![discovery_model("Existing")])
            .unwrap();
        let existing = state.model_catalog().unwrap();
        let admission = state.provider_support.try_acquire_discovery().unwrap();
        let reconnect = state.provider_support.acquire_oauth();
        tokio::pin!(reconnect);
        tokio::select! {
            _ = admission.cancelled() => {},
            result = &mut reconnect => panic!("reconnect admitted before discovery stopped: {result:?}"),
        }
        let error = publish_provider_models(
            &state,
            "xai",
            vec![discovery_model("Stale")],
            "original",
            Some("original"),
            &admission,
        )
        .unwrap_err();
        assert_eq!(error.code, "cancelled");
        assert_eq!(state.model_catalog().unwrap(), existing);
        assert!(!state.provider_model_cache.is_fresh("xai", "original"));
        drop(admission);
        reconnect.await.unwrap();
    }

    #[test]
    fn native_web_search_results_reject_credentials_answers_and_headers() {
        let result = |extra: Value| {
            let mut body = serde_json::json!({
                "protocol": PROVIDER_PROTOCOL,
                "type": "search",
                "provider": "xai",
                "status": "ok",
                "sources": [{"title": "Report", "url": "https://example.com/report", "snippet": "A public snippet"}],
            });
            body.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            serde_json::from_value::<NativeSearchFileResult>(body)
        };
        assert!(result(serde_json::json!({})).is_ok());
        for leaked in [
            serde_json::json!({"credential": {"key": "secret"}}),
            serde_json::json!({"answer": "provider synthesized answer"}),
            serde_json::json!({"authorization": "Bearer secret"}),
        ] {
            assert!(result(leaked).is_err());
        }
    }

    #[test]
    fn native_search_requires_a_complete_json_result_within_the_size_limit() {
        let root = tempfile::tempdir().unwrap();
        let result = root.path().join("result.json");
        for incomplete in [&b""[..], br#"{"protocol":"loupist-provider/1""#, b"{}"] {
            std::fs::write(&result, incomplete).unwrap();
            assert!(!search_support_result_ready(&result).unwrap());
        }

        let mut at_limit = serde_json::to_vec(&serde_json::json!({
            "protocol": PROVIDER_PROTOCOL, "type": "search", "provider": "openai-codex", "status": "ok", "sources": []
        }))
        .unwrap();
        at_limit.resize(MAX_RESULT_BYTES as usize, b' ');
        std::fs::write(&result, at_limit).unwrap();
        assert!(search_support_result_ready(&result).unwrap());
        assert_eq!(
            read_native_search_result(&result).unwrap().provider,
            "openai-codex"
        );

        std::fs::File::create(&result)
            .unwrap()
            .set_len(MAX_RESULT_BYTES + 1)
            .unwrap();
        assert!(search_support_result_ready(&result).is_err());
        assert!(read_native_search_result(&result).is_err());
        assert!(read_support_result(&result).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn provider_result_readers_reject_symlinked_files() {
        let root = tempfile::tempdir().unwrap();
        let result = root.path().join("result.json");
        let linked = root.path().join("linked.json");
        std::fs::write(&result, b"{}").unwrap();
        std::os::unix::fs::symlink(&result, &linked).unwrap();
        assert!(read_native_search_result_bytes(&linked).is_err());
        assert!(read_support_result(&linked).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn provider_result_readers_reject_a_fifo_without_blocking() {
        use std::os::unix::ffi::OsStrExt;

        let root = tempfile::tempdir().unwrap();
        let result = root.path().join("result.json");
        let name = std::ffi::CString::new(result.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);

        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let native_rejected = read_native_search_result(&result).is_err();
            let support_rejected = read_support_result(&result).is_err();
            let _ = sender.send((native_rejected, support_rejected));
        });
        let (native_rejected, support_rejected) = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("provider result readers must not wait for a FIFO writer");
        assert!(native_rejected && support_rejected);
    }
}
