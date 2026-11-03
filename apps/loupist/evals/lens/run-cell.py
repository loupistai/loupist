#!/usr/bin/env python3
"""Journal one existing production agent_eval invocation; never inspect auth files."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import uuid


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--binary", required=True)
    p.add_argument("--suite", required=True)
    p.add_argument("--output", required=True)
    p.add_argument("--pi-agent-dir", required=True)
    p.add_argument("--trial", type=int, required=True)
    p.add_argument("--condition", required=True)
    p.add_argument("--version", required=True)
    p.add_argument("--model", default="openai-codex/gpt-6.1-sol")
    p.add_argument("--thinking", default="high")
    p.add_argument("--turn-timeout-ms", type=int, default=600000)
    a = p.parse_args()
    out = Path(a.output).resolve()
    out.mkdir(parents=True, exist_ok=False)
    trace = out / "traces.jsonl"
    command = [str(Path(a.binary).resolve()), "--suite", str(Path(a.suite).resolve()),
               "--out", str(trace), "--trials", "1", "--keep", "--model", a.model,
               "--thinking", a.thinking, "--pi-agent-dir", a.pi_agent_dir,
               "--turn-timeout-ms", str(a.turn_timeout_ms)]
    suite = json.loads(Path(a.suite).read_text())
    manifest = {"schema": "lens-causal-attempt/1", "attempt_id": str(uuid.uuid4()),
                "started_at": now(), "status": "interrupted", "trial": a.trial,
                "condition": a.condition, "version": a.version, "command": command,
                "suite_sha256": digest(a.suite), "binary_sha256": digest(a.binary),
                "planned_turns": sum(len(s["turns"]) for c in suite["scenarios"] for s in c["sessions"])}
    (out / "pending.json").write_text(json.dumps(manifest, indent=2) + "\n")
    with (out / "runner.log").open("x") as log:
        result = subprocess.run(command, stdout=log, stderr=log)
    manifest.update(status="completed" if result.returncode == 0 else "failed",
                    finished_at=now(), exit_code=result.returncode)
    (out / "attempt.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({k: manifest[k] for k in ["attempt_id", "condition", "trial", "status", "exit_code"]}), flush=True)
    raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
