//! Host-owned isolated inference. Pi owns provider authentication/refresh; no
//! credential is returned to a graph worker, renderer, or inference result.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crate::{
    app::{AppArtifacts, AppState, CommandError},
    pi::provider_failure::ProviderFailure,
    pi::support::{self as pi_support, SupportFailure},
    pi::PiSupportLaunchConfig,
    providers::catalog::{
        provider_credential_from_environment, provider_credential_kind, provider_options,
        ConfiguredModel,
    },
    providers::connection::create_search_support_files,
    providers::status::BackgroundHold,
    runs::maintenance::{ActivityLease, MaintenanceBlocker, MaintenanceCoordinator},
    store::SqliteStore,
};

const PROTOCOL: &str = "loupist-provider/1";
const MAX_REQUEST_BYTES: usize = 512 * 1024;
const MAX_RESULT_BYTES: u64 = 512 * 1024;

/// Pinned Pi providers whose model adapters support our fetch guard
/// (`isolated_inference` in the provider manifest). The Pi process checks the
/// selected model's actual API again before dispatch.
pub fn supports_isolated_inference(provider: &str) -> bool {
    crate::providers::manifest::spec(provider).is_some_and(|spec| spec.isolated_inference)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderInferenceMessage {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderInferenceRequest {
    pub provider: String,
    pub model: String,
    pub messages: Vec<ProviderInferenceMessage>,
    pub schema: Value,
    pub max_output_tokens: u32,
    pub timeout_ms: u64,
    pub thinking_level: String,
    pub run_options: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderInferenceDisposition {
    Completed,
    NotSent,
    Uncertain,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderInferenceReply {
    pub disposition: ProviderInferenceDisposition,
    pub body: Value,
    pub usage: Option<Value>,
}

impl ProviderInferenceReply {
    fn failure(disposition: ProviderInferenceDisposition, message: &'static str) -> Self {
        Self {
            disposition,
            body: json!({"status": disposition, "message": message}),
            usage: None,
        }
    }
    fn not_sent(message: &'static str) -> Self {
        Self::failure(ProviderInferenceDisposition::NotSent, message)
    }
    fn uncertain() -> Self {
        Self::failure(
            ProviderInferenceDisposition::Uncertain,
            "The model call did not return a confirmed result. Automatic retry is disabled.",
        )
    }
}

/// Why a provider cannot run World processing now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum UnreadyReason {
    IsolationUnsupported,
    RuntimeMissing,
    ChooseConnectedModel,
    NotConnected,
    AccountWaiting,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInferenceReadiness {
    pub available: bool,
    pub reason: Option<UnreadyReason>,
    pub auth_kind: Option<String>,
    pub provider_label: Option<String>,
    pub model_label: Option<String>,
}

#[derive(Clone)]
pub struct ProviderInferenceService {
    artifacts: Arc<AppArtifacts>,
    agent_data_dir: PathBuf,
    maintenance: MaintenanceCoordinator,
    store: Arc<SqliteStore>,
    #[cfg(feature = "eval-harness")]
    eval_observations: Arc<std::sync::Mutex<Vec<Value>>>,
}

impl ProviderInferenceService {
    pub fn from_parts(
        artifacts: Arc<AppArtifacts>,
        agent_data_dir: PathBuf,
        maintenance: MaintenanceCoordinator,
        store: Arc<SqliteStore>,
    ) -> Self {
        Self {
            artifacts,
            agent_data_dir,
            maintenance,
            store,
            #[cfg(feature = "eval-harness")]
            eval_observations: Default::default(),
        }
    }
    pub fn from_state(state: &AppState) -> Result<Self, CommandError> {
        Ok(Self::from_parts(
            state.artifacts.clone(),
            state.pi_agent_data_dir()?,
            state.maintenance.clone(),
            state.store.clone(),
        ))
    }
    fn admit(&self) -> Result<ActivityLease, CommandError> {
        // Inference must block updates, but it must not occupy the interactive
        // provider connection slot. Pi owns cross-process credential locking.
        self.maintenance.admit_activity(MaintenanceBlocker {
            id: "provider-inference".into(),
            kind: "provider_inference".into(),
            label: "Background model inference".into(),
        })
    }
    fn model(&self, provider: &str, model: &str) -> Result<ConfiguredModel, CommandError> {
        self.store
            .get_model_catalog()
            .map_err(|_| CommandError::internal("Model catalog is unavailable"))?
            .unwrap_or_default()
            .resolve(&format!("{provider}/{model}"))
    }
    pub fn readiness(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<ProviderInferenceReadiness, CommandError> {
        let option = provider_options()
            .into_iter()
            .find(|item| item.id == provider);
        let selected = self.model(provider, model).ok();
        let auth_kind = if option.is_some() {
            provider_credential_kind(&self.agent_data_dir, provider)?
        } else {
            None
        };
        let reason = if !supports_isolated_inference(provider) {
            Some(UnreadyReason::IsolationUnsupported)
        } else if self.artifacts.pi.is_none() {
            Some(UnreadyReason::RuntimeMissing)
        } else if option.is_none() || selected.is_none() {
            Some(UnreadyReason::ChooseConnectedModel)
        } else if auth_kind.is_none() {
            Some(UnreadyReason::NotConnected)
        } else if crate::providers::status::background_hold(
            &self.store,
            provider,
            crate::commands::now_ms(),
        ) == BackgroundHold::Held
        {
            // Plan limit, a plan that cannot be shared, a sign-in to reconnect or
            // an empty balance: background work waits until a request goes
            // through, apart from one probe per back-off window.
            Some(UnreadyReason::AccountWaiting)
        } else {
            None
        };
        Ok(ProviderInferenceReadiness {
            available: reason.is_none(),
            reason,
            auth_kind,
            provider_label: option.map(|item| item.label.into()),
            model_label: selected.map(|item| item.name),
        })
    }
    pub async fn infer(&self, request: ProviderInferenceRequest) -> ProviderInferenceReply {
        #[cfg(feature = "eval-harness")]
        let observation = {
            let mut rows = self
                .eval_observations
                .lock()
                .expect("eval observations lock");
            let index = rows.len();
            rows.push(json!({
                "call_index": index,
                "provider": request.provider,
                "model": request.model,
                "thinking_level": request.thinking_level,
                "run_options": request.run_options,
                "max_output_tokens": request.max_output_tokens,
                "timeout_ms": request.timeout_ms,
                "prompt_bytes": request.messages.iter().map(|message| message.content.len()).sum::<usize>(),
                "schema_bytes": serde_json::to_vec(&request.schema).map(|bytes| bytes.len()).ok(),
                "started_at": chrono::Utc::now().to_rfc3339(),
                "disposition": "incomplete",
                "duration_ms": null,
                "usage": null,
                "cost_usd": null,
                "diagnostic": {"support_outcome": null, "shutdown": null, "reply_read": null, "helper": null}
            }));
            (index, std::time::Instant::now())
        };
        let reply = self
            .infer_once(
                request,
                #[cfg(feature = "eval-harness")]
                observation.0,
            )
            .await;
        #[cfg(feature = "eval-harness")]
        {
            let mut rows = self
                .eval_observations
                .lock()
                .expect("eval observations lock");
            rows[observation.0]["disposition"] = json!(reply.disposition);
            rows[observation.0]["duration_ms"] = json!(observation.1.elapsed().as_millis());
            rows[observation.0]["usage"] = json!(reply.usage);
        }
        reply
    }

    /// Evaluation-only accounting: no prompts, completions or credential data.
    #[cfg(feature = "eval-harness")]
    pub(crate) fn eval_observations(&self) -> Vec<Value> {
        self.eval_observations
            .lock()
            .expect("eval observations lock")
            .clone()
    }

    #[cfg(feature = "eval-harness")]
    fn record_diagnostic(&self, index: usize, field: &str, value: Value) {
        // A diagnostic must not make an otherwise valid provider call fail.
        if let Ok(mut rows) = self.eval_observations.lock() {
            if let Some(row) = rows.get_mut(index) {
                row["diagnostic"][field] = value;
            }
        }
    }

    async fn infer_once(
        &self,
        request: ProviderInferenceRequest,
        #[cfg(feature = "eval-harness")] observation: usize,
    ) -> ProviderInferenceReply {
        let model = match self.model(&request.provider, &request.model) {
            Ok(model) => model,
            Err(_) => {
                return ProviderInferenceReply::not_sent("The selected model is unavailable.")
            }
        };
        if validate_request(&request, &model).is_err() {
            return ProviderInferenceReply::not_sent(
                "The inference request or model controls are invalid.",
            );
        }
        if !self
            .readiness(&request.provider, &request.model)
            .is_ok_and(|value| value.available)
        {
            return ProviderInferenceReply::not_sent("The model provider is not connected.");
        }
        // A held account gets one attempt per back-off window; it is spent here,
        // once, even when several jobs reach this point together.
        let now = crate::commands::now_ms();
        if crate::providers::status::background_hold(&self.store, &request.provider, now)
            == BackgroundHold::ProbeDue
            && !crate::providers::status::claim_probe(&self.store, &request.provider, now)
        {
            return ProviderInferenceReply::not_sent(
                "This account is waiting until a request goes through.",
            );
        }
        let _admission = match self.admit() {
            Ok(lease) => lease,
            Err(_) => {
                return ProviderInferenceReply::not_sent(
                    "Model inference is unavailable during maintenance.",
                )
            }
        };
        let Some(pi) = &self.artifacts.pi else {
            return ProviderInferenceReply::not_sent("The model runtime is unavailable.");
        };
        let files = match create_search_support_files(&self.artifacts.app_data_dir) {
            Ok(files) => files,
            Err(_) => {
                return ProviderInferenceReply::not_sent(
                    "The isolated request could not be prepared.",
                )
            }
        };
        let Some(path) = &files.request else {
            return ProviderInferenceReply::not_sent("The isolated request could not be prepared.");
        };
        if write_request(path, &request).is_err() {
            return ProviderInferenceReply::not_sent("The isolated request could not be prepared.");
        }
        let launch = PiSupportLaunchConfig {
            executable: pi.executable.clone(),
            runtime_dir: pi.runtime_dir.clone(),
            extension: pi.provider_extension.clone(),
            agent_data_dir: self.agent_data_dir.clone(),
            private_working_dir: files.directory.clone(),
            lease_dir: self.artifacts.process_lease_dir.clone(),
            result_file: files.result.clone(),
            request_file: files.request.clone(),
            provider_credential: provider_credential_from_environment(&request.provider),
            mutation_api_key: None,
        };
        // The bundled support command consumes this flag; it is never part of
        // the inference request, model context or provider payload.
        #[cfg(feature = "eval-harness")]
        let command = "/loupist-provider-inference eval-diagnostics";
        #[cfg(not(feature = "eval-harness"))]
        let command = "/loupist-provider-inference";
        let outcome = pi_support::run(
            launch,
            command,
            Duration::from_millis(request.timeout_ms.saturating_add(15_000)),
            std::future::pending::<()>(),
            |_| Ok::<_, ()>(()),
            || pi_support::result_ready(&files.result, Some("inference")),
            None,
        )
        .await;
        #[cfg(feature = "eval-harness")]
        {
            self.record_diagnostic(
                observation,
                "support_outcome",
                json!(support_outcome_kind(&outcome.result)),
            );
            self.record_diagnostic(
                observation,
                "shutdown",
                json!(if outcome.shutdown.is_ok() {
                    "completed"
                } else {
                    "failed"
                }),
            );
        }
        match outcome.result {
            Err(SupportFailure::Launch(_)) => {
                return ProviderInferenceReply::not_sent(
                    "The isolated model runtime could not start.",
                )
            }
            Err(SupportFailure::CancelledBeforeDispatch) => {
                return ProviderInferenceReply::not_sent(
                    "The model call was cancelled before dispatch.",
                )
            }
            Err(_) => return ProviderInferenceReply::uncertain(),
            Ok(()) if outcome.shutdown.is_err() => return ProviderInferenceReply::uncertain(),
            Ok(()) => {}
        }
        let reply = read_reply(&files.result);
        #[cfg(feature = "eval-harness")]
        {
            self.record_diagnostic(
                observation,
                "reply_read",
                json!(if reply.is_ok() {
                    "validated"
                } else {
                    "invalid_or_unreadable"
                }),
            );
            if let Ok((_, Some(diagnostic), _)) = &reply {
                self.record_diagnostic(observation, "helper", diagnostic.clone());
            }
        }
        let (reply, provider_failure) = match reply {
            Ok((reply, _, failure)) => (reply, failure),
            Err(()) => (ProviderInferenceReply::uncertain(), None),
        };
        self.record_outcome(&request.provider, &reply, provider_failure);
        reply
    }

    /// The account's status follows background calls as it follows Chat: a
    /// plan limit holds, a completed call clears the hold.
    fn record_outcome(
        &self,
        provider: &str,
        reply: &ProviderInferenceReply,
        failure: Option<ProviderFailure>,
    ) {
        use crate::{domain::ChatFailureKind, providers::status::Trigger};
        let trigger = if reply.disposition == ProviderInferenceDisposition::Completed {
            Some(Trigger::Success)
        } else {
            failure.and_then(|failure| {
                Some(Trigger::Failure(match failure {
                    ProviderFailure::PlanLimit => ChatFailureKind::PlanLimit,
                    ProviderFailure::PlanNotEligible => ChatFailureKind::PlanNotEligible,
                    ProviderFailure::OutOfCredit => ChatFailureKind::OutOfCredit,
                    ProviderFailure::NeedsReconnect => ChatFailureKind::NeedsReconnect,
                    _ => return None,
                }))
            })
        };
        let Some(trigger) = trigger else {
            return;
        };
        if crate::providers::status::record_in(&self.store, &self.agent_data_dir, provider, trigger)
            .is_err()
        {
            eprintln!("Loupist could not record a provider status change");
        }
    }
}

#[cfg(feature = "eval-harness")]
fn support_outcome_kind<E>(result: &Result<(), SupportFailure<E>>) -> &'static str {
    match result {
        Ok(()) => "completed",
        Err(SupportFailure::Launch(_)) => "launch_failed",
        Err(SupportFailure::Dispatch(_)) => "dispatch_failed",
        Err(SupportFailure::CancelledBeforeDispatch) => "cancelled_before_dispatch",
        Err(SupportFailure::Cancelled) => "cancelled",
        Err(SupportFailure::Timeout) => "timeout",
        Err(SupportFailure::Stopped) => "stopped",
        Err(SupportFailure::Rejected) => "rejected",
        Err(SupportFailure::ExtensionFailed) => "extension_failed",
        Err(SupportFailure::Operation(_)) => "operation_failed",
    }
}

/// Reconstruct a bounded fixed-field diagnostic. Never retain provider errors,
/// headers, URLs, prompt/schema text, or arbitrary extension payload fields.
#[cfg(feature = "eval-harness")]
fn inference_diagnostic(diagnostic: &Value) -> Option<Value> {
    if serde_json::to_vec(diagnostic).ok()?.len() > 1024 {
        return None;
    }
    let fields = diagnostic.as_object()?;
    if fields.len() != 7
        || fields.keys().any(|key| {
            !matches!(
                key.as_str(),
                "type"
                    | "phase"
                    | "failure_kind"
                    | "dispatched"
                    | "cancelled"
                    | "http_status"
                    | "stop_reason"
            )
        })
        || fields.get("type")?.as_str()? != "inference_diagnostic"
    {
        return None;
    }
    let phase = fields.get("phase")?.as_str()?;
    if !matches!(
        phase,
        "provider" | "response" | "json" | "schema" | "complete"
    ) {
        return None;
    }
    let failure_kind = fields.get("failure_kind")?;
    if !failure_kind.is_null()
        && !failure_kind.as_str().is_some_and(|kind| {
            matches!(
                kind,
                "payload_invalid"
                    | "retry_refused"
                    | "dispatch_marker_failed"
                    | "identity_mismatch"
                    | "incomplete_response"
                    | "content_type"
                    | "output_size"
                    | "invalid_json"
                    | "schema_mismatch"
                    | "cancelled"
                    | "provider_failure"
            )
        })
    {
        return None;
    }
    let http_status = fields.get("http_status")?;
    if !http_status.is_null()
        && !http_status
            .as_u64()
            .is_some_and(|status| (100..=599).contains(&status))
    {
        return None;
    }
    let stop_reason = fields.get("stop_reason")?;
    if !stop_reason.is_null()
        && !stop_reason.as_str().is_some_and(|reason| {
            matches!(
                reason,
                "stop" | "length" | "toolUse" | "error" | "aborted" | "other"
            )
        })
    {
        return None;
    }
    Some(json!({
        "phase": phase,
        "failure_kind": failure_kind,
        "dispatched": fields.get("dispatched")?.as_bool()?,
        "cancelled": fields.get("cancelled")?.as_bool()?,
        "http_status": http_status,
        "stop_reason": stop_reason,
    }))
}

fn validate_request(request: &ProviderInferenceRequest, model: &ConfiguredModel) -> Result<(), ()> {
    model.validate().map_err(|_| ())?;
    model
        .validate_thinking_level(&request.thinking_level)
        .map_err(|_| ())?;
    model
        .validate_run_options(&request.run_options)
        .map_err(|_| ())?;
    if request.provider != model.provider
        || request.model != model.model
        || request.messages.is_empty()
        || request.messages.len() > 32
        || request.messages.iter().any(|message| {
            !matches!(message.role.as_str(), "system" | "user" | "assistant")
                || message.content.is_empty()
        })
        || !request
            .messages
            .iter()
            .any(|message| message.role == "user")
        || serde_json::to_vec(&request.messages).map_err(|_| ())?.len() > 256 * 1024
        || !request.schema.is_object()
        || serde_json::to_vec(&request.schema).map_err(|_| ())?.len() > 64 * 1024
        || !(1..=32768).contains(&request.max_output_tokens)
        || u64::from(request.max_output_tokens) > model.max_tokens
        || !(1000..=300_000).contains(&request.timeout_ms)
    {
        return Err(());
    }
    Ok(())
}

fn write_request(path: &Path, request: &ProviderInferenceRequest) -> Result<(), ()> {
    let mut body = serde_json::to_value(request).map_err(|_| ())?;
    body["protocol"] = json!(PROTOCOL);
    body["type"] = json!("inference");
    let bytes = serde_json::to_vec(&body).map_err(|_| ())?;
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(());
    }
    crate::fs::artifact_trust::ensure_private_regular_file(path).map_err(|_| ())?;
    let mut options = OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let mut file = options.open(path).map_err(|_| ())?;
    if !file.metadata().map_err(|_| ())?.is_file() {
        return Err(());
    }
    file.set_len(0).map_err(|_| ())?;
    file.write_all(&bytes).map_err(|_| ())?;
    file.sync_all().map_err(|_| ())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileReply {
    protocol: String,
    #[serde(rename = "type")]
    result_type: String,
    disposition: ProviderInferenceDisposition,
    body: Value,
    usage: Option<Value>,
    /// The provider's own error text, only to classify it here. It never
    /// reaches the reply body, a log or the graph worker.
    #[serde(default)]
    provider_error: Option<String>,
    #[cfg(feature = "eval-harness")]
    #[serde(default)]
    diagnostic: Option<Value>,
}
type ReadReply = (
    ProviderInferenceReply,
    Option<Value>,
    Option<ProviderFailure>,
);

fn read_reply(path: &Path) -> Result<ReadReply, ()> {
    crate::fs::artifact_trust::ensure_private_regular_file(path).map_err(|_| ())?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let file = options.open(path).map_err(|_| ())?;
    let metadata = file.metadata().map_err(|_| ())?;
    if !metadata.is_file() || metadata.len() > MAX_RESULT_BYTES {
        return Err(());
    }
    let mut bytes = Vec::new();
    file.take(MAX_RESULT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() as u64 > MAX_RESULT_BYTES {
        return Err(());
    }
    let result: FileReply = serde_json::from_slice(&bytes).map_err(|_| ())?;
    if result.protocol != PROTOCOL
        || result.result_type != "inference"
        || !result.body.is_object()
        || result.usage.as_ref().is_some_and(|usage| {
            !usage.is_object()
                || usage.as_object().unwrap().iter().any(|(key, value)| {
                    !matches!(
                        key.as_str(),
                        "input" | "output" | "cacheRead" | "cacheWrite" | "totalTokens"
                    ) || value.as_u64().is_none()
                })
        })
    {
        return Err(());
    }
    #[cfg(feature = "eval-harness")]
    let diagnostic = result.diagnostic.as_ref().and_then(inference_diagnostic);
    #[cfg(not(feature = "eval-harness"))]
    let diagnostic = None;
    Ok((
        ProviderInferenceReply {
            disposition: result.disposition,
            body: result.body,
            usage: result.usage,
        },
        diagnostic,
        result
            .provider_error
            .as_deref()
            .map(ProviderFailure::classify),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn background_inference_and_provider_controls_coexist_but_both_block_updates() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_test(dir.path().join("app"));
        let service = ProviderInferenceService::from_state(&state).unwrap();

        for inference_first in [true, false] {
            let (inference, support) = if inference_first {
                let inference = service.admit().unwrap();
                (
                    inference,
                    state.provider_support.acquire_oauth().await.unwrap(),
                )
            } else {
                let support = state.provider_support.acquire_oauth().await.unwrap();
                (service.admit().unwrap(), support)
            };
            assert_eq!(
                state.provider_support.try_acquire().unwrap_err().code,
                "conflict"
            );
            let blockers = state.maintenance.begin_update().unwrap_err();
            assert!(blockers
                .iter()
                .any(|item| item.kind == "provider_inference"));
            assert!(blockers.iter().any(|item| item.kind == "provider_support"));
            drop(support);
            drop(inference);

            let update = state.maintenance.begin_update().unwrap();
            assert_eq!(service.admit().unwrap_err().code, "maintenance_active");
            drop(update);
        }
    }

    #[test]
    fn background_calls_hold_on_a_plan_limit_and_clear_it_when_one_goes_through() {
        use crate::providers::status::{background_hold, record, CredentialKind, Trigger};
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_test(dir.path().join("app"));
        let service = ProviderInferenceService::from_state(&state).unwrap();
        record(&state, "openai", Trigger::Connected(CredentialKind::Plan)).unwrap();
        let now = crate::commands::now_ms;
        // World processing hit the plan limit while Chat used another account.
        service.record_outcome(
            "openai",
            &ProviderInferenceReply::uncertain(),
            Some(ProviderFailure::PlanLimit),
        );
        assert_ne!(
            background_hold(&state.store, "openai", now()),
            BackgroundHold::Clear
        );
        // An outage or a call that never left neither holds nor releases.
        service.record_outcome(
            "openai",
            &ProviderInferenceReply::uncertain(),
            Some(ProviderFailure::Unavailable),
        );
        service.record_outcome("openai", &ProviderInferenceReply::not_sent("x"), None);
        assert_ne!(
            background_hold(&state.store, "openai", now()),
            BackgroundHold::Clear
        );
        // The window's probe went through: the plan recovered.
        let completed = ProviderInferenceReply {
            disposition: ProviderInferenceDisposition::Completed,
            body: json!({}),
            usage: None,
        };
        service.record_outcome("openai", &completed, None);
        assert_eq!(
            background_hold(&state.store, "openai", now()),
            BackgroundHold::Clear
        );
    }

    #[test]
    fn reply_reader_rejects_truncation_wrong_contract_and_unbounded_usage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reply");
        crate::fs::artifact_trust::ensure_private_regular_file(&path).unwrap();
        let ordinary = json!({"protocol":PROTOCOL,"type":"inference","disposition":"completed","body":{"entities":[]},"usage":{"input":3,"output":1}});
        let mut unknown_field = ordinary.clone();
        unknown_field["unexpected"] = json!("private value");
        for value in [
            json!({"protocol":PROTOCOL,"type":"inference","disposition":"completed","body":{},"usage":{"headers":"secret"}}),
            json!({"protocol":"wrong","type":"inference","disposition":"completed","body":{},"usage":null}),
            unknown_field,
        ] {
            std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(read_reply(&path).is_err());
        }
        std::fs::write(&path, b"{\"protocol\":").unwrap();
        assert!(read_reply(&path).is_err());
        std::fs::write(&path, serde_json::to_vec(&ordinary).unwrap()).unwrap();
        let (reply, _, failure) = read_reply(&path).unwrap();
        assert_eq!(failure, None);
        let mut limited = json!({"protocol":PROTOCOL,"type":"inference","disposition":"uncertain","body":{"status":"uncertain","message":"host copy"},"usage":null});
        limited["provider_error"] =
            json!("429 {\"error\":{\"code\":\"subscription_sharing_usage_limit_exceeded\"}}");
        std::fs::write(&path, serde_json::to_vec(&limited).unwrap()).unwrap();
        let (limited, _, failure) = read_reply(&path).unwrap();
        assert_eq!(failure, Some(ProviderFailure::PlanLimit));
        assert!(!limited.body.to_string().contains("subscription_sharing"));
        assert_eq!(reply.disposition, ProviderInferenceDisposition::Completed);
        assert_eq!(reply.usage.as_ref(), Some(&ordinary["usage"]));
    }
}
