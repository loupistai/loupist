use std::{collections::HashMap, path::PathBuf};

use crate::{
    app::CommandError,
    commands::map_store,
    domain::{
        ChatSession, DeletionJournal, DeletionKind, DeletionPhase, LoupistProfile,
        RootFilesystemIdentity,
    },
    fs::secure_delete::SecureDeletionRoot,
    store::{DeletionJournalRecord, LoupistStore, SqliteStore},
};

#[derive(Debug)]
struct DeletionPath {
    source: PathBuf,
    tombstone: PathBuf,
    required: bool,
    expected_identity: Option<RootFilesystemIdentity>,
}

#[derive(Debug, Default)]
pub struct DeletionRecovery {
    pub quarantined_loupists: HashMap<String, String>,
}

pub fn delete_chat(
    store: &dyn LoupistStore,
    root: &SecureDeletionRoot,
    expected: &ChatSession,
    timestamp_ms: i64,
) -> Result<(), CommandError> {
    execute_deletion(
        store,
        root,
        None,
        chat_journal(expected, timestamp_ms),
        |store| store.delete_chat(expected),
    )
}

fn chat_journal(expected: &ChatSession, timestamp_ms: i64) -> DeletionJournal {
    DeletionJournal {
        id: format!("delete-chat-{}", expected.id),
        kind: DeletionKind::Chat,
        loupist_id: expected.loupist_id.clone(),
        target_id: expected.id.clone(),
        expected_source_identity: None,
        phase: DeletionPhase::Prepared,
        created_at_ms: timestamp_ms,
        updated_at_ms: timestamp_ms,
    }
}

/// Journal both roots before deleting canonical data so a crash between roots
/// cannot strand the private transcript cache without a recovery record.
pub(crate) fn delete_chat_with_runtime(
    store: &dyn LoupistStore,
    durable_root: &SecureDeletionRoot,
    runtime_root: &SecureDeletionRoot,
    chat: &ChatSession,
    timestamp_ms: i64,
) -> Result<(), CommandError> {
    execute_deletion(
        store,
        durable_root,
        Some(runtime_root),
        chat_journal(chat, timestamp_ms),
        |store| store.delete_chat(chat),
    )
}

pub fn delete_loupist(
    store: &dyn LoupistStore,
    root: &SecureDeletionRoot,
    expected: &LoupistProfile,
    timestamp_ms: i64,
) -> Result<(), CommandError> {
    let source = PathBuf::from("loupists").join(&expected.id);
    // Only app-owned runtime files belong to this deletion. A user workspace
    // can be moved, disconnected, or shared with other apps and is never removed.
    let source_identity = root.directory_identity(&source)?;
    if source_identity.is_some() && source_identity != expected.runtime_filesystem_identity {
        return Err(CommandError::conflict(
            "Loupist private runtime folder changed before deletion",
        ));
    }
    let journal = DeletionJournal {
        id: format!("delete-loupist-{}", expected.id),
        kind: DeletionKind::Loupist,
        loupist_id: expected.id.clone(),
        target_id: expected.id.clone(),
        expected_source_identity: source_identity,
        phase: DeletionPhase::Prepared,
        created_at_ms: timestamp_ms,
        updated_at_ms: timestamp_ms,
    };
    execute_deletion(store, root, None, journal, |store| {
        store.delete_loupist(expected)
    })
}

fn execute_deletion(
    store: &dyn LoupistStore,
    root: &SecureDeletionRoot,
    runtime_root: Option<&SecureDeletionRoot>,
    prepared: DeletionJournal,
    delete_target: impl FnOnce(&dyn LoupistStore) -> Result<(), crate::store::StoreError>,
) -> Result<(), CommandError> {
    prepared.validate().map_err(map_internal)?;
    store
        .create_deletion_journal(&prepared)
        .map_err(map_store)?;
    let paths = journal_paths(&prepared)?;
    if let Err(error) = detach_paths(root, &paths) {
        // Reconcile the complete path set even when the local rollback inside
        // `detach_paths` failed partway. The durable pointer may be removed only
        // after every required/live entry is proven restored.
        if rollback_paths(root, &paths).is_ok() {
            store
                .delete_deletion_journal(&prepared)
                .map_err(map_store)?;
        }
        return Err(error);
    }

    let mut detached = prepared.clone();
    detached.phase = DeletionPhase::Detached;
    detached.updated_at_ms = detached.updated_at_ms.saturating_add(1);
    if let Err(error) = store.replace_deletion_journal(&prepared, &detached) {
        if rollback_paths(root, &paths).is_ok() {
            store
                .delete_deletion_journal(&prepared)
                .map_err(map_store)?;
        }
        return Err(map_store(error));
    }

    if let Err(error) = delete_target(store) {
        if rollback_paths(root, &paths).is_ok() {
            store
                .delete_deletion_journal(&detached)
                .map_err(map_store)?;
        }
        return Err(map_store(error));
    }

    // Once SQLite commits absence, restoration would resurrect bytes without
    // authority. Cleanup therefore remains retryable in the durable journal.
    cleanup_paths(root, &paths)?;
    if let Some(runtime_root) = runtime_root {
        cleanup_runtime_paths(runtime_root, &chat_runtime_paths(&prepared))?;
    }
    store.delete_deletion_journal(&detached).map_err(map_store)
}

pub fn recover(
    store: &SqliteStore,
    root: &SecureDeletionRoot,
) -> Result<DeletionRecovery, CommandError> {
    let mut recovery = DeletionRecovery::default();
    let records = store.list_deletion_journals().map_err(map_store)?;
    for record in records {
        let journal = match record {
            DeletionJournalRecord::Valid(journal) => journal,
            DeletionJournalRecord::Invalid {
                id,
                loupist_id,
                reason,
            } => {
                let reason = format!("pending deletion journal {id} is invalid: {reason}");
                if store.get_loupist(&loupist_id).map_err(map_store)?.is_some() {
                    recovery.quarantined_loupists.insert(loupist_id, reason);
                }
                continue;
            }
        };
        if store.is_folder_catalog()
            && matches!(journal.kind, DeletionKind::Chat | DeletionKind::ChatRuntime)
        {
            // A missing/locked database is not proof that the Chat was deleted.
            if let Err(error) = store.lock_loupist(&journal.loupist_id) {
                recovery
                    .quarantined_loupists
                    .insert(journal.loupist_id.clone(), error.to_string());
                continue;
            }
        }
        let runtime_root = root;
        let durable_root;
        let root = if store.is_folder_catalog() && journal.kind == DeletionKind::Chat {
            match store.loupist_data_dir(&journal.loupist_id).and_then(|dir| {
                SecureDeletionRoot::open(&dir)
                    .map_err(|e| crate::store::StoreError::Invalid(e.to_string()))
            }) {
                Ok(resolved) => {
                    durable_root = resolved;
                    &durable_root
                }
                Err(error) => {
                    recovery
                        .quarantined_loupists
                        .insert(journal.loupist_id.clone(), error.to_string());
                    continue;
                }
            }
        } else {
            root
        };
        let paths = match journal_paths(&journal) {
            Ok(paths) => paths,
            Err(error) => {
                if store
                    .get_loupist(&journal.loupist_id)
                    .map_err(map_store)?
                    .is_some()
                {
                    recovery
                        .quarantined_loupists
                        .insert(journal.loupist_id.clone(), error.message);
                }
                continue;
            }
        };
        let target_exists = match journal.kind {
            DeletionKind::Chat | DeletionKind::ChatRuntime => {
                match store.get_chat(&journal.target_id) {
                    Ok(Some(chat)) if chat.loupist_id == journal.loupist_id => true,
                    Ok(Some(_)) => {
                        recovery.quarantined_loupists.insert(
                            journal.loupist_id.clone(),
                            "pending Chat deletion belongs to another Loupist".into(),
                        );
                        continue;
                    }
                    Ok(None) => false,
                    Err(error) => {
                        recovery
                            .quarantined_loupists
                            .insert(journal.loupist_id.clone(), error.to_string());
                        continue;
                    }
                }
            }
            DeletionKind::Loupist => match store.get_loupist(&journal.loupist_id) {
                Ok(profile) => profile.is_some(),
                Err(error) => {
                    recovery
                        .quarantined_loupists
                        .insert(journal.loupist_id.clone(), error.to_string());
                    continue;
                }
            },
        };

        let reconciled = if target_exists {
            rollback_paths(root, &paths)
        } else if journal.kind == DeletionKind::ChatRuntime {
            cleanup_runtime_paths(root, &paths)
        } else if store.is_folder_catalog() && journal.kind == DeletionKind::Chat {
            cleanup_paths(root, &paths)
                .and_then(|()| cleanup_runtime_paths(runtime_root, &chat_runtime_paths(&journal)))
        } else {
            cleanup_paths(root, &paths)
        };
        if let Err(error) = reconciled {
            if target_exists {
                recovery
                    .quarantined_loupists
                    .insert(journal.loupist_id.clone(), error.message);
            }
            // A committed deletion with cleanup failure remains absent and is
            // retried on a later startup; it must not block unrelated Loupists.
            continue;
        }
        if let Err(error) = store.delete_deletion_journal(&journal) {
            if target_exists {
                recovery
                    .quarantined_loupists
                    .insert(journal.loupist_id.clone(), error.to_string());
            }
        }
    }
    Ok(recovery)
}

pub fn has_pending_for(
    store: &dyn LoupistStore,
    loupist_id: &str,
    target_id: &str,
) -> Result<bool, CommandError> {
    Ok(store
        .list_deletion_journals()
        .map_err(map_store)?
        .into_iter()
        .any(|record| match record {
            DeletionJournalRecord::Valid(journal) => {
                journal.loupist_id == loupist_id && journal.target_id == target_id
            }
            DeletionJournalRecord::Invalid {
                loupist_id: stored_loupist,
                ..
            } => stored_loupist == loupist_id,
        }))
}

fn journal_paths(journal: &DeletionJournal) -> Result<Vec<DeletionPath>, CommandError> {
    journal.validate().map_err(map_internal)?;
    let mut paths = Vec::new();
    match journal.kind {
        DeletionKind::Chat => {
            let loupist = PathBuf::from("loupists").join(&journal.loupist_id);
            for parent in [
                loupist.join("pi-sessions"),
                loupist.join("pi-runtime"),
                loupist.join("attachments"),
                loupist.join("activities"),
            ] {
                paths.push(DeletionPath {
                    source: parent.join(&journal.target_id),
                    tombstone: parent.join(format!(".deleting-{}", journal.target_id)),
                    required: false,
                    expected_identity: None,
                });
            }
        }
        DeletionKind::ChatRuntime => {
            paths = chat_runtime_paths(journal);
        }
        DeletionKind::Loupist => {
            let loupists = PathBuf::from("loupists");
            paths.push(DeletionPath {
                source: loupists.join(&journal.loupist_id),
                tombstone: loupists.join(format!(".deleting-{}", journal.loupist_id)),
                required: journal.expected_source_identity.is_some(),
                expected_identity: journal.expected_source_identity.clone(),
            });
            let runs = PathBuf::from("runs");
            paths.push(DeletionPath {
                source: runs.join(&journal.loupist_id),
                tombstone: runs.join(format!(".deleting-{}", journal.loupist_id)),
                required: false,
                expected_identity: None,
            });
        }
    }
    Ok(paths)
}

fn chat_runtime_paths(journal: &DeletionJournal) -> Vec<DeletionPath> {
    let loupist = PathBuf::from("loupists").join(&journal.loupist_id);
    [loupist.join("pi-sessions"), loupist.join("pi-runtime")]
        .into_iter()
        .map(|parent| DeletionPath {
            source: parent.join(&journal.target_id),
            tombstone: parent.join(format!(".deleting-{}", journal.target_id)),
            required: false,
            expected_identity: None,
        })
        .collect()
}

fn detach_paths(root: &SecureDeletionRoot, paths: &[DeletionPath]) -> Result<(), CommandError> {
    let mut detached = Vec::new();
    for path in paths {
        if root.entry_exists(&path.tombstone)? {
            rollback_detached(root, &detached)?;
            return Err(CommandError::conflict(
                "private-storage deletion tombstone already exists",
            ));
        }
        match root.rename_sibling_expected(
            &path.source,
            &path.tombstone,
            path.expected_identity.as_ref(),
        ) {
            Ok(true) => detached.push(path),
            Ok(false) if !path.required => {}
            Ok(false) => {
                rollback_detached(root, &detached)?;
                return Err(CommandError::conflict(
                    "required private storage disappeared during deletion",
                ));
            }
            Err(error) => {
                rollback_detached(root, &detached)?;
                return Err(error);
            }
        }
    }
    Ok(())
}

fn rollback_detached(
    root: &SecureDeletionRoot,
    detached: &[&DeletionPath],
) -> Result<(), CommandError> {
    for path in detached.iter().rev() {
        if !root.rename_sibling_expected(
            &path.tombstone,
            &path.source,
            path.expected_identity.as_ref(),
        )? {
            return Err(CommandError::internal(
                "deletion rollback lost a detached private-storage entry",
            ));
        }
    }
    Ok(())
}

fn rollback_paths(root: &SecureDeletionRoot, paths: &[DeletionPath]) -> Result<(), CommandError> {
    for path in paths.iter().rev() {
        let live_exists = root.entry_exists(&path.source)?;
        let tombstone_exists = root.entry_exists(&path.tombstone)?;
        if live_exists {
            if let Some(expected) = &path.expected_identity {
                let observed = root.directory_identity(&path.source)?.ok_or_else(|| {
                    CommandError::internal(
                        "required private storage disappeared during identity recovery",
                    )
                })?;
                if &observed != expected {
                    return Err(CommandError::conflict(
                        "required private storage identity changed during recovery",
                    ));
                }
            }
        }
        match (live_exists, tombstone_exists) {
            (true, true) => {
                return Err(CommandError::internal(
                    "deletion rollback found both live and tombstone storage",
                ));
            }
            (false, true) => {
                if !root.rename_sibling_expected(
                    &path.tombstone,
                    &path.source,
                    path.expected_identity.as_ref(),
                )? {
                    return Err(CommandError::internal(
                        "deletion rollback lost a tombstone storage entry",
                    ));
                }
            }
            (false, false) if path.required => {
                return Err(CommandError::internal(
                    "required private storage is missing during deletion recovery",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

fn cleanup_paths(root: &SecureDeletionRoot, paths: &[DeletionPath]) -> Result<(), CommandError> {
    for path in paths {
        root.remove_tree_expected(&path.tombstone, path.expected_identity.as_ref())?;
    }
    Ok(())
}

fn cleanup_runtime_paths(
    root: &SecureDeletionRoot,
    paths: &[DeletionPath],
) -> Result<(), CommandError> {
    // Canonical absence has already committed. An interrupted runtime cleanup
    // can leave live paths, tombstones or neither; each path is retryable.
    for path in paths {
        if root.entry_exists(&path.source)? {
            if root.entry_exists(&path.tombstone)? {
                return Err(CommandError::conflict(
                    "runtime deletion found both live and tombstone storage",
                ));
            }
            root.rename_sibling_expected(
                &path.source,
                &path.tombstone,
                path.expected_identity.as_ref(),
            )?;
        }
        root.remove_tree_expected(&path.tombstone, path.expected_identity.as_ref())?;
    }
    Ok(())
}

fn map_internal(error: impl std::fmt::Display) -> CommandError {
    CommandError::internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        fs::artifact_trust::ensure_private_directory,
        store::SqliteStore,
        test_fixtures::{chat_session as chat, loupist_profile},
    };
    use std::fs;

    fn root(path: &std::path::Path) -> SecureDeletionRoot {
        SecureDeletionRoot::open(&path.canonicalize().unwrap()).unwrap()
    }

    fn tree_contains(path: &std::path::Path, needle: &[u8]) -> bool {
        let Ok(entries) = fs::read_dir(path) else {
            return false;
        };
        entries.filter_map(Result::ok).any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                tree_contains(&path, needle)
            } else {
                fs::read(path)
                    .is_ok_and(|bytes| bytes.windows(needle.len()).any(|window| window == needle))
            }
        })
    }

    fn seed_loupist(store: &SqliteStore, app_data: &std::path::Path, id: &str) -> LoupistProfile {
        let workspace_relative = PathBuf::from("user-folders").join(id);
        ensure_private_directory(&app_data.join(&workspace_relative)).unwrap();
        ensure_private_directory(&app_data.join("loupists").join(id)).unwrap();
        let deletion_root = root(app_data);
        let profile = LoupistProfile {
            workspace_root: app_data.join(&workspace_relative).display().to_string(),
            runtime_filesystem_identity: deletion_root
                .directory_identity(&PathBuf::from("loupists").join(id))
                .unwrap(),
            root_filesystem_identity: deletion_root
                .directory_identity(&workspace_relative)
                .unwrap(),
            ..loupist_profile(id)
        };
        store.create_loupist(&profile).unwrap();
        profile
    }

    #[test]
    fn chat_deletion_removes_crashed_scratch_and_attachments_before_finishing_journal() {
        let temporary = tempfile::tempdir().unwrap();
        let root = root(temporary.path());
        let store = SqliteStore::open_in_memory().unwrap();
        seed_loupist(&store, temporary.path(), "loupist-a");
        let chat = chat("chat-a", "loupist-a");
        store.create_chat(&chat).unwrap();
        let owned = [
            "loupists/loupist-a/pi-sessions/chat-a",
            "loupists/loupist-a/pi-runtime/chat-a",
            "loupists/loupist-a/attachments/chat-a",
            "loupists/loupist-a/activities/chat-a",
        ];
        for relative in owned {
            ensure_private_directory(&temporary.path().join(relative)).unwrap();
            fs::write(temporary.path().join(relative).join("secret"), b"secret").unwrap();
        }

        delete_chat(&store, &root, &chat, 10).unwrap();

        assert!(store.get_chat("chat-a").unwrap().is_none());
        assert!(store.list_deletion_journals().unwrap().is_empty());
        for relative in owned {
            assert!(!temporary.path().join(relative).exists(), "{relative}");
        }

        // A fresh chat may have none of its optional storage parents.
        let empty = crate::test_fixtures::chat_session("chat-empty", "loupist-a");
        store.create_chat(&empty).unwrap();
        delete_chat(&store, &root, &empty, 11).unwrap();
        assert!(store.get_chat("chat-empty").unwrap().is_none());
        assert!(store.list_deletion_journals().unwrap().is_empty());
    }

    #[test]
    fn runtime_recovery_handles_interruption_on_either_side_of_canonical_deletion() {
        for canonical_deleted in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let directory = temporary.path().canonicalize().unwrap();
            let runtime_root = root(&directory);
            let database = directory.join("catalog.sqlite3");
            let store = SqliteStore::open_catalog(&database).unwrap();
            seed_loupist(&store, &directory, "loupist-a");
            let chat = chat("chat-a", "loupist-a");
            store.create_chat(&chat).unwrap();
            let cache = directory.join("loupists/loupist-a/pi-sessions/chat-a");
            ensure_private_directory(&cache).unwrap();
            fs::write(cache.join("session.jsonl"), "private transcript").unwrap();

            store
                .create_deletion_journal(&chat_journal(&chat, 10))
                .unwrap();
            if canonical_deleted {
                store.delete_chat(&chat).unwrap();
            }
            drop(store);

            let restarted = SqliteStore::open_catalog(database).unwrap();
            let recovery = recover(&restarted, &runtime_root).unwrap();
            assert!(recovery.quarantined_loupists.is_empty(), "{recovery:?}");
            assert_eq!(cache.exists(), !canonical_deleted);
            assert_eq!(
                restarted.get_chat(&chat.id).unwrap().is_some(),
                !canonical_deleted
            );
            assert!(restarted.list_deletion_journals().unwrap().is_empty());
        }
    }

    #[test]
    fn runtime_cleanup_failure_keeps_the_canonical_deletion_journal_until_recovery() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().canonicalize().unwrap();
        let runtime_root = root(&directory);
        let store = SqliteStore::open_catalog(directory.join("catalog.sqlite3")).unwrap();
        seed_loupist(&store, &directory, "loupist-a");
        let durable_root = root(&store.loupist_data_dir("loupist-a").unwrap());
        let chat = chat("chat-a", "loupist-a");
        store.create_chat(&chat).unwrap();
        let live = PathBuf::from("loupists/loupist-a/pi-sessions/chat-a");
        let tombstone = PathBuf::from("loupists/loupist-a/pi-sessions/.deleting-chat-a");
        runtime_root.ensure_private_subdirectory(&live).unwrap();
        runtime_root
            .ensure_private_subdirectory(&tombstone)
            .unwrap();

        assert!(delete_chat_with_runtime(&store, &durable_root, &runtime_root, &chat, 10).is_err());
        assert!(store.get_chat(&chat.id).unwrap().is_none());
        assert_eq!(store.list_deletion_journals().unwrap().len(), 1);
        assert!(runtime_root.entry_exists(&live).unwrap());

        runtime_root.remove_tree_expected(&tombstone, None).unwrap();
        assert!(recover(&store, &runtime_root)
            .unwrap()
            .quarantined_loupists
            .is_empty());
        assert!(!runtime_root.entry_exists(&live).unwrap());
        assert!(store.list_deletion_journals().unwrap().is_empty());
    }

    #[test]
    fn loupist_deletion_removes_loupist_owned_run_scratch() {
        let temporary = tempfile::tempdir().unwrap();
        let root = root(temporary.path());
        let store = SqliteStore::open_in_memory().unwrap();
        let profile = seed_loupist(&store, temporary.path(), "loupist-a");
        let scratch = temporary.path().join("runs/loupist-a/stale-run");
        ensure_private_directory(&scratch).unwrap();
        fs::write(scratch.join("SKILL.md"), b"unique-sensitive-skill-marker").unwrap();
        let workspace = temporary
            .path()
            .join("loupists/loupist-a/activities/chat-stale");
        ensure_private_directory(&workspace).unwrap();
        fs::write(workspace.join("notes.txt"), b"unique-activity-marker").unwrap();

        delete_loupist(&store, &root, &profile, 10).unwrap();

        assert!(store.get_loupist("loupist-a").unwrap().is_none());
        assert!(!temporary.path().join("runs/loupist-a").exists());
        assert!(!temporary
            .path()
            .join("loupists/loupist-a/activities")
            .exists());
        assert!(!tree_contains(
            temporary.path(),
            b"unique-sensitive-skill-marker"
        ));
        assert!(!tree_contains(temporary.path(), b"unique-activity-marker"));
    }

    #[test]
    fn loupist_deletion_refuses_a_swapped_second_loupist_tree() {
        let temporary = tempfile::tempdir().unwrap();
        let root = root(temporary.path());
        let store = SqliteStore::open_in_memory().unwrap();
        let profile_a = seed_loupist(&store, temporary.path(), "loupist-a");
        seed_loupist(&store, temporary.path(), "loupist-b");
        fs::write(
            temporary
                .path()
                .join("loupists/loupist-b/second-loupist-sentinel"),
            b"keep",
        )
        .unwrap();
        fs::rename(
            temporary.path().join("loupists/loupist-a"),
            temporary.path().join("loupists/loupist-a-original"),
        )
        .unwrap();
        fs::rename(
            temporary.path().join("loupists/loupist-b"),
            temporary.path().join("loupists/loupist-a"),
        )
        .unwrap();

        assert!(delete_loupist(&store, &root, &profile_a, 10).is_err());
        assert_eq!(
            fs::read(
                temporary
                    .path()
                    .join("loupists/loupist-a/second-loupist-sentinel")
            )
            .unwrap(),
            b"keep"
        );
        assert!(store.get_loupist("loupist-a").unwrap().is_some());
        assert!(store.list_deletion_journals().unwrap().is_empty());
    }

    #[test]
    fn recovery_rolls_back_one_live_target_and_continues_cleaning_another() {
        let temporary = tempfile::tempdir().unwrap();
        let root = root(temporary.path());
        let store = SqliteStore::open_in_memory().unwrap();
        seed_loupist(&store, temporary.path(), "loupist-a");
        let chat = chat("chat-live", "loupist-a");
        store.create_chat(&chat).unwrap();
        let live_journal = DeletionJournal {
            id: "delete-chat-chat-live".into(),
            kind: DeletionKind::Chat,
            loupist_id: "loupist-a".into(),
            target_id: "chat-live".into(),
            expected_source_identity: None,
            phase: DeletionPhase::Detached,
            created_at_ms: 1,
            updated_at_ms: 2,
        };
        let absent_journal = DeletionJournal {
            id: "delete-chat-chat-absent".into(),
            kind: DeletionKind::Chat,
            loupist_id: "loupist-b".into(),
            target_id: "chat-absent".into(),
            expected_source_identity: None,
            phase: DeletionPhase::Detached,
            created_at_ms: 1,
            updated_at_ms: 2,
        };
        store.create_deletion_journal(&live_journal).unwrap();
        store.create_deletion_journal(&absent_journal).unwrap();
        for relative in [
            "loupists/loupist-a/pi-sessions/.deleting-chat-live",
            "loupists/loupist-b/pi-sessions/.deleting-chat-absent",
        ] {
            ensure_private_directory(&temporary.path().join(relative)).unwrap();
        }

        let recovery = recover(&store, &root).unwrap();

        assert!(recovery.quarantined_loupists.is_empty());
        assert!(temporary
            .path()
            .join("loupists/loupist-a/pi-sessions/chat-live")
            .is_dir());
        assert!(!temporary
            .path()
            .join("loupists/loupist-b/pi-sessions/.deleting-chat-absent")
            .exists());
        assert!(store.list_deletion_journals().unwrap().is_empty());
    }

    #[test]
    fn required_loupist_storage_missing_is_quarantined_without_blocking_other_recovery() {
        let temporary = tempfile::tempdir().unwrap();
        let root = root(temporary.path());
        let store = SqliteStore::open_in_memory().unwrap();
        let profile = LoupistProfile {
            workspace_root: temporary
                .path()
                .join("user-folders/loupist-bad")
                .display()
                .to_string(),
            root_filesystem_identity: None,
            ..loupist_profile("loupist-bad")
        };
        store.create_loupist(&profile).unwrap();
        let journal = DeletionJournal {
            id: "delete-loupist-loupist-bad".into(),
            kind: DeletionKind::Loupist,
            loupist_id: "loupist-bad".into(),
            target_id: "loupist-bad".into(),
            expected_source_identity: Some(RootFilesystemIdentity {
                device: 1,
                inode: 1,
            }),
            phase: DeletionPhase::Detached,
            created_at_ms: 1,
            updated_at_ms: 2,
        };
        store.create_deletion_journal(&journal).unwrap();

        let recovery = recover(&store, &root).unwrap();

        assert!(recovery.quarantined_loupists.contains_key("loupist-bad"));
        assert_eq!(store.list_deletion_journals().unwrap().len(), 1);
    }
}
