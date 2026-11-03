use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::Mutex,
};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;

use crate::{
    chat::artifacts::{
        ArtifactCommit, ChatArtifact, ChatArtifactRevision, MAX_CHAT_TURN_ARTIFACTS,
    },
    chat::chart::ChartDataset,
    domain::{
        default_loupist_capability_bindings, ChatSession, DeletionJournal,
        LoupistCapabilityBinding, LoupistProfile, LoupistTone,
    },
    fs::artifact_trust::ArtifactTrustError,
    providers::catalog::ModelCatalog,
    updater::PersistedUpdateSchedule,
};

pub(crate) mod app_settings;
mod artifact_history;
pub(crate) mod deletion;
mod folder;
mod host;
pub(crate) mod provider_status;
mod schema;
pub mod user_context;
pub(crate) use folder::StoreConnection;
pub(crate) use host::AppliedConnectorConfig;
pub(crate) use schema::{STORE_APPLICATION_ID, STORE_SCHEMA_VERSION};

/// Fields a profile edit may change; `None` keeps the stored value.
#[derive(Clone, Copy, Debug, Default)]
pub struct LoupistProfileEdit<'a> {
    pub name: Option<&'a str>,
    pub focus: Option<&'a str>,
    pub instructions: Option<&'a str>,
    pub avatar: Option<&'a str>,
    pub tone: Option<LoupistTone>,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("private storage boundary failed: {0}")]
    PrivateStorage(#[from] ArtifactTrustError),
    #[error("stored JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid object: {0}")]
    Invalid(String),
    #[error(
        "database schema {found} is unsupported (expected {expected}); initialize a fresh library"
    )]
    UnsupportedSchema { found: i64, expected: i64 },
    #[error("database schema is nonempty but has user_version 0; refusing to initialize it")]
    UnversionedNonemptySchema,
    #[error("immutable record conflicts with an existing record: {0}")]
    Conflict(&'static str),
    #[error("store lock was poisoned")]
    LockPoisoned,
    /// The folder's Memory schema predates this build; it is refused, never migrated.
    #[error("this Loupist was made by an earlier version of the app")]
    EarlierVersion,
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug)]
pub enum DeletionJournalRecord {
    Valid(DeletionJournal),
    Invalid {
        id: String,
        loupist_id: String,
        reason: String,
    },
}

pub trait LoupistStore: Send + Sync {
    fn create_deletion_journal(&self, journal: &DeletionJournal) -> StoreResult<()>;
    fn replace_deletion_journal(
        &self,
        expected: &DeletionJournal,
        journal: &DeletionJournal,
    ) -> StoreResult<()>;
    fn list_deletion_journals(&self) -> StoreResult<Vec<DeletionJournalRecord>>;
    fn delete_deletion_journal(&self, expected: &DeletionJournal) -> StoreResult<()>;
    fn save_model_catalog(&self, catalog: &ModelCatalog) -> StoreResult<()>;
    fn get_model_catalog(&self) -> StoreResult<Option<ModelCatalog>>;
    fn save_update_schedule(&self, schedule: &PersistedUpdateSchedule) -> StoreResult<()>;
    fn get_update_schedule(&self) -> StoreResult<Option<PersistedUpdateSchedule>>;

    fn create_loupist(&self, profile: &LoupistProfile) -> StoreResult<()>;
    fn save_loupist(&self, profile: &LoupistProfile) -> StoreResult<()>;
    fn reconnect_loupist_workspace(
        &self,
        expected: &LoupistProfile,
        updated: &LoupistProfile,
    ) -> StoreResult<()>;
    fn set_loupist_last_model_profile(
        &self,
        id: &str,
        model_profile_id: &str,
        updated_at_ms: i64,
    ) -> StoreResult<LoupistProfile>;
    fn update_loupist_profile(
        &self,
        id: &str,
        edit: LoupistProfileEdit<'_>,
        updated_at_ms: i64,
    ) -> StoreResult<LoupistProfile>;
    fn delete_loupist(&self, expected: &LoupistProfile) -> StoreResult<()>;
    fn get_loupist(&self, id: &str) -> StoreResult<Option<LoupistProfile>>;
    fn list_loupists(&self) -> StoreResult<Vec<LoupistProfile>>;
    fn save_loupist_capability(&self, binding: &LoupistCapabilityBinding) -> StoreResult<()>;
    fn get_loupist_capability(
        &self,
        loupist_id: &str,
        entry_id: &str,
    ) -> StoreResult<Option<LoupistCapabilityBinding>>;
    fn list_loupist_capabilities(
        &self,
        loupist_id: &str,
    ) -> StoreResult<Vec<LoupistCapabilityBinding>>;

    fn create_chat(&self, chat: &ChatSession) -> StoreResult<()>;
    fn replace_chat(&self, expected: &ChatSession, chat: &ChatSession) -> StoreResult<()>;
    fn delete_chat(&self, expected: &ChatSession) -> StoreResult<()>;
    fn get_chat(&self, id: &str) -> StoreResult<Option<ChatSession>>;
    fn list_chats_for_loupist(&self, loupist_id: &str) -> StoreResult<Vec<ChatSession>>;
    /// Chats updated since `since_ms` whose last message is an answer with tool refusals:
    /// `(chat id, title, updated at, refusals JSON)`. Reads only the last message.
    fn chats_ending_in_refusals(
        &self,
        loupist_id: &str,
        since_ms: i64,
    ) -> StoreResult<Vec<(String, String, i64, String)>>;
    /// Chats updated since `since_ms` whose last message is a finished answer the
    /// model wrote (no refusals, no host-posted receipt, delivery or alert):
    /// `(chat id, title, updated at, answer text)`. Reads only the last message.
    fn chats_ending_in_answer(
        &self,
        loupist_id: &str,
        since_ms: i64,
    ) -> StoreResult<Vec<(String, String, i64, String)>>;
    fn get_chat_artifact(&self, id: &str) -> StoreResult<Option<ChatArtifact>>;
    fn list_chat_artifacts(&self, chat_session_id: &str) -> StoreResult<Vec<ChatArtifact>>;
    /// The Loupist's charts and documents across its chats, newest first: `(artifact, chat title)`.
    fn list_loupist_artifacts(
        &self,
        loupist_id: &str,
        limit: usize,
    ) -> StoreResult<Vec<(ChatArtifact, String)>>;
    fn get_chat_artifact_current(
        &self,
        artifact_id: &str,
    ) -> StoreResult<Option<ChatArtifactRevision>>;
    fn get_chart_dataset(&self, id: &str) -> StoreResult<Option<ChartDataset>>;
}

pub struct SqliteStore {
    connection: Mutex<Connection>,
    user_context_previews: Mutex<HashMap<String, user_context::PendingPreview>>,
    database_path: Option<PathBuf>,
    folder_catalog: bool,
    folders: Mutex<HashMap<String, folder::FolderConnection>>,
}

fn root_identity_columns(profile: &LoupistProfile) -> (Option<String>, Option<String>) {
    profile
        .root_filesystem_identity
        .as_ref()
        .map(|identity| (identity.device.to_string(), identity.inode.to_string()))
        .unzip()
}

fn map_loupist_profile_write(result: rusqlite::Result<usize>) -> StoreResult<usize> {
    match result {
        Ok(changed) => Ok(changed),
        Err(error)
            if matches!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::ConstraintViolation)
            ) =>
        {
            Err(StoreError::Conflict("Loupist identity already exists"))
        }
        Err(error) => Err(error.into()),
    }
}

impl SqliteStore {
    fn update_loupist_fields(
        &self,
        id: &str,
        updated_at_ms: i64,
        update: impl FnOnce(&mut LoupistProfile),
    ) -> StoreResult<LoupistProfile> {
        let mut connection = self.lock_loupist(id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut profile = load_json_by_id::<LoupistProfile>(&transaction, "loupist_profiles", id)?
            .ok_or(StoreError::Conflict("Loupist is missing"))?;
        update(&mut profile);
        profile.updated_at_ms = updated_at_ms.max(profile.updated_at_ms.saturating_add(1));
        profile
            .validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        let (root_device, root_inode) = root_identity_columns(&profile);
        let changed = map_loupist_profile_write(transaction.execute(
            "UPDATE loupist_profiles SET updated_at_ms = ?2, root_device = ?3, root_inode = ?4, data_json = ?5, workspace_root = ?6 WHERE id = ?1",
            params![
                profile.id,
                profile.updated_at_ms,
                root_device,
                root_inode,
                to_json(&profile)?,
                profile.workspace_root,
            ],
        ))?;
        if changed != 1 {
            return Err(StoreError::Conflict("Loupist field update"));
        }
        transaction.commit()?;
        drop(connection);
        self.refresh_folder_registration(&profile)?;
        Ok(profile)
    }

    pub(crate) fn lock(&self) -> StoreResult<StoreConnection<'_>> {
        Ok(StoreConnection::Global(
            self.connection
                .lock()
                .map_err(|_| StoreError::LockPoisoned)?,
        ))
    }

    pub(crate) fn database_path(&self) -> Option<&Path> {
        self.database_path.as_deref()
    }

    pub(crate) fn replace_chat_with(
        &self,
        expected: &ChatSession,
        chat: &ChatSession,
        during: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<(), StoreError>,
    ) -> StoreResult<()> {
        expected
            .validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        chat.validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        if expected.id != chat.id
            || expected.loupist_id != chat.loupist_id
            || expected.pi_session_id != chat.pi_session_id
            || expected.created_at_ms != chat.created_at_ms
            || chat.updated_at_ms < expected.updated_at_ms
        {
            return Err(StoreError::Conflict("immutable Chat identity"));
        }
        let mut connection = self.lock_loupist(&chat.loupist_id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "UPDATE chat_sessions SET updated_at_ms = ?2, data_json = ?3 WHERE id = ?1 AND loupist_id = ?4 AND data_json = ?5",
            params![
                chat.id,
                chat.updated_at_ms,
                to_json(chat)?,
                chat.loupist_id,
                to_json(expected)?,
            ],
        )?;
        if changed != 1 {
            return Err(StoreError::Conflict("Chat compare-and-swap"));
        }
        during(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn save_chat_with_artifacts(
        &self,
        expected: &ChatSession,
        chat: &ChatSession,
        commits: &[ArtifactCommit],
    ) -> StoreResult<()> {
        self.save_chat_with_artifacts_and(expected, chat, commits, |_| Ok(()))
    }

    pub(crate) fn save_chat_with_artifacts_and(
        &self,
        expected: &ChatSession,
        chat: &ChatSession,
        commits: &[ArtifactCommit],
        during: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<(), StoreError>,
    ) -> StoreResult<()> {
        expected
            .validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        chat.validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        if commits.is_empty() || commits.len() > MAX_CHAT_TURN_ARTIFACTS {
            return Err(StoreError::Invalid("artifact commit set is invalid".into()));
        }
        let mut artifact_ids = BTreeSet::new();
        let mut expected_refs = Vec::with_capacity(commits.len());
        let mut source_message_id = None;
        for commit in commits {
            commit
                .validate()
                .map_err(|error| StoreError::Invalid(error.to_string()))?;
            if commit.artifact.chat_session_id != chat.id {
                return Err(StoreError::Conflict("artifact Chat binding"));
            }
            if !artifact_ids.insert(commit.artifact.id.as_str()) {
                return Err(StoreError::Conflict("duplicate artifact commit"));
            }
            match source_message_id {
                None => source_message_id = Some(commit.revision.source_message_id.as_str()),
                Some(existing) if existing != commit.revision.source_message_id => {
                    return Err(StoreError::Conflict("artifact source message"));
                }
                Some(_) => {}
            }
            expected_refs.push(commit.revision.artifact_ref(commit.artifact.title.clone()));
        }
        let message = chat
            .messages
            .iter()
            .find(|message| message.id == source_message_id.unwrap_or_default())
            .ok_or(StoreError::Conflict("artifact source message"))?;
        if message.artifact_refs != expected_refs {
            return Err(StoreError::Conflict("artifact message reference"));
        }

        let mut connection = self.lock_loupist(&chat.loupist_id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored_chat = load_json_by_id::<ChatSession>(&transaction, "chat_sessions", &chat.id)?
            .ok_or(StoreError::Conflict("chat session is missing"))?;
        if stored_chat != *expected
            || expected.id != chat.id
            || expected.loupist_id != chat.loupist_id
            || expected.pi_session_id != chat.pi_session_id
            || expected.created_at_ms != chat.created_at_ms
            || chat.updated_at_ms < expected.updated_at_ms
            || chat.messages.len() != expected.messages.len().saturating_add(1)
        {
            return Err(StoreError::Conflict("artifact Chat update"));
        }

        for commit in commits {
            artifact_history::retain_current(&transaction, &commit.artifact.id)?;
            persist_artifact_commit(&transaction, chat.id.as_str(), commit)?;
            artifact_history::retain_commit(&transaction, chat, commit)?;
        }
        let changed = transaction.execute(
            "UPDATE chat_sessions SET updated_at_ms = ?2, data_json = ?3 WHERE id = ?1 AND data_json = ?4",
            params![chat.id, chat.updated_at_ms, to_json(chat)?, to_json(expected)?],
        )?;
        if changed != 1 {
            return Err(StoreError::Conflict("artifact Chat compare-and-swap"));
        }
        during(&transaction)?;
        transaction.commit()?;
        Ok(())
    }
}

impl LoupistStore for SqliteStore {
    fn create_deletion_journal(&self, journal: &DeletionJournal) -> StoreResult<()> {
        journal
            .validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        let data = to_json(journal)?;
        let connection = self.lock()?;
        let changed = connection.execute(
            "INSERT INTO deletion_journals (id, loupist_id, target_id, updated_at_ms, data_json) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT DO NOTHING",
            params![journal.id, journal.loupist_id, journal.target_id, journal.updated_at_ms, data],
        )?;
        if changed != 1 {
            return Err(StoreError::Conflict("deletion journal creation"));
        }
        Ok(())
    }

    fn replace_deletion_journal(
        &self,
        expected: &DeletionJournal,
        journal: &DeletionJournal,
    ) -> StoreResult<()> {
        expected
            .validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        journal
            .validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        if expected.id != journal.id
            || expected.loupist_id != journal.loupist_id
            || expected.target_id != journal.target_id
            || journal.updated_at_ms < expected.updated_at_ms
        {
            return Err(StoreError::Invalid(
                "deletion journal replacement identity is invalid".into(),
            ));
        }
        let expected_json = to_json(expected)?;
        let data = to_json(journal)?;
        let connection = self.lock()?;
        let changed = connection.execute(
            "UPDATE deletion_journals SET updated_at_ms = ?2, data_json = ?3 WHERE id = ?1 AND loupist_id = ?4 AND target_id = ?5 AND data_json = ?6",
            params![journal.id, journal.updated_at_ms, data, journal.loupist_id, journal.target_id, expected_json],
        )?;
        if changed != 1 {
            return Err(StoreError::Conflict("deletion journal changed"));
        }
        Ok(())
    }

    fn list_deletion_journals(&self) -> StoreResult<Vec<DeletionJournalRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT id, loupist_id, data_json FROM deletion_journals ORDER BY updated_at_ms ASC, id ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (id, loupist_id, data) = row?;
            Ok(match from_json::<DeletionJournal>(&data) {
                Ok(journal) if journal.validate().is_ok() => DeletionJournalRecord::Valid(journal),
                Ok(_) => DeletionJournalRecord::Invalid {
                    id,
                    loupist_id,
                    reason: "deletion journal failed domain validation".into(),
                },
                Err(_) => DeletionJournalRecord::Invalid {
                    id,
                    loupist_id,
                    reason: "deletion journal JSON is invalid".into(),
                },
            })
        })
        .collect()
    }

    fn delete_deletion_journal(&self, expected: &DeletionJournal) -> StoreResult<()> {
        expected
            .validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        let expected_json = to_json(expected)?;
        let connection = self.lock()?;
        let changed = connection.execute(
            "DELETE FROM deletion_journals WHERE id = ?1 AND data_json = ?2",
            params![expected.id, expected_json],
        )?;
        if changed != 1 {
            return Err(StoreError::Conflict("deletion journal changed"));
        }
        Ok(())
    }

    fn save_model_catalog(&self, catalog: &ModelCatalog) -> StoreResult<()> {
        catalog
            .validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO app_settings (id, data_json) VALUES ('model', ?1) ON CONFLICT(id) DO UPDATE SET data_json = excluded.data_json",
            [to_json(catalog)?],
        )?;
        Ok(())
    }

    fn get_model_catalog(&self) -> StoreResult<Option<ModelCatalog>> {
        let connection = self.lock()?;
        load_json_by_id(&connection, "app_settings", "model")
    }

    fn save_update_schedule(&self, schedule: &PersistedUpdateSchedule) -> StoreResult<()> {
        schedule.validate()?;
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO app_settings (id, data_json) VALUES ('update', ?1) ON CONFLICT(id) DO UPDATE SET data_json = excluded.data_json",
            [to_json(schedule)?],
        )?;
        Ok(())
    }

    fn get_update_schedule(&self) -> StoreResult<Option<PersistedUpdateSchedule>> {
        let connection = self.lock()?;
        let schedule: Option<PersistedUpdateSchedule> =
            load_json_by_id(&connection, "app_settings", "update")?;
        if let Some(schedule) = &schedule {
            schedule.validate()?;
        }
        Ok(schedule)
    }

    fn create_loupist(&self, profile: &LoupistProfile) -> StoreResult<()> {
        profile
            .validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        if self.folder_catalog {
            return self.create_folder_loupist(profile);
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if transaction
            .query_row(
                "SELECT 1 FROM loupist_profiles WHERE id = ?1 LIMIT 1",
                [&profile.id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some()
        {
            return Err(StoreError::Conflict("Loupist already exists"));
        }
        let (root_device, root_inode) = root_identity_columns(profile);
        map_loupist_profile_write(transaction.execute(
            "INSERT INTO loupist_profiles (id, updated_at_ms, root_device, root_inode, data_json, workspace_root) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                profile.id,
                profile.updated_at_ms,
                root_device,
                root_inode,
                to_json(profile)?,
                profile.workspace_root
            ],
        ))?;
        for binding in default_loupist_capability_bindings(&profile.id, profile.created_at_ms) {
            binding
                .validate()
                .map_err(|error| StoreError::Invalid(error.to_string()))?;
            transaction.execute(
                "INSERT INTO loupist_capability_bindings (loupist_id, entry_id, updated_at_ms, data_json) VALUES (?1, ?2, ?3, ?4)",
                params![
                    binding.loupist_id,
                    binding.entry_id,
                    binding.updated_at_ms,
                    to_json(&binding)?
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn save_loupist(&self, profile: &LoupistProfile) -> StoreResult<()> {
        profile
            .validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        let mut connection = self.lock_loupist(&profile.id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        load_json_by_id::<LoupistProfile>(&transaction, "loupist_profiles", &profile.id)?
            .ok_or(StoreError::Conflict("Loupist is missing"))?;
        let data = to_json(profile)?;
        let (root_device, root_inode) = root_identity_columns(profile);
        let changed = map_loupist_profile_write(transaction.execute(
            r#"
            UPDATE loupist_profiles
            SET updated_at_ms = ?2,
                root_device = ?3,
                root_inode = ?4,
                data_json = ?5,
                workspace_root = ?6
            WHERE id = ?1 AND updated_at_ms <= ?2
            "#,
            params![
                profile.id,
                profile.updated_at_ms,
                root_device,
                root_inode,
                data,
                profile.workspace_root
            ],
        ))?;
        if changed != 1 {
            return Err(StoreError::Conflict("loupist profile update"));
        }
        transaction.commit()?;
        drop(connection);
        self.refresh_folder_registration(profile)?;
        Ok(())
    }

    fn reconnect_loupist_workspace(
        &self,
        expected: &LoupistProfile,
        updated: &LoupistProfile,
    ) -> StoreResult<()> {
        if self.folder_catalog {
            return self.rebind_folder(expected, updated);
        }
        updated
            .validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        if expected.id != updated.id || updated.updated_at_ms <= expected.updated_at_ms {
            return Err(StoreError::Invalid(
                "workspace reconnection is invalid".into(),
            ));
        }
        let mut connection = self.lock_loupist(&expected.id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (device, inode) = root_identity_columns(updated);
        let changed = map_loupist_profile_write(transaction.execute(
            "UPDATE loupist_profiles SET workspace_root = ?2, root_device = ?3, root_inode = ?4,
             updated_at_ms = ?5, data_json = ?6 WHERE id = ?1 AND data_json = ?7",
            params![
                updated.id,
                updated.workspace_root,
                device,
                inode,
                updated.updated_at_ms,
                to_json(updated)?,
                to_json(expected)?
            ],
        ))?;
        if changed != 1 {
            return Err(StoreError::Conflict(
                "Loupist changed before folder reconnection",
            ));
        }
        // Pi sessions are bound to the old CWD and must be rebuilt from the
        // canonical transcript after a folder is reconnected.
        transaction.execute(
            "UPDATE chat_sessions SET data_json = json_remove(data_json, '$.piSessionCache')
             WHERE loupist_id = ?1",
            [&updated.id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn set_loupist_last_model_profile(
        &self,
        id: &str,
        model_profile_id: &str,
        updated_at_ms: i64,
    ) -> StoreResult<LoupistProfile> {
        self.update_loupist_fields(id, updated_at_ms, |profile| {
            profile.last_model_profile_id = Some(model_profile_id.to_owned());
        })
    }

    fn update_loupist_profile(
        &self,
        id: &str,
        edit: LoupistProfileEdit<'_>,
        updated_at_ms: i64,
    ) -> StoreResult<LoupistProfile> {
        self.update_loupist_fields(id, updated_at_ms, |profile| {
            if let Some(value) = edit.name {
                profile.name = value.to_owned();
            }
            if let Some(value) = edit.focus {
                profile.focus = value.to_owned();
            }
            if let Some(value) = edit.instructions {
                profile.instructions = value.to_owned();
            }
            if let Some(value) = edit.avatar {
                profile.avatar = value.to_owned();
            }
            if let Some(value) = edit.tone {
                profile.tone = value;
            }
        })
    }

    fn delete_loupist(&self, expected: &LoupistProfile) -> StoreResult<()> {
        expected
            .validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        if self.folder_catalog {
            return self.unregister_folder(expected);
        }
        let id = &expected.id;
        let expected_json = to_json(expected)?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = transaction
            .query_row(
                "SELECT data_json FROM loupist_profiles WHERE id = ?1 LIMIT 1",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if current.as_deref() != Some(expected_json.as_str()) {
            return Err(StoreError::Conflict("Loupist changed before deletion"));
        }

        crate::scheduled_tasks::store::erase_loupist_in(&transaction, id)?;
        transaction.execute(
            "DELETE FROM chart_artifact_datasets WHERE artifact_id IN (SELECT id FROM chat_artifacts WHERE chat_session_id IN (SELECT id FROM chat_sessions WHERE loupist_id = ?1))",
            [id],
        )?;
        transaction.execute(
            "DELETE FROM chat_artifacts WHERE chat_session_id IN (SELECT id FROM chat_sessions WHERE loupist_id = ?1)",
            [id],
        )?;
        transaction.execute("DELETE FROM chat_sessions WHERE loupist_id = ?1", [id])?;
        transaction.execute(
            "DELETE FROM loupist_capability_bindings WHERE loupist_id = ?1",
            [id],
        )?;
        let changed = transaction.execute(
            "DELETE FROM loupist_profiles WHERE id = ?1 AND data_json = ?2",
            params![id, expected_json],
        )?;
        if changed != 1 {
            return Err(StoreError::Conflict("Loupist deletion"));
        }
        crate::knowledge::erase_loupist_private(&transaction, id)
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        transaction.commit()?;
        Ok(())
    }

    fn get_loupist(&self, id: &str) -> StoreResult<Option<LoupistProfile>> {
        let connection = self.lock()?;
        load_json_by_id(&connection, "loupist_profiles", id)
    }

    fn list_loupists(&self) -> StoreResult<Vec<LoupistProfile>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT data_json FROM loupist_profiles ORDER BY updated_at_ms DESC, id ASC",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| from_json(&row?)).collect()
    }

    fn save_loupist_capability(&self, binding: &LoupistCapabilityBinding) -> StoreResult<()> {
        binding
            .validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        if self.get_loupist(&binding.loupist_id)?.is_none() {
            return Err(StoreError::Conflict("Loupist is missing"));
        }
        let changed = self.lock_loupist(&binding.loupist_id)?.execute(
            r#"
            INSERT INTO loupist_capability_bindings (loupist_id, entry_id, updated_at_ms, data_json)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(loupist_id, entry_id) DO UPDATE SET
                updated_at_ms = excluded.updated_at_ms,
                data_json = excluded.data_json
            WHERE excluded.updated_at_ms >= loupist_capability_bindings.updated_at_ms
            "#,
            params![
                binding.loupist_id,
                binding.entry_id,
                binding.updated_at_ms,
                to_json(binding)?
            ],
        )?;
        if changed != 1 {
            return Err(StoreError::Conflict("Loupist capability update"));
        }
        Ok(())
    }

    fn get_loupist_capability(
        &self,
        loupist_id: &str,
        entry_id: &str,
    ) -> StoreResult<Option<LoupistCapabilityBinding>> {
        let connection = self.lock_loupist(loupist_id)?;
        connection
            .query_row(
                "SELECT data_json FROM loupist_capability_bindings WHERE loupist_id = ?1 AND entry_id = ?2",
                params![loupist_id, entry_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|value| from_json(&value))
            .transpose()
    }

    fn list_loupist_capabilities(
        &self,
        loupist_id: &str,
    ) -> StoreResult<Vec<LoupistCapabilityBinding>> {
        let connection = self.lock_loupist(loupist_id)?;
        let mut statement = connection.prepare(
            "SELECT data_json FROM loupist_capability_bindings WHERE loupist_id = ?1 ORDER BY entry_id ASC",
        )?;
        let rows = statement.query_map([loupist_id], |row| row.get::<_, String>(0))?;
        rows.map(|row| from_json(&row?)).collect()
    }

    fn create_chat(&self, chat: &ChatSession) -> StoreResult<()> {
        chat.validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        let data = to_json(chat)?;
        self.lock_loupist(&chat.loupist_id)?.execute(
            "INSERT INTO chat_sessions (id, loupist_id, updated_at_ms, data_json) VALUES (?1, ?2, ?3, ?4)",
            params![chat.id, chat.loupist_id, chat.updated_at_ms, data],
        )?;
        Ok(())
    }

    fn replace_chat(&self, expected: &ChatSession, chat: &ChatSession) -> StoreResult<()> {
        self.replace_chat_with(expected, chat, |_| Ok(()))
    }

    fn delete_chat(&self, expected: &ChatSession) -> StoreResult<()> {
        expected
            .validate()
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        let id = &expected.id;
        let loupist_id = &expected.loupist_id;
        let mut connection = self.lock_loupist(loupist_id)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        crate::scheduled_tasks::store::forget_chat_in(&transaction, loupist_id, id)?;
        transaction.execute(
            r#"
            DELETE FROM chart_artifact_datasets
            WHERE artifact_id IN (
                SELECT artifact.id
                FROM chat_artifacts AS artifact
                JOIN chat_sessions AS chat ON chat.id = artifact.chat_session_id
                WHERE chat.id = ?1 AND chat.loupist_id = ?2
            )
            "#,
            params![id, loupist_id],
        )?;
        transaction.execute(
            r#"
            DELETE FROM chat_artifacts
            WHERE chat_session_id IN (
                SELECT id FROM chat_sessions WHERE id = ?1 AND loupist_id = ?2
            )
            "#,
            params![id, loupist_id],
        )?;
        let deleted = transaction.execute(
            "DELETE FROM chat_sessions WHERE id = ?1 AND loupist_id = ?2 AND data_json = ?3",
            params![id, loupist_id, to_json(expected)?],
        )?;
        if deleted != 1 {
            return Err(StoreError::Conflict("Chat changed before deletion"));
        }
        crate::knowledge::note_chat_deleted_in(&transaction, loupist_id, id)
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        transaction.commit()?;
        Ok(())
    }

    fn get_chat(&self, id: &str) -> StoreResult<Option<ChatSession>> {
        let connection = self.lock_record("chat_sessions", id)?;
        load_json_by_id(&connection, "chat_sessions", id)
    }

    fn list_chats_for_loupist(&self, loupist_id: &str) -> StoreResult<Vec<ChatSession>> {
        let connection = self.lock_loupist(loupist_id)?;
        let mut statement = connection.prepare(
            "SELECT data_json FROM chat_sessions WHERE loupist_id = ?1 ORDER BY updated_at_ms DESC, id ASC",
        )?;
        let rows = statement.query_map([loupist_id], |row| row.get::<_, String>(0))?;
        let mut chats = Vec::new();
        for row in rows {
            // One unreadable Chat must not hide the others; it stays stored as it is.
            match from_json::<ChatSession>(&row?) {
                Ok(chat) => chats.push(chat),
                Err(_) => eprintln!("Loupist skipped a Chat it could not read"),
            }
        }
        Ok(chats)
    }

    fn chats_ending_in_refusals(
        &self,
        loupist_id: &str,
        since_ms: i64,
    ) -> StoreResult<Vec<(String, String, i64, String)>> {
        let connection = self.lock_loupist(loupist_id)?;
        let mut statement = connection.prepare(
            "SELECT id, json_extract(data_json, '$.title'), updated_at_ms,
                    json_extract(data_json, '$.messages[#-1].toolRefusals')
             FROM chat_sessions
             WHERE loupist_id = ?1 AND updated_at_ms >= ?2
               AND json_extract(data_json, '$.messages[#-1].role') = 'assistant'
               AND json_extract(data_json, '$.messages[#-1].toolRefusals') IS NOT NULL
             ORDER BY updated_at_ms DESC, id ASC LIMIT 50",
        )?;
        let rows = statement.query_map(rusqlite::params![loupist_id, since_ms], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        rows.collect::<Result<_, _>>().map_err(Into::into)
    }

    fn chats_ending_in_answer(
        &self,
        loupist_id: &str,
        since_ms: i64,
    ) -> StoreResult<Vec<(String, String, i64, String)>> {
        let connection = self.lock_loupist(loupist_id)?;
        // The last message is read once, then its few fields from that small object.
        let mut statement = connection.prepare(
            "SELECT id, title, updated_at_ms, json_extract(last, '$.content')
             FROM (SELECT id, json_extract(data_json, '$.title') AS title, updated_at_ms,
                          json_extract(data_json, '$.messages[#-1]') AS last
                   FROM chat_sessions
                   WHERE loupist_id = ?1 AND updated_at_ms >= ?2)
             WHERE json_extract(last, '$.role') = 'assistant'
               AND json_extract(last, '$.status') = 'complete'
               AND json_extract(last, '$.content') IS NOT NULL
               AND json_extract(last, '$.toolRefusals') IS NULL
               AND json_extract(last, '$.hostComposed') IS NULL
               AND json_extract(last, '$.delivery') IS NULL
               AND json_extract(last, '$.alert') IS NULL
               AND json_extract(last, '$.watch') IS NULL
               AND json_extract(last, '$.earnings') IS NULL
             ORDER BY updated_at_ms DESC, id ASC LIMIT 50",
        )?;
        let rows = statement.query_map(rusqlite::params![loupist_id, since_ms], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        rows.collect::<Result<_, _>>().map_err(Into::into)
    }

    fn get_chat_artifact(&self, id: &str) -> StoreResult<Option<ChatArtifact>> {
        let connection = self.lock_record("chat_artifacts", id)?;
        load_json_by_id(&connection, "chat_artifacts", id)
    }

    fn list_chat_artifacts(&self, chat_session_id: &str) -> StoreResult<Vec<ChatArtifact>> {
        let connection = self.lock_record("chat_sessions", chat_session_id)?;
        let mut statement = connection.prepare(
            "SELECT data_json FROM chat_artifacts WHERE chat_session_id = ?1 ORDER BY updated_at_ms DESC, id ASC",
        )?;
        let rows = statement.query_map([chat_session_id], |row| row.get::<_, String>(0))?;
        rows.map(|row| from_json(&row?)).collect()
    }

    fn list_loupist_artifacts(
        &self,
        loupist_id: &str,
        limit: usize,
    ) -> StoreResult<Vec<(ChatArtifact, String)>> {
        let connection = self.lock_loupist(loupist_id)?;
        let mut statement = connection.prepare(
            "SELECT a.data_json, json_extract(c.data_json, '$.title')
             FROM chat_artifacts a JOIN chat_sessions c ON c.id = a.chat_session_id
             WHERE c.loupist_id = ?1
             ORDER BY a.updated_at_ms DESC, a.id ASC LIMIT ?2",
        )?;
        let rows = statement.query_map(rusqlite::params![loupist_id, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        })?;
        rows.map(|row| {
            let (artifact, title) = row?;
            Ok((from_json(&artifact)?, title))
        })
        .collect()
    }

    fn get_chat_artifact_current(
        &self,
        artifact_id: &str,
    ) -> StoreResult<Option<ChatArtifactRevision>> {
        let connection = self.lock_record("chat_artifacts", artifact_id)?;
        connection
            .query_row(
                "SELECT current_content_json FROM chat_artifacts WHERE id = ?1",
                [artifact_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|data| from_json(&data))
            .transpose()
    }

    fn get_chart_dataset(&self, id: &str) -> StoreResult<Option<ChartDataset>> {
        let connection = self.lock_record("chart_artifact_datasets", id)?;
        let stored = connection
            .query_row(
                "SELECT digest, data_json FROM chart_artifact_datasets WHERE id = ?1",
                [id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        stored
            .map(|(stored_digest, data)| {
                let dataset: ChartDataset = from_json(&data)?;
                dataset
                    .validate()
                    .map_err(|error| StoreError::Invalid(error.to_string()))?;
                if dataset.digest != stored_digest {
                    return Err(StoreError::Invalid("chart dataset digest binding".into()));
                }
                Ok(dataset)
            })
            .transpose()
    }
}

fn persist_artifact_commit(
    transaction: &rusqlite::Transaction<'_>,
    chat_id: &str,
    commit: &ArtifactCommit,
) -> StoreResult<()> {
    let stored_artifact =
        load_json_by_id::<ChatArtifact>(transaction, "chat_artifacts", &commit.artifact.id)?;
    match stored_artifact {
        None => {
            if commit.artifact.current_revision != 1
                || commit.revision.revision != 1
                || commit.artifact.created_at_ms != commit.artifact.updated_at_ms
            {
                return Err(StoreError::Conflict("artifact initial revision"));
            }
            transaction.execute(
                "INSERT INTO chat_artifacts (id, chat_session_id, kind, title, current_revision, created_at_ms, updated_at_ms, data_json, current_content_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    commit.artifact.id,
                    commit.artifact.chat_session_id,
                    commit.artifact.kind.as_str(),
                    commit.artifact.title,
                    commit.artifact.current_revision,
                    commit.artifact.created_at_ms,
                    commit.artifact.updated_at_ms,
                    to_json(&commit.artifact)?,
                    to_json(&commit.revision)?
                ],
            )?;
        }
        Some(previous) => {
            if previous.chat_session_id != chat_id
                || previous.kind != commit.artifact.kind
                || previous.created_at_ms != commit.artifact.created_at_ms
                || previous.current_revision.checked_add(1)
                    != Some(commit.artifact.current_revision)
                || commit.artifact.updated_at_ms < previous.updated_at_ms
            {
                return Err(StoreError::Conflict("artifact revision sequence"));
            }
            let changed = transaction.execute(
                "UPDATE chat_artifacts SET title = ?2, current_revision = ?3, updated_at_ms = ?4, data_json = ?5, current_content_json = ?6 WHERE id = ?1 AND data_json = ?7",
                params![
                    commit.artifact.id,
                    commit.artifact.title,
                    commit.artifact.current_revision,
                    commit.artifact.updated_at_ms,
                    to_json(&commit.artifact)?,
                    to_json(&commit.revision)?,
                    to_json(&previous)?
                ],
            )?;
            if changed != 1 {
                return Err(StoreError::Conflict("artifact compare-and-swap"));
            }
        }
    }
    transaction.execute(
        "DELETE FROM chart_artifact_datasets WHERE artifact_id = ?1",
        [&commit.artifact.id],
    )?;
    for dataset in &commit.datasets {
        transaction.execute(
            "INSERT INTO chart_artifact_datasets (id, artifact_id, digest, data_json) VALUES (?1, ?2, ?3, ?4)",
            params![
                dataset.id,
                commit.revision.artifact_id,
                dataset.digest,
                to_json(dataset)?
            ],
        )?;
    }
    Ok(())
}

fn load_json_by_id<T: DeserializeOwned>(
    connection: &Connection,
    table: &'static str,
    id: &str,
) -> StoreResult<Option<T>> {
    let data = connection
        .query_row(
            &format!("SELECT data_json FROM {table} WHERE id = ?1"),
            params![id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    data.map(|data| from_json(&data)).transpose()
}

fn to_json<T: Serialize>(value: &T) -> StoreResult<String> {
    Ok(serde_json::to_string(value)?)
}

fn from_json<T: DeserializeOwned>(value: &str) -> StoreResult<T> {
    Ok(serde_json::from_str(value)?)
}

#[cfg(test)]
mod tests;
