use super::support::*;
use super::*;
use crate::fs::artifact_trust::ArtifactTrustError;
use crate::store::STORE_SCHEMA_VERSION;
use rusqlite::Connection;

#[test]
fn linked_sqlite_includes_the_crafted_wal_memory_disclosure_fix() {
    let version = rusqlite::version();
    let parts: Vec<u32> = version
        .split('.')
        .map(|part| part.parse().unwrap())
        .collect();
    assert!(
        parts.as_slice() >= [3, 54, 0].as_slice(),
        "SQLite {version} must be at least 3.54.0 (RUSTSEC-2026-0459)"
    );
}

#[cfg(unix)]
#[test]
fn file_backed_store_hardens_database_and_rejects_side_symlinks() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
    let database = directory.join("loupist.sqlite3");
    let store = SqliteStore::open(&database).unwrap();
    seed_loupist(&store, &loupist());
    assert_eq!(
        std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&database).unwrap().permissions().mode() & 0o777,
        0o600
    );
    for suffix in ["-wal", "-shm"] {
        let side = sqlite_side_path(&database, suffix);
        assert!(side.is_file());
        assert_eq!(
            std::fs::metadata(side).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    drop(store);
    for suffix in ["-wal", "-shm", "-journal"] {
        let side = sqlite_side_path(&database, suffix);
        if side.exists() {
            assert_eq!(
                std::fs::metadata(side).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    let outside = temporary.path().join("outside");
    std::fs::write(&outside, b"outside").unwrap();
    let linked_database = directory.join("linked.sqlite3");
    symlink(&outside, &linked_database).unwrap();
    assert!(matches!(
        SqliteStore::open(&linked_database),
        Err(StoreError::PrivateStorage(ArtifactTrustError::Untrusted))
    ));

    let side_database = directory.join("side.sqlite3");
    std::fs::write(&side_database, []).unwrap();
    symlink(&outside, sqlite_side_path(&side_database, "-wal")).unwrap();
    assert!(matches!(
        SqliteStore::open(&side_database),
        Err(StoreError::PrivateStorage(ArtifactTrustError::Untrusted))
    ));
}

#[test]
fn foreign_or_unsupported_databases_are_rejected_without_mutation() {
    let temporary = tempfile::tempdir().unwrap();
    let sentinel = "CREATE TABLE sentinel (value TEXT NOT NULL) STRICT;\
                    INSERT INTO sentinel (value) VALUES ('preserve-me');";
    let mut cases = [2, 18, STORE_SCHEMA_VERSION + 1]
        .map(|version| format!("{sentinel} PRAGMA user_version = {version};"))
        .to_vec();
    cases.push(format!("{sentinel} PRAGMA user_version = 0;"));
    for (version, application) in [
        (STORE_SCHEMA_VERSION, 0),
        (0, 1234),
        (STORE_SCHEMA_VERSION, 1234),
    ] {
        cases.push(format!(
            "PRAGMA user_version={version}; PRAGMA application_id={application};"
        ));
    }
    for (index, setup) in cases.iter().enumerate() {
        let path = temporary.path().join(format!("foreign-{index}.sqlite3"));
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(setup).unwrap();
        drop(connection);
        let before = std::fs::read(&path).unwrap();
        assert!(SqliteStore::open(&path).is_err(), "{setup}");
        assert_eq!(std::fs::read(&path).unwrap(), before, "{setup}");
        assert!(!sqlite_side_path(&path, "-wal").exists(), "{setup}");
    }
}

#[test]
fn current_schema_is_reopened_without_replacement() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("loupist.sqlite3");
    let store = SqliteStore::open(&path).unwrap();
    let profile = loupist();
    seed_loupist(&store, &profile);
    let chat = crate::test_fixtures::chat_session("chat-preserved", &profile.id);
    store.create_chat(&chat).unwrap();
    drop(store);

    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.get_loupist(&profile.id).unwrap(), Some(profile));
    assert_eq!(store.get_chat(&chat.id).unwrap(), Some(chat));
}

#[test]
fn a_folder_store_from_an_earlier_build_is_refused_as_an_earlier_version() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("earlier.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(&format!(
            "CREATE TABLE sentinel (value TEXT NOT NULL) STRICT; PRAGMA user_version = {}; PRAGMA application_id = {};",
            crate::store::STORE_BASELINE_VERSION - 1,
            crate::store::schema::STORE_APPLICATION_ID
        ))
        .unwrap();
    drop(connection);
    assert!(matches!(
        SqliteStore::open(&path),
        Err(StoreError::EarlierVersion)
    ));
}

#[test]
fn reopening_a_folder_in_delete_mode_keeps_a_concurrent_reader_working() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("loupist.sqlite3");
    drop(SqliteStore::open_folder(&path).unwrap());
    let reader = Connection::open(&path).unwrap();
    reader
        .execute_batch("BEGIN; SELECT count(*) FROM loupist_profiles;")
        .unwrap();
    let reopened = SqliteStore::open_folder(&path).unwrap();
    let journal: String = reopened
        .lock()
        .unwrap()
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal, "delete");
    reader.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn known_chat_ownership_does_not_open_other_registered_folders() {
    let root = tempfile::tempdir().unwrap();
    let store = SqliteStore::open_catalog(root.path().join("catalog.sqlite3")).unwrap();
    let mut profiles = Vec::new();
    for (index, id) in ["owner", "unrelated"].iter().enumerate() {
        let workspace = root.path().join(id);
        std::fs::create_dir(&workspace).unwrap();
        let mut profile = crate::test_fixtures::loupist_profile(id);
        profile.workspace_root = workspace.to_str().unwrap().into();
        profile.root_filesystem_identity =
            crate::fs::loupist_root::BoundLoupistRoot::open_unbound(workspace.clone())
                .unwrap()
                .identity();
        profile.updated_at_ms = index as i64 + 1;
        store.create_loupist(&profile).unwrap();
        profiles.push(profile);
    }
    let chat = crate::test_fixtures::chat_session("known-chat", &profiles[0].id);
    store.create_chat(&chat).unwrap();
    assert_eq!(store.folders.lock().unwrap().len(), 1);
    assert_eq!(store.get_chat(&chat.id).unwrap(), Some(chat));
    assert_eq!(store.folders.lock().unwrap().len(), 1);
    assert!(!store.folders.lock().unwrap().contains_key("unrelated"));
}
