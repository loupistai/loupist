use super::*;
use crate::commands::tests::{chat, profile};
use serde_json::{json, Value};
use tauri::Manager;

fn stored_draft_round_trip(value: Value) -> Value {
    let (_temporary, state) = fixture();
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    app.manage(state);
    let identity = || DraftIdentity {
        loupist_id: "loupist-a".into(),
        thread_id: "chat-a".into(),
    };
    chat_draft_write(
        app.state(),
        identity(),
        serde_json::from_value(value).unwrap(),
    )
    .unwrap();
    let restored = chat_draft_read(app.state(), identity()).unwrap().unwrap();
    let expected = serde_json::to_value(&restored).unwrap();
    chat_draft_write(app.state(), identity(), restored).unwrap();
    assert_eq!(
        serde_json::to_value(chat_draft_read(app.state(), identity()).unwrap().unwrap()).unwrap(),
        expected
    );
    expected
}

fn inline_reference(prompt: &str, text: &str, key: &str, kind: &str) -> Value {
    let start = prompt[..prompt.find(text).unwrap()].encode_utf16().count();
    json!({ "key": key, "kind": kind, "text": text, "start": start, "end": start + text.encode_utf16().count() })
}

#[test]
fn restored_ticker_draft_keeps_inline_text_and_supported_context() {
    let prompt = "😀 Compare $Apple · AAPL and $Bitcoin · BTC with @report and ⟦Source⟧.";
    let file = inline_reference(prompt, "@report", "chip:file:report", "file");
    let source = inline_reference(prompt, "⟦Source⟧", "pin:source", "source");
    let supported = json!({
        "pins": [{ "kind": "source", "loupistId": "loupist-a", "objectId": "source", "label": "Source" }],
        "chips": [
            { "key": "skill:research", "kind": "skill", "id": "research", "label": "Research" },
            { "key": "connection:market", "kind": "connection", "id": "market", "label": "Market" },
            { "key": "file:report", "kind": "file", "id": "report.md", "label": "report" },
            { "key": "quote:answer", "kind": "quote", "id": "answer", "label": "Margin", "text": "Margin increased." }
        ],
        "inlineReferences": [file, source],
        "asOf": "2026-10-01",
        "artifact": { "artifact_id": "report", "revision": 1, "kind": "markdown", "title": "Report", "digest": "a".repeat(64) }
    });
    let mut context = supported.clone();
    context["chips"].as_array_mut().unwrap().extend([
        json!({ "key": "company:AAPL", "kind": "company", "id": "AAPL:NASDAQ", "label": "Apple · AAPL", "text": "Apple" }),
        json!({ "key": "company:BTC", "kind": "company", "id": "BTC:CRYPTO", "label": "Bitcoin · BTC", "text": "Bitcoin" }),
    ]);
    context["inlineReferences"].as_array_mut().unwrap().extend([
        inline_reference(prompt, "$Apple · AAPL", "chip:company:AAPL", "company"),
        inline_reference(prompt, "$Bitcoin · BTC", "chip:company:BTC", "asset"),
    ]);
    assert_eq!(
        stored_draft_round_trip(json!({ "prompt": prompt, "context": context })),
        json!({ "prompt": prompt, "context": supported })
    );
}

#[test]
fn restored_tray_tickers_become_prose_and_preserve_other_reference_positions() {
    let prompt = "Compare @report with ⟦Source⟧ and $AAPL:NASDAQ (Apple).";
    let prefix = "Bitcoin 🪙 (BTC · CRYPTO)\nGold (GC · COMEX)\n\n";
    let restored_prompt = format!("{prefix}{prompt}");
    let file =
        json!({ "key": "file:report", "kind": "file", "id": "report.md", "label": "report" });
    let restored = stored_draft_round_trip(json!({ "prompt": prompt, "context": {
        "pins": [],
        "chips": [
            file,
            { "key": "company:BTC", "kind": "company", "id": "BTC:CRYPTO", "label": "Bitcoin 🪙 · BTC", "text": "Bitcoin 🪙" },
            { "key": "company:GC", "kind": "company", "id": "GC:COMEX", "label": "Gold", "text": "Gold" }
        ],
        "inlineReferences": [inline_reference(prompt, "@report", "chip:file:report", "file"), inline_reference(prompt, "⟦Source⟧", "pin:source", "source")]
    } }));
    assert_eq!(restored["prompt"], restored_prompt);
    assert_eq!(restored["context"]["chips"], json!([file]));
    assert_eq!(
        restored["context"]["inlineReferences"],
        json!([
            inline_reference(&restored_prompt, "@report", "chip:file:report", "file"),
            inline_reference(&restored_prompt, "⟦Source⟧", "pin:source", "source")
        ])
    );
}

#[test]
fn restored_legacy_tray_ticker_keeps_its_only_content_without_inline_ranges() {
    let restored = stored_draft_round_trip(json!({ "prompt": "", "context": {
        "pins": [], "chips": [{ "key": "company:AAPL", "kind": "company", "id": "AAPL:NASDAQ", "label": "Apple", "text": "Apple" }]
    } }));
    assert_eq!(restored["prompt"], "Apple (AAPL · NASDAQ)\n\n");
    assert_eq!(restored["context"], json!({ "pins": [], "chips": [] }));
}

fn fixture() -> (tempfile::TempDir, AppState) {
    fixture_with_folder_catalog(false)
}

fn fixture_with_folder_catalog(folder_catalog: bool) -> (tempfile::TempDir, AppState) {
    let temporary = tempfile::tempdir().unwrap();
    let state = if folder_catalog {
        AppState::for_folder_test(temporary.path().join("profile"))
    } else {
        AppState::for_persistent_test(temporary.path().join("profile"))
    };
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

#[test]
fn a_work_tab_reference_is_exact_while_a_chat_reads_the_same_reference_leniently() {
    let mut extra = serde_json::to_value(preferences()).unwrap();
    extra["sessions"][0]["tabs"][2]["artifact"]["preview"] = json!("newer field");
    assert!(serde_json::from_value::<WorkspaceUiState>(extra.clone()).is_err());
    let stored = extra["sessions"][0]["tabs"][2]["artifact"].clone();
    let read: ChatArtifactRef = serde_json::from_value(stored).unwrap();
    assert_eq!(read.artifact_id, "report");
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

#[tokio::test]
async fn offline_loupist_tabs_survive_read_and_write_until_the_folder_returns() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let state = AppState::for_folder_test(temporary.path().join("profile"));
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    state
        .store
        .create_chat(&chat("chat-a", &loupist.id, 1))
        .unwrap();
    let mut input = preferences();
    input.sessions[0].loupist_id = loupist.id.clone();
    // These references carry their own owner checks when opened; this test
    // exercises the file tab and Chat identity only.
    input.sessions[0].tabs.truncate(1);
    input.sessions[0].active_tab_id = Some("files".into());
    let expected = serde_json::to_value(write_state(&state, input).unwrap()).unwrap();
    let moved = temporary.path().join("offline");
    std::fs::rename(&workspace, &moved).unwrap();
    let restored = read_state(&state).unwrap().unwrap();
    assert_eq!(serde_json::to_value(&restored).unwrap(), expected);
    write_state(&state, restored).unwrap();
    std::fs::rename(&moved, &workspace).unwrap();
    assert_eq!(
        serde_json::to_value(read_state(&state).unwrap().unwrap()).unwrap(),
        expected
    );
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

#[tokio::test]
async fn deleting_a_chat_removes_only_its_prompt_context_and_attachment_drafts() {
    let (_temporary, state) = fixture();
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    app.manage(state);
    for (loupist, thread) in [
        ("loupist-a", "chat-a"),
        ("loupist-b", "chat-b"),
        ("loupist-a", "__empty-chat__"),
    ] {
        let id = || DraftIdentity {
            loupist_id: loupist.into(),
            thread_id: thread.into(),
        };
        chat_draft_write(
            app.state(),
            id(),
            serde_json::from_value(json!({"prompt": "Unsent", "context": {"asOf": "2026-11-01"}}))
                .unwrap(),
        )
        .unwrap();
        chat_draft_files_write(
            app.state(),
            id(),
            vec![DraftAttachment {
                id: "file".into(),
                filename: "report.txt".into(),
                media_type: "text/plain".into(),
                url: "data:text/plain;base64,cmVwb3J0".into(),
            }],
        )
        .unwrap();
    }
    super::super::records::chat_delete(
        super::super::types::ChatDeleteRequest {
            loupist_id: "loupist-a".into(),
            thread_id: "chat-a".into(),
        },
        app.state(),
    )
    .await
    .unwrap();
    let state = app.state::<AppState>();
    assert!(state.store.get_chat("chat-a").unwrap().is_none());
    let db = state.store.lock().unwrap();
    for table in [
        "composer_drafts",
        "composer_draft_context",
        "composer_draft_files",
    ] {
        let rows: Vec<(String, String)> = db
            .prepare(&format!(
                "SELECT loupist_id,thread_id FROM {table} ORDER BY loupist_id,thread_id"
            ))
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows,
            [
                ("loupist-a".into(), "__empty-chat__".into()),
                ("loupist-b".into(), "chat-b".into())
            ]
        );
    }
}

fn draft_identity() -> DraftIdentity {
    DraftIdentity {
        loupist_id: "loupist-a".into(),
        thread_id: "chat-a".into(),
    }
}

fn draft_files() -> Vec<DraftAttachment> {
    vec![DraftAttachment {
        id: "file".into(),
        filename: "report.txt".into(),
        media_type: "text/plain".into(),
        url: "data:text/plain;base64,cmVwb3J0".into(),
    }]
}

async fn delete_draft_chat(app: &tauri::App<tauri::test::MockRuntime>) -> Result<(), CommandError> {
    super::super::records::chat_delete(
        super::super::types::ChatDeleteRequest {
            loupist_id: "loupist-a".into(),
            thread_id: "chat-a".into(),
        },
        app.state(),
    )
    .await
}

fn draft_row_count(state: &AppState, table: &str) -> i64 {
    state
        .store
        .lock()
        .unwrap()
        .query_row(
            &format!(
                "SELECT count(*) FROM {table} WHERE loupist_id='loupist-a' AND thread_id='chat-a'"
            ),
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[tokio::test]
async fn review_storage_failed_chat_deletion_preserves_its_drafts() {
    for folder_catalog in [false, true] {
        let (_temporary, state) = fixture_with_folder_catalog(folder_catalog);
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        app.manage(state);
        let draft = json!({"prompt":"Unsent judgment", "context":{"asOf":"2026-11-01"}});
        chat_draft_write(
            app.state(),
            draft_identity(),
            serde_json::from_value(draft.clone()).unwrap(),
        )
        .unwrap();
        chat_draft_files_write(app.state(), draft_identity(), draft_files()).unwrap();
        app.state::<AppState>().store.lock_loupist("loupist-a").unwrap().execute_batch(
            "CREATE TRIGGER reject_chat_deletion BEFORE DELETE ON chat_sessions BEGIN SELECT RAISE(ABORT, 'fixture deletion refusal'); END;"
        ).unwrap();
        assert!(delete_draft_chat(&app).await.is_err());
        assert!(app
            .state::<AppState>()
            .store
            .get_chat("chat-a")
            .unwrap()
            .is_some());
        assert_eq!(
            serde_json::to_value(chat_draft_read(app.state(), draft_identity()).unwrap()).unwrap(),
            draft
        );
        assert_eq!(
            chat_draft_files_read(app.state(), draft_identity())
                .unwrap()
                .len(),
            1
        );
        for table in [
            "composer_drafts",
            "composer_draft_context",
            "composer_draft_files",
        ] {
            assert_eq!(draft_row_count(&app.state(), table), 1);
        }
        app.state::<AppState>()
            .store
            .lock_loupist("loupist-a")
            .unwrap()
            .execute_batch("DROP TRIGGER reject_chat_deletion")
            .unwrap();
        // Write-before-delete: a successful retry removes every draft row.
        delete_draft_chat(&app).await.unwrap();
        for table in [
            "composer_drafts",
            "composer_draft_context",
            "composer_draft_files",
        ] {
            assert_eq!(draft_row_count(&app.state(), table), 0);
        }
    }
}

#[tokio::test]
async fn review_storage_draft_admitted_before_delete_cannot_write_afterwards() {
    for folder_catalog in [false, true] {
        let (_temporary, state) = fixture_with_folder_catalog(folder_catalog);
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        app.manage(state);
        // Pause the production write between its initial admission/table setup
        // and its persistence half. Deletion wins this exact interleaving.
        draft_table(&app.state(), &draft_identity()).unwrap();
        delete_draft_chat(&app).await.unwrap();
        let result = write_draft_files(&app.state(), &draft_identity(), &draft_files());
        assert!(
            result.is_err(),
            "a late admitted write must not recreate an unreachable row"
        );
        assert_eq!(draft_row_count(&app.state(), "composer_draft_files"), 0);
        assert!(chat_draft_files_write(app.state(), draft_identity(), draft_files()).is_err());
        assert!(chat_draft_write(
            app.state(),
            draft_identity(),
            serde_json::from_value(json!({"prompt":"late"})).unwrap()
        )
        .is_err());
    }
}
