import json
import os
import pathlib
import re
import socket
import sys


def argument(name):
    return sys.argv[sys.argv.index(name) + 1]


def emit(payload):
    print(json.dumps(payload), flush=True)


def respond(request, data=None):
    emit({"type": "response", "id": request["id"], "success": True, "data": data})


def call_history(call_id, args):
    name = "chat_history"
    emit({"type": "message_start", "message": {"role": "assistant"}})
    emit({"type": "message_end", "message": {"role": "assistant", "stopReason": "toolUse",
          "content": [{"type": "toolCall", "id": call_id, "name": name, "arguments": args}]}})
    emit({"type": "tool_execution_start", "toolCallId": call_id, "toolName": name, "args": args})
    protocol = "loupist-tool/1"
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(5)
        connection.connect(os.environ["LOUPIST_BROKER_SOCKET"])
        with connection.makefile("rwb") as wire:
            wire.write((json.dumps({"protocol": protocol, "id": call_id, "call_id": call_id,
                "token": token, "method": "chat.history", "params": args}) + "\n").encode())
            wire.flush()
            reply = json.loads(wire.readline())
            wire.write((json.dumps({"protocol": protocol, "id": call_id, "delivered": True}) + "\n").encode())
            wire.flush()
            assert json.loads(wire.readline())["committed"]
    emit({"type": "tool_execution_end", "toolCallId": call_id, "toolName": name,
          "isError": not reply["ok"], "result": reply.get("result", reply.get("error"))})
    assert reply["ok"], reply
    return reply["result"]


def body(field):
    match = re.fullmatch(r"<<untrusted [^\n]+>>\n(.*)\n<<end untrusted id=[a-f0-9]+>>", field, re.S)
    assert match, field
    return match[1]


with os.fdopen(int(os.environ.pop("LOUPIST_BROKER_TOKEN_FD")), "rb") as descriptor:
    token = descriptor.readline(513).decode().rstrip("\n")
session_id = argument("--session-id")
session_file = pathlib.Path(argument("--session-dir")) / f"fixture_{session_id}.jsonl"
entries = []
title = "Alder Rail capacity"
preview = "The revised timetable is feasible."
conclusion = "Only the northern spur can cover the maintenance window."

for line in sys.stdin:
    request = json.loads(line)
    command = request["type"]
    if command == "get_available_thinking_levels":
        respond(request, {"levels": [argument("--thinking")]})
    elif command == "get_state":
        respond(request, {
            "model": {"provider": argument("--provider"), "id": argument("--model")},
            "thinkingLevel": argument("--thinking"), "sessionId": session_id,
            "sessionFile": str(session_file), "messageCount": len(entries),
            "autoCompactionEnabled": True, "isStreaming": False, "isCompacting": False,
            "pendingMessageCount": 0,
        })
    elif command == "get_entries":
        respond(request, {"entries": entries, "leafId": entries[-1]["id"] if entries else None})
    elif command == "prompt" and request.get("message", "").startswith("/loupist-verify-settings "):
        respond(request, {"disposition": "handled"})
    elif command == "prompt":
        respond(request, {})
        prompt = request["message"]
        envelope = json.loads(prompt.split("<turn_envelope>\n", 1)[1].split("\n</turn_envelope>", 1)[0])
        surface = envelope["chat_surface"]["surface"]
        if surface == "delegated":
            answer = f"{preview}\n\n{conclusion}"
        else:
            assert surface == "bot" and "What did that research conclude?" in prompt
            history = call_history("history-page", {})
            cards = [entry for entry in history["messages"] if "card_id" in entry]
            assert len(cards) == 1 and cards[0]["content"] == "", cards
            delivered = call_history("history-answer", {"card_id": cards[0]["card_id"]})
            assert body(delivered["content"]) == f"{preview}\n\n{conclusion}"
            assert body(delivered["title"]) == title
            answer = f"The {body(delivered['title'])} research concluded: {body(delivered['content']).splitlines()[-1]}"
        message = {"role": "assistant", "stopReason": "stop", "content": [{"type": "text", "text": answer}]}
        entries = [{"id": "assistant-answer", "type": "message", "message": message}]
        session_file.write_text(json.dumps(entries[0]) + "\n")
        emit({"type": "message_start", "message": {"role": "assistant"}})
        emit({"type": "message_update", "assistantMessageEvent": {"type": "text_delta", "delta": answer}})
        emit({"type": "message_end", "message": message})
        emit({"type": "agent_settled"})
    elif command == "abort":
        respond(request, {})
        emit({"type": "agent_settled"})
    else:
        emit({"type": "response", "id": request["id"], "success": False, "error": "Unexpected fixture control"})
