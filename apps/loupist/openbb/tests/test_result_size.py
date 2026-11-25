"""A reply over the host's frame limit fails as that one call over real stdio.

The host reads one MCP line of at most 32 MiB and ends the session on a longer
one. The child process lowers the bound to a few KiB so the test stays small.
"""

import json
import os
import queue
import subprocess
import sys
import threading
from pathlib import Path

LIMIT = 4096


def serve() -> None:
    from fastapi import FastAPI
    from openbb_core.api.app_loader import AppLoader
    from openbb_mcp_server.app.app import create_mcp_server
    from openbb_mcp_server.models.settings import MCPSettings
    from pydantic import BaseModel

    from loupist_openbb import failures
    from loupist_openbb.server import (
        mark_registered_tools_read_only,
        retain_read_only_routes,
    )

    failures.MAX_RESULT_BYTES = LIMIT

    class Row(BaseModel):
        value: str

    class Output(BaseModel):
        results: list[Row]

    app = FastAPI()
    AppLoader.add_exception_handlers(app)

    @app.get("/api/v1/equity/estimates/forward_eps", response_model=Output)
    async def estimate(provider: str, rows: int):
        return {"results": [{"value": "x" * 100} for _ in range(rows)]}

    server = create_mcp_server(
        MCPSettings(
            name="Result size",
            default_tool_categories=[],
            allowed_tool_categories=["equity"],
            enable_tool_discovery=True,
            describe_responses=False,
            mask_error_details=True,
        ),
        retain_read_only_routes(app, {"equity"}, "/api/v1"),
    )
    mark_registered_tools_read_only(server, {})
    server.run("stdio", show_banner=False)


class Session:
    def __init__(self) -> None:
        # This checkout, not another interpreter's editable install. A worktree
        # console script can run under the main checkout's Python.
        src = Path(__file__).resolve().parents[1] / "src"
        env = os.environ.copy()
        previous = env.get("PYTHONPATH")
        env["PYTHONPATH"] = (
            str(src) if previous is None else f"{src}{os.pathsep}{previous}"
        )
        self.child = subprocess.Popen(
            [sys.executable, __file__],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            env=env,
        )
        self.lines: queue.Queue[bytes] = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()
        self.next_id = 0

    def _read(self) -> None:
        for line in self.child.stdout:
            self.lines.put(line)

    def send(self, method: str, params: dict, *, notify: bool = False) -> int | None:
        message = {"jsonrpc": "2.0", "method": method, "params": params}
        request_id = None
        if not notify:
            self.next_id += 1
            request_id = message["id"] = self.next_id
        self.child.stdin.write(json.dumps(message).encode() + b"\n")
        self.child.stdin.flush()
        return request_id

    def replies(self, ids: set[int]) -> dict[int, dict]:
        found = {}
        while ids - found.keys():
            line = self.lines.get(timeout=60)
            # Every line the host reads stays within its frame.
            assert len(line) <= LIMIT + 1024, len(line)
            message = json.loads(line)
            if message.get("id") in ids:
                found[message["id"]] = message
        return found

    def call(self, name: str, arguments: dict) -> int:
        return self.send("tools/call", {"name": name, "arguments": arguments})

    def close(self) -> None:
        self.child.stdin.close()
        self.child.kill()
        self.child.wait(timeout=10)


def test_an_oversized_reply_fails_one_call_and_the_session_keeps_serving() -> None:
    name = "equity_estimates_forward_eps"
    session = Session()
    try:
        initialize = session.send(
            "initialize",
            {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1"},
            },
        )
        session.replies({initialize})
        session.send("notifications/initialized", {}, notify=True)
        activate = session.call("activate_tools", {"tool_names": [name]})
        assert "error" not in session.replies({activate})[activate]

        # An oversized read and a small read sent beside it.
        large = session.call(name, {"provider": "fixture", "rows": 100})
        beside = session.call(name, {"provider": "fixture", "rows": 1})
        replies = session.replies({large, beside})
        refused = replies[large]["result"]
        assert refused["isError"] is True
        marker = refused["structuredContent"]["loupist_error"]
        assert marker["kind"] == "result_too_large"
        assert marker["provider"] == "fixture"
        assert marker["limit_bytes"] == LIMIT < marker["bytes"]
        assert replies[beside]["result"]["structuredContent"] == {
            "results": [{"value": "x" * 100}]
        }

        # The next call on the same session succeeds.
        after = session.call(name, {"provider": "fixture", "rows": 2})
        result = session.replies({after})[after]["result"]
        assert not result.get("isError")
        assert len(result["structuredContent"]["results"]) == 2
    finally:
        session.close()


if __name__ == "__main__":
    serve()
