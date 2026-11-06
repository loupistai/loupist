use super::*;

#[tokio::test]
async fn folder_loupist_creation_preserves_files_and_seeds_memory_and_tools() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("research");
    fs::create_dir(&workspace).unwrap();
    fs::write(workspace.join("analysis.py"), "print(42)\n").unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    let summary = crate::commands::loupist::create_folder_loupist_with_setup(
        &state,
        &workspace,
        Some("Durable cash compounders".into()),
        None,
    )
    .await
    .unwrap();
    let saved = state.store.get_loupist(&summary.id).unwrap().unwrap();
    assert_eq!(summary.name, "research");
    assert_eq!(
        Path::new(&saved.workspace_root),
        workspace.canonicalize().unwrap()
    );
    assert_eq!(summary.availability, LoupistAvailability::Available);
    assert_eq!(
        fs::read_to_string(workspace.join("analysis.py")).unwrap(),
        "print(42)\n"
    );
    assert_eq!(fs::read_dir(&workspace).unwrap().count(), 1);
    assert!(!state
        .artifacts
        .app_data_dir
        .join("loupists")
        .join(&saved.id)
        .join("workspace")
        .exists());
    assert!(state
        .store
        .list_loupist_capabilities(&saved.id)
        .unwrap()
        .iter()
        .any(|binding| binding.enabled));
    assert_eq!(
        summary.enabled_skill_ids,
        agent_harness::default_skill_ids()
            .into_iter()
            .filter(|id| agent_harness::skill_default_enabled(id))
            .collect::<Vec<_>>()
    );
    assert!(
        summary.record_count > 0,
        "the perspective filed from the focus is retained outside the user folder"
    );
}

#[tokio::test]
async fn disconnected_loupist_can_be_deleted_without_touching_moved_or_replacement_folders() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("research");
    fs::create_dir(&workspace).unwrap();
    fs::write(workspace.join("keep.txt"), "original").unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    let summary = create_folder_loupist(&state, &workspace).await.unwrap();
    let moved = temporary.path().join("moved");
    fs::rename(&workspace, &moved).unwrap();
    fs::create_dir(&workspace).unwrap();
    fs::write(workspace.join("keep.txt"), "replacement").unwrap();
    fs::remove_dir_all(
        state
            .artifacts
            .app_data_dir
            .join("loupists")
            .join(&summary.id),
    )
    .unwrap();
    delete_loupist_inner(&state, &summary.id).await.unwrap();
    assert_eq!(
        fs::read_to_string(moved.join("keep.txt")).unwrap(),
        "original"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("keep.txt")).unwrap(),
        "replacement"
    );
    assert!(state.store.get_loupist(&summary.id).unwrap().is_none());
}

#[tokio::test]
async fn update_maintenance_rejects_loupist_deletion_before_native_mutation() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    let _update = state.maintenance.begin_update().unwrap();
    assert_eq!(
        delete_loupist_inner(&state, "loupist-delete")
            .await
            .unwrap_err()
            .code,
        "maintenance_active"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn folder_binding_rejects_symlinks_and_replaced_roots() {
    use crate::fs::loupist_root::profile_workspace;
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("research");
    fs::create_dir(&workspace).unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    let link = temporary.path().join("link");
    std::os::unix::fs::symlink(&workspace, &link).unwrap();
    assert!(create_folder_loupist(&state, &link).await.is_err());
    let saved = profile("loupist-a", &workspace, 1);
    assert!(profile_workspace(&saved).is_ok());
    fs::rename(&workspace, temporary.path().join("original")).unwrap();
    fs::create_dir(&workspace).unwrap();
    assert!(profile_workspace(&saved).is_err());
}

#[test]
fn local_skill_preferences_are_used_without_database_bindings() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    let profile = profile("loupist-skills-off", &workspace, 1);
    seed_profile(state.store.as_ref(), &profile);

    let directory = loupist_config_directory(&state, &profile.id).unwrap();
    std::fs::write(
        directory.join("config.toml"),
        "[skills.research]\nenabled=false\n",
    )
    .unwrap();
    assert!(!loupist_configuration(&state, &profile.id)
        .unwrap()
        .enabled_skill_ids()
        .contains(&"research".to_string()));
}

#[test]
fn a_principle_set_aside_in_one_chat_stays_out_of_that_chat_until_undone() {
    use crate::knowledge::KnowledgeStore;
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    let workspace = temporary.path().join("loupist");
    fs::create_dir_all(&workspace).unwrap();
    seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
    state
        .store
        .create_chat(&chat("chat-a", "loupist-a", 1))
        .unwrap();
    let record = KnowledgeStore::for_loupist(state.store.clone(), "loupist-a")
        .unwrap()
        .file_method(
            "loupist-a",
            "Check cash flow",
            "Compare cash from operations with profit.",
            &crate::knowledge::test_support::user_origin(),
            1,
        )
        .unwrap();
    let request = |record_id: &str, set_aside: bool| super::types::ChatMemorySetAsideRequest {
        loupist_id: "loupist-a".into(),
        thread_id: "chat-a".into(),
        record_id: record_id.into(),
        set_aside,
    };
    let set = super::records::set_aside_memory(&state, request(&record.record_id, true)).unwrap();
    assert_eq!(
        set.set_aside_memory,
        std::slice::from_ref(&record.record_id)
    );
    // Setting it aside twice keeps one entry; an unknown record is refused.
    super::records::set_aside_memory(&state, request(&record.record_id, true)).unwrap();
    assert!(super::records::set_aside_memory(&state, request("lens-missing", true)).is_err());
    let stored = state.store.get_chat("chat-a").unwrap().unwrap();
    assert_eq!(
        stored.set_aside_memory,
        std::slice::from_ref(&record.record_id)
    );
    let undone =
        super::records::set_aside_memory(&state, request(&record.record_id, false)).unwrap();
    assert!(undone.set_aside_memory.is_empty());
}

#[test]
fn editing_a_sent_message_keeps_the_earlier_version_and_switches_back() {
    use crate::domain::ChatRole;
    use crate::test_fixtures::chat_message;
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    let workspace = temporary.path().join("loupist");
    fs::create_dir_all(&workspace).unwrap();
    seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
    let mut seeded = chat("chat-a", "loupist-a", 1);
    seeded.messages = vec![
        chat_message("q1", ChatRole::User, "Why did the margin fall?"),
        chat_message("a1", ChatRole::Assistant, "Qualification costs."),
        chat_message("q2", ChatRole::User, "Compare it with last year."),
        chat_message("a2", ChatRole::Assistant, "It fell by 1.4 points."),
    ];
    state.store.create_chat(&seeded).unwrap();
    let branch = |message_id: &str| super::types::ChatBranchRequest {
        loupist_id: "loupist-a".into(),
        thread_id: "chat-a".into(),
        message_id: message_id.into(),
        position: None,
        content: None,
    };

    // Editing q2 keeps q1/a1 and sets q2/a2 aside as an earlier version.
    let edited = super::records::branch_chat(&state, branch("q2")).unwrap();
    assert_eq!(
        edited
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        ["q1", "a1"]
    );
    assert_eq!(edited.versions.len(), 1);
    assert_eq!(edited.versions[0].after_message_id.as_deref(), Some("a1"));
    assert_eq!(edited.versions[0].message_count, 2);
    let stored = state.store.get_chat("chat-a").unwrap().unwrap();
    // The Pi transcript restarts cold from SQLite on the next turn.
    assert!(stored.pi_session_cache.is_none());
    // Only a message the user sent can be edited.
    assert!(super::records::branch_chat(&state, branch("a1")).is_err());
    // A message the renderer still knows by its own id is found by place and exact text only.
    let by_place = |content: &str| super::types::ChatBranchRequest {
        loupist_id: "loupist-a".into(),
        thread_id: "chat-a".into(),
        message_id: "client-id".into(),
        position: Some(0),
        content: Some(content.into()),
    };
    assert!(super::records::branch_chat(&state, by_place("A different question")).is_err());

    // The new version is sent, then the earlier one is brought back; the newer one is kept.
    let mut sent = stored.clone();
    sent.messages.push(chat_message(
        "q2b",
        ChatRole::User,
        "Compare it with two years ago.",
    ));
    sent.messages.push(chat_message(
        "a2b",
        ChatRole::Assistant,
        "It fell by 2.1 points.",
    ));
    state.store.replace_chat(&stored, &sent).unwrap();
    let earlier_id = edited.versions[0].id.clone();
    let switched = super::records::switch_version(
        &state,
        super::types::ChatVersionSwitchRequest {
            loupist_id: "loupist-a".into(),
            thread_id: "chat-a".into(),
            version_id: earlier_id,
        },
    )
    .unwrap();
    assert_eq!(
        switched
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        ["q1", "a1", "q2", "a2"]
    );
    assert_eq!(switched.versions.len(), 1);
    assert_eq!(switched.versions[0].after_message_id.as_deref(), Some("a1"));
    let stored = state.store.get_chat("chat-a").unwrap().unwrap();
    assert_eq!(stored.versions[0].messages[0].id, "q2b");
    // An unknown version is refused.
    assert!(super::records::switch_version(
        &state,
        super::types::ChatVersionSwitchRequest {
            loupist_id: "loupist-a".into(),
            thread_id: "chat-a".into(),
            version_id: "version-missing".into(),
        }
    )
    .is_err());
}
