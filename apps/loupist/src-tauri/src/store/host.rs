//! Host-only environment identity and configuration reconciliation metadata.
//! Editable values stay in TOML; these rows only attest what the host applied.
use super::{SqliteStore, StoreError, StoreResult};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::path::Path;

const HOST_DDL: &str = "CREATE TABLE IF NOT EXISTS host_environment (
        id INTEGER PRIMARY KEY CHECK(id=1), environment_id TEXT NOT NULL
     ) STRICT;
     CREATE TABLE IF NOT EXISTS connector_config_state (
        entry_id TEXT PRIMARY KEY NOT NULL,
        config_digest TEXT NOT NULL,
        scope_digest TEXT NOT NULL
     ) STRICT;
     CREATE TABLE IF NOT EXISTS connector_scope_pending (
        entry_id TEXT PRIMARY KEY NOT NULL
     ) STRICT;
     CREATE TABLE IF NOT EXISTS credential_namespace_created (
        id INTEGER PRIMARY KEY CHECK(id=1)
     ) STRICT;
     CREATE TABLE IF NOT EXISTS connector_credential_status (
        entry_id TEXT PRIMARY KEY NOT NULL,
        status_json TEXT NOT NULL
     ) STRICT;";

fn is_environment_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id)
        .ok()
        .map(|value| value.to_string())
        .as_deref()
        == Some(id)
}

/// The environment id a catalog recorded, read without changing the file. A
/// catalog set aside as an earlier format hands it to its successor, so
/// Keychain items keyed by it stay reachable. `None` when it cannot be read.
pub(crate) fn recorded_environment_id(catalog: &Path) -> Option<String> {
    // NOFOLLOW refuses a symlink in any component; read through the parent's
    // canonical spelling as `SqliteStore::open` does, keeping the leaf unfollowed.
    let catalog = catalog
        .parent()?
        .canonicalize()
        .ok()?
        .join(catalog.file_name()?);
    let connection = Connection::open_with_flags(
        catalog,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .ok()?;
    let id: String = connection
        .query_row(
            "SELECT environment_id FROM host_environment WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .ok()?;
    is_environment_id(&id).then_some(id)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppliedConnectorConfig {
    pub config_digest: String,
    pub scope_digest: String,
}

impl SqliteStore {
    pub(crate) fn initialize_host_environment(&self) -> StoreResult<String> {
        self.seed_host_environment(&uuid::Uuid::new_v4().to_string(), true)
    }

    /// Records `id` as this catalog's environment identity unless it has one.
    pub(crate) fn adopt_host_environment(&self, id: &str) -> StoreResult<String> {
        if !is_environment_id(id) {
            return Err(StoreError::Invalid(
                "host environment identity is invalid".into(),
            ));
        }
        self.seed_host_environment(id, false)
    }

    fn seed_host_environment(&self, candidate: &str, fresh_namespace: bool) -> StoreResult<String> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        tx.execute_batch(HOST_DDL)?;
        let seeded = tx.execute(
            "INSERT OR IGNORE INTO host_environment(id, environment_id) VALUES(1, ?1)",
            [candidate],
        )?;
        if seeded != 0 && fresh_namespace {
            tx.execute("INSERT INTO credential_namespace_created(id) VALUES(1)", [])?;
        }
        let id: String = tx.query_row(
            "SELECT environment_id FROM host_environment WHERE id=1",
            [],
            |row| row.get(0),
        )?;
        if !is_environment_id(&id) {
            return Err(StoreError::Invalid(
                "host environment identity is invalid".into(),
            ));
        }
        tx.commit()?;
        Ok(id)
    }

    pub(crate) fn legacy_credential_namespace(&self) -> StoreResult<bool> {
        Ok(!self.lock()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM credential_namespace_created WHERE id=1)",
            [],
            |row| row.get::<_, bool>(0),
        )?)
    }

    pub(crate) fn connector_credential_status(
        &self,
        entry_id: &str,
    ) -> StoreResult<Option<String>> {
        self.lock()?
            .query_row(
                "SELECT status_json FROM connector_credential_status WHERE entry_id=?1",
                [entry_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn set_connector_credential_status(
        &self,
        entry_id: &str,
        status: &str,
    ) -> StoreResult<()> {
        self.lock()?.execute(
            "INSERT INTO connector_credential_status(entry_id,status_json) VALUES(?1,?2)
             ON CONFLICT(entry_id) DO UPDATE SET status_json=excluded.status_json",
            params![entry_id, status],
        )?;
        Ok(())
    }

    pub(crate) fn connector_scope_pending(&self, entry_id: &str) -> StoreResult<bool> {
        self.lock()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM connector_scope_pending WHERE entry_id=?1)",
                [entry_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub(crate) fn note_connector_scope_change(&self, entry_id: &str) -> StoreResult<()> {
        self.lock()?.execute(
            "INSERT OR IGNORE INTO connector_scope_pending(entry_id) VALUES(?1)",
            [entry_id],
        )?;
        Ok(())
    }

    pub(crate) fn finish_connector_scope_change(
        &self,
        entry_id: &str,
        applied: &AppliedConnectorConfig,
    ) -> StoreResult<()> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        Self::write_applied_connector_config(&tx, entry_id, applied)?;
        tx.execute(
            "DELETE FROM connector_scope_pending WHERE entry_id=?1",
            [entry_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn applied_connector_config(
        &self,
        entry_id: &str,
    ) -> StoreResult<Option<AppliedConnectorConfig>> {
        self.lock()?
            .query_row(
                "SELECT config_digest, scope_digest FROM connector_config_state WHERE entry_id=?1",
                [entry_id],
                |row| {
                    Ok(AppliedConnectorConfig {
                        config_digest: row.get(0)?,
                        scope_digest: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn set_applied_connector_config(
        &self,
        entry_id: &str,
        applied: &AppliedConnectorConfig,
    ) -> StoreResult<()> {
        let connection = self.lock()?;
        Self::write_applied_connector_config(&connection, entry_id, applied)
    }

    fn write_applied_connector_config(
        connection: &Connection,
        entry_id: &str,
        applied: &AppliedConnectorConfig,
    ) -> StoreResult<()> {
        connection.execute(
            "INSERT INTO connector_config_state(entry_id,config_digest,scope_digest) VALUES(?1,?2,?3)
             ON CONFLICT(entry_id) DO UPDATE SET config_digest=excluded.config_digest,scope_digest=excluded.scope_digest",
            params![entry_id, applied.config_digest, applied.scope_digest],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_identity_survives_restart_and_is_distinct_per_catalog() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("catalog.sqlite3");
        let store = SqliteStore::open_catalog(&path).unwrap();
        let id = store.initialize_host_environment().unwrap();
        assert_eq!(id, store.initialize_host_environment().unwrap());
        drop(store);
        assert_eq!(
            id,
            SqliteStore::open_catalog(&path)
                .unwrap()
                .initialize_host_environment()
                .unwrap()
        );
        assert_ne!(
            id,
            SqliteStore::open_in_memory()
                .unwrap()
                .initialize_host_environment()
                .unwrap()
        );
    }
}
