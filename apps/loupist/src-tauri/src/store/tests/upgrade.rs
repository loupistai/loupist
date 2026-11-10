//! The upgrade ladder from the frozen baseline (store 4, Knowledge 6).
use super::*;
use crate::store::upgrade::{normalize_sql, BASELINE_FIXTURE};
use crate::store::STORE_SCHEMA_VERSION;
use rusqlite::{types::Value, Connection};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const SEED: &str = "
INSERT INTO loupist_profiles VALUES ('loupist-a', 1, '/research/a', NULL, NULL, '{}');
INSERT INTO chat_sessions VALUES ('chat-a', 'loupist-a', 2, '{\"messages\":[]}');
INSERT INTO chat_artifacts VALUES ('artifact-a', 'chat-a', 'chart', 'Margins', 1, 3, 3, '{}', '{}');
INSERT INTO chart_artifact_datasets VALUES ('dataset-a', 'artifact-a', 'digest', '{}');
INSERT INTO chat_handoffs VALUES ('handoff-a', 'loupist-a', 'request', 'chat-a', 'running', 'run-a', 4, '{}');
INSERT INTO user_wallet_addresses VALUES ('wallet-a', 'evm', '0xabc', 'Main', 5, 6, 6, 'ok', 0);
INSERT INTO user_holdings_snapshots VALUES ('snapshot-a', 'wallet', 'wallet-a', 7, 'USD', NULL);
INSERT INTO user_holdings_snapshots VALUES ('snapshot-b', 'csv', NULL, 8, 'USD', 'broker');
INSERT INTO user_holdings_positions VALUES ('snapshot-a', 'ETH', 'CRYPTO', 'Ether', '2', NULL, NULL, 'USD');
INSERT INTO app_settings VALUES ('model', '{}');
INSERT INTO app_settings VALUES ('world_processing:loupist-a', '{\"paused\":true}');
INSERT INTO knowledge_growth_events (event_id, loupist_id, at_ms, kind, actor, summary)
    VALUES ('gev-b', 'loupist-a', 9, 'claim_added', 'processing', 'Added a claim');
INSERT INTO knowledge_growth_events (event_id, loupist_id, at_ms, kind, actor, summary)
    VALUES ('gev-a', 'loupist-a', 10, 'lens_created', 'user', 'Created a method');
INSERT INTO knowledge_records VALUES ('lens-a', 'lens', 'loupist-a', 'Method', 11);
INSERT INTO knowledge_lens_metadata VALUES ('lens-a', 'loupist-a', 'Method', 'method', 'perspective', NULL, NULL, 0, 12);
INSERT INTO knowledge_lens_log (op_id, loupist_id, record_id, op_json, outcome, origin, created_at_ms)
    VALUES ('op-a', 'loupist-a', 'lens-a', '{}', 'added', 'turn', 13);
INSERT INTO knowledge_lens_anchors (loupist_id, record_id, role, name_fold, created_at_ms)
    VALUES ('loupist-a', 'lens-a', 'mention', 'zeta', 14);
INSERT INTO knowledge_lens_anchors (loupist_id, record_id, role, name_fold, created_at_ms)
    VALUES ('loupist-a', 'lens-a', 'mention', 'alpha', 15);
INSERT INTO knowledge_connections VALUES ('connection-a', 'loupist-a', 'n1', 'n2', '[]', 3, 'shown', 16, 16, 16);
INSERT INTO knowledge_learning_reviews (review_id, loupist_id, world_job_id, status, created_at_ms,
    through_event, settings_json, call_budget)
    VALUES ('review-a', 'loupist-a', 'job-a', 'running', 17, 1, '{}', 1);
INSERT INTO knowledge_watch_events (event_id, loupist_id, asset_id, kind, natural_key, fact_date,
    facts_json, evidence_json, detected_at_ms, delivered_in)
    VALUES ('watch-a', 'loupist-a', 'asset-a', 'depeg', 'key', '2026-11-01', '{}', '[]', 18, 'morning');
INSERT INTO knowledge_earnings_notices VALUES ('loupist-a', 'notice-a', 'event-a', 'reported', 'after', 19, NULL);
";

fn baseline_database(directory: &Path, fixture: &str) -> PathBuf {
    let path = directory.join("loupist.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch(fixture).unwrap();
    connection.execute_batch(SEED).unwrap();
    path
}

fn versions(connection: &Connection) -> (i64, i64) {
    (
        connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap(),
        connection
            .query_row("SELECT version FROM knowledge_schema", [], |row| row.get(0))
            .unwrap(),
    )
}

/// Every row of every table, by rowid, except the version row itself.
fn data(connection: &Connection) -> BTreeMap<String, Vec<Vec<Value>>> {
    let mut rows = rows(connection);
    rows.remove("knowledge_schema");
    rows
}

/// Every row of every table, by rowid.
fn rows(connection: &Connection) -> BTreeMap<String, Vec<Vec<Value>>> {
    let tables = {
        let mut statement = connection
            .prepare(
                "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            )
            .unwrap();
        let names = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        names
    };
    tables
        .into_iter()
        .map(|table| {
            let mut statement = connection
                .prepare(&format!("SELECT rowid, * FROM \"{table}\" ORDER BY rowid"))
                .unwrap();
            let width = statement.column_count();
            let values = statement
                .query_map([], |row| (0..width).map(|index| row.get(index)).collect())
                .unwrap()
                .collect::<Result<Vec<Vec<Value>>, _>>()
                .unwrap();
            (table, values)
        })
        .collect()
}

/// Every schema object with its whitespace-, case- and quote-normalized SQL.
fn shape(connection: &Connection) -> BTreeMap<(String, String), Option<String>> {
    let mut statement = connection
        .prepare("SELECT type, name, sql FROM sqlite_schema")
        .unwrap();
    let shape = statement
        .query_map([], |row| {
            Ok((
                (row.get(0)?, row.get(1)?),
                row.get::<_, Option<String>>(2)?
                    .map(|sql| normalize_sql(&sql)),
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    shape
}

fn backups(directory: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(directory)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?;
            name.starts_with("loupist.sqlite3.upgrade-backup-")
                .then_some(path.clone())
        })
        .collect()
}

/// A backup an earlier upgrade left, named for `suffix` (its stamp and any
/// same-second attempt).
fn prior_backup(directory: &Path, suffix: &str) -> PathBuf {
    let path = directory.join(format!("loupist.sqlite3.upgrade-backup-{suffix}"));
    std::fs::write(&path, suffix).unwrap();
    path
}

/// The backup this upgrade wrote: the only one stamped in the present.
fn new_backup(directory: &Path) -> PathBuf {
    let year = chrono::Utc::now().format("%Y").to_string();
    let [backup] = backups(directory)
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(&format!("-backup-{year}")))
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    backup
}

/// The baseline with a shape the store step does not expect in its fifth
/// table, after it has rebuilt four: the whole step must roll back.
fn unexpected_baseline() -> String {
    let unexpected = BASELINE_FIXTURE.replacen(
        "status TEXT NOT NULL CHECK (status IN ('created','running','done','failed','stopped')),",
        "status TEXT NOT NULL,",
        1,
    );
    assert_ne!(unexpected, BASELINE_FIXTURE);
    unexpected
}

#[test]
fn a_baseline_database_upgrades_to_the_fresh_schema_with_its_data_intact() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = baseline_database(&directory, BASELINE_FIXTURE);
    let before = rows(&Connection::open(&path).unwrap());
    let data_before = data(&Connection::open(&path).unwrap());

    drop(SqliteStore::open(&path).unwrap());

    let upgraded = Connection::open(&path).unwrap();
    assert_eq!(
        versions(&upgraded),
        (
            STORE_SCHEMA_VERSION,
            crate::knowledge::KNOWLEDGE_SCHEMA_VERSION
        )
    );
    assert_eq!(data(&upgraded), data_before, "rows and rowids are kept");
    // An upgraded install has exactly the shape a fresh install has.
    let fresh = SqliteStore::open_in_memory().unwrap();
    assert_eq!(shape(&upgraded), shape(&fresh.lock().unwrap()));
    // Immutability triggers, partial indexes and references were rebuilt.
    let immutable = upgraded
        .execute("UPDATE knowledge_growth_events SET summary = 'changed'", [])
        .unwrap_err();
    assert!(immutable.to_string().contains("immutable"), "{immutable}");
    assert!(upgraded
        .execute(
            "INSERT INTO knowledge_records VALUES ('lens-b', 'lens', 'loupist-a', 'Other', 20)",
            [],
        )
        .is_ok());
    assert!(upgraded
        .execute(
            "INSERT INTO knowledge_lens_metadata VALUES ('lens-b', 'loupist-a', 'Other', 'other', 'perspective', NULL, NULL, 0, 20)",
            [],
        )
        .is_err());
    upgraded.execute_batch("PRAGMA foreign_keys = ON").unwrap();
    assert!(upgraded
        .execute(
            "INSERT INTO chart_artifact_datasets VALUES ('dataset-x', 'missing', 'digest', '{}')",
            [],
        )
        .is_err());
    // Extensible enumerations are Rust's to validate now; structure stays SQL's.
    upgraded
        .execute(
            "INSERT INTO chat_artifacts VALUES ('artifact-b', 'chat-a', 'table', 'Grid', 1, 3, 3, '{}', '{}')",
            [],
        )
        .unwrap();
    assert!(upgraded
        .execute(
            "INSERT INTO user_holdings_snapshots VALUES ('snapshot-c', 'wallet', NULL, 9, 'USD', NULL)",
            [],
        )
        .is_err());
    // The backup is the untouched baseline, owner-only, beside the database.
    let [backup] = backups(&directory).try_into().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let backup = Connection::open(&backup).unwrap();
    assert_eq!(versions(&backup), (4, 6));
    assert_eq!(rows(&backup), before);
    // Opening the current database again writes no further backup.
    drop(SqliteStore::open(&path).unwrap());
    assert_eq!(backups(&directory).len(), 1);
}

#[test]
fn the_upgrade_restores_a_session_cache_key_an_earlier_reconnect_removed() {
    // Before store 5 a reconnect cleared caches with `json_remove`, and the
    // Chat's text compare-and-swap then refused every later write.
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = baseline_database(&directory, BASELINE_FIXTURE);
    let chat = crate::test_fixtures::chat_session("chat-cleared", "loupist-a");
    let written = serde_json::to_string(&chat).unwrap();
    assert!(
        written.contains(r#""piSessionCache":null,"title":"#),
        "{written}"
    );
    Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO chat_sessions VALUES (?1, ?2, ?3, json_remove(?4, '$.piSessionCache'))",
            rusqlite::params![chat.id, chat.loupist_id, chat.updated_at_ms, written],
        )
        .unwrap();

    let store = SqliteStore::open(&path).unwrap();
    let stored: String = store
        .lock()
        .unwrap()
        .query_row(
            "SELECT data_json FROM chat_sessions WHERE id = 'chat-cleared'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        stored, written,
        "the key is back where serialization writes it"
    );
    let current = store.get_chat(&chat.id).unwrap().unwrap();
    let renamed = crate::domain::ChatSession {
        title: "Renamed".into(),
        updated_at_ms: current.updated_at_ms + 1,
        ..current.clone()
    };
    store.replace_chat(&current, &renamed).unwrap();
    // The baseline seed's minimal row has no `title` and is left as it was.
    let seed: String = store
        .lock()
        .unwrap()
        .query_row(
            "SELECT data_json FROM chat_sessions WHERE id = 'chat-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(seed, r#"{"messages":[]}"#);
}

#[test]
fn the_upgrade_gives_an_earlier_settings_table_keep_awake_off() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = baseline_database(&directory, BASELINE_FIXTURE);
    Connection::open(&path)
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

    let store = SqliteStore::open(&path).unwrap();
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

#[test]
fn a_failed_step_leaves_the_database_unchanged_beside_its_backup() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = baseline_database(&directory, &unexpected_baseline());
    let original = Connection::open(&path).unwrap();
    let (before_rows, before_shape) = (rows(&original), shape(&original));
    drop(original);

    let error = SqliteStore::open(&path).err().expect("the upgrade fails");
    assert!(
        matches!(&error, StoreError::Upgrade(message) if message.contains("chat_handoffs")),
        "{error}"
    );

    let unchanged = Connection::open(&path).unwrap();
    assert_eq!(versions(&unchanged), (4, 6));
    assert_eq!(rows(&unchanged), before_rows);
    assert_eq!(shape(&unchanged), before_shape);
    let [backup] = backups(&directory).try_into().unwrap();
    assert!(error.to_string().contains(backup.to_str().unwrap()));
    assert_eq!(rows(&Connection::open(&backup).unwrap()), before_rows);
}

#[test]
fn a_successful_upgrade_keeps_its_backup_and_the_newest_prior_one() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = baseline_database(&directory, BASELINE_FIXTURE);
    // A same-second attempt orders by its number, not its spelling.
    let oldest = prior_backup(&directory, "20010101T000000Z");
    let older = prior_backup(&directory, "20010102T000000Z-2");
    let newest = prior_backup(&directory, "20010102T000000Z-10");

    drop(SqliteStore::open(&path).unwrap());

    let backup = new_backup(&directory);
    let mut kept = backups(&directory);
    kept.sort();
    let mut expected = vec![newest.clone(), backup.clone()];
    expected.sort();
    assert_eq!(kept, expected);
    assert_eq!(kept.len(), crate::store::upgrade::KEPT_UPGRADE_BACKUPS);
    assert!(!oldest.exists() && !older.exists());
    assert_eq!(std::fs::read(&newest).unwrap(), b"20010102T000000Z-10");
    assert_eq!(versions(&Connection::open(&backup).unwrap()), (4, 6));
}

#[cfg(unix)]
#[test]
fn pruning_never_touches_a_symlink_or_a_name_it_did_not_write() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = baseline_database(&directory, BASELINE_FIXTURE);
    // Newer than every candidate below, so any of them counted as a backup
    // would be the one deleted.
    let prior = prior_backup(&directory, "20010101T000000Z");
    let outside = temporary.path().join("outside.sqlite3");
    std::fs::write(&outside, "outside").unwrap();
    let link = directory.join("loupist.sqlite3.upgrade-backup-20000101T000000Z");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let folder = directory.join("loupist.sqlite3.upgrade-backup-20000102T000000Z");
    std::fs::create_dir(&folder).unwrap();
    let nested_directory = directory.join("nested");
    std::fs::create_dir(&nested_directory).unwrap();
    let nested = prior_backup(&nested_directory, "20000103T000000Z");
    let untouched = [
        "loupist.sqlite3.upgrade-backup-2000",
        "loupist.sqlite3.upgrade-backup-20000101T000000",
        "loupist.sqlite3.upgrade-backup-20000101-000000Z",
        "loupist.sqlite3.upgrade-backup-20000101T000000Z.old",
        "loupist.sqlite3.upgrade-backup-20000101T000000Z-journal",
        "loupist.sqlite3.upgrade-backup-20000101T000000Z-",
        "loupist.sqlite3.upgrade-backup-20000101T000000Z-0",
        "loupist.sqlite3.upgrade-backup-20000101T000000Z-01",
        "loupist.sqlite3.upgrade-backup-20000101T000000Z-101",
        "loupist.sqlite3.upgrade-backup-20000101T000000Z-1-1",
        "other.sqlite3.upgrade-backup-20000101T000000Z",
        "xloupist.sqlite3.upgrade-backup-20000101T000000Z",
        // A distinct date: on a case-insensitive volume this spelling would
        // name the symlink above.
        "loupist.sqlite3.upgrade-backup-20000104t000000z",
    ]
    .map(|name| {
        let path = directory.join(name);
        std::fs::write(&path, name).unwrap();
        path
    });

    drop(SqliteStore::open(&path).unwrap());

    new_backup(&directory);
    assert_eq!(std::fs::read(&prior).unwrap(), b"20010101T000000Z");
    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
    assert!(folder.is_dir());
    assert!(nested.is_file());
    for path in untouched {
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        assert_eq!(std::fs::read(&path).unwrap(), name.as_bytes(), "{name}");
    }
}

#[test]
fn a_failed_step_prunes_no_backup() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = baseline_database(&directory, &unexpected_baseline());
    let priors = ["20010101T000000Z", "20010102T000000Z", "20010103T000000Z"]
        .map(|stamp| prior_backup(&directory, stamp));

    assert!(matches!(
        SqliteStore::open(&path),
        Err(StoreError::Upgrade(_))
    ));

    new_backup(&directory);
    assert_eq!(backups(&directory).len(), priors.len() + 1);
    for prior in priors {
        assert!(prior.is_file(), "{}", prior.display());
    }
}

#[test]
fn newer_data_is_refused_and_left_untouched() {
    for newer in [
        format!("PRAGMA user_version = {};", STORE_SCHEMA_VERSION + 1),
        format!(
            "UPDATE knowledge_schema SET version = {};",
            crate::knowledge::KNOWLEDGE_SCHEMA_VERSION + 1
        ),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("loupist.sqlite3");
        drop(SqliteStore::open(&path).unwrap());
        Connection::open(&path)
            .unwrap()
            .execute_batch(&newer)
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            matches!(SqliteStore::open(&path), Err(StoreError::NewerVersion)),
            "{newer}"
        );
        assert!(
            matches!(
                SqliteStore::open_catalog(&path),
                Err(StoreError::NewerVersion)
            ),
            "{newer}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "{newer}");
        assert!(backups(temporary.path()).is_empty());
    }
}

#[test]
fn a_restored_baseline_round_trips_through_the_ladder() {
    // The folder tests rely on this test helper reproducing the baseline.
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("loupist.sqlite3");
    drop(SqliteStore::open(&path).unwrap());
    let mut connection = Connection::open(&path).unwrap();
    crate::store::upgrade::restore_baseline_for_test(&mut connection);
    let baseline = Connection::open_in_memory().unwrap();
    baseline.execute_batch(BASELINE_FIXTURE).unwrap();
    assert_eq!(shape(&connection), shape(&baseline));
}
