pub(crate) mod execution;
pub(crate) mod provider_failure;
pub(crate) mod response;
pub(crate) mod shell_path;
pub(crate) mod support;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
#[cfg(any(windows, not(debug_assertions)))]
use std::fs::File;
#[cfg(not(debug_assertions))]
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

#[cfg(windows)]
use crate::fs::windows::add_open_reparse_point_flag;
#[cfg(all(windows, not(debug_assertions)))]
use crate::fs::windows::{metadata_is_reparse, open_directory_no_reparse, open_regular_no_reparse};
#[cfg(windows)]
use crate::process_lease::ChildProcessJob;
#[cfg(unix)]
use crate::process_lease::{
    process_group_has_exited, signal_process_group, terminate_and_reap_process_group,
    wait_for_process_group_exit, ChildProcessLease, ProcessKind,
};
use crate::{
    agent_harness,
    fs::artifact_trust::{
        ensure_private_directory, read_private_regular_file_bounded, verify_executable,
        VerifiedExecutable,
    },
    process_lease::ProcessLeaseError,
    providers::catalog::{PI_THINKING_LEVELS, PROVIDER_CREDENTIAL_ENVIRONMENTS},
};
use thiserror::Error;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter},
    process::{Child, ChildStdin, Command},
    sync::{broadcast, Mutex},
    task::JoinHandle,
    time::timeout,
};

#[cfg(not(debug_assertions))]
use std::sync::OnceLock;

pub const PI_VERSION: &str = "1.4.0";
pub(crate) const PI_HTTP_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const LOUPIST_SYSTEM_PROMPT: &str = include_str!("../../agent/SYSTEM.md");
#[cfg(any(test, all(target_os = "macos", target_arch = "aarch64")))]
pub const PI_DARWIN_ARM64_ARCHIVE_SHA256: &str =
    "79d9979d76a2a5adb7c28e27e9b44629928ccd0dc9fbb6ae7217860bd5e8be5d";
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub const PI_WINDOWS_X64_ARCHIVE_SHA256: &str =
    "0f7256ea8bdac3af20012b94fdc1252a31cc6dc0d2f9b9eb539555d22e3e321b";
const MAX_RPC_FRAME_BYTES: usize = 8 * 1024 * 1024;
const PI_EVENT_BUFFER_CAPACITY: usize = 1_024;
const MAX_HOST_CONTEXT_BYTES: usize = 64 * 1024;
const RPC_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
const PI_PROJECT_SETTINGS_MAX_BYTES: u64 = 4 * 1024;
/// Versioned identity of the app-owned Pi cache environment. Bump this when a
/// change makes an existing derived Pi JSONL unsafe to resume (for example,
/// its CWD, transport, or extension/session contract changes).
pub(crate) const PI_SESSION_CACHE_POLICY_VERSION: &str = "scoped-settings-native-tools/v1";

#[derive(Debug, Error)]
pub enum PiError {
    #[error("Pi runtime directory is missing: {0}")]
    MissingRuntime(PathBuf),
    #[error("Pi runtime metadata does not match the pinned release")]
    UntrustedRuntime,
    #[error("Loupist Pi extension is missing: {0}")]
    MissingExtension(PathBuf),
    #[error("unsupported provider credential environment name")]
    UnsupportedCredential,
    #[error("invalid Pi launch value")]
    InvalidLaunchValue,
    #[error("Pi I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("Pi RPC frame exceeded {MAX_RPC_FRAME_BYTES} bytes")]
    FrameTooLarge,
    #[error("Pi RPC returned malformed JSON")]
    MalformedFrame,
    #[error("Pi RPC write timed out")]
    WriteTimeout,
    #[error("Pi process did not stop")]
    StopTimeout,
    #[error("Pi process ownership failed: {0}")]
    Lease(#[from] ProcessLeaseError),
}

#[derive(Clone)]
pub struct PiLaunchConfig {
    pub executable: PathBuf,
    pub runtime_dir: PathBuf,
    pub extension: PathBuf,
    pub system_prompt: PathBuf,
    pub agent_data_dir: PathBuf,
    pub working_dir: PathBuf,
    pub private_run_dir: PathBuf,
    pub lease_dir: PathBuf,
    pub broker_socket: PathBuf,
    pub broker_token: String,
    pub provider: String,
    pub model: String,
    pub thinking_level: String,
    pub run_options: BTreeMap<String, String>,
    pub pi_settings: crate::scoped_config::PiConfig,
    pub provider_credential: Option<(String, String)>,
    pub host_context: Option<String>,
    pub skill_files: Vec<PathBuf>,
    pub session: Option<PiSessionConfig>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PiSessionConfig {
    pub id: String,
    pub directory: PathBuf,
}

#[derive(Clone, Debug)]
pub struct PiImageContent {
    pub data: String,
    pub mime_type: String,
}

impl PiLaunchConfig {
    pub fn with_session(mut self, session: PiSessionConfig) -> Result<Self, PiError> {
        validate_session_config(&session)?;
        self.session = Some(session);
        Ok(self)
    }

    pub fn with_host_context(mut self, host_context: String) -> Result<Self, PiError> {
        if host_context.trim().is_empty()
            || host_context.len() > MAX_HOST_CONTEXT_BYTES
            || host_context.contains('\0')
        {
            return Err(PiError::InvalidLaunchValue);
        }
        self.host_context = Some(host_context);
        Ok(self)
    }

    pub fn with_skills(mut self, skill_files: Vec<PathBuf>) -> Result<Self, PiError> {
        let agent_root = self
            .system_prompt
            .parent()
            .ok_or(PiError::InvalidLaunchValue)?;
        for (index, path) in skill_files.iter().enumerate() {
            agent_harness::validate_skill_path(agent_root, path)
                .map_err(|_| PiError::InvalidLaunchValue)?;
            if skill_files[..index].iter().any(|seen| seen == path) {
                return Err(PiError::InvalidLaunchValue);
            }
        }
        self.skill_files = skill_files;
        Ok(self)
    }

    pub(crate) fn runtime_working_dir(&self) -> PathBuf {
        // Pi resumes CWD from the JSONL header. Keep it stable for this Chat;
        // its exclusive execution lease protects the per-turn settings swap.
        self.working_dir.clone()
    }

    fn validate(&self) -> Result<(), PiError> {
        validate_pi_settings(&self.pi_settings)?;
        if agent_harness::validate_extension_bundle(&self.extension).is_err() {
            return Err(PiError::MissingExtension(self.extension.clone()));
        }
        let system_bytes = include_bytes!("../../agent/SYSTEM.md").as_slice();
        #[cfg(feature = "eval-harness")]
        let registered = crate::eval_harness::artifacts::registered_bytes(&self.system_prompt);
        #[cfg(feature = "eval-harness")]
        let system_bytes = registered.as_deref().unwrap_or(system_bytes);
        if !is_exact_regular_file(&self.system_prompt, system_bytes) {
            return Err(PiError::MissingExtension(self.system_prompt.clone()));
        }
        let agent_root = self
            .system_prompt
            .parent()
            .ok_or(PiError::InvalidLaunchValue)?;
        for (index, path) in self.skill_files.iter().enumerate() {
            agent_harness::validate_skill_path(agent_root, path)
                .map_err(|_| PiError::InvalidLaunchValue)?;
            if self.skill_files[..index].iter().any(|seen| seen == path) {
                return Err(PiError::InvalidLaunchValue);
            }
        }
        for value in [&self.provider, &self.model, &self.broker_token] {
            if value.is_empty() || value.len() > 512 || value.contains('\0') {
                return Err(PiError::InvalidLaunchValue);
            }
        }
        if !PI_THINKING_LEVELS.contains(&self.thinking_level.as_str()) {
            return Err(PiError::InvalidLaunchValue);
        }
        let run_options =
            serde_json::to_string(&self.run_options).map_err(|_| PiError::InvalidLaunchValue)?;
        if run_options.len() > 4 * 1024 || run_options.contains('\0') {
            return Err(PiError::InvalidLaunchValue);
        }
        let host_context = self
            .host_context
            .as_deref()
            .ok_or(PiError::InvalidLaunchValue)?;
        if host_context.trim().is_empty()
            || host_context.len() > MAX_HOST_CONTEXT_BYTES
            || host_context.contains('\0')
        {
            return Err(PiError::InvalidLaunchValue);
        }
        if let Some((name, secret)) = &self.provider_credential {
            if !PROVIDER_CREDENTIAL_ENVIRONMENTS.contains(&name.as_str())
                || secret.is_empty()
                || secret.contains('\0')
            {
                return Err(PiError::UnsupportedCredential);
            }
        }
        if let Some(session) = &self.session {
            validate_session_config(session)?;
        }
        Ok(())
    }

    pub fn rpc_arguments(&self) -> Vec<String> {
        let system_prompt = LOUPIST_SYSTEM_PROMPT.to_owned();
        #[cfg(feature = "eval-harness")]
        let system_prompt = crate::eval_harness::artifacts::registered_bytes(&self.system_prompt)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or(system_prompt);
        let mut arguments = vec!["--mode".into(), "rpc".into()];
        if let Some(session) = &self.session {
            arguments.extend([
                "--session-dir".into(),
                session.directory.to_string_lossy().into_owned(),
                "--session-id".into(),
                session.id.clone(),
            ]);
        } else {
            arguments.push("--no-session".into());
        }
        arguments.extend([
            "--no-builtin-tools".into(),
            "--no-extensions".into(),
            "--extension".into(),
            self.extension.to_string_lossy().into_owned(),
            "--system-prompt".into(),
            system_prompt,
            // A trusted run-private project must not discover an append prompt.
            "--append-system-prompt".into(),
            String::new(),
            "--no-skills".into(),
        ]);
        for skill_file in &self.skill_files {
            arguments.push("--skill".into());
            arguments.push(skill_file.to_string_lossy().into_owned());
        }
        arguments.extend([
            "--no-prompt-templates".into(),
            "--no-themes".into(),
            "--no-context-files".into(),
            // `PiProcess::spawn` writes this execution snapshot into the Chat
            // private CWD, separate from both the workspace and shared auth.
            "--approve".into(),
            "--offline".into(),
            "--provider".into(),
            self.provider.clone(),
            "--model".into(),
            self.model.clone(),
            "--thinking".into(),
            self.thinking_level.clone(),
        ]);
        arguments
    }

    fn environment(
        &self,
        host_context_file: &Path,
        login_path: Option<&[PathBuf]>,
    ) -> BTreeMap<String, String> {
        // Supply OS essentials for native local tools, never inherit arbitrary
        // shell startup variables or credentials. The Bash definition applies
        // its own allowlist before spawning commands so provider/broker values
        // below are not handed to those child processes.
        let mut env = local_tool_environment();
        // Native Pi find runs in offline mode. Resolve its pinned helper from
        // the app resources before the user's login-shell and inherited PATH.
        if let Some(path) = shell_path::bash_path(
            &self.runtime_dir.join("workspace-tools"),
            login_path,
            env.get("PATH").map(String::as_str),
        ) {
            env.insert("PATH".into(), path);
        }
        env.extend([
            (
                "PI_CODING_AGENT_DIR".into(),
                self.agent_data_dir.to_string_lossy().into_owned(),
            ),
            (
                "PI_PACKAGE_DIR".into(),
                self.runtime_dir.to_string_lossy().into_owned(),
            ),
            ("PI_OFFLINE".into(), "1".into()),
            ("PI_TELEMETRY".into(), "0".into()),
            (
                "LOUPIST_BROKER_SOCKET".into(),
                self.broker_socket.to_string_lossy().into_owned(),
            ),
            (
                "LOUPIST_HOST_CONTEXT_FILE".into(),
                host_context_file.to_string_lossy().into_owned(),
            ),
        ]);
        if let Some((name, value)) = &self.provider_credential {
            env.insert(name.clone(), value.clone());
        }
        env.insert(
            "LOUPIST_MODEL_RUN_OPTIONS".into(),
            serde_json::to_string(&self.run_options).expect("model run options serialize"),
        );
        let temporary_directory = self.working_dir.join("temp").to_string_lossy().into_owned();
        for name in ["TMPDIR", "TMP", "TEMP"] {
            env.insert(name.into(), temporary_directory.clone());
        }
        #[cfg(feature = "e2e")]
        env.insert("LOUPIST_E2E_CHAT_DIAGNOSTICS".into(), "1".into());
        env
    }
}

fn local_tool_environment() -> BTreeMap<String, String> {
    [
        "PATH",
        "HOME",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "TMPDIR",
        "TMP",
        "TEMP",
        "LANG",
        "LANGUAGE",
        "LC_ALL",
        "LC_CTYPE",
        "LC_MESSAGES",
        "TZ",
        "SYSTEMROOT",
        "SystemRoot",
        "WINDIR",
        "COMSPEC",
        "ComSpec",
        "PATHEXT",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "LOCALAPPDATA",
        "APPDATA",
    ]
    .into_iter()
    .filter_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| (name.to_owned(), value))
    })
    .collect()
}

// E2E diagnostics deliberately reconstruct fixed fields rather than logging
// provider errors, headers, prompts, or complete RPC payloads.
#[cfg(any(debug_assertions, test, feature = "e2e", feature = "eval-harness"))]
pub(crate) fn e2e_rpc_diagnostic(payload: &Value) -> Option<Value> {
    match payload.get("type").and_then(Value::as_str)? {
        "loupist_diagnostic" => match payload.get("phase").and_then(Value::as_str)? {
            phase @ ("before_agent_start"
            | "before_provider_headers"
            | "before_provider_request") => Some(json!({ "phase": phase })),
            "provider_response" => {
                let status = payload.get("status").and_then(Value::as_u64)?;
                (100..=599)
                    .contains(&status)
                    .then(|| json!({ "phase": "provider_response", "status": status }))
            }
            _ => None,
        },
        "extension_error" => Some(json!({ "phase": "extension", "category": "failed" })),
        "response" if payload.get("success").and_then(Value::as_bool) == Some(false) => {
            let command = payload
                .get("command")
                .and_then(Value::as_str)
                .filter(|command| {
                    matches!(
                        *command,
                        "get_state" | "get_entries" | "get_available_thinking_levels" | "prompt"
                    )
                })
                .unwrap_or("other");
            Some(json!({ "phase": "rpc_rejected", "category": command }))
        }
        "message_end"
            if payload.pointer("/message/role").and_then(Value::as_str) == Some("assistant") =>
        {
            match payload
                .pointer("/message/stopReason")
                .and_then(Value::as_str)
            {
                Some("error") => {
                    let message = payload
                        .pointer("/message/errorMessage")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let category = e2e_provider_failure_category(message);
                    let detail = e2e_provider_failure_detail(message);
                    Some(
                        json!({ "phase": "assistant_end", "category": category, "detail": detail }),
                    )
                }
                Some("aborted") => Some(json!({ "phase": "assistant_end", "category": "aborted" })),
                _ => None,
            }
        }
        _ => None,
    }
}

// Match SDK-owned error shapes without retaining their variable provider,
// path, response-body, or credential details. These describe a local setup
// stage and do not imply that a remote provider rejected authentication.
#[cfg(any(debug_assertions, test, feature = "e2e", feature = "eval-harness"))]
fn e2e_provider_failure_detail(message: &str) -> &'static str {
    let message: String = message.chars().take(8192).collect();
    let message = message.to_ascii_lowercase();
    let shapes: &[(&[&str], &str)] = &[
        (
            &["credential store read failed", "failed to read auth.json"],
            "credential_store_read",
        ),
        (
            &[
                "credential store modify failed",
                "credential store delete failed",
                "read-only credential storage",
            ],
            "credential_store_write",
        ),
        (
            &["auth storage lock", "failed to acquire auth storage lock"],
            "credential_store_lock",
        ),
        (
            &["oauth refresh failed", "openai codex token refresh error"],
            "oauth_refresh",
        ),
        (
            &["oauth refresh returned a token that expires too soon"],
            "oauth_expiry",
        ),
        (&["oauth auth derivation failed"], "oauth_derivation"),
        (
            &["api key auth failed", "api key auth check failed"],
            "api_key_resolution",
        ),
        (
            &[
                "provider is not configured",
                "no authentication method configured",
            ],
            "provider_not_configured",
        ),
        (&["unknown provider:"], "provider_missing"),
        (
            &["no api provider registered", "has no api implementation"],
            "api_implementation_missing",
        ),
        (
            &["failed to extract accountid from token"],
            "token_account_parse",
        ),
        (
            &[
                "requires json-schema constrained sampling",
                "cannot use grammar constrained sampling",
                "grammar constrained sampling requires",
            ],
            "tool_schema",
        ),
        (
            &[
                "cannot find module",
                "cannot find package",
                "module not found",
                "failed to resolve module",
                "dynamic require of",
                "cannot resolve module",
            ],
            "module_load",
        ),
        (
            &[
                "cannot read properties of",
                "cannot read property",
                "is not an object (evaluating",
            ],
            "javascript_property_access",
        ),
        (
            &["is not a function", "is not callable"],
            "javascript_function",
        ),
        (&["is not iterable"], "javascript_iterable"),
        (
            &[
                "invalid character in header",
                "invalid header name",
                "invalid header value",
                "is not a valid http header",
            ],
            "request_headers",
        ),
        (
            &[
                "circular structure",
                "bigint value can't be serialized",
                "do not know how to serialize a bigint",
            ],
            "request_serialization",
        ),
        (&["invalid timeoutms"], "request_timeout_option"),
        (&["invalid url"], "request_url"),
    ];
    shapes
        .iter()
        .find(|(patterns, _)| patterns.iter().any(|pattern| message.contains(pattern)))
        .map_or("unclassified", |(_, detail)| detail)
}

// Provider text is used only for a best-effort local classification. The
// returned static value is the complete diagnostic; none of the input leaves
// this function. Keep inspection bounded even if a provider returns a body.
#[cfg(any(debug_assertions, test, feature = "e2e", feature = "eval-harness"))]
fn e2e_provider_failure_category(message: &str) -> &'static str {
    let message: String = message.chars().take(8192).collect();
    let message = message.to_ascii_lowercase();
    let contains_any = |patterns: &[&str]| patterns.iter().any(|pattern| message.contains(pattern));
    let has_status = |status: &str| {
        message.starts_with(&format!("{status} "))
            || message.starts_with(&format!("{status}:"))
            || message.contains(&format!("http {status}"))
            || message.contains(&format!("status {status}"))
            || message.contains(&format!("status: {status}"))
    };
    if contains_any(&[
        "no api key",
        "missing api key",
        "api key is missing",
        "no credentials",
        "missing credentials",
        "not logged in",
        "authentication credentials were not provided",
    ]) {
        "missing_auth"
    } else if has_status("429")
        || contains_any(&[
            "rate limit",
            "rate_limit",
            "too many requests",
            "usage limit",
            "quota exceeded",
            "insufficient_quota",
        ])
    {
        "rate_limit"
    } else if has_status("401")
        || has_status("403")
        || contains_any(&[
            "unauthorized",
            "forbidden",
            "invalid api key",
            "incorrect api key",
            "invalid token",
            "expired token",
            "token has expired",
            "invalid_grant",
            "authentication failed",
            "authentication_error",
            "failed to extract accountid from token",
        ])
    {
        "auth_rejected"
    } else if contains_any(&["model_not_found", "unknown model", "unsupported model"])
        || (message.contains("model")
            && contains_any(&[
                "not supported",
                "does not exist",
                "not available",
                "do not have access",
            ]))
        || (message.contains("reasoning") && contains_any(&["not supported", "unsupported"]))
    {
        "model_unsupported"
    } else if contains_any(&[
        "fetch failed",
        "network error",
        "network request failed",
        "connection error",
        "connection refused",
        "econnreset",
        "econnrefused",
        "enotfound",
        "etimedout",
        "timed out",
        "timeout",
        "socket hang up",
        "websocket closed",
        "websocket error",
        "websocket transport is not available",
    ]) {
        "transport"
    } else {
        "unknown"
    }
}

#[cfg(any(debug_assertions, test, feature = "e2e", feature = "eval-harness"))]
pub(crate) fn e2e_failure_category(message: &str) -> &'static str {
    match message {
        "Pi did not confirm its model" | "Pi did not confirm the requested model" => {
            "model_mismatch"
        }
        "Pi did not confirm thinking level" | "Pi did not confirm the requested thinking level" => {
            "thinking_mismatch"
        }
        "thinking level is not supported by Pi for the selected model" => "thinking_unsupported",
        "Pi returned invalid thinking levels" => "thinking_catalog_invalid",
        "Pi session identity drifted" => "session_identity_mismatch",
        "Pi session file is invalid"
        | "Pi session file is unavailable"
        | "Pi session file escaped its app-owned directory"
        | "Pi session path is untrusted"
        | "Pi session directory is unavailable" => "session_path_invalid",
        "Pi session is not idle or compactable" => "session_state_invalid",
        "Pi returned invalid session entries"
        | "Pi returned an invalid session cursor"
        | "cold Pi session loaded unexpected prior context" => "session_entries_invalid",
        "Pi stopped during session initialization" => "process_stopped_before_prompt",
        "Pi session initialization timed out" => "startup_timeout",
        "Pi event stream closed during session initialization" => "startup_stream_closed",
        _ if message.starts_with(provider_failure::OTHER) => "provider_turn_failed",
        "Pi rejected the prompt" => "prompt_rejected",
        "Pi extension failed" => "extension_failed",
        "Pi emitted malformed JSON" => "malformed_json",
        "Pi event stream fell behind and was stopped safely" => "event_stream_lagged",
        "Pi event stream was interrupted" => "event_stream_closed",
        "Pi message_start omitted its message" => "message_start_missing",
        "Pi started an assistant message before ending the previous message" => {
            "message_start_overlap"
        }
        "Pi streamed assistant text outside a message" => "text_outside_message",
        "Pi message_end omitted its message" => "message_end_missing",
        "Pi ended an assistant message that was not started" => "message_end_unstarted",
        "Pi streamed assistant text did not match message_end" => "text_mismatch",
        "Pi assistant message omitted its stop reason" => "stop_reason_missing",
        "Pi assistant message used an unknown stop reason" => "stop_reason_unknown",
        "Pi assistant message content is malformed" => "message_content_malformed",
        "Pi assistant text block is malformed" => "text_block_malformed",
        "Pi assistant thinking block is malformed" => "thinking_block_malformed",
        "Pi assistant tool call is malformed" => "tool_call_malformed",
        "Pi assistant message contains an unknown content block" => "content_block_unknown",
        "Pi settled with an unfinished assistant message" => "message_unfinished",
        "Pi settled with an incomplete tool request" => "tool_request_incomplete",
        "Pi settled without authoritative assistant text" => "authoritative_text_missing",
        "Pi assistant response reached its length limit" => "length_limit",
        "Pi settled without an assistant message" => "missing_assistant_message",
        _ if message.starts_with("Pi rejected the ") && message.ends_with(" control") => {
            "startup_rpc_rejected"
        }
        _ => "other",
    }
}

#[cfg(any(debug_assertions, feature = "e2e"))]
pub(crate) fn trace_e2e_failure(phase: &'static str, message: &str) {
    let category = e2e_failure_category(message);
    eprintln!("Loupist E2E Chat diagnostic: phase={phase} category={category}");
}

// Transport, provider retries, and timeout are host policy, never inherited
// from Pi's shared authentication-directory settings.
pub(crate) fn pi_settings_value(settings: &crate::scoped_config::PiConfig) -> Value {
    json!({
        "transport": "auto",
        "httpIdleTimeoutMs": PI_HTTP_IDLE_TIMEOUT.as_millis() as u64,
        "compaction": {
            "enabled": true,
            "reserveTokens": settings.compaction.reserve_tokens,
            "keepRecentTokens": settings.compaction.keep_recent_tokens,
            // Explicitly clear any app-private historical per-model overrides.
            "modelOverrides": null,
        },
        "retry": {
            "enabled": settings.retry.enabled,
            "maxRetries": settings.retry.max_retries,
            "baseDelayMs": settings.retry.base_delay_ms,
            "maxAgentDelayMs": settings.retry.max_agent_delay_ms,
            "provider": {
                "maxRetries": 0,
                "timeoutMs": PI_HTTP_IDLE_TIMEOUT.as_millis() as u64,
                "maxRetryDelayMs": 60_000,
            },
        },
    })
}

fn validate_pi_settings(settings: &crate::scoped_config::PiConfig) -> Result<(), PiError> {
    if settings.compaction.reserve_tokens > 9_007_199_254_740_991
        || settings.compaction.keep_recent_tokens > 9_007_199_254_740_991
        || settings.retry.max_retries > 10
        || settings.retry.base_delay_ms > 300_000
        || settings.retry.max_agent_delay_ms > 300_000
    {
        return Err(PiError::InvalidLaunchValue);
    }
    Ok(())
}

/// Materialize a frozen execution snapshot. Shared authentication and the
/// selected workspace are never destinations for project-specific settings.
fn ensure_pi_project_settings(config: &PiLaunchConfig) -> Result<(), PiError> {
    validate_pi_settings(&config.pi_settings)?;
    let runtime = config.runtime_working_dir();
    ensure_private_directory(&runtime).map_err(|_| PiError::InvalidLaunchValue)?;
    let directory = runtime.join(".pi");
    ensure_private_directory(&directory).map_err(|_| PiError::InvalidLaunchValue)?;
    let path = directory.join("settings.json");
    let mut expected = serde_json::to_vec(&pi_settings_value(&config.pi_settings))
        .map_err(|_| PiError::InvalidLaunchValue)?;
    expected.push(b'\n');
    match std::fs::symlink_metadata(&path) {
        Ok(_) => {
            let existing = read_private_regular_file_bounded(&path, PI_PROJECT_SETTINGS_MAX_BYTES)
                .map_err(|_| PiError::InvalidLaunchValue)?;
            if existing == expected {
                return Ok(());
            }
            // This is disposable app-owned state, never a user config file.
            // Replace an earlier execution snapshot atomically before launch.
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(PiError::Io(error)),
    }
    let temporary = directory.join(format!(".settings-{}.tmp", uuid::Uuid::new_v4()));
    let write = || -> Result<(), PiError> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        #[cfg(windows)]
        add_open_reparse_point_flag(&mut options);
        let mut file = options.open(&temporary)?;
        std::io::Write::write_all(&mut file, &expected)?;
        file.sync_all()?;
        std::fs::rename(&temporary, &path)?;
        Ok(())
    };
    let result = write();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result?;
    let verified = read_private_regular_file_bounded(&path, PI_PROJECT_SETTINGS_MAX_BYTES)
        .map_err(|_| PiError::InvalidLaunchValue)?;
    (verified == expected)
        .then_some(())
        .ok_or(PiError::InvalidLaunchValue)
}

fn validate_session_config(session: &PiSessionConfig) -> Result<(), PiError> {
    if uuid::Uuid::parse_str(&session.id)
        .ok()
        .map(|id| id.to_string())
        .as_deref()
        != Some(session.id.as_str())
        || !session.directory.is_absolute()
    {
        return Err(PiError::InvalidLaunchValue);
    }
    Ok(())
}

#[derive(Clone)]
pub struct PiSupportLaunchConfig {
    pub executable: PathBuf,
    pub runtime_dir: PathBuf,
    pub extension: PathBuf,
    pub agent_data_dir: PathBuf,
    pub private_working_dir: PathBuf,
    pub lease_dir: PathBuf,
    pub result_file: PathBuf,
    pub request_file: Option<PathBuf>,
    pub provider_credential: Option<(String, String)>,
    pub mutation_api_key: Option<String>,
}

impl PiSupportLaunchConfig {
    fn validate(&self) -> Result<(), PiError> {
        if agent_harness::validate_provider_extension_bundle(&self.extension).is_err() {
            return Err(PiError::MissingExtension(self.extension.clone()));
        }
        if self.result_file.as_os_str().is_empty()
            || self
                .request_file
                .as_ref()
                .is_some_and(|path| path.as_os_str().is_empty())
        {
            return Err(PiError::InvalidLaunchValue);
        }
        if let Some((name, secret)) = &self.provider_credential {
            if !PROVIDER_CREDENTIAL_ENVIRONMENTS.contains(&name.as_str())
                || secret.is_empty()
                || secret.contains('\0')
            {
                return Err(PiError::UnsupportedCredential);
            }
        }
        if self.mutation_api_key.as_ref().is_some_and(|secret| {
            secret.is_empty() || secret.len() > 8 * 1024 || secret.contains('\0')
        }) {
            return Err(PiError::UnsupportedCredential);
        }
        Ok(())
    }

    fn rpc_arguments(&self) -> Vec<String> {
        vec![
            "--mode".into(),
            "rpc".into(),
            "--no-session".into(),
            "--no-builtin-tools".into(),
            "--no-extensions".into(),
            "--extension".into(),
            self.extension.to_string_lossy().into_owned(),
            "--no-skills".into(),
            "--no-prompt-templates".into(),
            "--no-themes".into(),
            "--no-context-files".into(),
            "--provider".into(),
            "google".into(),
            "--model".into(),
            "gemini-2.5-flash".into(),
        ]
    }

    fn environment(&self) -> BTreeMap<String, String> {
        let mut environment = BTreeMap::from([
            (
                "PI_CODING_AGENT_DIR".into(),
                self.agent_data_dir.to_string_lossy().into_owned(),
            ),
            (
                "PI_PACKAGE_DIR".into(),
                self.runtime_dir.to_string_lossy().into_owned(),
            ),
            ("PI_TELEMETRY".into(), "0".into()),
            // The support command is intercepted by the bundled extension before
            // any model request. A non-secret bootstrap value lets Pi initialize
            // the RPC session without borrowing a user's provider credential.
            ("GEMINI_API_KEY".into(), "loupist-provider-bootstrap".into()),
            (
                "LOUPIST_PROVIDER_RESULT_FILE".into(),
                self.result_file.to_string_lossy().into_owned(),
            ),
        ]);
        if let Some(request_file) = &self.request_file {
            environment.insert(
                "LOUPIST_PROVIDER_REQUEST_FILE".into(),
                request_file.to_string_lossy().into_owned(),
            );
        }
        if let Some((name, value)) = &self.provider_credential {
            environment.insert(name.clone(), value.clone());
        }
        if let Some(value) = &self.mutation_api_key {
            environment.insert("LOUPIST_PROVIDER_API_KEY".into(), value.clone());
        }
        environment
    }
}

#[must_use]
struct VerifiedPiRuntime {
    _executable: VerifiedExecutable,
    #[cfg(windows)]
    _tree_handles: Vec<File>,
}

fn verify_pi_runtime(executable: &Path, runtime_dir: &Path) -> Result<VerifiedPiRuntime, PiError> {
    let verified_executable =
        verify_executable(executable).map_err(|_| PiError::UntrustedRuntime)?;
    if !runtime_dir.is_dir() {
        return Err(PiError::MissingRuntime(runtime_dir.to_path_buf()));
    }
    let version = std::fs::read_to_string(runtime_dir.join(".pi-version"))
        .map_err(|_| PiError::UntrustedRuntime)?;
    let archive_digest = std::fs::read_to_string(runtime_dir.join(".pi-archive.sha256"))
        .map_err(|_| PiError::UntrustedRuntime)?;
    #[cfg(debug_assertions)]
    let executable_matches_development_metadata = {
        let executable_digest = std::fs::read_to_string(runtime_dir.join(".pi-executable.sha256"))
            .map_err(|_| PiError::UntrustedRuntime)?;
        file_sha256(executable)? == executable_digest.trim()
    };
    #[cfg(not(debug_assertions))]
    let executable_matches_development_metadata = true;
    let package: Value = serde_json::from_slice(
        &std::fs::read(runtime_dir.join("package.json")).map_err(|_| PiError::UntrustedRuntime)?,
    )
    .map_err(|_| PiError::UntrustedRuntime)?;
    let expected_archive_digest = pinned_pi_archive_sha256().ok_or(PiError::UntrustedRuntime)?;
    if version.trim() != PI_VERSION
        || archive_digest.trim() != expected_archive_digest
        || package.get("version").and_then(Value::as_str) != Some(PI_VERSION)
        || !executable_matches_development_metadata
    {
        return Err(PiError::UntrustedRuntime);
    }
    #[cfg(not(debug_assertions))]
    let _tree_handles = verify_pi_runtime_tree(runtime_dir)?;
    Ok(VerifiedPiRuntime {
        _executable: verified_executable,
        #[cfg(all(windows, not(debug_assertions)))]
        _tree_handles,
        #[cfg(all(windows, debug_assertions))]
        _tree_handles: Vec::new(),
    })
}

#[cfg(not(debug_assertions))]
fn verify_pi_runtime_tree(runtime_dir: &Path) -> Result<Vec<File>, PiError> {
    static VERIFIED_TREE: OnceLock<String> = OnceLock::new();
    let expected = env!("LOUPIST_PI_RUNTIME_TREE_SHA256");
    if VERIFIED_TREE.get().is_some_and(|digest| digest == expected) {
        return Ok(Vec::new());
    }
    let (tree_digest, handles) = pi_runtime_tree_digest(runtime_dir)?;
    if tree_digest != expected {
        return Err(PiError::UntrustedRuntime);
    }
    let _ = VERIFIED_TREE.set(tree_digest);
    Ok(handles)
}

#[cfg(not(debug_assertions))]
fn pi_runtime_tree_digest(runtime_dir: &Path) -> Result<(String, Vec<File>), PiError> {
    let mut records = Vec::new();
    let mut handles = Vec::new();
    let mut entries = 0_usize;
    let mut bytes = 0_u64;
    collect_pi_runtime_tree(
        runtime_dir,
        runtime_dir,
        0,
        &mut entries,
        &mut bytes,
        &mut records,
        &mut handles,
    )?;
    records.sort_by(|left, right| left.0.cmp(&right.0));
    let mut tree = Sha256::new();
    for (relative, kind, size, digest) in records {
        tree.update([kind]);
        tree.update((relative.len() as u64).to_be_bytes());
        tree.update(relative.as_bytes());
        tree.update(size.to_be_bytes());
        tree.update(digest);
    }
    Ok((hex::encode(tree.finalize()), handles))
}

#[cfg(not(debug_assertions))]
#[allow(clippy::too_many_arguments)]
fn collect_pi_runtime_tree(
    root: &Path,
    directory: &Path,
    depth: usize,
    entries: &mut usize,
    bytes: &mut u64,
    records: &mut Vec<(String, u8, u64, [u8; 32])>,
    handles: &mut Vec<File>,
) -> Result<(), PiError> {
    const MAX_ENTRIES: usize = 20_000;
    const MAX_DEPTH: usize = 64;
    const MAX_BYTES: u64 = 512 * 1024 * 1024;
    if depth > MAX_DEPTH {
        return Err(PiError::UntrustedRuntime);
    }
    #[cfg(windows)]
    handles.push(open_directory_no_reparse(directory).map_err(|_| PiError::UntrustedRuntime)?);
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        *entries += 1;
        if *entries > MAX_ENTRIES {
            return Err(PiError::UntrustedRuntime);
        }
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink()
            || cfg!(windows) && pi_windows_metadata_is_reparse(&metadata)
        {
            return Err(PiError::UntrustedRuntime);
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| PiError::UntrustedRuntime)?
            .to_str()
            .ok_or(PiError::UntrustedRuntime)?
            .replace(std::path::MAIN_SEPARATOR, "/");
        if metadata.is_dir() {
            records.push((relative, b'd', 0, [0; 32]));
            collect_pi_runtime_tree(root, &path, depth + 1, entries, bytes, records, handles)?;
        } else if metadata.is_file() {
            *bytes = bytes.saturating_add(metadata.len());
            if *bytes > MAX_BYTES {
                return Err(PiError::UntrustedRuntime);
            }
            #[cfg(windows)]
            let mut file = open_regular_no_reparse(&path).map_err(|_| PiError::UntrustedRuntime)?;
            #[cfg(not(windows))]
            let mut file = {
                let mut options = OpenOptions::new();
                options.read(true);
                #[cfg(unix)]
                options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
                options.open(&path)?
            };
            let mut digest = Sha256::new();
            let mut total = 0_u64;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                total += read as u64;
                digest.update(&buffer[..read]);
            }
            if total != metadata.len() {
                return Err(PiError::UntrustedRuntime);
            }
            records.push((relative, b'f', total, digest.finalize().into()));
            #[cfg(windows)]
            handles.push(file);
        } else {
            return Err(PiError::UntrustedRuntime);
        }
    }
    Ok(())
}

#[cfg(all(windows, not(debug_assertions)))]
fn pi_windows_metadata_is_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata_is_reparse(metadata)
}

#[cfg(all(not(windows), not(debug_assertions)))]
fn pi_windows_metadata_is_reparse(_metadata: &std::fs::Metadata) -> bool {
    false
}

fn pinned_pi_archive_sha256() -> Option<&'static str> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Some(PI_DARWIN_ARM64_ARCHIVE_SHA256);
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Some(PI_WINDOWS_X64_ARCHIVE_SHA256);
    #[allow(unreachable_code)]
    None
}

/// A one-shot private file the extension reads and deletes at load. Rust
/// removes it if Pi exits or fails before then.
struct BootstrapFile(PathBuf);

impl BootstrapFile {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for BootstrapFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Descriptor on which Pi inherits the tool-broker token.
#[cfg(unix)]
pub(crate) const BROKER_TOKEN_FD: i32 = 3;
#[cfg(unix)]
pub(crate) const BROKER_TOKEN_FD_ENV: &str = "LOUPIST_BROKER_TOKEN_FD";

/// Hands the per-turn broker token to the extension without placing it in
/// Pi's environment or argv, which same-user processes can read (`ps -E`).
/// Unix: a pipe whose read end Pi inherits at fd 3; the token is followed by a
/// newline, and the extension reads only up to it (never waiting for EOF,
/// which a child that briefly inherited the write end could delay) and closes
/// it at load, before any native tool can start, so no Bash command inherits
/// it. Other platforms: a one-shot private file.
pub(crate) struct BrokerTokenHandoff {
    #[cfg(unix)]
    reader: std::os::fd::OwnedFd,
    #[cfg(not(unix))]
    _file: BootstrapFile,
}

#[cfg(unix)]
impl BrokerTokenHandoff {
    pub(crate) fn pipe(token: &str) -> io::Result<Self> {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        let mut descriptors = [0; 2];
        // SAFETY: pipe(2) writes two new descriptors into the array.
        if unsafe { libc::pipe(descriptors.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: both descriptors were just created and are owned only here.
        let (reader, writer) = unsafe {
            (
                OwnedFd::from_raw_fd(descriptors[0]),
                OwnedFd::from_raw_fd(descriptors[1]),
            )
        };
        for descriptor in [&reader, &writer] {
            // SAFETY: sets close-on-exec on an owned descriptor so no other
            // child the app spawns inherits either end.
            if unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } == -1
            {
                return Err(io::Error::last_os_error());
            }
        }
        // A token (at most 512 bytes) and its newline fit the pipe buffer, so
        // this completes before Pi starts. The reader stops at the newline.
        let mut line = Vec::with_capacity(token.len() + 1);
        line.extend_from_slice(token.as_bytes());
        line.push(b'\n');
        std::io::Write::write_all(&mut std::fs::File::from(writer), &line)?;
        Ok(Self { reader })
    }

    fn prepare(
        config: &PiLaunchConfig,
        environment: &mut BTreeMap<String, String>,
    ) -> Result<Self, PiError> {
        environment.insert(BROKER_TOKEN_FD_ENV.into(), BROKER_TOKEN_FD.to_string());
        Ok(Self::pipe(&config.broker_token)?)
    }

    pub(crate) fn attach(&self, command: &mut Command) {
        use std::os::fd::AsRawFd;
        let source = self.reader.as_raw_fd();
        // SAFETY: runs in the forked child before exec and calls only the
        // async-signal-safe fcntl/dup2. dup2 onto the same number would keep
        // close-on-exec set, so that case clears the flag instead.
        unsafe {
            command.pre_exec(move || {
                let result = if source == BROKER_TOKEN_FD {
                    libc::fcntl(source, libc::F_SETFD, 0)
                } else {
                    libc::dup2(source, BROKER_TOKEN_FD)
                };
                if result == -1 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
    }
}

#[cfg(not(unix))]
impl BrokerTokenHandoff {
    fn prepare(
        config: &PiLaunchConfig,
        environment: &mut BTreeMap<String, String>,
    ) -> Result<Self, PiError> {
        let file = write_bootstrap_file(
            &config.private_run_dir,
            "broker-token",
            config.broker_token.as_bytes(),
        )?;
        environment.insert(
            "LOUPIST_BROKER_TOKEN_FILE".into(),
            file.path().to_string_lossy().into_owned(),
        );
        Ok(Self { _file: file })
    }

    fn attach(&self, _command: &mut Command) {}
}

impl BrokerTokenHandoff {
    /// Pi holds its own copy of the pipe once started, so Unix keeps nothing;
    /// a file stays until Pi exits as the cleanup fallback.
    fn after_spawn(self) -> Option<Self> {
        if cfg!(unix) {
            None
        } else {
            Some(self)
        }
    }
}

fn write_host_context_file(config: &PiLaunchConfig) -> Result<BootstrapFile, PiError> {
    let metadata = std::fs::symlink_metadata(&config.private_run_dir)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PiError::InvalidLaunchValue);
    }
    #[cfg(unix)]
    std::fs::set_permissions(
        &config.private_run_dir,
        std::fs::Permissions::from_mode(0o700),
    )?;

    let host_context = config
        .host_context
        .as_deref()
        .ok_or(PiError::InvalidLaunchValue)?;
    write_bootstrap_file(
        &config.private_run_dir,
        "host-context",
        host_context.as_bytes(),
    )
}

fn write_bootstrap_file(
    directory: &Path,
    kind: &str,
    bytes: &[u8],
) -> Result<BootstrapFile, PiError> {
    let path = directory.join(format!(
        ".loupist-{kind}-{}.json",
        uuid::Uuid::new_v4().simple()
    ));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&path)?;
    let bootstrap = BootstrapFile(path);
    std::io::Write::write_all(&mut file, bytes)?;
    file.sync_all()?;
    #[cfg(unix)]
    std::fs::set_permissions(bootstrap.path(), std::fs::Permissions::from_mode(0o600))?;
    Ok(bootstrap)
}

fn pi_command(
    executable: &Path,
    arguments: Vec<String>,
    working_dir: &Path,
    environment: BTreeMap<String, String>,
    broker_token: Option<&BrokerTokenHandoff>,
) -> Command {
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .current_dir(working_dir)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(windows)]
    ChildProcessJob::configure_command(&mut command);
    if let Some(broker_token) = broker_token {
        broker_token.attach(&mut command);
    }
    command
}

fn file_sha256(path: &Path) -> Result<String, PiError> {
    let metadata = std::fs::symlink_metadata(path).map_err(PiError::Io)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(PiError::UntrustedRuntime);
    }
    Ok(hex::encode(Sha256::digest(std::fs::read(path)?)))
}

fn is_exact_regular_file(path: &Path, expected: &[u8]) -> bool {
    file_sha256(path).is_ok_and(|digest| digest == hex::encode(Sha256::digest(expected)))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PiEvent {
    Rpc { payload: Value },
    ProtocolError { message: String },
    Exited,
}

// Constructed immediately after spawn, before any fallible pipe setup or await.
// This value drops before Child, keeping the Unix leader's PID reserved until
// the last possible group signal has been sent.
struct PiProcessGroup {
    #[cfg(unix)]
    process_group_id: i32,
    #[cfg(unix)]
    lease: Option<ChildProcessLease>,
    #[cfg(windows)]
    job: Option<ChildProcessJob>,
    armed: bool,
}

impl PiProcessGroup {
    fn own(child: &Child) -> Result<Self, PiError> {
        let mut group = Self {
            #[cfg(unix)]
            process_group_id: child.id().ok_or(PiError::InvalidLaunchValue)? as i32,
            #[cfg(unix)]
            lease: None,
            #[cfg(windows)]
            job: None,
            armed: true,
        };
        #[cfg(windows)]
        {
            group.job = Some(ChildProcessJob::assign(child)?);
        }
        let _ = &mut group;
        #[cfg(not(any(unix, windows)))]
        let _ = child;
        Ok(group)
    }

    #[cfg(unix)]
    async fn signal_and_confirm(&mut self, signal: i32) -> Result<(), PiError> {
        if !self.armed {
            return Ok(());
        }
        if !process_group_has_exited(self.process_group_id)? {
            if let Err(error) = signal_process_group(self.process_group_id, signal) {
                // macOS can reject a signal to a group that just became all
                // zombies. Only a confirmed observation can make that safe.
                if !process_group_has_exited(self.process_group_id)? {
                    return Err(error.into());
                }
            }
            timeout(
                Duration::from_secs(2),
                wait_for_process_group_exit(self.process_group_id),
            )
            .await
            .map_err(|_| PiError::StopTimeout)??;
        }
        self.armed = false;
        Ok(())
    }
}

impl Drop for PiProcessGroup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        #[cfg(unix)]
        {
            if let Some(lease) = self.lease.take() {
                terminate_and_reap_process_group(self.process_group_id, lease);
            } else {
                let _ = signal_process_group(self.process_group_id, libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        if let Some(job) = &self.job {
            let _ = job.terminate();
        }
    }
}

pub struct PiProcess {
    group: PiProcessGroup,
    child: Child,
    stdin: Arc<Mutex<BufWriter<ChildStdin>>>,
    events: broadcast::Sender<PiEvent>,
    reader: JoinHandle<()>,
    next_id: AtomicU64,
    host_context_file: Option<BootstrapFile>,
    broker_token: Option<BrokerTokenHandoff>,
}

impl Drop for PiProcess {
    fn drop(&mut self) {
        self.reader.abort();
        self.host_context_file.take();
        self.broker_token.take();
    }
}

impl PiProcess {
    /// Identifies the Unix process group that owns Pi's derived session files.
    /// A caller must quarantine those files if shutdown cannot confirm this
    /// group has exited.
    #[cfg(unix)]
    pub(crate) const fn process_group_id(&self) -> i32 {
        self.group.process_group_id
    }

    pub async fn spawn(config: PiLaunchConfig) -> Result<Self, PiError> {
        config.validate()?;
        for directory in [
            &config.agent_data_dir,
            &config.working_dir,
            &config.private_run_dir,
        ] {
            ensure_private_directory(directory).map_err(|_| PiError::InvalidLaunchValue)?;
        }
        // Native Pi may spill a truncated shell result. Keep it for this Chat's
        // lifetime rather than placing it in disposable per-turn scratch.
        ensure_private_directory(&config.working_dir.join("temp"))
            .map_err(|_| PiError::InvalidLaunchValue)?;
        if let Some(session) = &config.session {
            ensure_private_directory(&session.directory)
                .map_err(|_| PiError::InvalidLaunchValue)?;
        }
        ensure_pi_project_settings(&config)?;
        let host_context_file = write_host_context_file(&config)?;
        let login_path = shell_path::login_shell_path_for_launch().await;
        let mut environment = config.environment(host_context_file.path(), login_path);
        let broker_token = BrokerTokenHandoff::prepare(&config, &mut environment)?;

        Self::spawn_command(
            &config.executable,
            &config.runtime_dir,
            config.rpc_arguments(),
            &config.runtime_working_dir(),
            environment,
            &config.lease_dir,
            Some(host_context_file),
            Some(broker_token),
        )
        .await
    }

    pub async fn spawn_support(config: PiSupportLaunchConfig) -> Result<Self, PiError> {
        config.validate()?;
        std::fs::create_dir_all(&config.agent_data_dir)?;
        std::fs::create_dir_all(&config.private_working_dir)?;
        Self::spawn_command(
            &config.executable,
            &config.runtime_dir,
            config.rpc_arguments(),
            &config.private_working_dir,
            config.environment(),
            &config.lease_dir,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn spawn_command(
        executable: &Path,
        runtime_dir: &Path,
        arguments: Vec<String>,
        working_dir: &Path,
        environment: BTreeMap<String, String>,
        lease_dir: &Path,
        host_context_file: Option<BootstrapFile>,
        broker_token: Option<BrokerTokenHandoff>,
    ) -> Result<Self, PiError> {
        #[cfg(not(unix))]
        let _ = lease_dir;
        let _verified_runtime = verify_pi_runtime(executable, runtime_dir)?;
        let mut command = pi_command(
            executable,
            arguments,
            working_dir,
            environment,
            broker_token.as_ref(),
        );

        let mut child = command.spawn()?;
        let broker_token = broker_token.and_then(BrokerTokenHandoff::after_spawn);
        let mut group = PiProcessGroup::own(&child)?;
        let _ = &mut group;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "Pi stdin was not created"))?;
        let stdout = child.stdout.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "Pi stdout was not created")
        })?;
        #[cfg(unix)]
        {
            group.lease = Some(ChildProcessLease::register(
                lease_dir,
                ProcessKind::Pi,
                group.process_group_id,
                group.process_group_id,
                executable,
            )?);
        }
        let (events, _) = broadcast::channel(PI_EVENT_BUFFER_CAPACITY);
        let event_writer = events.clone();
        let reader = tokio::spawn(async move {
            let mut stdout = BufReader::new(stdout);
            loop {
                match read_rpc_frame(&mut stdout).await {
                    Ok(Some(frame)) => match serde_json::from_slice(&frame) {
                        Ok(payload) => {
                            #[cfg(any(debug_assertions, feature = "e2e"))]
                            if let Some(diagnostic) = e2e_rpc_diagnostic(&payload) {
                                eprintln!("Loupist E2E Chat diagnostic: {diagnostic}");
                            }
                            let _ = event_writer.send(PiEvent::Rpc { payload });
                        }
                        Err(_) => {
                            let _ = event_writer.send(PiEvent::ProtocolError {
                                message: "Pi emitted malformed JSON".into(),
                            });
                            break;
                        }
                    },
                    Ok(None) => {
                        let _ = event_writer.send(PiEvent::Exited);
                        break;
                    }
                    Err(error) => {
                        let _ = event_writer.send(PiEvent::ProtocolError {
                            message: error.to_string(),
                        });
                        break;
                    }
                }
            }
        });

        Ok(Self {
            group,
            child,
            stdin: Arc::new(Mutex::new(BufWriter::new(stdin))),
            events,
            reader,
            next_id: AtomicU64::new(1),
            host_context_file,
            broker_token,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PiEvent> {
        self.events.subscribe()
    }

    pub fn try_exit_code(&mut self) -> Option<i32> {
        #[cfg(unix)]
        if self.group.armed {
            // Reaping a leader while descendants still execute would release
            // the numeric PID before the final group signal.
            if !process_group_has_exited(self.group.process_group_id).ok()? {
                return None;
            }
            self.group.armed = false;
        }
        self.child
            .try_wait()
            .ok()
            .flatten()
            .and_then(|status| status.code())
    }

    pub async fn prompt(&self, message: &str) -> Result<u64, PiError> {
        self.prompt_with_images(message, &[]).await
    }

    pub async fn prompt_with_images(
        &self,
        message: &str,
        images: &[PiImageContent],
    ) -> Result<u64, PiError> {
        self.send(prompt_request(message, images)?).await
    }

    /// Queue an instruction for the active Pi turn. Unlike `prompt`, Pi accepts
    /// this command while it is streaming and applies it at its next steering
    /// boundary.
    pub async fn steer(&self, message: &str) -> Result<u64, PiError> {
        self.send(text_request("steer", message)?).await
    }

    pub async fn get_state(&self) -> Result<u64, PiError> {
        self.send(json!({ "type": "get_state" })).await
    }

    pub async fn get_entries(&self, since: Option<&str>) -> Result<u64, PiError> {
        let mut request = json!({ "type": "get_entries" });
        if let Some(since) = since {
            bounded_rpc_value(since)?;
            request
                .as_object_mut()
                .ok_or(PiError::InvalidLaunchValue)?
                .insert("since".into(), Value::String(since.to_owned()));
        }
        self.send(request).await
    }

    pub async fn get_available_thinking_levels(&self) -> Result<u64, PiError> {
        self.send(json!({ "type": "get_available_thinking_levels" }))
            .await
    }

    pub async fn abort(&self) -> Result<u64, PiError> {
        self.send(json!({ "type": "abort" })).await
    }

    /// Answers one extension UI request (`ctx.ui.input`) by its string id. Used
    /// only for a sign-in's pasted address or code, which Rust validated first.
    pub async fn respond_extension_ui(&self, request_id: &str, value: &str) -> Result<(), PiError> {
        bounded_rpc_value(request_id)?;
        if value.is_empty() || value.len() > 8 * 1024 || value.contains('\0') {
            return Err(PiError::InvalidLaunchValue);
        }
        let mut encoded = serde_json::to_vec(&json!({
            "type": "extension_ui_response",
            "id": request_id,
            "value": value,
        }))
        .map_err(|_| PiError::MalformedFrame)?;
        encoded.push(b'\n');
        write_rpc_bytes(&self.stdin, &encoded, RPC_WRITE_TIMEOUT).await
    }

    async fn send(&self, mut request: Value) -> Result<u64, PiError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        request
            .as_object_mut()
            .ok_or(PiError::InvalidLaunchValue)?
            .insert("id".into(), Value::from(id));
        let mut encoded = serde_json::to_vec(&request).map_err(|_| PiError::MalformedFrame)?;
        if encoded.len() > MAX_RPC_FRAME_BYTES {
            return Err(PiError::FrameTooLarge);
        }
        encoded.push(b'\n');
        write_rpc_bytes(&self.stdin, &encoded, RPC_WRITE_TIMEOUT).await?;
        Ok(id)
    }

    async fn stop_owned_process(&mut self) -> Result<(), PiError> {
        if self.group.armed {
            #[cfg(unix)]
            {
                self.group.signal_and_confirm(libc::SIGTERM).await?;
            }
            #[cfg(windows)]
            if let Some(job) = &self.group.job {
                job.terminate_and_wait(Duration::from_secs(2)).await?;
            }
            #[cfg(not(any(unix, windows)))]
            {
                self.child.start_kill()?;
            }
            // The unreaped leader still reserves the Unix numeric identity.
            // After group confirmation neither shutdown nor Drop may signal it.
            self.group.armed = false;
        }
        self.reap_leader().await
    }

    async fn force_stop_owned_process(&mut self) -> Result<(), PiError> {
        if self.group.armed {
            #[cfg(unix)]
            {
                self.group.signal_and_confirm(libc::SIGKILL).await?;
            }
            #[cfg(windows)]
            if let Some(job) = &self.group.job {
                job.terminate_and_wait(Duration::from_secs(2)).await?;
            }
            #[cfg(not(any(unix, windows)))]
            {
                self.child.start_kill()?;
            }
            self.group.armed = false;
        }
        self.reap_leader().await
    }

    async fn reap_leader(&mut self) -> Result<(), PiError> {
        timeout(Duration::from_secs(2), self.child.wait())
            .await
            .map_err(|_| PiError::StopTimeout)??;
        Ok(())
    }

    pub async fn shutdown(self, grace: Duration) -> Result<(), PiError> {
        let mut events = self.subscribe();
        let _ = self.abort().await;
        let _ = timeout(grace, async {
            loop {
                match events.recv().await {
                    Ok(PiEvent::Rpc { payload }) if is_agent_settled(&payload) => break,
                    Ok(PiEvent::Exited) | Err(broadcast::error::RecvError::Closed) => break,
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                }
            }
        })
        .await;

        self.finish_shutdown().await
    }

    /// Stops a process only after its session-level `agent_settled` boundary.
    /// Sending a late abort here can append a second terminal mutation after
    /// the host has read the cursor that it is about to seal in SQLite.
    pub async fn shutdown_settled(self) -> Result<(), PiError> {
        self.finish_shutdown().await
    }

    async fn finish_shutdown(mut self) -> Result<(), PiError> {
        let stop_result = match self.stop_owned_process().await {
            Ok(()) => Ok(()),
            Err(_) => self.force_stop_owned_process().await,
        };
        self.reader.abort();
        let _ = timeout(Duration::from_secs(2), &mut self.reader).await;
        self.host_context_file.take();
        self.broker_token.take();
        stop_result?;
        #[cfg(windows)]
        self.group.job.take();
        #[cfg(unix)]
        if let Some(lease) = self.group.lease.take() {
            lease.complete()?;
        }
        Ok(())
    }
}

fn text_request(command: &'static str, message: &str) -> Result<Value, PiError> {
    if !matches!(command, "prompt" | "steer")
        || message.trim().is_empty()
        || message.len() > 512 * 1024
        || message.contains('\0')
    {
        return Err(PiError::InvalidLaunchValue);
    }
    Ok(json!({ "type": command, "message": message }))
}

fn prompt_request(message: &str, images: &[PiImageContent]) -> Result<Value, PiError> {
    if images.len() > 4
        || images.iter().any(|image| {
            image.data.is_empty()
                || image.data.len() > 7 * 1024 * 1024
                || !matches!(
                    image.mime_type.as_str(),
                    "image/jpeg" | "image/png" | "image/gif" | "image/webp"
                )
        })
    {
        return Err(PiError::InvalidLaunchValue);
    }
    let mut request = text_request("prompt", message)?;
    if !images.is_empty() {
        request
            .as_object_mut()
            .ok_or(PiError::InvalidLaunchValue)?
            .insert(
                "images".into(),
                Value::Array(
                    images
                        .iter()
                        .map(|image| {
                            json!({
                                "type": "image",
                                "data": image.data,
                                "mimeType": image.mime_type,
                            })
                        })
                        .collect(),
                ),
            );
    }
    Ok(request)
}

fn bounded_rpc_value(value: &str) -> Result<(), PiError> {
    if value.is_empty() || value.len() > 512 || value.contains('\0') {
        Err(PiError::InvalidLaunchValue)
    } else {
        Ok(())
    }
}

async fn write_rpc_bytes<W>(
    writer: &Mutex<W>,
    bytes: &[u8],
    deadline: Duration,
) -> Result<(), PiError>
where
    W: AsyncWrite + Unpin,
{
    timeout(deadline, async {
        let mut writer = writer.lock().await;
        writer.write_all(bytes).await?;
        writer.flush().await
    })
    .await
    .map_err(|_| PiError::WriteTimeout)??;
    Ok(())
}

fn is_agent_settled(payload: &Value) -> bool {
    payload.get("type").and_then(Value::as_str) == Some("agent_settled")
}

async fn read_rpc_frame<R>(reader: &mut R) -> Result<Option<Vec<u8>>, PiError>
where
    R: AsyncBufRead + Unpin,
{
    let mut frame = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err(PiError::MalformedFrame)
            };
        }
        let (take, complete) = match available.iter().position(|byte| *byte == b'\n') {
            Some(position) => (position, true),
            None => (available.len(), false),
        };
        if frame.len() + take > MAX_RPC_FRAME_BYTES {
            return Err(PiError::FrameTooLarge);
        }
        frame.extend_from_slice(&available[..take]);
        reader.consume(take + usize::from(complete));
        if complete {
            if frame.last() == Some(&b'\r') {
                frame.pop();
            }
            if frame.is_empty() {
                return Err(PiError::MalformedFrame);
            }
            return Ok(Some(frame));
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(test, not(feature = "eval-harness")))]
mod production_asset_isolation_tests {
    use super::*;

    #[test]
    fn production_ignores_unregistered_agent_assets_and_uses_compiled_prompt() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../agent");
        // The production extension's exact bytes are valid; only SYSTEM is changed.
        let config = PiLaunchConfig {
            extension: source.join("loupist-extension.mjs"),
            system_prompt: root.join("SYSTEM.md"),
            agent_data_dir: root.join("agent"),
            working_dir: root.join("cwd"),
            private_run_dir: root.join("run"),
            ..tests::launch_config()
        };
        std::fs::write(&config.system_prompt, "An attempted override").unwrap();
        assert!(matches!(
            config.validate(),
            Err(PiError::MissingExtension(_))
        ));
        let args = config.rpc_arguments();
        let index = args
            .iter()
            .position(|arg| arg == "--system-prompt")
            .unwrap();
        assert_eq!(args[index + 1], LOUPIST_SYSTEM_PROMPT);
    }
}
