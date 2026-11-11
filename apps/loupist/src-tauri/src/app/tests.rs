use super::*;

#[cfg(unix)]
#[tokio::test]
async fn shutdown_stops_support_sidecars_without_a_chat_registration() {
    use crate::process_lease::{confirm_process_group_exit, ChildProcessLease, ProcessKind};
    use std::time::Duration;

    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_persistent_test(temporary.path().join("app-data"));
    let program = Path::new("/bin/sleep").canonicalize().unwrap();
    let mut child = tokio::process::Command::new(&program)
        .arg("30")
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let group = child.id().unwrap() as i32;
    let _lease = ChildProcessLease::register(
        &state.artifacts.app_data_dir.join("process-leases"),
        ProcessKind::Pi,
        group,
        group,
        &program,
    )
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), state.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert!(
        confirm_process_group_exit(group, Duration::from_millis(100))
            .await
            .is_confirmed()
    );
    assert!(!child.wait().await.unwrap().success());
}

#[test]
fn development_build_isolated_from_installed_app_data() {
    let user_home = PathBuf::from("user-home");

    assert_eq!(
        app_data_dir_for_build(user_home.clone(), PRODUCTION_APP_IDENTIFIER),
        user_home.join(".loupist/development")
    );
    assert_eq!(
        app_data_dir_for_build(user_home.clone(), "com.monarchjuno.loupist.manual.research"),
        user_home.join(".loupist/profiles/com.monarchjuno.loupist.manual.research")
    );
}

#[tokio::test]
async fn restart_fixture_waits_for_final_instance_owner() {
    use std::time::Duration;

    let temporary = tempfile::tempdir().unwrap();
    let app_data = temporary.path().join("app-data");
    let state = AppState::for_test(app_data.clone());
    let finishing_turn = state.clone();
    let closing = state.close_after_turns_for_restart_test();
    tokio::pin!(closing);

    assert!(
        tokio::time::timeout(Duration::from_millis(20), closing.as_mut())
            .await
            .is_err()
    );
    assert!(is_app_instance_conflict(
        &AppInstanceLock::acquire(&app_data).err().unwrap()
    ));

    drop(finishing_turn);
    tokio::time::timeout(Duration::from_secs(1), closing)
        .await
        .expect("restart did not observe the final owner leaving");
    let _restarted = AppState::for_test(app_data);
}

#[test]
fn hidden_models_are_rejected_before_pi_execution() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app-data"));
    state.set_model_visible("fixture", false).unwrap();

    let error = state
        .pi_execution("fixture", "off", &std::collections::BTreeMap::new())
        .err()
        .unwrap();
    assert!(error.to_string().contains("hidden from Chat"));
}

#[cfg(feature = "e2e")]
#[test]
fn e2e_app_data_requires_an_explicit_absolute_root() {
    let absolute = std::env::current_dir().unwrap().join("isolated-e2e");

    assert!(e2e_app_data_dir(None).is_err());
    assert!(e2e_app_data_dir(Some(PathBuf::from("relative"))).is_err());
    assert_eq!(e2e_app_data_dir(Some(absolute.clone())).unwrap(), absolute);
}

#[cfg(unix)]
#[test]
fn app_data_paths_are_derived_from_one_canonical_root() {
    let temporary = tempfile::tempdir().unwrap();
    let actual_parent = temporary.path().join("actual");
    ensure_private_directory(&actual_parent).unwrap();
    let alias_parent = temporary.path().join("alias");
    std::os::unix::fs::symlink(&actual_parent, &alias_parent).unwrap();
    let requested = alias_parent.join("app-data");

    let state = AppState::for_test(requested.clone());

    assert_eq!(
        state.artifacts.app_data_dir,
        requested.canonicalize().unwrap()
    );
    assert!(state
        .artifacts
        .process_lease_dir
        .starts_with(&state.artifacts.app_data_dir));
    assert!(!state
        .artifacts
        .app_data_dir
        .join("finance-snapshots")
        .exists());
}

#[cfg(target_os = "macos")]
#[test]
fn transient_broker_socket_fits_macos_unix_path_limit() {
    use std::os::unix::ffi::OsStrExt;

    let socket = transient_broker_directory().join(format!("{}.sock", "0".repeat(32)));
    assert!(socket.starts_with("/tmp"));
    assert!(socket.as_os_str().as_bytes().len() < 104);
}

#[cfg(unix)]
#[test]
fn transient_broker_directory_is_scoped_to_the_effective_user() {
    assert_eq!(
        transient_broker_directory()
            .file_name()
            .unwrap()
            .to_string_lossy(),
        format!("loupist-{}", unsafe { libc::geteuid() })
    );
}

#[test]
fn app_data_is_owned_by_only_one_live_instance() {
    let temporary = tempfile::tempdir().unwrap();
    ensure_private_directory(temporary.path()).unwrap();
    let first = AppInstanceLock::acquire(temporary.path()).unwrap();
    let conflict = AppInstanceLock::acquire(temporary.path()).err().unwrap();
    assert!(is_app_instance_conflict(&conflict));
    drop(first);
    AppInstanceLock::acquire(temporary.path()).unwrap();
}

#[cfg(unix)]
#[test]
fn final_instance_owner_releases_lock_with_an_inherited_file_handle() {
    let temporary = tempfile::tempdir().unwrap();
    let first = AppInstanceLock::acquire(temporary.path()).unwrap();
    let inherited = first._file.try_clone().unwrap();
    drop(first);
    let second = AppInstanceLock::acquire(temporary.path()).unwrap();
    assert!(is_app_instance_conflict(
        &AppInstanceLock::acquire(temporary.path()).err().unwrap()
    ));
    drop(inherited);
    drop(second);
}

#[test]
fn clearing_memory_write_recovery_does_not_clear_deletion_quarantine() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().to_path_buf());
    state.quarantine_loupist("loupist-a", QuarantineSource::Deletion, "pending deletion");
    state.quarantine_loupist(
        "loupist-a",
        QuarantineSource::MemoryWrite,
        "interrupted memory write",
    );

    state.clear_loupist_quarantine("loupist-a", QuarantineSource::MemoryWrite);

    assert_eq!(state.loupist_access("loupist-a"), LoupistAccess::Hidden);
    assert_eq!(
        state
            .ensure_loupist_available("loupist-a")
            .unwrap_err()
            .code,
        "loupist_storage_unavailable"
    );
}

#[test]
fn loupist_recovery_admission_bypasses_only_its_authoritative_recovery_state() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().to_path_buf());
    let error = state
        .register_loupist_recovery(
            "recovery-none".into(),
            "loupist-a".into(),
            LoupistRecoveryAction::RecoverMemory,
        )
        .err()
        .unwrap();
    assert_eq!(error.code, "conflict");

    state.quarantine_loupist(
        "loupist-a",
        QuarantineSource::MemoryWrite,
        "interrupted memory write",
    );

    let recovery = state
        .register_loupist_recovery(
            "recovery-a".into(),
            "loupist-a".into(),
            LoupistRecoveryAction::RecoverMemory,
        )
        .unwrap();
    drop(recovery);
    assert_eq!(
        state
            .ensure_loupist_available("loupist-a")
            .unwrap_err()
            .code,
        "loupist_recovery_required"
    );

    state.quarantine_loupist("loupist-a", QuarantineSource::Deletion, "pending deletion");
    let error = state
        .register_loupist_recovery(
            "recovery-b".into(),
            "loupist-a".into(),
            LoupistRecoveryAction::RecoverMemory,
        )
        .unwrap_err();
    assert_eq!(error.code, "loupist_storage_unavailable");
}

#[cfg(windows)]
#[test]
fn live_instance_lock_cannot_be_replaced() {
    let temporary = tempfile::tempdir().unwrap();
    ensure_private_directory(temporary.path()).unwrap();
    let lock = AppInstanceLock::acquire(temporary.path()).unwrap();
    let lock_path = temporary.path().join("loupist.instance.lock");
    let conflict = AppInstanceLock::acquire(temporary.path()).err().unwrap();
    assert!(is_app_instance_conflict(&conflict));
    assert!(std::fs::remove_file(&lock_path).is_err());
    drop(lock);
    std::fs::remove_file(lock_path).unwrap();
}

#[cfg(unix)]
#[test]
fn test_state_hardens_app_data_children_and_lock() {
    use std::os::unix::fs::PermissionsExt;

    let temporary = tempfile::tempdir().unwrap();
    let app_data = temporary.path().join("app");
    let state = AppState::for_test(app_data.clone());
    assert_eq!(
        std::fs::metadata(&app_data).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for child in ["brokers", "pi", "runs", "process-leases"] {
        assert_eq!(
            std::fs::metadata(app_data.join(child))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    assert_eq!(
        std::fs::metadata(app_data.join("loupist.instance.lock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    drop(state);
}

#[test]
fn opening_a_foreign_schema_fails_without_overwriting_the_database() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("app-data");
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("loupist.sqlite3");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(&format!(
            "CREATE TABLE sentinel (value TEXT NOT NULL) STRICT;\
             INSERT INTO sentinel (value) VALUES ('preserve-me');\
             PRAGMA user_version = {};",
            crate::store::STORE_SCHEMA_VERSION
        ))
        .unwrap();
    drop(connection);

    assert!(open_app_store(&path).is_err());

    let connection = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        crate::store::STORE_SCHEMA_VERSION
    );
    assert_eq!(
        connection
            .query_row("SELECT value FROM sentinel", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "preserve-me"
    );
}

#[test]
fn an_earlier_build_catalog_is_set_aside_and_the_app_starts_fresh() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog.sqlite3");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(&format!(
            "CREATE TABLE sentinel (value TEXT NOT NULL) STRICT; PRAGMA user_version = {}; PRAGMA application_id = {};",
            1,          // the first store schema
            0x4755_5255 // the store's SQLite application id
        ))
        .unwrap();
    drop(connection);
    let (_store, fresh) = super::open_app_store(&path).unwrap();
    assert!(fresh);
    let set_aside: Vec<_> = std::fs::read_dir(temporary.path())
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with("catalog.sqlite3.earlier-"))
        .collect();
    assert_eq!(set_aside.len(), 1, "{set_aside:?}");
}

#[test]
fn an_earlier_catalog_hands_its_environment_identity_to_the_fresh_one() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog.sqlite3");
    let environment = uuid::Uuid::new_v4().to_string();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(&format!(
            "CREATE TABLE host_environment (id INTEGER PRIMARY KEY CHECK(id=1), environment_id TEXT NOT NULL) STRICT;
             INSERT INTO host_environment VALUES (1, '{environment}');
             PRAGMA user_version = {}; PRAGMA application_id = {};",
            crate::store::STORE_BASELINE_VERSION - 1,
            crate::store::STORE_APPLICATION_ID
        ))
        .unwrap();
    drop(connection);
    let (store, fresh) = super::open_app_store(&path).unwrap();
    assert!(fresh);
    // Keychain items are keyed by this identity, so they stay reachable.
    assert_eq!(store.initialize_host_environment().unwrap(), environment);
}

#[test]
fn a_newer_catalog_is_refused_with_a_dialog_and_left_untouched() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog.sqlite3");
    drop(SqliteStore::open_catalog(&path).unwrap());
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(&format!(
            "PRAGMA user_version = {};",
            crate::store::STORE_SCHEMA_VERSION + 1
        ))
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let error = super::open_app_store(&path).err().unwrap();
    assert_eq!(error.code, super::CATALOG_NEWER_VERSION);
    let (title, body) = super::startup_storage_notice(&error).unwrap();
    assert_eq!(title, "Loupist can't open your data");
    assert!(body.contains("newer version of Loupist"), "{body}");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let siblings = std::fs::read_dir(temporary.path())
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.contains(".earlier-") || name.contains(".upgrade-backup-"))
        .count();
    assert_eq!(siblings, 0);
    // Other startup failures are logged without a dialog.
    assert!(super::startup_storage_notice(&CommandError::internal("x")).is_none());
}
