"""Single-turn RPC fixture for the persisted native activity integration test."""

import json
import os
import pathlib
import sys
import socket
import uuid
import time


def argument(name):
    return sys.argv[sys.argv.index(name) + 1]


def emit(payload):
    print(json.dumps(payload), flush=True)


def respond(request, data=None):
    emit({"type": "response", "id": request["id"], "success": True, "data": data})


def take_broker_token():
    """Read the inherited token line once, like the Loupist extension: up to
    its newline, never waiting for EOF."""
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
session_id = argument("--session-id")
session_file = pathlib.Path(argument("--session-dir")) / f"fixture_{session_id}.jsonl"
fixture_root = pathlib.Path(__file__).parent
trace_file = fixture_root / "startup-trace.jsonl"


def trace(event, **values):
    if trace_file.exists():
        with trace_file.open("a") as output:
            output.write(json.dumps({"event": event, "session_id": session_id, **values}) + "\n")


def call_tool(name, method, args):
    call_id = str(uuid.uuid4())
    emit({"type": "message_start", "message": {"role": "assistant"}})
    emit({"type": "message_end", "message": {"role": "assistant", "stopReason": "toolUse", "content": [{"type": "toolCall", "id": call_id, "name": name, "arguments": args}]}})
    emit({"type": "tool_execution_start", "toolCallId": call_id, "toolName": name, "args": args})
    if "LONG_TOOL_EVAL" in current_request:
        time.sleep(1.2)  # No Pi events while the tool owns execution.
    sock = socket.socket(socket.AF_UNIX)
    sock.connect(os.environ["LOUPIST_BROKER_SOCKET"])
    wire = sock.makefile("rwb")
    def send_wire(value):
        wire.write((json.dumps(value) + "\n").encode()); wire.flush()
    send_wire({"protocol": "loupist-tool/1", "id": call_id, "call_id": call_id, "token": BROKER_TOKEN, "method": method, "params": args})
    reply = json.loads(wire.readline())
    send_wire({"protocol": "loupist-tool/1", "id": call_id, "delivered": True})
    barrier = json.loads(wire.readline())
    assert barrier["committed"], "broker did not commit delivery"
    wire.close(); sock.close()
    emit({"type": "tool_execution_end", "toolCallId": call_id, "toolName": name, "isError": not reply["ok"], "result": reply.get("result", reply.get("error"))})
    return reply


trace("launch", pid=os.getpid())
entries = []
tool_args = {"path": "notes.txt"}
tool_output = "Retained local evidence.\n"
answer = "Read the local note."

for line in sys.stdin:
    request = json.loads(line)
    command = request["type"]
    is_settings_control = command == "prompt" and request.get("message", "").startswith("/loupist-verify-settings ")
    trace("settings_confirmation" if is_settings_control else command)
    # A test-controlled startup stall still handles abort on the live stdin
    # stream, so Stop exercises the real RPC and owned-process shutdown path.
    if (fixture_root / "hold-pi-startup").exists() and command != "abort":
        continue
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
        envelope = json.loads(request["message"].split("<turn_envelope>\n", 1)[1].split("\n</turn_envelope>", 1)[0])
        index = envelope.get("memory_protocol", {}).get("learned_index", [])
        learned = [entry for entry in index if entry["title"] == "Hanbit diluted-share rule"]
        provider_replay = "DART_EVAL" in request["message"]
        cite_note = "CITE_NOTE_EVAL" in request["message"]
        replay_case = None
        if cite_note:
            path = "northwind-loom.md"
            if "<user_attachments>" in request["message"]:
                attachments = json.loads(request["message"].split("<user_attachments>\n", 1)[1].split("\n</user_attachments>", 1)[0])
                path = str(pathlib.Path(attachments[0]["path"]).resolve().relative_to((fixture_root / "workspace").resolve()))
            name, method, args = "workspace_capture", "workspace.capture", {"path": path}
        elif "REPLAY_CASE_EVAL" in request["message"]:
            replay_case = json.loads((fixture_root / "replay-case.json").read_text())
            name, method, args = replay_case["name"], replay_case["method"], replay_case["args"]
        elif provider_replay:
            name, method = "dart_filing_search", "dart.filing_search"
            args = {"company":"00999999", "start":"2020-01-01", "end":"2020-12-31", "limit":1}
        elif "TIMEOUT_EVAL" in request["message"]:
            continue  # Remain responsive to the production abort RPC.
        elif "BADLINK_EVAL" in request["message"]:
            name, method = "lens_file", "lens.file"
            args = {"pages": [{"page": {"create": {"kind": "subject", "title": "Hanbit Battery"}},
                               "ops": [{"op": "put", "key": "facts", "heading": "Facts",
                                        "markdown": "Supplies [Northgate](world:northgate-motors).",
                                        "basis": {"refs": [{"said": True}]}}]}],
                    "rationale": "The user said who Hanbit supplies."}
        elif "TEACH_EVAL" in request["message"]:
            name, method = "lens_file", "lens.file"
            args = {"pages": [{"page": {"create": {"kind": "method", "title": "Hanbit diluted-share rule"}},
                               "ops": [{"op": "put", "key": "use-when", "heading": "Use when",
                                        "markdown": "Valuing fictional Hanbit companies.",
                                        "basis": {"refs": [{"said": True}]}},
                                       {"op": "put", "key": "steps", "heading": "Steps",
                                        "markdown": "Divide equity value by fully diluted shares including warrants.",
                                        "basis": {"refs": [{"said": True}]}}]}],
                    "rationale": "The user taught a reusable valuation method."}
        else:
            assert learned, "new chat host memory context did not include the learned Lens"
            name, method = "lens_read", "lens.read"
            args = {"recordId": learned[0]["id"], "status": "applied"}
        if "EMPTY_NO_WRITE_EVAL" in request["message"]:
            emit({"type": "message_start", "message": {"role": "assistant"}})
            emit({"type": "message_end", "message": {"role": "assistant", "stopReason": "stop", "content": []}})
            emit({"type": "agent_settled"})
            continue
        current_request = request["message"].rsplit("<current_user_message>\n", 1)[-1].split("\n</current_user_message>", 1)[0]
        reply = call_tool(name, method, args)
        if replay_case and replay_case.get("error"):
            assert not reply["ok"], "expected native rejection"
            assert replay_case["error"] in json.dumps(reply), str(reply)
        elif "BADLINK_EVAL" in request["message"]:
            assert not reply["ok"], "a world: link to an unknown node must be refused when staged"
            assert "pages[0].ops[0].markdown" in json.dumps(reply), str(reply)
        else:
            assert reply["ok"], str(reply.get("error"))
        if provider_replay:
            assert "result_ref" in json.dumps(reply["result"]), "host did not create result reference"
            assert "citation_url" in json.dumps(reply["result"]), "host did not create citation"
        if learned:
            assert "diluted" in json.dumps(reply["result"]), "exact Lens read missing method"
        answer = ("Noted, without saving the link." if "BADLINK_EVAL" in request["message"]
                  else "Saved the rule.") if name == "lens_file" else 'Applied saved Hanbit method: 720 / (15 + 3) = 40. {"metrics":{"fair_value_per_share":40}}'
        if provider_replay:
            answer = "Read the fictional Hanbit filing through provider replay."
        if replay_case:
            answer = json.dumps(reply.get("result", reply.get("error")), ensure_ascii=False)
        if cite_note:
            captured = reply["result"]
            assert "18420" in captured["content"]
            saved = call_tool("source_save", "source.save", {
                "result_ref": captured["result_ref"], "title": "Northwind Loom note", "coverage": "included_full"
            })
            assert saved["ok"], str(saved.get("error"))
            assert saved["result"]["result_ref"] == captured["result_ref"]
            answer = f'Northwind Loom recorded loom-yield token [18420]({captured["citation_url"]}) on 12 March 2026.'
        if "STALL_AFTER_TOOL_EVAL" in current_request:
            continue  # Provider continuation never responds; abort RPC remains live.
        if "EXIT_AFTER_WRITE_EVAL" in request["message"]:
            sys.exit(23)
        if "EMPTY_REPLY_EVAL" in request["message"]:
            answer = ""
        message = {"role": "assistant", "stopReason": "stop", "content": [{"type": "text", "text": answer}]}
        if "PROVIDER_ERROR_EVAL" in request["message"]:
            answer = ""
            message = {"role": "assistant", "stopReason": "error", "content": [], "errorMessage": "private provider body /fixture/private sk-fixture"}
        entries = [{"id": "assistant-eval", "type": "message", "message": message}]
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
