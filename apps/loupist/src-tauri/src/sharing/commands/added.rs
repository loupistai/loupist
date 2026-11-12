//! Packs already added: the list, Gallery block marks, Remove, Report and origin.
use rusqlite::Transaction;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use super::{admit, gallery_cache, internal, knowledge_error, with_tx, LoupistRequest};
use crate::app::{AppState, CommandError};
use crate::knowledge::{shared_pack, BlobStore};
use crate::runs::maintenance::MaintenanceActivityKind;
use crate::sharing::gallery;
use crate::sharing::github;
use crate::sharing::pack::License;
use crate::sharing::store::{self, ImportMode};

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Removable {
    names: u32,
    links: u32,
    accepted: u32,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
    let Some(blocked) = gallery::blocked_list(Some(&gallery_cache(state))).await else {
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
    blocked: &[gallery::Blocked],
) -> Result<(), CommandError> {
    for row in store::imports(tx, loupist_id).map_err(internal)? {
        let reason = gallery::find(blocked, &row.pack_id, row.repo.as_deref()).map(gallery::reason);
        if reason != row.blocked_reason.as_deref() {
            store::set_blocked(tx, loupist_id, &row.import_id, reason).map_err(internal)?;
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
    let imports = with_tx(state, &request.loupist_id, |tx, blobs| {
        remove(
            tx,
            blobs,
            &request.loupist_id,
            &request.import_id,
            request.archive_accepted,
            crate::commands::now_ms(),
        )?;
        list_imports(tx, &request.loupist_id)
    })?;
    super::add::prune_copies(state, &request.loupist_id, &request.import_id);
    Ok(imports)
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
    let url = gallery::report_url(index, &pack_url).ok_or_else(|| {
        CommandError::new("not_configured", "Reporting needs the community index.")
    })?;
    crate::web::external_browser::open(&url).map_err(internal)
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
