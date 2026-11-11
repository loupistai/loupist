//! Reviewing an added pack's suggestions: reading, accepting and skipping them.
use rusqlite::Transaction;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use super::{admit, internal, knowledge_error, with_tx, LoupistRequest};
use crate::app::{AppState, CommandError};
use crate::knowledge::shared_pack;
use crate::runs::maintenance::MaintenanceActivityKind;
use crate::sharing::import;
use crate::sharing::pack;
use crate::sharing::store::{self, ImportMode};
use crate::sharing::update;

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Accept,
    Skip,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
