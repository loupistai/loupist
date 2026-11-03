use serde::{Deserialize, Serialize};

use super::{SqliteStore, StoreResult};
use rusqlite::{Connection, OptionalExtension};

pub(crate) const CURRENT_NOTICE_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct AppSettings {
    pub notice_acknowledged_version: u32,
}

// Host settings live outside Loupist content, like the host environment table.
fn initialize(connection: &Connection) -> StoreResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS application_settings (
            id INTEGER PRIMARY KEY CHECK(id = 1),
            notice_acknowledged_version INTEGER NOT NULL DEFAULT 0
                CHECK(notice_acknowledged_version BETWEEN 0 AND 4294967295)
         ) STRICT;",
    )?;
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
}
