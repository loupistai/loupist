"""Exercise release helper startup from disposable hardened, individually signed copies."""

import argparse
import os
import platform
import queue
import runpy
import subprocess
import tempfile
import threading
from pathlib import Path

import macos_release_signing as signing
import stage_python_runtime as worker
from release_profile import load_profile, check_team

APP = Path(__file__).resolve().parents[1]
SMOKE = runpy.run_path(str(APP / "scripts/check-sidecars.py"))


def pi_state(pi, scratch, log):
    env = SMOKE["pi_environment"](pi)
    env.update(
        PI_CODING_AGENT_DIR=str(scratch / "agent"),
        GEMINI_API_KEY="offline-fixture",
        HOME=str(scratch),
        LOUPIST_PROVIDER_RESULT_FILE=str(scratch / "result"),
    )
    args = [
        pi / "loupist-pi",
        "--mode",
        "rpc",
        "--no-session",
        "--no-builtin-tools",
        "--no-extensions",
        "--extension",
        APP / "agent/loupist-provider-extension.mjs",
        "--no-skills",
        "--no-prompt-templates",
        "--no-themes",
        "--no-context-files",
        "--offline",
        "--provider",
        "google",
        "--model",
        "gemini-3.5-flash-lite",
    ]
    process = subprocess.Popen(
        args,
        cwd=scratch,
        env=env,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=log,
        text=True,
        start_new_session=True,
    )
    completed = False
    try:
        lines = queue.Queue()
        threading.Thread(
            target=SMOKE["stream_lines"], args=(process.stdout, lines), daemon=True
        ).start()
        SMOKE["send_json"](process, {"type": "get_state", "id": "state"})
        response, _ = SMOKE["read_response"](lines, "state", 30)
        if response.get("type") != "response" or response.get("success") is not True:
            raise ValueError("Pi refused get_state")
        SMOKE["stop_root_process"](process)
        completed = True
    finally:
        SMOKE["finish_smoke_process"](process, process.pid, None, completed=completed)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "identity",
        help="Developer ID Application name or SHA-1, or - for a partial ad-hoc check",
    )
    parser.add_argument("--no-timestamp", action="store_true")
    args = parser.parse_args()
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        parser.error("requires macOS Apple Silicon")
    adhoc = args.identity == "-"
    if adhoc and not args.no_timestamp:
        parser.error("ad-hoc checks require --no-timestamp")
    team = None if adhoc else check_team(load_profile()["macos_signing"]["team_id"])
    timestamp = not args.no_timestamp
    os.environ.update(
        APPLE_SIGNING_IDENTITY=args.identity, LOUPIST_MACOS_SIGNING_MODE="developer-id"
    )
    resources = APP / "src-tauri/resources"
    with tempfile.TemporaryDirectory(prefix="loupist-hardened-") as directory:
        root = Path(directory).resolve()
        pi, py = root / "pi-runtime", root / "python-runtime"
        for target in (pi, py):
            signing.run("ditto", str(resources / target.name), str(target))
        # Stage Pi before OpenBB, exactly as the release does.
        openbb = root / "openbb"
        (pi / "openbb").rename(openbb)
        signing.sign_runtime(
            pi, "pi", "developer-id", args.identity, timestamp=timestamp
        )
        signing.sign_runtime(
            openbb, "openbb", "developer-id", args.identity, timestamp=timestamp
        )
        openbb.rename(pi / "openbb")
        openbb = pi / "openbb"
        worker.verify(py, worker.source_digest())
        worker.sign_staged_worker(py, timestamp=timestamp)
        binary = py / worker.BINARY
        for path in signing.macho_files(pi) + [binary]:
            if adhoc:
                signing.run("codesign", "--verify", "--strict", str(path))
            else:
                signing.verify_binary(
                    path,
                    "developer-id",
                    None,
                    team,
                    timestamp=timestamp,
                )
            signing.verify_flags(path, "developer-id", pi, binary)
        failed = False
        for name, probe in [
            ("Pi RPC get_state", lambda log: pi_state(pi, root, log)),
            (
                "OpenBB MCP initialize, tools/list and available_categories",
                lambda log: SMOKE["openbb_smoke"](
                    openbb / "loupist-openbb", openbb, startup_only=True, stderr=log
                ),
            ),
            ("Python WASI snapshot", lambda log: worker.smoke(py)),
        ]:
            with tempfile.TemporaryFile(mode="w+") as log:
                try:
                    probe(log)
                except (
                    OSError,
                    ValueError,
                    RuntimeError,
                    subprocess.SubprocessError,
                ) as error:
                    log.seek(0)
                    print(f"FAIL {name}: {error}\n{log.read()[-4000:]}", flush=True)
                    failed = True
                else:
                    print(f"PASS {name}", flush=True)
        if adhoc:
            print(
                "Ad-hoc run: release library validation requires an Apple team.",
                flush=True,
            )
        return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
