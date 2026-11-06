//! User controls use the existing per-Loupist JSON settings store. Removed
//! records remain tombstoned so immutable evidence and past receipts stay intact.
use super::{KnowledgeError, KnowledgeResult, RecordKind};
use rusqlite::Transaction;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MemoryControls {
    pub paused: BTreeSet<String>,
    pub removed: BTreeSet<String>,
}

pub fn load(tx: &Transaction<'_>, loupist: &str) -> KnowledgeResult<MemoryControls> {
    super::model::validate_loupist_id(loupist)?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='app_settings')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(MemoryControls::default());
    }
    Ok(super::graph_jobs::load_settings(tx, loupist)?.memory_controls)
}

pub(crate) fn save(
    tx: &Transaction<'_>,
    loupist: &str,
    controls: &MemoryControls,
) -> KnowledgeResult<()> {
    let mut settings = super::graph_jobs::load_settings(tx, loupist)?;
    settings.memory_controls = controls.clone();
    super::graph_jobs::store_settings(tx, loupist, &settings)
}

pub fn pause(
    tx: &Transaction<'_>,
    loupist: &str,
    record: &str,
    paused: bool,
) -> KnowledgeResult<MemoryControls> {
    let owned = super::authority::require_record(tx, loupist, record)?;
    if owned.kind != RecordKind::Lens {
        return Err(KnowledgeError::Invalid(
            "only a principle or method can be paused",
        ));
    }
    if super::lens::lens_kind(tx, loupist, record)? == super::lens_page::LensKind::Perspective {
        return Err(KnowledgeError::Invalid("the perspective cannot be paused"));
    }
    let mut controls = load(tx, loupist)?;
    if paused {
        controls.paused.insert(record.into());
    } else {
        controls.paused.remove(record);
    }
    save(tx, loupist, &controls)?;
    Ok(controls)
}

pub fn require_delivery(tx: &Transaction<'_>, loupist: &str, record: &str) -> KnowledgeResult<()> {
    let controls = load(tx, loupist)?;
    if controls.removed.contains(record) {
        return Err(KnowledgeError::NotFound);
    }
    if controls.paused.contains(record) {
        return Err(KnowledgeError::Invalid("this Memory item is paused"));
    }
    Ok(())
}
