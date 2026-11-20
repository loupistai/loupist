//! Scripted Pi uses the real Chat transport; no model or app window is involved.
#![cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]

use super::*;
use crate::commands::{
    chat_runtime::{send_chat, ChatEventSink},
    tests::chat_behavior::{drain, install_chat_rpc_fixture, send_request},
};

const SCRIPT: &str = r###"import json
import os
import pathlib
import socket
import sys


def argument(name):
    return sys.argv[sys.argv.index(name) + 1]


def emit(payload):
    print(json.dumps(payload), flush=True)


def respond(request, data=None):
    emit({"type": "response", "id": request["id"], "success": True, "data": data})


def take_broker_token():
    descriptor = os.environ.pop("LOUPIST_BROKER_TOKEN_FD", None)
    if descriptor is None:
        return None
    descriptor = int(descriptor)
    line = b""
    while not line.endswith(b"\n") and len(line) <= 512:
        chunk = os.read(descriptor, 513 - len(line))
        if not chunk:
            break
        line += chunk
    os.close(descriptor)
    if not line.endswith(b"\n"):
        raise SystemExit("broker token line is invalid")
    return line[:-1].decode()


BROKER_TOKEN = take_broker_token()


def broker_call(call_id, method, params):
    protocol = "loupist-tool/1"
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(5)
        connection.connect(os.environ["LOUPIST_BROKER_SOCKET"])
        request = {"protocol": protocol, "id": call_id, "call_id": call_id,
                   "token": BROKER_TOKEN, "method": method, "params": params}
        connection.sendall((json.dumps(request) + "\n").encode())
        reader = connection.makefile("rb")
        result = json.loads(reader.readline())
        connection.sendall((json.dumps({"protocol": protocol, "id": call_id, "delivered": True}) + "\n").encode())
        assert json.loads(reader.readline())["committed"] is True
        return result


session_id = argument("--session-id")
session_file = pathlib.Path(argument("--session-dir")) / f"fixture_{session_id}.jsonl"
fixture_root = pathlib.Path(__file__).parent

entries = []
answer = "# HBM4 supply\n\n**HBM4** supply widens in [2027](https://example.com/hbm4).\n\nTwo more suppliers announced capacity."

for line in sys.stdin:
    request = json.loads(line)
    command = request["type"]
    is_settings_control = command == "prompt" and request.get("message", "").startswith("/loupist-verify-settings ")
    if command == "get_available_thinking_levels":
        respond(request, {"levels": [argument("--thinking")]})
    elif command == "get_state":
        respond(request, {
            "model": {"provider": argument("--provider"), "id": argument("--model")},
            "thinkingLevel": argument("--thinking"),
            "sessionId": session_id,
            "sessionFile": str(session_file),
            "messageCount": len(entries),
            "autoCompactionEnabled": True,
            "isStreaming": False,
            "isCompacting": False,
            "pendingMessageCount": 0,
        })
    elif command == "get_entries":
        respond(request, {"entries": entries, "leafId": entries[-1]["id"] if entries else None})
    elif is_settings_control:
        respond(request, {"disposition": "handled"})
    elif command == "prompt":
        respond(request, {})
        calc = broker_call("source", "calculation.run", {
            "operation": "margin", "unit": "USD", "inputs": {"profit": "7", "revenue": "100"},
        })
        assert calc["ok"], calc
        citation = calc["result"]["citation_url"]
        table = {
            "mode": "create", "layout": "assets", "title": "Margins",
            "rows": [{"symbol": "AAA", "market": "NASDAQ", "label": "A"},
                     {"symbol": "BBB", "market": "NASDAQ", "label": "B"}],
            "columns": [{"id": "margin", "label": "Margin", "unit": "percent"}],
            "cells": [[{"value": "8%", "citation_url": citation}], [{"empty": "unavailable"}]],
        }
        first = broker_call("table", "grid.publish", table)
        (fixture_root / "first-publish.json").write_text(json.dumps(first))
        assert first["ok"], first
        assert first["result"]["cell_checks"][0]["status"] == "differs", first
        table.update(mode="revise", artifact_id=first["result"]["artifact"]["artifact_id"],
                     edit_token=first["result"]["edit_token"])
        table["cells"][0][0]["value"] = "7%"
        revised = broker_call("correction", "grid.publish", table)
        assert revised["ok"], revised
        assert revised["result"]["cell_checks"][0]["status"] == "matched", revised
        answer = "The table now shows the corrected margin."
        message = {"role": "assistant", "stopReason": "stop", "content": [{"type": "text", "text": answer}]}
        entries = [{"id": "assistant-answer", "type": "message", "message": message}]
        session_file.write_text("\n".join(json.dumps(entry) for entry in entries) + "\n")
        emit({"type": "message_start", "message": {"role": "assistant"}})
        emit({"type": "message_update", "assistantMessageEvent": {"type": "text_delta", "delta": answer}})
        emit({"type": "message_end", "message": message})
        emit({"type": "agent_settled"})
    elif command == "abort":
        respond(request, {})
        emit({"type": "agent_settled"})
    else:
        emit({"type": "response", "id": request["id"], "success": False, "error": "Unexpected fixture control"})
"###;

#[tokio::test]
async fn scripted_chat_corrects_a_delivered_table_without_artifact_read() {
    let temporary = tempfile::tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    initialized_workspace(&workspace, "artifact-revise");
    let mut state = AppState::for_persistent_test(temporary.path().join("app"));
    install_chat_rpc_fixture(&mut state, temporary.path(), SCRIPT);
    let profile = profile("loupist-revise", &workspace, 1);
    seed_profile(state.store.as_ref(), &profile);
    let session = chat("research-revise", &profile.id, 1);
    state.store.create_chat(&session).unwrap();
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = events.clone();
    send_chat(
        send_request(
            "correct-table",
            &profile.id,
            &session.id,
            "Compare the margins.",
        ),
        ChatEventSink::new(move |event| {
            captured.lock().unwrap().push(event);
            Ok(())
        }),
        &state,
        None,
    )
    .await
    .unwrap();
    drain(&state).await;
    let first: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(temporary.path().join("first-publish.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        first["result"]["cell_checks"][0]["status"], "differs",
        "{first}"
    );
    assert!(first["result"]["edit_token"].is_string(), "{first}");
    let saved = state.store.get_chat(&session.id).unwrap().unwrap();
    let reply = saved.messages.last().unwrap();
    assert_eq!(reply.status, ChatMessageStatus::Complete, "{reply:?}");
    let artifacts = state.store.list_chat_artifacts(&session.id).unwrap();
    assert_eq!(artifacts.len(), 1, "{reply:?}");
    assert_eq!(artifacts[0].current_revision, 1);
    let snapshot = state
        .store
        .read_chat_artifact(&artifacts[0].id, None)
        .unwrap()
        .unwrap();
    let payload = serde_json::to_value(&snapshot.revision.payload).unwrap();
    assert_eq!(payload["grid"]["cells"][0][0]["value"], "7%");
    let staged: Vec<_> = events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            ChatStreamEvent::ArtifactStaged { view, .. } => {
                Some((view.artifact.id.clone(), view.revision.revision))
            }
            _ => None,
        })
        .collect();
    assert_eq!(staged, vec![(artifacts[0].id.clone(), 1); 2]);
    assert_eq!(
        state
            .store
            .list_chat_artifact_versions(&artifacts[0].id)
            .unwrap()
            .len(),
        1
    );
}
