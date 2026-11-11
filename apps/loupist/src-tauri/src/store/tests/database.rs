use super::support::*;
use super::*;
use crate::fs::artifact_trust::ArtifactTrustError;
use crate::store::STORE_SCHEMA_VERSION;
use rusqlite::Connection;

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
