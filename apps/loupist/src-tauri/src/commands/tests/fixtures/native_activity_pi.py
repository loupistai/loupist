"""Single-turn RPC fixture for the persisted native activity integration test."""

import json
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


def broker_history(call_id, params):
    protocol = "loupist-tool/1"
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(5)
        connection.connect(os.environ["LOUPIST_BROKER_SOCKET"])
        request = {"protocol": protocol, "id": call_id, "call_id": call_id,
                   "token": BROKER_TOKEN,
                   "method": "chat.history", "params": params}
        connection.sendall((json.dumps(request) + "\n").encode())
        reader = connection.makefile("rb")
        result = json.loads(reader.readline())
        connection.sendall((json.dumps({"protocol": protocol, "id": call_id, "delivered": True}) + "\n").encode())
        assert json.loads(reader.readline())["committed"] is True
        return result


session_id = argument("--session-id")
session_file = pathlib.Path(argument("--session-dir")) / f"fixture_{session_id}.jsonl"
fixture_root = pathlib.Path(__file__).parent
trace_file = fixture_root / "startup-trace.jsonl"


def trace(event, **values):
    if trace_file.exists():
        with trace_file.open("a") as output:
            output.write(json.dumps({"event": event, "session_id": session_id, **values}) + "\n")


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
        if (fixture_root / "trace-broker-calls").exists():
            assert broker_history("history-ok", {})["ok"] is True
            failed = broker_history("history-bad", {"password": "fictional-sensitive-value", "limit": "bad"})
            assert failed["error"]["code"] == "malformed_request"
        emit({"type": "message_start", "message": {"role": "assistant"}})
        emit({"type": "message_end", "message": {
            "role": "assistant", "stopReason": "toolUse", "content": [
                {"type": "toolCall", "id": "read-note", "name": "read", "arguments": tool_args}
            ],
        }})
        emit({"type": "tool_execution_start", "toolCallId": "read-note", "toolName": "read", "args": tool_args})
        emit({"type": "tool_execution_update", "toolCallId": "read-note", "partialResult": {
            "content": [{"type": "text", "text": "Reading the note…"}],
        }})
        emit({"type": "tool_execution_end", "toolCallId": "read-note", "toolName": "read", "isError": False, "result": {
            "content": [{"type": "text", "text": tool_output}],
        }})
        message = {"role": "assistant", "stopReason": "stop", "content": [{"type": "text", "text": answer}]}
        entries = [{"id": "assistant-note", "type": "message", "message": message}]
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
