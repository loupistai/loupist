//! Tauri commands for sharing. Each takes typed intent, rechecks authority,
//! and keeps credentials, paths and network calls in Rust.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::Transaction;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{ipc::Channel, State};

use super::export::{self, Choice, ExportInput, Finding, PackMeta, ProfileInfo, Scope, Sections};
use super::github::{self, ApiError, Github, Poll};
use super::import::{self, Fetched, OpenResult};
use super::pack::{self, Files, License, Rejection};
use super::scan::{HeldPosition, PrivateContext};
use super::store::{self, Identity, ImportMode, ImportRow};
use super::update;
use crate::app::{AppState, CommandError};
use crate::knowledge::{shared_pack, BlobStore, KnowledgeError};
use crate::runs::maintenance::MaintenanceActivityKind;
use crate::store::LoupistStore;

fn internal(error: impl std::fmt::Display) -> CommandError {
    CommandError::internal(error.to_string())
}

fn knowledge_error(error: KnowledgeError) -> CommandError {
    match error {
        KnowledgeError::Invalid("world processing is busy") => CommandError::new(
            "world_busy",
            "The map is being updated. Try again in a minute.",
        ),
        KnowledgeError::NotFound => CommandError::not_found("record"),
        other => CommandError::invalid(other.to_string()),
    }
}

pub(crate) fn with_tx<T>(
    state: &AppState,
    loupist_id: &str,
    body: impl FnOnce(&Transaction<'_>, &BlobStore) -> Result<T, CommandError>,
) -> Result<T, CommandError> {
    let blobs = crate::commands::knowledge::knowledge_blobs_for_loupist(state, loupist_id)?;
    let mut connection = state.store.lock_loupist(loupist_id).map_err(internal)?;
    let tx = connection.transaction().map_err(internal)?;
    let value = body(&tx, &blobs)?;
    tx.commit().map_err(internal)?;
    Ok(value)
}

fn admit(
    state: &AppState,
    loupist_id: &str,
) -> Result<crate::domain::LoupistProfile, CommandError> {
    state.ensure_loupist_available(loupist_id)?;
    state
        .store
        .get_loupist(loupist_id)
        .map_err(internal)?
        .ok_or_else(|| CommandError::not_found("Loupist"))
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn tone_name(tone: crate::domain::LoupistTone) -> String {
    serde_json::to_value(tone)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "balanced".into())
}

// ---------------------------------------------------------------- export

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ScopeRequest {
    Loupist,
    #[serde(rename_all = "camelCase")]
    Item {
        record_id: String,
    },
}

impl ScopeRequest {
    fn scope(&self) -> Scope {
        match self {
            Self::Loupist => Scope::Loupist,
            Self::Item { record_id } => Scope::Item(record_id.clone()),
        }
    }
    fn echo(&self) -> Value {
        match self {
            Self::Loupist => serde_json::json!({"kind": "loupist"}),
            Self::Item { record_id } => serde_json::json!({"kind": "item", "recordId": record_id}),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShareSummaryRequest {
    loupist_id: String,
    scope: ScopeRequest,
    #[serde(default)]
    sections: Option<Sections>,
    #[serde(default)]
    excluded_item_keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShareRequest {
    loupist_id: String,
    scope: ScopeRequest,
    sections: Sections,
    excluded_item_keys: Vec<String>,
    acknowledged_finding_ids: Vec<String>,
    #[serde(default)]
    license: Option<License>,
    #[serde(default)]
    byline: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Destination {
    Folder,
    Zip,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShareExportRequest {
    loupist_id: String,
    scope: ScopeRequest,
    sections: Sections,
    excluded_item_keys: Vec<String>,
    acknowledged_finding_ids: Vec<String>,
    #[serde(default)]
    license: Option<License>,
    #[serde(default)]
    byline: Option<String>,
    destination: Destination,
}

impl ShareExportRequest {
    fn share(&self) -> ShareRequest {
        ShareRequest {
            loupist_id: self.loupist_id.clone(),
            scope: self.scope.clone(),
            sections: self.sections,
            excluded_item_keys: self.excluded_item_keys.clone(),
            acknowledged_finding_ids: self.acknowledged_finding_ids.clone(),
            license: self.license,
            byline: self.byline.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PublishTarget {
    Repo { name: String },
    Update(bool),
    Gist(String),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SharePublishRequest {
    loupist_id: String,
    scope: ScopeRequest,
    sections: Sections,
    excluded_item_keys: Vec<String>,
    acknowledged_finding_ids: Vec<String>,
    #[serde(default)]
    license: Option<License>,
    #[serde(default)]
    byline: Option<String>,
    target: PublishTarget,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoView {
    full_name: String,
    url: String,
    version: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemTitle {
    kind: &'static str,
    title: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareLicense {
    id: License,
    url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareSummary {
    loupist_name: String,
    scope: Value,
    item: Option<ItemTitle>,
    counts: export::SummaryCounts,
    findings: Vec<Finding>,
    allowed_licenses: Vec<ShareLicense>,
    default_license: License,
    license_conflict: Option<pack::LicenseConflict>,
    byline: Option<String>,
    repo: Option<RepoView>,
    next_version: u32,
}

/// Secrets Rust holds and the person's identifiers, for the private-string
/// scan: the GitHub token, every saved model-provider key or sign-in token,
/// every custom-connection secret and every finance connector credential.
/// The values are compared here and never leave Rust.
pub(crate) fn private_context(state: &AppState) -> PrivateContext {
    let mut context = PrivateContext::default();
    let environment = state.credential_environment.to_string();
    if let Some(token) = github::load_token(&environment) {
        context.secrets.push(token);
    }
    if let Ok(directory) = state.pi_agent_data_dir() {
        context
            .secrets
            .extend(crate::providers::catalog::saved_credential_values(
                &directory,
            ));
    }
    context
        .secrets
        .extend(crate::custom_connections::service::protected_values_for_state(state));
    if let Ok(catalog) = crate::marketplace::bundled_catalog() {
        for entry in catalog
            .entries
            .iter()
            .filter(|entry| !entry.runtime.credential_mapping.is_empty())
        {
            if let Ok(Some(bundle)) = crate::finance_credentials::get(&environment, &entry.id) {
                for field in entry.runtime.credential_mapping.keys() {
                    if let Some(value) = bundle.get(field).filter(|value| value.len() >= 8) {
                        context.secrets.push(value.to_owned());
                    }
                }
                if let Some(account) = bundle.get("account_number") {
                    context.accounts.push(account.to_owned());
                }
            }
        }
    }
    if let Ok(connection) = state.store.lock() {
        let positions = connection.prepare(&format!(
            "SELECT p.symbol, p.display_name, p.quantity, p.cost_basis, p.market_value
             FROM user_holdings_positions p JOIN ({}) s ON s.snapshot_id = p.snapshot_id",
            crate::store::user_context::LATEST_SNAPSHOTS
        ));
        if let Ok(mut statement) = positions {
            let rows = statement.query_map([], |row| {
                Ok(HeldPosition {
                    symbol: row.get(0)?,
                    names: row.get::<_, Option<String>>(1)?.into_iter().collect(),
                    amounts: [row.get::<_, Option<String>>(2)?, row.get(3)?, row.get(4)?]
                        .into_iter()
                        .flatten()
                        .collect(),
                })
            });
            if let Ok(rows) = rows {
                context.holdings.extend(rows.filter_map(Result::ok));
            }
        }
        // Wallet addresses are as private as account numbers.
        if let Ok(mut statement) = connection.prepare("SELECT address FROM user_wallet_addresses") {
            if let Ok(rows) = statement.query_map([], |row| row.get::<_, String>(0)) {
                context.accounts.extend(rows.filter_map(Result::ok));
            }
        }
    }
    context.personal = personal_names();
    context
}

fn personal_names() -> Vec<String> {
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    NAMES
        .get_or_init(|| {
            let mut names = Vec::new();
            #[cfg(target_os = "macos")]
            if let Ok(output) = std::process::Command::new("/usr/bin/id").arg("-F").output() {
                let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                if name.chars().count() >= 3 {
                    names.push(name);
                }
            }
            if let Some(home) = std::env::var_os("HOME") {
                let home = home.to_string_lossy().to_string();
                if home.len() > 6 {
                    names.push(home);
                }
            }
            names
        })
        .clone()
}

pub(crate) struct Gathered {
    pub(crate) input: ExportInput,
    pub(crate) identity: Identity,
    /// The shared Loupist this one was started from, and its ancestors.
    pub(crate) origin: Vec<pack::OriginEntry>,
}

/// Everything an export may draw from. The pack id is minted and kept on
/// first use, so item keys and finding ids agree between summary and export.
pub(crate) fn gather(state: &AppState, loupist_id: &str) -> Result<Gathered, CommandError> {
    let profile = admit(state, loupist_id)?;
    let (snapshot, (lens_origins, world_origins), (identity, origin)) =
        with_tx(state, loupist_id, |tx, _| {
            let identity = match store::identity(tx, loupist_id).map_err(internal)? {
                Some(identity) => identity,
                None => {
                    let identity = Identity {
                        pack_id: uuid::Uuid::new_v4().hyphenated().to_string(),
                        version: 1,
                        content_hash: None,
                        created_at: now_rfc3339(),
                        byline: None,
                        repo: None,
                        repo_url: None,
                        repo_commit: None,
                    };
                    store::save_identity(tx, loupist_id, &identity).map_err(internal)?;
                    identity
                }
            };
            Ok((
                shared_pack::export_snapshot(tx, loupist_id).map_err(knowledge_error)?,
                store::shared_licenses(tx, loupist_id).map_err(internal)?,
                (
                    identity,
                    origin_chain(store::fork_parent(tx, loupist_id).map_err(internal)?),
                ),
            ))
        })?;
    Ok(Gathered {
        input: ExportInput {
            profile: ProfileInfo {
                name: profile.name.clone(),
                focus: profile.focus.clone(),
                tone: tone_name(profile.tone),
                avatar: profile.avatar.clone(),
            },
            snapshot,
            lens_origins,
            world_origins,
        },
        identity,
        origin,
    })
}

/// A Loupist started from a shared one re-shares with that pack first in its
/// origin chain, then the pack's own ancestors (at most 8, nearest first).
fn origin_chain(parent: Option<ImportRow>) -> Vec<pack::OriginEntry> {
    let Some(parent) = parent else {
        return Vec::new();
    };
    let parent_id = parent.pack_id.clone();
    std::iter::once(pack::OriginEntry {
        id: parent.pack_id,
        name: parent.name,
        version: parent.version,
        url: parent.source_url,
        commit: parent.commit,
        author: parent.author,
        license: Some(parent.license),
    })
    .chain(
        parent
            .origin
            .into_iter()
            .filter(|origin| origin.id != parent_id),
    )
    .take(pack::MAX_ORIGIN)
    .collect()
}

pub(crate) fn require_shareable(license: License) -> Result<(), CommandError> {
    if license.redistributable() {
        Ok(())
    } else {
        Err(CommandError::new(
            "license_not_shareable",
            "This license does not allow sharing.",
        ))
    }
}

fn license_conflict(conflict: pack::LicenseConflict) -> CommandError {
    CommandError::new(
        conflict.code,
        format!(
            "These items have incompatible licenses: {}",
            conflict
                .items
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    )
}

/// A built pack, and the share identity to record once it is saved or published.
pub(crate) struct Built {
    pub(crate) files: Files,
    pub(crate) name: String,
    pub(crate) version: u32,
    pub(crate) identity: Identity,
    item_scope: bool,
}

fn has_content(selected: &export::Prepared) -> bool {
    selected.perspective.is_some()
        || !selected.methods.is_empty()
        || !selected.principles.is_empty()
        || !selected.claims.is_empty()
}

pub(crate) fn build_pack(
    state: &AppState,
    request: &ShareRequest,
    open_url: Option<String>,
) -> Result<Built, CommandError> {
    let gathered = gather(state, &request.loupist_id)?;
    let identity = gathered.identity;
    let origin = gathered.origin;
    let prepared = export::prepare(&gathered.input, &identity.pack_id);
    let findings = export::findings(&prepared, &private_context(state));
    let choice = Choice {
        scope: request.scope.scope(),
        sections: request.sections,
        excluded: request.excluded_item_keys.iter().cloned().collect(),
        acknowledged: request.acknowledged_finding_ids.iter().cloned().collect(),
    };
    let selected = export::select(&prepared, &choice);
    if !has_content(&selected) {
        return Err(CommandError::new(
            "nothing_to_share",
            "There is nothing to share with these parts turned on.",
        ));
    }
    if !export::unresolved(&findings, &selected, &choice).is_empty() {
        return Err(CommandError::new(
            "findings_unresolved",
            "Leave out the items that look private before sharing.",
        ));
    }
    let item_scope = matches!(choice.scope, Scope::Item(_));
    let allowed = export::license_options(&selected).map_err(license_conflict)?;
    let license = request.license.unwrap_or(allowed[0]);
    require_shareable(license)?;
    if !allowed.contains(&license) {
        return Err(CommandError::new(
            "license_conflict",
            format!(
                "The selected license is incompatible with: {}",
                export::licensed_items(&selected)
                    .iter()
                    .map(|item| item.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    let byline = request
        .byline
        .as_ref()
        .map(|byline| pack::fit(byline, 80))
        .filter(|byline| !byline.is_empty());
    let mut meta = PackMeta {
        pack_id: identity.pack_id.clone(),
        version: identity.version,
        created_at: identity.created_at.clone(),
        license,
        terms_url: None,
        byline: byline.clone(),
        notice: super::readme_notice(&selected.profile.language).to_owned(),
        open_url,
        app: env!("CARGO_PKG_VERSION").to_owned(),
        origin,
    };
    let hash = export::content_hash_for(&selected, &choice, &meta);
    if item_scope {
        meta.version = 1;
    } else if identity.content_hash.as_deref() != Some(hash.as_str()) {
        if identity.content_hash.is_some() {
            meta.version = identity.version + 1;
        }
        meta.created_at = now_rfc3339();
    }
    let (manifest, files) = export::build(&selected, &choice, &meta);
    Ok(Built {
        files,
        name: manifest.name.clone(),
        version: meta.version,
        identity: Identity {
            pack_id: identity.pack_id,
            version: meta.version,
            content_hash: Some(hash),
            created_at: meta.created_at,
            byline,
            repo: identity.repo,
            repo_url: identity.repo_url,
            repo_commit: identity.repo_commit,
        },
        item_scope,
    })
}

pub(crate) fn record_identity(
    state: &AppState,
    loupist_id: &str,
    built: &Built,
) -> Result<(), CommandError> {
    if built.item_scope {
        return Ok(());
    }
    with_tx(state, loupist_id, |tx, _| {
        store::save_identity(tx, loupist_id, &built.identity).map_err(internal)
    })
}

#[tauri::command(rename_all = "snake_case")]
pub fn share_summary(
    request: ShareSummaryRequest,
    state: State<'_, AppState>,
) -> Result<ShareSummary, CommandError> {
    let state = state.inner();
    let profile = admit(state, &request.loupist_id)?;
    let gathered = gather(state, &request.loupist_id)?;
    let identity = &gathered.identity;
    let prepared = export::prepare(&gathered.input, &identity.pack_id);
    let findings = export::findings(&prepared, &private_context(state));
    let choice = Choice {
        scope: request.scope.scope(),
        sections: request.sections.unwrap_or(Sections::ALL),
        excluded: request.excluded_item_keys.iter().cloned().collect(),
        acknowledged: BTreeSet::new(),
    };
    let selected = export::select(&prepared, &choice);
    let item = match &choice.scope {
        Scope::Loupist => None,
        Scope::Item(record_id) => {
            let key = prepared
                .record_keys
                .iter()
                .find(|(_, id)| *id == record_id)
                .map(|(key, _)| key.as_str());
            let method = prepared
                .perspective
                .iter()
                .chain(&prepared.methods)
                .find(|doc| Some(doc.key.as_str()) == key);
            let principle = prepared
                .principles
                .iter()
                .find(|doc| Some(doc.key.as_str()) == key);
            Some(match (method, principle) {
                (Some(method), _) => ItemTitle {
                    kind: "method",
                    title: method.title.clone(),
                },
                (_, Some(principle)) => ItemTitle {
                    kind: "principle",
                    title: principle.statement.clone(),
                },
                _ => return Err(CommandError::not_found("method or principle")),
            })
        }
    };
    let included: BTreeSet<String> = match &choice.scope {
        Scope::Loupist => findings.iter().map(|f| f.item_key.clone()).collect(),
        Scope::Item(_) => selected
            .methods
            .iter()
            .map(|doc| doc.key.clone())
            .chain(selected.principles.iter().map(|doc| doc.key.clone()))
            .collect(),
    };
    let (allowed_licenses, license_conflict) = match export::license_options(&selected) {
        Ok(licenses) => (licenses, None),
        Err(conflict) => (Vec::new(), Some(conflict)),
    };
    let default_license = allowed_licenses.first().copied().unwrap_or_default();
    let meta = PackMeta {
        pack_id: identity.pack_id.clone(),
        version: identity.version,
        created_at: identity.created_at.clone(),
        license: default_license,
        terms_url: None,
        byline: identity.byline.clone(),
        notice: String::new(),
        open_url: None,
        app: String::new(),
        origin: gathered.origin,
    };
    let next_version = match &identity.content_hash {
        Some(hash) if *hash == export::content_hash_for(&selected, &choice, &meta) => {
            identity.version
        }
        Some(_) => identity.version + 1,
        None => 1,
    };
    Ok(ShareSummary {
        loupist_name: profile.name,
        scope: request.scope.echo(),
        item,
        counts: export::counts(&selected),
        findings: findings
            .into_iter()
            .filter(|finding| included.contains(&finding.item_key))
            .collect(),
        allowed_licenses: allowed_licenses
            .into_iter()
            .map(|id| ShareLicense {
                id,
                url: id.url(None).to_owned(),
            })
            .collect(),
        default_license,
        license_conflict,
        byline: identity.byline.clone(),
        repo: identity
            .repo
            .as_ref()
            .zip(identity.repo_url.as_ref())
            .map(|(repo, url)| RepoView {
                full_name: repo.clone(),
                url: url.clone(),
                version: next_version,
            }),
        next_version,
    })
}

fn saved_paths() -> &'static Mutex<BTreeMap<String, PathBuf>> {
    static SAVED: std::sync::OnceLock<Mutex<BTreeMap<String, PathBuf>>> =
        std::sync::OnceLock::new();
    SAVED.get_or_init(Default::default)
}

fn write_new_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Writes a pack folder under `parent`; an existing name gets `-2`, `-3`, …
pub(crate) fn write_folder(parent: &Path, slug: &str, files: &Files) -> std::io::Result<PathBuf> {
    let mut target = parent.join(slug);
    let mut suffix = 2;
    loop {
        match std::fs::create_dir(&target) {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && suffix < 100 => {
                target = parent.join(format!("{slug}-{suffix}"));
                suffix += 1;
            }
            Err(error) => return Err(error),
        }
    }
    for (path, bytes) in files {
        let destination = target.join(path);
        if let Some(directory) = destination.parent() {
            std::fs::create_dir_all(directory)?;
        }
        write_new_file(&destination, bytes)?;
    }
    Ok(target)
}

/// A deterministic ZIP: sorted entries under one folder, fixed times, 0644.
pub(crate) fn zip_bytes(slug: &str, files: &Files) -> std::io::Result<Vec<u8>> {
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    for (path, bytes) in files {
        writer
            .start_file(format!("{slug}/{path}"), options)
            .map_err(std::io::Error::other)?;
        writer.write_all(bytes)?;
    }
    Ok(writer.finish().map_err(std::io::Error::other)?.into_inner())
}

fn write_zip(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| !metadata.is_file()) {
        return Err(std::io::Error::other("not a regular file"));
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(feature = "e2e")]
fn e2e_share_dir() -> Option<PathBuf> {
    std::env::var_os("LOUPIST_E2E_SHARE_DIR").map(PathBuf::from)
}
#[cfg(not(feature = "e2e"))]
fn e2e_share_dir() -> Option<PathBuf> {
    None
}

#[tauri::command(rename_all = "snake_case")]
pub async fn share_export(
    request: ShareExportRequest,
    state: State<'_, AppState>,
) -> Result<Option<Value>, CommandError> {
    let state = state.inner();
    let share = request.share();
    let built = build_pack(state, &share, None)?;
    let slug = pack::slug(&built.name, "loupist");
    let path = match request.destination {
        Destination::Folder => {
            let parent = match e2e_share_dir() {
                Some(directory) => directory,
                None => match rfd::AsyncFileDialog::new()
                    .set_title("Choose where to save the shared Loupist")
                    .pick_folder()
                    .await
                {
                    Some(folder) => folder.path().to_owned(),
                    None => return Ok(None),
                },
            };
            let files = built.files.clone();
            tauri::async_runtime::spawn_blocking(move || write_folder(&parent, &slug, &files))
                .await
                .map_err(internal)?
                .map_err(internal)?
        }
        Destination::Zip => {
            let path = match e2e_share_dir() {
                Some(directory) => directory.join(format!("{slug}.zip")),
                None => match rfd::AsyncFileDialog::new()
                    .set_title("Save the shared Loupist")
                    .set_file_name(format!("{slug}.zip"))
                    .add_filter("ZIP", &["zip"])
                    .save_file()
                    .await
                {
                    Some(file) => file.path().to_owned(),
                    None => return Ok(None),
                },
            };
            if path.extension().and_then(|e| e.to_str()) != Some("zip") {
                return Err(CommandError::invalid(
                    "Save the shared Loupist as a .zip file.",
                ));
            }
            let bytes = zip_bytes(&slug, &built.files).map_err(internal)?;
            let target = path.clone();
            tauri::async_runtime::spawn_blocking(move || write_zip(&target, &bytes))
                .await
                .map_err(internal)?
                .map_err(internal)?;
            path
        }
    };
    record_identity(state, &share.loupist_id, &built)?;
    if let Ok(mut saved) = saved_paths().lock() {
        saved.insert(share.loupist_id.clone(), path.clone());
    }
    Ok(Some(
        serde_json::json!({"path": path.display().to_string()}),
    ))
}

#[tauri::command(rename_all = "snake_case")]
pub fn share_copy_text(
    request: ShareRequest,
    state: State<'_, AppState>,
) -> Result<Value, CommandError> {
    if !matches!(request.scope, ScopeRequest::Item { .. }) {
        return Err(CommandError::invalid(
            "Copy as text shares one method or principle.",
        ));
    }
    let built = build_pack(state.inner(), &request, None)?;
    let text = built
        .files
        .iter()
        .find(|(path, _)| path.starts_with("method-") || path.starts_with("principle-"))
        .map(|(_, bytes)| String::from_utf8_lossy(bytes).to_string())
        .ok_or_else(|| CommandError::not_found("method or principle"))?;
    Ok(serde_json::json!({ "text": text }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoupistRequest {
    loupist_id: String,
}

#[tauri::command(rename_all = "snake_case")]
pub fn share_reveal(
    request: LoupistRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    admit(state.inner(), &request.loupist_id)?;
    let path = saved_paths()
        .lock()
        .ok()
        .and_then(|saved| saved.get(&request.loupist_id).cloned())
        .ok_or_else(|| CommandError::not_found("saved copy"))?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map_err(internal)?;
    #[cfg(windows)]
    std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map_err(internal)?;
    #[cfg(not(any(target_os = "macos", windows)))]
    open::that_detached(&path).map_err(internal)?;
    Ok(())
}

fn api_error(error: ApiError, environment: &str, state: &AppState) -> CommandError {
    match error {
        ApiError::Offline => CommandError::new("offline", "Couldn't reach GitHub."),
        ApiError::LicenseNotShareable => CommandError::new(
            "license_not_shareable",
            "This license does not allow sharing.",
        ),
        ApiError::NameTaken => CommandError::new("name_taken", "That repository name is taken."),
        ApiError::RateLimited => CommandError::new("rate_limited", "GitHub asked to slow down."),
        ApiError::Denied => {
            github::delete_token(environment);
            if let Ok(connection) = state.store.lock() {
                let _ = github::store_login(&connection, None);
            }
            CommandError::new("publish_denied", "GitHub didn't allow this. Sign in again.")
        }
        ApiError::NotFound => CommandError::new("repo_gone", "The repository is gone."),
        ApiError::TooLarge => CommandError::new("too_large", "The response was too large."),
        ApiError::Other(message) => CommandError::new("github_failed", message),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PublishEvent {
    Creating,
    Uploading { files: u32 },
}

#[tauri::command(rename_all = "snake_case")]
pub async fn share_publish(
    request: SharePublishRequest,
    on_event: Channel<PublishEvent>,
    state: State<'_, AppState>,
) -> Result<Value, CommandError> {
    let state = state.inner();
    if let Some(license) = request.license {
        require_shareable(license)?;
    }
    if github::client_id().is_none() {
        return Err(CommandError::new(
            "not_configured",
            "GitHub publishing isn't available in this version.",
        ));
    }
    let environment = state.credential_environment.to_string();
    let token = github::load_token(&environment)
        .ok_or_else(|| CommandError::new("signed_out", "Sign in to GitHub first."))?;
    let share = ShareRequest {
        loupist_id: request.loupist_id.clone(),
        scope: request.scope.clone(),
        sections: request.sections,
        excluded_item_keys: request.excluded_item_keys.clone(),
        acknowledged_finding_ids: request.acknowledged_finding_ids.clone(),
        license: request.license,
        byline: request.byline.clone(),
    };
    let api = Github::production();
    match request.target {
        PublishTarget::Gist(visibility) => {
            if !matches!(share.scope, ScopeRequest::Item { .. }) {
                return Err(CommandError::invalid(
                    "A gist shares one method or principle.",
                ));
            }
            let built = build_pack(state, &share, None)?;
            let _ = on_event.send(PublishEvent::Uploading {
                files: built.files.len() as u32,
            });
            let url = api
                .create_gist(&token, &built.files, &built.name, visibility == "public")
                .await
                .map_err(|error| api_error(error, &environment, state))?;
            Ok(
                serde_json::json!({"url": url, "fullName": null, "version": built.version, "commit": null}),
            )
        }
        target => {
            if !matches!(share.scope, ScopeRequest::Loupist) {
                return Err(CommandError::invalid("Publish a single method as a gist."));
            }
            // Validate everything before anything is created on GitHub.
            let checked = build_pack(state, &share, None)?;
            let repo = match target {
                PublishTarget::Repo { name } => {
                    if !github::valid_repo_name(&name) {
                        return Err(CommandError::new(
                            "name_taken",
                            "Choose another repository name.",
                        ));
                    }
                    let _ = on_event.send(PublishEvent::Creating);
                    // The Loupist's name leaves only with its Profile.
                    let description = if share.sections.profile {
                        format!("{} — a shared Loupist", checked.name)
                    } else {
                        "A shared Loupist".to_owned()
                    };
                    let repo = api
                        .create_repo(&token, &name, &description)
                        .await
                        .map_err(|error| api_error(error, &environment, state))?;
                    let _ = api.set_topic(&token, &repo.full_name).await;
                    repo
                }
                PublishTarget::Update(false) => {
                    return Err(CommandError::invalid("Choose a repository to publish to."))
                }
                _ => {
                    let recorded = checked.identity.repo.clone().ok_or_else(|| {
                        CommandError::new("repo_gone", "No repository is recorded.")
                    })?;
                    match api
                        .recorded_repo(&token, &recorded, &checked.identity.pack_id)
                        .await
                    {
                        Ok(repo) => repo,
                        Err(github::RecordedError::Api(error)) => {
                            return Err(api_error(error, &environment, state))
                        }
                        Err(github::RecordedError::Mismatch) => {
                            return Err(CommandError::new(
                                "repo_mismatch",
                                "That repository no longer holds this shared Loupist. Publish it as a new repository.",
                            ))
                        }
                    }
                }
            };
            let mut built = build_pack(state, &share, Some(repo.html_url.clone()))?;
            let _ = on_event.send(PublishEvent::Uploading {
                files: built.files.len() as u32,
            });
            let commit = api
                .commit_files(
                    &token,
                    &repo.full_name,
                    &repo.default_branch,
                    &built.files,
                    &format!("Loupist pack v{}", built.version),
                )
                .await
                .map_err(|error| api_error(error, &environment, state))?;
            built.identity.repo = Some(repo.full_name.clone());
            built.identity.repo_url = Some(repo.html_url.clone());
            built.identity.repo_commit = Some(commit.clone());
            record_identity(state, &share.loupist_id, &built)?;
            Ok(serde_json::json!({
                "url": repo.html_url,
                "fullName": repo.full_name,
                "version": built.version,
                "commit": commit,
            }))
        }
    }
}

// ---------------------------------------------------------------- GitHub sign-in

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubAccount {
    client_configured: bool,
    index_configured: bool,
    gallery_terms_url: Option<String>,
    login: Option<String>,
}

fn account(state: &AppState) -> GithubAccount {
    let environment = state.credential_environment.to_string();
    let login = github::load_token(&environment).and_then(|_| {
        state
            .store
            .lock()
            .ok()
            .and_then(|connection| github::stored_login(&connection).ok().flatten())
    });
    GithubAccount {
        client_configured: github::client_id().is_some(),
        index_configured: github::index_url().is_some(),
        gallery_terms_url: github::gallery_terms_url().map(str::to_owned),
        login,
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn github_account(state: State<'_, AppState>) -> GithubAccount {
    account(state.inner())
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SignInEvent {
    #[serde(rename_all = "camelCase")]
    Code {
        user_code: String,
        expires_in_sec: u64,
    },
    Waiting,
    SignedIn {
        login: String,
    },
    Failed {
        reason: &'static str,
    },
}

static SIGN_IN: AtomicU64 = AtomicU64::new(0);

fn verification() -> &'static Mutex<Option<String>> {
    static URI: Mutex<Option<String>> = Mutex::new(None);
    &URI
}

#[tauri::command(rename_all = "snake_case")]
pub async fn github_sign_in(
    on_event: Channel<SignInEvent>,
    state: State<'_, AppState>,
) -> Result<GithubAccount, CommandError> {
    let state = state.inner();
    let fail = |reason| {
        let _ = on_event.send(SignInEvent::Failed { reason });
    };
    let Some(client_id) = github::client_id() else {
        fail("not_configured");
        return Ok(account(state));
    };
    let generation = SIGN_IN.fetch_add(1, Ordering::SeqCst) + 1;
    let api = Github::production();
    let code = match api.device_code(client_id).await {
        Ok(code) => code,
        Err(_) => {
            fail("offline");
            return Ok(account(state));
        }
    };
    let uri = reqwest::Url::parse(&code.verification_uri)
        .ok()
        .filter(|url| url.scheme() == "https" && url.host_str() == Some("github.com"))
        .map(|url| url.to_string())
        .unwrap_or_else(|| "https://github.com/login/device".into());
    if let Ok(mut held) = verification().lock() {
        *held = Some(uri);
    }
    let _ = on_event.send(SignInEvent::Code {
        user_code: code.user_code.clone(),
        expires_in_sec: code.expires_in,
    });
    let _ = on_event.send(SignInEvent::Waiting);
    let deadline = Instant::now() + Duration::from_secs(code.expires_in.min(3_600));
    let mut interval = code.interval.max(1);
    let mut network_errors = 0;
    loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        if SIGN_IN.load(Ordering::SeqCst) != generation {
            return Ok(account(state));
        }
        if Instant::now() >= deadline {
            fail("expired");
            return Ok(account(state));
        }
        match api.poll_token(client_id, &code.device_code).await {
            Ok(Poll::Token(token)) => {
                let login = match api.login(&token).await {
                    Ok(login) => login,
                    Err(_) => {
                        fail("offline");
                        return Ok(account(state));
                    }
                };
                let environment = state.credential_environment.to_string();
                if !github::save_token(&environment, &token) {
                    fail("offline");
                    return Ok(account(state));
                }
                if let Ok(connection) = state.store.lock() {
                    github::store_login(&connection, Some(&login)).map_err(internal)?;
                }
                let _ = on_event.send(SignInEvent::SignedIn { login });
                return Ok(account(state));
            }
            Ok(Poll::Pending) => network_errors = 0,
            Ok(Poll::SlowDown(next)) => interval = next.unwrap_or(interval + 5).max(interval),
            Ok(Poll::Expired) => {
                fail("expired");
                return Ok(account(state));
            }
            Ok(Poll::Denied | Poll::Failed) => {
                fail("denied");
                return Ok(account(state));
            }
            Err(_) => {
                network_errors += 1;
                if network_errors >= 3 {
                    fail("offline");
                    return Ok(account(state));
                }
            }
        }
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn github_sign_in_open() -> Result<(), CommandError> {
    let uri = verification()
        .lock()
        .ok()
        .and_then(|held| held.clone())
        .ok_or_else(|| CommandError::not_found("sign-in code"))?;
    crate::web::external_browser::open(&uri).map_err(internal)
}

#[tauri::command(rename_all = "snake_case")]
pub fn github_sign_in_cancel() {
    SIGN_IN.fetch_add(1, Ordering::SeqCst);
}

#[tauri::command(rename_all = "snake_case")]
pub fn github_sign_out(state: State<'_, AppState>) -> Result<GithubAccount, CommandError> {
    let state = state.inner();
    github::delete_token(&state.credential_environment);
    if let Ok(connection) = state.store.lock() {
        github::store_login(&connection, None).map_err(internal)?;
    }
    Ok(account(state))
}

// ---------------------------------------------------------------- import

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum PackSource {
    Url {
        url: String,
    },
    Zip {
        #[allow(dead_code)]
        zip: bool,
    },
    Folder {
        #[allow(dead_code)]
        folder: bool,
    },
    Pending {
        #[allow(dead_code)]
        pending: bool,
    },
    /// A Gallery card: Rust resolves the listed repository, path and commit.
    Gallery {
        gallery: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackOpenRequest {
    source: PackSource,
}

fn link_error(error: ApiError) -> Result<Fetched, Result<OpenResult, CommandError>> {
    Err(match error {
        ApiError::NotFound | ApiError::Denied => Err(CommandError::new(
            "link_not_found",
            "This link isn't public, or it moved.",
        )),
        ApiError::TooLarge => Ok(OpenResult::Rejected {
            reason: Rejection::TooLarge,
            source_label: None,
        }),
        ApiError::RateLimited => Err(CommandError::new(
            "rate_limited",
            "GitHub asked to slow down.",
        )),
        _ => Err(CommandError::new("offline", "Couldn't reach GitHub.")),
    })
}

async fn fetch_github(
    state: &AppState,
    location: super::deep_link::GithubLocation,
) -> Result<Fetched, Result<OpenResult, CommandError>> {
    use super::deep_link::GithubLocation;
    let api = Github::production();
    let token = github::load_token(&state.credential_environment);
    let label = Some(location.label());
    let source_url = Some(location.url());
    match &location {
        GithubLocation::Repo {
            owner,
            repo,
            reference,
            path,
        } => {
            let commit = match api
                .resolve_commit(token.as_deref(), owner, repo, reference.as_deref())
                .await
            {
                Err(ApiError::Denied) => {
                    api.resolve_commit(None, owner, repo, reference.as_deref())
                        .await
                }
                other => other,
            };
            let commit = match commit {
                Ok(commit) => commit,
                Err(error) => return link_error(error),
            };
            let bytes = match api.archive(owner, repo, &commit).await {
                Ok(bytes) => bytes,
                Err(error) => return link_error(error),
            };
            let files = import::read_zip(&bytes, path.as_deref()).map_err(|reason| {
                Ok(OpenResult::Rejected {
                    reason,
                    source_label: label.clone(),
                })
            })?;
            Ok(Fetched {
                files,
                source_url,
                label,
                repo: Some(format!("{owner}/{repo}")),
                path: path.clone(),
                commit: Some(commit),
            })
        }
        GithubLocation::Gist { user, id } => {
            let files = match api.gist(token.as_deref(), user, id).await {
                Err(ApiError::Denied) => api.gist(None, user, id).await,
                other => other,
            };
            match files {
                Ok(files) => Ok(Fetched {
                    files,
                    source_url,
                    label,
                    repo: None,
                    path: None,
                    commit: None,
                }),
                Err(error) => link_error(error),
            }
        }
    }
}

#[cfg(feature = "e2e")]
fn e2e_pack_path() -> Option<PathBuf> {
    std::env::var_os("LOUPIST_E2E_PACK_PATH").map(PathBuf::from)
}
#[cfg(not(feature = "e2e"))]
fn e2e_pack_path() -> Option<PathBuf> {
    None
}

fn manifest_id(files: &Files) -> Option<String> {
    let value: Value = serde_json::from_slice(files.get(pack::MANIFEST)?).ok()?;
    value["id"].as_str().map(str::to_owned)
}

#[derive(Debug, Clone, Copy)]
enum Local {
    Zip,
    Folder,
}

/// A ZIP or folder the person picks in the native dialog; None when cancelled.
async fn fetch_local(kind: Local) -> Result<Option<Fetched>, Result<OpenResult, CommandError>> {
    let fetched = match kind {
        Local::Zip => {
            let path = match e2e_pack_path() {
                Some(path) => path,
                None => match rfd::AsyncFileDialog::new()
                    .set_title("Choose a shared Loupist ZIP")
                    .add_filter("ZIP", &["zip"])
                    .pick_file()
                    .await
                {
                    Some(file) => file.path().to_owned(),
                    None => return Ok(None),
                },
            };
            let bytes = tauri::async_runtime::spawn_blocking(move || {
                crate::fs::artifact_trust::read_private_regular_file_bounded(
                    &path,
                    pack::MAX_BYTES as u64,
                )
            })
            .await
            .map_err(|error| Err(internal(error)))?;
            match bytes {
                Err(_) => Err(Ok(OpenResult::Rejected {
                    reason: Rejection::TooLarge,
                    source_label: None,
                })),
                Ok(bytes) => import::read_zip(&bytes, None)
                    .map(|files| Fetched {
                        files,
                        ..Fetched::default()
                    })
                    .map_err(|reason| {
                        Ok(OpenResult::Rejected {
                            reason,
                            source_label: None,
                        })
                    }),
            }
        }
        Local::Folder => {
            let folder = match e2e_pack_path() {
                Some(path) => path,
                None => match rfd::AsyncFileDialog::new()
                    .set_title("Choose a shared Loupist folder")
                    .pick_folder()
                    .await
                {
                    Some(folder) => folder.path().to_owned(),
                    None => return Ok(None),
                },
            };
            tauri::async_runtime::spawn_blocking(move || import::read_folder(&folder))
                .await
                .map_err(|error| Err(internal(error)))?
                .map(|files| Fetched {
                    files,
                    ..Fetched::default()
                })
                .map_err(|reason| {
                    Ok(OpenResult::Rejected {
                        reason,
                        source_label: None,
                    })
                })
        }
    };
    fetched.map(Some)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn pack_open(
    request: PackOpenRequest,
    state: State<'_, AppState>,
) -> Result<Option<OpenResult>, CommandError> {
    let state = state.inner();
    let fetched = match request.source {
        PackSource::Url { url } => {
            let Some(location) = super::deep_link::parse_github(&url) else {
                return Err(CommandError::new(
                    "link_invalid",
                    "Use a github.com or gist.github.com link.",
                ));
            };
            fetch_github(state, location).await
        }
        PackSource::Pending { .. } => {
            let Some(location) = super::deep_link::held() else {
                return Err(CommandError::new(
                    "link_invalid",
                    "This link can't be opened.",
                ));
            };
            let fetched = fetch_github(state, location).await;
            // A network failure keeps the link so Preview can be tried again.
            if !matches!(fetched, Err(Err(_))) {
                super::deep_link::dismiss();
            }
            fetched
        }
        PackSource::Zip { .. } => match fetch_local(Local::Zip).await {
            Ok(None) => return Ok(None),
            Ok(Some(fetched)) => Ok(fetched),
            Err(error) => Err(error),
        },
        PackSource::Folder { .. } => match fetch_local(Local::Folder).await {
            Ok(None) => return Ok(None),
            Ok(Some(fetched)) => Ok(fetched),
            Err(error) => Err(error),
        },
        PackSource::Gallery { gallery } => {
            // The renderer names a card; the location comes from the index Rust read.
            let entry = match super::gallery::cached_entry(&gallery) {
                Some(entry) => Some(entry),
                None => super::gallery::load(Some(&gallery_cache(state)), false)
                    .await
                    .and_then(|_| super::gallery::cached_entry(&gallery)),
            };
            let Some(entry) = entry else {
                return Err(CommandError::new(
                    "link_not_found",
                    "This shared Loupist is no longer listed.",
                ));
            };
            fetch_github(state, entry.location()).await
        }
    };
    let fetched = match fetched {
        Ok(fetched) => fetched,
        Err(Ok(result)) => return Ok(Some(result)),
        Err(Err(error)) => return Err(error),
    };
    let blocked = match manifest_id(&fetched.files) {
        Some(id) => {
            super::gallery::blocked_reason(
                Some(&gallery_cache(state)),
                &id,
                fetched.repo.as_deref(),
            )
            .await
        }
        None => None,
    };
    Ok(Some(import::stage(fetched, blocked)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AddMode {
    New {
        name: String,
        tone: crate::domain::LoupistTone,
    },
    Merge {
        #[serde(rename = "loupistId")]
        loupist_id: String,
    },
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddSections {
    pub(crate) perspective: bool,
    pub(crate) methods: bool,
    pub(crate) principles: bool,
    pub(crate) map: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackAddRequest {
    staging_id: String,
    mode: AddMode,
    sections: AddSections,
}

#[derive(Debug, Clone, Serialize)]
pub struct AddProgress {
    done: u32,
    total: u32,
}

async fn wait_for_world(state: &AppState, loupist_id: &str) -> Result<(), CommandError> {
    for _ in 0..300 {
        let running = with_tx(state, loupist_id, |tx, _| {
            shared_pack::world_job_running(tx, loupist_id).map_err(knowledge_error)
        })?;
        if !running {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err(CommandError::new(
        "world_busy",
        "The map is being updated. Try again in a minute.",
    ))
}

/// Records the import, its items as pending suggestions and its World
/// content in one transaction; nothing reaches the Lens before Accept.
pub(crate) fn record_import(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    loupist_id: &str,
    staged: &import::Staged,
    row: &ImportRow,
    sections: AddSections,
    progress: &dyn Fn(u32),
) -> Result<(), CommandError> {
    store::insert_import(tx, loupist_id, row).map_err(internal)?;
    let mut done = 0;
    for (key, kind, item) in &staged.items {
        let wanted = match *kind {
            "perspective" => sections.perspective && row.mode == ImportMode::New,
            "method" => sections.methods,
            _ => sections.principles,
        };
        if !wanted {
            continue;
        }
        let body_json = serde_json::to_string(item).map_err(internal)?;
        let flags = serde_json::to_string(
            &item
                .findings
                .iter()
                .map(|finding| finding.flag)
                .collect::<Vec<_>>(),
        )
        .map_err(internal)?;
        store::insert_item(
            tx,
            loupist_id,
            &store::ItemRow {
                import_id: row.import_id.clone(),
                key: key.clone(),
                kind: (*kind).to_owned(),
                title: item.title.clone(),
                body_hash: pack::sha256_hex(body_json.as_bytes()),
                body_json,
                flags,
                state: "pending".into(),
                lens_record_id: None,
                update_json: None,
            },
        )
        .map_err(internal)?;
        done += 1;
        progress(done);
    }
    let parsed = &staged.parsed;
    if sections.map && !parsed.claims.is_empty() {
        let imported = shared_pack::import_world(
            tx,
            blobs,
            loupist_id,
            &shared_pack::WorldImport {
                import_id: &row.import_id,
                pack_id: &row.pack_id,
                pack_name: &row.name,
                author: row.author.as_deref(),
                license: row.license.id(),
                entities: &parsed.entities,
                claims: &parsed.claims,
                sources: &parsed.sources,
                at_ms: row.added_at_ms,
                known_spans: &BTreeMap::new(),
                withdraw: &BTreeSet::new(),
                version: None,
            },
        )
        .map_err(knowledge_error)?;
        for (kind, subjects) in [
            ("node", &imported.nodes_added),
            ("claim", &imported.claims_added),
            ("source", &imported.source_records),
            ("span", &imported.spans),
        ] {
            for subject in subjects {
                store::insert_world_ref(tx, loupist_id, &row.import_id, kind, subject)
                    .map_err(internal)?;
            }
        }
        progress(done + 1);
    }
    Ok(())
}

pub(crate) fn import_row(staged: &import::Staged, mode: ImportMode, at_ms: i64) -> ImportRow {
    let manifest = &staged.parsed.manifest;
    ImportRow {
        import_id: format!("imp_{}", uuid::Uuid::new_v4().simple()),
        pack_id: manifest.id.clone(),
        name: manifest.name.clone(),
        author: manifest
            .author
            .as_ref()
            .map(|author| author.display.clone()),
        version: manifest.version,
        license: manifest.license,
        source_url: staged.fetched.source_url.clone(),
        repo: staged.fetched.repo.clone(),
        path: staged.fetched.path.clone(),
        commit: staged.fetched.commit.clone(),
        mode,
        added_at_ms: at_ms,
        blocked_reason: None,
        origin: store::manifest_origins(manifest),
        checked_at_ms: None,
    }
}

/// Eval and test seeding: opens a pack folder, adds it to an existing Loupist
/// and accepts every item one at a time, as a person accepting each would.
#[cfg(any(test, feature = "eval-harness"))]
pub(crate) fn seed_shared_pack(
    state: &AppState,
    loupist_id: &str,
    folder: &Path,
) -> Result<String, CommandError> {
    let files = import::read_folder(folder)
        .map_err(|reason| CommandError::invalid(format!("shared pack rejected: {reason:?}")))?;
    let OpenResult::Preview { preview } = import::stage(
        Fetched {
            files,
            ..Fetched::default()
        },
        None,
    ) else {
        return Err(CommandError::invalid("shared pack rejected"));
    };
    let staged = import::staged(&preview.staging_id)
        .ok_or_else(|| CommandError::not_found("staged pack"))?;
    let row = import_row(&staged, ImportMode::Merge, crate::commands::now_ms());
    let sections = AddSections {
        perspective: false,
        methods: true,
        principles: true,
        map: true,
    };
    with_tx(state, loupist_id, |tx, blobs| {
        record_import(tx, blobs, loupist_id, &staged, &row, sections, &|_| {})?;
        for (key, kind, _) in &staged.items {
            if *kind != "perspective" {
                decide(
                    tx,
                    loupist_id,
                    &row.import_id,
                    std::slice::from_ref(key),
                    Decision::Accept,
                    row.added_at_ms,
                )?;
            }
        }
        Ok(())
    })?;
    import::unstage(&preview.staging_id);
    Ok(row.import_id)
}

pub(crate) fn retain_copy(
    state: &AppState,
    loupist_id: &str,
    import_id: &str,
    version: u32,
    files: &Files,
) {
    if !state.store.is_folder_catalog() {
        return;
    }
    let Ok(directory) = state.store.loupist_data_dir(loupist_id) else {
        return;
    };
    let target = directory
        .join("packs")
        .join(import_id)
        .join(version.to_string());
    if std::fs::create_dir_all(&target).is_err() {
        return;
    }
    for (path, bytes) in files {
        let destination = target.join(path);
        if let Some(parent) = destination.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = write_new_file(&destination, bytes);
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn pack_add(
    request: PackAddRequest,
    on_event: Channel<AddProgress>,
    state: State<'_, AppState>,
) -> Result<Value, CommandError> {
    add(state.inner(), request, &|progress| {
        let _ = on_event.send(progress);
    })
    .await
}

/// Adds a staged pack: a new Loupist or a merge, its suggestions and its map.
pub(crate) async fn add(
    state: &AppState,
    request: PackAddRequest,
    send: &(dyn Fn(AddProgress) + Send + Sync),
) -> Result<Value, CommandError> {
    let staged = import::staged(&request.staging_id).ok_or_else(|| {
        CommandError::new(
            "staging_expired",
            "Open the shared Loupist again to add it.",
        )
    })?;
    if staged.blocked.is_some() {
        return Err(CommandError::new(
            "safety_blocked",
            "This shared Loupist was removed from the Gallery.",
        ));
    }
    let sections = request.sections;
    let new = matches!(request.mode, AddMode::New { .. });
    let item_count = staged
        .items
        .iter()
        .filter(|(_, kind, _)| match *kind {
            "perspective" => sections.perspective && new,
            "method" => sections.methods,
            _ => sections.principles,
        })
        .count() as u32;
    let world = sections.map && !staged.parsed.claims.is_empty();
    let total = 1 + item_count + u32::from(world);
    send(AddProgress { done: 0, total });
    let (loupist_id, mode) = match request.mode {
        AddMode::New { name, tone } => {
            let profile = staged.parsed.profile.as_ref();
            let created = crate::commands::loupist::create_managed_loupist(
                state,
                crate::commands::types::LoupistCreateRequest {
                    name,
                    focus: staged
                        .focus_clean
                        .then(|| staged.profile.focus.clone())
                        .filter(|focus| !focus.trim().is_empty()),
                    avatar: profile
                        .map(|profile| profile.avatar.clone())
                        .filter(|avatar| {
                            crate::domain::LOUPIST_AVATAR_IDS.contains(&avatar.as_str())
                        }),
                    template: None,
                    tone: Some(tone),
                },
            )
            .await?;
            (created.id, ImportMode::New)
        }
        AddMode::Merge { loupist_id } => {
            admit(state, &loupist_id)?;
            (loupist_id, ImportMode::Merge)
        }
    };
    send(AddProgress { done: 1, total });
    let row = import_row(&staged, mode, crate::commands::now_ms());
    let result = async {
        // Wait for a running World job before taking the maintenance lease, so
        // a long wait never holds back other maintenance.
        if world {
            wait_for_world(state, &loupist_id).await?;
        }
        let _activity = state
            .maintenance
            .admit_kind(MaintenanceActivityKind::MemoryMutation)?;
        let progress = |done: u32| {
            send(AddProgress {
                done: 1 + done,
                total,
            });
        };
        with_tx(state, &loupist_id, |tx, blobs| {
            record_import(tx, blobs, &loupist_id, &staged, &row, sections, &progress)
        })
    }
    .await;
    if let Err(error) = result {
        if mode == ImportMode::New {
            let _ = crate::commands::loupist::delete_loupist_inner(state, &loupist_id).await;
        }
        return Err(match error.code.as_str() {
            "world_busy" => error,
            _ => CommandError::new("add_failed", error.message),
        });
    }
    retain_copy(
        state,
        &loupist_id,
        &row.import_id,
        row.version,
        &staged.fetched.files,
    );
    import::unstage(&request.staging_id);
    send(AddProgress { done: total, total });
    Ok(serde_json::json!({"loupistId": loupist_id, "importId": row.import_id}))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Removable {
    names: u32,
    links: u32,
    accepted: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackImportView {
    import_id: String,
    pack_id: String,
    name: String,
    author: Option<String>,
    version: u32,
    license: License,
    source_url: Option<String>,
    added_at_ms: i64,
    mode: ImportMode,
    blocked_reason: Option<String>,
    removable: Removable,
    /// When "Check for updates" last ran; null before the first check.
    checked_at_ms: Option<i64>,
}

pub(crate) fn list_imports(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> Result<Vec<PackImportView>, CommandError> {
    let mut views = Vec::new();
    for row in store::imports(tx, loupist_id).map_err(internal)? {
        let refs = store::world_refs(tx, loupist_id, &row.import_id).map_err(internal)?;
        let (names, links) =
            shared_pack::removable_world(tx, loupist_id, &refs).map_err(knowledge_error)?;
        let accepted = store::items(tx, loupist_id, &row.import_id)
            .map_err(internal)?
            .iter()
            .filter(|item| item.state == "accepted")
            .count() as u32;
        views.push(PackImportView {
            import_id: row.import_id,
            pack_id: row.pack_id,
            name: row.name,
            author: row.author,
            version: row.version,
            license: row.license,
            source_url: row.source_url,
            added_at_ms: row.added_at_ms,
            mode: row.mode,
            blocked_reason: row.blocked_reason,
            removable: Removable {
                names: names as u32,
                links: links as u32,
                accepted,
            },
            checked_at_ms: row.checked_at_ms,
        });
    }
    Ok(views)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn pack_list(
    request: LoupistRequest,
    state: State<'_, AppState>,
) -> Result<Vec<PackImportView>, CommandError> {
    let state = state.inner();
    admit(state, &request.loupist_id)?;
    refresh_blocked(state, &request.loupist_id).await?;
    with_tx(state, &request.loupist_id, |tx, _| {
        list_imports(tx, &request.loupist_id)
    })
}

/// Report-and-remove: the community index's blocked list (read at most every
/// 6 h) marks the added packs it names; their rows offer Remove what it added.
async fn refresh_blocked(state: &AppState, loupist_id: &str) -> Result<(), CommandError> {
    let Some(blocked) = super::gallery::blocked_list(Some(&gallery_cache(state))).await else {
        return Ok(());
    };
    with_tx(state, loupist_id, |tx, _| {
        mark_blocked(tx, loupist_id, &blocked)
    })
}

/// Records, per added pack, whether the blocked list names it (cleared once it no longer does).
pub(crate) fn mark_blocked(
    tx: &Transaction<'_>,
    loupist_id: &str,
    blocked: &[super::gallery::Blocked],
) -> Result<(), CommandError> {
    for row in store::imports(tx, loupist_id).map_err(internal)? {
        let reason = super::gallery::find(blocked, &row.pack_id, row.repo.as_deref())
            .map(super::gallery::reason);
        if reason != row.blocked_reason.as_deref() {
            store::set_blocked(tx, loupist_id, &row.import_id, reason).map_err(internal)?;
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    key: String,
    kind: String,
    title: String,
    flags: Vec<Value>,
    state: String,
    /// The author's newer text for an item already accepted (W19.5).
    update: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionGroup {
    import_id: String,
    pack_name: String,
    items: Vec<Suggestion>,
}

/// The author's newer text waiting on an accepted item.
fn pending_update(item: &store::ItemRow) -> Option<import::StoredItem> {
    (item.state == "accepted")
        .then_some(item.update_json.as_deref())
        .flatten()
        .and_then(|json| serde_json::from_str(json).ok())
}

/// A suggestion still waits: a pending item, or an accepted one with newer text.
fn waiting(item: &store::ItemRow) -> bool {
    item.state == "pending" || pending_update(item).is_some()
}

pub(crate) fn suggestion_groups(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> Result<Vec<SuggestionGroup>, CommandError> {
    let mut groups = Vec::new();
    for row in store::imports(tx, loupist_id).map_err(internal)? {
        let items: Vec<Suggestion> = store::items(tx, loupist_id, &row.import_id)
            .map_err(internal)?
            .into_iter()
            .filter(waiting)
            .map(|item| match pending_update(&item) {
                Some(update) => Suggestion {
                    key: item.key,
                    kind: item.kind,
                    title: update.title.clone(),
                    flags: serde_json::from_str(&update::flags_json(&update)).unwrap_or_default(),
                    state: "pending".into(),
                    update: true,
                },
                None => Suggestion {
                    key: item.key,
                    kind: item.kind,
                    title: item.title,
                    flags: serde_json::from_str(&item.flags).unwrap_or_default(),
                    state: item.state,
                    update: false,
                },
            })
            .collect();
        if !items.is_empty() {
            groups.push(SuggestionGroup {
                import_id: row.import_id,
                pack_name: row.name,
                items,
            });
        }
    }
    Ok(groups)
}

#[tauri::command(rename_all = "snake_case")]
pub fn pack_suggestions(
    request: LoupistRequest,
    state: State<'_, AppState>,
) -> Result<Vec<SuggestionGroup>, CommandError> {
    admit(state.inner(), &request.loupist_id)?;
    with_tx(state.inner(), &request.loupist_id, |tx, _| {
        suggestion_groups(tx, &request.loupist_id)
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackItemRequest {
    loupist_id: String,
    import_id: String,
    key: String,
}

#[tauri::command(rename_all = "snake_case")]
pub fn pack_item_read(
    request: PackItemRequest,
    state: State<'_, AppState>,
) -> Result<import::ItemView, CommandError> {
    admit(state.inner(), &request.loupist_id)?;
    with_tx(state.inner(), &request.loupist_id, |tx, _| {
        let item = store::items(tx, &request.loupist_id, &request.import_id)
            .map_err(internal)?
            .into_iter()
            .find(|item| item.key == request.key)
            .ok_or_else(|| CommandError::not_found("suggestion"))?;
        let body = match (&item.update_json, item.state.as_str()) {
            (Some(update), "accepted") => update,
            _ => &item.body_json,
        };
        import::stored_view(&item.key, &item.kind, body)
            .ok_or_else(|| CommandError::internal("stored suggestion is unreadable"))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Accept,
    Skip,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackDecideRequest {
    loupist_id: String,
    import_id: String,
    item_keys: Vec<String>,
    decision: Decision,
}

/// Accept creates ordinary Lens records with shared origin; flagged items one at a time.
pub(crate) fn decide(
    tx: &Transaction<'_>,
    loupist_id: &str,
    import_id: &str,
    keys: &[String],
    decision: Decision,
    at_ms: i64,
) -> Result<(), CommandError> {
    if keys.is_empty() || keys.len() > 400 {
        return Err(CommandError::invalid(
            "Choose between 1 and 400 suggestions.",
        ));
    }
    let row = store::import(tx, loupist_id, import_id)
        .map_err(internal)?
        .ok_or_else(|| CommandError::not_found("shared Loupist"))?;
    let items = store::items(tx, loupist_id, import_id).map_err(internal)?;
    let chosen: Vec<&store::ItemRow> = keys
        .iter()
        .map(|key| {
            items
                .iter()
                .find(|item| &item.key == key && waiting(item))
                .ok_or_else(|| CommandError::not_found("suggestion"))
        })
        .collect::<Result<_, _>>()?;
    let flagged = |item: &store::ItemRow| match pending_update(item) {
        Some(update) => !update.findings.is_empty(),
        None => item.flags != "[]",
    };
    if decision == Decision::Accept && chosen.len() > 1 && chosen.iter().any(|item| flagged(item)) {
        return Err(CommandError::new(
            "flagged_one_at_a_time",
            "Accept items with warnings one at a time.",
        ));
    }
    for item in chosen {
        if let Some(theirs) = pending_update(item) {
            // Skip keeps the accepted text; Accept takes the author's newer one.
            let mut kept = store::ItemRow {
                update_json: None,
                ..item.clone()
            };
            if decision == Decision::Accept
                && update::take_theirs(tx, loupist_id, &row, item, &theirs, at_ms)?
            {
                let (body_json, body_hash) = import::body_of(&theirs);
                kept = store::ItemRow {
                    title: theirs.title.clone(),
                    body_json,
                    body_hash,
                    flags: update::flags_json(&theirs),
                    ..kept
                };
            }
            store::set_item_body(tx, loupist_id, &kept).map_err(internal)?;
            continue;
        }
        if decision == Decision::Skip {
            store::set_item_state(tx, loupist_id, import_id, &item.key, "skipped", None)
                .map_err(internal)?;
            continue;
        }
        if pack::sha256_hex(item.body_json.as_bytes()) != item.body_hash {
            return Err(CommandError::invalid(
                "This suggestion was changed after it was added.",
            ));
        }
        let body: import::StoredItem = serde_json::from_str(&item.body_json).map_err(internal)?;
        let origin = import::lens_origin(&row);
        let record = match item.kind.as_str() {
            "method" => Some(
                shared_pack::create_shared_method(
                    tx,
                    loupist_id,
                    &body.title,
                    &row.name,
                    body.document
                        .as_ref()
                        .ok_or_else(|| CommandError::internal("method body"))?,
                    origin,
                    at_ms,
                )
                .map_err(knowledge_error)?,
            ),
            "perspective" if row.mode == ImportMode::New => Some(
                shared_pack::replace_perspective(
                    tx,
                    loupist_id,
                    &body.title,
                    body.document
                        .as_ref()
                        .ok_or_else(|| CommandError::internal("perspective body"))?,
                    origin,
                    at_ms,
                )
                .map_err(knowledge_error)?,
            ),
            "perspective" => {
                return Err(CommandError::invalid(
                    "Your principles and perspective stay as they are.",
                ))
            }
            _ => shared_pack::create_shared_principle(
                tx,
                loupist_id,
                body.statement.as_deref().unwrap_or(&body.title),
                body.why.as_deref(),
                body.example.as_deref(),
                at_ms,
            )
            .map_err(knowledge_error)?,
        };
        match record {
            Some(record) => store::set_item_state(
                tx,
                loupist_id,
                import_id,
                &item.key,
                "accepted",
                Some(&record),
            ),
            // The Loupist already holds this principle.
            None => store::set_item_state(tx, loupist_id, import_id, &item.key, "skipped", None),
        }
        .map_err(internal)?;
    }
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub fn pack_decide(
    request: PackDecideRequest,
    state: State<'_, AppState>,
) -> Result<Vec<SuggestionGroup>, CommandError> {
    let state = state.inner();
    admit(state, &request.loupist_id)?;
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::MemoryMutation)?;
    with_tx(state, &request.loupist_id, |tx, _| {
        decide(
            tx,
            &request.loupist_id,
            &request.import_id,
            &request.item_keys,
            request.decision,
            crate::commands::now_ms(),
        )?;
        suggestion_groups(tx, &request.loupist_id)
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackRemoveRequest {
    loupist_id: String,
    import_id: String,
    archive_accepted: bool,
}

pub(crate) fn remove(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    loupist_id: &str,
    import_id: &str,
    archive_accepted: bool,
    at_ms: i64,
) -> Result<(usize, usize), CommandError> {
    let row = store::import(tx, loupist_id, import_id)
        .map_err(internal)?
        .ok_or_else(|| CommandError::not_found("shared Loupist"))?;
    let refs = store::world_refs(tx, loupist_id, import_id).map_err(internal)?;
    let removed =
        shared_pack::remove_world(tx, blobs, loupist_id, import_id, &row.name, &refs, at_ms)
            .map_err(knowledge_error)?;
    if archive_accepted {
        for item in store::items(tx, loupist_id, import_id).map_err(internal)? {
            if let (Some(record), "accepted") =
                (item.lens_record_id.as_deref(), item.state.as_str())
            {
                shared_pack::archive_lens(tx, loupist_id, record, at_ms)
                    .map_err(knowledge_error)?;
            }
        }
    }
    store::mark_removed(tx, loupist_id, import_id, at_ms).map_err(internal)?;
    Ok(removed)
}

#[tauri::command(rename_all = "snake_case")]
pub fn pack_remove(
    request: PackRemoveRequest,
    state: State<'_, AppState>,
) -> Result<Vec<PackImportView>, CommandError> {
    let state = state.inner();
    admit(state, &request.loupist_id)?;
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::MemoryMutation)?;
    with_tx(state, &request.loupist_id, |tx, blobs| {
        remove(
            tx,
            blobs,
            &request.loupist_id,
            &request.import_id,
            request.archive_accepted,
            crate::commands::now_ms(),
        )?;
        list_imports(tx, &request.loupist_id)
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackImportRequest {
    loupist_id: String,
    import_id: String,
}

#[tauri::command(rename_all = "snake_case")]
pub fn pack_report(
    request: PackImportRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let state = state.inner();
    admit(state, &request.loupist_id)?;
    let index = github::index_url().ok_or_else(|| {
        CommandError::new("not_configured", "Reporting needs the community index.")
    })?;
    let row = with_tx(state, &request.loupist_id, |tx, _| {
        store::import(tx, &request.loupist_id, &request.import_id).map_err(internal)
    })?
    .ok_or_else(|| CommandError::not_found("shared Loupist"))?;
    let pack_url = row.source_url.ok_or_else(|| {
        CommandError::invalid("Only a shared Loupist from a link can be reported.")
    })?;
    let url = super::gallery::report_url(index, &pack_url).ok_or_else(|| {
        CommandError::new("not_configured", "Reporting needs the community index.")
    })?;
    crate::web::external_browser::open(&url).map_err(internal)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackOriginRequest {
    loupist_id: String,
    subject_id: String,
}

#[tauri::command(rename_all = "snake_case")]
pub fn pack_origin(
    request: PackOriginRequest,
    state: State<'_, AppState>,
) -> Result<Option<Value>, CommandError> {
    admit(state.inner(), &request.loupist_id)?;
    let origin = with_tx(state.inner(), &request.loupist_id, |tx, _| {
        store::origin_of(tx, &request.loupist_id, &request.subject_id).map_err(internal)
    })?;
    Ok(origin.map(|(import_id, name, author)| {
        serde_json::json!({"importId": import_id, "packName": name, "author": author})
    }))
}

// ---------------------------------------------------------------- updates (W19.5)

/// The community index and Gallery images, kept in app data.
fn gallery_cache(state: &AppState) -> PathBuf {
    state.artifacts.app_data_dir.join("gallery-cache")
}

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// GitHub results are reused for 10 minutes (unauthenticated reads are 60 an hour).
const CHECK_REUSE: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpdateCheck {
    UpToDate,
    /// An automatic check already ran today.
    NotChecked,
    /// From a ZIP or folder: the person chooses a newer copy.
    Local,
    OriginGone,
    Offline,
    DifferentPack,
    Rejected {
        reason: Rejection,
    },
    Available {
        update: Box<update::PackUpdate>,
    },
}

fn checks() -> &'static Mutex<BTreeMap<String, (Instant, UpdateCheck)>> {
    static CHECKS: std::sync::OnceLock<Mutex<BTreeMap<String, (Instant, UpdateCheck)>>> =
        std::sync::OnceLock::new();
    CHECKS.get_or_init(Default::default)
}

fn recent_check(import_id: &str) -> Option<UpdateCheck> {
    let mut checks = checks().lock().ok()?;
    checks.retain(|_, (at, _)| at.elapsed() < CHECK_REUSE);
    let (_, check) = checks.get(import_id)?;
    // A staged update lasts an hour; one already applied or expired is checked again.
    match check {
        UpdateCheck::Available { update } if import::staged(&update.staging_id).is_none() => None,
        other => Some(other.clone()),
    }
}

fn forget_check(import_id: &str) {
    if let Ok(mut checks) = checks().lock() {
        checks.remove(import_id);
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackCheckRequest {
    loupist_id: String,
    import_id: String,
    /// The section opened (at most once a day); false for the button.
    #[serde(default)]
    automatic: bool,
}

fn import_of(
    state: &AppState,
    loupist_id: &str,
    import_id: &str,
) -> Result<ImportRow, CommandError> {
    with_tx(state, loupist_id, |tx, _| {
        store::import(tx, loupist_id, import_id).map_err(internal)
    })?
    .ok_or_else(|| CommandError::not_found("shared Loupist"))
}

fn is_commit(reference: &str) -> bool {
    reference.len() == 40 && reference.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(crate) enum Newer {
    Fetched(Fetched),
    Settled(UpdateCheck),
}

fn check_error(error: ApiError) -> Result<Newer, CommandError> {
    match error {
        ApiError::NotFound | ApiError::Denied => Ok(Newer::Settled(UpdateCheck::OriginGone)),
        ApiError::RateLimited => Err(CommandError::new(
            "rate_limited",
            "GitHub asked to slow down.",
        )),
        ApiError::TooLarge => Ok(Newer::Settled(UpdateCheck::Rejected {
            reason: Rejection::TooLarge,
        })),
        _ => Ok(Newer::Settled(UpdateCheck::Offline)),
    }
}

/// The newest copy at the import's GitHub origin: the default-branch head (or
/// the branch its link named), unless the import already holds that commit.
pub(crate) async fn fetch_newer(
    api: &Github,
    token: Option<&str>,
    row: &ImportRow,
) -> Result<Newer, CommandError> {
    use super::deep_link::GithubLocation;
    let Some(location) = row
        .source_url
        .as_deref()
        .and_then(super::deep_link::parse_github)
    else {
        return Ok(Newer::Settled(UpdateCheck::OriginGone));
    };
    match location {
        GithubLocation::Repo {
            owner,
            repo,
            reference,
            path,
        } => {
            // A link pinned to a commit (a Gallery card's) follows the default branch.
            let reference = reference.filter(|reference| !is_commit(reference));
            let commit = match api
                .resolve_commit(token, &owner, &repo, reference.as_deref())
                .await
            {
                Err(ApiError::Denied) => {
                    api.resolve_commit(None, &owner, &repo, reference.as_deref())
                        .await
                }
                other => other,
            };
            let commit = match commit {
                Ok(commit) => commit,
                Err(error) => return check_error(error),
            };
            if row.commit.as_deref() == Some(commit.as_str()) {
                return Ok(Newer::Settled(UpdateCheck::UpToDate));
            }
            let bytes = match api.archive(&owner, &repo, &commit).await {
                Ok(bytes) => bytes,
                Err(error) => return check_error(error),
            };
            match import::read_zip(&bytes, path.as_deref()) {
                Ok(files) => Ok(Newer::Fetched(Fetched {
                    files,
                    source_url: row.source_url.clone(),
                    label: Some(format!("github.com/{owner}/{repo}")),
                    repo: Some(format!("{owner}/{repo}")),
                    path,
                    commit: Some(commit),
                })),
                // The repository no longer holds a pack where the link points.
                Err(Rejection::NotPack) => Ok(Newer::Settled(UpdateCheck::OriginGone)),
                Err(reason) => Ok(Newer::Settled(UpdateCheck::Rejected { reason })),
            }
        }
        GithubLocation::Gist { user, id } => {
            let files = match api.gist(token, &user, &id).await {
                Err(ApiError::Denied) => api.gist(None, &user, &id).await,
                other => other,
            };
            match files {
                Ok(files) => Ok(Newer::Fetched(Fetched {
                    files,
                    source_url: row.source_url.clone(),
                    label: Some(format!("gist.github.com/{user}/{id}")),
                    repo: None,
                    path: None,
                    commit: None,
                })),
                Err(error) => check_error(error),
            }
        }
    }
}

/// What the version already added held: its retained copy when it still
/// validates, else what the import recorded.
fn base_for(
    state: &AppState,
    loupist_id: &str,
    row: &ImportRow,
) -> Result<update::Base, CommandError> {
    let retained = state
        .store
        .loupist_data_dir(loupist_id)
        .ok()
        .map(|directory| {
            directory
                .join("packs")
                .join(&row.import_id)
                .join(row.version.to_string())
        })
        .filter(|directory| directory.is_dir())
        .and_then(|directory| import::read_folder(&directory).ok());
    with_tx(state, loupist_id, |tx, _| {
        let refs = store::world_refs(tx, loupist_id, &row.import_id).map_err(internal)?;
        let mut base = match retained.as_ref().and_then(update::base_from_files) {
            Some(base) => base,
            None => {
                let items = store::items(tx, loupist_id, &row.import_id).map_err(internal)?;
                update::base_from_store(&items, &refs, &row.import_id)
            }
        };
        base.map_added = update::map_added(&refs);
        Ok(base)
    })
}

/// Stages a newer copy through the same validation and scans as a first
/// import and compares it with the version added. A newer version with
/// nothing to offer here moves the import forward without asking.
pub(crate) async fn evaluate(
    state: &AppState,
    loupist_id: &str,
    row: &ImportRow,
    fetched: Fetched,
) -> Result<UpdateCheck, CommandError> {
    let commit = fetched.commit.clone();
    let blocked = match manifest_id(&fetched.files) {
        Some(id) => {
            super::gallery::blocked_reason(
                Some(&gallery_cache(state)),
                &id,
                fetched.repo.as_deref().or(row.repo.as_deref()),
            )
            .await
        }
        None => None,
    };
    let staging_id = match import::stage(fetched, blocked) {
        OpenResult::Rejected { reason, .. } => return Ok(UpdateCheck::Rejected { reason }),
        OpenResult::Preview { preview } => preview.staging_id,
    };
    let staged =
        import::staged(&staging_id).ok_or_else(|| CommandError::not_found("staged pack"))?;
    let manifest = &staged.parsed.manifest;
    if !manifest.id.eq_ignore_ascii_case(&row.pack_id) {
        import::unstage(&staging_id);
        return Ok(UpdateCheck::DifferentPack);
    }
    if let Some(reason) = blocked {
        with_tx(state, loupist_id, |tx, _| {
            store::set_blocked(tx, loupist_id, &row.import_id, Some(reason)).map_err(internal)
        })?;
    }
    if manifest.version <= row.version {
        import::unstage(&staging_id);
        if let Some(commit) = &commit {
            with_tx(state, loupist_id, |tx, _| {
                store::set_commit(tx, loupist_id, &row.import_id, commit).map_err(internal)
            })?;
        }
        return Ok(UpdateCheck::UpToDate);
    }
    let base = base_for(state, loupist_id, row)?;
    let diff = with_tx(state, loupist_id, |tx, _| {
        update::diff(tx, loupist_id, row, &base, &staged, &staging_id)
    })?;
    let nothing = diff.new_items.is_empty()
        && diff.changed.is_empty()
        && diff.removed.is_empty()
        && diff.map.is_none();
    if nothing && blocked.is_none() {
        let _activity = state
            .maintenance
            .admit_kind(MaintenanceActivityKind::MemoryMutation)?;
        with_tx(state, loupist_id, |tx, blobs| {
            update::apply(
                tx,
                blobs,
                loupist_id,
                row,
                &base,
                &staged,
                &BTreeSet::new(),
                false,
                crate::commands::now_ms(),
            )
        })?;
        retain_copy(
            state,
            loupist_id,
            &row.import_id,
            manifest.version,
            &staged.fetched.files,
        );
        import::unstage(&staging_id);
        return Ok(UpdateCheck::UpToDate);
    }
    Ok(UpdateCheck::Available {
        update: Box::new(diff),
    })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn pack_check_update(
    request: PackCheckRequest,
    state: State<'_, AppState>,
) -> Result<UpdateCheck, CommandError> {
    let state = state.inner();
    admit(state, &request.loupist_id)?;
    let row = import_of(state, &request.loupist_id, &request.import_id)?;
    if row.source_url.is_none() {
        return Ok(UpdateCheck::Local);
    }
    let now = crate::commands::now_ms();
    if request.automatic && row.checked_at_ms.is_some_and(|at| now - at < DAY_MS) {
        return Ok(UpdateCheck::NotChecked);
    }
    if let Some(check) = recent_check(&row.import_id) {
        return Ok(check);
    }
    with_tx(state, &request.loupist_id, |tx, _| {
        store::set_checked(tx, &request.loupist_id, &row.import_id, now).map_err(internal)
    })?;
    let token = github::load_token(&state.credential_environment);
    let check = match fetch_newer(&Github::production(), token.as_deref(), &row).await? {
        Newer::Fetched(fetched) => evaluate(state, &request.loupist_id, &row, fetched).await?,
        Newer::Settled(check) => check,
    };
    if let Ok(mut checks) = checks().lock() {
        checks.insert(row.import_id.clone(), (Instant::now(), check.clone()));
    }
    Ok(check)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackChooseNewerRequest {
    loupist_id: String,
    import_id: String,
    source: PackSource,
}

/// "Choose a newer copy" for a pack added from a ZIP or folder (same pack id required).
#[tauri::command(rename_all = "snake_case")]
pub async fn pack_choose_newer(
    request: PackChooseNewerRequest,
    state: State<'_, AppState>,
) -> Result<Option<UpdateCheck>, CommandError> {
    let state = state.inner();
    admit(state, &request.loupist_id)?;
    let row = import_of(state, &request.loupist_id, &request.import_id)?;
    let kind = match request.source {
        PackSource::Zip { .. } => Local::Zip,
        PackSource::Folder { .. } => Local::Folder,
        _ => return Err(CommandError::invalid("Choose a ZIP or a folder.")),
    };
    let fetched = match fetch_local(kind).await {
        Ok(None) => return Ok(None),
        Ok(Some(fetched)) => fetched,
        Err(Ok(OpenResult::Rejected { reason, .. })) => {
            return Ok(Some(UpdateCheck::Rejected { reason }))
        }
        Err(Ok(OpenResult::Preview { .. })) => {
            return Err(CommandError::internal("unexpected preview"))
        }
        Err(Err(error)) => return Err(error),
    };
    with_tx(state, &request.loupist_id, |tx, _| {
        store::set_checked(
            tx,
            &request.loupist_id,
            &row.import_id,
            crate::commands::now_ms(),
        )
        .map_err(internal)
    })?;
    evaluate(state, &request.loupist_id, &row, fetched)
        .await
        .map(Some)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackApplyUpdateRequest {
    loupist_id: String,
    import_id: String,
    staging_id: String,
    item_keys: Vec<String>,
    apply_map: bool,
}

/// Applies the chosen part of an update; the rest of that version is declined.
#[tauri::command(rename_all = "snake_case")]
pub async fn pack_apply_update(
    request: PackApplyUpdateRequest,
    state: State<'_, AppState>,
) -> Result<Vec<PackImportView>, CommandError> {
    apply_update(state.inner(), request).await
}

pub(crate) async fn apply_update(
    state: &AppState,
    request: PackApplyUpdateRequest,
) -> Result<Vec<PackImportView>, CommandError> {
    admit(state, &request.loupist_id)?;
    if request.item_keys.len() > 1_000 {
        return Err(CommandError::invalid("Choose at most 1,000 items."));
    }
    let staged = import::staged(&request.staging_id)
        .ok_or_else(|| CommandError::new("staging_expired", "Check for updates again."))?;
    if staged.blocked.is_some() {
        return Err(CommandError::new(
            "safety_blocked",
            "This shared Loupist was removed from the Gallery.",
        ));
    }
    let row = import_of(state, &request.loupist_id, &request.import_id)?;
    let manifest = &staged.parsed.manifest;
    if !manifest.id.eq_ignore_ascii_case(&row.pack_id) || manifest.version <= row.version {
        return Err(CommandError::new(
            "update_changed",
            "This update changed. Check for updates again.",
        ));
    }
    let base = base_for(state, &request.loupist_id, &row)?;
    if request.apply_map
        && with_tx(state, &request.loupist_id, |tx, _| {
            update::touches_map(tx, &request.loupist_id, &row, &base, &staged)
        })?
    {
        wait_for_world(state, &request.loupist_id).await?;
    }
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::MemoryMutation)?;
    let keys: BTreeSet<String> = request.item_keys.into_iter().collect();
    with_tx(state, &request.loupist_id, |tx, blobs| {
        update::apply(
            tx,
            blobs,
            &request.loupist_id,
            &row,
            &base,
            &staged,
            &keys,
            request.apply_map,
            crate::commands::now_ms(),
        )
    })?;
    retain_copy(
        state,
        &request.loupist_id,
        &row.import_id,
        manifest.version,
        &staged.fetched.files,
    );
    import::unstage(&request.staging_id);
    forget_check(&row.import_id);
    with_tx(state, &request.loupist_id, |tx, _| {
        list_imports(tx, &request.loupist_id)
    })
}

// ---------------------------------------------------------------- Gallery (W19.6)

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GalleryIndexRequest {
    #[serde(default)]
    refresh: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryView {
    /// `ready`, `offline` (the kept copy), `unavailable` (none kept) or `notConfigured`.
    status: &'static str,
    fetched_at_ms: Option<i64>,
    entries: Vec<super::gallery::Entry>,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn gallery_index(
    request: GalleryIndexRequest,
    state: State<'_, AppState>,
) -> Result<GalleryView, CommandError> {
    if github::index_url().is_none() {
        return Ok(GalleryView {
            status: "notConfigured",
            fetched_at_ms: None,
            entries: Vec::new(),
        });
    }
    Ok(
        match super::gallery::load(Some(&gallery_cache(state.inner())), request.refresh).await {
            Some(loaded) => GalleryView {
                status: if loaded.offline { "offline" } else { "ready" },
                fetched_at_ms: Some(loaded.fetched_at_ms),
                entries: loaded.index.entries.clone(),
            },
            None => GalleryView {
                status: "unavailable",
                fetched_at_ms: None,
                entries: Vec::new(),
            },
        },
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GalleryImageRequest {
    entry_id: String,
}

/// A card's Map image as a `data:image/png` URL, or null.
#[tauri::command(rename_all = "snake_case")]
pub async fn gallery_image(
    request: GalleryImageRequest,
    state: State<'_, AppState>,
) -> Result<Option<String>, CommandError> {
    use base64::Engine;
    let Some(entry) = super::gallery::cached_entry(&request.entry_id) else {
        return Ok(None);
    };
    let bytes = super::gallery::preview_from(
        super::gallery::RAW_BASE,
        &gallery_cache(state.inner()),
        &entry,
    )
    .await;
    Ok(bytes.map(|bytes| {
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    }))
}

/// Whether the index lists this Loupist's repository at its last published
/// commit (`listed`), at an earlier one (`outdated`), or not at all (null).
#[tauri::command(rename_all = "snake_case")]
pub async fn gallery_listing_status(
    request: LoupistRequest,
    state: State<'_, AppState>,
) -> Result<Option<&'static str>, CommandError> {
    let state = state.inner();
    admit(state, &request.loupist_id)?;
    let identity = with_tx(state, &request.loupist_id, |tx, _| {
        store::identity(tx, &request.loupist_id).map_err(internal)
    })?;
    let Some(repo) = identity.as_ref().and_then(|identity| identity.repo.clone()) else {
        return Ok(None);
    };
    let Some(loaded) = super::gallery::load(Some(&gallery_cache(state)), false).await else {
        return Ok(None);
    };
    let listed = loaded
        .index
        .entries
        .iter()
        .find(|entry| entry.full_name().eq_ignore_ascii_case(&repo));
    Ok(listed.map(|entry| {
        if identity
            .as_ref()
            .and_then(|identity| identity.repo_commit.as_deref())
            .is_some_and(|commit| commit.eq_ignore_ascii_case(&entry.commit))
        {
            "listed"
        } else {
            "outdated"
        }
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GalleryListingRequest {
    loupist_id: String,
    #[serde(default)]
    accepted_terms: bool,
    #[serde(default)]
    terms_url: Option<String>,
}

pub(crate) fn listing_terms<'a>(
    configured: Option<&'a str>,
    accepted: bool,
    supplied: Option<&str>,
) -> Result<&'a str, CommandError> {
    let terms = configured
        .filter(|url| pack::valid_terms_url(url))
        .ok_or_else(|| {
            CommandError::new(
                "not_configured",
                "Gallery listing terms are not configured.",
            )
        })?;
    if !accepted || supplied != Some(terms) {
        return Err(CommandError::new(
            "gallery_terms_required",
            "Accept the current Gallery listing terms before requesting a listing.",
        ));
    }
    Ok(terms)
}

pub(crate) fn listing_body(terms: &str, accepted_at: &str) -> String {
    format!("Requested from Loupist. The index's automatic checks validate the pack; nobody reviews it by hand.\n\nGallery listing terms accepted: {terms}\nAcceptance date (UTC): {accepted_at}\n")
}

/// "List it in the Gallery": Rust forks the index repository, adds
/// `entries/<owner>__<repo>.json` (`{repo, path, commit}` only) on a branch of
/// that fork and opens a pull request. The index's checks decide; nobody
/// reviews it by hand. Rust writes only to its own fork.
#[tauri::command(rename_all = "snake_case")]
pub async fn gallery_listing_request(
    request: GalleryListingRequest,
    state: State<'_, AppState>,
) -> Result<Value, CommandError> {
    let state = state.inner();
    admit(state, &request.loupist_id)?;
    let terms = listing_terms(
        github::gallery_terms_url(),
        request.accepted_terms,
        request.terms_url.as_deref(),
    )?;
    let accepted_at = now_rfc3339();
    let not_configured = || {
        CommandError::new(
            "not_configured",
            "The Gallery isn't available in this version.",
        )
    };
    if github::client_id().is_none() {
        return Err(not_configured());
    }
    let (index_owner, index_repo) = github::index_url()
        .and_then(super::gallery::index_repo)
        .ok_or_else(not_configured)?;
    let environment = state.credential_environment.to_string();
    let token = github::load_token(&environment)
        .ok_or_else(|| CommandError::new("signed_out", "Sign in to GitHub first."))?;
    let login = state
        .store
        .lock()
        .ok()
        .and_then(|connection| github::stored_login(&connection).ok().flatten())
        .ok_or_else(|| CommandError::new("signed_out", "Sign in to GitHub first."))?;
    let identity = with_tx(state, &request.loupist_id, |tx, _| {
        store::identity(tx, &request.loupist_id).map_err(internal)
    })?;
    let (Some(repo), Some(commit)) = (
        identity.as_ref().and_then(|identity| identity.repo.clone()),
        identity
            .as_ref()
            .and_then(|identity| identity.repo_commit.clone()),
    ) else {
        return Err(CommandError::invalid("Publish to GitHub first."));
    };
    let (path, bytes) = super::gallery::listing_entry(&repo, "", &commit)
        .ok_or_else(|| CommandError::invalid("Publish to GitHub first."))?;
    let failed = |error: ApiError| match error {
        ApiError::Offline | ApiError::RateLimited | ApiError::Denied => {
            api_error(error, &environment, state)
        }
        _ => CommandError::new("list_failed", "The Gallery request didn't go through."),
    };
    let api = Github::production();
    let manifest = api
        .pack_manifest(&token, &repo, &commit)
        .await
        .map_err(failed)?;
    require_shareable(manifest.license)?;
    let identity = identity.ok_or_else(|| CommandError::invalid("Publish to GitHub first."))?;
    if manifest.id != identity.pack_id {
        return Err(CommandError::new(
            "repo_mismatch",
            "The published pack identity changed.",
        ));
    }
    with_tx(state, &request.loupist_id, |tx, _| {
        store::record_listing_acceptance(
            tx,
            &request.loupist_id,
            &identity.pack_id,
            &commit,
            terms,
            &accepted_at,
        )
        .map_err(internal)
    })?;
    let upstream = api
        .repo(Some(&token), &format!("{index_owner}/{index_repo}"))
        .await
        .map_err(failed)?;
    let fork = api
        .fork(&token, &upstream.full_name)
        .await
        .map_err(failed)?;
    if !fork
        .full_name
        .split('/')
        .next()
        .is_some_and(|owner| owner.eq_ignore_ascii_case(&login))
    {
        return Err(CommandError::new(
            "list_failed",
            "The Gallery request didn't go through.",
        ));
    }
    let branch = format!("loupist-{}", repo.replace('/', "__"));
    api.commit_file(
        &token,
        &fork.full_name,
        &fork.default_branch,
        &branch,
        &path,
        &bytes,
        &format!("List {repo} in the Gallery"),
    )
    .await
    .map_err(failed)?;
    let url = api
        .open_pull(
            &token,
            &upstream.full_name,
            &format!("{login}:{branch}"),
            &upstream.default_branch,
            &format!("List {repo}"),
            &listing_body(terms, &accepted_at),
        )
        .await
        .map_err(failed)?;
    Ok(serde_json::json!({ "url": url }))
}

// ---------------------------------------------------------------- deep links

#[tauri::command(rename_all = "snake_case")]
pub fn deep_link_pending() -> Option<super::deep_link::DeepLinkEvent> {
    super::deep_link::held().map(|location| super::deep_link::DeepLinkEvent::Ok {
        source: location.label(),
    })
}

#[tauri::command(rename_all = "snake_case")]
pub fn deep_link_dismiss() {
    super::deep_link::dismiss();
}

/// Validates each arriving `loupist://` URL in Rust and tells the renderer.
#[cfg(all(any(target_os = "macos", windows), not(feature = "e2e")))]
pub(crate) fn deliver_links(app: &tauri::AppHandle, urls: impl IntoIterator<Item = String>) {
    use tauri::Emitter;
    for url in urls {
        let event = super::deep_link::receive(&url);
        let _ = app.emit("deep-link-import", event);
    }
}
