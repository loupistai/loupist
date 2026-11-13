//! Adding a staged pack as a new Loupist or into an existing one.
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use rusqlite::Transaction;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{ipc::Channel, State};

use super::{admit, internal, knowledge_error, with_tx};
use crate::app::{AppState, CommandError};
use crate::knowledge::{shared_pack, BlobStore};
use crate::runs::maintenance::MaintenanceActivityKind;
use crate::sharing::import;
use crate::sharing::pack::{self, Files};
use crate::sharing::store::{self, ImportMode, ImportRow};

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct AddSections {
    pub(crate) perspective: bool,
    pub(crate) methods: bool,
    pub(crate) principles: bool,
    pub(crate) map: bool,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackAddRequest {
    staging_id: String,
    mode: AddMode,
    sections: AddSections,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AddProgress {
    done: u32,
    total: u32,
}

pub(super) async fn wait_for_world(state: &AppState, loupist_id: &str) -> Result<(), CommandError> {
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
    folder: &std::path::Path,
) -> Result<String, CommandError> {
    use super::review::{decide, Decision};
    use crate::sharing::import::{Fetched, OpenResult};
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

/// Copies are a derived update baseline. Only the active import version owns one;
/// accepted records retain their text and rights in SQLite after pack removal.
/// Hold the database lock through pruning so concurrent retain/update/remove calls
/// cannot delete a version whose reference has just changed.
pub(crate) fn prune_copies(state: &AppState, loupist_id: &str, import_id: &str) {
    if !state.store.is_folder_catalog()
        || !import_id.starts_with("imp_")
        || !import_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return;
    }
    // Resolve catalog metadata before taking the folder database lock.
    let Ok(directory) = state.store.loupist_data_dir(loupist_id) else {
        return;
    };
    let Ok(mut connection) = state.store.lock_loupist(loupist_id) else {
        return;
    };
    let Ok(tx) = connection.transaction() else {
        return;
    };
    let Ok(row) = store::import(&tx, loupist_id, import_id) else {
        return;
    };
    let Ok(root) = crate::fs::secure_delete::SecureDeletionRoot::open(&directory) else {
        return;
    };
    let relative = std::path::Path::new("packs").join(import_id);
    match row {
        None => {
            let _ = root.remove_tree(&relative);
        }
        Some(row) => {
            let Ok(entries) = std::fs::read_dir(directory.join(&relative)) else {
                return;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(version) = name.to_str().and_then(|name| name.parse::<u32>().ok()) else {
                    continue;
                };
                if version != row.version {
                    let _ = root.remove_tree(&relative.join(name));
                }
            }
        }
    }
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
    // Resolve catalog metadata before taking the folder database lock.
    let Ok(directory) = state.store.loupist_data_dir(loupist_id) else {
        return;
    };
    let Ok(mut connection) = state.store.lock_loupist(loupist_id) else {
        return;
    };
    let Ok(tx) = connection.transaction() else {
        return;
    };
    let Ok(Some(row)) = store::import(&tx, loupist_id, import_id) else {
        return;
    };
    if row.version != version {
        return;
    }
    let Ok(root) = crate::fs::secure_delete::SecureDeletionRoot::open(&directory) else {
        return;
    };
    let target = std::path::Path::new("packs")
        .join(import_id)
        .join(version.to_string());
    for (path, bytes) in files {
        let destination = target.join(path);
        let Some(parent) = destination.parent() else {
            continue;
        };
        let Some(name) = destination.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Ok(guard) = root.ensure_private_subdirectory(parent) else {
            continue;
        };
        let _ = guard.write_atomic(&directory.join(parent), name, bytes, false);
    }
    drop(tx);
    drop(connection);
    prune_copies(state, loupist_id, import_id);
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
