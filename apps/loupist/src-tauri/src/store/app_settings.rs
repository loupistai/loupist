use serde::{Deserialize, Serialize};

use super::{SqliteStore, StoreResult};
use rusqlite::{Connection, OptionalExtension};

pub(crate) const CURRENT_NOTICE_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct AppSettings {
    pub notice_acknowledged_version: u32,
}

const KEEP_AWAKE_COLUMN: &str = "keep_awake INTEGER NOT NULL DEFAULT 0 CHECK(keep_awake IN (0, 1))";

// Host settings live outside Loupist content, like the host environment table.
fn initialize(connection: &Connection) -> StoreResult<()> {
    connection.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS application_settings (
            id INTEGER PRIMARY KEY CHECK(id = 1),
            notice_acknowledged_version INTEGER NOT NULL DEFAULT 0
                CHECK(notice_acknowledged_version BETWEEN 0 AND 4294967295),
            {KEEP_AWAKE_COLUMN}
         ) STRICT;"
    ))?;
    // A table created before keep-awake existed gains the column, off.
    let has_keep_awake = connection
        .prepare(
            "SELECT 1 FROM pragma_table_info('application_settings') WHERE name = 'keep_awake'",
        )?
        .exists([])?;
    if !has_keep_awake {
        connection.execute_batch(&format!(
            "ALTER TABLE application_settings ADD COLUMN {KEEP_AWAKE_COLUMN};"
        ))?;
    }
    Ok(())
}

fn read(connection: &Connection) -> StoreResult<AppSettings> {
    Ok(connection
        .query_row(
            "SELECT notice_acknowledged_version FROM application_settings WHERE id = 1",
            [],
            |row| {
                Ok(AppSettings {
                    notice_acknowledged_version: row.get(0)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

impl SqliteStore {
    pub(crate) fn notice_settings(&self) -> StoreResult<AppSettings> {
        let connection = self.lock()?;
        initialize(&connection)?;
        read(&connection)
    }

    pub(crate) fn acknowledge_notice(&self) -> StoreResult<AppSettings> {
        let connection = self.lock()?;
        initialize(&connection)?;
        connection.execute(
            "INSERT INTO application_settings (id, notice_acknowledged_version) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET notice_acknowledged_version = MAX(notice_acknowledged_version, excluded.notice_acknowledged_version)",
            [CURRENT_NOTICE_VERSION],
        )?;
        read(&connection)
    }

    /// Whether World processing keeps this computer awake (every Loupist).
    pub(crate) fn keep_awake_enabled(&self) -> StoreResult<bool> {
        let connection = self.lock()?;
        initialize(&connection)?;
        Ok(connection
            .query_row(
                "SELECT keep_awake FROM application_settings WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }

    pub(crate) fn set_keep_awake(&self, enabled: bool) -> StoreResult<()> {
        let connection = self.lock()?;
        initialize(&connection)?;
        connection.execute(
            "INSERT INTO application_settings (id, keep_awake) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET keep_awake = excluded.keep_awake",
            [enabled],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_acknowledgment_survives_reopen_and_never_lowers_a_newer_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notice.sqlite3");
        let store = SqliteStore::open(&path).unwrap();
        let version =
            |store: &SqliteStore| store.notice_settings().unwrap().notice_acknowledged_version;
        assert_eq!(version(&store), 0);
        assert_eq!(
            store
                .acknowledge_notice()
                .unwrap()
                .notice_acknowledged_version,
            1
        );
        drop(store);
        let reopened = SqliteStore::open(&path).unwrap();
        assert_eq!(version(&reopened), 1);
        reopened
            .lock()
            .unwrap()
            .execute(
                "UPDATE application_settings SET notice_acknowledged_version = 2",
                [],
            )
            .unwrap();
        assert_eq!(
            reopened
                .acknowledge_notice()
                .unwrap()
                .notice_acknowledged_version,
            2
        );
    }

    #[test]
    fn keep_awake_survives_reopen_without_touching_the_notice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keep-awake.sqlite3");
        let store = SqliteStore::open(&path).unwrap();
        assert!(!store.keep_awake_enabled().unwrap());
        store.acknowledge_notice().unwrap();
        store.set_keep_awake(true).unwrap();
        drop(store);
        let reopened = SqliteStore::open(&path).unwrap();
        assert!(reopened.keep_awake_enabled().unwrap());
        assert_eq!(
            reopened
                .notice_settings()
                .unwrap()
                .notice_acknowledged_version,
            CURRENT_NOTICE_VERSION
        );
        reopened.set_keep_awake(false).unwrap();
        assert!(!reopened.keep_awake_enabled().unwrap());
    }

    #[test]
    fn a_settings_table_from_before_keep_awake_gains_the_column_off() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(dir.path().join("upgrade.sqlite3")).unwrap();
        store
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TABLE application_settings (
                    id INTEGER PRIMARY KEY CHECK(id = 1),
                    notice_acknowledged_version INTEGER NOT NULL DEFAULT 0
                        CHECK(notice_acknowledged_version BETWEEN 0 AND 4294967295)
                 ) STRICT;
                 INSERT INTO application_settings (id, notice_acknowledged_version) VALUES (1, 1);",
            )
            .unwrap();
        assert!(!store.keep_awake_enabled().unwrap());
        assert_eq!(
            store.notice_settings().unwrap().notice_acknowledged_version,
            1
        );
        store.set_keep_awake(true).unwrap();
        assert!(store.keep_awake_enabled().unwrap());
        assert!(store
            .lock()
            .unwrap()
            .execute("UPDATE application_settings SET keep_awake = 2", [])
            .is_err());
    }
}
