//! The community Gallery: its index and card images, and listing a published Loupist.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use super::github_account::api_error;
use super::share::require_shareable;
use super::{admit, gallery_cache, internal, now_rfc3339, with_tx, LoupistRequest};
use crate::app::{AppState, CommandError};
use crate::sharing::gallery;
use crate::sharing::github::{self, ApiError, Github};
use crate::sharing::pack;
use crate::sharing::store;

#[derive(Debug, Default, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GalleryIndexRequest {
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    refresh: bool,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct GalleryView {
    /// `ready`, `offline` (the kept copy), `unavailable` (none kept) or `notConfigured`.
    status: &'static str,
    fetched_at_ms: Option<i64>,
    entries: Vec<gallery::Entry>,
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
        match gallery::load(Some(&gallery_cache(state.inner())), request.refresh).await {
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
#[cfg_attr(test, derive(ts_rs::TS))]
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
    let Some(entry) = gallery::cached_entry(&request.entry_id) else {
        return Ok(None);
    };
    let bytes =
        gallery::preview_from(gallery::RAW_BASE, &gallery_cache(state.inner()), &entry).await;
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
    let Some(loaded) = gallery::load(Some(&gallery_cache(state)), false).await else {
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
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GalleryListingRequest {
    loupist_id: String,
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
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
        .and_then(gallery::index_repo)
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
    let (path, bytes) = gallery::listing_entry(&repo, "", &commit)
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
