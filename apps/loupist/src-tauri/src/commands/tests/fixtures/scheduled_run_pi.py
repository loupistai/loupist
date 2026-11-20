"""RPC fixture for scheduled sessions: every prompt gets the same short answer, and
each turn envelope is kept in turn-envelopes.jsonl beside the script."""

import json
import pathlib
import sys


def argument(name):
    return sys.argv[sys.argv.index(name) + 1]


def emit(payload):
    print(json.dumps(payload), flush=True)


def respond(request, data=None):
    emit({"type": "response", "id": request["id"], "success": True, "data": data})


fixture_root = pathlib.Path(__file__).parent
session_id = argument("--session-id")
session_file = pathlib.Path(argument("--session-dir")) / f"fixture_{session_id}.jsonl"
answer = "Checked the chip makers. Nothing crossed the line today."
entries = []

for line in sys.stdin:
    request = json.loads(line)
    command = request["type"]
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
    elif command == "prompt" and request.get("message", "").startswith("/loupist-verify-settings "):
        respond(request, {"disposition": "handled"})
    elif command == "prompt":
        respond(request, {})
        prompt = request.get("message", "")
        if "<turn_envelope>\n" in prompt:
            envelope = prompt.split("<turn_envelope>\n", 1)[1].split("\n</turn_envelope>", 1)[0]
            with (fixture_root / "turn-envelopes.jsonl").open("a") as output:
                output.write(envelope.replace("\n", " ") + "\n")
        message = {"role": "assistant", "stopReason": "stop", "content": [{"type": "text", "text": answer}]}
        entries = [{"id": f"assistant-{len(entries)}", "type": "message", "message": message}]
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
