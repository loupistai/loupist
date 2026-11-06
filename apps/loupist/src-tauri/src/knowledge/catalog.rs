use super::authority::require_record;
use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{
    mint_id, validate_knowledge_id, validate_loupist_id, validate_time, validate_title,
    CreateRecord, OwnedRecord, RecordKind, RecordRevision,
};
use super::revision::{load_revision, StoredRevision};
#[cfg(feature = "eval-harness")]
use super::{model::RecordSnapshot, schema::KNOWLEDGE_FORMAT_VERSION};
use rusqlite::{params, Transaction};

/// Source and World records. A Lens page is created by `KnowledgeWrite::file`
/// and a principle by `principle_save`.
pub(super) fn create_record(
    tx: &Transaction<'_>,
    request: CreateRecord,
) -> KnowledgeResult<OwnedRecord> {
    if request.kind == RecordKind::Lens {
        return Err(KnowledgeError::Invalid(
            "Lens pages are filed, not created as bare records",
        ));
    }
    create_record_typed(tx, request, None, false)
}

pub(super) fn create_principle_record(
    tx: &Transaction<'_>,
    request: CreateRecord,
    record_id: Option<String>,
) -> KnowledgeResult<OwnedRecord> {
    create_record_typed(tx, request, record_id, true)
}

fn create_record_typed(
    tx: &Transaction<'_>,
    request: CreateRecord,
    record_id: Option<String>,
    principle: bool,
) -> KnowledgeResult<OwnedRecord> {
    validate_loupist_id(&request.loupist_id)?;
    validate_title(&request.title)?;
    validate_time(request.at_ms)?;
    if let Some(record_id) = &record_id {
        validate_knowledge_id(record_id, "rec")?;
    }
    if request.kind == RecordKind::World {
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_records WHERE owner_loupist_id = ?1 AND kind = 'world')", [&request.loupist_id], |row| row.get(0))?;
        if exists {
            return Err(KnowledgeError::Invalid("Loupist already owns a World"));
        }
    }
    if principle
        && super::lens_page::text_forbidden(super::lens_page::LensKind::Principle, &request.title)
    {
        return Err(KnowledgeError::LensBodyForbidden);
    }
    let record_id = record_id.unwrap_or_else(|| mint_id("rec"));
    tx.execute("INSERT INTO knowledge_records(record_id,kind,owner_loupist_id,title,created_at_ms) VALUES (?1,?2,?3,?4,?5)", params![record_id,request.kind.as_str(),request.loupist_id,request.title,request.at_ms])?;
    if principle {
        tx.execute(
            "INSERT INTO knowledge_lens_metadata (record_id, loupist_id, title, title_fold, archived_at_ms, updated_at_ms, kind) VALUES (?1, ?2, ?3, ?4, NULL, ?5, 'principle')",
            params![record_id, request.loupist_id, request.title, super::lens::fold_lens_title(&request.title), request.at_ms],
        )?;
    }
    Ok(OwnedRecord {
        record_id,
        kind: request.kind,
        owner_loupist_id: request.loupist_id,
        title: request.title,
        revision_id: None,
    })
}

pub(super) fn get_revision(
    tx: &Transaction<'_>,
    loupist_id: &str,
    revision_id: &str,
) -> KnowledgeResult<RecordRevision> {
    let revision = visible_revision(tx, loupist_id, revision_id)?;
    revision.to_model()
}

#[cfg(feature = "eval-harness")]
pub(super) fn record_snapshot(
    tx: &Transaction<'_>,
    loupist_id: &str,
    revision_id: &str,
) -> KnowledgeResult<RecordSnapshot> {
    let revision = visible_revision(tx, loupist_id, revision_id)?;
    let model = revision.to_model()?;
    Ok(RecordSnapshot {
        format_version: KNOWLEDGE_FORMAT_VERSION,
        record_id: model.record_id,
        kind: model.kind,
        revision_id: model.revision_id,
        revision_digest: model.revision_digest,
        dependencies: model.dependencies,
        payload: model.payload,
    })
}

pub(super) fn owned_record(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<OwnedRecord> {
    let record = require_record(tx, loupist_id, record_id)?;
    Ok(OwnedRecord {
        record_id: record.record_id,
        kind: record.kind,
        owner_loupist_id: record.owner_loupist_id,
        title: record.title,
        revision_id: record.head_revision_id,
    })
}
pub(super) fn list_records(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<Vec<OwnedRecord>> {
    validate_loupist_id(loupist_id)?;
    let mut statement = tx.prepare("SELECT record_id FROM knowledge_records WHERE owner_loupist_id = ?1 ORDER BY created_at_ms DESC, record_id")?;
    let ids = statement
        .query_map([loupist_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let removed = super::memory_controls::load(tx, loupist_id)?.removed;
    ids.iter()
        .filter(|id| !removed.contains(*id))
        .map(|id| owned_record(tx, loupist_id, id))
        .collect()
}

fn visible_revision(
    tx: &Transaction<'_>,
    loupist_id: &str,
    revision_id: &str,
) -> KnowledgeResult<StoredRevision> {
    let revision = load_revision(tx, revision_id)?;
    require_record(tx, loupist_id, &revision.record_id)?;
    Ok(revision)
}
