//! Update checks for added packs: fetching a newer copy, comparing it with
//! the version added, and applying the chosen part.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::State;

use super::add::{retain_copy, wait_for_world};
use super::added::{list_imports, PackImportView};
use super::open::{fetch_local, manifest_id, Local, PackSource};
use super::{admit, gallery_cache, internal, with_tx};
use crate::app::{AppState, CommandError};
use crate::runs::maintenance::MaintenanceActivityKind;
use crate::sharing::deep_link::{self, GithubLocation};
use crate::sharing::gallery;
use crate::sharing::github::{self, ApiError, Github};
use crate::sharing::import::{self, Fetched, OpenResult};
use crate::sharing::pack::Rejection;
use crate::sharing::store::{self, ImportRow};
use crate::sharing::update;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// GitHub results are reused for 10 minutes (unauthenticated reads are 60 an hour).
const CHECK_REUSE: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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

/// "Check for updates" results this app session reuses for [`CHECK_REUSE`].
#[derive(Clone, Default)]
pub(crate) struct RecentChecks(Arc<Mutex<BTreeMap<String, (Instant, UpdateCheck)>>>);

impl RecentChecks {
    fn recent(&self, import_id: &str) -> Option<UpdateCheck> {
        let mut checks = self.0.lock().ok()?;
        checks.retain(|_, (at, _)| at.elapsed() < CHECK_REUSE);
        let (_, check) = checks.get(import_id)?;
        // A staged update lasts an hour; one already applied or expired is checked again.
        match check {
            UpdateCheck::Available { update } if import::staged(&update.staging_id).is_none() => {
                None
            }
            other => Some(other.clone()),
        }
    }

    fn remember(&self, import_id: &str, check: &UpdateCheck) {
        if matches!(check, UpdateCheck::Offline) {
            self.forget(import_id);
            return;
        }
        if let Ok(mut checks) = self.0.lock() {
            checks.insert(import_id.to_owned(), (Instant::now(), check.clone()));
        }
    }

    fn forget(&self, import_id: &str) {
        if let Ok(mut checks) = self.0.lock() {
            checks.remove(import_id);
        }
    }
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackCheckRequest {
    loupist_id: String,
    import_id: String,
    /// The section opened (at most once a day); false for the button.
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
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
    let Some(location) = row.source_url.as_deref().and_then(deep_link::parse_github) else {
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
            gallery::blocked_reason(
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
    check_update(state.inner(), request, &Github::production()).await
}

pub(crate) async fn check_update(
    state: &AppState,
    request: PackCheckRequest,
    api: &Github,
) -> Result<UpdateCheck, CommandError> {
    admit(state, &request.loupist_id)?;
    let row = import_of(state, &request.loupist_id, &request.import_id)?;
    if row.source_url.is_none() {
        return Ok(UpdateCheck::Local);
    }
    let now = crate::commands::now_ms();
    if request.automatic && row.checked_at_ms.is_some_and(|at| now - at < DAY_MS) {
        return Ok(UpdateCheck::NotChecked);
    }
    if let Some(check) = state.sharing.checks.recent(&row.import_id) {
        return Ok(check);
    }
    let token = github::load_token(&state.credential_environment);
    let newer = match fetch_newer(api, token.as_deref(), &row).await {
        Err(error) if error.code == "rate_limited" => {
            with_tx(state, &request.loupist_id, |tx, _| {
                store::set_checked(tx, &request.loupist_id, &row.import_id, now).map_err(internal)
            })?;
            return Err(error);
        }
        result => result?,
    };
    let check = match newer {
        Newer::Fetched(fetched) => evaluate(state, &request.loupist_id, &row, fetched).await?,
        Newer::Settled(check) => check,
    };
    if !matches!(check, UpdateCheck::Offline) {
        with_tx(state, &request.loupist_id, |tx, _| {
            store::set_checked(tx, &request.loupist_id, &row.import_id, now).map_err(internal)
        })?;
    }
    state.sharing.checks.remember(&row.import_id, &check);
    Ok(check)
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
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
    state.sharing.checks.forget(&row.import_id);
    with_tx(state, &request.loupist_id, |tx, _| {
        list_imports(tx, &request.loupist_id)
    })
}
