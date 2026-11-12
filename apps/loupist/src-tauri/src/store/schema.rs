use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use super::upgrade::{self, STORE_BASELINE_VERSION};
use super::{SqliteStore, StoreError, StoreResult};
use crate::fs::artifact_trust::{
    ensure_private_directory, ensure_private_regular_file, harden_private_regular_file_if_exists,
    ArtifactTrustError,
};

// Fresh databases are created at the current versions; `upgrade` brings a
// database from `STORE_BASELINE_VERSION` onward to them. Older development
// formats are refused with a typed state, and newer ones are left untouched.
// The SQLite application id distinguishes Loupist files from unrelated
// databases and discarded development formats.
pub(crate) const STORE_SCHEMA_VERSION: i64 = 6;
pub(crate) const STORE_APPLICATION_ID: i64 = 0x4755_5255; // "GURU": the id stored in every existing database, kept as is
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) fn sqlite_side_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn harden_sqlite_side_files(path: &Path) -> StoreResult<()> {
    for suffix in ["-wal", "-shm", "-journal"] {
        harden_private_regular_file_if_exists(&sqlite_side_path(path, suffix))?;
    }
    Ok(())
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        Self::open_as(path.as_ref(), false)
    }

    /// Opens a Loupist folder's database for use. A folder whose Memory schema
    /// is a pre-ladder format is refused before anything is written, so it is
    /// never half-upgraded or half-working.
    pub(super) fn open_folder(path: impl AsRef<Path>) -> StoreResult<Self> {
        Self::open_as(path.as_ref(), true)
    }

    fn open_as(path: &Path, folder: bool) -> StoreResult<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| StoreError::Invalid("SQLite path has no parent".into()))?;
        ensure_private_directory(parent)?;
        let file_name = path
            .file_name()
            .ok_or_else(|| StoreError::Invalid("SQLite path has no file name".into()))?;
        // SQLite's NOFOLLOW flag rejects a symlink in any path component. Use
        // the already-opened app-data directory's canonical spelling so normal
        // platform aliases such as macOS `/var` -> `/private/var` do not make a
        // private database unusable, while the database leaf still cannot be a
        // symlink.
        let canonical_path = parent
            .canonicalize()
            .map_err(ArtifactTrustError::from)?
            .join(file_name);
        ensure_private_regular_file(&canonical_path)?;
        harden_sqlite_side_files(&canonical_path)?;
        let connection = Connection::open_with_flags(
            &canonical_path,
            OpenFlags::default() | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        let store = Self::from_connection(connection, Some(canonical_path.clone()), folder)?;
        ensure_private_regular_file(&canonical_path)?;
        harden_sqlite_side_files(&canonical_path)?;
        Ok(store)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> StoreResult<Self> {
        Self::from_connection(Connection::open_in_memory()?, None, false)
    }

    pub(super) fn from_connection(
        mut connection: Connection,
        database_path: Option<PathBuf>,
        folder: bool,
    ) -> StoreResult<Self> {
        connection.busy_timeout(SQLITE_BUSY_TIMEOUT)?;
        let version =
            connection.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))?;
        let application_id: i64 =
            connection.query_row("PRAGMA application_id", [], |row| row.get(0))?;
        match version {
            0 => {
                let has_schema = connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_schema LIMIT 1)",
                    [],
                    |row| row.get::<_, bool>(0),
                )?;
                if has_schema {
                    return Err(StoreError::UnversionedNonemptySchema);
                }
            }
            found if (STORE_BASELINE_VERSION..=STORE_SCHEMA_VERSION).contains(&found) => {}
            // Development formats before the ladder, with or without the id.
            found if (1..STORE_BASELINE_VERSION).contains(&found) => {
                return Err(StoreError::EarlierVersion)
            }
            found if found > STORE_SCHEMA_VERSION && application_id == STORE_APPLICATION_ID => {
                return Err(StoreError::NewerVersion)
            }
            _ => {
                return Err(StoreError::Invalid(
                    "database schema version is unsupported".into(),
                ))
            }
        }
        if (version == 0 && application_id != 0)
            || (version != 0 && application_id != STORE_APPLICATION_ID)
        {
            return Err(StoreError::Invalid(
                "database application identity is unsupported".into(),
            ));
        }
        if version != 0 {
            if folder && upgrade::standing(&connection)? == upgrade::Standing::Earlier {
                return Err(StoreError::EarlierVersion);
            }
            upgrade::upgrade(&mut connection, database_path.as_deref())?;
        }
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        // Folder databases are copyable single files. Opening one must never
        // switch DELETE → WAL → DELETE, which takes needless exclusive locks.
        let journal: String = connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        let expected_journal = if folder { "delete" } else { "wal" };
        if database_path.is_some() && !journal.eq_ignore_ascii_case(expected_journal) {
            connection.pragma_update(None, "journal_mode", expected_journal)?;
        }
        if folder {
            connection.execute_batch("PRAGMA synchronous = FULL;")?;
        }
        if version == 0 {
            Self::initialize_schema(&connection)?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
            user_context_previews: Mutex::new(std::collections::HashMap::new()),
            database_path,
            folder_catalog: false,
            folders: Mutex::new(std::collections::HashMap::new()),
        })
    }

    fn initialize_schema(connection: &Connection) -> StoreResult<()> {
        connection.execute_batch(
            r#"
            BEGIN IMMEDIATE;
            CREATE TABLE user_watchlist (
                symbol TEXT NOT NULL, market TEXT NOT NULL, display_name TEXT,
                added_at_ms INTEGER NOT NULL, PRIMARY KEY(symbol, market)
            ) STRICT;
            -- Read-only public addresses; balances live only in their wallet snapshots.
            CREATE TABLE user_wallet_addresses (
                id TEXT PRIMARY KEY NOT NULL,
                family TEXT NOT NULL,
                address TEXT NOT NULL,
                name TEXT,
                added_at_ms INTEGER NOT NULL,
                last_read_at_ms INTEGER,
                last_good_read_at_ms INTEGER,
                status TEXT NOT NULL,
                unlisted_count INTEGER NOT NULL DEFAULT 0 CHECK(unlisted_count >= 0),
                UNIQUE(family, address)
            ) STRICT;
            CREATE TABLE user_holdings_snapshots (
                snapshot_id TEXT PRIMARY KEY NOT NULL,
                source TEXT NOT NULL,
                wallet_id TEXT REFERENCES user_wallet_addresses(id) ON DELETE CASCADE,
                imported_at_ms INTEGER NOT NULL, currency TEXT NOT NULL, note TEXT,
                CHECK((source = 'wallet') = (wallet_id IS NOT NULL))
            ) STRICT;
            CREATE TABLE user_holdings_positions (
                snapshot_id TEXT NOT NULL REFERENCES user_holdings_snapshots(snapshot_id) ON DELETE CASCADE,
                symbol TEXT NOT NULL, market TEXT NOT NULL, display_name TEXT,
                quantity TEXT NOT NULL, cost_basis TEXT, market_value TEXT, currency TEXT NOT NULL,
                PRIMARY KEY(snapshot_id, symbol, market)
            ) STRICT;
            CREATE TABLE app_settings (
                id TEXT PRIMARY KEY NOT NULL,
                data_json TEXT NOT NULL
            ) STRICT;
            CREATE TABLE deletion_journals (
                id TEXT PRIMARY KEY NOT NULL,
                loupist_id TEXT NOT NULL,
                target_id TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                data_json TEXT NOT NULL,
                UNIQUE (loupist_id, target_id)
            ) STRICT;
            CREATE TABLE loupist_profiles (
                id TEXT PRIMARY KEY NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                workspace_root TEXT NOT NULL UNIQUE,
                root_device TEXT,
                root_inode TEXT,
                data_json TEXT NOT NULL,
                CHECK ((root_device IS NULL) = (root_inode IS NULL))
            ) STRICT;
            CREATE TABLE loupist_capability_bindings (
                loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE RESTRICT,
                entry_id TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                data_json TEXT NOT NULL,
                PRIMARY KEY (loupist_id, entry_id)
            ) STRICT;
            -- Message feedback is owned by each message in data_json; thread deletion removes it.
            CREATE TABLE chat_sessions (
                id TEXT PRIMARY KEY NOT NULL,
                loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE RESTRICT,
                updated_at_ms INTEGER NOT NULL,
                data_json TEXT NOT NULL
            ) STRICT;
            CREATE TABLE chat_artifacts (
                id TEXT PRIMARY KEY NOT NULL,
                chat_session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE RESTRICT,
                kind TEXT NOT NULL,
                title TEXT NOT NULL,
                current_revision INTEGER NOT NULL CHECK (current_revision > 0),
                created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
                updated_at_ms INTEGER NOT NULL,
                data_json TEXT NOT NULL,
                current_content_json TEXT NOT NULL
            ) STRICT;
            CREATE TABLE chart_artifact_datasets (
                id TEXT PRIMARY KEY NOT NULL,
                artifact_id TEXT NOT NULL UNIQUE REFERENCES chat_artifacts(id) ON DELETE RESTRICT,
                digest TEXT NOT NULL,
                data_json TEXT NOT NULL
            ) STRICT;
            CREATE INDEX deletion_journals_by_updated_at
                ON deletion_journals(updated_at_ms ASC, id ASC);
            CREATE INDEX chat_sessions_by_loupist
                ON chat_sessions(loupist_id, updated_at_ms DESC);
            CREATE UNIQUE INDEX loupist_profiles_by_root_identity
                ON loupist_profiles(root_device, root_inode)
                WHERE root_device IS NOT NULL AND root_inode IS NOT NULL;
            CREATE INDEX chat_artifacts_by_session
                ON chat_artifacts(chat_session_id, updated_at_ms DESC);
            "#,
        )?;
        connection.execute_batch(&crate::knowledge::embedded_ddl())?;
        connection.execute_batch(
            "CREATE INDEX knowledge_spans_by_id ON knowledge_source_spans(span_id, record_id);
             CREATE INDEX knowledge_representations_by_id
                 ON knowledge_source_representations(representation_id, record_id);",
        )?;
        connection.execute_batch(crate::datasets::embedded_ddl())?;
        connection.execute_batch(crate::scheduled_tasks::store::DDL)?;
        connection.execute_batch(crate::chat::handoff::DDL)?;
        connection.execute_batch(&format!(
            "PRAGMA application_id = {STORE_APPLICATION_ID}; PRAGMA user_version = {STORE_SCHEMA_VERSION}; COMMIT;"
        ))?;
        Ok(())
    }
}
