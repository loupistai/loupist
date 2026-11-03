//! Pi process launch for one Chat turn: warm/cold resume selection, startup
//! cancellation, confirmed shutdown of failed launches and cache seals.
use super::*;

#[allow(clippy::large_enum_variant)]
pub(super) enum ChatPiResume {
    Cold { session_id: String },
    Warm { cache: PiSessionCache },
}

impl ChatPiResume {
    fn session_id(&self) -> &str {
        match self {
            Self::Cold { session_id } => session_id,
            Self::Warm { cache } => &cache.derived_session_id,
        }
    }
}

pub(super) fn select_chat_pi_resume(
    intended_cache: Option<PiSessionCache>,
    had_prior_messages: bool,
    chat_seed_session_id: &str,
    fresh_cold_session_id: impl FnOnce() -> String,
) -> ChatPiResume {
    match intended_cache {
        Some(cache) => ChatPiResume::Warm { cache },
        None => ChatPiResume::Cold {
            // The first turn may retain the immutable local Chat seed. Every
            // later cold rebuild receives a fresh provider/cache identity.
            session_id: if had_prior_messages {
                fresh_cold_session_id()
            } else {
                chat_seed_session_id.to_owned()
            },
        },
    }
}

#[cfg(test)]
mod chat_pi_resume_tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn changed_scoped_settings_or_global_instructions_rebuild_the_session_cache() {
        let global = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let seal = || {
            let config = crate::scoped_config::resolve(global.path(), None).unwrap();
            let resources = agent_harness::workspace::workspace_snapshot_with_preferences(
                workspace.path(),
                global.path(),
                &BTreeMap::new(),
            )
            .unwrap();
            pi_session_cache_runtime_policy_sha256(
                None,
                crate::web::WebSearchPolicy::default(),
                &resources.digest,
                &config.execution_digest(),
            )
            .unwrap()
        };
        let original = seal();
        assert_eq!(original, seal());
        std::fs::write(
            global.path().join("config.toml"),
            "[pi.retry]\nenabled = false\n",
        )
        .unwrap();
        let config_changed = seal();
        assert_ne!(original, config_changed);
        std::fs::write(
            global.path().join("AGENTS.md"),
            "Use monthly reporting periods.",
        )
        .unwrap();
        assert_ne!(config_changed, seal());
    }

    #[test]
    fn resume_reuses_only_a_matching_warm_cache_and_rotates_cold_ids_after_history() {
        const SEED: &str = "11111111-1111-4111-8111-111111111111";
        const FRESH: &str = "22222222-2222-4222-8222-222222222222";
        const CACHED: &str = "33333333-3333-4333-8333-333333333333";
        let cache = PiSessionCache {
            entries_sha256: "a".repeat(64),
            leaf_id: Some("leaf".into()),
            harness_digest: "b".repeat(64),
            runtime_policy_sha256: "c".repeat(64),
            runtime_surface_sha256: "d".repeat(64),
            connector_authority_sha256: "e".repeat(64),
            memory_access_enabled: true,
            memory_update_enabled: true,
            derived_session_id: CACHED.into(),
            context_envelope_sha256: String::new(),
            context_envelope_stale: false,
            execution_model: ExecutionModelLock {
                profile_id: "profile".into(),
                name: "model".into(),
                provider: "provider".into(),
                model: "model".into(),
                thinking_level: "off".into(),
                run_options: BTreeMap::new(),
            },
        };
        let resume = |authority: &str, had_history: bool| {
            let scope = PiSessionCacheScope {
                harness_digest: &cache.harness_digest,
                runtime_policy_sha256: &cache.runtime_policy_sha256,
                runtime_surface_sha256: &cache.runtime_surface_sha256,
                connector_authority_sha256: authority,
                memory_access_enabled: true,
                memory_update_enabled: true,
                execution_model: &cache.execution_model,
            };
            let intended = cache.matches(&scope).then(|| cache.clone());
            select_chat_pi_resume(intended, had_history, SEED, || FRESH.into())
                .session_id()
                .to_owned()
        };
        // A changed connector authority must never reuse the provider cache.
        assert_eq!(resume(&"e".repeat(64), true), CACHED);
        assert_eq!(resume(&"f".repeat(64), true), FRESH);
        assert_eq!(resume(&"f".repeat(64), false), SEED);
    }
}

/// A failed warm launch may be retried cold only after its Pi child is known
/// to have exited. Wiping the session directory while an unconfirmed child is
/// still alive could let it recreate JSONL after the new cold launch begins.
enum ChatPiLaunchFailure {
    Cancelled,
    Stopped(CommandError),
    StopUnconfirmed {
        error: CommandError,
        process_group_id: Option<i32>,
    },
}

impl ChatPiLaunchFailure {
    fn unconfirmed(error: CommandError) -> Self {
        Self::StopUnconfirmed {
            error,
            process_group_id: None,
        }
    }
}

#[cfg(unix)]
pub(super) fn pi_process_group_id(pi: &PiProcess) -> Option<i32> {
    Some(pi.process_group_id())
}

#[cfg(not(unix))]
pub(super) fn pi_process_group_id(_pi: &PiProcess) -> Option<i32> {
    None
}

pub(super) fn record_unconfirmed_pi_stop(
    session: &ChatExecutionSession,
    process_group_id: Option<i32>,
) -> Result<(), CommandError> {
    #[cfg(unix)]
    if let Some(process_group_id) = process_group_id {
        session.record_unconfirmed_pi_stop(process_group_id)?;
    }
    #[cfg(not(unix))]
    let _ = (session, process_group_id);
    Ok(())
}

fn finish_failed_chat_setup(
    session: &mut ChatExecutionSession,
    process_group_id: Option<i32>,
    shutdown: Result<(), PiError>,
) -> Result<(), CommandError> {
    if shutdown.is_ok() {
        session.wipe()
    } else {
        record_unconfirmed_pi_stop(session, process_group_id)
    }
}

pub(super) async fn shutdown_failed_chat_setup(
    pi: PiProcess,
    session: &mut ChatExecutionSession,
) -> Result<(), CommandError> {
    let process_group_id = pi_process_group_id(&pi);
    let shutdown = pi.shutdown(Duration::from_secs(1)).await;
    finish_failed_chat_setup(session, process_group_id, shutdown)
}

#[cfg(all(test, unix))]
mod failed_chat_setup_tests {
    use super::*;
    use crate::fs::secure_delete::SecureDeletionRoot;

    #[test]
    fn failed_setup_keeps_its_cache_until_pi_is_known_to_have_stopped() {
        let temporary = tempfile::tempdir().unwrap();
        let root =
            Arc::new(SecureDeletionRoot::open(&temporary.path().canonicalize().unwrap()).unwrap());
        for (chat_id, shutdown) in [("chat-a", Err(PiError::StopTimeout)), ("chat-b", Ok(()))] {
            let confirmed = shutdown.is_ok();
            let chat = crate::test_fixtures::chat_session(chat_id, "loupist-a");
            let mut session = ChatExecutionSession::prepare(root.clone(), &chat).unwrap();
            let cache = session.session_directory().join("session.jsonl");
            std::fs::write(&cache, "unfinished cache").unwrap();

            finish_failed_chat_setup(&mut session, Some(424_242), shutdown).unwrap();
            drop(session);

            assert_eq!(cache.exists(), !confirmed);
            let mut next_session = ChatExecutionSession::prepare(root.clone(), &chat).unwrap();
            // An unconfirmed child could still recreate the cache after a wipe.
            assert_eq!(next_session.wipe().is_ok(), confirmed);
        }
    }
}

async fn stopped_launch_failure(pi: PiProcess, error: CommandError) -> ChatPiLaunchFailure {
    #[cfg(feature = "e2e")]
    crate::pi::trace_e2e_failure("launch", &error.message);
    let process_group_id = pi_process_group_id(&pi);
    if pi.shutdown(Duration::from_secs(1)).await.is_ok() {
        ChatPiLaunchFailure::Stopped(error)
    } else {
        ChatPiLaunchFailure::StopUnconfirmed {
            error,
            process_group_id,
        }
    }
}

fn chat_startup_cancelled(cancel: &tokio::sync::watch::Receiver<bool>) -> bool {
    *cancel.borrow() || cancel.has_changed().is_err()
}

fn chat_startup_cancelled_error() -> CommandError {
    CommandError::new("chat_cancelled", "Chat startup was stopped")
}

async fn stopped_cancelled_launch(pi: PiProcess) -> ChatPiLaunchFailure {
    let process_group_id = pi_process_group_id(&pi);
    if pi.shutdown(Duration::from_secs(1)).await.is_ok() {
        ChatPiLaunchFailure::Cancelled
    } else {
        ChatPiLaunchFailure::StopUnconfirmed {
            error: chat_startup_cancelled_error(),
            process_group_id,
        }
    }
}

async fn launch_chat_pi(
    config: PiLaunchConfig,
    execution: &PiExecutionConfig,
    session: &ChatExecutionSession,
    resume: &ChatPiResume,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<
    (
        PiProcess,
        tokio::sync::broadcast::Receiver<PiEvent>,
        ExecutionModelLock,
        PiSessionState,
    ),
    ChatPiLaunchFailure,
> {
    if chat_startup_cancelled(cancel) {
        return Err(ChatPiLaunchFailure::Cancelled);
    }
    session
        .validate_current_binding()
        .map_err(ChatPiLaunchFailure::unconfirmed)?;
    let config = bind_chat_pi_session(config, session, resume.session_id()).map_err(|error| {
        ChatPiLaunchFailure::unconfirmed(CommandError::new("pi_unavailable", error.to_string()))
    })?;
    let pi = PiProcess::spawn(config).await.map_err(|error| {
        ChatPiLaunchFailure::unconfirmed(CommandError::new("pi_unavailable", error.to_string()))
    })?;
    let mut events = pi.subscribe();
    let file_requirement = match resume {
        ChatPiResume::Cold { .. } => PiSessionFileRequirement::ColdMayBeUnpersisted,
        ChatPiResume::Warm { .. } => PiSessionFileRequirement::Persisted,
    };
    // Retain ownership of Pi when Stop wins. Dropping a launch future alone
    // cannot attest process-group exit or clear its durable process lease.
    let configured = tokio::select! {
        biased;
        _ = cancel.wait_for(|cancelled| *cancelled) => None,
        configured = configure_pi_session_and_read_entries(
            &pi,
            &mut events,
            execution,
            resume.session_id(),
            session.session_directory(),
            file_requirement,
        ) => Some(configured),
    };
    let Some(configured) = configured else {
        return Err(stopped_cancelled_launch(pi).await);
    };
    match configured {
        Ok((model, state, entries)) => {
            let acceptable = match resume {
                ChatPiResume::Cold { .. } => entries.cold_startup_only,
                ChatPiResume::Warm { cache } => entries.matches_cache(cache),
            };
            if !acceptable {
                return Err(stopped_launch_failure(
                    pi,
                    CommandError::new(
                        "pi_unavailable",
                        match resume {
                            ChatPiResume::Cold { .. } => {
                                "cold Pi session loaded unexpected prior context"
                            }
                            ChatPiResume::Warm { .. } => "warm Pi session cache digest mismatched",
                        },
                    ),
                )
                .await);
            }
            Ok((pi, events, model, state))
        }
        Err(error) => Err(stopped_launch_failure(pi, error).await),
    }
}

pub(super) async fn launch_chat_pi_resuming(
    config: PiLaunchConfig,
    execution: &PiExecutionConfig,
    session: &mut ChatExecutionSession,
    resume: ChatPiResume,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<
    (
        PiProcess,
        tokio::sync::broadcast::Receiver<PiEvent>,
        ExecutionModelLock,
        PiSessionState,
        bool,
    ),
    CommandError,
> {
    if chat_startup_cancelled(cancel) {
        return Err(chat_startup_cancelled_error());
    }
    // A previous launch that could not prove its process group exited leaves
    // a durable sentinel. Resolve it before either warm reuse or a cold wipe.
    session.resolve_unconfirmed_pi_stops().await?;
    if chat_startup_cancelled(cancel) {
        return Err(chat_startup_cancelled_error());
    }
    let cold_resume = match resume {
        ChatPiResume::Warm { cache } => {
            let warm_resume = ChatPiResume::Warm { cache };
            match launch_chat_pi(config.clone(), execution, session, &warm_resume, cancel).await {
                Ok((pi, events, model, state)) => return Ok((pi, events, model, state, true)),
                Err(ChatPiLaunchFailure::Cancelled) => return Err(chat_startup_cancelled_error()),
                Err(ChatPiLaunchFailure::Stopped(_)) => {
                    // Stop may arrive while a failed warm child is draining.
                    // A cancelled setup never replays initialization cold.
                    if chat_startup_cancelled(cancel) {
                        return Err(chat_startup_cancelled_error());
                    }
                    session.wipe()?;
                    ChatPiResume::Cold {
                        session_id: uuid::Uuid::new_v4().to_string(),
                    }
                }
                Err(ChatPiLaunchFailure::StopUnconfirmed {
                    error,
                    process_group_id,
                }) => {
                    record_unconfirmed_pi_stop(session, process_group_id)?;
                    return Err(error);
                }
            }
        }
        ChatPiResume::Cold { session_id } => {
            session.wipe()?;
            ChatPiResume::Cold { session_id }
        }
    };
    let (pi, events, model, state) =
        match launch_chat_pi(config, execution, session, &cold_resume, cancel).await {
            Ok(launched) => launched,
            Err(ChatPiLaunchFailure::Cancelled) => return Err(chat_startup_cancelled_error()),
            Err(ChatPiLaunchFailure::Stopped(error)) => return Err(error),
            Err(ChatPiLaunchFailure::StopUnconfirmed {
                error,
                process_group_id,
            }) => {
                record_unconfirmed_pi_stop(session, process_group_id)?;
                return Err(error);
            }
        };
    Ok((pi, events, model, state, false))
}

pub(super) fn cache_from_entries(
    entries: PiEntriesState,
    scope: &PiSessionCacheScope<'_>,
    derived_session_id: &str,
) -> Option<PiSessionCache> {
    let cache = PiSessionCache {
        entries_sha256: entries.entries_sha256,
        leaf_id: entries.leaf_id,
        harness_digest: scope.harness_digest.to_owned(),
        runtime_policy_sha256: scope.runtime_policy_sha256.to_owned(),
        runtime_surface_sha256: scope.runtime_surface_sha256.to_owned(),
        connector_authority_sha256: scope.connector_authority_sha256.to_owned(),
        memory_access_enabled: scope.memory_access_enabled,
        memory_update_enabled: scope.memory_update_enabled,
        derived_session_id: derived_session_id.to_owned(),
        context_envelope_sha256: String::new(),
        context_envelope_stale: false,
        execution_model: scope.execution_model.clone(),
    };
    cache.validate().ok().map(|_| cache)
}

/// Execution-policy inputs which influence what existing Pi JSONL tool
/// results are safe to retain. The stable runtime/component surface and the
/// exact Memory authority profile are sealed separately.
#[derive(Serialize)]
struct PiSessionCacheRuntimePolicy<'a> {
    version: &'static str,
    pi_version: &'static str,
    as_of: Option<&'a str>,
    web_search_policy: PiSessionCacheWebSearchPolicy,
    workspace_digest: &'a str,
    execution_settings_digest: &'a str,
}

#[derive(Clone, Copy)]
enum PiSessionCacheWebSearchPolicy {
    Automatic,
    ModelOnly,
    ExaOnly,
}

impl From<crate::web::WebSearchPolicy> for PiSessionCacheWebSearchPolicy {
    fn from(policy: crate::web::WebSearchPolicy) -> Self {
        match policy {
            crate::web::WebSearchPolicy::Automatic => Self::Automatic,
            crate::web::WebSearchPolicy::ModelOnly => Self::ModelOnly,
            crate::web::WebSearchPolicy::ExaOnly => Self::ExaOnly,
        }
    }
}

impl Serialize for PiSessionCacheWebSearchPolicy {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match self {
            Self::Automatic => "automatic",
            Self::ModelOnly => "model_only",
            Self::ExaOnly => "exa_only",
        })
    }
}

pub(super) fn pi_session_cache_runtime_policy_sha256(
    as_of: Option<&str>,
    web_search_policy: crate::web::WebSearchPolicy,
    workspace_digest: &str,
    execution_settings_digest: &str,
) -> Result<String, CommandError> {
    let policy = PiSessionCacheRuntimePolicy {
        version: crate::pi::PI_SESSION_CACHE_POLICY_VERSION,
        pi_version: crate::pi::PI_VERSION,
        as_of,
        web_search_policy: web_search_policy.into(),
        workspace_digest,
        execution_settings_digest,
    };
    let serialized = serde_json::to_vec(&policy).map_err(map_internal)?;
    Ok(crate::util::sha256(&serialized))
}

pub(super) fn pi_session_cache_runtime_surface_sha256(
    capability_ids: &[String],
) -> Result<String, CommandError> {
    // `true, true` yields the complete Memory-capable core surface. Per-turn
    // Memory authority is sealed as an exact cache field, while this digest
    // keeps every component binding exact and rejects removed tool results.
    let surface = agent_harness::AgentRuntimeProfile::new("chat", true, true, capability_ids)
        .map_err(map_internal)?;
    let serialized = serde_json::to_vec(&surface).map_err(map_internal)?;
    Ok(crate::util::sha256(&serialized))
}

pub(in crate::commands) fn bind_chat_pi_session(
    config: PiLaunchConfig,
    session: &ChatExecutionSession,
    session_id: &str,
) -> Result<PiLaunchConfig, PiError> {
    config.with_session(session.pi_config_with_id(session_id))
}

/// Pi's CLI starts migrations before it applies project trust. Keep its runtime
/// CWD private so opening a user's folder cannot rewrite their `.pi` settings.
/// The app extension binds native tool execution and Pi's prompt CWD to the
/// separately snapshotted workspace root; session JSONL stays app-owned.
pub(in crate::commands) fn chat_pi_launch_config(
    pi_artifacts: &crate::app::PiArtifacts,
    app_data_dir: &Path,
    loupist_id: &str,
    run_id: &str,
    broker_socket: std::path::PathBuf,
    broker_token: String,
    session: &ChatExecutionSession,
) -> PiLaunchConfig {
    pi_artifacts.launch_config(
        app_data_dir,
        loupist_id,
        run_id,
        session.runtime_working_directory().to_path_buf(),
        broker_socket,
        broker_token,
    )
}
