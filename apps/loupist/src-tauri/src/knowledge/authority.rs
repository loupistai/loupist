use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{validate_knowledge_id, validate_loupist_id, RecordKind};
use rusqlite::{params, OptionalExtension, Transaction};

#[derive(Debug, Clone)]
pub(super) struct OwnedRecordState {
    pub record_id: String,
    pub kind: RecordKind,
    pub owner_loupist_id: String,
    pub title: String,
    pub head_revision_id: Option<String>,
    pub archived_at_ms: Option<i64>,
}

pub(super) fn require_record(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<OwnedRecordState> {
    validate_loupist_id(loupist_id)?;
    validate_knowledge_id(record_id, "rec")?;
    let row = tx.query_row("SELECT m.kind, m.owner_loupist_id, m.title, h.revision_id FROM knowledge_records m LEFT JOIN knowledge_record_heads h ON h.record_id = m.record_id WHERE m.record_id = ?1", [record_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?))).optional()?.ok_or(KnowledgeError::NotFound)?;
    if row.1 != loupist_id {
        return Err(KnowledgeError::Unauthorized(
            "record belongs to another Loupist",
        ));
    }
    if super::memory_controls::load(tx, loupist_id)?
        .removed
        .contains(record_id)
    {
        return Err(KnowledgeError::NotFound);
    }
    let metadata = tx
            .query_row(
            "SELECT title, archived_at_ms FROM knowledge_lens_metadata WHERE record_id = ?1 AND loupist_id = ?2",
            params![record_id, loupist_id],
            |meta| {
                Ok((
                    meta.get::<_, String>(0)?,
                    meta.get::<_, Option<i64>>(1)?,
                ))
            },
        )
        .optional()?;
    Ok(OwnedRecordState {
        record_id: record_id.into(),
        kind: RecordKind::parse(&row.0)?,
        owner_loupist_id: row.1,
        title: metadata
            .as_ref()
            .map(|(title, _)| title.clone())
            .unwrap_or(row.2),
        head_revision_id: row.3,
        archived_at_ms: metadata.as_ref().and_then(|(_, archived)| *archived),
    })
}
pub(super) fn require_edit(record: &OwnedRecordState, loupist_id: &str) -> KnowledgeResult<()> {
    if record.owner_loupist_id != loupist_id {
        return Err(KnowledgeError::Unauthorized(
            "record belongs to another Loupist",
        ));
    }
    Ok(())
}
pub(super) fn revision_in_record(revision_record_id: &str, record_id: &str) -> KnowledgeResult<()> {
    if revision_record_id != record_id {
        return Err(KnowledgeError::Unauthorized(
            "revision belongs to another record",
        ));
    }
    Ok(())
}
