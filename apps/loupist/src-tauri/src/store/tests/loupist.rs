use super::support::*;
use super::*;

#[test]
fn catalog_reconnection_rejects_invalid_identity_and_nonmonotonic_updates() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open_catalog(dir.path().join("catalog.sqlite3")).unwrap();
    let mut expected = loupist();
    expected.updated_at_ms = 10;
    store.refresh_folder_registration(&expected).unwrap();
    for change in ["same_time", "older_time", "different_id", "invalid_profile"] {
        let mut updated = expected.clone();
        updated.updated_at_ms += 1;
        match change {
            "same_time" => updated.updated_at_ms = expected.updated_at_ms,
            "older_time" => updated.updated_at_ms = expected.updated_at_ms - 1,
            "different_id" => updated.id = "loupist-other".into(),
            _ => updated.workspace_root = "relative/folder".into(),
        }
        let error = store
            .reconnect_loupist_workspace(&expected, &updated)
            .unwrap_err();
        match error {
            StoreError::Invalid(message) if change == "invalid_profile" => {
                assert!(message.contains("workspace"))
            }
            StoreError::Invalid(message) => {
                assert_eq!(message, "workspace reconnection is invalid", "{change}")
            }
            error => panic!("unexpected {change} error: {error}"),
        }
        assert_eq!(
            store.get_loupist(&expected.id).unwrap().as_ref(),
            Some(&expected)
        );
    }
}

#[test]
fn missing_folder_identities_do_not_collide_during_catalog_reconnection() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open_catalog(dir.path().join("catalog.sqlite3")).unwrap();
    let mut expected = loupist();
    expected.root_filesystem_identity = None;
    let mut other = loupist_with_root("loupist-other", 33, 44);
    other.root_filesystem_identity = None;
    store.refresh_folder_registration(&expected).unwrap();
    store.refresh_folder_registration(&other).unwrap();
    let mut updated = expected.clone();
    updated.workspace_root = dir.path().join("missing-folder").to_str().unwrap().into();
    updated.updated_at_ms += 1;
    let error = store
        .reconnect_loupist_workspace(&expected, &updated)
        .unwrap_err();
    assert!(
        !matches!(error, StoreError::Conflict("Folder is already registered")),
        "{error}"
    );
    assert_eq!(
        store.get_loupist(&expected.id).unwrap().as_ref(),
        Some(&expected)
    );
}

#[test]
fn profile_round_trips() {
    let store = SqliteStore::open_in_memory().unwrap();
    let profile = loupist();
    assert!(matches!(
        store.save_loupist(&profile),
        Err(StoreError::Conflict(_))
    ));
    seed_loupist(&store, &profile);
    assert_eq!(store.get_loupist(&profile.id).unwrap(), Some(profile));
}

#[test]
fn disconnected_workspace_metadata_round_trips_across_platforms() {
    for path in [
        "/Users/research/workspace",
        r"C:\Users\research\workspace",
        r"\\server\share\workspace",
    ] {
        let store = SqliteStore::open_in_memory().unwrap();
        let mut profile = loupist();
        profile.workspace_root = path.into();
        profile.root_filesystem_identity = None;
        seed_loupist(&store, &profile);
        assert_eq!(
            crate::domain::workspace_folder_name(&profile.workspace_root),
            "workspace"
        );
        assert_eq!(store.get_loupist(&profile.id).unwrap(), Some(profile));
    }
    for path in ["relative/workspace", r"C:workspace", ""] {
        let mut profile = loupist();
        profile.workspace_root = path.into();
        assert!(profile.validate().is_err(), "accepted relative path {path}");
    }
}

#[test]
fn deleting_a_loupist_removes_every_dependent_row_in_one_transaction() {
    let store = SqliteStore::open_in_memory().unwrap();
    let profile = loupist();
    seed_loupist(&store, &profile);
    let chat = crate::test_fixtures::chat_session("chat-delete", &profile.id);
    store.create_chat(&chat).unwrap();
    store.delete_loupist(&profile).unwrap();
    assert!(store.delete_loupist(&profile).is_err());
    assert_eq!(store.get_loupist("loupist-1").unwrap(), None);
    let connection = store.lock().unwrap();
    for table in [
        "loupist_profiles",
        "loupist_capability_bindings",
        "chat_sessions",
    ] {
        let count = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        assert_eq!(count, 0, "table {table} retained a deleted Loupist row");
    }
}

#[test]
fn capability_bindings_are_seeded_and_isolated_per_loupist() {
    let store = SqliteStore::open_in_memory().unwrap();
    let first = loupist_with_root("loupist-a", 11, 22);
    let second = loupist_with_root("loupist-b", 33, 44);
    seed_loupist(&store, &first);
    seed_loupist(&store, &second);

    let first_defaults = store.list_loupist_capabilities("loupist-a").unwrap();
    assert_eq!(
        first_defaults.len(),
        crate::domain::default_loupist_capability_bindings("loupist-a", 1).len()
    );
    assert!(first_defaults
        .iter()
        .filter(|binding| !binding.enabled)
        .all(|binding| binding.granted_permissions.is_empty()));
    let mut disabled = store
        .get_loupist_capability("loupist-a", "openbb.platform")
        .unwrap()
        .unwrap();
    disabled.enabled = false;
    disabled.granted_permissions.clear();
    disabled.updated_at_ms += 1;
    store.save_loupist_capability(&disabled).unwrap();

    assert!(
        !store
            .get_loupist_capability("loupist-a", "openbb.platform")
            .unwrap()
            .unwrap()
            .enabled
    );
    assert!(
        store
            .get_loupist_capability("loupist-b", "openbb.platform")
            .unwrap()
            .unwrap()
            .enabled
    );
}

#[test]
fn same_loupist_can_update_while_cross_loupist_root_reuse_is_rejected() {
    let store = SqliteStore::open_in_memory().unwrap();
    let mut first = loupist_with_root("loupist-a", 11, 22);
    seed_loupist(&store, &first);
    first.name = "Updated Loupist".into();
    first.workspace_root = "/tmp/loupist-a-renamed".into();
    first.updated_at_ms = 2;
    store.save_loupist(&first).unwrap();
    assert_eq!(
        store.get_loupist("loupist-a").unwrap().as_ref(),
        Some(&first)
    );

    assert!(matches!(
        create_loupist(&store, &loupist_with_root("loupist-b", 11, 22)),
        Err(StoreError::Conflict(_))
    ));
    // The same path is rejected even when its inode changed or is unbound.
    let mut second = loupist_with_root("loupist-b", 33, 44);
    second.workspace_root = first.workspace_root.clone();
    assert!(matches!(
        store.create_loupist(&second),
        Err(StoreError::Conflict(_))
    ));
    second.root_filesystem_identity = None;
    assert!(matches!(
        store.create_loupist(&second),
        Err(StoreError::Conflict(_))
    ));
}

#[test]
fn concurrent_connections_atomically_admit_only_one_loupist_per_root() {
    use std::sync::{Arc, Barrier};

    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("loupist.sqlite3");
    drop(SqliteStore::open(&path).unwrap());
    let barrier = Arc::new(Barrier::new(3));
    let handles = ["loupist-a", "loupist-b"].map(|id| {
        let path = path.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            let store = SqliteStore::open(path).unwrap();
            barrier.wait();
            let profile = loupist_with_root(id, 101, 202);
            store.create_loupist(&profile)
        })
    });
    barrier.wait();
    let results = handles.map(|handle| handle.join().unwrap());
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(StoreError::Conflict(_))))
            .count(),
        1
    );
}
