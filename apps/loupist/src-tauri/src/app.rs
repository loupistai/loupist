#[cfg(any(test, feature = "e2e", not(target_os = "macos")))]
use std::env;
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
#[cfg(not(windows))]
use std::fs::OpenOptions;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use tauri::{AppHandle, Manager};

#[cfg(windows)]
use crate::fs::windows::{filesystem_identity, open_regular_no_reparse};
use crate::{
    chat::control::{AcceptedChatControl, ChatControlHandle},
    finance_data::FinanceDataService,
    fs::artifact_trust::{ensure_private_directory, ensure_private_regular_file},
    fs::secure_delete::SecureDeletionRoot,
    pi::execution::PiExecutionConfig,
    pi::PiLaunchConfig,
    process_lease::{prepare_lease_directory, recover_orphaned_processes},
    providers::catalog::{
        catalog_view, provider_credential_from_environment, ConfiguredModel, ModelCatalog,
        ModelCatalogView, ModelVisibility,
    },
    providers::connection::ProviderModelDiscoveryCache,
    providers::support::ProviderSupportCoordinator,
    runs::coordinator::{
        PendingMemoryWrite, RunCoordinator, RunKind, RunRegistration, RunSpec, RunTarget,
    },
    runs::maintenance::MaintenanceCoordinator,
    store::deletion,
    store::{LoupistStore, SqliteStore},
    updater::UpdateCoordinator,
    web::browser::BrowserManager,
};

const APP_INSTANCE_CONFLICT_CODE: &str = "app_instance_conflict";
#[cfg(any(not(feature = "e2e"), test))]
const PRODUCTION_APP_IDENTIFIER: &str = crate::DEVELOPMENT_APP_IDENTIFIER;
#[cfg(any(test, feature = "e2e"))]
pub(crate) const LIVE_PI_AGENT_DATA_DIR_ENV: &str = "LOUPIST_LIVE_PI_AGENT_DATA_DIR";

#[derive(Clone, Debug, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn unavailable(component: &str) -> Self {
        Self::new(
            format!("{component}_unavailable"),
            format!("{component} is not available in this Loupist build"),
        )
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_request", message)
    }

    pub fn not_found(kind: &str) -> Self {
        Self::new("not_found", format!("{kind} was not found"))
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new("conflict", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", message)
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for CommandError {}

#[derive(Clone)]
pub struct PiArtifacts {
    pub executable: PathBuf,
    pub runtime_dir: PathBuf,
    pub extension: PathBuf,
    pub provider_extension: PathBuf,
    pub system_prompt: PathBuf,
    pub provider: String,
    pub model: String,
    pub thinking_level: String,
    pub run_options: std::collections::BTreeMap<String, String>,
    pub provider_credential: Option<(String, String)>,
}

impl PiArtifacts {
    pub fn launch_config(
        &self,
        app_data_dir: &Path,
        loupist_id: &str,
        run_id: &str,
        working_dir: PathBuf,
        broker_socket: PathBuf,
        broker_token: String,
    ) -> PiLaunchConfig {
        PiLaunchConfig {
            executable: self.executable.clone(),
            runtime_dir: self.runtime_dir.clone(),
            extension: self.extension.clone(),
            system_prompt: self.system_prompt.clone(),
            // Pi authentication and provider settings are app-scoped. Chat
            // supplies a stable, session-private CWD; run-private capability
            // files remain isolated below the separate private run directory.
            agent_data_dir: app_data_dir.join("pi"),
            working_dir,
            private_run_dir: app_data_dir.join("runs").join(loupist_id).join(run_id),
            lease_dir: app_data_dir.join("process-leases"),
            broker_socket,
            broker_token,
            provider: self.provider.clone(),
            model: self.model.clone(),
            thinking_level: self.thinking_level.clone(),
            run_options: self.run_options.clone(),
            provider_credential: self.provider_credential.clone(),
            pi_settings: Default::default(),
            host_context: None,
            skill_files: Vec::new(),
            session: None,
        }
    }
}

#[derive(Clone)]
pub struct AppArtifacts {
    pub app_data_dir: PathBuf,
    pub broker_dir: PathBuf,
    pub process_lease_dir: PathBuf,
    pub deletion_root: Arc<SecureDeletionRoot>,
    pub pi: Option<PiArtifacts>,
    pub mcp_runtimes: BTreeMap<String, crate::mcp::BundledMcpRuntime>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum QuarantineSource {
    Deletion,
    MemoryWrite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LoupistAvailability {
    Available,
    WorkspaceUnavailable,
    /// The folder was made by an earlier build; it is refused, never migrated.
    EarlierVersion,
    RecoveryRequired {
        reason: LoupistRecoveryReason,
        action: LoupistRecoveryAction,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoupistRecoveryAction {
    RecoverMemory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoupistRecoveryReason {
    InterruptedMemoryUpdate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LoupistAccess {
    Visible(LoupistAvailability),
    Hidden,
}

pub(crate) struct AppInstanceLock {
    _file: File,
}

impl Drop for AppInstanceLock {
    fn drop(&mut self) {
        // A forked child can briefly retain this open description before exec.
        // Release ownership instead of waiting for every copy to close.
        let _ = FileExt::unlock(&self._file);
    }
}

impl AppInstanceLock {
    pub(crate) fn acquire(app_data_dir: &Path) -> Result<Self, CommandError> {
        let lock_path = app_data_dir.join("loupist.instance.lock");
        #[cfg(windows)]
        if matches!(std::fs::symlink_metadata(&lock_path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        {
            ensure_private_regular_file(&lock_path)
                .map_err(|error| CommandError::internal(error.to_string()))?;
        }
        #[cfg(not(windows))]
        ensure_private_regular_file(&lock_path)
            .map_err(|error| CommandError::internal(error.to_string()))?;
        #[cfg(windows)]
        let file = {
            let file = open_regular_no_reparse(&lock_path)
                .map_err(|error| CommandError::internal(error.to_string()))?;
            let reopened = open_regular_no_reparse(&lock_path)
                .map_err(|error| CommandError::internal(error.to_string()))?;
            let identity = filesystem_identity(&file)
                .map_err(|error| CommandError::internal(error.to_string()))?;
            let reopened_identity = filesystem_identity(&reopened)
                .map_err(|error| CommandError::internal(error.to_string()))?;
            if identity != reopened_identity {
                return Err(CommandError::internal(
                    "instance lock path changed while opening",
                ));
            }
            file
        };
        #[cfg(not(windows))]
        let file = {
            let mut options = OpenOptions::new();
            options.truncate(false).read(true).write(true);
            #[cfg(unix)]
            options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
            options
                .open(lock_path)
                .map_err(|error| CommandError::internal(error.to_string()))?
        };
        file.try_lock_exclusive().map_err(|error| {
            CommandError::new(
                APP_INSTANCE_CONFLICT_CODE,
                format!("another Loupist instance already owns this app data ({error})"),
            )
        })?;
        Ok(Self { _file: file })
    }
}

pub(crate) fn is_app_instance_conflict(error: &CommandError) -> bool {
    error.code == APP_INSTANCE_CONFLICT_CODE
}

#[derive(Clone)]
pub struct AppState {
    #[cfg(feature = "eval-harness")]
    pub(crate) eval_pi_agent_dir: Option<PathBuf>,
    pub(crate) scheduled: crate::scheduled_tasks::ScheduledSupervisor,
    pub store: Arc<SqliteStore>,
    pub(crate) credential_environment: Arc<str>,
    pub artifacts: Arc<AppArtifacts>,
    pub(crate) mcp_pool: crate::mcp::pool::McpProcessPool,
    pub finance_data: Arc<FinanceDataService>,
    pub run_coordinator: RunCoordinator,
    pub provider_support: ProviderSupportCoordinator,
    pub(crate) provider_model_cache: ProviderModelDiscoveryCache,
    pub maintenance: MaintenanceCoordinator,
    pub updates: UpdateCoordinator,
    pub browser: BrowserManager,
    quarantined_loupists: Arc<RwLock<HashMap<String, HashMap<QuarantineSource, String>>>>,
    _instance_lock: Arc<AppInstanceLock>,
    model_catalog: Arc<RwLock<ModelCatalog>>,
    pub(crate) fresh_install: bool,
    pub graph: crate::knowledge::GraphSupervisor,
    /// Writes what the user follows to every Loupist once a burst of follows settles.
    pub(crate) followed: crate::commands::knowledge::FollowedSync,
}

impl AppState {
    #[cfg(feature = "eval-harness")]
    pub(crate) fn for_eval(app_data_dir: PathBuf) -> Self {
        Self::for_test_with_store(app_data_dir, true, true, None)
    }
    pub(crate) fn loupist_files_root(
        &self,
        loupist_id: &str,
    ) -> Result<Arc<SecureDeletionRoot>, CommandError> {
        if !self.store.is_folder_catalog() {
            return Ok(self.artifacts.deletion_root.clone());
        }
        let dir = self
            .store
            .loupist_data_dir(loupist_id)
            .map_err(crate::commands::map_store)?;
        Ok(Arc::new(SecureDeletionRoot::open(&dir)?))
    }

    pub fn initialize(app: &AppHandle) -> Result<Self, CommandError> {
        #[cfg(not(feature = "e2e"))]
        let user_home = app
            .path()
            .home_dir()
            .map_err(|error| CommandError::internal(error.to_string()))?;

        #[cfg(feature = "e2e")]
        let requested_app_data_dir = {
            if app.config().identifier != crate::E2E_APP_IDENTIFIER {
                return Err(CommandError::internal(
                    "the E2E build requires the isolated E2E app identifier",
                ));
            }
            e2e_app_data_dir(env::var_os("LOUPIST_E2E_APP_DATA_DIR").map(PathBuf::from))?
        };
        #[cfg(not(feature = "e2e"))]
        let requested_app_data_dir =
            app_data_dir_for_build(user_home, app.config().identifier.as_str());
        let app_data_dir = pin_app_data_dir(&requested_app_data_dir)?;
        let instance_lock = Arc::new(AppInstanceLock::acquire(&app_data_dir)?);

        let process_lease_dir = app_data_dir.join("process-leases");
        ensure_private_directory(&process_lease_dir)
            .map_err(|error| CommandError::internal(error.to_string()))?;
        prepare_lease_directory(&process_lease_dir)
            .and_then(|()| recover_orphaned_processes(&process_lease_dir))
            .map_err(|error| CommandError::internal(error.to_string()))?;

        for child in ["browser-profile", "loupists", "mcp-pool", "pi", "runs"] {
            ensure_private_directory(&app_data_dir.join(child))
                .map_err(|error| CommandError::internal(error.to_string()))?;
        }
        let mcp_pool = crate::mcp::pool::McpProcessPool::prepare(app_data_dir.join("mcp-pool"))
            .map_err(CommandError::internal)?;
        let deletion_root = Arc::new(SecureDeletionRoot::open(&app_data_dir)?);
        crate::runs::scratch::sweep_stale_runs(&deletion_root)?;
        let broker_dir = transient_broker_directory();
        ensure_private_directory(&broker_dir)
            .map_err(|error| CommandError::internal(error.to_string()))?;

        let database_path = app_data_dir.join("catalog.sqlite3");
        let (store, fresh_catalog) = open_app_store(&database_path)?;
        let credential_environment = store
            .initialize_host_environment()
            .map_err(crate::commands::map_store)?;
        let fresh_install = fresh_catalog;
        let deletion_recovery = deletion::recover(&store, &deletion_root)?;
        let model_catalog = store
            .get_model_catalog()
            .map_err(|error| CommandError::internal(error.to_string()))?
            .unwrap_or_default();
        let resource_dir = app.path().resource_dir().ok();
        #[cfg(debug_assertions)]
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

        let pi_executable_candidates = resource_dir
            .iter()
            .map(|directory| {
                directory
                    .join("pi-runtime")
                    .join(platform_binary("loupist-pi"))
            })
            .collect::<Vec<_>>();
        #[cfg(debug_assertions)]
        let pi_executable_candidates = {
            let mut candidates = pi_executable_candidates;
            candidates.extend([manifest_dir
                .join("resources/pi-runtime")
                .join(platform_binary("loupist-pi"))]);
            candidates
        };
        let pi_executable = resolve_file(pi_executable_candidates);

        let pi_runtime_candidates = resource_dir
            .iter()
            .map(|directory| directory.join("pi-runtime"))
            .collect::<Vec<_>>();
        #[cfg(debug_assertions)]
        let pi_runtime_candidates = {
            let mut candidates = pi_runtime_candidates;
            candidates.push(manifest_dir.join("resources/pi-runtime"));
            candidates
        };
        let pi_runtime_dir = resolve_directory(pi_runtime_candidates);
        let mcp_runtimes = pi_runtime_dir
            .as_deref()
            .map(|root| crate::mcp::discover_bundled_runtimes(root, &process_lease_dir))
            .unwrap_or_default();

        let agent_candidates = resource_dir
            .iter()
            .map(|directory| directory.join("loupist-agent"))
            .collect::<Vec<_>>();
        #[cfg(debug_assertions)]
        let agent_candidates = {
            let mut candidates = agent_candidates;
            candidates.push(manifest_dir.join("../agent"));
            candidates
        };
        let agent_resource = resolve_directory(agent_candidates);
        let pi = resolve_pi_artifacts(pi_executable, pi_runtime_dir, agent_resource);

        let finance_data = Arc::new(
            FinanceDataService::new()
                .map_err(|error| CommandError::internal(error.to_string()))?
                .with_krx_daily_cache(app_data_dir.join("cache").join("krx-daily")),
        );
        let store = Arc::new(store);
        let maintenance = MaintenanceCoordinator::default();
        let updates = UpdateCoordinator::new(store.clone())?;
        let quarantined_loupists = deletion_recovery
            .quarantined_loupists
            .into_iter()
            .map(|(loupist_id, reason)| {
                (
                    loupist_id,
                    HashMap::from([(QuarantineSource::Deletion, reason)]),
                )
            })
            .collect::<HashMap<_, _>>();
        let artifacts = Arc::new(AppArtifacts {
            broker_dir,
            process_lease_dir,
            deletion_root,
            app_data_dir: app_data_dir.clone(),
            pi,
            mcp_runtimes,
        });
        let provider_support = ProviderSupportCoordinator::new(maintenance.clone());
        let inference = crate::providers::inference::ProviderInferenceService::from_parts(
            artifacts.clone(),
            inference_agent_data_dir(&app_data_dir)?,
            maintenance.clone(),
            store.clone(),
        );
        let state = Self {
            #[cfg(feature = "eval-harness")]
            eval_pi_agent_dir: None,
            scheduled: crate::scheduled_tasks::ScheduledSupervisor::default(),
            store: store.clone(),
            credential_environment: credential_environment.into(),
            artifacts,
            mcp_pool,
            finance_data,
            run_coordinator: RunCoordinator::new(maintenance.clone()),
            provider_support,
            provider_model_cache: ProviderModelDiscoveryCache::default(),
            maintenance: maintenance.clone(),
            updates,
            browser: BrowserManager::new(app_data_dir.join("browser-profile")),
            quarantined_loupists: Arc::new(RwLock::new(quarantined_loupists)),
            _instance_lock: instance_lock,
            model_catalog: Arc::new(RwLock::new(model_catalog)),
            fresh_install,
            graph: crate::knowledge::GraphSupervisor::with_inference(
                store.clone(),
                app_data_dir.clone(),
                resource_dir.clone(),
                maintenance,
                Some(inference),
            ),
            followed: Default::default(),
        };
        // Invalid editable configuration must leave Settings available. Every
        // execution admission rechecks reconciliation before using credentials.
        let _ = crate::marketplace::connector_config::reconcile_connector_configs(&state);
        // A build that no longer offers a provider deletes what still names it.
        crate::providers::unknown::delete_unknown_provider_state(&state);
        Ok(state)
    }

    pub fn pi_execution(
        &self,
        model_profile_id: &str,
        thinking_level: &str,
        run_options: &std::collections::BTreeMap<String, String>,
    ) -> Result<PiExecutionConfig, CommandError> {
        // An empty choice is Auto; a choice that left the catalog falls back to
        // Auto at this use (W17.5). Both resolve here, in Rust.
        let choice =
            crate::providers::auto::admit(self, model_profile_id, thinking_level, run_options)?;
        if !self.model_visibility()?.is_visible(&choice.model.id) {
            return Err(CommandError::invalid(
                "the selected Pi model is hidden from Chat",
            ));
        }
        let mut pi = self
            .artifacts
            .pi
            .clone()
            .ok_or_else(|| CommandError::unavailable("pi"))?;
        pi.provider_credential = provider_credential_from_environment(&choice.model.provider);
        PiExecutionConfig::new(
            pi,
            choice.model,
            &choice.thinking_level,
            &choice.run_options,
        )
    }

    pub fn pi_agent_data_dir(&self) -> Result<PathBuf, CommandError> {
        #[cfg(feature = "eval-harness")]
        if let Some(path) = &self.eval_pi_agent_dir {
            return Ok(path.clone());
        }
        #[cfg(any(test, feature = "e2e"))]
        if let Some(path) = live_pi_agent_data_dir_override()? {
            return Ok(path);
        }
        Ok(self.artifacts.app_data_dir.join("pi"))
    }

    pub fn model_catalog(&self) -> Result<ModelCatalog, CommandError> {
        self.model_catalog
            .read()
            .map_err(|_| CommandError::internal("model settings lock was poisoned"))
            .map(|catalog| catalog.clone())
    }

    pub fn model_visibility(&self) -> Result<ModelVisibility, CommandError> {
        Ok(ModelVisibility {
            hidden_model_profile_ids: crate::scoped_config::read_hidden_model_profile_ids(
                &self.artifacts.app_data_dir,
            )?
            .into_iter()
            .collect(),
        })
    }

    pub fn model_catalog_view(&self) -> Result<ModelCatalogView, CommandError> {
        let mut view = catalog_view(
            &self.model_catalog()?,
            &self.pi_agent_data_dir()?,
            &self.model_visibility()?,
        )?;
        crate::providers::status::decorate(self, &mut view)?;
        Ok(view)
    }

    pub fn set_model_visible(
        &self,
        model_profile_id: &str,
        visible: bool,
    ) -> Result<(), CommandError> {
        self.model_catalog()?.resolve(model_profile_id)?;
        crate::scoped_config::set_model_visible(
            &self.artifacts.app_data_dir,
            model_profile_id,
            visible,
        )?;
        Ok(())
    }

    pub fn set_model_catalog(&self, catalog: ModelCatalog) -> Result<(), CommandError> {
        catalog.validate()?;
        self.store
            .save_model_catalog(&catalog)
            .map_err(|error| CommandError::internal(error.to_string()))?;
        *self
            .model_catalog
            .write()
            .map_err(|_| CommandError::internal("model settings lock was poisoned"))? = catalog;
        Ok(())
    }

    pub fn replace_provider_models(
        &self,
        provider: &str,
        models: Vec<ConfiguredModel>,
    ) -> Result<(), CommandError> {
        let mut catalog = self.model_catalog()?;
        let removed = catalog
            .models
            .iter()
            .filter(|model| {
                model.provider == provider && !models.iter().any(|next| next.id == model.id)
            })
            .cloned()
            .collect::<Vec<_>>();
        catalog.models.retain(|model| model.provider != provider);
        catalog.models.extend(models);
        catalog.models.sort_by(|left, right| {
            left.provider
                .cmp(&right.provider)
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.model.cmp(&right.model))
        });
        self.set_model_catalog(catalog)?;
        // A pruned Chat default falls back to Auto at its next use; say so once.
        crate::providers::auto::note_removed_default(self, &removed);
        Ok(())
    }

    pub fn quarantine_loupist(
        &self,
        loupist_id: &str,
        source: QuarantineSource,
        reason: impl Into<String>,
    ) {
        self.quarantined_loupists
            .write()
            .expect("Loupist quarantine lock was poisoned")
            .entry(loupist_id.to_owned())
            .or_default()
            .insert(source, reason.into());
    }

    pub fn clear_loupist_quarantine(&self, loupist_id: &str, source: QuarantineSource) {
        let mut quarantines = self
            .quarantined_loupists
            .write()
            .expect("Loupist quarantine lock was poisoned");
        if let Some(sources) = quarantines.get_mut(loupist_id) {
            sources.remove(&source);
            if sources.is_empty() {
                quarantines.remove(loupist_id);
            }
        }
    }

    pub(crate) fn loupist_access(&self, loupist_id: &str) -> LoupistAccess {
        let quarantines = self
            .quarantined_loupists
            .read()
            .expect("Loupist quarantine lock was poisoned");
        let Some(sources) = quarantines.get(loupist_id) else {
            return LoupistAccess::Visible(LoupistAvailability::Available);
        };
        if sources.contains_key(&QuarantineSource::Deletion) {
            return LoupistAccess::Hidden;
        }
        if sources.contains_key(&QuarantineSource::MemoryWrite) {
            return LoupistAccess::Visible(LoupistAvailability::RecoveryRequired {
                reason: LoupistRecoveryReason::InterruptedMemoryUpdate,
                action: LoupistRecoveryAction::RecoverMemory,
            });
        }
        LoupistAccess::Hidden
    }

    pub fn ensure_loupist_available(&self, loupist_id: &str) -> Result<(), CommandError> {
        match self.loupist_access(loupist_id) {
            // Storage binding refuses a missing or earlier-build folder itself.
            LoupistAccess::Visible(
                LoupistAvailability::Available
                | LoupistAvailability::WorkspaceUnavailable
                | LoupistAvailability::EarlierVersion,
            ) => {}
            LoupistAccess::Visible(LoupistAvailability::RecoveryRequired { .. }) => {
                return Err(CommandError::new(
                    "loupist_recovery_required",
                    "Loupist requires recovery before it can be used",
                ));
            }
            LoupistAccess::Hidden => {
                return Err(CommandError::new(
                    "loupist_storage_unavailable",
                    "Loupist storage is unavailable",
                ));
            }
        }
        // Registration metadata remains visible when a Loupist folder is missing.
        // Canonical storage and execution bind the folder before accessing it.
        Ok(())
    }

    pub fn register_run(
        &self,
        run_id: String,
        loupist_id: String,
        kind: RunKind,
        target: RunTarget,
    ) -> Result<RunRegistration, CommandError> {
        self.ensure_loupist_available(&loupist_id)?;
        self.run_coordinator.register(
            RunSpec {
                run_id,
                loupist_id: loupist_id.clone(),
                kind,
                target,
            },
            || self.ensure_loupist_available(&loupist_id),
        )
    }

    pub fn reserve_memory_write(
        &self,
        run_id: String,
        loupist_id: String,
        target: RunTarget,
    ) -> Result<PendingMemoryWrite, CommandError> {
        self.ensure_loupist_available(&loupist_id)?;
        self.run_coordinator.reserve_memory_write(
            RunSpec {
                run_id,
                loupist_id: loupist_id.clone(),
                kind: RunKind::MemoryWrite,
                target,
            },
            || self.ensure_loupist_available(&loupist_id),
        )
    }

    pub fn register_loupist_recovery(
        &self,
        run_id: String,
        loupist_id: String,
        action: LoupistRecoveryAction,
    ) -> Result<RunRegistration, CommandError> {
        self.run_coordinator.register(
            RunSpec {
                run_id,
                loupist_id: loupist_id.clone(),
                kind: RunKind::MemoryWrite,
                target: RunTarget::MemoryWriteSession("pending-recovery".into()),
            },
            || match self.loupist_access(&loupist_id) {
                LoupistAccess::Visible(LoupistAvailability::RecoveryRequired {
                    action: required_action,
                    ..
                }) if action == required_action => Ok(()),
                LoupistAccess::Visible(LoupistAvailability::RecoveryRequired { .. }) => Err(
                    CommandError::invalid("requested Loupist recovery action is not available"),
                ),
                LoupistAccess::Visible(
                    LoupistAvailability::Available
                    | LoupistAvailability::WorkspaceUnavailable
                    | LoupistAvailability::EarlierVersion,
                ) => Err(CommandError::conflict(
                    "Loupist does not currently require recovery",
                )),
                LoupistAccess::Hidden => Err(CommandError::new(
                    "loupist_storage_unavailable",
                    "Loupist storage is unavailable",
                )),
            },
        )
    }

    pub fn register_chat_run(
        &self,
        run_id: String,
        loupist_id: String,
        thread_id: String,
        control: ChatControlHandle,
    ) -> Result<RunRegistration, CommandError> {
        self.ensure_loupist_available(&loupist_id)?;
        self.run_coordinator.register_chat(
            RunSpec {
                run_id,
                loupist_id: loupist_id.clone(),
                kind: RunKind::Chat,
                target: RunTarget::ChatThread(thread_id),
            },
            control,
            || self.ensure_loupist_available(&loupist_id),
        )
    }

    pub async fn cancel_run(
        &self,
        run_id: &str,
        expected_kind: RunKind,
    ) -> Result<(), CommandError> {
        self.run_coordinator.cancel(run_id, expected_kind).await
    }

    pub(crate) async fn shutdown(&self) -> Result<(), CommandError> {
        self.scheduled.stop();
        self.run_coordinator.request_shutdown()?;
        self.scheduled.drain().await;
        self.provider_support.cancel_oauth();
        self.mcp_pool.close_admission();
        let (runs, graph) = tokio::join!(self.run_coordinator.drain(), self.graph.shutdown());
        self.mcp_pool.shutdown().await;
        // Also stop support sidecars that do not own a Chat run. The instance
        // lock is still held, and durable identities are checked before any
        // signal; no process is selected by name or executable text.
        let lease_dir = self.artifacts.app_data_dir.join("process-leases");
        let recovered = tokio::task::spawn_blocking(move || recover_orphaned_processes(&lease_dir))
            .await
            .map_err(|_| CommandError::internal("sidecar shutdown observation failed"))?
            .map_err(|error| CommandError::internal(error.to_string()));
        runs?;
        graph.map_err(|error| CommandError::internal(error.to_string()))?;
        recovered
    }

    pub async fn submit_chat_control(
        &self,
        loupist_id: &str,
        thread_id: &str,
        prompt: String,
    ) -> Result<AcceptedChatControl, CommandError> {
        self.ensure_loupist_available(loupist_id)?;
        self.run_coordinator
            .submit_chat_control(loupist_id, thread_id, prompt)
            .await
    }

    pub fn claim_run_completion(
        &self,
        run_id: &str,
        expected_kind: RunKind,
    ) -> Result<bool, CommandError> {
        self.run_coordinator.claim_completion(run_id, expected_kind)
    }

    pub fn begin_loupist_deletion(&self, loupist_id: &str) -> Result<(), CommandError> {
        self.run_coordinator.begin_loupist_mutation(loupist_id, || {
            self.quarantine_loupist(
                loupist_id,
                QuarantineSource::Deletion,
                "Loupist deletion is in progress",
            )
        })
    }

    pub async fn forget_loupist(&self, loupist_id: &str) {
        self.clear_loupist_quarantine(loupist_id, QuarantineSource::Deletion);
    }

    #[cfg(test)]
    pub(crate) fn for_test(app_data_dir: PathBuf) -> Self {
        Self::for_test_with_store(app_data_dir, false, false, None)
    }

    #[cfg(test)]
    pub(crate) fn for_persistent_test(app_data_dir: PathBuf) -> Self {
        Self::for_test_with_store(app_data_dir, true, false, None)
    }

    #[cfg(test)]
    pub(crate) fn for_folder_test(app_data_dir: PathBuf) -> Self {
        Self::for_test_with_store(app_data_dir, true, true, None)
    }

    #[cfg(test)]
    pub(crate) fn close_for_restart_test(self) {
        // Restart tests intentionally reacquire the same app-data lock in one
        // process. Transfer the final lock reference out of AppState, prove no
        // hidden state clone can still own it, then establish an explicit
        // unlock boundary before constructing the replacement instance.
        let instance_lock = Arc::clone(&self._instance_lock);
        drop(self);
        assert_eq!(
            Arc::strong_count(&instance_lock),
            1,
            "restart fixture retained an AppState instance-lock owner"
        );
        FileExt::unlock(&instance_lock._file).unwrap();
    }

    #[cfg(any(test, feature = "eval-harness"))]
    fn for_test_with_store(
        app_data_dir: PathBuf,
        persistent: bool,
        folder_catalog: bool,
        acquisition_stage: Option<&'static str>,
    ) -> Self {
        let app_data_dir = pin_app_data_dir(&app_data_dir).unwrap();
        for child in ["brokers", "loupists", "mcp-pool", "pi", "runs"] {
            ensure_private_directory(&app_data_dir.join(child)).unwrap();
        }
        let mcp_pool = crate::mcp::pool::McpProcessPool::prepare(app_data_dir.join("mcp-pool"))
            .expect("test MCP pool");
        let instance_lock = Arc::new(AppInstanceLock::acquire(&app_data_dir).unwrap_or_else(
            |error| {
                panic!(
                    "{} could not acquire the test app-instance lock: {error:?}",
                    acquisition_stage.unwrap_or("test AppState initialization")
                )
            },
        ));
        let process_lease_dir = app_data_dir.join("process-leases");
        ensure_private_directory(&process_lease_dir).unwrap();
        prepare_lease_directory(&process_lease_dir).unwrap();
        if persistent {
            recover_orphaned_processes(&process_lease_dir).unwrap();
        }
        let finance_data = Arc::new(FinanceDataService::new().unwrap());
        let deletion_root =
            Arc::new(SecureDeletionRoot::open(&app_data_dir.canonicalize().unwrap()).unwrap());
        crate::runs::scratch::sweep_stale_runs(&deletion_root).unwrap();
        let store = if folder_catalog {
            SqliteStore::open_catalog(app_data_dir.join("catalog.sqlite3")).unwrap()
        } else {
            SqliteStore::open(app_data_dir.join("loupist.sqlite3")).unwrap()
        };
        let deletion_recovery = deletion::recover(&store, &deletion_root).unwrap();
        let model_catalog = store.get_model_catalog().unwrap().unwrap_or(ModelCatalog {
            models: vec![ConfiguredModel {
                id: "fixture".into(),
                name: "Fixture".into(),
                provider: "openai".into(),
                model: "fixture-model".into(),
                input: vec!["text".into()],
                reasoning: true,
                context_window: 128_000,
                max_tokens: 32_000,
                thinking_levels: vec!["off".into(), "low".into(), "medium".into(), "high".into()],
                thinking_level_map: std::collections::BTreeMap::new(),
                run_controls: vec![],
            }],
        });
        let credential_environment = store.initialize_host_environment().unwrap();
        let store = Arc::new(store);
        let maintenance = MaintenanceCoordinator::default();
        let updates = UpdateCoordinator::new(store.clone()).unwrap();
        let quarantined_loupists = deletion_recovery
            .quarantined_loupists
            .into_iter()
            .map(|(loupist_id, reason)| {
                (
                    loupist_id,
                    HashMap::from([(QuarantineSource::Deletion, reason)]),
                )
            })
            .collect::<HashMap<_, _>>();
        let artifacts = Arc::new(AppArtifacts {
            broker_dir: app_data_dir.join("brokers"),
            process_lease_dir,
            deletion_root,
            app_data_dir: app_data_dir.clone(),
            pi: None,
            mcp_runtimes: BTreeMap::new(),
        });
        let provider_support = ProviderSupportCoordinator::new(maintenance.clone());
        let inference = crate::providers::inference::ProviderInferenceService::from_parts(
            artifacts.clone(),
            inference_agent_data_dir(&app_data_dir).expect("provider app data"),
            maintenance.clone(),
            store.clone(),
        );
        let state = Self {
            #[cfg(feature = "eval-harness")]
            eval_pi_agent_dir: None,
            scheduled: crate::scheduled_tasks::ScheduledSupervisor::default(),
            store: store.clone(),
            credential_environment: credential_environment.into(),
            artifacts,
            mcp_pool,
            finance_data,
            run_coordinator: RunCoordinator::new(maintenance.clone()),
            provider_support,
            provider_model_cache: ProviderModelDiscoveryCache::default(),
            maintenance: maintenance.clone(),
            updates,
            browser: BrowserManager::new(app_data_dir.join("browser-profile")),
            quarantined_loupists: Arc::new(RwLock::new(quarantined_loupists)),
            _instance_lock: instance_lock,
            model_catalog: Arc::new(RwLock::new(model_catalog)),
            fresh_install: false,
            graph: crate::knowledge::GraphSupervisor::with_inference(
                store.clone(),
                app_data_dir.clone(),
                None,
                maintenance,
                Some(inference),
            ),
            followed: Default::default(),
        };
        let _ = crate::marketplace::connector_config::reconcile_connector_configs(&state);
        state
    }
}

#[cfg(any(test, feature = "e2e"))]
pub(crate) fn with_live_pi_agent_data_dir_override(
    mut config: PiLaunchConfig,
) -> Result<PiLaunchConfig, CommandError> {
    let Some(path) = live_pi_agent_data_dir_override()? else {
        return Ok(config);
    };
    config.agent_data_dir = path;
    Ok(config)
}

#[cfg(any(test, feature = "e2e"))]
fn live_pi_agent_data_dir_override() -> Result<Option<PathBuf>, CommandError> {
    let Some(value) = std::env::var_os(LIVE_PI_AGENT_DATA_DIR_ENV) else {
        return Ok(None);
    };
    if value.is_empty() {
        return Err(CommandError::new(
            "pi_unavailable",
            "live Pi agent data directory is invalid",
        ));
    }
    let path = PathBuf::from(value);
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| {
        CommandError::new(
            "pi_unavailable",
            "live Pi agent data directory is unavailable",
        )
    })?;
    if !path.is_absolute() || metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CommandError::new(
            "pi_unavailable",
            "live Pi agent data directory is invalid",
        ));
    }
    // Test code validates the directory entry only; it never enumerates, reads,
    // copies, serializes, or prints credentials. The real Pi process may still
    // refresh OAuth state in this connected directory during a successful run.
    Ok(Some(path))
}

fn resolve_pi_artifacts(
    executable: Option<PathBuf>,
    runtime_dir: Option<PathBuf>,
    agent_resource: Option<PathBuf>,
) -> Option<PiArtifacts> {
    let executable = executable?;
    let runtime_dir = runtime_dir?;
    let agent_resource = agent_resource?;
    let extension = agent_resource.join("loupist-extension.mjs");
    let provider_extension = agent_resource.join("loupist-provider-extension.mjs");
    let system_prompt = agent_resource.join("SYSTEM.md");
    if crate::agent_harness::validate_extension_bundle(&extension).is_err()
        || crate::agent_harness::validate_provider_extension_bundle(&provider_extension).is_err()
        || !system_prompt.is_file()
    {
        return None;
    }
    let bundled_skills =
        crate::agent_harness::run_skill_ids(&crate::agent_harness::default_skill_ids()).ok()?;
    crate::agent_harness::resolve_skill_paths(&agent_resource, &bundled_skills).ok()?;
    Some(PiArtifacts {
        executable,
        runtime_dir,
        extension,
        provider_extension,
        system_prompt,
        provider: String::new(),
        model: String::new(),
        thinking_level: String::new(),
        run_options: std::collections::BTreeMap::new(),
        provider_credential: None,
    })
}

fn resolve_file(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|path| path.is_file())
}

fn resolve_directory(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|path| path.is_dir())
}

fn open_app_store(database_path: &Path) -> Result<(SqliteStore, bool), CommandError> {
    let existed = database_path.exists();
    match SqliteStore::open_catalog(database_path) {
        Ok(store) => Ok((store, !existed)),
        // Pre-1.0 an earlier build's catalog is set aside, never migrated: the
        // app starts fresh and the old file stays next to the new one.
        Err(crate::store::StoreError::EarlierVersion) => {
            set_aside_earlier_catalog(database_path)?;
            SqliteStore::open_catalog(database_path)
                .map(|store| (store, true))
                .map_err(|error| CommandError::internal(error.to_string()))
        }
        Err(error) => Err(CommandError::internal(error.to_string())),
    }
}

fn set_aside_earlier_catalog(database_path: &Path) -> Result<(), CommandError> {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    for suffix in ["", "-wal", "-shm"] {
        let mut from = database_path.as_os_str().to_os_string();
        from.push(suffix);
        let from = PathBuf::from(from);
        if !from.exists() {
            continue;
        }
        let mut to = database_path.as_os_str().to_os_string();
        to.push(format!(".earlier-{stamp}{suffix}"));
        std::fs::rename(&from, PathBuf::from(to)).map_err(|error| {
            CommandError::internal(format!("earlier catalog could not be set aside: {error}"))
        })?;
    }
    eprintln!("Loupist set aside a catalog from an earlier build and started fresh.");
    Ok(())
}

fn pin_app_data_dir(path: &Path) -> Result<PathBuf, CommandError> {
    ensure_private_directory(path).map_err(|error| CommandError::internal(error.to_string()))?;
    // macOS exposes `/tmp` as a lexical alias of `/private/tmp`. Pin the
    // app-data root once and derive every persisted path from that exact
    // identity so profiles created through an alias remain app-owned.
    path.canonicalize()
        .map_err(|error| CommandError::internal(format!("app data path cannot be pinned: {error}")))
}

#[cfg(any(not(feature = "e2e"), test))]
fn app_data_dir_for_build(path: PathBuf, identifier: &str) -> PathBuf {
    let path = path.join(".loupist");
    if cfg!(debug_assertions) && identifier == PRODUCTION_APP_IDENTIFIER {
        path.join("development")
    } else if identifier != PRODUCTION_APP_IDENTIFIER {
        // Manual profiles use distinct identifiers and must not share either
        // the installed library or the default development environment.
        path.join("profiles").join(identifier)
    } else {
        path
    }
}

#[cfg(feature = "e2e")]
fn e2e_app_data_dir(path: Option<PathBuf>) -> Result<PathBuf, CommandError> {
    let path = path.ok_or_else(|| {
        CommandError::internal("the E2E launcher must provide LOUPIST_E2E_APP_DATA_DIR")
    })?;
    if !path.is_absolute() {
        return Err(CommandError::internal(
            "LOUPIST_E2E_APP_DATA_DIR must be absolute",
        ));
    }
    Ok(path)
}

fn transient_broker_directory() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        // macOS commonly resolves `env::temp_dir()` below a long
        // `/var/folders/...` path, while Unix-domain sockets allow only 103
        // path bytes. Use the stable short alias and isolate the private
        // broker directory by effective user ID.
        PathBuf::from("/tmp").join(format!("loupist-{}", unsafe { libc::geteuid() }))
    }
    #[cfg(not(target_os = "macos"))]
    {
        env::temp_dir().join("loupist")
    }
}

fn platform_binary(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests;

fn inference_agent_data_dir(app_data: &std::path::Path) -> Result<PathBuf, CommandError> {
    #[cfg(any(test, feature = "e2e"))]
    if let Some(path) = live_pi_agent_data_dir_override()? {
        return Ok(path);
    }
    Ok(app_data.join("pi"))
}
