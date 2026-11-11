"""Exercise the check gate with real subprocesses, locks and a real Git checkout."""

from __future__ import annotations

import json
import os
from contextlib import chdir
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

import check_runner as checks


def touch(path):
    return f"from pathlib import Path; Path({str(path)!r}).touch()"


class CheckRunnerTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source = self.root / "source"
        self.app = self.source / "apps/loupist"
        self.app.mkdir(parents=True)
        subprocess.run(["git", "init", "-q", str(self.source)], check=True)
        patcher = patch.object(checks, "APP", self.app)
        patcher.start()
        self.addCleanup(patcher.stop)

    def command(self, name, code):
        return checks.step(name, [sys.executable, "-c", code], self.root)

    def run_plans(self, plans, timeout=5, install=False):
        return checks.run(
            list(plans),
            self.root / "output",
            timeout,
            install=install,
            plans=plans,
            lock_root=self.root / "locks",
        )

    def item(self, name, code):
        return {**self.command(name, code), "scope": "web"}

    def test_failure_is_kept_without_retry_and_other_scopes_still_run(self):
        attempts = self.root / "attempts"
        failing = f"from pathlib import Path; p = Path({str(attempts)!r}); p.write_text((p.read_text() if p.exists() else '') + 'x'); print('real failure'); raise SystemExit(7)"
        receipt, directory = self.run_plans(
            {
                "web": [
                    self.command("fail", failing),
                    self.command("dependent", "raise SystemExit('must not run')"),
                ],
                "python": [self.command("independent", "pass")],
            }
        )
        self.assertEqual(receipt["status"], "failed")
        statuses = [item["status"] for item in receipt["steps"]]
        self.assertEqual(statuses, ["failed", "not_run", "passed"])
        self.assertEqual(receipt["steps"][0]["exit_code"], 7)
        self.assertEqual(attempts.read_text(), "x")
        self.assertIn("real failure", (directory / "web-fail.log").read_text())
        self.assertEqual(json.loads((directory / "receipt.json").read_text()), receipt)

    def test_only_lists_the_named_steps_and_refuses_unknown_names(self):
        from contextlib import redirect_stderr, redirect_stdout
        from io import StringIO

        listed = StringIO()
        with redirect_stdout(listed):
            self.assertEqual(
                checks.main(["rust", "--list", "--only", "clippy,tests"]), 0
            )
        names = [
            line.split("]", 1)[1].split(":", 1)[0].strip()
            for line in listed.getvalue().splitlines()
        ]
        self.assertEqual(names, ["clippy", "tests"])
        for only in ("clipy", ""):
            with self.subTest(only=only), redirect_stderr(StringIO()):
                with self.assertRaises(SystemExit) as refused:
                    checks.main(["rust", "--list", "--only", only])
                self.assertEqual(refused.exception.code, 2)

    def test_shard_reaches_only_the_renderer_test_command(self):
        from contextlib import redirect_stderr, redirect_stdout
        from io import StringIO

        real_app = patch.object(
            checks, "APP", Path(checks.__file__).resolve().parents[1]
        )
        real_app.start()
        self.addCleanup(real_app.stop)
        listed = StringIO()
        with redirect_stdout(listed):
            checks.main(["web", "--list", "--shard", "2/2"])
        sharded = [line for line in listed.getvalue().splitlines() if "--shard" in line]
        self.assertEqual(len(sharded), 1)
        self.assertIn("renderer-tests: npm test -- --shard=2/2", sharded[0])
        for scope, shard in (
            ("web", "3/2"),
            ("web", "0/2"),
            ("web", "a/b"),
            ("rust", "1/2"),
        ):
            with self.subTest(scope=scope, shard=shard), redirect_stderr(StringIO()):
                with self.assertRaises(SystemExit) as refused:
                    checks.main([scope, "--list", "--shard", shard])
                self.assertEqual(refused.exception.code, 2)

    def test_affected_selects_ci_scopes_from_commits_and_the_working_tree(self):
        from contextlib import redirect_stderr, redirect_stdout
        from io import StringIO

        def git(*arguments):
            identity = [
                "-c",
                "user.name=check",
                "-c",
                "user.email=check@example.invalid",
            ]
            subprocess.run(
                ["git", *identity, *arguments],
                cwd=self.source,
                check=True,
                capture_output=True,
            )

        def affected(base="base"):
            with redirect_stdout(StringIO()), redirect_stderr(StringIO()):
                return checks.affected_scopes(base)

        (self.source / "README.md").write_text("one\n")
        git("add", "-A")
        git("commit", "-qm", "base")
        git("branch", "base")
        self.assertEqual(affected(), [])
        (self.source / "README.md").write_text("two\n")
        self.assertEqual(affected(), [], "a guide edit selects no scope")
        (self.app / "src").mkdir()
        (self.app / "src/App.test.tsx").write_text("")
        self.assertEqual(affected(), ["web"], "an untracked file counts")
        git("add", "-A")
        git("commit", "-qm", "renderer test")
        (self.app / "src-tauri/tests").mkdir(parents=True)
        (self.app / "src-tauri/tests/store.rs").write_text("")
        git("add", "-A")
        self.assertEqual(affected(), ["web", "rust"], "commits and the index add up")
        (self.source / "unclassified.txt").write_text("")
        self.assertEqual(affected(), list(checks.SCOPES))
        self.assertEqual(affected("no-such-revision"), list(checks.SCOPES))

    def test_affected_always_checks_the_repository_and_installs_what_rust_needs(self):
        from contextlib import redirect_stderr, redirect_stdout
        from io import StringIO

        real_app = patch.object(
            checks, "APP", Path(checks.__file__).resolve().parents[1]
        )
        real_app.start()
        self.addCleanup(real_app.stop)

        def listed(scopes, *arguments):
            output = StringIO()
            with (
                patch.object(checks, "affected_scopes", return_value=scopes),
                redirect_stdout(output),
            ):
                checks.main(["all", "--affected", "--list", *arguments])
            rows = [
                line[1:].split(": ", 1)[0] for line in output.getvalue().splitlines()
            ]
            return [tuple(row.split("] ", 1)) for row in rows]

        guides_only = listed([], "--install")
        self.assertEqual({scope for scope, _ in guides_only}, {"repository"})
        self.assertIn(("repository", "repository-guidance"), guides_only)
        rust = listed(["rust"], "--install")
        self.assertEqual(
            list(dict.fromkeys(scope for scope, _ in rust)),
            ["repository", "web", "python", "rust"],
        )
        self.assertIn(("rust", "tests"), rust)
        for scope, name in rust:
            if scope in ("web", "python"):
                self.assertTrue(name == "node-version" or name.startswith("install-"))
        self.assertEqual(
            {scope for scope, _ in listed(["rust"])}, {"repository", "rust"}
        )
        with redirect_stderr(StringIO()), self.assertRaises(SystemExit) as refused:
            checks.main(["web", "--affected", "--list"])
        self.assertEqual(refused.exception.code, 2)

    def test_full_web_includes_root_contracts_from_another_working_directory(self):
        app = Path(checks.__file__).resolve().parents[1]
        contract = app.parents[1] / "scripts/runs.test.mjs"
        with patch.object(checks, "APP", app), chdir(self.root):
            web = checks.plan("web")
            harness = checks.plan("harness")
        contracts = next(item for item in web if item["name"] == "node-contracts")
        self.assertIn(
            contract, [Path(arg).resolve() for arg in contracts["command"][2:]]
        )
        harness_contracts = next(
            item for item in harness if item["name"] == "agent-eval-contracts"
        )
        self.assertNotIn(
            contract,
            [Path(arg).resolve() for arg in harness_contracts["command"][2:]],
        )
        result = checks.execute(
            {
                **contracts,
                "scope": "web",
                "command": [*contracts["command"][:2], str(contract)],
            },
            self.root,
            10,
            threading.Event(),
        )
        self.assertEqual(
            result["status"], "passed", (self.root / result["log"]).read_text()
        )

    def test_rust_runs_after_python_install_and_only_when_it_passed(self):
        marker = self.root / "python-ready"
        ready = f"from pathlib import Path; assert Path({str(marker)!r}).exists()"
        receipt, _ = self.run_plans(
            {
                "rust": [self.command("worker", ready)],
                "python": [self.command("install-openbb", touch(marker))],
            },
            install=True,
        )
        self.assertEqual(receipt["status"], "passed")
        receipt, _ = self.run_plans(
            {
                "rust": [self.command("worker", "raise SystemExit('must not run')")],
                "python": [self.command("install-openbb", "raise SystemExit(1)")],
                "web": [self.command("independent", "pass")],
            },
            install=True,
        )
        statuses = [item["status"] for item in receipt["steps"]]
        self.assertEqual(statuses, ["not_run", "failed", "passed"])
        self.assertIn("Python dependency installation", receipt["errors"][0]["error"])

    def test_source_change_or_missing_source_evidence_cannot_qualify_a_pass(self):
        (self.source / ".gitignore").write_text("output/\n")
        source = self.source / "code.py"
        source.write_text("before")
        edit = f"from pathlib import Path; Path({str(source)!r}).write_text('after')"
        receipt, _ = self.run_plans({"web": [self.command("edit", edit)]})
        self.assertEqual(receipt["steps"][0]["status"], "passed")
        self.assertEqual(receipt["status"], "failed")
        self.assertTrue(receipt["source_changed_during_run"])
        # Ignored output does not change the fingerprint.
        before = checks.revision()["source_tree"]
        (self.source / "output").mkdir()
        (self.source / "output/ignored.log").write_text("not source")
        self.assertEqual(checks.revision()["source_tree"], before)
        receipt, _ = self.run_plans({"web": [self.command("stable", "pass")]})
        self.assertEqual(receipt["status"], "passed")
        with patch.object(checks, "APP", self.root):  # not a Git checkout
            receipt, _ = self.run_plans({"web": [self.command("success", "pass")]})
        self.assertEqual(receipt["status"], "failed")
        self.assertIn("unavailable", receipt["errors"][0]["error"])

    def test_live_app_blocks_rust_and_install_without_touching_its_owner(self):
        owner = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        self.addCleanup(owner.wait)
        self.addCleanup(owner.terminate)
        started = subprocess.check_output(
            ["ps", "-p", str(owner.pid), "-o", "lstart="],
            text=True,
            env={**os.environ, "LC_ALL": "C"},
        ).strip()
        path = self.app / "e2e/artifacts/current-session.json"
        path.parent.mkdir(parents=True)
        path.write_text(
            json.dumps({"launcherPid": owner.pid, "launcherStartedAt": started})
        )
        marker = self.root / "never-written"
        receipt, _ = self.run_plans({"rust": [self.command("unsafe", touch(marker))]})
        self.assertEqual(receipt["steps"][0]["status"], "not_run")
        self.assertIn("live native app", receipt["errors"][0]["error"])
        with self.assertRaisesRegex(RuntimeError, "live native app"):
            checks.guard_active_app("web", install=True)
        checks.guard_active_app("web", install=False)
        self.assertFalse(marker.exists())
        self.assertIsNone(owner.poll())
        # A reused PID with another start time does not block the checkout.
        path.write_text(
            json.dumps({"launcherPid": owner.pid, "launcherStartedAt": "old"})
        )
        checks.guard_active_app("rust", install=False)

    def test_timeout_and_completion_stop_every_descendant(self):
        for name, timeout, parent_sleep in (("timeout", 0.3, 30), ("launcher", 5, 0)):
            with self.subTest(name=name):
                marker = self.root / f"{name}-orphan"
                child = f"import time; time.sleep(0.8); {touch(marker)}"
                code = f"import subprocess, sys, time; subprocess.Popen([sys.executable, '-c', {child!r}]); time.sleep({parent_sleep})"
                receipt, _ = self.run_plans(
                    {"web": [self.command(name, code)]}, timeout=timeout
                )
                expected = "timed_out" if name == "timeout" else "passed"
                self.assertEqual(receipt["steps"][0]["status"], expected)
                time.sleep(1.1)
                self.assertFalse(marker.exists(), "a descendant outlived its step")

    def test_killed_runner_child_keeps_the_scope_lock(self):
        started = self.root / "child-started"
        lock = self.root / "scope.lock"
        code = f"import time; {touch(started)}; time.sleep(1.5)"
        supervisor = (
            f"import sys, threading; sys.path.insert(0, {str(Path(checks.__file__).parent)!r}); "
            "import check_runner as c; from pathlib import Path; "
            f"item = c.step('child', [sys.executable, '-c', {code!r}], Path({str(self.root)!r})); item['scope'] = 'web'; "
            f"held = c.exclusive_lock(Path({str(lock)!r})); fd = held.__enter__(); "
            f"c.execute(item, Path({str(self.root)!r}), 10, threading.Event(), (fd,))"
        )
        process = subprocess.Popen([sys.executable, "-c", supervisor])
        try:
            deadline = time.monotonic() + 5
            while not started.exists() and time.monotonic() < deadline:
                time.sleep(0.02)
            process.kill()
            process.wait(timeout=3)
            with self.assertRaises(RuntimeError):
                with checks.exclusive_lock(lock):
                    self.fail("the orphaned step must keep its owner's lock")
            time.sleep(1.7)
            with checks.exclusive_lock(lock):
                pass
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()

    def test_busy_scope_lock_refuses_without_executing(self):
        marker = self.root / "not-executed"
        with checks.exclusive_lock(self.root / "locks/web.lock"):
            receipt, _ = self.run_plans(
                {"harness": [self.command("blocked", touch(marker))]}
            )
        self.assertEqual(receipt["steps"][0]["status"], "not_run")
        self.assertIn("busy", receipt["errors"][0]["error"])
        self.assertFalse(marker.exists())
        receipt, _ = self.run_plans({"web": [self.command("available", "pass")]})
        self.assertEqual(receipt["status"], "passed")

    def test_zombie_only_group_preserves_the_failed_command_exit(self):
        # The zombie stays in the step's group while its parent moves to another
        # session before reaping it; Darwin killpg then reports EPERM.
        code = """import os, time
child = os.fork()
if child == 0:
    zombie = os.fork()
    if zombie == 0:
        os._exit(0)
    os.setsid()
    time.sleep(1)
    os.waitpid(zombie, 0)
    os._exit(0)
time.sleep(0.3)
raise SystemExit(7)
"""
        receipt, _ = self.run_plans({"web": [self.command("zombie", code)]})
        self.assertEqual(receipt["steps"][0]["exit_code"], 7)
        self.assertEqual(receipt["steps"][0]["status"], "failed")
        self.assertNotIn("cleanup_error", receipt["steps"][0])
        time.sleep(1)

    def test_cleanup_failure_and_cancellation_keep_the_real_outcome(self):
        denied = PermissionError("live group denied")
        with patch.object(checks, "stop_process", side_effect=denied):
            item = self.item("cleanup", "raise SystemExit(7)")
            result = checks.execute(item, self.root, 5, threading.Event())
        self.assertEqual((result["exit_code"], result["status"]), (7, "failed"))
        self.assertEqual(result["command_status"], "failed")
        self.assertIn("Traceback", (self.root / result["log"]).read_text())
        cancelled = threading.Event()
        timer = threading.Timer(0.2, cancelled.set)
        timer.start()
        self.addCleanup(timer.cancel)
        item = self.item("cancel", "import time; time.sleep(30)")
        result = checks.execute(item, self.root, 5, cancelled)
        self.assertEqual(result["status"], "cancelled")
        self.assertLess(result["duration_seconds"], 5)

    def test_temporary_path_leaves_room_for_unix_sockets(self):
        code = "import socket, tempfile; from pathlib import Path; s = socket.socket(socket.AF_UNIX); s.bind(str(Path(tempfile.mkdtemp(prefix='loupist-broker-protocol-')) / 'broker.sock')); s.close()"
        receipt, _ = self.run_plans({"web": [self.command("socket", code)]})
        self.assertEqual(receipt["status"], "passed")

    def test_retention_removes_only_expired_run_directories(self):
        output = self.root / "checks"
        old, recent = output / "20200101T000000Z-aaaaaaaa", output / "recent"
        for path in (old, recent, output / "20200101T000000Z-bbbbbbbb.txt"):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.mkdir() if path.suffix != ".txt" else path.write_text("keep")
        now = time.time()
        fresh = output / time.strftime("%Y%m%dT%H%M%SZ-cccccccc", time.gmtime(now))
        fresh.mkdir()
        checks.prune_old_runs(output, now)
        self.assertEqual(
            sorted(p.name for p in output.iterdir()),
            sorted([fresh.name, "recent", "20200101T000000Z-bbbbbbbb.txt"]),
        )


if __name__ == "__main__":
    unittest.main()
