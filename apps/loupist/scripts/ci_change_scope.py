#!/usr/bin/env python3
"""Select CI jobs by affected surface; unknown paths and failed diffs run all jobs.

PRs compare against their merge base. Main pushes compare before/after directly,
including every commit in the push. Manual and release checks force every scope.
Repository guidance and security invariants run even when every scope is false.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from collections.abc import Iterable
from pathlib import Path, PurePosixPath

SCOPES = ("web", "python", "rust", "native", "packaging")
APP = "apps/loupist/"
FULL_PATHS = {
    ".nvmrc",
    ".gitattributes",
    ".gitignore",
    "scripts/verify.sh",
    f"{APP}package.json",
    f"{APP}package-lock.json",
    f"{APP}scripts/ci_change_scope.py",
    f"{APP}scripts/check_runner.py",
    f"{APP}scripts/check.sh",
    f"{APP}scripts/setup.sh",
    f"{APP}scripts/verify.sh",
    f"{APP}scripts/tauri.mjs",
}
WORKFLOWS_OUTSIDE_CI = {
    ".github/workflows/windows-compatibility.yml",
}


def normalize(path: str) -> str:
    normalized = path.replace("\\", "/")
    while normalized.startswith("./"):
        normalized = normalized[2:]
    return normalized


def path_scopes(path: str) -> set[str]:
    """Keep exceptions narrow: Markdown Skills and embedded fixtures are code."""
    if path in WORKFLOWS_OUTSIDE_CI:
        return set()
    if path in FULL_PATHS or path.startswith(
        (".github/workflows/", ".github/actions/")
    ):
        return set(SCOPES)
    relative = path.removeprefix(APP)
    name = PurePosixPath(path).name

    if path.startswith(APP):
        # Inline tests in production Rust files still take the native path:
        # filenames cannot separate them from shipped code.
        if relative.startswith(("src/", "agent/", "e2e/", "scripts/")) and (
            name.endswith((".test.ts", ".test.tsx", ".test.mjs"))
            or relative.startswith("src/test/")
        ):
            return {"web"}
        if relative.startswith("src-tauri/") and (
            relative.startswith(("src-tauri/tests/", "src-tauri/src/eval_harness/"))
            or "/tests/" in relative
            or name == "tests.rs"
            or name.endswith("_tests.rs")
        ):
            return {"rust"}
        if relative.startswith("openbb/tests/") or (
            relative.startswith(("scripts/", "e2e/"))
            and name.startswith("test_")
            and name.endswith(".py")
        ):
            return {"python"}

        # These contain embedded Markdown/JSON and shared contracts.
        if relative.startswith(("agent/", "marketplace/")):
            return {"web", "rust", "native", "packaging"}
        if relative.startswith("evals/"):
            return {"web", "rust"}
        if relative.startswith("src/contracts/"):
            return {"web", "rust", "native"}
        if relative.startswith("src-tauri/"):
            scopes = {"rust", "native", "packaging"}
            if relative in {
                "src-tauri/Cargo.toml",
                "src-tauri/Info.plist",
            } or name.startswith("tauri."):
                scopes.add("python")  # Version/bundle metadata contracts.
            return scopes

    # Documentation exemptions come after embedded resources and test fixtures.
    if (
        path.startswith("docs/")
        or name.endswith(".md")
        and (
            "/" not in path
            or path
            in {f"{APP}README.md", f"{APP}e2e/README.md", f"{APP}openbb/README.md"}
        )
    ):
        return set()
    if path.startswith(".github/ISSUE_TEMPLATE/") or path in {
        ".github/CODEOWNERS",
        ".github/dependabot.yml",
        ".github/pull_request_template.md",
        "scripts/guide.py",
    }:
        return set()
    if path in {"LICENSE", "NOTICE", f"{APP}THIRD_PARTY_NOTICES.md"}:
        return {"python", "packaging"}
    if path.startswith("src/"):
        return {"rust", "native", "packaging"}
    if path.startswith(APP):
        if relative.startswith("openbb/"):
            return {"python", "rust", "native", "packaging"}
        if relative.startswith(("src/", "public/")) or relative in {
            "index.html",
            "preview.html",
            "vite.config.ts",
            "components.json",
            "tsconfig.json",
            "tsconfig.app.json",
            "tsconfig.node.json",
        }:
            return {"web", "native"}
        if relative.startswith("e2e/"):
            return {"web", "python", "native"}
        if relative.startswith("scripts/"):
            if name.startswith(
                ("stage-", "stage_", "check-macos-", "check-windows-", "check-package-")
            ) or name in {"check-sidecars.py", "check_embed_assets.py"}:
                return set(SCOPES)
            if name.endswith(".py"):
                return {"python"}
            if name.endswith((".mjs", ".json")):
                return {"web"}
    # A new surface must be classified explicitly before it can skip checks.
    return set(SCOPES)


def classify(paths: Iterable[str], *, force_full: bool = False) -> dict[str, bool]:
    selected = set(SCOPES) if force_full else set()
    for path in paths:
        if path:
            selected.update(path_scopes(normalize(path)))
    return {scope: scope in selected for scope in SCOPES}


def git_changed_files(
    base_sha: str, head_sha: str, *, merge_base: bool = True, root: Path | None = None
) -> list[str]:
    completed = subprocess.run(
        [
            "git",
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "--diff-filter=ACDMRTUXB",
            f"{base_sha}{'...' if merge_base else '..'}{head_sha}",
            "--",
        ],
        check=False,
        capture_output=True,
        text=True,
        cwd=root,
    )
    if completed.returncode != 0:
        raise RuntimeError(completed.stderr.strip() or "git diff failed")
    return [path for path in completed.stdout.split("\0") if path]


def write_github_output(scopes: dict[str, bool]) -> None:
    payload = "".join(
        f"{scope}={str(enabled).lower()}\n" for scope, enabled in scopes.items()
    )
    print(payload, end="")
    if output_path := os.environ.get("GITHUB_OUTPUT"):
        with Path(output_path).open("a", encoding="utf-8") as handle:
            handle.write(payload)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--force-full", action="store_true")
    parser.add_argument("--base-sha")
    parser.add_argument("--head-sha")
    parser.add_argument(
        "--diff-mode",
        choices=("merge-base", "direct"),
        default=os.environ.get("CI_DIFF_MODE", "merge-base"),
    )
    arguments = parser.parse_args()
    force_full = arguments.force_full or os.environ.get("CI_FORCE_FULL") == "1"
    try:
        paths: list[str] = []
        if not force_full:
            base_sha = arguments.base_sha or os.environ.get("CI_BASE_SHA", "")
            head_sha = arguments.head_sha or os.environ.get("CI_HEAD_SHA", "")
            if not base_sha or not head_sha:
                raise RuntimeError("classification needs base and head SHAs")
            paths = git_changed_files(
                base_sha, head_sha, merge_base=arguments.diff_mode == "merge-base"
            )
        scopes = classify(paths, force_full=force_full)
    except Exception as error:
        print(
            f"change-scope classification failed ({error}); running the full CI set",
            file=sys.stderr,
        )
        scopes = classify([], force_full=True)
    write_github_output(scopes)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
