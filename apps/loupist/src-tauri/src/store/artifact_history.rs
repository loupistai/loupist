use super::*;
use crate::chat::artifacts::{
    ChatArtifactPayload, ChatArtifactSnapshot, ChatArtifactVersion, MAX_ARTIFACT_HISTORY_BYTES,
    MAX_ARTIFACT_REVISIONS,
};

// Optional feature tables follow the sharing-store pattern. No user_version change.
fn ensure_table(connection: &Connection) -> StoreResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS chat_artifact_revisions (
            artifact_id TEXT NOT NULL REFERENCES chat_artifacts(id) ON DELETE CASCADE,
            revision INTEGER NOT NULL CHECK (revision > 0),
            metadata_json TEXT NOT NULL,
            snapshot_json TEXT NOT NULL,
            byte_size INTEGER NOT NULL CHECK (byte_size > 0),
            PRIMARY KEY (artifact_id, revision)
        ) STRICT;",
    )?;
    Ok(())
}

fn snapshot_for_commit(chat: &ChatSession, commit: &ArtifactCommit) -> ChatArtifactSnapshot {
    let evidence_refs = match &commit.revision.payload {
        ChatArtifactPayload::Grid { grid, .. } => {
            let cited = grid.evidence_ids();
            chat.messages
                .iter()
                .chain(chat.versions.iter().flat_map(|version| &version.messages))
                .find(|message| message.id == commit.revision.source_message_id)
                .map(|message| {
                    message
                        .evidence_refs
                        .iter()
                        .filter(|evidence| cited.contains(&evidence.id))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };
    ChatArtifactSnapshot {
        artifact: commit.artifact.clone(),
        revision: commit.revision.clone(),
        chart_dataset: commit.datasets.first().cloned(),
        evidence_refs,
        restored_from_revision_id: None,
    }
}

fn current_snapshot(
    connection: &Connection,
    id: &str,
) -> StoreResult<Option<ChatArtifactSnapshot>> {
    let Some(artifact) = load_json_by_id::<ChatArtifact>(connection, "chat_artifacts", id)? else {
        return Ok(None);
    };
    let revision: ChatArtifactRevision = from_json(&connection.query_row(
        "SELECT current_content_json FROM chat_artifacts WHERE id = ?1",
        [id],
        |row| row.get::<_, String>(0),
    )?)?;
    let datasets = match &revision.payload {
        ChatArtifactPayload::Chart { chart, .. } => {
            let dataset = load_json_by_id::<ChartDataset>(
                connection,
                "chart_artifact_datasets",
                &chart.dataset_id,
            )?
            .ok_or(StoreError::Conflict("chart dataset is missing"))?;
            vec![dataset]
        }
        _ => Vec::new(),
    };
    let chat =
        load_json_by_id::<ChatSession>(connection, "chat_sessions", &artifact.chat_session_id)?
            .ok_or(StoreError::Conflict("artifact Chat is missing"))?;
    let commit = ArtifactCommit {
        artifact,
        revision,
        datasets,
    };
    commit
        .validate()
        .map_err(|error| StoreError::Invalid(error.to_string()))?;
    Ok(Some(snapshot_for_commit(&chat, &commit)))
}

fn insert_snapshot(connection: &Connection, snapshot: &ChatArtifactSnapshot) -> StoreResult<()> {
    let metadata = to_json(&snapshot.version())?;
    let data = to_json(snapshot)?;
    let bytes = metadata.len() + data.len();
    if bytes > MAX_ARTIFACT_HISTORY_BYTES {
        return Err(StoreError::Invalid(
            "artifact revision exceeds the 32 MiB history limit".into(),
        ));
    }
    connection.execute(
        "INSERT INTO chat_artifact_revisions (artifact_id, revision, metadata_json, snapshot_json, byte_size)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![snapshot.artifact.id, snapshot.revision.revision, metadata, data, bytes as i64],
    )?;
    prune(connection, &snapshot.artifact.id)
}

fn prune(connection: &Connection, id: &str) -> StoreResult<()> {
    let mut statement = connection.prepare(
        "SELECT revision, byte_size FROM chat_artifact_revisions WHERE artifact_id = ?1 ORDER BY revision DESC",
    )?;
    let rows = statement
        .query_map([id], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, i64>(1)? as usize))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut bytes = 0;
    for (index, (revision, size)) in rows.into_iter().enumerate() {
        bytes += size;
        if index >= MAX_ARTIFACT_REVISIONS || bytes > MAX_ARTIFACT_HISTORY_BYTES {
            connection.execute(
                "DELETE FROM chat_artifact_revisions WHERE artifact_id = ?1 AND revision = ?2",
                params![id, revision],
            )?;
        }
    }
    Ok(())
}

/// Retain an existing current revision before its first replacement after this feature.
pub(super) fn retain_current(connection: &Connection, id: &str) -> StoreResult<()> {
    ensure_table(connection)?;
    let retained: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM chat_artifact_revisions r JOIN chat_artifacts a ON a.id = r.artifact_id
         WHERE a.id = ?1 AND r.revision = a.current_revision)", [id], |row| row.get(0),
    )?;
    if !retained {
        if let Some(snapshot) = current_snapshot(connection, id)? {
            insert_snapshot(connection, &snapshot)?;
        }
    }
    Ok(())
}

pub(super) fn retain_commit(
    connection: &Connection,
    chat: &ChatSession,
    commit: &ArtifactCommit,
) -> StoreResult<()> {
    insert_snapshot(connection, &snapshot_for_commit(chat, commit))
}

fn read_snapshot(
    connection: &Connection,
    id: &str,
    revision: u32,
) -> StoreResult<Option<ChatArtifactSnapshot>> {
    connection.query_row(
        "SELECT snapshot_json FROM chat_artifact_revisions WHERE artifact_id = ?1 AND revision = ?2",
        params![id, revision], |row| row.get::<_, String>(0),
    ).optional()?.map(|data| {
        let snapshot: ChatArtifactSnapshot = from_json(&data)?;
        let commit = ArtifactCommit {
            artifact: snapshot.artifact.clone(), revision: snapshot.revision.clone(),
            datasets: snapshot.chart_dataset.iter().cloned().collect(),
        };
        commit.validate().map_err(|error| StoreError::Invalid(error.to_string()))?;
        if snapshot.artifact.id != id || snapshot.revision.revision != revision {
            return Err(StoreError::Conflict("artifact history binding"));
        }
        Ok(snapshot)
    }).transpose()
}

impl SqliteStore {
    /// Returns the requested content with current_revision pointing to the latest version.
    /// The title and updated time describe the requested snapshot, not a later answer.
    pub fn read_chat_artifact(
        &self,
        id: &str,
        revision_id: Option<u32>,
    ) -> StoreResult<Option<ChatArtifactSnapshot>> {
        let mut connection = self.lock_record("chat_artifacts", id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        retain_current(&transaction, id)?;
        let Some(current) = load_json_by_id::<ChatArtifact>(&transaction, "chat_artifacts", id)?
        else {
            transaction.commit()?;
            return Ok(None);
        };
        let mut snapshot = read_snapshot(
            &transaction,
            id,
            revision_id.unwrap_or(current.current_revision),
        )?;
        if let Some(snapshot) = &mut snapshot {
            snapshot.artifact.current_revision = current.current_revision;
        }
        transaction.commit()?;
        Ok(snapshot)
    }

    pub fn list_chat_artifact_versions(&self, id: &str) -> StoreResult<Vec<ChatArtifactVersion>> {
        let mut connection = self.lock_record("chat_artifacts", id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        retain_current(&transaction, id)?;
        let versions = {
            let mut statement = transaction.prepare(
                "SELECT metadata_json FROM chat_artifact_revisions WHERE artifact_id = ?1 ORDER BY revision DESC",
            )?;
            let rows = statement.query_map([id], |row| row.get::<_, String>(0))?;
            rows.map(|row| from_json(&row?))
                .collect::<StoreResult<Vec<_>>>()?
        };
        transaction.commit()?;
        Ok(versions)
    }

    /// Restore is serialized with publication and guarded by the version the user viewed.
    /// The original producer remains provenance; restored_from identifies the user's action.
    pub fn restore_chat_artifact(
        &self,
        chat: &ChatSession,
        id: &str,
        revision_id: u32,
        expected_current_revision: u32,
        at_ms: i64,
    ) -> StoreResult<ChatArtifactSnapshot> {
        let mut connection = self.lock_loupist(&chat.loupist_id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored_chat = load_json_by_id::<ChatSession>(&transaction, "chat_sessions", &chat.id)?;
        if stored_chat.as_ref() != Some(chat) {
            return Err(StoreError::Conflict("Chat changed before artifact restore"));
        }
        let current = load_json_by_id::<ChatArtifact>(&transaction, "chat_artifacts", id)?
            .ok_or(StoreError::Conflict("artifact is missing"))?;
        if current.chat_session_id != chat.id
            || current.current_revision != expected_current_revision
        {
            return Err(StoreError::Conflict("artifact changed before restore"));
        }
        retain_current(&transaction, id)?;
        let mut snapshot = read_snapshot(&transaction, id, revision_id)?.ok_or(
            StoreError::Conflict("artifact revision is no longer available"),
        )?;
        let revision = current
            .current_revision
            .checked_add(1)
            .ok_or(StoreError::Conflict("artifact revision limit"))?;
        snapshot.artifact.current_revision = revision;
        snapshot.artifact.updated_at_ms = at_ms.max(current.updated_at_ms);
        snapshot.revision.revision = revision;
        snapshot.revision.created_at_ms = snapshot.artifact.updated_at_ms;
        snapshot.restored_from_revision_id = Some(revision_id);
        let commit = ArtifactCommit {
            artifact: snapshot.artifact.clone(),
            revision: snapshot.revision.clone(),
            datasets: snapshot.chart_dataset.iter().cloned().collect(),
        };
        commit
            .validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        persist_artifact_commit(&transaction, &chat.id, &commit)?;
        insert_snapshot(&transaction, &snapshot)?;
        transaction.commit()?;
        Ok(snapshot)
    }
}
