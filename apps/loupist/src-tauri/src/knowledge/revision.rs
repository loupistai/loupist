use std::collections::BTreeMap;

use rusqlite::{params, OptionalExtension, Transaction};

use super::authority::{require_edit, require_record, OwnedRecordState};
use super::canonical::{canonical_string, from_json, revision_digest, to_value};
use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{
    mint_id, validate_digest, validate_knowledge_id, validate_optional_knowledge_id, validate_time,
    EvidenceRevision, RecordKind, RecordRevision,
};

#[derive(Debug, Clone)]
pub(super) struct StoredRevision {
    pub revision_id: String,
    pub record_id: String,
    pub kind: RecordKind,
    pub revision_digest: String,
    pub payload_json: String,
    pub dependency_json: String,
    pub created_at_ms: i64,
}

impl StoredRevision {
    pub(super) fn to_model(&self) -> KnowledgeResult<RecordRevision> {
        Ok(RecordRevision {
            record_id: self.record_id.clone(),
            kind: self.kind,
            revision_id: self.revision_id.clone(),
            revision_digest: self.revision_digest.clone(),
            dependencies: from_json(&self.dependency_json)?,
            payload: from_json(&self.payload_json)?,
            created_at_ms: self.created_at_ms,
        })
    }
}

pub(super) struct PublishedRevision {
    pub revision_id: String,
    pub revision_digest: String,
}

pub(super) fn normalize_dependencies(
    dependencies: &[EvidenceRevision],
    kind: RecordKind,
) -> KnowledgeResult<Vec<EvidenceRevision>> {
    let limit = if kind == RecordKind::World {
        super::MAX_WORLD_DEPENDENCIES
    } else {
        64
    };
    if dependencies.len() > limit {
        return Err(KnowledgeError::Invalid("too many dependencies"));
    }
    let mut normalized = Vec::with_capacity(dependencies.len());
    for dependency in dependencies {
        validate_knowledge_id(&dependency.record_id, "rec")?;
        validate_digest(&dependency.revision_digest)?;
        normalized.push(dependency.clone());
    }
    normalized.sort_by(|left, right| {
        (&left.record_id, left.role.as_str(), &left.revision_digest).cmp(&(
            &right.record_id,
            right.role.as_str(),
            &right.revision_digest,
        ))
    });
    normalized.dedup();
    let mut digests = BTreeMap::<String, String>::new();
    for dependency in &normalized {
        if let Some(prior) = digests.get(&dependency.record_id) {
            if prior != &dependency.revision_digest {
                return Err(KnowledgeError::ConflictingDependencyRevision);
            }
        } else {
            digests.insert(
                dependency.record_id.clone(),
                dependency.revision_digest.clone(),
            );
        }
    }
    Ok(normalized)
}

pub(super) fn current_head(
    tx: &Transaction<'_>,
    record_id: &str,
) -> KnowledgeResult<Option<String>> {
    Ok(tx
        .query_row(
            "SELECT revision_id FROM knowledge_record_heads WHERE record_id = ?1",
            [record_id],
            |row| row.get(0),
        )
        .optional()?)
}

pub(super) fn publish_revision(
    tx: &Transaction<'_>,
    module: &OwnedRecordState,
    loupist_id: &str,
    expected_revision_id: Option<&str>,
    payload: &serde_json::Value,
    dependencies: &[EvidenceRevision],
    at_ms: i64,
) -> KnowledgeResult<PublishedRevision> {
    require_edit(module, loupist_id)?;
    validate_time(at_ms)?;
    validate_optional_knowledge_id(&expected_revision_id.map(str::to_string), "rev")?;
    let dependencies = normalize_dependencies(dependencies, module.kind)?;
    let head = current_head(tx, &module.record_id)?;
    if head.as_deref() != expected_revision_id {
        return Err(KnowledgeError::RevisionConflict {
            expected: expected_revision_id.map(str::to_string),
            actual: head,
        });
    }
    let digest = revision_digest(module.kind, payload, &dependencies)?;
    let revision_id = mint_id("rev");
    let payload_json = canonical_string(payload);
    let dependency_json = canonical_string(&to_value(&dependencies)?);
    // The transaction replaces the current document atomically. Readers never
    // observe a missing head, and rollback restores the previous body in full.
    if head.is_some() {
        tx.execute("UPDATE knowledge_erasure SET armed = 1 WHERE id = 1", [])?;
        tx.execute(
            "DELETE FROM knowledge_record_heads WHERE record_id = ?1",
            [&module.record_id],
        )?;
        tx.execute(
            "DELETE FROM knowledge_world_snapshots WHERE record_id = ?1",
            [&module.record_id],
        )?;
        tx.execute(
            "DELETE FROM knowledge_lens_changes WHERE record_id = ?1",
            [&module.record_id],
        )?;
        tx.execute(
            "DELETE FROM knowledge_search_documents WHERE record_id = ?1",
            [&module.record_id],
        )?;
        tx.execute(
            "DELETE FROM knowledge_record_revisions WHERE record_id = ?1",
            [&module.record_id],
        )?;
        tx.execute("UPDATE knowledge_erasure SET armed = 0 WHERE id = 1", [])?;
    }
    tx.execute(
        "INSERT INTO knowledge_record_revisions (
            revision_id, record_id, revision_digest, payload_json, dependency_json, created_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            revision_id,
            module.record_id,
            digest,
            payload_json,
            dependency_json,
            at_ms
        ],
    )?;
    tx.execute(
        "INSERT INTO knowledge_record_heads (record_id, revision_id) VALUES (?1, ?2)",
        params![module.record_id, revision_id],
    )?;
    Ok(PublishedRevision {
        revision_id,
        revision_digest: digest,
    })
}

pub(super) fn load_revision(
    tx: &Transaction<'_>,
    revision_id: &str,
) -> KnowledgeResult<StoredRevision> {
    validate_knowledge_id(revision_id, "rev")?;
    let Some(revision) = load_revision_optional(tx, revision_id)? else {
        return Err(KnowledgeError::NotFound);
    };
    Ok(revision)
}

// Source material is immutable; metadata edits and added captures replace only
// its catalog. Trusted World references resolve their retained span in this catalog.
pub(super) fn current_source(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<StoredRevision> {
    let record = require_record(tx, loupist_id, record_id)?;
    if record.kind != RecordKind::Source {
        return Err(KnowledgeError::Invalid("record is not a Source"));
    }
    load_revision(
        tx,
        record
            .head_revision_id
            .as_deref()
            .ok_or(KnowledgeError::NotFound)?,
    )
}

pub(super) fn load_revision_by_digest(
    tx: &Transaction<'_>,
    record_id: &str,
    digest: &str,
) -> KnowledgeResult<StoredRevision> {
    validate_knowledge_id(record_id, "rec")?;
    validate_digest(digest)?;
    let revision_id: Option<String> = tx
        .query_row(
            "SELECT revision_id FROM knowledge_record_revisions
             WHERE record_id = ?1 AND revision_digest = ?2
             ORDER BY created_at_ms, revision_id LIMIT 1",
            params![record_id, digest],
            |row| row.get(0),
        )
        .optional()?;
    let Some(revision_id) = revision_id else {
        return Err(KnowledgeError::NotFound);
    };
    load_revision(tx, &revision_id)
}

pub(super) fn visible_by_digest(
    tx: &Transaction<'_>,
    module: &OwnedRecordState,
    digest: &str,
) -> KnowledgeResult<StoredRevision> {
    load_revision_by_digest(tx, &module.record_id, digest)
}

fn load_revision_optional(
    tx: &Transaction<'_>,
    revision_id: &str,
) -> KnowledgeResult<Option<StoredRevision>> {
    let row = tx
        .query_row(
            "SELECT r.revision_id, r.record_id, m.kind, r.revision_digest,
                    r.payload_json, r.dependency_json, r.created_at_ms
             FROM knowledge_record_revisions r
             JOIN knowledge_records m ON m.record_id = r.record_id
             WHERE r.revision_id = ?1",
            [revision_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )
        .optional()?;
    let Some((
        revision_id,
        record_id,
        kind,
        revision_digest,
        payload_json,
        dependency_json,
        created_at_ms,
    )) = row
    else {
        return Ok(None);
    };
    Ok(Some(StoredRevision {
        revision_id,
        record_id,
        kind: RecordKind::parse(&kind)?,
        revision_digest,
        payload_json,
        dependency_json,
        created_at_ms,
    }))
}
