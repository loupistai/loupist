#!/usr/bin/env python3
"""Run the repository checks for one scope or all of them.

Each step runs once (no retries) in its own process group with a private temporary
directory and a timeout. Every run writes a receipt and one log per step under
`output/checks/<run>/`. A pass also requires the source tree to be unchanged
between the start and the end of the run.
"""

from __future__ import annotations

import argparse
import calendar
from contextlib import contextmanager
import fcntl
import glob
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import traceback
import uuid

APP = Path(__file__).resolve().parent.parent
SCOPES = ("web", "python", "rust")
RETENTION_SECONDS = 7 * 24 * 3600
NODE_VERSION = 'const pin=require("node:fs").readFileSync("../../.nvmrc","utf8").trim(); const min=pin.split(".").map(Number), v=process.versions.node.split(".").map(Number); if(v[0]!==min[0]||v[1]<min[1]||(v[1]===min[1]&&v[2]<min[2]))throw Error(`Node ${pin}+ LTS (${min[0]}.x) required`); console.log(process.version)'
INSTALLED = 'for (const p of process.argv.slice(1)) if (!require("node:fs").existsSync(p)) throw Error(`Locked dependencies missing: ${p}; run setup.sh first`)'
PROCESS_TOOLS = 'import shutil, sys; missing = [name for name in ("ps", "pgrep", "lsof") if shutil.which(name) is None]; sys.exit("Missing host process tools: " + ", ".join(missing) + "; on Debian/Ubuntu install procps and lsof" if missing else 0)'
SHELL_SYNTAX = 'for script; do IFS= read -r interpreter < "$script"; case "$interpreter" in *bash) bash -n "$script" ;; *) sh -n "$script" ;; esac || exit; done'
MODULE_SYNTAX = 'for module; do node --check "$module" || exit; done'
TAURI_E2E_CONFIG = 'JSON.parse(require("node:fs").readFileSync("src-tauri/tauri.e2e.conf.json", "utf8"))'
PYTHON_VERSION = "import sys; assert sys.version_info[:2] == (3, 12), sys.version"


def step(name, command, cwd=None):
    return {
        "name": name,
        "command": [str(part) for part in command],
        "cwd": str(cwd or APP),
    }


def matches(pattern):
    paths = sorted(glob.glob(str(APP / pattern)))
    if not paths:
        raise ValueError(f"no files matched required check group: {pattern}")
    return paths


def existing(*patterns):
    return sorted(
        path for pattern in patterns for path in glob.glob(str(APP / pattern))
    )


def contract_tests(harness_only=False):
    patterns = ["agent/*.test.mjs", "evals/*.test.mjs", "evals/agent/*.test.mjs"]
    patterns += ["evals/meta/*.test.mjs", "evals/world/*.test.mjs"]
    if not harness_only:
        patterns += ["e2e/*.test.mjs", "scripts/*.test.mjs", "../../scripts/*.test.mjs"]
    return [path for pattern in patterns for path in matches(pattern)]


def node(name, arguments):
    return step(name, ["node", *arguments.split()])


def web_plan(harness, install):
    lockfiles = ["node_modules/.package-lock.json"]
    commands = [step("node-version", ["node", "-e", NODE_VERSION])]
    if install:
        commands.append(step("install-app", ["npm", "ci"]))
        target = (
            "aarch64-apple-darwin"
            if platform.system() == "Darwin"
            else "x86_64-unknown-linux-musl"
        )
        commands.append(
            step(
                "install-workspace-search",
                [
                    sys.executable,
                    APP / "scripts/stage_workspace_tools.py",
                    "--ensure",
                    "--runtime",
                    APP / "src-tauri/resources/pi-runtime",
                    "--target",
                    target,
                ],
            )
        )
    if not harness:
        lockfiles.append("e2e/node_modules/.package-lock.json")
        if install:
            commands.append(step("install-e2e", ["npm", "ci"], APP / "e2e"))
    commands.append(
        step("installed-dependencies", ["node", "-e", INSTALLED, *lockfiles])
    )
    if harness:
        tests = contract_tests(harness_only=True)
        return [*commands, step("agent-eval-contracts", ["node", "--test", *tests])]
    shells = existing("e2e/*.sh", "scripts/*.sh")
    modules = existing("agent/*.mjs", "agent/*/*.mjs", "e2e/*.mjs", "scripts/*.mjs")
    modules += existing("evals/*.mjs", "evals/*/*.mjs")
    modules = [path for path in modules if not path.endswith(".test.mjs")]
    return [
        *commands,
        step("process-tools", [sys.executable, "-c", PROCESS_TOOLS]),
        step(
            "repository-guidance", [sys.executable, "../../scripts/guide.py", "--check"]
        ),
        step("shell-syntax", ["sh", "-c", SHELL_SYNTAX, "sh", *shells]),
        step("tauri-e2e-config", ["node", "-e", TAURI_E2E_CONFIG]),
        step("module-syntax", ["sh", "-c", MODULE_SYNTAX, "sh", *modules]),
        step("node-contracts", ["node", "--test", *contract_tests()]),
        step("e2e-security", ["npm", "run", "verify-security"], APP / "e2e"),
        node("i18n-report", "scripts/check-i18n.mjs --report"),
        node("i18n-parity", "scripts/check-i18n-parity.mjs"),
        node("ui-copy", "scripts/check-ui-copy.mjs"),
        node("design-tokens", "scripts/check-design-tokens.mjs --enforce"),
        node("boundaries", "scripts/check-boundaries.mjs --enforce"),
        step("unused-files", ["npm", "run", "check:unused"]),
        step("renderer-tests", ["npm", "test"]),
        step("chat-layout", ["npm", "run", "preview:layout"]),
        step("renderer-build", ["npm", "run", "build"]),
    ]


def cargo(arguments):
    command, *rest = arguments.split()
    return ["cargo", command, "--manifest-path", APP / "src-tauri/Cargo.toml", *rest]


def python_runtime_step(name, flag):
    return step(name, [sys.executable, APP / "scripts/stage_python_runtime.py", flag])


def rust_plan(install=False):
    steps = [
        step("format", cargo("fmt -- --check")),
        step("clippy", cargo("clippy --locked --all-targets -- -D warnings")),
        # The native E2E build compiles feature-gated fixtures that the default build skips.
        step(
            "e2e-feature-clippy",
            cargo(
                "clippy --locked --all-targets --no-default-features --features e2e -- -D warnings"
            ),
        ),
        step("tests", cargo("test --locked --all-targets")),
        step(
            "headless-eval-contracts",
            cargo("test --locked --features eval-harness --lib eval_harness::"),
        ),
        python_runtime_step("python-runtime-verify", "--verify"),
    ]
    if install:
        steps.insert(0, python_runtime_step("install-python-runtime", "--ensure"))
    return steps


def python_plan(install):
    openbb = APP / "openbb"
    scripts, e2e = APP / "scripts", APP / "e2e"
    uv = ["uv", "run", "--locked", "--no-sync"]
    py = ["uv", "run", "--project", openbb, "--locked", "--no-sync", "python"]
    plist = ["--source-plist", APP / "src-tauri/Info.plist"]
    tauri = ["--tauri-config", APP / "src-tauri/tauri.conf.json"]
    sources = [*matches("scripts/*.py"), *matches("e2e/*.py")]
    commands = [
        step(
            f"install-{path.name}",
            ["uv", "sync", "--project", path, "--locked", "--python", "3.12"],
        )
        for path in (openbb,)
        if install
    ]
    return [
        *commands,
        step(
            "bundle-version",
            [*py, scripts / "check-macos-bundle-version.py", *tauri, *plist],
        ),
        step("python-version", [*py, "-c", PYTHON_VERSION]),
        step("python-syntax", [*py, "-m", "py_compile", *sources]),
        step("release-version", [*py, scripts / "set-version.py", "--check"]),
        step(
            "script-tests",
            [*py, "-m", "unittest", "discover", "-s", scripts, "-p", "test_*.py"],
        ),
        step("openbb-lint", [*uv, "ruff", "check", ".", scripts, e2e], openbb),
        step(
            "openbb-format",
            [*uv, "ruff", "format", "--check", ".", scripts, e2e],
            openbb,
        ),
        step("openbb-tests", [*uv, "pytest"], openbb),
        step("openbb-build-contract", [*py, openbb / "build_sidecar.py", "--check"]),
    ]


def plan(scope, install=False):
    if scope in ("web", "harness"):
        return web_plan(scope == "harness", install)
    return rust_plan(install) if scope == "rust" else python_plan(install)


def setup_steps(scope, steps):
    """Dependency installation only; it never claims verification."""
    if scope == "rust":
        return [
            step("install-cargo", cargo("fetch --locked")),
            python_runtime_step("install-python-runtime", "--ensure"),
        ]
    return [
        s
        for s in steps
        if s["name"] == "node-version" or s["name"].startswith("install-")
    ]


@contextmanager
def exclusive_lock(path):
    """Hold an flock on `path`; the inode is never unlinked, so waiters stay safe."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+") as handle:
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise RuntimeError(
                f"check resource is busy: {path}; use another checkout or wait for its owner"
            ) from error
        # Close only (no LOCK_UN): a child that inherited the descriptor keeps the
        # lock until it exits, even if this runner is killed first.
        yield handle.fileno()


def group_alive(group):
    rows = subprocess.check_output(["ps", "-axo", "pgid=,stat="], text=True, timeout=5)
    return any(
        parts[0] == str(group) and not parts[1].startswith("Z")
        for parts in (row.split() for row in rows.splitlines())
        if len(parts) >= 2
    )


def stop_process(process):
    """Stop the step's whole process group, including background descendants."""
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.killpg(process.pid, sig)
        except ProcessLookupError:
            break
        except PermissionError:
            # Darwin reports EPERM for a group that only holds zombies.
            if (
                sys.platform == "darwin"
                and process.poll() is not None
                and not group_alive(process.pid)
            ):
                break
            raise
        if sig == signal.SIGTERM:
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                pass
    process.wait()


def execute(item, directory, timeout, cancelled, lock_fds=()):
    started = time.monotonic()
    result = {
        **item,
        "started_at": time.time(),
        "log": f"{item['scope']}-{item['name']}.log",
    }
    process = None
    with (
        (directory / result["log"]).open("wb") as log,
        # A short /tmp path leaves room for UNIX socket names created by tests.
        tempfile.TemporaryDirectory(prefix="loupist-check-", dir="/tmp") as temporary,
    ):
        environment = {
            **os.environ,
            "TMPDIR": temporary,
            "TMP": temporary,
            "TEMP": temporary,
            # The gate rebuilds from a sealed tree; incremental session caches
            # only grow target/ (several GB per run) without speeding it up.
            "CARGO_INCREMENTAL": "0",
        }
        if item["name"] == "python-syntax":
            environment["PYTHONPYCACHEPREFIX"] = str(Path(temporary) / "pycache")
        if item["name"] == "chat-layout":
            result["command"] = [
                *item["command"],
                "--",
                "--out",
                str(directory.resolve() / "chat-layout"),
            ]
        try:
            process = subprocess.Popen(
                result["command"],
                cwd=item["cwd"],
                env=environment,
                stdout=log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
                pass_fds=lock_fds,
            )
            status = None
            while status is None and process.poll() is None:
                if cancelled.wait(0.1):
                    status = "cancelled"
                elif time.monotonic() - started > timeout:
                    status = "timed_out"
            if status:
                stop_process(process)
            result["exit_code"] = process.wait()
            result["status"] = status or (
                "passed" if process.returncode == 0 else "failed"
            )
        except OSError as error:
            result.update(status="failed", exit_code=None, error=str(error))
            log.write(str(error).encode())
        finally:
            if process is not None:
                try:
                    stop_process(process)  # Also reaps background descendants.
                except (OSError, subprocess.SubprocessError) as error:
                    # A cleanup failure cannot erase the command's own outcome.
                    result["command_status"] = result.get("status")
                    result.update(status="failed", cleanup_error=str(error))
                    log.write(traceback.format_exc().encode())
    result.update(
        duration_seconds=round(time.monotonic() - started, 3), finished_at=time.time()
    )
    return result


def git(*args, cwd, env=None, timeout=60):
    return subprocess.check_output(
        ["git", *args],
        cwd=cwd,
        env=env,
        stderr=subprocess.DEVNULL,
        text=True,
        timeout=timeout,
    ).strip()


def revision():
    """Identify the source under test: commit, dirtiness and a working-tree id.

    The tree id is `git write-tree` over a private copy of the index after
    `git add -A`, so it covers tracked and unignored files; generated Tauri
    schemas are left out because builds rewrite them. Objects it hashes go to
    a temporary object directory, so untracked files never land in the
    repository's object store.
    """
    result = {"commit": None, "dirty": None, "source_tree": None}
    try:
        root = Path(git("rev-parse", "--show-toplevel", cwd=APP))
        try:
            result["commit"] = git("rev-parse", "--verify", "HEAD", cwd=root)
        except subprocess.CalledProcessError:
            pass  # A repository without commits still has a working tree.
        result["dirty"] = bool(git("status", "--porcelain", cwd=root))
        index = Path(
            git("rev-parse", "--path-format=absolute", "--git-path", "index", cwd=root)
        )
        objects = git(
            "rev-parse", "--path-format=absolute", "--git-path", "objects", cwd=root
        )
        with tempfile.TemporaryDirectory() as temporary:
            copy = Path(temporary) / "index"
            if index.exists():
                shutil.copyfile(index, copy)
            scratch_objects = Path(temporary) / "objects"
            scratch_objects.mkdir()
            env = {
                **os.environ,
                "GIT_INDEX_FILE": str(copy),
                "GIT_OBJECT_DIRECTORY": str(scratch_objects),
                "GIT_ALTERNATE_OBJECT_DIRECTORIES": objects,
            }
            git("add", "-A", "--", ".", cwd=root, env=env, timeout=300)
            git(
                "rm",
                "-r",
                "-q",
                "--cached",
                "--ignore-unmatch",
                "--",
                "apps/loupist/src-tauri/gen",
                cwd=root,
                env=env,
            )
            result["source_tree"] = git("write-tree", cwd=root, env=env)
    except (OSError, subprocess.SubprocessError) as error:
        result["source_error"] = str(error) or type(error).__name__
    return result


def guard_active_app(scope, install):
    """Do not replace dependencies or rebuild Rust under a live native session."""
    if scope != "rust" and not install:
        return
    path = APP / "e2e/artifacts/current-session.json"
    if not path.exists():
        return
    try:
        session = json.loads(path.read_text())
        pid = session.get("launcherPid")
        if not isinstance(pid, int) or pid <= 0:
            raise ValueError("missing launcher identity")
        process = subprocess.run(
            ["ps", "-p", str(pid), "-o", "lstart=", "-o", "stat="],
            capture_output=True,
            text=True,
            timeout=5,
            env={**os.environ, "LC_ALL": "C"},
        )
        identity = process.stdout.strip().rsplit(None, 1)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        raise RuntimeError(
            "Cannot verify the existing app receipt; preserve it and inspect its owner before installing or running Rust checks."
        ) from error
    live = (
        process.returncode == 0
        and len(identity) == 2
        and not identity[1].startswith("Z")
    )
    if live and identity[0].strip() == session.get("launcherStartedAt"):
        raise RuntimeError(
            "This checkout has a live native app session. Dependency installation and Rust checks can interrupt it; use a separate checkout or wait for its owner to stop it. No app was stopped."
        )


def prune_old_runs(output, now=None):
    """Remove run directories older than the retention window; nothing else."""
    cutoff = (now or time.time()) - RETENTION_SECONDS
    for path in output.glob("????????T??????Z-????????"):
        try:
            created = calendar.timegm(time.strptime(path.name[:16], "%Y%m%dT%H%M%SZ"))
        except ValueError:
            continue
        if created < cutoff and path.is_dir() and not path.is_symlink():
            shutil.rmtree(path, ignore_errors=True)


def run(
    scopes, output, timeout, install=False, setup_only=False, plans=None, lock_root=None
):
    output.mkdir(parents=True, exist_ok=True)
    directory = (
        output
        / f"{time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())}-{uuid.uuid4().hex[:8]}"
    )
    directory.mkdir(mode=0o700)
    lock_root = lock_root or output / ".locks"
    receipt = {
        "schema": 1,
        "kind": "loupist-check-run",
        "status": "running",
        "started_at": time.time(),
        "finished_at": None,
        "revision": revision(),
        "host": {
            "system": platform.system(),
            "machine": platform.machine(),
            "python": platform.python_version(),
        },
        "requested_scopes": list(scopes),
        "step_timeout_seconds": timeout,
        "install": install,
        "setup_only": setup_only,
        "steps": [],
        "errors": [],
    }
    for scope in scopes:
        selected = plans[scope] if plans is not None else plan(scope, install)
        if setup_only:
            selected = setup_steps(scope, selected)
        receipt["steps"] += [
            {**item, "scope": scope, "status": "not_run"} for item in selected
        ]

    def save():
        temporary = directory / "receipt.tmp"
        temporary.write_text(json.dumps(receipt, indent=2) + "\n")
        temporary.replace(directory / "receipt.json")

    def fail(scope, message):
        receipt["errors"].append({"scope": scope, "error": message})
        print(f"[{scope}] {message}", file=sys.stderr, flush=True)

    def run_scope(scope):
        indices = [
            i for i, item in enumerate(receipt["steps"]) if item["scope"] == scope
        ]
        python_installs = [
            s
            for s in receipt["steps"]
            if s["scope"] == "python" and s["name"].startswith("install-")
        ]
        if (
            scope == "rust"
            and not setup_only
            and any(s["status"] != "passed" for s in python_installs)
        ):
            return fail(
                scope, "Rust checks require successful Python dependency installation"
            )
        lock = lock_root / ("web.lock" if scope == "harness" else f"{scope}.lock")
        try:
            with exclusive_lock(lock) as lock_fd:
                guard_active_app(scope, install)
                for index in indices:
                    if cancelled.is_set():
                        return
                    item = receipt["steps"][index]
                    item.update(status="running", log=f"{scope}-{item['name']}.log")
                    save()
                    print(f"[{scope}] {item['name']} started", flush=True)
                    result = execute(item, directory, timeout, cancelled, (lock_fd,))
                    receipt["steps"][index] = result
                    save()
                    print(
                        f"[{scope}] {item['name']}: {result['status']} ({result['duration_seconds']}s)",
                        flush=True,
                    )
                    if result["status"] != "passed":
                        return  # Later scopes still run, so their evidence is kept.
        except Exception as error:
            for index in indices:
                if receipt["steps"][index]["status"] == "running":
                    receipt["steps"][index].update(
                        status="failed", error=str(error), finished_at=time.time()
                    )
                    with (directory / receipt["steps"][index]["log"]).open("a") as log:
                        log.write(traceback.format_exc())
            fail(scope, str(error))

    cancelled = threading.Event()
    previous = {}
    if threading.current_thread() is threading.main_thread():
        for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            previous[sig] = signal.signal(sig, lambda _sig, _frame: cancelled.set())
    try:
        save()
        print(f"Check evidence: {directory / 'receipt.json'}", flush=True)
        ordered = [scope for scope in SCOPES if scope in scopes] + [
            s for s in scopes if s not in SCOPES
        ]
        for scope in ordered:
            if not cancelled.is_set():
                run_scope(scope)
        receipt["revision_after"] = revision()
        before, after = (
            receipt["revision"]["source_tree"],
            receipt["revision_after"]["source_tree"],
        )
        receipt["source_changed_during_run"] = (
            before != after if before and after else None
        )
        if not before or not after:
            fail(
                "source",
                "Source fingerprint unavailable; step outcomes are retained, but source qualification failed.",
            )
        elif before != after:
            fail(
                "source",
                "Source changed during checks; step outcomes are retained, but this run cannot qualify one source state.",
            )
        passed = not receipt["errors"] and all(
            item["status"] == "passed" for item in receipt["steps"]
        )
        receipt["status"] = (
            "cancelled" if cancelled.is_set() else "passed" if passed else "failed"
        )
        receipt["finished_at"] = time.time()
        receipt["duration_seconds"] = round(
            receipt["finished_at"] - receipt["started_at"], 3
        )
        save()
    finally:
        for sig, handler in previous.items():
            signal.signal(sig, handler)
    print(f"Checks {receipt['status']}: {directory / 'receipt.json'}", flush=True)
    for item in receipt["steps"]:
        if item["status"] in ("failed", "timed_out", "cancelled"):
            print(f"Failure log: {directory / item['log']}", file=sys.stderr)
    return receipt, directory


def positive(value):
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError("must be greater than zero")
    return number


def main(argv=None):
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "scope",
        nargs="?",
        choices=("all", "web", "rust", "python", "harness"),
        default="all",
    )
    parser.add_argument(
        "--timeout",
        type=positive,
        default=1800,
        help="seconds per command (default: 1800)",
    )
    parser.add_argument(
        "--install", action="store_true", help="install locked dependencies first"
    )
    parser.add_argument(
        "--setup-only", action="store_true", help="install locked dependencies and stop"
    )
    parser.add_argument(
        "--list",
        action="store_true",
        help="print the planned steps without running them",
    )
    parser.add_argument(
        "--only",
        help="comma-separated step names to run from the selected scope (see --list)",
    )
    parser.add_argument(
        "--shard",
        help="run one Vitest shard, such as 1/2, in the renderer-tests step",
    )
    parser.add_argument("--output", type=Path, default=APP / "output/checks")
    args = parser.parse_args(argv)
    if os.name != "posix":
        parser.error(
            "use this check gate on macOS/Linux; Windows has its own compatibility workflow"
        )
    if not shutil.which("git"):
        parser.error("git is required for source evidence")
    scopes = list(SCOPES) if args.scope == "all" else [args.scope]
    install = args.install or args.setup_only
    plans = {scope: plan(scope, install) for scope in scopes}
    if args.only is not None:
        wanted = {name for name in args.only.split(",") if name}
        known = {item["name"] for steps in plans.values() for item in steps}
        if not wanted or wanted - known:
            parser.error(
                f"--only names unknown steps {sorted(wanted - known) or '(none)'}; "
                f"choose from {sorted(known)}"
            )
        plans = {
            scope: [item for item in steps if item["name"] in wanted]
            for scope, steps in plans.items()
        }
    if args.shard is not None:
        index, _, total = args.shard.partition("/")
        renderer = [
            i for s in plans.values() for i in s if i["name"] == "renderer-tests"
        ]
        if not (index.isdigit() and total.isdigit() and 0 < int(index) <= int(total)):
            parser.error("--shard must be INDEX/TOTAL, such as 1/2")
        if not renderer:
            parser.error("--shard needs a plan that includes renderer-tests")
        for item in renderer:
            item["command"] = [*item["command"], "--", f"--shard={args.shard}"]
    if args.list:
        for scope in scopes:
            steps = plans[scope]
            for item in setup_steps(scope, steps) if args.setup_only else steps:
                words = [part.replace(f"{APP}/", "") for part in item["command"]]
                shown = " ".join(w if len(w) <= 40 else w[:37] + "..." for w in words)
                print(f"[{scope}] {item['name']}: {shown[:160]}")
        return 0
    output = args.output.resolve()
    if output == APP / "output/checks":
        prune_old_runs(output)
    receipt, _ = run(scopes, output, args.timeout, install, args.setup_only, plans)
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
