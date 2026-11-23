//! Tauri runs a synchronous command inside IPC dispatch, which in the app is
//! the main thread: AppKit, accessibility and every click wait for it. These
//! commands are the ones the renderer sends while World publishes a Source
//! and a session runs. A World publication holds the Loupist's store lock for
//! as long as it seals and indexes the World, so each must be dispatched off
//! that thread and answer once the lock is free.
use std::sync::{mpsc, Arc, Barrier};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponse};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::Manager;

use crate::app::AppState;

const LOUPIST: &str = "loupist-busy";
const HOLD: Duration = Duration::from_millis(400);
/// Far below the hold: a dispatch that ran the body inline would take it all.
const DISPATCH_BUDGET: Duration = Duration::from_millis(150);

fn requests(chat_id: &str) -> Vec<(&'static str, Value)> {
    vec![
        // The Library's and the Chat's Map polls, once per followed job.
        (
            "knowledge_graph_job",
            json!({"request": {"loupistId": LOUPIST, "jobId": "job-missing"}}),
        ),
        // The composer saves the unsent text while the user types.
        (
            "chat_draft_write",
            json!({"request": {"loupist_id": LOUPIST, "thread_id": chat_id}, "draft": {"prompt": "draft"}}),
        ),
        (
            "chat_draft_read",
            json!({"request": {"loupist_id": LOUPIST, "thread_id": chat_id}}),
        ),
        // A finished answer asks what it taught the Loupist.
        (
            "memory_learned",
            json!({"request": {"loupistId": LOUPIST, "messageIds": ["message-missing"]}}),
        ),
        // The sidebar's Schedule badge.
        ("inbox_unread_counts", json!({})),
        // Map, Lens and Library lists.
        ("principle_list", json!({"request": {"loupistId": LOUPIST}})),
        ("journal_list", json!({"request": {"loupistId": LOUPIST}})),
        (
            "learning_review_list",
            json!({"request": {"loupistId": LOUPIST}}),
        ),
    ]
}

fn app(state: AppState) -> (tauri::App<MockRuntime>, tauri::WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![
            crate::commands::knowledge::knowledge_graph_job,
            crate::commands::workspace_ui::chat_draft_write,
            crate::commands::workspace_ui::chat_draft_read,
            crate::commands::knowledge::memory_learned,
            crate::briefings::commands::inbox_unread_counts,
            crate::commands::knowledge::principles::principle_list,
            crate::commands::knowledge::journal::journal_list,
            crate::commands::knowledge::method_learning::learning_review_list,
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    assert!(app.manage(state));
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    (app, webview)
}

fn dispatch(
    webview: &tauri::WebviewWindow<MockRuntime>,
    command: &str,
    body: Value,
) -> mpsc::Receiver<(InvokeResponse, Instant)> {
    let (sender, receiver) = mpsc::channel();
    webview.clone().on_message(
        InvokeRequest {
            cmd: command.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.into(),
        },
        Box::new(move |_webview, _command, response, _callback, _error| {
            let _ = sender.send((response, Instant::now()));
        }),
    );
    receiver
}

#[test]
fn renderer_commands_during_world_publication_leave_the_dispatching_thread_free() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_folder_test(temporary.path().join("app"));
    crate::test_fixtures::create_folder_loupist(
        state.store.as_ref(),
        LOUPIST,
        &temporary.path().join("workspace"),
    );
    let chat = crate::test_fixtures::chat_session("chat-busy", LOUPIST);
    state.store.create_chat(&chat).unwrap();
    let holder = state.clone();
    let (_app, webview) = app(state);

    let mut blocked = Vec::new();
    for (command, body) in requests(&chat.id) {
        // A World publication's transaction: the Loupist's store lock and an
        // open write, held while the dispatching thread is asked to answer.
        let started = Arc::new(Barrier::new(2));
        let held = started.clone();
        let store = holder.store.clone();
        let writer = std::thread::spawn(move || {
            let connection = store.lock_loupist(LOUPIST).unwrap();
            connection.execute_batch("BEGIN IMMEDIATE").unwrap();
            held.wait();
            std::thread::sleep(HOLD);
            connection.execute_batch("ROLLBACK").unwrap();
            Instant::now()
        });
        started.wait();
        let asked = Instant::now();
        let reply = dispatch(&webview, command, body);
        let dispatch_took = asked.elapsed();
        let released = writer.join().unwrap();
        if dispatch_took >= DISPATCH_BUDGET {
            blocked.push(format!("{command} ({dispatch_took:?})"));
        }
        let (response, at) = reply
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| panic!("{command} never answered"));
        // It waited for the store, so it read what the write left behind.
        assert!(
            at >= released,
            "{command} answered before the store lock was free"
        );
        if let InvokeResponse::Err(error) = &response {
            let text = error.0.to_string();
            assert!(
                !text.contains("invalid args") && !text.contains("missing required key"),
                "{command} rejected its request instead of running: {text}"
            );
        }
    }
    assert!(
        blocked.is_empty(),
        "these commands held the dispatching thread behind the store lock: {}",
        blocked.join(", ")
    );
}
