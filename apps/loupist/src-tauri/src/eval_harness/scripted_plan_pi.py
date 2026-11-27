"""Plan-driven RPC fixture: each prompt takes the next step of plan.json,
across every Pi process of the fixture.

A step is {"calls": [{"name", "method", "args"}], "answer": "..."}. Every call
goes through the production broker and must succeed. An argument string
"$N.path.to.field" is the field of call N's result in this step. The full
message of each prompt is appended to prompts.jsonl, so a test can read what
the host sent.
"""

import json
import os
import pathlib
import socket
import sys
import uuid


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


fixture_root = pathlib.Path(__file__).parent
BROKER_TOKEN = take_broker_token()
session_id = argument("--session-id")
session_file = pathlib.Path(argument("--session-dir")) / f"fixture_{session_id}.jsonl"


def call_tool(name, method, args):
    call_id = str(uuid.uuid4())
    emit({"type": "message_start", "message": {"role": "assistant"}})
    message = {"role": "assistant", "stopReason": "toolUse",
               "content": [{"type": "toolCall", "id": call_id, "name": name, "arguments": args}]}
    emit({"type": "message_end", "message": message})
    emit({"type": "tool_execution_start", "toolCallId": call_id, "toolName": name, "args": args})
    sock = socket.socket(socket.AF_UNIX)
    sock.connect(os.environ["LOUPIST_BROKER_SOCKET"])
    wire = sock.makefile("rwb")

    def send_wire(value):
        wire.write((json.dumps(value) + "\n").encode())
        wire.flush()

    send_wire({"protocol": "loupist-tool/1", "id": call_id, "call_id": call_id, "token": BROKER_TOKEN,
               "method": method, "params": args})
    reply = json.loads(wire.readline())
    send_wire({"protocol": "loupist-tool/1", "id": call_id, "delivered": True})
    barrier = json.loads(wire.readline())
    assert barrier["committed"], "broker did not commit delivery"
    wire.close()
    sock.close()
    emit({"type": "tool_execution_end", "toolCallId": call_id, "toolName": name,
          "isError": not reply["ok"], "result": reply.get("result", reply.get("error"))})
    return reply


plan_file = fixture_root / "plan.json"
steps = json.loads(plan_file.read_text()) if plan_file.is_file() else []

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
            "messageCount": 0,
            "autoCompactionEnabled": True,
            "isStreaming": False,
            "isCompacting": False,
            "pendingMessageCount": 0,
        })
    elif command == "get_entries":
        respond(request, {"entries": [], "leafId": None})
    elif command == "prompt" and request.get("message", "").startswith("/loupist-verify-settings "):
        respond(request, {"disposition": "handled"})
    elif command == "prompt":
        respond(request, {})
        # Each turn starts its own Pi, so the step is the number of prompts
        # every Pi of this fixture has already taken.
        sent = fixture_root / "prompts.jsonl"
        taken = len(sent.read_text().splitlines()) if sent.is_file() else 0
        with sent.open("a") as output:
            output.write(json.dumps(request["message"]) + "\n")
        step = steps[taken]
        results = []

        def resolve(value):
            if isinstance(value, str) and value.startswith("$"):
                index, *path = value[1:].split(".")
                found = results[int(index)]
                for key in path:
                    found = found[key]
                return found
            if isinstance(value, list):
                return [resolve(item) for item in value]
            if isinstance(value, dict):
                return {key: resolve(item) for key, item in value.items()}
            return value

        for call in step.get("calls", []):
            reply = call_tool(call["name"], call["method"], resolve(call["args"]))
            if not reply["ok"]:
                with (fixture_root / "failures.jsonl").open("a") as output:
                    output.write(json.dumps({"name": call["name"], "error": reply.get("error")}) + "\n")
            assert reply["ok"], str(reply.get("error"))
            results.append(reply["result"])
        answer = step["answer"]
        message = {"role": "assistant", "stopReason": "stop", "content": [{"type": "text", "text": answer}]}
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
