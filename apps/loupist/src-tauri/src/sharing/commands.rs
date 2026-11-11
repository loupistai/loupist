//! Tauri commands for sharing. Each takes typed intent, rechecks authority,
//! and keeps credentials, paths and network calls in Rust. Each submodule owns
//! one part of sharing; this module holds what they share.
use std::path::PathBuf;

use rusqlite::Transaction;
use serde::Deserialize;

use crate::app::{AppState, CommandError};
use crate::knowledge::{BlobStore, KnowledgeError};

pub(crate) mod add;
pub(crate) mod added;
pub(crate) mod deep_links;
pub(crate) mod gallery;
pub(crate) mod github_account;
pub(crate) mod open;
pub(crate) mod publish;
pub(crate) mod review;
pub(crate) mod save;
pub(crate) mod share;
pub(crate) mod updates;

/// What the sharing commands remember between calls. [`AppState`] holds it,
/// so each app session (and each test's state) has its own.
#[derive(Clone, Default)]
pub(crate) struct SharingSession {
    saved: save::SavedCopies,
    sign_in: github_account::SignInSession,
    checks: updates::RecentChecks,
}

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

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "SharingLoupistRequest"))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoupistRequest {
    loupist_id: String,
}

/// The community index and Gallery images, kept in app data.
fn gallery_cache(state: &AppState) -> PathBuf {
    state.artifacts.app_data_dir.join("gallery-cache")
}
