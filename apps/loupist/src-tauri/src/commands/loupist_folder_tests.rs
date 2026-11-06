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
    store::LoupistStore,
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
    // A focus files the perspective page: one Memory record before any chat.
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
    assert_eq!(record_count(&state, &b.id).await, 1);
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

/// Pre-1.0 folders are never migrated: a database an earlier build made is
/// refused with a typed state on every path (listing, storage, reopening), and
/// a fresh folder keeps working with asset pages.
#[tokio::test]
async fn a_folder_made_by_an_earlier_build_is_refused_with_a_typed_state() {
    use crate::app::LoupistAvailability;
    let temp = tempfile::tempdir().unwrap();
    let fresh = temp.path().join("fresh");
    let old = temp.path().join("old");
    fs::create_dir(&fresh).unwrap();
    fs::create_dir(&old).unwrap();
    let state = AppState::for_folder_test(temp.path().join("app"));
    let current = create_folder_loupist(&state, &fresh).await.unwrap();
    let earlier = create_folder_loupist(&state, &old).await.unwrap();
    state.close_for_restart_test();
    // Rewrite one folder as the previous build left it: Memory schema 1 and no
    // asset page tables.
    {
        let connection = rusqlite::Connection::open(old.join(".loupist/loupist.sqlite3")).unwrap();
        connection
            .execute_batch(
                "DROP TABLE knowledge_asset_pages; DROP TABLE knowledge_asset_observations;
                 DROP TABLE knowledge_asset_moves; DROP TABLE knowledge_asset_section_deltas;
                 UPDATE knowledge_schema SET version = 1;",
            )
            .unwrap();
    }
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
    assert_eq!(
        availability(&earlier.id),
        LoupistAvailability::EarlierVersion
    );
    assert_eq!(
        serde_json::to_value(availability(&earlier.id)).unwrap(),
        serde_json::json!({"status": "earlier_version"})
    );
    assert!(matches!(
        state.store.lock_loupist(&earlier.id),
        Err(crate::store::StoreError::EarlierVersion)
    ));
    // Asset work, turns and briefings all bind storage first, so none half-works.
    let refused =
        crate::commands::assets::with_assets(&state, &earlier.id, true, |_| Ok(())).unwrap_err();
    assert_eq!(refused.code, "loupist_earlier_version");
    let identity = crate::knowledge::assets::AssetIdentity {
        symbol: "MORR".into(),
        market: "NASDAQ".into(),
        name: "Morrow Components".into(),
    };
    crate::commands::assets::with_assets(&state, &current.id, true, |tx| {
        crate::knowledge::assets::ensure_page(tx, &current.id, &identity, None, 1)
    })
    .unwrap();
    // Reopening the earlier folder elsewhere is refused the same way.
    delete_loupist_inner(&state, &earlier.id).await.unwrap();
    let reopened = create_folder_loupist(&state, &old).await.unwrap_err();
    assert_eq!(reopened.code, "loupist_earlier_version");
    assert!(reopened.message.contains("earlier version"));
}
