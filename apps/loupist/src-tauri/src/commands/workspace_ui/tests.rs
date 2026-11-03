use super::*;
use crate::commands::tests::{chat, profile};
use serde_json::{json, Value};

fn fixture() -> (tempfile::TempDir, AppState) {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_persistent_test(temporary.path().join("profile"));
    for (loupist_id, thread_id) in [("loupist-a", "chat-a"), ("loupist-b", "chat-b")] {
        let workspace = temporary.path().join(loupist_id);
        std::fs::create_dir(&workspace).unwrap();
        state
            .store
            .create_loupist(&profile(loupist_id, &workspace, 1))
            .unwrap();
        state
            .store
            .create_chat(&chat(thread_id, loupist_id, 1))
            .unwrap();
    }
    (temporary, state)
}

fn preferences() -> WorkspaceUiState {
    serde_json::from_value(json!({
        "version": 1, "open": true, "placement": "bottom", "dock_by_kind": { "chart": "side" }, "width": 720.5, "height": 480,
        "sessions": [{ "loupist_id": "loupist-a", "thread_id": "chat-a", "active_tab_id": "memory:source", "tabs": [
            { "id": "files", "kind": "files", "title": "Files", "path": "reports", "filePath": "reports/result.md" },
            { "id": "memory:source", "kind": "memory", "title": "Source", "record_id": "source-a", "location": {
                "loupistId": "loupist-a", "surface": "source", "recordId": "source-a", "revisionId": "rev-a", "objectId": "span-a",
                "selection": { "kind": "snapshot", "pin": { "kind": "source", "loupistId": "loupist-a", "recordId": "source-a", "revisionId": "rev-a", "objectId": "source-a", "spanId": "span-a", "label": "Selected source" } }
            } },
            { "id": "work", "kind": "work", "artifact_thread_id": "chat-a", "artifact": { "artifact_id": "report", "revision": 1, "kind": "markdown", "title": "Report", "digest": "a".repeat(64) } },
            { "id": "browser:tab", "kind": "browser", "title": "Example", "url": "https://example.com/report" }
        ] }]
    })).unwrap()
}

fn file_path(state: &AppState) -> std::path::PathBuf {
    state.artifacts.app_data_dir.join(DIRECTORY).join(FILE_NAME)
}

#[test]
fn panel_geometry_and_all_tab_references_survive_profile_restart() {
    let (temporary, state) = fixture();
    assert!(read_state(&state).unwrap().is_none());
    assert!(!state.artifacts.app_data_dir.join(DIRECTORY).exists());
    let expected = preferences();
    write_state(&state, expected.clone()).unwrap();
    let mut changed = expected.clone();
    changed.open = false;
    write_state(&state, changed).unwrap();
    write_state(&state, expected.clone()).unwrap();
    let path = file_path(&state);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        1
    );
    state.close_for_restart_test();

    let reopened = AppState::for_persistent_test(temporary.path().join("profile"));
    let actual = read_state(&reopened).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(
        std::fs::read(path).unwrap(),
        bytes,
        "read must not rewrite preferences"
    );
    let isolated = AppState::for_test(temporary.path().join("other-profile"));
    assert!(read_state(&isolated).unwrap().is_none());
}

#[test]
fn every_read_and_write_rechecks_chat_ownership_and_removes_deleted_sessions() {
    let (_temporary, state) = fixture();
    let mut input = preferences();
    let mut wrong_owner = input.sessions[0].clone();
    wrong_owner.thread_id = "chat-b".into();
    let mut deleted = input.sessions[0].clone();
    deleted.thread_id = "chat-missing".into();
    input.sessions.extend([wrong_owner, deleted]);
    input.sessions[0].active_tab_id = Some("missing-tab".into());
    let stored = write_state(&state, input).unwrap();
    assert_eq!(stored.sessions.len(), 1);
    assert_eq!(stored.sessions[0].active_tab_id.as_deref(), Some("files"));
    let before = std::fs::read(file_path(&state)).unwrap();
    let chat = state.store.get_chat("chat-a").unwrap().unwrap();
    state.store.delete_chat(&chat).unwrap();
    let loaded = read_state(&state).unwrap().unwrap();
    assert!(loaded.sessions.is_empty());
    assert!(loaded.open);
    assert_eq!(std::fs::read(file_path(&state)).unwrap(), before);
}

#[test]
fn invalid_references_and_bounds_leave_the_previous_file_intact() {
    let (_temporary, state) = fixture();
    let initial = preferences();
    write_state(&state, initial.clone()).unwrap();
    let before = std::fs::read(file_path(&state)).unwrap();
    let value = serde_json::to_value(initial).unwrap();
    let changes = [
        ("/version", json!(2)),
        ("/width", json!(359)),
        ("/height", json!(721)),
        ("/sessions/0/tabs/0/path", json!("../outside")),
        ("/sessions/0/tabs/0/filePath", json!("/etc/passwd")),
        ("/sessions/0/tabs/0/title", json!("a".repeat(513))),
        ("/sessions/0/tabs/1/location/loupistId", json!("loupist-b")),
        (
            "/sessions/0/tabs/1/location/selection/pin/loupistId",
            json!("loupist-b"),
        ),
        // An open Work item must say which chat made it.
        ("/sessions/0/tabs/2/artifact_thread_id", json!(null)),
        ("/sessions/0/tabs/3/url", json!("file:///tmp/file")),
        (
            "/sessions/0/tabs/3/url",
            json!("https://user:password@example.com"),
        ),
    ];
    for (pointer, change) in changes {
        let mut input = value.clone();
        *input.pointer_mut(pointer).unwrap() = change;
        assert!(
            write_state(&state, serde_json::from_value(input).unwrap()).is_err(),
            "accepted {pointer}"
        );
        assert_eq!(std::fs::read(file_path(&state)).unwrap(), before);
    }
    let mut invalid_float = preferences();
    invalid_float.width = f64::NAN;
    assert!(write_state(&state, invalid_float).is_err());
    let mut duplicate = preferences();
    duplicate.sessions.push(duplicate.sessions[0].clone());
    assert!(write_state(&state, duplicate).is_err());
    let mut duplicate_tab = preferences();
    let tab = duplicate_tab.sessions[0].tabs[0].clone();
    duplicate_tab.sessions[0].tabs.push(tab);
    assert!(write_state(&state, duplicate_tab).is_err());
    // Limits apply before sessions owned by missing Chats are dropped.
    let mut too_many_sessions = preferences();
    too_many_sessions.sessions = vec![too_many_sessions.sessions[0].clone(); MAX_SESSIONS + 1];
    assert!(write_state(&state, too_many_sessions).is_err());
    let mut too_many_tabs = preferences();
    too_many_tabs.sessions[0].tabs = vec![too_many_tabs.sessions[0].tabs[0].clone(); MAX_TABS + 1];
    assert!(write_state(&state, too_many_tabs).is_err());
    let mut oversized = preferences();
    oversized.sessions = (0..4)
        .map(|session_index| WorkspaceUiSession {
            loupist_id: "loupist-a".into(),
            thread_id: format!("missing-{session_index}"),
            active_tab_id: None,
            dismissed: false,
            tabs: (0..MAX_TABS)
                .map(|index| WorkspaceUiTab::Browser {
                    id: format!("{index:02}{}", "a".repeat(8190)),
                    title: "Browser".into(),
                    url: String::new(),
                })
                .collect(),
        })
        .collect();
    assert!(write_state(&state, oversized).is_err());
    assert_eq!(std::fs::read(file_path(&state)).unwrap(), before);
}

#[test]
fn a_chat_closed_by_the_user_stays_dismissed_across_restart_without_tabs() {
    let (temporary, state) = fixture();
    let mut input = preferences();
    let mut closed = input.sessions[0].clone();
    closed.thread_id = "chat-a".into();
    closed.tabs.clear();
    closed.active_tab_id = None;
    closed.dismissed = true;
    input.sessions = vec![closed];
    let stored = write_state(&state, input).unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(file_path(&state)).unwrap()).unwrap();
    assert_eq!(saved["sessions"][0]["dismissed"], json!(true));
    assert_eq!(saved["sessions"][0]["tabs"], json!([]));
    state.close_for_restart_test();

    let reopened = AppState::for_persistent_test(temporary.path().join("profile"));
    let loaded = read_state(&reopened).unwrap().unwrap();
    assert!(loaded.sessions[0].dismissed);
    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(stored).unwrap()
    );
    // An open chat omits the flag, so earlier files without it still read.
    let open = preferences();
    let bytes = serde_json::to_vec(&open).unwrap();
    assert!(!String::from_utf8(bytes).unwrap().contains("dismissed"));
    let mut wrong_type: Value = serde_json::to_value(preferences()).unwrap();
    wrong_type["sessions"][0]["dismissed"] = json!("yes");
    assert!(serde_json::from_value::<WorkspaceUiState>(wrong_type).is_err());
}

#[test]
fn oversized_malformed_and_older_files_read_as_absent_and_are_replaced_by_the_next_write() {
    let (_temporary, state) = fixture();
    write_state(&state, preferences()).unwrap();
    let path = file_path(&state);
    std::fs::write(&path, vec![b' '; MAX_BYTES + 1]).unwrap();
    assert!(read_state(&state).unwrap().is_none());
    std::fs::write(&path, b"{truncated").unwrap();
    assert!(read_state(&state).unwrap().is_none());
    let mut unknown_version: Value = serde_json::to_value(preferences()).unwrap();
    unknown_version["version"] = json!(2);
    std::fs::write(&path, serde_json::to_vec(&unknown_version).unwrap()).unwrap();
    assert!(read_state(&state).unwrap().is_none());
    // The panel can save again: the next write replaces the unusable file.
    write_state(&state, preferences()).unwrap();
    assert!(read_state(&state).unwrap().is_some());
}

#[cfg(unix)]
#[test]
fn symlinked_preferences_cannot_read_or_overwrite_another_location() {
    use std::os::unix::fs::symlink;
    let (temporary, state) = fixture();
    let outside = temporary.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let outside_file = outside.join(FILE_NAME);
    std::fs::write(&outside_file, b"untouched").unwrap();
    let preferences_dir = state.artifacts.app_data_dir.join(DIRECTORY);
    symlink(&outside, &preferences_dir).unwrap();
    assert!(read_state(&state).is_err());
    assert!(write_state(&state, preferences()).is_err());
    assert_eq!(std::fs::read(&outside_file).unwrap(), b"untouched");

    std::fs::remove_file(&preferences_dir).unwrap();
    std::fs::create_dir(&preferences_dir).unwrap();
    symlink(&outside_file, file_path(&state)).unwrap();
    assert!(read_state(&state).is_err());
    write_state(&state, preferences()).unwrap();
    assert!(!std::fs::symlink_metadata(file_path(&state))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read(&outside_file).unwrap(), b"untouched");
    assert!(read_state(&state).unwrap().is_some());
}
