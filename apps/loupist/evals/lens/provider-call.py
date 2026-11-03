#!/usr/bin/env python3
"""An isolated JSON inference using the existing credential-owning Pi extension.

No Chat tools, file access, web tools, auth-file reads, or automatic paid retries.
Only the extension's bounded result is retained; raw process output is discarded.
"""
import argparse
import datetime
import json
import os
from pathlib import Path
import signal
import subprocess
import time

p = argparse.ArgumentParser()
p.add_argument("--request", required=True)
p.add_argument("--output", required=True)
p.add_argument("--runtime", required=True)
p.add_argument("--extension", required=True)
p.add_argument("--pi-agent-dir", required=True)
a = p.parse_args()
out = Path(a.output).resolve()
out.mkdir(parents=True, exist_ok=False)
result = out / "result.json"
result.write_text("")
result.chmod(0o600)
request = Path(a.request).resolve()
value = json.loads(request.read_text())
pending = {"started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
           "request": str(request), "model": value["provider"] + "/" + value["model"],
           "thinking": value["thinkingLevel"], "status": "interrupted"}
(out / "pending.json").write_text(json.dumps(pending, indent=2) + "\n")
runtime = Path(a.runtime).resolve()
command = [str(runtime / "loupist-pi"), "--mode", "rpc", "--no-session",
           "--no-builtin-tools", "--no-extensions", "--extension", str(Path(a.extension).resolve()),
           "--no-skills", "--no-prompt-templates", "--no-themes", "--no-context-files",
           "--provider", "google", "--model", "gemini-2.5-flash"]
env = {"PI_CODING_AGENT_DIR": a.pi_agent_dir, "PI_PACKAGE_DIR": str(runtime),
       "PI_TELEMETRY": "0", "GEMINI_API_KEY": "loupist-provider-bootstrap",
       "LOUPIST_PROVIDER_RESULT_FILE": str(result),
       "LOUPIST_PROVIDER_REQUEST_FILE": str(request)}
child = subprocess.Popen(command, cwd=out, env=env, stdin=subprocess.PIPE,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                         text=True, start_new_session=True)
completed = None
try:
    child.stdin.write(json.dumps({"id": "eval-inference", "type": "prompt", "message": "/loupist-provider-inference"}) + "\n")
    child.stdin.flush()
    deadline = time.monotonic() + value["timeoutMs"] / 1000 + 15
    while child.poll() is None and time.monotonic() < deadline:
        try:
            candidate = json.loads(result.read_text())
            if candidate.get("type") == "inference" and candidate.get("body", {}).get("status") != "in_flight":
                completed = candidate
                break
        except (json.JSONDecodeError, OSError):
            pass
        time.sleep(0.2)
finally:
    if child.poll() is None:
        os.killpg(child.pid, signal.SIGTERM)
        child.wait(timeout=10)
pending.update(finished_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
               status=completed.get("disposition") if completed else "interrupted")
(out / "attempt.json").write_text(json.dumps(pending, indent=2) + "\n")
print(json.dumps({"status": pending["status"], "output": str(out)}))
raise SystemExit(0 if pending["status"] == "completed" else 1)
