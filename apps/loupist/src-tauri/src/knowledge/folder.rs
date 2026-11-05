//! Validate a copied Loupist folder before registering it for execution.
use super::{
    canonical::{digest_bytes, from_json, revision_digest},
    error::{KnowledgeError, KnowledgeResult},
    model::{validate_digest, EvidenceRevision, RecordKind, MAX_BLOB_BYTES},
    schema,
};
use crate::{
    fs::artifact_trust::{read_private_regular_file_bounded, ArtifactTrustError},
    store::{STORE_APPLICATION_ID, STORE_SCHEMA_VERSION},
};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::Path;
pub(crate) fn validate_folder(directory: &Path) -> KnowledgeResult<()> {
    let database = directory.join("loupist.sqlite3");
    // Admission is read-only: a damaged or incomplete research folder must
    // never acquire an empty database as a side effect of validation.
    let metadata = std::fs::symlink_metadata(&database)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(KnowledgeError::Invalid(
            "Loupist folder database is not a regular file",
        ));
    }
    #[cfg(windows)]
    let _database_guard = crate::fs::windows::reopen_regular_no_reparse_for_identity(&database)?;
    let connection = Connection::open_with_flags(
        &database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let application_id: i64 =
        connection.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    if version != STORE_SCHEMA_VERSION || application_id != STORE_APPLICATION_ID {
        return Err(KnowledgeError::Invalid(
            "Loupist folder schema is unsupported",
        ));
    }
    let tables = {
        let mut statement = connection.prepare(
            "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let names = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        names
    };
    match schema::inspect_knowledge(&connection, &tables)? {
        schema::SchemaState::Current => {}
        // An earlier build's folder is refused with a typed state, never migrated.
        schema::SchemaState::Unsupported(found) if found < schema::KNOWLEDGE_SCHEMA_VERSION => {
            return Err(KnowledgeError::EarlierVersion)
        }
        _ => {
            return Err(KnowledgeError::Invalid(
                "Loupist folder schema is unsupported",
            ))
        }
    }
    let profiles = {
        let mut statement = connection
            .prepare("SELECT id, workspace_root, updated_at_ms, data_json FROM loupist_profiles")?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    if profiles.len() != 1 {
        return Err(KnowledgeError::Invalid(
            "a Loupist folder must contain exactly one Loupist",
        ));
    }
    let (loupist_id, root, updated, data) = &profiles[0];
    let profile: crate::domain::LoupistProfile = serde_json::from_str(data)?;
    profile
        .validate()
        .map_err(|_| KnowledgeError::Invalid("Loupist profile is invalid"))?;
    if &profile.id != loupist_id
        || &profile.workspace_root != root
        || &profile.updated_at_ms != updated
    {
        return Err(KnowledgeError::Invalid(
            "Loupist profile metadata is inconsistent",
        ));
    }
    // Knowledge's private records intentionally lack profile foreign keys;
    // enforce single-Loupist ownership when admitting a portable folder.
    for table in &tables {
        let quoted = format!("\"{}\"", table.replace('"', "\"\""));
        let mut columns = connection.prepare(&format!("PRAGMA table_info({quoted})"))?;
        let names = columns
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        for owner in ["loupist_id", "owner_loupist_id"] {
            if names.iter().any(|name| name == owner) {
                let foreign: bool = connection.query_row(
                    &format!("SELECT EXISTS(SELECT 1 FROM {quoted} WHERE {owner}<>?1)"),
                    [loupist_id],
                    |r| r.get(0),
                )?;
                if foreign {
                    return Err(KnowledgeError::Invalid(
                        "Loupist folder contains another Loupist's records",
                    ));
                }
            }
        }
    }
    super::graph_jobs::validate_saved_settings(&connection)?;
    crate::scheduled_tasks::store::validate_saved_tasks(&connection)
        .map_err(|_| KnowledgeError::Invalid("saved scheduled tasks are invalid"))?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(KnowledgeError::Invalid(
            "Loupist folder database is corrupt",
        ));
    }
    if connection
        .prepare("PRAGMA foreign_key_check")?
        .query([])?
        .next()?
        .is_some()
    {
        return Err(KnowledgeError::Invalid(
            "Loupist folder contains broken references",
        ));
    }
    let mut revisions = connection.prepare(
        "SELECT m.kind, r.revision_digest, r.payload_json, r.dependency_json
         FROM knowledge_record_revisions r
         JOIN knowledge_records m ON m.record_id = r.record_id",
    )?;
    let mut rows = revisions.query([])?;
    while let Some(row) = rows.next()? {
        let kind = RecordKind::parse(&row.get::<_, String>(0)?)?;
        let stored_digest: String = row.get(1)?;
        let payload: Value = from_json(&row.get::<_, String>(2)?)?;
        let dependencies: Vec<EvidenceRevision> = from_json(&row.get::<_, String>(3)?)?;
        if revision_digest(kind, &payload, &dependencies)? != stored_digest {
            return Err(KnowledgeError::DigestMismatch);
        }
    }
    let mut notes =
        connection.prepare("SELECT body_json, record_digest FROM knowledge_research_records UNION ALL SELECT body_json, body_digest FROM knowledge_research_experiences")?;
    let mut rows = notes.query([])?;
    while let Some(row) = rows.next()? {
        let value: Value = from_json(&row.get::<_, String>(0)?)?;
        if super::canonical::digest_json(&value)? != row.get::<_, String>(1)? {
            return Err(KnowledgeError::DigestMismatch);
        }
    }
    let mut statement =
        connection.prepare("SELECT digest, byte_length FROM knowledge_blobs ORDER BY digest")?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (digest, length) in rows {
        read_retained_snapshot_blob(directory, &digest, length)?;
    }
    Ok(())
}

fn read_retained_snapshot_blob(
    directory: &Path,
    digest: &str,
    length: i64,
) -> KnowledgeResult<Vec<u8>> {
    validate_digest(digest)?;
    if length < 0 || length > MAX_BLOB_BYTES as i64 {
        return Err(KnowledgeError::Invalid("blob is too large"));
    }
    let path = directory
        .join("knowledge-blobs")
        .join("sha256")
        .join(&digest[..2])
        .join(digest);
    let bytes = read_snapshot_blob(&path)?;
    if bytes.len() as i64 != length || digest_bytes(&bytes) != digest {
        return Err(KnowledgeError::DigestMismatch);
    }
    Ok(bytes)
}

fn read_snapshot_blob(path: &Path) -> KnowledgeResult<Vec<u8>> {
    match read_private_regular_file_bounded(path, MAX_BLOB_BYTES as u64) {
        Ok(bytes) => Ok(bytes),
        Err(ArtifactTrustError::Oversized) => Err(KnowledgeError::Invalid("blob is too large")),
        Err(ArtifactTrustError::Untrusted) => {
            Err(KnowledgeError::Invalid("blob path is untrusted"))
        }
        Err(ArtifactTrustError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(KnowledgeError::MissingBytes)
        }
        Err(error) => Err(error.into()),
    }
}
