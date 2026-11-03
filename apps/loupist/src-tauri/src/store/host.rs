//! Host-only environment identity and configuration reconciliation metadata.
//! Editable values stay in TOML; these rows only attest what the host applied.
use super::{SqliteStore, StoreError, StoreResult};
use rusqlite::{params, OptionalExtension};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppliedConnectorConfig {
    pub config_digest: String,
    pub scope_digest: String,
}

impl SqliteStore {
    pub(crate) fn initialize_host_environment(&self) -> StoreResult<String> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS host_environment (
                id INTEGER PRIMARY KEY CHECK(id=1), environment_id TEXT NOT NULL
             ) STRICT;
             CREATE TABLE IF NOT EXISTS connector_config_state (
                entry_id TEXT PRIMARY KEY NOT NULL,
                config_digest TEXT NOT NULL,
                scope_digest TEXT NOT NULL
             ) STRICT;",
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO host_environment(id, environment_id) VALUES(1, ?1)",
            [uuid::Uuid::new_v4().to_string()],
        )?;
        let id: String = tx.query_row(
            "SELECT environment_id FROM host_environment WHERE id=1",
            [],
            |row| row.get(0),
        )?;
        if uuid::Uuid::parse_str(&id)
            .ok()
            .map(|value| value.to_string())
            .as_deref()
            != Some(&id)
        {
            return Err(StoreError::Invalid(
                "host environment identity is invalid".into(),
            ));
        }
        tx.commit()?;
        Ok(id)
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
        self.lock()?.execute(
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
