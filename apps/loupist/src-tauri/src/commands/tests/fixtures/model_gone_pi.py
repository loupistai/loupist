"""RPC fixture: the model named `gone-model` answers 404 model_not_found; any
other model answers. Each launch appends its model to `launches.txt`."""

import json
import pathlib
import sys


def argument(name):
    return sys.argv[sys.argv.index(name) + 1]


def emit(payload):
    print(json.dumps(payload), flush=True)


def respond(request, data=None):
    emit({"type": "response", "id": request["id"], "success": True, "data": data})


session_id = argument("--session-id")
session_file = pathlib.Path(argument("--session-dir")) / f"fixture_{session_id}.jsonl"
model = argument("--model")
with (pathlib.Path(__file__).parent / "launches.txt").open("a") as launches:
    launches.write(model + "\n")
entries = []

for line in sys.stdin:
    request = json.loads(line)
    command = request["type"]
    if command == "get_available_thinking_levels":
        respond(request, {"levels": [argument("--thinking")]})
    elif command == "get_state":
        respond(request, {
            "model": {"provider": argument("--provider"), "id": model},
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
    elif command == "prompt" and request.get("message", "").startswith("/loupist-verify-settings "):
        respond(request, {"disposition": "handled"})
    elif command == "prompt":
        respond(request, {})
        emit({"type": "message_start", "message": {"role": "assistant"}})
        if model == "gone-model":
            emit({"type": "message_end", "message": {
                "role": "assistant", "stopReason": "error", "content": [],
                "errorMessage": '404 {"error":{"code":"model_not_found","message":"The model does not exist"}}',
            }})
        else:
            answer = "Answered on Auto."
            message = {"role": "assistant", "stopReason": "stop", "content": [{"type": "text", "text": answer}]}
            entries = [{"id": "assistant-answer", "type": "message", "message": message}]
            session_file.write_text("\n".join(json.dumps(entry) for entry in entries) + "\n")
            emit({"type": "message_update", "assistantMessageEvent": {"type": "text_delta", "delta": answer}})
            emit({"type": "message_end", "message": message})
        emit({"type": "agent_settled"})
    elif command == "abort":
        respond(request, {})
        emit({"type": "agent_settled"})
    else:
        emit({"type": "response", "id": request["id"], "success": False, "error": "Unexpected fixture control"})
