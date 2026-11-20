use super::loupist::*;
use super::types::LoupistReconnectRequest;
use crate::{
    app::AppState,
    knowledge::{
        Authorship, Coverage, KnowledgeStore, RepresentationRole, RetainMaterial, RetainNewSource,
        Selector, SourceClass,
    },
    runs::coordinator::{RunKind, RunTarget},
    scheduled_tasks::types::{Schedule, TaskDraft, TaskStatus, TaskTarget},
};
use std::{collections::BTreeMap, fs, path::Path};

fn retain(store: &KnowledgeStore, loupist: &str, text: &str) -> crate::knowledge::RetainedSource {
    store
        .transact(|write| {
            write.retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: loupist.into(),
                at_ms: 1_700_000_000_010,
                title: text.into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "https://example.test/research".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                publication_time: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "document".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: vec![],
                    bytes: text.as_bytes().to_vec(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: text.len() as u64,
                    },
                    context_note: String::new(),
                }),
            })
        })
        .unwrap()
}

async fn record_count(state: &AppState, loupist_id: &str) -> usize {
    list_available_loupists(state)
        .await
        .unwrap()
        .into_iter()
        .find(|summary| summary.id == loupist_id)
        .unwrap()
        .record_count
}
fn draft() -> TaskDraft {
    TaskDraft {
        title: "Saved task".into(),
        prompt: "Summarize research".into(),
        schedule: Schedule::Interval { every_minutes: 5 },
        target: TaskTarget::NewChat,
        model_profile_id: "fixture/model".into(),
        thinking_level: "off".into(),
        run_options: BTreeMap::new(),
        use_memory: true,
        update_memory: false,
        status: TaskStatus::Active,
    }
}
fn copy_folder(source: &Path, destination: &Path) {
    fs::create_dir(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let to = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_folder(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), to).unwrap();
        }
    }
}

#[cfg(unix)]
async fn remounted_root_fixture() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    crate::domain::LoupistProfile,
    crate::domain::LoupistProfile,
) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("research");
    fs::create_dir(&root).unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let summary = create_folder_loupist(&state, &root).await.unwrap();
    let current = state.store.get_loupist(&summary.id).unwrap().unwrap();
    drop(state);
    let mut recorded = current.clone();
    recorded.root_filesystem_identity.as_mut().unwrap().device ^= 1;
    recorded
        .runtime_filesystem_identity
        .as_mut()
        .unwrap()
        .device ^= 1;
    write_remount_profile(temp.path(), &root, &recorded);
    (temp, root, current, recorded)
}

#[cfg(unix)]
fn write_remount_profile(temp: &Path, root: &Path, recorded: &crate::domain::LoupistProfile) {
    for database in [
        temp.join("app/catalog.sqlite3"),
        root.join(".loupist/loupist.sqlite3"),
    ] {
        rusqlite::Connection::open(database)
            .unwrap()
            .execute(
                "UPDATE loupist_profiles SET root_device=?1, data_json=?2, root_inode=?4, updated_at_ms=?5 WHERE id=?3",
                rusqlite::params![
                    recorded
                        .root_filesystem_identity
                        .as_ref()
                        .unwrap()
                        .device
                        .to_string(),
                    serde_json::to_string(&recorded).unwrap(),
                    recorded.id,
                    recorded.root_filesystem_identity.as_ref().unwrap().inode.to_string(),
                    recorded.updated_at_ms,
                ],
            )
            .unwrap();
    }
}

#[cfg(unix)]
fn assert_recorded_identity(database: &Path, expected: &crate::domain::LoupistProfile) {
    let connection = rusqlite::Connection::open(database).unwrap();
    let (device, inode, json): (String, String, String) = connection
        .query_row(
            "SELECT root_device, root_inode, data_json FROM loupist_profiles WHERE id=?1",
            [&expected.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let profile: crate::domain::LoupistProfile = serde_json::from_str(&json).unwrap();
    let identity = expected.root_filesystem_identity.as_ref().unwrap();
    assert_eq!(device, identity.device.to_string());
    assert_eq!(inode, identity.inode.to_string());
    assert_eq!(
        profile.root_filesystem_identity,
        expected.root_filesystem_identity
    );
    assert_eq!(
        profile.runtime_filesystem_identity,
        expected.runtime_filesystem_identity
    );
    let journal: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        journal,
        if database.file_name().unwrap() == "catalog.sqlite3" {
            "wal"
        } else {
            "delete"
        }
    );
}

#[cfg(unix)]
#[tokio::test]
async fn remounted_root_opens_without_unavailable_state_and_persists_both_identities() {
    let (temp, root, current, _) = remounted_root_fixture().await;
    let state = AppState::for_folder_test(temp.path().join("app"));
    let listed = list_available_loupists(&state).await.unwrap();
    assert_eq!(
        listed[0].availability,
        crate::app::LoupistAvailability::Available
    );
    let profile = state.store.get_loupist(&current.id).unwrap().unwrap();
    assert!(crate::fs::loupist_root::profile_workspace(&profile).is_ok());
    assert!(state.store.lock_loupist(&current.id).is_ok());
    drop(state);
    for database in [
        temp.path().join("app/catalog.sqlite3"),
        root.join(".loupist/loupist.sqlite3"),
    ] {
        assert_recorded_identity(&database, &current);
    }
    let state = AppState::for_folder_test(temp.path().join("app"));
    assert_eq!(
        list_available_loupists(&state).await.unwrap()[0].availability,
        crate::app::LoupistAvailability::Available
    );
}

#[cfg(unix)]
#[tokio::test]
async fn remounted_root_direct_read_recovers_before_listing() {
    let (temp, root, current, _) = remounted_root_fixture().await;
    let state = AppState::for_folder_test(temp.path().join("app"));
    let profile = state.store.get_loupist(&current.id).unwrap().unwrap();
    assert_eq!(
        profile.root_filesystem_identity,
        current.root_filesystem_identity
    );
    assert_eq!(
        profile.runtime_filesystem_identity,
        current.runtime_filesystem_identity
    );
    drop(state);
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &current);
    assert_recorded_identity(&root.join(".loupist/loupist.sqlite3"), &current);
}

#[cfg(unix)]
#[tokio::test]
async fn remounted_root_concurrent_readers_observe_the_recovered_binding() {
    let (temp, _, current, _) = remounted_root_fixture().await;
    let state = AppState::for_folder_test(temp.path().join("app"));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let store = state.store.clone();
            let id = current.id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.get_loupist(&id).unwrap().unwrap()
            })
        })
        .collect();
    for reader in readers {
        let profile = reader.join().unwrap();
        assert_eq!(
            profile.root_filesystem_identity,
            current.root_filesystem_identity
        );
        assert_eq!(
            profile.runtime_filesystem_identity,
            current.runtime_filesystem_identity
        );
    }
}

#[cfg(unix)]
async fn assert_remounted_root_refused(case: &str) {
    let (temp, root, _, mut recorded) = remounted_root_fixture().await;
    match case {
        "different_inode" => {
            // Keep creation time and all other evidence valid to isolate the
            // workspace inode guard from the birth-time guard.
            recorded.root_filesystem_identity.as_mut().unwrap().inode ^= 1;
            write_remount_profile(temp.path(), &root, &recorded);
        }
        "missing_marker" => fs::remove_file(root.join(".loupist/loupist.sqlite3")).unwrap(),
        "foreign_marker" => {
            let foreign = temp.path().join("foreign");
            fs::create_dir(&foreign).unwrap();
            let other_state = AppState::for_folder_test(temp.path().join("other-app"));
            create_folder_loupist(&other_state, &foreign).await.unwrap();
            drop(other_state);
            fs::remove_dir_all(root.join(".loupist")).unwrap();
            copy_folder(&foreign.join(".loupist"), &root.join(".loupist"));
        }
        "symlink" => {
            let moved = temp.path().join("original");
            fs::rename(&root, &moved).unwrap();
            std::os::unix::fs::symlink(&moved, &root).unwrap();
        }
        "data_symlink" => {
            let moved = temp.path().join("data");
            fs::rename(root.join(".loupist"), &moved).unwrap();
            std::os::unix::fs::symlink(&moved, root.join(".loupist")).unwrap();
        }
        "database_symlink" => {
            let moved = temp.path().join("database.sqlite3");
            fs::rename(root.join(".loupist/loupist.sqlite3"), &moved).unwrap();
            std::os::unix::fs::symlink(&moved, root.join(".loupist/loupist.sqlite3")).unwrap();
        }
        "not_directory" => {
            fs::rename(&root, temp.path().join("original")).unwrap();
            fs::write(&root, "not a directory").unwrap();
        }
        "missing_path" => fs::rename(&root, temp.path().join("original")).unwrap(),
        "runtime_inode" => {
            recorded.runtime_filesystem_identity.as_mut().unwrap().inode ^= 1;
            write_remount_profile(temp.path(), &root, &recorded);
        }
        "rollback" => {
            rusqlite::Connection::open(root.join(".loupist/loupist.sqlite3"))
                .unwrap()
                .execute_batch("CREATE TRIGGER refuse_identity_update BEFORE UPDATE ON loupist_profiles BEGIN SELECT RAISE(ABORT, 'fixture identity write rejected'); END;")
                .unwrap();
        }
        _ => unreachable!(),
    }
    let state = AppState::for_folder_test(temp.path().join("app"));
    assert_eq!(
        list_available_loupists(&state).await.unwrap()[0].availability,
        crate::app::LoupistAvailability::WorkspaceUnavailable,
        "{case}"
    );
    drop(state);
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &recorded);
    if case == "rollback" {
        assert_recorded_identity(&root.join(".loupist/loupist.sqlite3"), &recorded);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn remounted_root_rejects_different_inode() {
    assert_remounted_root_refused("different_inode").await;
}
#[cfg(unix)]
#[tokio::test]
async fn remounted_root_rejects_missing_marker() {
    assert_remounted_root_refused("missing_marker").await;
}
#[cfg(unix)]
#[tokio::test]
async fn remounted_root_rejects_foreign_marker() {
    assert_remounted_root_refused("foreign_marker").await;
}
#[cfg(unix)]
#[tokio::test]
async fn remounted_root_rejects_symlink() {
    assert_remounted_root_refused("symlink").await;
}
#[cfg(unix)]
#[tokio::test]
async fn remounted_root_rejects_data_directory_symlink() {
    assert_remounted_root_refused("data_symlink").await;
}
#[cfg(unix)]
#[tokio::test]
async fn remounted_root_rejects_database_symlink() {
    assert_remounted_root_refused("database_symlink").await;
}
#[cfg(unix)]
#[tokio::test]
async fn remounted_root_rejects_non_directory() {
    assert_remounted_root_refused("not_directory").await;
}
#[cfg(unix)]
#[tokio::test]
async fn remounted_root_rejects_missing_path() {
    assert_remounted_root_refused("missing_path").await;
}
#[cfg(unix)]
#[tokio::test]
async fn remounted_root_rejects_runtime_substitution() {
    assert_remounted_root_refused("runtime_inode").await;
}
#[cfg(unix)]
#[tokio::test]
async fn remounted_root_rolls_back_both_databases_on_write_failure() {
    assert_remounted_root_refused("rollback").await;
}

#[cfg(unix)]
#[tokio::test]
async fn remount_security_recreated_workspace_at_reused_inode_is_refused() {
    let (temp, root, _, mut recorded) = remounted_root_fixture().await;
    let data = temp.path().join("saved-data");
    fs::rename(root.join(".loupist"), &data).unwrap();
    fs::remove_dir(&root).unwrap();
    fs::create_dir(&root).unwrap();
    fs::rename(data, root.join(".loupist")).unwrap();
    let actual = crate::fs::loupist_root::BoundLoupistRoot::open_unbound(root.clone())
        .unwrap()
        .identity()
        .unwrap();
    let saved = recorded.root_filesystem_identity.as_mut().unwrap();
    // Force the historical inode to equal the recreated directory's inode,
    // independently of the filesystem allocator, retaining its older birth.
    saved.inode = actual.inode;
    saved.birth_time.as_mut().unwrap().seconds -= 1;
    write_remount_profile(temp.path(), &root, &recorded);
    let state = AppState::for_folder_test(temp.path().join("app"));
    assert_eq!(
        state.store.get_loupist(&recorded.id).unwrap().unwrap(),
        recorded
    );
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &recorded);
    assert_recorded_identity(&root.join(".loupist/loupist.sqlite3"), &recorded);
}

#[cfg(unix)]
#[tokio::test]
async fn remount_security_recreated_runtime_at_reused_inode_is_refused() {
    let (temp, root, _, mut recorded) = remounted_root_fixture().await;
    let runtime = temp.path().join("app/loupists").join(&recorded.id);
    fs::remove_dir_all(&runtime).unwrap();
    fs::create_dir(&runtime).unwrap();
    fs::write(runtime.join("foreign-content"), "must remain untouched").unwrap();
    let actual = crate::fs::loupist_root::BoundLoupistRoot::open_unbound(runtime.clone())
        .unwrap()
        .identity()
        .unwrap();
    let saved = recorded.runtime_filesystem_identity.as_mut().unwrap();
    saved.inode = actual.inode;
    saved.birth_time.as_mut().unwrap().seconds -= 1;
    write_remount_profile(temp.path(), &root, &recorded);
    let state = AppState::for_folder_test(temp.path().join("app"));
    assert_eq!(
        state.store.get_loupist(&recorded.id).unwrap().unwrap(),
        recorded
    );
    assert_eq!(
        fs::read_to_string(runtime.join("foreign-content")).unwrap(),
        "must remain untouched"
    );
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &recorded);
    assert_recorded_identity(&root.join(".loupist/loupist.sqlite3"), &recorded);
}

#[cfg(unix)]
async fn remount_security_birth_refused(runtime: bool, missing: bool) {
    let (temp, root, _, mut recorded) = remounted_root_fixture().await;
    let saved = if runtime {
        recorded.runtime_filesystem_identity.as_mut()
    } else {
        recorded.root_filesystem_identity.as_mut()
    }
    .unwrap();
    let inode = saved.inode;
    let mut future = saved.birth_time.unwrap();
    future.seconds = recorded.updated_at_ms / 1000 + 10;
    saved.birth_time = None;
    write_remount_profile(temp.path(), &root, &recorded);
    let state = AppState::for_folder_test(temp.path().join("app"));
    crate::fs::filesystem_identity::with_birth_time(inode, (!missing).then_some(future), || {
        assert_eq!(
            state.store.get_loupist(&recorded.id).unwrap().unwrap(),
            recorded
        );
    });
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &recorded);
    assert_recorded_identity(&root.join(".loupist/loupist.sqlite3"), &recorded);
}

#[cfg(unix)]
#[tokio::test]
async fn remount_security_workspace_newer_than_legacy_binding_is_refused() {
    remount_security_birth_refused(false, false).await;
}
#[cfg(unix)]
#[tokio::test]
async fn remount_security_runtime_newer_than_legacy_binding_is_refused() {
    remount_security_birth_refused(true, false).await;
}
#[cfg(unix)]
#[tokio::test]
async fn remount_security_workspace_without_birth_time_is_refused() {
    remount_security_birth_refused(false, true).await;
}
#[cfg(unix)]
#[tokio::test]
async fn remount_security_runtime_without_birth_time_is_refused() {
    remount_security_birth_refused(true, true).await;
}

#[cfg(unix)]
#[tokio::test]
async fn remount_security_legacy_owner_recovers_and_next_open_checks_saved_birth() {
    let (temp, root, current, mut recorded) = remounted_root_fixture().await;
    recorded
        .root_filesystem_identity
        .as_mut()
        .unwrap()
        .birth_time = None;
    recorded
        .runtime_filesystem_identity
        .as_mut()
        .unwrap()
        .birth_time = None;
    // Make the precision boundary explicit: both directories predate this
    // legacy row's identity write by at least one second.
    recorded.updated_at_ms += 1000;
    write_remount_profile(temp.path(), &root, &recorded);
    let state = AppState::for_folder_test(temp.path().join("app"));
    assert_eq!(
        list_available_loupists(&state).await.unwrap()[0].availability,
        crate::app::LoupistAvailability::Available
    );
    drop(state);
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &current);
    assert_recorded_identity(&root.join(".loupist/loupist.sqlite3"), &current);
    let state = AppState::for_folder_test(temp.path().join("app"));
    let binding = current.root_filesystem_identity.as_ref().unwrap();
    let mut foreign_birth = binding.birth_time.unwrap();
    foreign_birth.seconds += 1;
    crate::fs::filesystem_identity::with_birth_time(binding.inode, Some(foreign_birth), || {
        let profile = state.store.get_loupist(&current.id).unwrap().unwrap();
        assert!(crate::fs::loupist_root::profile_workspace(&profile).is_err());
    });
    assert_eq!(
        list_available_loupists(&state).await.unwrap()[0].availability,
        crate::app::LoupistAvailability::Available
    );
}

#[cfg(unix)]
#[tokio::test]
async fn remount_security_manual_reconnect_compares_pre_recovery_runtime_binding() {
    let (temp, root, _, recorded) = remounted_root_fixture().await;
    let state = AppState::for_folder_test(temp.path().join("app"));
    let error = reconnect_loupist_inner(
        &state,
        &LoupistReconnectRequest {
            loupist_id: recorded.id.clone(),
            workspace_path: root.to_string_lossy().into_owned(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.message, "Loupist private runtime folder changed");
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &recorded);
}

#[cfg(unix)]
fn insert_rejected_research(database: &Path, loupist: &str, digest: bool) {
    let connection = rusqlite::Connection::open(database).unwrap();
    // In the foreign-owner case the body digest is correct, so the ownership
    // check is independently necessary. In the digest case ownership is right.
    connection.execute(
        "INSERT INTO knowledge_research_records(research_id,loupist_id,body_json,record_digest,created_at_ms) VALUES('injected',?1,'{}',?2,1)",
        rusqlite::params![if digest { loupist } else { "foreign-loupist" },
            if digest { "not-a-digest" } else { "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a" }],
    ).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn remount_security_database_replaced_after_admission_commits_neither_file() {
    let (temp, root, _, recorded) = remounted_root_fixture().await;
    let database = root.join(".loupist/loupist.sqlite3");
    let replacement = temp.path().join("rejected.sqlite3");
    fs::copy(&database, &replacement).unwrap();
    insert_rejected_research(&replacement, &recorded.id, true);
    let rejected_dir = temp.path().join("rejected");
    fs::create_dir(&rejected_dir).unwrap();
    fs::copy(&replacement, rejected_dir.join("loupist.sqlite3")).unwrap();
    assert!(crate::knowledge::validate_folder(&rejected_dir).is_err());
    let replaced = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = replaced.clone();
    let swap_path = database.clone();
    let state = AppState::for_folder_test(temp.path().join("app"));
    crate::store::with_after_admission(
        move || {
            use std::os::unix::fs::MetadataExt;
            let admitted_inode = fs::metadata(&swap_path).unwrap().ino();
            fs::remove_file(&swap_path).unwrap();
            fs::copy(replacement, &swap_path).unwrap();
            // Holding both the original descriptor and SQLite connection keeps its
            // inode allocated: the attack cannot recycle that inode for this file.
            assert_ne!(admitted_inode, fs::metadata(&swap_path).unwrap().ino());
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        },
        || {
            assert_eq!(
                state.store.get_loupist(&recorded.id).unwrap().unwrap(),
                recorded
            );
        },
    );
    assert!(replaced.load(std::sync::atomic::Ordering::SeqCst));
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &recorded);
    assert_recorded_identity(&database, &recorded);
}

#[cfg(unix)]
async fn remount_security_transaction_revalidates(digest: bool) {
    let (temp, root, _, recorded) = remounted_root_fixture().await;
    let database = root.join(".loupist/loupist.sqlite3");
    let write_path = database.clone();
    let id = recorded.id.clone();
    let mutated = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = mutated.clone();
    let state = AppState::for_folder_test(temp.path().join("app"));
    crate::store::with_after_admission(
        move || {
            insert_rejected_research(&write_path, &id, digest);
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        },
        || {
            assert_eq!(
                state.store.get_loupist(&recorded.id).unwrap().unwrap(),
                recorded
            );
        },
    );
    assert!(mutated.load(std::sync::atomic::Ordering::SeqCst));
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &recorded);
    assert_recorded_identity(&database, &recorded);
}

#[cfg(unix)]
#[tokio::test]
async fn remount_security_transaction_rechecks_digest_after_admission() {
    remount_security_transaction_revalidates(true).await;
}
#[cfg(unix)]
#[tokio::test]
async fn remount_security_transaction_rechecks_foreign_owner_after_admission() {
    remount_security_transaction_revalidates(false).await;
}

#[cfg(unix)]
#[tokio::test]
async fn remount_security_transaction_rechecks_rows_written_by_profile_trigger() {
    let (temp, root, _, recorded) = remounted_root_fixture().await;
    let database = root.join(".loupist/loupist.sqlite3");
    rusqlite::Connection::open(&database).unwrap().execute_batch(
        "CREATE TRIGGER inject_rejected_research AFTER UPDATE ON loupist_profiles BEGIN
         INSERT INTO knowledge_research_records(research_id,loupist_id,body_json,record_digest,created_at_ms)
         VALUES('trigger-injected',NEW.id,'{}','not-a-digest',1); END;"
    ).unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    assert_eq!(
        state.store.get_loupist(&recorded.id).unwrap().unwrap(),
        recorded
    );
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &recorded);
    assert_recorded_identity(&database, &recorded);
    let injected: i64 = rusqlite::Connection::open(&database)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_research_records WHERE research_id='trigger-injected'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(injected, 0);
}

#[cfg(unix)]
async fn rereview_invalid_birth(seconds: i64, stored: bool, runtime: bool) {
    let (temp, root, _, mut recorded) = remounted_root_fixture().await;
    let invalid = crate::domain::FilesystemBirthTime {
        seconds,
        nanoseconds: 0,
    };
    let identity = if runtime {
        recorded.runtime_filesystem_identity.as_mut()
    } else {
        recorded.root_filesystem_identity.as_mut()
    }
    .unwrap();
    let inode = identity.inode;
    identity.birth_time = stored.then_some(invalid);
    write_remount_profile(temp.path(), &root, &recorded);
    let state = AppState::for_folder_test(temp.path().join("app"));
    crate::fs::filesystem_identity::with_birth_time(inode, Some(invalid), || {
        assert_eq!(
            state.store.get_loupist(&recorded.id).unwrap().unwrap(),
            recorded
        );
    });
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &recorded);
    assert_recorded_identity(&root.join(".loupist/loupist.sqlite3"), &recorded);
}

#[cfg(unix)]
#[tokio::test]
async fn rereview_workspace_legacy_zero_birth_is_refused() {
    rereview_invalid_birth(0, false, false).await;
}

#[cfg(unix)]
#[tokio::test]
async fn rereview_workspace_legacy_negative_birth_is_refused() {
    rereview_invalid_birth(-1, false, false).await;
}

#[cfg(unix)]
#[tokio::test]
async fn rereview_workspace_saved_zero_birth_is_refused() {
    rereview_invalid_birth(0, true, false).await;
}

#[cfg(unix)]
#[tokio::test]
async fn rereview_workspace_saved_negative_birth_is_refused() {
    rereview_invalid_birth(-1, true, false).await;
}

#[cfg(unix)]
#[tokio::test]
async fn rereview_runtime_legacy_zero_birth_is_refused() {
    rereview_invalid_birth(0, false, true).await;
}

#[cfg(unix)]
#[tokio::test]
async fn rereview_runtime_legacy_negative_birth_is_refused() {
    rereview_invalid_birth(-1, false, true).await;
}

#[cfg(unix)]
#[tokio::test]
async fn rereview_runtime_saved_zero_birth_is_refused() {
    rereview_invalid_birth(0, true, true).await;
}

#[cfg(unix)]
#[tokio::test]
async fn rereview_runtime_saved_negative_birth_is_refused() {
    rereview_invalid_birth(-1, true, true).await;
}

#[cfg(unix)]
#[tokio::test]
async fn rereview_workspace_creation_cutoff_survives_a_later_profile_update() {
    let (temp, root, _, mut recorded) = remounted_root_fixture().await;
    let saved = recorded.root_filesystem_identity.as_mut().unwrap();
    let inode = saved.inode;
    saved.birth_time = None;
    let substitute = crate::domain::FilesystemBirthTime {
        seconds: recorded.created_at_ms / 1000 + 10,
        nanoseconds: 0,
    };
    recorded.updated_at_ms += 3_600_000;
    write_remount_profile(temp.path(), &root, &recorded);
    let state = AppState::for_folder_test(temp.path().join("app"));
    crate::fs::filesystem_identity::with_birth_time(inode, Some(substitute), || {
        assert_eq!(
            state.store.get_loupist(&recorded.id).unwrap().unwrap(),
            recorded
        );
    });
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &recorded);
    assert_recorded_identity(&root.join(".loupist/loupist.sqlite3"), &recorded);
}

#[cfg(unix)]
#[tokio::test]
async fn rereview_owner_november_13_birth_precedes_database_creation_and_catalog_update() {
    let (temp, root, mut current, mut recorded) = remounted_root_fixture().await;
    let parse = |time: &str| {
        chrono::DateTime::parse_from_rfc3339(time)
            .unwrap()
            .timestamp_millis()
    };
    let root_ms = parse("2026-11-13T11:49:30+09:00");
    let creation_ms = parse("2026-11-13T11:49:33+09:00");
    // A legitimate private runtime can be allocated after profile creation.
    let runtime_ms = parse("2026-11-13T11:49:34+09:00");
    recorded.created_at_ms = creation_ms;
    recorded.updated_at_ms = parse("2026-11-13T12:19:00+09:00");
    recorded
        .root_filesystem_identity
        .as_mut()
        .unwrap()
        .birth_time = None;
    recorded
        .runtime_filesystem_identity
        .as_mut()
        .unwrap()
        .birth_time = None;
    write_remount_profile(temp.path(), &root, &recorded);
    let mut local = recorded.clone();
    local.updated_at_ms = creation_ms;
    rusqlite::Connection::open(root.join(".loupist/loupist.sqlite3"))
        .unwrap()
        .execute(
            "UPDATE loupist_profiles SET updated_at_ms=?1,data_json=?2 WHERE id=?3",
            rusqlite::params![
                local.updated_at_ms,
                serde_json::to_string(&local).unwrap(),
                local.id
            ],
        )
        .unwrap();
    let root_birth = crate::domain::FilesystemBirthTime {
        seconds: root_ms / 1000,
        nanoseconds: 0,
    };
    let runtime_birth = crate::domain::FilesystemBirthTime {
        seconds: runtime_ms / 1000,
        nanoseconds: 0,
    };
    let root_id = current.root_filesystem_identity.as_mut().unwrap();
    root_id.birth_time = Some(root_birth);
    let root_inode = root_id.inode;
    let runtime_id = current.runtime_filesystem_identity.as_mut().unwrap();
    runtime_id.birth_time = Some(runtime_birth);
    let runtime_inode = runtime_id.inode;
    let state = AppState::for_folder_test(temp.path().join("app"));
    crate::fs::filesystem_identity::with_birth_time(root_inode, Some(root_birth), || {
        crate::fs::filesystem_identity::with_birth_time(runtime_inode, Some(runtime_birth), || {
            let recovered = state.store.get_loupist(&recorded.id).unwrap().unwrap();
            assert!(crate::fs::loupist_root::profile_workspace(&recovered).is_ok());
            assert!(state.store.lock_loupist(&recorded.id).is_ok());
            assert_eq!(recovered.created_at_ms, creation_ms);
        });
    });
    drop(state);
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &current);
    assert_recorded_identity(&root.join(".loupist/loupist.sqlite3"), &current);
}

#[cfg(unix)]
#[tokio::test]
async fn rereview_unknown_creation_time_retains_the_catalog_cutoff() {
    let (temp, root, current, mut recorded) = remounted_root_fixture().await;
    recorded
        .root_filesystem_identity
        .as_mut()
        .unwrap()
        .birth_time = None;
    recorded.created_at_ms = 0;
    recorded.updated_at_ms += 1000;
    write_remount_profile(temp.path(), &root, &recorded);
    let state = AppState::for_folder_test(temp.path().join("app"));
    let recovered = state.store.get_loupist(&recorded.id).unwrap().unwrap();
    assert_eq!(
        recovered.root_filesystem_identity,
        current.root_filesystem_identity
    );
    drop(state);
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &current);
    assert_recorded_identity(&root.join(".loupist/loupist.sqlite3"), &current);
}

#[cfg(unix)]
#[tokio::test]
async fn rereview_database_replaced_after_writes_before_commit_is_refused() {
    let (temp, root, _, recorded) = remounted_root_fixture().await;
    let database = root.join(".loupist/loupist.sqlite3");
    let replacement = temp.path().join("replacement.sqlite3");
    fs::copy(&database, &replacement).unwrap();
    insert_rejected_research(&replacement, &recorded.id, true);
    let swap_path = database.clone();
    let displaced = temp.path().join("admitted.sqlite3");
    let state = AppState::for_folder_test(temp.path().join("app"));
    let fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = fired.clone();
    crate::store::with_before_commit(
        move || {
            fs::rename(&swap_path, displaced).unwrap();
            fs::copy(replacement, &swap_path).unwrap();
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        },
        || {
            assert_eq!(
                state.store.get_loupist(&recorded.id).unwrap().unwrap(),
                recorded
            );
        },
    );
    assert!(fired.load(std::sync::atomic::Ordering::SeqCst));
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &recorded);
    assert_recorded_identity(&database, &recorded);
}

#[cfg(unix)]
#[tokio::test]
async fn rereview_post_write_validation_rejects_foreign_owner_written_by_profile_trigger() {
    let (temp, root, _, recorded) = remounted_root_fixture().await;
    let database = root.join(".loupist/loupist.sqlite3");
    rusqlite::Connection::open(&database).unwrap().execute_batch(
        "CREATE TRIGGER inject_foreign_owner AFTER UPDATE ON loupist_profiles BEGIN
         INSERT INTO knowledge_research_records(research_id,loupist_id,body_json,record_digest,created_at_ms)
         VALUES('foreign-trigger','foreign-loupist','{}','44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a',1); END;"
    ).unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    assert_eq!(
        state.store.get_loupist(&recorded.id).unwrap().unwrap(),
        recorded
    );
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &recorded);
    assert_recorded_identity(&database, &recorded);
}

#[cfg(unix)]
#[tokio::test]
async fn rereview_folder_cannot_advance_creation_time_past_the_catalog_original() {
    let (temp, root, _, mut recorded) = remounted_root_fixture().await;
    recorded
        .root_filesystem_identity
        .as_mut()
        .unwrap()
        .birth_time = None;
    recorded.updated_at_ms += 3_600_000;
    write_remount_profile(temp.path(), &root, &recorded);
    let mut forged = recorded.clone();
    forged.created_at_ms += 20_000;
    let database = root.join(".loupist/loupist.sqlite3");
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE loupist_profiles SET data_json=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_string(&forged).unwrap(), recorded.id],
        )
        .unwrap();
    let substitute = crate::domain::FilesystemBirthTime {
        seconds: recorded.created_at_ms / 1000 + 10,
        nanoseconds: 0,
    };
    let state = AppState::for_folder_test(temp.path().join("app"));
    crate::fs::filesystem_identity::with_birth_time(
        recorded.root_filesystem_identity.as_ref().unwrap().inode,
        Some(substitute),
        || {
            assert_eq!(
                state.store.get_loupist(&recorded.id).unwrap().unwrap(),
                recorded
            );
        },
    );
    assert_recorded_identity(&temp.path().join("app/catalog.sqlite3"), &recorded);
    assert_recorded_identity(&database, &recorded);
}

#[tokio::test]
async fn checked_in_configuration_can_initialize_a_loupist_without_losing_resources() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("research");
    let data = folder.join(".loupist");
    fs::create_dir_all(data.join("skills/research-method")).unwrap();
    let config = "# shared defaults\n[pi.retry]\nmax_retries = 1\n";
    fs::write(data.join("config.toml"), config).unwrap();
    fs::write(data.join("AGENTS.md"), "Use primary sources.").unwrap();
    fs::write(
        data.join("skills/research-method/SKILL.md"),
        "A project method.",
    )
    .unwrap();
    fs::write(data.join(".gitignore"), "*\n").unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let loupist = create_folder_loupist(&state, &folder).await.unwrap();
    assert_eq!(
        fs::read_to_string(data.join("config.toml")).unwrap(),
        config
    );
    assert!(data.join("loupist.sqlite3").is_file());
    assert_eq!(state.store.list_loupists().unwrap()[0].id, loupist.id);
    // Exercise git's actual ignore semantics rather than locking file wording.
    let repo = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&folder)
        .status()
        .unwrap();
    assert!(repo.success());
    for resource in [
        ".loupist/config.toml",
        ".loupist/AGENTS.md",
        ".loupist/skills/research-method/SKILL.md",
        ".loupist/.gitignore",
    ] {
        assert!(!std::process::Command::new("git")
            .args(["check-ignore", "--quiet", resource])
            .current_dir(&folder)
            .status()
            .unwrap()
            .success());
    }
    assert!(std::process::Command::new("git")
        .args(["check-ignore", "--quiet", ".loupist/loupist.sqlite3"])
        .current_dir(&folder)
        .status()
        .unwrap()
        .success());
}

#[tokio::test]
async fn incomplete_research_folder_is_not_reinitialized_as_configuration_only() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("research");
    fs::create_dir_all(folder.join(".loupist/knowledge-blobs")).unwrap();
    fs::write(folder.join(".loupist/config.toml"), "").unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    assert!(create_folder_loupist(&state, &folder).await.is_err());
    assert!(!folder.join(".loupist/loupist.sqlite3").exists());
    assert!(state.store.list_loupists().unwrap().is_empty());
}

#[tokio::test]
async fn folder_registration_rejects_private_storage_duplicates_and_invalid_paths() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let state = AppState::for_folder_test(home.join(".loupist/development"));
    for refused in [
        home.clone(),
        home.join(".loupist"),
        state.artifacts.app_data_dir.clone(),
        temp.path().join("missing"),
        std::path::PathBuf::from("research"),
    ] {
        assert!(
            create_folder_loupist(&state, &refused).await.is_err(),
            "{refused:?}"
        );
    }
    assert!(!home.join(".loupist/loupist.sqlite3").exists());
    let folder = temp.path().join("research");
    fs::create_dir(&folder).unwrap();
    create_folder_loupist(&state, &folder).await.unwrap();
    assert_eq!(
        create_folder_loupist(&state, &folder)
            .await
            .unwrap_err()
            .code,
        "conflict"
    );
    assert_eq!(state.store.list_loupists().unwrap().len(), 1);
}

#[tokio::test]
async fn changed_connector_scope_stays_denied_when_missing_folder_blocks_cleanup() {
    use crate::marketplace::connector_config::{
        configure_connector, connector_config_value, reconcile_connector_configs,
    };
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let folder = temp.path().join("research");
    fs::create_dir(&folder).unwrap();
    let loupist = create_folder_loupist(&state, &folder).await.unwrap();
    let id = crate::finance_data::KIS_SOURCE_ID;
    configure_connector(
        crate::marketplace::MarketplaceConnectorConfigureRequest {
            entry_id: id.into(),
            config: BTreeMap::from([("environment".into(), "real".into())]),
        },
        &state,
    )
    .unwrap();
    let binding_timestamp = state
        .store
        .get_loupist_capability(&loupist.id, id)
        .unwrap()
        .map_or(1, |binding| binding.updated_at_ms + 1);
    state
        .store
        .save_loupist_capability(&crate::domain::LoupistCapabilityBinding {
            loupist_id: loupist.id.clone(),
            entry_id: id.into(),
            enabled: true,
            granted_permissions: vec!["execute".into()],
            config: BTreeMap::new(),
            updated_at_ms: binding_timestamp,
        })
        .unwrap();
    let moved = temp.path().join("research-moved");
    fs::rename(&folder, &moved).unwrap();
    crate::scoped_config::write_global_connector_settings(
        &state.artifacts.app_data_dir,
        id,
        &BTreeMap::from([("environment".into(), "demo".into())]),
    )
    .unwrap();
    assert!(reconcile_connector_configs(&state).is_err());
    assert!(connector_config_value(&state, id, "environment").is_err());
    drop(state);
    // A failed cleanup must not attest the new scope even after startup retries.
    let state = AppState::for_folder_test(temp.path().join("app"));
    assert!(connector_config_value(&state, id, "environment").is_err());
    fs::rename(&moved, &folder).unwrap();
    reconcile_connector_configs(&state).unwrap();
    assert_eq!(
        connector_config_value(&state, id, "environment")
            .unwrap()
            .as_deref(),
        Some("demo")
    );
    assert!(
        !state
            .store
            .get_loupist_capability(&loupist.id, id)
            .unwrap()
            .unwrap()
            .enabled
    );
}

#[tokio::test]
async fn folder_copy_reopens_chat_source_lens_and_paused_tasks_without_app_data() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("research");
    fs::create_dir(&original).unwrap();
    fs::write(original.join("notes.md"), "working file").unwrap();
    let other = temp.path().join("other");
    fs::create_dir(&other).unwrap();
    let state = AppState::for_folder_test(temp.path().join("app-a"));
    let a = create_folder_loupist(&state, &original).await.unwrap();
    let b = super::loupist::create_folder_loupist_with_setup(
        &state,
        &other,
        Some("Ferry operators".into()),
        None,
    )
    .await
    .unwrap();
    let mut chat = super::tests::chat("chat-copy", &a.id, 1_700_000_000_000);
    chat.title = "Saved conversation".into();
    state.store.create_chat(&chat).unwrap();
    let memory = KnowledgeStore::for_loupist(state.store.clone(), &a.id).unwrap();
    let retained = retain(&memory, &a.id, "exact retained original");
    let task = state
        .store
        .scheduled_task_create(&a.id, draft(), 1_700_000_000_020)
        .unwrap();
    let dataset = state
        .store
        .dataset_put(
            &a.id,
            "saved-data",
            &serde_json::json!({"rows":[1,2,3]}),
            serde_json::json!({"origin":"local_file"}),
        )
        .unwrap();
    assert!(state
        .store
        .dataset(&b.id, &dataset.dataset_id, dataset.revision)
        .is_err());
    let saved_records = record_count(&state, &a.id).await;
    assert_eq!(
        record_count(&state, &b.id).await,
        0,
        "a focus files no Lens page"
    );
    let catalog = state.store.lock().unwrap();
    assert_eq!(
        catalog
            .query_row("SELECT count(*) FROM chat_sessions", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        catalog
            .query_row("SELECT count(*) FROM knowledge_records", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(catalog);
    drop(memory);
    state.close_for_restart_test();
    let copied = temp.path().join("copied");
    copy_folder(&original, &copied);
    fs::remove_dir_all(temp.path().join("app-a")).unwrap();
    fs::remove_dir_all(&original).unwrap();
    let reopened = AppState::for_folder_test(temp.path().join("app-b"));
    let summary = create_folder_loupist(&reopened, &copied).await.unwrap();
    assert_eq!(summary.id, a.id);
    assert_eq!(summary.name, a.name);
    assert_eq!(
        reopened.store.get_chat("chat-copy").unwrap().unwrap().title,
        "Saved conversation"
    );
    assert!(reopened.store.get_loupist(&b.id).unwrap().is_none());
    let (reference, payload, _) = reopened
        .store
        .dataset(&a.id, &dataset.dataset_id, dataset.revision)
        .unwrap();
    assert_eq!(reference.digest, dataset.digest);
    assert_eq!(payload, serde_json::json!({"rows":[1,2,3]}));
    assert_eq!(record_count(&reopened, &a.id).await, saved_records);
    let memory = KnowledgeStore::for_loupist(reopened.store.clone(), &a.id).unwrap();
    assert_eq!(
        memory
            .get_revision(&a.id, &retained.revision_id)
            .unwrap()
            .revision_digest,
        retained.revision_digest
    );
    let task = reopened
        .store
        .scheduled_task_get(&a.id, &task.id)
        .unwrap()
        .unwrap();
    assert_eq!(task.status, TaskStatus::Paused);
    assert!(reopened
        .store
        .scheduled_tasks_due(i64::MAX / 2, 100)
        .unwrap()
        .is_empty());
    assert!(
        crate::knowledge::load_settings(&reopened.store.lock_loupist(&a.id).unwrap(), &a.id)
            .unwrap()
            .paused
    );
    assert_eq!(
        fs::read_to_string(copied.join("notes.md")).unwrap(),
        "working file"
    );
}

#[tokio::test]
async fn removing_registration_waits_for_runs_and_keeps_the_folder_for_reopening() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("research");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("keep.csv"), "revenue\n42\n").unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let loupist = create_folder_loupist(&state, &root).await.unwrap();
    let chat = super::tests::chat("chat-kept", &loupist.id, 10);
    state.store.create_chat(&chat).unwrap();
    let runtime = state
        .artifacts
        .app_data_dir
        .join("loupists")
        .join(&loupist.id);
    let active = state
        .register_run(
            "run-delete".into(),
            loupist.id.clone(),
            RunKind::Chat,
            RunTarget::ChatThread(chat.id.clone()),
        )
        .unwrap();
    assert_eq!(
        delete_loupist_inner(&state, &loupist.id)
            .await
            .unwrap_err()
            .code,
        "conflict"
    );
    drop(active);
    delete_loupist_inner(&state, &loupist.id).await.unwrap();
    assert!(state.store.list_loupists().unwrap().is_empty());
    assert!(!runtime.exists());
    assert_eq!(
        fs::read_to_string(root.join("keep.csv")).unwrap(),
        "revenue\n42\n"
    );
    let reopened = create_folder_loupist(&state, &root).await.unwrap();
    assert_eq!(reopened.id, loupist.id);
    assert!(state.store.get_chat(&chat.id).unwrap().is_some());
}

#[tokio::test]
async fn moved_folder_requires_same_loupist_and_rebinds_without_losing_chat() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("research");
    fs::create_dir(&root).unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let loupist = create_folder_loupist(&state, &root).await.unwrap();
    let chat = super::tests::chat("chat-moved", &loupist.id, 10);
    state.store.create_chat(&chat).unwrap();
    let previous_memory = KnowledgeStore::for_loupist(state.store.clone(), &loupist.id).unwrap();
    // Neither an unrelated folder nor another registered Loupist can be adopted.
    let empty = temp.path().join("empty");
    let other = temp.path().join("other");
    fs::create_dir(&empty).unwrap();
    fs::create_dir(&other).unwrap();
    create_folder_loupist(&state, &other).await.unwrap();
    let reconnect = |path: &Path| LoupistReconnectRequest {
        loupist_id: loupist.id.clone(),
        workspace_path: path.to_string_lossy().into(),
    };
    for path in [&empty, &other] {
        assert!(reconnect_loupist_inner(&state, &reconnect(path))
            .await
            .is_err());
    }
    let active = state
        .register_run(
            "run-reconnect".into(),
            loupist.id.clone(),
            RunKind::Chat,
            RunTarget::ChatThread("chat-active".into()),
        )
        .unwrap();
    assert_eq!(
        reconnect_loupist_inner(&state, &reconnect(&root))
            .await
            .unwrap_err()
            .code,
        "conflict"
    );
    drop(active);
    let records = record_count(&state, &loupist.id).await;
    let moved = temp.path().join("moved");
    fs::rename(&root, &moved).unwrap();
    let listed = list_available_loupists(&state).await.unwrap();
    let moved_summary = listed
        .iter()
        .find(|summary| summary.id == loupist.id)
        .unwrap();
    assert_eq!(
        moved_summary.availability,
        crate::app::LoupistAvailability::WorkspaceUnavailable
    );
    let reconnected = reconnect_loupist_inner(&state, &reconnect(&moved))
        .await
        .unwrap();
    assert_eq!(reconnected.id, loupist.id);
    assert_eq!(reconnected.name, loupist.name);
    assert!(state.store.get_chat(&chat.id).unwrap().is_some());
    // A handle bound to the old folder location must not write through.
    assert!(previous_memory.read(|_| Ok(())).is_err());
    assert_eq!(reconnected.record_count, records);
}

#[tokio::test]
async fn registered_duplicate_and_corrupt_copies_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("research");
    fs::create_dir(&root).unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let loupist = create_folder_loupist(&state, &root).await.unwrap();
    let memory = KnowledgeStore::for_loupist(state.store.clone(), &loupist.id).unwrap();
    retain(&memory, &loupist.id, "immutable bytes");
    drop(memory);
    let copied = temp.path().join("copied");
    copy_folder(&root, &copied);
    assert!(create_folder_loupist(&state, &copied).await.is_err());
    state.close_for_restart_test();
    let other = AppState::for_folder_test(temp.path().join("other-app"));
    let connection = rusqlite::Connection::open(copied.join(".loupist/loupist.sqlite3")).unwrap();
    let digest: String = connection
        .query_row("SELECT digest FROM knowledge_blobs LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    drop(connection);
    fs::write(
        copied
            .join(".loupist/knowledge-blobs/sha256")
            .join(&digest[..2])
            .join(digest),
        "tampered",
    )
    .unwrap();
    assert!(create_folder_loupist(&other, &copied).await.is_err());
    assert!(other.store.list_loupists().unwrap().is_empty());
}

#[tokio::test]
async fn another_app_cannot_import_a_live_loupist_or_change_its_journal_mode() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("loupist");
    fs::create_dir(&folder).unwrap();
    let first = AppState::for_folder_test(temp.path().join("app-a"));
    let loupist = create_folder_loupist(&first, &folder).await.unwrap();
    let second = AppState::for_folder_test(temp.path().join("app-b"));
    assert!(create_folder_loupist(&second, &folder).await.is_err());
    assert!(second.store.list_loupists().unwrap().is_empty());
    let mode: String = first
        .store
        .lock_loupist(&loupist.id)
        .unwrap()
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "delete");
    first
        .store
        .create_chat(&super::tests::chat("chat-still-writable", &loupist.id, 10))
        .unwrap();
}

#[tokio::test]
async fn portable_folder_rejects_records_owned_by_another_loupist() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("loupist");
    fs::create_dir(&folder).unwrap();
    let original = AppState::for_folder_test(temp.path().join("app-a"));
    create_folder_loupist(&original, &folder).await.unwrap();
    original.close_for_restart_test();
    let connection = rusqlite::Connection::open(folder.join(".loupist/loupist.sqlite3")).unwrap();
    connection.execute("INSERT INTO knowledge_records(record_id,kind,owner_loupist_id,title,created_at_ms) VALUES('foreign-record','source','other-loupist','foreign source',10)", []).unwrap();
    drop(connection);
    let new_app = AppState::for_folder_test(temp.path().join("app-b"));
    assert!(create_folder_loupist(&new_app, &folder).await.is_err());
    assert!(new_app.store.list_loupists().unwrap().is_empty());
}

#[tokio::test]
async fn chat_removal_erases_durable_attachments_and_private_runtime_files() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("loupist");
    fs::create_dir(&folder).unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let loupist = create_folder_loupist(&state, &folder).await.unwrap();
    let chat = super::tests::chat("chat-erased", &loupist.id, 10);
    state.store.create_chat(&chat).unwrap();
    let durable = state.loupist_files_root(&loupist.id).unwrap();
    let runtime = &state.artifacts.deletion_root;
    let attachment = std::path::PathBuf::from("loupists")
        .join(&loupist.id)
        .join("attachments")
        .join(&chat.id);
    let cache = std::path::PathBuf::from("loupists")
        .join(&loupist.id)
        .join("pi-sessions")
        .join(&chat.id);
    durable.ensure_private_subdirectory(&attachment).unwrap();
    runtime.ensure_private_subdirectory(&cache).unwrap();
    fs::write(
        durable.absolute_path(&attachment).unwrap().join("file.txt"),
        "attachment",
    )
    .unwrap();
    fs::write(
        runtime.absolute_path(&cache).unwrap().join("session.jsonl"),
        "conversation",
    )
    .unwrap();
    crate::store::deletion::delete_chat_with_runtime(
        state.store.as_ref(),
        durable.as_ref(),
        runtime.as_ref(),
        &chat,
        20,
    )
    .unwrap();
    assert!(!durable.entry_exists(&attachment).unwrap());
    assert!(!runtime.entry_exists(&cache).unwrap());
    assert!(state.store.list_deletion_journals().unwrap().is_empty());
}

#[tokio::test]
async fn deletion_recovery_preserves_files_when_canonical_database_is_unavailable() {
    use crate::domain::{DeletionJournal, DeletionKind, DeletionPhase};

    for kind in [DeletionKind::Chat, DeletionKind::ChatRuntime] {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("loupist");
        fs::create_dir(&folder).unwrap();
        let state = AppState::for_folder_test(temp.path().join("app"));
        let loupist = create_folder_loupist(&state, &folder).await.unwrap();
        let chat = super::tests::chat("chat-interrupted", &loupist.id, 10);
        state.store.create_chat(&chat).unwrap();
        let root = if kind == DeletionKind::Chat {
            state.loupist_files_root(&loupist.id).unwrap()
        } else {
            state.artifacts.deletion_root.clone()
        };
        let tombstone = std::path::PathBuf::from("loupists")
            .join(&loupist.id)
            .join(if kind == DeletionKind::Chat {
                "attachments"
            } else {
                "pi-sessions"
            })
            .join(format!(".deleting-{}", chat.id));
        root.ensure_private_subdirectory(&tombstone).unwrap();
        let file = root.absolute_path(&tombstone).unwrap().join("retained.txt");
        fs::write(&file, "keep until canonical absence is confirmed").unwrap();
        state
            .store
            .create_deletion_journal(&DeletionJournal {
                id: "delete-interrupted".into(),
                kind,
                loupist_id: loupist.id.clone(),
                target_id: chat.id,
                expected_source_identity: None,
                phase: DeletionPhase::Detached,
                created_at_ms: 20,
                updated_at_ms: 21,
            })
            .unwrap();
        fs::rename(
            folder.join(".loupist/loupist.sqlite3"),
            folder.join(".loupist/temporarily-moved.sqlite3"),
        )
        .unwrap();

        let recovery =
            crate::store::deletion::recover(state.store.as_ref(), &state.artifacts.deletion_root)
                .unwrap();
        assert!(recovery.quarantined_loupists.contains_key(&loupist.id));
        assert!(file.is_file());
        assert_eq!(state.store.list_deletion_journals().unwrap().len(), 1);
    }
}

fn folder_database(folder: &Path) -> std::path::PathBuf {
    folder.join(".loupist/loupist.sqlite3")
}

fn upgrade_backups(folder: &Path) -> Vec<std::path::PathBuf> {
    fs::read_dir(folder.join(".loupist"))
        .unwrap()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            path.file_name()?
                .to_str()?
                .starts_with("loupist.sqlite3.upgrade-backup-")
                .then_some(path.clone())
        })
        .collect()
}

fn folder_versions(folder: &Path) -> (i64, i64) {
    let connection = rusqlite::Connection::open(folder_database(folder)).unwrap();
    (
        connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap(),
        connection
            .query_row("SELECT version FROM knowledge_schema", [], |row| row.get(0))
            .unwrap(),
    )
}

const CURRENT_VERSIONS: (i64, i64) = (
    crate::store::STORE_SCHEMA_VERSION,
    crate::knowledge::KNOWLEDGE_SCHEMA_VERSION,
);

/// A folder an installed 0.0.1 build left (store 4, Knowledge 6) opens after
/// a private backup and keeps its chats.
#[tokio::test]
async fn a_baseline_folder_is_upgraded_in_place_after_a_backup() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("research");
    fs::create_dir(&root).unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let loupist = create_folder_loupist(&state, &root).await.unwrap();
    let chat = super::tests::chat("chat-baseline", &loupist.id, 10);
    state.store.create_chat(&chat).unwrap();
    state.close_for_restart_test();
    crate::store::restore_baseline_for_test(
        &mut rusqlite::Connection::open(folder_database(&root)).unwrap(),
    );
    assert_eq!(folder_versions(&root), (4, 6));

    let state = AppState::for_folder_test(temp.path().join("app"));
    let listed = list_available_loupists(&state).await.unwrap();
    assert_eq!(
        listed[0].availability,
        crate::app::LoupistAvailability::Available
    );
    assert_eq!(state.store.get_chat(&chat.id).unwrap(), Some(chat));
    assert_eq!(folder_versions(&root), CURRENT_VERSIONS);
    let [backup] = upgrade_backups(&root).try_into().unwrap();
    let backup = rusqlite::Connection::open(backup).unwrap();
    assert_eq!(
        backup
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        4
    );
}

/// Admission validates an upgraded copy without writing to the folder; only
/// registration upgrades it, after a backup.
#[tokio::test]
async fn a_baseline_folder_is_validated_read_only_and_upgraded_when_imported() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("research");
    fs::create_dir(&root).unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let loupist = create_folder_loupist(&state, &root).await.unwrap();
    let chat = super::tests::chat("chat-imported", &loupist.id, 10);
    state.store.create_chat(&chat).unwrap();
    delete_loupist_inner(&state, &loupist.id).await.unwrap();
    crate::store::restore_baseline_for_test(
        &mut rusqlite::Connection::open(folder_database(&root)).unwrap(),
    );
    let bytes = fs::read(folder_database(&root)).unwrap();

    // The picker hands admission a canonical folder path.
    let workspace =
        crate::fs::loupist_root::BoundLoupistRoot::open_unbound(root.canonicalize().unwrap())
            .unwrap();
    let admitted = state.store.folder_profile(&workspace).unwrap().unwrap();
    assert_eq!(admitted.id, loupist.id);
    assert_eq!(fs::read(folder_database(&root)).unwrap(), bytes);
    assert!(upgrade_backups(&root).is_empty());

    let reopened = create_folder_loupist(&state, &root).await.unwrap();
    assert_eq!(reopened.id, loupist.id);
    assert_eq!(folder_versions(&root), CURRENT_VERSIONS);
    assert_eq!(upgrade_backups(&root).len(), 1);
    assert!(state.store.get_chat(&chat.id).unwrap().is_some());
}

/// A folder a newer build saved is its own state, not a moved folder, and it
/// is never written: listing, binding storage and reopening all refuse it.
#[tokio::test]
async fn a_folder_saved_by_a_newer_build_is_refused_untouched() {
    use crate::app::LoupistAvailability;
    let temp = tempfile::tempdir().unwrap();
    let current_root = temp.path().join("current");
    let newer_root = temp.path().join("newer");
    fs::create_dir(&current_root).unwrap();
    fs::create_dir(&newer_root).unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let current = create_folder_loupist(&state, &current_root).await.unwrap();
    let newer = create_folder_loupist(&state, &newer_root).await.unwrap();
    state.close_for_restart_test();
    rusqlite::Connection::open(folder_database(&newer_root))
        .unwrap()
        .execute(
            "UPDATE knowledge_schema SET version = ?1",
            [crate::knowledge::KNOWLEDGE_SCHEMA_VERSION + 1],
        )
        .unwrap();
    let bytes = fs::read(folder_database(&newer_root)).unwrap();

    let state = AppState::for_folder_test(temp.path().join("app"));
    let listed = list_available_loupists(&state).await.unwrap();
    let availability = |id: &str| {
        listed
            .iter()
            .find(|summary| summary.id == id)
            .unwrap()
            .availability
    };
    assert_eq!(availability(&current.id), LoupistAvailability::Available);
    assert_eq!(availability(&newer.id), LoupistAvailability::NewerVersion);
    assert_eq!(
        serde_json::to_value(availability(&newer.id)).unwrap(),
        serde_json::json!({"status": "newer_version"})
    );
    assert!(matches!(
        state.store.lock_loupist(&newer.id),
        Err(crate::store::StoreError::NewerVersion)
    ));
    let refused =
        crate::commands::assets::with_assets(&state, &newer.id, true, |_| Ok(())).unwrap_err();
    assert_eq!(refused.code, "loupist_newer_version");
    delete_loupist_inner(&state, &newer.id).await.unwrap();
    let reopened = create_folder_loupist(&state, &newer_root)
        .await
        .unwrap_err();
    assert_eq!(reopened.code, "loupist_newer_version");
    assert_eq!(fs::read(folder_database(&newer_root)).unwrap(), bytes);
    assert!(upgrade_backups(&newer_root).is_empty());
}

/// A pre-ladder store schema is typed on import and reconnect, as on listing.
#[tokio::test]
async fn an_earlier_store_schema_is_refused_as_an_earlier_version_on_reconnect_and_import() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("research");
    fs::create_dir(&root).unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let loupist = create_folder_loupist(&state, &root).await.unwrap();
    state.close_for_restart_test();
    let moved = temp.path().join("moved");
    fs::rename(&root, &moved).unwrap();
    rusqlite::Connection::open(folder_database(&moved))
        .unwrap()
        .execute_batch(&format!(
            "PRAGMA user_version = {};",
            crate::store::STORE_BASELINE_VERSION - 1
        ))
        .unwrap();

    let state = AppState::for_folder_test(temp.path().join("app"));
    let reconnected = reconnect_loupist_inner(
        &state,
        &LoupistReconnectRequest {
            loupist_id: loupist.id.clone(),
            workspace_path: moved.to_string_lossy().into(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(reconnected.code, "loupist_earlier_version");
    delete_loupist_inner(&state, &loupist.id).await.unwrap();
    let imported = create_folder_loupist(&state, &moved).await.unwrap_err();
    assert_eq!(imported.code, "loupist_earlier_version");
}

/// Cross-Loupist reads skip one broken database and return the others;
/// writes still stop at the first failure.
#[tokio::test]
async fn a_scan_skips_one_broken_folder_and_returns_the_healthy_ones() {
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let mut roots = Vec::new();
    for name in ["first", "broken", "third"] {
        let root = temp.path().join(name);
        fs::create_dir(&root).unwrap();
        create_folder_loupist(&state, &root).await.unwrap();
        roots.push(root);
    }
    state.close_for_restart_test();
    rusqlite::Connection::open(folder_database(&roots[1]))
        .unwrap()
        .execute_batch("DROP TABLE knowledge_saved_price_reads;")
        .unwrap();

    let state = AppState::for_folder_test(temp.path().join("app"));
    let scan = |_: &str, connection: &mut rusqlite::Connection| {
        Ok(connection.query_row(
            "SELECT COUNT(*) FROM knowledge_saved_price_reads",
            [],
            |row| row.get::<_, i64>(0),
        )?)
    };
    assert_eq!(
        state.store.for_each_readable_database(scan).unwrap(),
        vec![0, 0]
    );
    assert!(state.store.for_each_database(scan).is_err());
}

#[tokio::test]
async fn removed_managed_loupist_reopens_through_import_with_chats_and_memory() {
    use tauri::Manager;
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let loupist = create_managed_loupist(
        &state,
        super::types::LoupistCreateRequest {
            name: "Research desk".into(),
            focus: Some("Cash flow".into()),
            avatar: None,
            template: None,
            tone: None,
        },
    )
    .await
    .unwrap();
    let chat = super::tests::chat("chat-kept", &loupist.id, 10);
    state.store.create_chat(&chat).unwrap();
    let memory = KnowledgeStore::for_loupist(state.store.clone(), &loupist.id).unwrap();
    let source = retain(&memory, &loupist.id, "Retained research");
    drop(memory);
    delete_loupist_inner(&state, &loupist.id).await.unwrap();
    assert!(state.store.list_loupists().unwrap().is_empty());
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    app.manage(state);
    let reopened = loupist_import(
        super::types::LoupistImportRequest {
            workspace_path: loupist.workspace_path.clone(),
        },
        app.state(),
    )
    .await
    .unwrap();
    assert_eq!(reopened.id, loupist.id);
    let state = app.state::<AppState>();
    assert_eq!(state.store.get_chat(&chat.id).unwrap().unwrap(), chat);
    let memory = KnowledgeStore::for_loupist(state.store.clone(), &loupist.id).unwrap();
    assert_eq!(
        memory
            .read(|write| write.get_revision(&loupist.id, &source.revision_id))
            .unwrap()
            .record_id,
        source.record_id
    );
    // Private profile data and a runtime directory stay inadmissible.
    for path in [
        state.artifacts.app_data_dir.clone(),
        state
            .artifacts
            .app_data_dir
            .join("loupists")
            .join(&loupist.id),
    ] {
        assert!(loupist_import(
            super::types::LoupistImportRequest {
                workspace_path: path.to_str().unwrap().into()
            },
            app.state()
        )
        .await
        .is_err());
    }
}

#[tokio::test]
async fn import_does_not_turn_a_private_runtime_directory_into_a_workspace() {
    use tauri::Manager;
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let loupist = create_managed_loupist(
        &state,
        super::types::LoupistCreateRequest {
            name: "Research".into(),
            focus: None,
            avatar: None,
            template: None,
            tone: None,
        },
    )
    .await
    .unwrap();
    delete_loupist_inner(&state, &loupist.id).await.unwrap();
    let runtime = state
        .artifacts
        .app_data_dir
        .join("loupists")
        .join(&loupist.id);
    fs::rename(&loupist.workspace_path, &runtime).unwrap();
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    app.manage(state);
    assert!(loupist_import(
        super::types::LoupistImportRequest {
            workspace_path: runtime.to_str().unwrap().into()
        },
        app.state()
    )
    .await
    .is_err());
    assert!(runtime.join(".loupist/loupist.sqlite3").is_file());
}
