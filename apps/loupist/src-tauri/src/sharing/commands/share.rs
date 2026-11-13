//! Share summary, the pack build shared by every destination, and Copy as text.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use super::{admit, internal, knowledge_error, now_rfc3339, with_tx};
use crate::app::{AppState, CommandError};
use crate::knowledge::shared_pack;
use crate::sharing::export::{
    self, Choice, ExportInput, Finding, PackMeta, ProfileInfo, Scope, Sections,
};
use crate::sharing::github;
use crate::sharing::pack::{self, Files, License};
use crate::sharing::scan::{HeldPosition, PrivateContext};
use crate::sharing::store::{self, Identity, ImportRow};

#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShareSummaryRequest {
    loupist_id: String,
    scope: ScopeRequest,
    #[serde(default)]
    sections: Option<Sections>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    excluded_item_keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShareRequest {
    pub(super) loupist_id: String,
    pub(super) scope: ScopeRequest,
    pub(super) sections: Sections,
    pub(super) excluded_item_keys: Vec<String>,
    pub(super) acknowledged_finding_ids: Vec<String>,
    #[serde(default)]
    pub(super) license: Option<License>,
    #[serde(default)]
    pub(super) byline: Option<String>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RepoView {
    full_name: String,
    url: String,
    version: u32,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ItemTitle {
    kind: &'static str,
    title: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ShareLicense {
    id: License,
    url: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
pub(crate) fn private_context(state: &AppState) -> Result<PrivateContext, CommandError> {
    private_context_with_finance(state, crate::finance_credentials::get)
}

pub(crate) fn private_context_with_finance(
    state: &AppState,
    get: impl Fn(
        &str,
        &str,
    ) -> Result<
        Option<crate::finance_credentials::ActiveCredentialBundle>,
        crate::finance_credentials::FinanceCredentialError,
    >,
) -> Result<PrivateContext, CommandError> {
    private_context_with_credentials(
        state,
        get,
        || crate::custom_connections::service::protected_values_for_state(state),
        github::try_load_token(&state.credential_environment),
    )
}

fn credential_scan_unavailable(_: impl std::fmt::Debug) -> CommandError {
    CommandError::new(
        "credential_store_unavailable",
        "The scan could not check your saved credentials. Unlock the Keychain or reconnect and try again.",
    )
}

fn private_context_with_credentials(
    state: &AppState,
    get: impl Fn(
        &str,
        &str,
    ) -> Result<
        Option<crate::finance_credentials::ActiveCredentialBundle>,
        crate::finance_credentials::FinanceCredentialError,
    >,
    connections: impl FnOnce() -> Result<Vec<String>, crate::custom_connections::ConnectionError>,
    github_token: Result<Option<String>, github::TokenStoreError>,
) -> Result<PrivateContext, CommandError> {
    let mut context = PrivateContext::default();
    let environment = state.credential_environment.to_string();
    if let Some(token) = github_token.map_err(credential_scan_unavailable)? {
        context.secrets.push(token);
    }
    let directory = state.pi_agent_data_dir()?;
    context.secrets.extend(
        crate::providers::catalog::saved_credential_values(&directory)
            .map_err(credential_scan_unavailable)?,
    );
    context
        .secrets
        .extend(connections().map_err(credential_scan_unavailable)?);
    let catalog = crate::marketplace::bundled_catalog().map_err(internal)?;
    for entry in catalog
        .entries
        .iter()
        .filter(|entry| !entry.runtime.credential_mapping.is_empty())
    {
        if let Some(bundle) = get(&environment, &entry.id).map_err(credential_scan_unavailable)? {
            for field in entry.runtime.credential_mapping.keys() {
                if let Some(value) = bundle.get(field).filter(|value| !value.is_empty()) {
                    context.secrets.push(value.to_owned());
                }
            }
            if let Some(account) = bundle.get("account_number") {
                context.accounts.push(account.to_owned());
            }
        }
    }
    let connection = state.store.lock().map_err(internal)?;
    let mut positions = connection
        .prepare(&format!(
            "SELECT p.symbol, p.display_name, p.quantity, p.cost_basis, p.market_value
         FROM user_holdings_positions p JOIN ({}) s ON s.snapshot_id = p.snapshot_id",
            crate::store::user_context::LATEST_SNAPSHOTS
        ))
        .map_err(internal)?;
    let rows = positions
        .query_map([], |row| {
            Ok(HeldPosition {
                symbol: row.get(0)?,
                names: row.get::<_, Option<String>>(1)?.into_iter().collect(),
                amounts: [row.get::<_, Option<String>>(2)?, row.get(3)?, row.get(4)?]
                    .into_iter()
                    .flatten()
                    .collect(),
            })
        })
        .map_err(internal)?;
    context.holdings = rows.collect::<rusqlite::Result<_>>().map_err(internal)?;
    // Wallet addresses are as private as account numbers.
    let mut addresses = connection
        .prepare("SELECT address FROM user_wallet_addresses")
        .map_err(internal)?;
    context.accounts.extend(
        addresses
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(internal)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(internal)?,
    );
    context.personal = personal_names();
    Ok(context)
}

/// The OS account's full name and home path. They belong to the process's
/// user, not to an app session, so one read serves the whole process.
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

#[cfg(test)]
mod credential_scan_tests {
    use super::*;
    use crate::custom_connections::ConnectionError;
    use crate::fs::artifact_trust::ensure_private_regular_file;
    use crate::sharing::scan::{scan_private, PrivateClass};

    fn assert_recoverable(result: Result<PrivateContext, CommandError>) {
        let error = result.unwrap_err();
        assert_eq!(error.code, "credential_store_unavailable");
        assert_eq!(error.message, "The scan could not check your saved credentials. Unlock the Keychain or reconnect and try again.");
    }

    #[test]
    fn credential_load_and_connection_file_failures_block_the_share_context() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().to_path_buf());
        assert_recoverable(private_context_with_credentials(
            &state,
            |_, _| Ok(None),
            || Err(ConnectionError::CredentialStore),
            Ok(None),
        ));
        let directory = state.artifacts.app_data_dir.join("custom-connections");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("connections.json");
        ensure_private_regular_file(&path).unwrap();
        std::fs::write(&path, "invalid connection file").unwrap();
        assert_recoverable(private_context_with_finance(&state, |_, _| Ok(None)));
    }

    #[test]
    fn github_keychain_failure_blocks_the_scan_but_absent_and_saved_tokens_are_distinct() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().to_path_buf());
        assert_recoverable(private_context_with_credentials(
            &state,
            |_, _| Ok(None),
            || Ok(Vec::new()),
            Err(github::TokenStoreError),
        ));
        let absent =
            private_context_with_credentials(&state, |_, _| Ok(None), || Ok(Vec::new()), Ok(None))
                .unwrap();
        assert!(absent.secrets.is_empty());
        let saved = private_context_with_credentials(
            &state,
            |_, _| Ok(None),
            || Ok(Vec::new()),
            Ok(Some("pine".into())),
        )
        .unwrap();
        assert!(scan_private("pine입니다", &saved)
            .iter()
            .any(|hit| hit.class == PrivateClass::Credential));
    }

    #[test]
    fn saved_finance_credentials_of_four_to_seven_characters_reach_the_scan() {
        use crate::finance_credentials::{self, VerificationOutcome};
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().to_path_buf());
        let catalog = crate::marketplace::bundled_catalog().unwrap();
        let entry = catalog
            .entries
            .iter()
            .find(|entry| !entry.runtime.credential_mapping.is_empty())
            .unwrap();
        let field = entry.runtime.credential_mapping.keys().next().unwrap();
        let environment = &state.credential_environment;
        for key in ["pine", "oak12", "cedar12", "abc"] {
            finance_credentials::stage(
                environment,
                &entry.id,
                &std::collections::BTreeMap::from([(field.clone(), key.into())]),
            )
            .unwrap();
            let candidate = finance_credentials::candidate_for_verification(environment, &entry.id)
                .unwrap()
                .unwrap();
            finance_credentials::finish_verification(
                environment,
                &entry.id,
                candidate.revision(),
                VerificationOutcome::Verified,
                1,
                None,
            )
            .unwrap();
            let context = private_context(&state).unwrap();
            assert!(context.secrets.contains(&key.to_owned()));
            let flagged = scan_private(&format!("{key}입니다"), &context)
                .iter()
                .any(|hit| hit.class == PrivateClass::Credential);
            assert_eq!(flagged, key.len() >= 4, "{key}");
            assert!(scan_private(&format!("x{key}y"), &context).is_empty());
        }
    }

    #[test]
    fn provider_read_failure_blocks_the_share_context_and_short_keys_are_scanned() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().to_path_buf());
        let path = state.pi_agent_data_dir().unwrap().join("auth.json");
        ensure_private_regular_file(&path).unwrap();
        std::fs::write(&path, "invalid auth file").unwrap();
        assert_recoverable(private_context_with_finance(&state, |_, _| Ok(None)));
        std::fs::write(&path, serde_json::json!({
            "custom-fixture": {"type":"api_key", "key":"pine", "port":"3000", "locale":"en-US", "mode":"fast", "enabled":"true"}
        }).to_string()).unwrap();
        let context = private_context_with_finance(&state, |_, _| Ok(None)).unwrap();
        assert_eq!(context.secrets, ["pine"]);
        for text in ["pine", "key=pine", "(pine)", "pine-tree"] {
            assert!(
                scan_private(text, &context)
                    .iter()
                    .any(|hit| hit.class == PrivateClass::Credential),
                "{text}"
            );
        }
        for text in [
            "pineapple",
            "spine",
            "alpine",
            "3000",
            "en-US",
            "fast",
            "true",
            "api_key",
        ] {
            assert!(scan_private(text, &context).is_empty(), "{text}");
        }
    }
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
    let findings = export::findings(&prepared, &private_context(state)?);
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
        notice: crate::sharing::readme_notice(&selected.profile.language).to_owned(),
        open_url,
        app: env!("CARGO_PKG_VERSION").to_owned(),
        origin,
    };
    let (mut manifest, mut files) = export::build(&selected, &choice, &meta);
    let hash = pack::content_hash(&files);
    if item_scope {
        meta.version = 1;
    } else if identity.content_hash.as_deref() != Some(hash.as_str()) {
        if identity.content_hash.is_some() {
            meta.version = identity.version + 1;
        }
        meta.created_at = now_rfc3339();
    }
    manifest.version = meta.version;
    manifest.created_at = meta.created_at.clone();
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest).expect("manifest serializes");
    manifest_bytes.push(b'\n');
    files.insert(pack::MANIFEST.into(), manifest_bytes);
    pack::validate(&files).map_err(|reason| {
        let items = export::oversized_items(&selected);
        let detail = if items.is_empty() { manifest.name.clone() } else {
            let mut detail = items.iter().take(40).cloned().collect::<Vec<_>>().join(", ");
            if items.len() > 40 { detail.push_str(&format!(" and {} more items", items.len() - 40)); }
            detail
        };
        CommandError::invalid(format!("The selected content cannot form a shared pack ({reason:?}): {detail}. Leave out these items and try again."))
    })?;
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

fn tone_name(tone: crate::domain::LoupistTone) -> String {
    serde_json::to_value(tone)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "balanced".into())
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
    let findings = export::findings(&prepared, &private_context(state)?);
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
                .methods
                .iter()
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
    let mut default_license = allowed_licenses.first().copied().unwrap_or_default();
    let mut meta = PackMeta {
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
    // The existing identity stores the license inside its content hash. Recover
    // it from the allowed choices without adding a stored field or schema step.
    if let Some(hash) = &identity.content_hash {
        for license in &allowed_licenses {
            meta.license = *license;
            if *hash == export::content_hash_for(&selected, &choice, &meta) {
                default_license = *license;
                break;
            }
        }
        meta.license = default_license;
    }
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
