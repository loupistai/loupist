#!/usr/bin/env python3
"""Exercise scope selection, real Git diffs and release validation boundaries."""

from __future__ import annotations

import os
import re
import runpy
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
SCOPE = runpy.run_path(str(SCRIPTS / "ci_change_scope.py"))
classify = SCOPE["classify"]
git_changed_files = SCOPE["git_changed_files"]
ALL = set(SCOPE["SCOPES"])


def selected(paths, **kwargs):
    return {scope for scope, enabled in classify(paths, **kwargs).items() if enabled}


class ChangeScopeTests(unittest.TestCase):
    def test_change_scope_matrix(self) -> None:
        cases = (
            (["README.md", "docs/ci-cd.md", "CHANGELOG.md"], set()),
            (["apps/loupist/e2e/README.md"], set()),
            (["apps/loupist/src/App.tsx"], {"web", "native"}),
            (["apps/loupist/src/styles.css"], {"web", "native"}),
            (["apps/loupist/vite.config.ts"], {"web", "native"}),
            (["apps/loupist/src/App.test.tsx"], {"web"}),
            (["apps/loupist/src/test/setup.ts"], {"web"}),
            (["apps/loupist/agent/pi-settings.test.mjs"], {"web"}),
            (["apps/loupist/e2e/agent-driver.test.mjs"], {"web"}),
            (["apps/loupist/src-tauri/src/store/tests.rs"], {"rust"}),
            (["apps/loupist/src-tauri/src/commands/tests/tools.rs"], {"rust"}),
            (["apps/loupist/src-tauri/tests/debug_credentials.rs"], {"rust"}),
            (["apps/loupist/src-tauri/src/eval_harness/replay_scenarios.rs"], {"rust"}),
            (["apps/loupist/openbb/tests/test_server.py"], {"python"}),
            (["apps/loupist/scripts/test_ci_change_scope.py"], {"python"}),
            (["apps/loupist/src-tauri/src/lib.rs"], {"rust", "native", "packaging"}),
            (
                ["src/knowledge/embed_assets/model.onnx"],
                {"rust", "native", "packaging"},
            ),
            (
                ["apps/loupist/openbb/src/loupist_openbb/server.py"],
                {"python", "rust", "native", "packaging"},
            ),
            (
                ["apps/loupist/src/contracts/assetRequests.example.json"],
                {"web", "rust", "native"},
            ),
            (["apps/loupist/agent/SYSTEM.md"], {"web", "rust", "native", "packaging"}),
            (
                ["apps/loupist/agent/skills/research/SKILL.md"],
                {"web", "rust", "native", "packaging"},
            ),
            (
                ["apps/loupist/marketplace/catalog.json"],
                {"web", "rust", "native", "packaging"},
            ),
            (
                ["apps/loupist/evals/agent/suites/replay/opendart-hanbit.json"],
                {"web", "rust"},
            ),
            (["apps/loupist/evals/sharing/fixture/README.md"], {"web", "rust"}),
            (["apps/loupist/e2e/native-smoke.mjs"], {"web", "python", "native"}),
            (
                ["apps/loupist/src-tauri/tauri.conf.json"],
                {"python", "rust", "native", "packaging"},
            ),
            (["apps/loupist/THIRD_PARTY_NOTICES.md"], {"python", "packaging"}),
            ([".nvmrc"], ALL),
            (["apps/loupist/package.json"], ALL),
            (["apps/loupist/package-lock.json"], ALL),
            (["apps/loupist/scripts/stage-macos-arm64.sh"], ALL),
            (["apps/loupist/scripts/stage_workspace_tools.py"], ALL),
            ([".github/workflows/ci.yml"], ALL),
            ([".github/workflows/release.yml"], ALL),
            ([".github/actions/setup-toolchains/action.yml"], ALL),
            (["apps/loupist/scripts/check_runner.py"], ALL),
            (["new-runtime/unknown.py"], ALL),
            (["apps/loupist/new-config.json"], ALL),
            ([], set()),
        )
        for paths, expected in cases:
            with self.subTest(paths=paths):
                self.assertEqual(selected(paths), expected)

    def test_union_normalization_and_explicit_full(self) -> None:
        self.assertEqual(
            selected(
                [
                    "./apps/loupist/src/App.test.tsx",
                    "apps\\loupist\\openbb\\tests\\test_server.py",
                ]
            ),
            {"web", "python"},
        )
        self.assertEqual(selected(["docs/ci-cd.md"], force_full=True), ALL)

    def test_rename_out_of_native_surface_keeps_deleted_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            self._git(repo, "init", "-b", "main")
            source = repo / "apps/loupist/src/retired.ts"
            source.parent.mkdir(parents=True)
            source.write_text("export const retired = true;\n")
            base = self.commit(repo)
            source.rename(repo / "retired.md")
            self.commit(repo)
            for merge_base in (True, False):
                paths = git_changed_files(
                    base, "HEAD", root=repo, merge_base=merge_base
                )
                self.assertIn("apps/loupist/src/retired.ts", paths)
                self.assertEqual(selected(paths), {"web", "native"})

    def test_push_includes_all_commits_not_only_last_commit(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            self._git(repo, "init", "-b", "main")
            (repo / "README.md").write_text("base")
            base = self.commit(repo)
            source = repo / "apps/loupist/src/file with\nnewline.ts"
            source.parent.mkdir(parents=True)
            source.write_text("export const changed = true;")
            self.commit(repo)
            (repo / "README.md").write_text("docs after product edit")
            self.commit(repo)
            paths = git_changed_files(base, "HEAD", root=repo, merge_base=False)
            self.assertIn("apps/loupist/src/file with\nnewline.ts", paths)
            self.assertEqual(selected(paths), {"web", "native"})

    def test_pr_ignores_base_branch_changes_but_direct_diff_does_not(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            self._git(repo, "init", "-b", "main")
            (repo / "README.md").write_text("base")
            self.commit(repo)
            self._git(repo, "checkout", "-b", "feature")
            (repo / "README.md").write_text("branch docs")
            head = self.commit(repo)
            self._git(repo, "checkout", "main")
            source = repo / "apps/loupist/src-tauri/src/lib.rs"
            source.parent.mkdir(parents=True)
            source.write_text("fn main() {}")
            base = self.commit(repo)
            self.assertEqual(selected(git_changed_files(base, head, root=repo)), set())
            self.assertEqual(
                selected(git_changed_files(base, head, root=repo, merge_base=False)),
                {"rust", "native", "packaging"},
            )

    def test_missing_or_unavailable_history_fails_to_full(self) -> None:
        for base in ("", "0" * 40, "unavailable-commit"):
            with self.subTest(base=base), tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / "outputs"
                result = subprocess.run(
                    [sys.executable, str(SCRIPTS / "ci_change_scope.py")],
                    env={
                        **os.environ,
                        "CI_FORCE_FULL": "0",
                        "CI_BASE_SHA": base,
                        "CI_HEAD_SHA": "HEAD",
                        "GITHUB_OUTPUT": str(output),
                    },
                    capture_output=True,
                    text=True,
                    check=True,
                )
                self.assertIn("running the full CI set", result.stderr)
                self.assertEqual(
                    dict(line.split("=") for line in output.read_text().splitlines()),
                    {scope: "true" for scope in ALL},
                )

    def test_release_cannot_sign_after_only_scoped_ci(self) -> None:
        # Security boundary: every signed build depends on full, same-commit CI,
        # which receives no signing or model-provider secrets from the caller.
        root = SCRIPTS.parents[2]
        release = (root / ".github/workflows/release.yml").read_text()
        job = release.split("\n  full-ci:\n", 1)[1].split("\n  build-macos:", 1)[0]
        self.assertIn("uses: ./.github/workflows/ci.yml", job)
        self.assertIn("full: true", job)
        self.assertNotIn("secrets:", job)
        build = release.split("\n  build-macos:\n", 1)[1].split("\n  publish:", 1)[0]
        self.assertIn("needs: [validate, full-ci]", build)
        ci = (root / ".github/workflows/ci.yml").read_text()
        self.assertIn("  workflow_call:", ci)
        self.assertIn("inputs.full || github.event_name == 'workflow_dispatch'", ci)
        self.assertNotIn("secrets.", ci)

    def test_selected_failure_or_unexpected_skip_cannot_pass_source_gate(self) -> None:
        # Execute the workflow's gate with simulated GitHub job results. Checking
        # only job-level `if` strings would miss a gate that accepts all skips.
        workflow = (SCRIPTS.parents[2] / ".github/workflows/ci.yml").read_text()
        gate = workflow.split("      - name: Require every selected check\n", 1)[1]
        script = textwrap.dedent(gate.split("        run: |\n", 1)[1])
        scopes = ("WEB", "PYTHON", "RUST", "RUST_MACOS", "NATIVE", "PACKAGE")
        for required, result, success in (
            ("false", "skipped", True),
            ("true", "success", True),
            ("true", "skipped", False),
            ("true", "failure", False),
            ("true", "cancelled", False),
            ("true", "abandoned", False),
            ("false", "failure", False),
            ("false", "success", False),
            ("", "skipped", False),
        ):
            for scope in scopes:
                with self.subTest(scope=scope, required=required, result=result):
                    environment = dict(os.environ)
                    for name in scopes:
                        environment[f"{name}_REQUIRED"] = "false"
                        environment[f"{name}_RESULT"] = "skipped"
                    environment[f"{scope}_REQUIRED"] = required
                    environment[f"{scope}_RESULT"] = result
                    completed = subprocess.run(
                        [
                            "bash",
                            "-c",
                            script.replace("${{ needs.changes.result }}", "success"),
                        ],
                        env=environment,
                        capture_output=True,
                        text=True,
                    )
                    self.assertEqual(
                        completed.returncode == 0, success, completed.stderr
                    )
        for name in (
            "verify-web",
            "verify-python",
            "verify-rust",
            "verify-rust-macos",
            "native-e2e",
            "package-smoke",
        ):
            self.assertIn(f"      - {name}\n", self.job_block(workflow, "verify"))

    def test_pull_requests_never_wait_for_macos_runners(self) -> None:
        workflow = (SCRIPTS.parents[2] / ".github/workflows/ci.yml").read_text()
        self.assertIn("macos=${{ github.event_name != 'pull_request' }}", workflow)
        macos_jobs = [
            job
            for job in re.findall(r"\n  ([A-Za-z0-9_-]+):\n", workflow)
            if "runs-on: macos" in self.job_block(workflow, job)
        ]
        self.assertEqual(
            sorted(macos_jobs), ["native-e2e", "package-smoke", "verify-rust-macos"]
        )
        for job in macos_jobs:
            condition = (
                self.job_block(workflow, job)
                .split("\n    if: ", 1)[1]
                .split("\n", 1)[0]
            )
            self.assertIn("needs.changes.outputs.macos == 'true'", condition, job)

    def test_web_legs_split_the_renderer_tests_and_keep_every_web_step(self) -> None:
        workflow = (SCRIPTS.parents[2] / ".github/workflows/ci.yml").read_text()
        block = self.job_block(workflow, "verify-web")
        legs = re.findall(r"- shard: (\S+)\n\s+only: (\S*)\n", block)
        self.assertEqual(sorted(shard for shard, _ in legs), ["1/2", "2/2"])
        runner = runpy.run_path(str(SCRIPTS / "check_runner.py"))
        planned = {item["name"] for item in runner["web_plan"](False, True)}
        self.assertIn('""', [only for _, only in legs])
        for _, only in legs:
            if only != '""':
                self.assertIn("renderer-tests", only.split(","))
                self.assertLessEqual(set(only.split(",")), planned)

    def test_linux_rust_legs_cover_the_whole_rust_plan_once(self) -> None:
        workflow = (SCRIPTS.parents[2] / ".github/workflows/ci.yml").read_text()
        legs = re.findall(
            r"\n            steps: (\S+)\n", self.job_block(workflow, "verify-rust")
        )
        names = [name for leg in legs for name in leg.split(",")]
        runner = runpy.run_path(str(SCRIPTS / "check_runner.py"))
        planned = [item["name"] for item in runner["rust_plan"]()]
        self.assertEqual(sorted(names), sorted(planned))

    def job_block(self, workflow: str, job_id: str) -> str:
        match = re.search(
            rf"\n  {re.escape(job_id)}:\n(.*?)(?=\n  [A-Za-z0-9_-]+:\n|\Z)",
            workflow,
            re.S,
        )
        self.assertIsNotNone(match, job_id)
        return match.group(1)

    def commit(self, repo):
        self._git(repo, "add", "-A")
        self._git(repo, "commit", "-m", "fixture")
        return self._git(repo, "rev-parse", "HEAD").strip()

    def _git(self, repo: Path, *args: str) -> str:
        return subprocess.run(
            [
                "git",
                "-c",
                "user.email=ci-scope@example.com",
                "-c",
                "user.name=CI Scope",
                *args,
            ],
            cwd=repo,
            check=True,
            capture_output=True,
            text=True,
        ).stdout


if __name__ == "__main__":
    unittest.main()
