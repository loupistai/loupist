//! User-owned global and Loupist settings. TOML is the only settings source;
//! operational state and credentials belong to their existing stores.
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::Path,
    sync::Mutex,
};

#[cfg(unix)]
use std::fs::OpenOptions;
#[cfg(not(unix))]
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use toml_edit::{Document, Item, Table};

use crate::app::CommandError;

const MAX_CONFIG_BYTES: usize = 256 * 1024;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const FILE_NAME: &str = "config.toml";
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConfigScope {
    Global,
    Project {
        #[serde(rename = "loupistId")]
        loupist_id: String,
    },
}

macro_rules! optional_config {
    ($name:ident { $($field:ident: $kind:ty),* $(,)? }) => {
        #[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
        #[cfg_attr(test, derive(ts_rs::TS))]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            $(#[serde(default, skip_serializing_if = "Option::is_none")]
            #[cfg_attr(test, ts(optional))]
            pub $field: Option<$kind>,)*
        }
    };
}

optional_config!(ConfigDocument {
    appearance: AppearanceConfig,
    models: ModelsConfig,
    connectors: BTreeMap<String, BTreeMap<String, String>>,
    chat: ChatConfig,
    skills: BTreeMap<String, SkillConfig>,
    pi: PartialPiConfig,
});
optional_config!(AppearanceConfig {
    theme: String,
    language: String
});
optional_config!(ModelsConfig { hidden_model_profile_ids: Vec<String> });
optional_config!(ChatConfig {
    model_profile_id: String,
    thinking_level: String,
    run_options: BTreeMap<String, String>,
    use_memory: bool,
    update_memory: bool,
});
optional_config!(SkillConfig { enabled: bool });
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResolvedSkillConfig {
    /// Missing on a custom Skill only. Included Skills are filled from the catalog first.
    #[serde(default = "custom_skill_enabled_when_unset")]
    pub enabled: bool,
}
fn custom_skill_enabled_when_unset() -> bool {
    true
}

optional_config!(PartialPiConfig {
    compaction: PartialPiCompactionConfig,
    retry: PartialPiRetryConfig,
});
optional_config!(PartialPiCompactionConfig {
    reserve_tokens: u64,
    keep_recent_tokens: u64
});
optional_config!(PartialPiRetryConfig {
    enabled: bool,
    max_retries: u32,
    base_delay_ms: u64,
    max_agent_delay_ms: u64,
});

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct PiConfig {
    pub compaction: PiCompactionConfig,
    pub retry: PiRetryConfig,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct PiCompactionConfig {
    pub reserve_tokens: u64,
    pub keep_recent_tokens: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct PiRetryConfig {
    pub enabled: bool,
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_agent_delay_ms: u64,
}
impl Default for PiConfig {
    fn default() -> Self {
        Self {
            compaction: PiCompactionConfig {
                reserve_tokens: 16_384,
                keep_recent_tokens: 20_000,
            },
            retry: PiRetryConfig {
                enabled: true,
                max_retries: 3,
                base_delay_ms: 2_000,
                max_agent_delay_ms: 60_000,
            },
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResolvedAppearanceConfig {
    pub theme: String,
    pub language: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResolvedModelsConfig {
    pub hidden_model_profile_ids: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResolvedChatConfig {
    pub model_profile_id: Option<String>,
    pub thinking_level: Option<String>,
    pub run_options: BTreeMap<String, String>,
    pub use_memory: bool,
    pub update_memory: bool,
}
impl Default for ResolvedChatConfig {
    fn default() -> Self {
        Self {
            model_profile_id: None,
            thinking_level: None,
            run_options: BTreeMap::new(),
            use_memory: true,
            update_memory: true,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResolvedConfig {
    pub appearance: ResolvedAppearanceConfig,
    pub models: ResolvedModelsConfig,
    pub connectors: BTreeMap<String, BTreeMap<String, String>>,
    pub chat: ResolvedChatConfig,
    pub skills: BTreeMap<String, ResolvedSkillConfig>,
    pub pi: PiConfig,
}
impl Default for ResolvedConfig {
    fn default() -> Self {
        Self {
            appearance: ResolvedAppearanceConfig {
                theme: "system".into(),
                language: "system".into(),
            },
            models: ResolvedModelsConfig::default(),
            connectors: BTreeMap::new(),
            chat: ResolvedChatConfig::default(),
            skills: crate::agent_harness::default_skill_ids()
                .into_iter()
                .map(|id| {
                    (
                        id.clone(),
                        ResolvedSkillConfig {
                            enabled: crate::agent_harness::skill_default_enabled(&id),
                        },
                    )
                })
                .collect(),
            pi: PiConfig::default(),
        }
    }
}
impl ResolvedConfig {
    pub fn execution_digest(&self) -> String {
        digest(
            &serde_json::to_vec(&(&self.chat, &self.pi, &self.skills))
                .expect("serializable execution config"),
        )
    }
    pub fn enabled_skill_ids(&self) -> Vec<String> {
        crate::agent_harness::default_skill_ids()
            .into_iter()
            .filter(|id| {
                self.skills
                    .get(id)
                    .map(|skill| skill.enabled)
                    .unwrap_or_else(|| crate::agent_harness::skill_default_enabled(id))
            })
            .collect()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigDiagnostic {
    pub path: String,
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ConfigSnapshot {
    pub scope: ConfigScope,
    pub path: String,
    pub revision: String,
    pub explicit: Option<ConfigDocument>,
    pub effective: ResolvedConfig,
    pub sources: BTreeMap<String, String>,
    /// Errors: the file is malformed or a known key has an invalid value. Any
    /// error blocks dependent runs and settings saves until the file is fixed.
    pub diagnostics: Vec<ConfigDiagnostic>,
    /// Settings this version ignores because its bundled catalog no longer
    /// has that connector or connector field, or keeps that Skill always on.
    /// The text stays in the file; nothing is blocked.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(optional = nullable))]
    pub warnings: Vec<ConfigDiagnostic>,
}
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigReadRequest {
    pub scope: ConfigScope,
}
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigUpdateRequest {
    pub scope: ConfigScope,
    pub expected_revision: String,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub set: ConfigDocument,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub unset: Vec<Vec<String>>,
}

struct LoadedConfig {
    revision: String,
    document: Option<Document>,
    /// The file's settings without the ignored ones (`ignore_unknown_catalog_keys`).
    explicit: Option<ConfigDocument>,
    diagnostics: Vec<ConfigDiagnostic>,
    warnings: Vec<ConfigDiagnostic>,
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn io_error(error: impl std::fmt::Display) -> CommandError {
    CommandError::new(
        "configuration_unavailable",
        format!("Could not access configuration: {error}"),
    )
}
fn config_error(message: impl Into<String>) -> CommandError {
    CommandError::new("invalid_configuration", message)
}

/// The caller supplies the registered Loupist's canonical `.loupist` directory,
/// never an arbitrary renderer path. Global root is pinned by AppState.
fn scope_directory<'a>(
    global_root: &'a Path,
    project_loupist_dir: Option<&'a Path>,
    scope: &ConfigScope,
) -> Result<&'a Path, CommandError> {
    match scope {
        ConfigScope::Global => Ok(global_root),
        ConfigScope::Project { loupist_id } => {
            if loupist_id.is_empty() {
                return Err(CommandError::invalid("Select a Loupist configuration"));
            }
            project_loupist_dir
                .ok_or_else(|| CommandError::invalid("Loupist configuration directory is missing"))
        }
    }
}

// Keep a directory handle for the full read/write operation. On Unix all file
// access is relative to that descriptor; on Windows the handle excludes rename.
struct ConfigDirectory {
    #[cfg(not(unix))]
    path: PathBuf,
    _handle: File,
    #[cfg(windows)]
    _parents: Vec<File>,
}
impl ConfigDirectory {
    fn open(path: &Path) -> Result<Self, CommandError> {
        if !path.is_absolute() {
            return Err(config_error("Configuration directory must be absolute"));
        }
        #[cfg(unix)]
        let handle = {
            use std::os::unix::fs::OpenOptionsExt;
            OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW)
                .open(path)
                .map_err(io_error)?
        };
        #[cfg(windows)]
        let parents =
            crate::fs::windows::open_parent_directories_no_reparse(path).map_err(io_error)?;
        #[cfg(windows)]
        let handle = crate::fs::windows::open_directory_no_reparse(path).map_err(io_error)?;
        #[cfg(not(any(unix, windows)))]
        let handle = {
            let metadata = std::fs::symlink_metadata(path).map_err(io_error)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(config_error(
                    "Configuration directory must be a regular directory",
                ));
            }
            File::open(path).map_err(io_error)?
        };
        Ok(Self {
            #[cfg(not(unix))]
            path: path.to_path_buf(),
            _handle: handle,
            #[cfg(windows)]
            _parents: parents,
        })
    }
    fn read(&self) -> Result<Option<Vec<u8>>, CommandError> {
        #[cfg(unix)]
        let file = {
            use rustix::fs::{openat, Mode, OFlags};
            openat(
                &self._handle,
                FILE_NAME,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
                Mode::empty(),
            )
            .map(File::from)
            .map_err(std::io::Error::from)
        };
        #[cfg(windows)]
        let file = crate::fs::windows::open_atomic_snapshot_no_reparse(&self.path.join(FILE_NAME));
        #[cfg(not(any(unix, windows)))]
        let file = {
            let path = self.path.join(FILE_NAME);
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(config_error("Configuration cannot be a symbolic link"))
                }
                _ => File::open(path),
            }
        };
        let file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error(error)),
        };
        let metadata = file.metadata().map_err(io_error)?;
        if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES as u64 {
            return Err(config_error(
                "Configuration must be a regular file no larger than 256 KiB",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_CONFIG_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(config_error("Configuration exceeds 256 KiB"));
        }
        Ok(Some(bytes))
    }
    fn replace(&self, bytes: &[u8], expected_revision: &str) -> Result<(), CommandError> {
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(config_error("Configuration exceeds 256 KiB"));
        }
        #[cfg(unix)]
        {
            use rustix::fs::{openat, renameat, unlinkat, AtFlags, Mode, OFlags};
            let name = format!(".config-{}.tmp", uuid::Uuid::new_v4());
            let descriptor = openat(
                &self._handle,
                name.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )
            .map_err(io_error)?;
            let mut file = File::from(descriptor);
            let result = (|| {
                file.write_all(bytes).map_err(io_error)?;
                file.sync_all().map_err(io_error)?;
                self.check_revision(expected_revision)?;
                renameat(&self._handle, name.as_str(), &self._handle, FILE_NAME)
                    .map_err(io_error)?;
                self._handle.sync_all().map_err(io_error)
            })();
            let _ = unlinkat(&self._handle, name.as_str(), AtFlags::empty());
            result
        }
        #[cfg(not(unix))]
        {
            let mut file = tempfile::NamedTempFile::new_in(&self.path).map_err(io_error)?;
            file.write_all(bytes).map_err(io_error)?;
            file.as_file().sync_all().map_err(io_error)?;
            self.check_revision(expected_revision)?;
            file.persist(self.path.join(FILE_NAME)).map_err(io_error)?;
            Ok(())
        }
    }
    fn check_revision(&self, expected: &str) -> Result<(), CommandError> {
        if revision(self.read()?.as_deref()) != expected {
            return Err(CommandError::conflict(
                "Configuration changed outside this view; reload before saving",
            ));
        }
        Ok(())
    }
}
fn revision(bytes: Option<&[u8]>) -> String {
    bytes.map(digest).unwrap_or_else(|| "missing".into())
}
fn diagnostic(
    path: &Path,
    message: impl Into<String>,
    source: Option<&str>,
    offset: Option<usize>,
) -> ConfigDiagnostic {
    let position = source.zip(offset).map(|(source, offset)| {
        let mut offset = offset.min(source.len());
        while !source.is_char_boundary(offset) {
            offset -= 1;
        }
        let prefix = &source[..offset];
        (
            prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
            prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
        )
    });
    ConfigDiagnostic {
        path: path.to_string_lossy().into_owned(),
        message: message.into(),
        line: position.map(|p| p.0),
        column: position.map(|p| p.1),
    }
}
fn load(directory: &Path, scope: &ConfigScope) -> LoadedConfig {
    let path = directory.join(FILE_NAME);
    let bytes = match ConfigDirectory::open(directory).and_then(|directory| directory.read()) {
        Ok(bytes) => bytes,
        Err(error) => {
            return LoadedConfig {
                revision: "unavailable".into(),
                document: None,
                explicit: None,
                diagnostics: vec![diagnostic(&path, error.message, None, None)],
                warnings: Vec::new(),
            }
        }
    };
    let revision = revision(bytes.as_deref());
    let source = match bytes.map(String::from_utf8).transpose() {
        Ok(value) => value.unwrap_or_default(),
        Err(_) => {
            return LoadedConfig {
                revision,
                document: None,
                explicit: None,
                diagnostics: vec![diagnostic(
                    &path,
                    "Configuration must be UTF-8 text",
                    None,
                    None,
                )],
                warnings: Vec::new(),
            }
        }
    };
    let document = match source.parse::<Document>() {
        Ok(document) => document,
        Err(error) => {
            return LoadedConfig {
                revision,
                document: None,
                explicit: None,
                diagnostics: vec![diagnostic(
                    &path,
                    format!("Invalid TOML: {}", error.message()),
                    Some(&source),
                    error.span().map(|span| span.start),
                )],
                warnings: Vec::new(),
            }
        }
    };
    let mut config: ConfigDocument = match toml_edit::de::from_str(&source) {
        Ok(config) => config,
        Err(error) => {
            return LoadedConfig {
                revision,
                document: Some(document),
                explicit: None,
                diagnostics: vec![diagnostic(
                    &path,
                    "Unsupported configuration key or invalid value type",
                    Some(&source),
                    error.span().map(|span| span.start),
                )],
                warnings: Vec::new(),
            }
        }
    };
    let ignored = validate(&config, scope).and_then(|()| ignore_unknown_catalog_keys(&mut config));
    let ignored = match ignored {
        Ok(ignored) => ignored,
        Err(message) => {
            let offset = validation_offset(&source, &message);
            return LoadedConfig {
                revision,
                document: Some(document),
                explicit: None,
                diagnostics: vec![diagnostic(&path, message, Some(&source), offset)],
                warnings: Vec::new(),
            };
        }
    };
    let warnings = ignored
        .into_iter()
        .map(|message| {
            let offset = validation_offset(&source, &message);
            diagnostic(&path, message, Some(&source), offset)
        })
        .collect();
    LoadedConfig {
        revision,
        document: Some(document),
        explicit: Some(config),
        diagnostics: Vec::new(),
        warnings,
    }
}
fn validation_offset(source: &str, message: &str) -> Option<usize> {
    // Service errors name a dotted setting first; catalog validation appends
    // its canonical connector field after a colon. Never include a value in
    // a diagnostic just to locate the setting in user-authored TOML.
    let setting = message
        .rsplit_once(": ")
        .map(|(_, path)| path)
        .unwrap_or_else(|| message.split_whitespace().next().unwrap_or(""));
    let key = setting.rsplit('.').next()?;
    source.find(key).or_else(|| {
        message
            .contains("connector")
            .then(|| source.find("[connectors"))
            .flatten()
    })
}

fn bounded(value: &str, label: &str, maximum: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > maximum || value.contains('\0') {
        return Err(format!("{label} must contain 1–{maximum} bytes"));
    }
    Ok(())
}
/// Removes the settings this version's bundled catalog does not have and
/// returns one warning per setting: a connector or connector field the catalog
/// no longer declares, and a Skill that is now always on. These keys are not
/// typos of this build's own settings but the catalog moving under a file the
/// user (or another Loupist version) wrote, so they never block a save; the
/// TOML text keeps them. Malformed TOML, unknown sections and invalid values
/// of known keys stay errors (`validate`).
fn ignore_unknown_catalog_keys(config: &mut ConfigDocument) -> Result<Vec<String>, String> {
    let mut ignored = Vec::new();
    if let Some(skills) = &mut config.skills {
        skills.retain(|id, _| {
            let fixed = crate::skills::is_fixed(id);
            if fixed {
                ignored.push(format!(
                    "skills.{id} is always on in this version; this setting is ignored"
                ));
            }
            !fixed
        });
    }
    if let Some(connectors) = &mut config.connectors {
        for (id, field) in crate::marketplace::connector_config::ignored_config_entries(connectors)
            .map_err(|error| error.message)?
        {
            match field {
                None => {
                    connectors.remove(&id);
                    ignored.push(format!(
                        "connectors.{id} is not a connector in this version; it is ignored"
                    ));
                }
                Some(field) => {
                    if let Some(options) = connectors.get_mut(&id) {
                        options.remove(&field);
                    }
                    ignored.push(format!(
                        "connectors.{id}.{field} is not a setting of this connector in this version; it is ignored"
                    ));
                }
            }
        }
    }
    Ok(ignored)
}

fn validate(config: &ConfigDocument, scope: &ConfigScope) -> Result<(), String> {
    if let Some(skills) = &config.skills {
        for id in skills.keys() {
            if !crate::skills::valid_name(id) {
                return Err("skills contains an invalid name".into());
            }
        }
    }
    if matches!(scope, ConfigScope::Project { .. })
        && (config.appearance.is_some() || config.models.is_some() || config.connectors.is_some())
    {
        let section = if config.appearance.is_some() {
            "appearance"
        } else if config.models.is_some() {
            "models"
        } else {
            "connectors"
        };
        return Err(format!(
            "{section} is only supported in global configuration"
        ));
    }
    if let Some(theme) = config
        .appearance
        .as_ref()
        .and_then(|value| value.theme.as_deref())
    {
        if !["system", "light", "dark"].contains(&theme) {
            return Err("appearance.theme must be system, light or dark".into());
        }
    }
    if let Some(language) = config
        .appearance
        .as_ref()
        .and_then(|value| value.language.as_deref())
    {
        if !["system", "en", "ko"].contains(&language) {
            return Err("appearance.language must be system, en or ko".into());
        }
    }
    if let Some(ids) = config
        .models
        .as_ref()
        .and_then(|value| value.hidden_model_profile_ids.as_ref())
    {
        if ids.len() > 2_000 {
            return Err("models.hidden_model_profile_ids exceeds 2000 entries".into());
        }
        for id in ids {
            bounded(id, "models.hidden_model_profile_ids", 512)?;
        }
    }
    if let Some(connectors) = &config.connectors {
        crate::marketplace::connector_config::validate_config_entries(connectors)
            .map_err(|error| error.message)?;
        if connectors.len() > 512 {
            return Err("connectors exceeds 512 entries".into());
        }
        for (id, options) in connectors {
            bounded(id, "connectors entry identifier", 256)?;
            if options.len() > 128 {
                return Err("connectors entry exceeds 128 settings".into());
            }
            for (key, value) in options {
                bounded(key, "connectors option name", 128)?;
                let normalized = key.to_ascii_lowercase().replace(['-', '_'], "");
                if [
                    "apikey",
                    "secret",
                    "clientsecret",
                    "password",
                    "accesstoken",
                    "refreshtoken",
                    "authorization",
                    "credential",
                    "credentials",
                ]
                .contains(&normalized.as_str())
                {
                    return Err("connectors cannot contain credentials; use the connection credential controls".into());
                }
                if value.len() > 8_192 || value.contains('\0') {
                    return Err(
                        "connectors option value exceeds 8192 bytes or contains a null character"
                            .into(),
                    );
                }
            }
        }
    }
    if let Some(chat) = &config.chat {
        if let Some(id) = &chat.model_profile_id {
            bounded(id, "chat.model_profile_id", 512)?;
        }
        if let Some(level) = &chat.thinking_level {
            if !["off", "minimal", "low", "medium", "high", "xhigh", "max"]
                .contains(&level.as_str())
            {
                return Err("chat.thinking_level is not a supported thinking level".into());
            }
        }
        if let Some(options) = &chat.run_options {
            if options.len() > 64 {
                return Err("chat.run_options exceeds 64 entries".into());
            }
            for (key, value) in options {
                bounded(key, "chat.run_options key", 128)?;
                bounded(value, "chat.run_options value", 512)?;
            }
        }
    }
    if let Some(pi) = &config.pi {
        if let Some(compaction) = &pi.compaction {
            for (name, value) in [
                ("reserve_tokens", compaction.reserve_tokens),
                ("keep_recent_tokens", compaction.keep_recent_tokens),
            ] {
                if value.is_some_and(|value| value > MAX_SAFE_INTEGER) {
                    return Err(format!(
                        "pi.compaction.{name} must be a nonnegative safe integer"
                    ));
                }
            }
        }
        if let Some(retry) = &pi.retry {
            if retry.max_retries.is_some_and(|value| value > 10) {
                return Err("pi.retry.max_retries must be between 0 and 10".into());
            }
            for (name, value) in [
                ("base_delay_ms", retry.base_delay_ms),
                ("max_agent_delay_ms", retry.max_agent_delay_ms),
            ] {
                if value.is_some_and(|value| value > 300_000) {
                    return Err(format!("pi.retry.{name} must be between 0 and 300000"));
                }
            }
        }
    }
    Ok(())
}
fn merge(
    target: &mut Value,
    patch: Value,
    prefix: &str,
    source: &str,
    sources: &mut BTreeMap<String, String>,
) {
    if let Value::Object(patch) = patch {
        if !target.is_object() {
            *target = Value::Object(Default::default());
        }
        let target = target.as_object_mut().expect("object initialized");
        for (key, value) in patch {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            merge(
                target.entry(key).or_insert(Value::Null),
                value,
                &path,
                source,
                sources,
            );
        }
    } else {
        *target = patch;
        sources.insert(prefix.to_owned(), source.to_owned());
    }
}
fn assemble(
    global: &LoadedConfig,
    project: Option<&LoadedConfig>,
) -> (ResolvedConfig, BTreeMap<String, String>) {
    let mut value = Value::Null;
    let mut sources = BTreeMap::new();
    merge(
        &mut value,
        serde_json::to_value(ResolvedConfig::default()).expect("serializable defaults"),
        "",
        "default",
        &mut sources,
    );
    if let Some(config) = &global.explicit {
        merge(
            &mut value,
            serde_json::to_value(config).expect("serializable config"),
            "",
            "global",
            &mut sources,
        );
    }
    if let Some(config) = project.and_then(|project| project.explicit.as_ref()) {
        merge(
            &mut value,
            serde_json::to_value(config).expect("serializable config"),
            "",
            "project",
            &mut sources,
        );
    }
    apply_catalog_skill_defaults(&mut value);
    (
        serde_json::from_value(value).expect("validated configuration merge"),
        sources,
    )
}

/// An explicit `enabled` value already in the merged document wins. Otherwise
/// each included Skill takes its catalog default. Custom Skills are left unset
/// so deserialization keeps them enabled.
fn apply_catalog_skill_defaults(value: &mut Value) {
    let Some(skills) = value.get_mut("skills").and_then(Value::as_object_mut) else {
        return;
    };
    for id in crate::agent_harness::default_skill_ids() {
        let enabled = Value::Bool(crate::agent_harness::skill_default_enabled(&id));
        match skills.get_mut(&id) {
            Some(Value::Object(entry)) => {
                entry.entry("enabled").or_insert(enabled);
            }
            None => {
                let mut entry = serde_json::Map::new();
                entry.insert("enabled".to_owned(), enabled);
                skills.insert(id, Value::Object(entry));
            }
            Some(_) => {}
        }
    }
}

pub fn read_scope(
    global_root: &Path,
    project_loupist_dir: Option<&Path>,
    scope: ConfigScope,
) -> Result<ConfigSnapshot, CommandError> {
    let directory = scope_directory(global_root, project_loupist_dir, &scope)?;
    let global = load(global_root, &ConfigScope::Global);
    let project = if matches!(&scope, ConfigScope::Project { .. }) {
        Some(load(directory, &scope))
    } else {
        None
    };
    let (effective, sources) = assemble(&global, project.as_ref());
    let mut diagnostics = global.diagnostics.clone();
    let mut warnings = global.warnings.clone();
    if let Some(project) = &project {
        diagnostics.extend(project.diagnostics.clone());
        warnings.extend(project.warnings.clone());
    }
    let selected = project.as_ref().unwrap_or(&global);
    Ok(ConfigSnapshot {
        scope,
        path: directory.join(FILE_NAME).to_string_lossy().into_owned(),
        revision: selected.revision.clone(),
        explicit: selected.explicit.clone(),
        effective,
        sources,
        diagnostics,
        warnings,
    })
}
pub fn resolve(
    global_root: &Path,
    project_loupist_dir: Option<&Path>,
) -> Result<ResolvedConfig, CommandError> {
    let scope = if project_loupist_dir.is_some() {
        ConfigScope::Project {
            loupist_id: "execution".into(),
        }
    } else {
        ConfigScope::Global
    };
    let snapshot = read_scope(global_root, project_loupist_dir, scope)?;
    if let Some(error) = snapshot.diagnostics.first() {
        return Err(config_error(format!(
            "{}: {}{}",
            error.path,
            error.message,
            error
                .line
                .map(|line| format!(" (line {line})"))
                .unwrap_or_default()
        )));
    }
    Ok(snapshot.effective)
}

fn update_table(
    table: &mut dyn toml_edit::TableLike,
    patch: &serde_json::Map<String, Value>,
) -> Result<(), CommandError> {
    for (key, value) in patch {
        if let Value::Object(object) = value {
            if table.get(key).and_then(Item::as_table_like).is_none() {
                let mut nested = Table::new();
                nested.set_implicit(true);
                table.insert(key, Item::Table(nested));
            }
            update_table(
                table
                    .get_mut(key)
                    .and_then(Item::as_table_like_mut)
                    .expect("table inserted"),
                object,
            )?;
        } else {
            let new = match value {
                Value::Bool(value) => toml_edit::Value::from(*value),
                Value::String(value) => toml_edit::Value::from(value.as_str()),
                Value::Number(value) => {
                    toml_edit::Value::from(value.as_i64().ok_or_else(|| {
                        config_error("Configuration number exceeds TOML integer bounds")
                    })?)
                }
                Value::Array(values) => {
                    let mut array = toml_edit::Array::new();
                    for value in values {
                        array.push(value.as_str().ok_or_else(|| {
                            config_error("Configuration array must contain strings")
                        })?);
                    }
                    toml_edit::Value::Array(array)
                }
                _ => return Err(config_error("Configuration values cannot be null")),
            };
            let mut new = new;
            if let Some(old) = table.get(key).and_then(Item::as_value) {
                *new.decor_mut() = old.decor().clone();
            }
            table.insert(key, Item::Value(new));
        }
    }
    Ok(())
}
fn remove_path(table: &mut dyn toml_edit::TableLike, path: &[String]) {
    if path.len() == 1 {
        table.remove(&path[0]);
    } else if let Some(nested) = table.get_mut(&path[0]).and_then(Item::as_table_like_mut) {
        remove_path(nested, &path[1..]);
    }
}
fn validate_unset(path: &[String], scope: &ConfigScope) -> Result<(), CommandError> {
    if path.is_empty()
        || path.len() > 4
        || path.iter().any(|part| part.is_empty() || part.len() > 512)
    {
        return Err(CommandError::invalid("Invalid configuration reset path"));
    }
    let valid = match path[0].as_str() {
        "appearance" => {
            matches!(scope, ConfigScope::Global)
                && (path.len() == 1
                    || path.len() == 2 && ["theme", "language"].contains(&path[1].as_str()))
        }
        "models" => {
            matches!(scope, ConfigScope::Global)
                && (path.len() == 1 || path.len() == 2 && path[1] == "hidden_model_profile_ids")
        }
        "connectors" => matches!(scope, ConfigScope::Global) && path.len() <= 3,
        "chat" => {
            path.len() == 1
                || path.len() == 2
                    && [
                        "model_profile_id",
                        "thinking_level",
                        "run_options",
                        "use_memory",
                        "update_memory",
                    ]
                    .contains(&path[1].as_str())
                || path.len() == 3 && path[1] == "run_options"
        }
        "skills" => {
            // Resetting an always-on Skill's ignored setting is allowed.
            path.len() == 1
                || (path.len() == 2 || path.len() == 3 && path[2] == "enabled")
                    && crate::skills::valid_name(&path[1])
        }
        "pi" => {
            path.len() == 1
                || path.len() == 2 && ["compaction", "retry"].contains(&path[1].as_str())
                || path.len() == 3
                    && match path[1].as_str() {
                        "compaction" => {
                            ["reserve_tokens", "keep_recent_tokens"].contains(&path[2].as_str())
                        }
                        "retry" => [
                            "enabled",
                            "max_retries",
                            "base_delay_ms",
                            "max_agent_delay_ms",
                        ]
                        .contains(&path[2].as_str()),
                        _ => false,
                    }
        }
        _ => false,
    };
    if !valid {
        return Err(CommandError::invalid(
            "Unknown or unsupported configuration reset path",
        ));
    }
    Ok(())
}
pub fn update_scope(
    global_root: &Path,
    project_loupist_dir: Option<&Path>,
    request: &ConfigUpdateRequest,
) -> Result<ConfigSnapshot, CommandError> {
    validate(&request.set, &request.scope).map_err(config_error)?;
    // A request names only settings this version has; ignoring is for the file.
    if let Some(ignored) = ignore_unknown_catalog_keys(&mut request.set.clone())
        .map_err(config_error)?
        .into_iter()
        .next()
    {
        return Err(config_error(ignored));
    }
    for path in &request.unset {
        validate_unset(path, &request.scope)?;
    }
    let directory_path = scope_directory(global_root, project_loupist_dir, &request.scope)?;
    let _lock = WRITE_LOCK
        .lock()
        .map_err(|_| CommandError::internal("Configuration write lock is unavailable"))?;
    let directory = ConfigDirectory::open(directory_path)?;
    directory.check_revision(&request.expected_revision)?;
    let loaded = load(directory_path, &request.scope);
    if !loaded.diagnostics.is_empty() {
        return Err(config_error(
            "Configuration has errors; edit the file before using settings controls",
        ));
    }
    let mut document = loaded.document.unwrap_or_default();
    for path in &request.unset {
        remove_path(document.as_table_mut(), path);
    }
    let patch = serde_json::to_value(&request.set).map_err(io_error)?;
    update_table(
        document.as_table_mut(),
        patch.as_object().expect("config object"),
    )?;
    let source = document.to_string();
    let result: ConfigDocument = toml_edit::de::from_str(&source)
        .map_err(|_| config_error("Updated configuration is invalid"))?;
    // Settings the catalog no longer has stay as they were and do not block the save.
    validate(&result, &request.scope).map_err(config_error)?;
    directory.replace(source.as_bytes(), &request.expected_revision)?;
    read_scope(global_root, project_loupist_dir, request.scope.clone())
}

pub fn read_global_connector_settings(
    global_root: &Path,
    entry_id: &str,
) -> Result<BTreeMap<String, String>, CommandError> {
    Ok(resolve(global_root, None)?
        .connectors
        .remove(entry_id)
        .unwrap_or_default())
}
pub fn write_global_connector_settings(
    global_root: &Path,
    entry_id: &str,
    settings: &BTreeMap<String, String>,
) -> Result<(), CommandError> {
    let snapshot = read_scope(global_root, None, ConfigScope::Global)?;
    let unset = snapshot
        .explicit
        .as_ref()
        .and_then(|document| document.connectors.as_ref())
        .and_then(|connectors| connectors.get(entry_id))
        .into_iter()
        .flat_map(|previous| previous.keys())
        .filter(|key| !settings.contains_key(*key))
        .map(|key| vec!["connectors".into(), entry_id.to_owned(), key.clone()])
        .collect();
    update_scope(
        global_root,
        None,
        &ConfigUpdateRequest {
            scope: ConfigScope::Global,
            expected_revision: snapshot.revision,
            set: ConfigDocument {
                connectors: Some(BTreeMap::from([(entry_id.to_owned(), settings.clone())])),
                ..Default::default()
            },
            unset,
        },
    )?;
    Ok(())
}
pub fn read_hidden_model_profile_ids(global_root: &Path) -> Result<Vec<String>, CommandError> {
    Ok(resolve(global_root, None)?.models.hidden_model_profile_ids)
}
pub fn set_model_visible(
    global_root: &Path,
    model_profile_id: &str,
    visible: bool,
) -> Result<(), CommandError> {
    let snapshot = read_scope(global_root, None, ConfigScope::Global)?;
    let mut ids = snapshot.effective.models.hidden_model_profile_ids;
    ids.retain(|id| id != model_profile_id);
    if !visible {
        ids.push(model_profile_id.to_owned());
    }
    ids.sort();
    ids.dedup();
    update_scope(
        global_root,
        None,
        &ConfigUpdateRequest {
            scope: ConfigScope::Global,
            expected_revision: snapshot.revision,
            set: ConfigDocument {
                models: Some(ModelsConfig {
                    hidden_model_profile_ids: Some(ids),
                }),
                ..Default::default()
            },
            unset: Vec::new(),
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
