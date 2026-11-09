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
turns_file = fixture_root / "bot-memory-turns.jsonl"
script_file = fixture_root / "bot-memory-turn.json"

entries = [json.loads(line) for line in session_file.read_text().splitlines()] if session_file.exists() else []

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
        # The test writes what this turn does: broker calls, then an answer or none.
        turn = json.loads(script_file.read_text()) if script_file.exists() else {}
        results = [
            broker_call(f"call-{index}", call["method"], call["params"])
            for index, call in enumerate(turn.get("calls", []))
        ]
        with turns_file.open("a") as output:
            output.write(json.dumps({"session_id": session_id, "prompt": request.get("message", ""), "results": results}) + "\n")
        if turn.get("end_without_answer"):
            emit({"type": "agent_settled"})
            continue
        if turn.get("wait_for_stop"):
            continue
        answer = turn.get("answer", "Noted.")
        message = {"role": "assistant", "stopReason": "stop", "content": [{"type": "text", "text": answer}]}
        entries = [{"id": "assistant-answer", "type": "message", "message": message}]
        session_file.write_text("\n".join(json.dumps(entry) for entry in entries) + "\n")
        emit({"type": "message_start", "message": {"role": "assistant"}})
        emit({"type": "message_update", "assistantMessageEvent": {"type": "text_delta", "delta": answer}})
        emit({"type": "message_end", "message": message})
        if turn.get("exit_during_post_answer_compaction"):
            emit({"type": "compaction_start", "reason": "threshold"})
            sys.exit(1)
        if turn.get("deferred_compaction"):
            emit({"type": "compaction_start", "reason": "threshold"})
            emit({"type": "compaction_end", "reason": "threshold", "aborted": True})
        emit({"type": "agent_settled"})
    elif command == "abort":
        respond(request, {})
        emit({"type": "agent_settled"})
    else:
        emit({"type": "response", "id": request["id"], "success": False, "error": "Unexpected fixture control"})
