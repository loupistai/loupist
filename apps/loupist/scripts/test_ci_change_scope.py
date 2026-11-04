#!/usr/bin/env python3
"""Unit tests for CI change-scope classification."""

from __future__ import annotations

import runpy
import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPTS = Path(__file__).resolve().parent
SCOPE = runpy.run_path(str(SCRIPTS / "ci_change_scope.py"))
classify = SCOPE["classify"]
git_changed_files = SCOPE["git_changed_files"]


class ChangeScopeTests(unittest.TestCase):
    def test_change_scope_matrix(self) -> None:
        cases = (
            (["apps/loupist/marketplace/catalog.json"], (True, True)),
            ([".nvmrc"], (True, True)),
            (["apps/loupist/package.json"], (True, True)),
            (["apps/loupist/package-lock.json"], (True, True)),
            (["apps/loupist/src/App.tsx"], (True, False)),
            (["apps/loupist/vite.config.ts"], (True, False)),
            (["docs/ci-cd.md", "README.md", "CHANGELOG.md"], (False, False)),
            (["apps/loupist/src-tauri/src/lib.rs"], (True, True)),
            (["src/knowledge/embed_assets/model.onnx"], (True, True)),
            (
                ["apps/loupist/scripts/stage-embed-assets.py"],
                (True, True),
            ),
            ([".github/workflows/ci.yml", "docs/ci-cd.md"], (True, True)),
            ([".github/actions/setup-toolchains/action.yml"], (True, True)),
            (["apps/loupist/scripts/ci_change_scope.py"], (True, True)),
            (["apps/loupist/src/main.tsx", "docs/architecture.md"], (True, False)),
            (["apps/loupist/scripts/stage-macos-arm64.sh"], (True, True)),
            (["apps/loupist/scripts/check_embed_assets.py"], (False, True)),
            (["apps/loupist/agent/SYSTEM.md"], (True, True)),
            ([], (False, False)),
        )
        for paths, expected in cases:
            with self.subTest(paths=paths):
                self.assertEqual(classify(paths), expected)

    def test_force_full_overrides_docs_only(self) -> None:
        self.assertEqual(classify(["docs/ci-cd.md"], force_full=True), (True, True))

    def test_deleted_native_path_is_included_in_the_diff_scope(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            self._git(repo, "init", "-b", "main")
            deleted = repo / "apps" / "loupist" / "src" / "removed.tsx"
            deleted.parent.mkdir(parents=True)
            deleted.write_text("export const removed = true;\n", encoding="utf-8")
            self._git(repo, "add", ".")
            self._git(repo, "commit", "-m", "add native file")
            base = self._git(repo, "rev-parse", "HEAD").strip()
            deleted.unlink()
            self._git(repo, "add", "-A")
            self._git(repo, "commit", "-m", "delete native file")
            head = self._git(repo, "rev-parse", "HEAD").strip()
            changed = git_changed_files(base, head, root=repo)
            self.assertIn("apps/loupist/src/removed.tsx", changed)
            native, packaging = classify(changed)
            self.assertTrue(native)
            self.assertFalse(packaging)

    def test_rename_out_of_native_surface_keeps_deleted_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            self._git(repo, "init", "-b", "main")
            source = repo / "apps/loupist/src/retired.ts"
            source.parent.mkdir(parents=True)
            source.write_text("export const retired = true;\n")
            self._git(repo, "add", ".")
            self._git(repo, "commit", "-m", "native source")
            base = self._git(repo, "rev-parse", "HEAD").strip()
            source.rename(repo / "retired.md")
            self._git(repo, "add", "-A")
            self._git(repo, "commit", "-m", "move to documentation")
            paths = git_changed_files(base, "HEAD", root=repo)
            self.assertIn("apps/loupist/src/retired.ts", paths)
            self.assertEqual(classify(paths), (True, False))

    def _git(self, repo: Path, *args: str) -> str:
        completed = subprocess.run(
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
        )
        return completed.stdout


if __name__ == "__main__":
    unittest.main()
