#!/usr/bin/env python3
"""Fingerprint and verify unsigned CI runtimes; never used by signed releases."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import stat
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
RESOURCES = "apps/loupist/src-tauri/resources"
MANIFEST = ".ci-runtime-cache.json"
SOURCE_PREFIXES = (
    ".github/actions/setup-toolchains/",
    ".github/actions/stage-ci-runtimes/",
    "apps/loupist/scripts/",
    "apps/loupist/openbb/src/",
    "apps/loupist/python-worker/",
    "src/knowledge/embed_assets/",
)
SOURCE_FILES = {
    ".nvmrc",
    "LICENSE",
    "NOTICE",
    "apps/loupist/package.json",
    "apps/loupist/package-lock.json",
    "apps/loupist/THIRD_PARTY_NOTICES.md",
    "apps/loupist/src-tauri/PiEntitlements.plist",
    "apps/loupist/openbb/pyproject.toml",
    "apps/loupist/openbb/uv.lock",
    "apps/loupist/openbb/runtime-manifest.json",
    "apps/loupist/openbb/build_sidecar.py",
    "apps/loupist/openbb/materialize_bundle_symlinks.py",
}


def digest(path: Path) -> str:
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def fingerprint(root: Path, paths: list[str], tools: dict[str, str]) -> str:
    source = {}
    for name in sorted(paths):
        path = root / name
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"runtime source is not a regular file: {name}")
        source[name] = digest(path)
    return hashlib.sha256(
        json.dumps({"source": source, "tools": tools}, sort_keys=True).encode()
    ).hexdigest()


def source_key(root: Path) -> str:
    paths = (
        subprocess.check_output(
            ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
            cwd=root,
        )
        .decode()
        .split("\0")
    )
    selected = [
        path
        for path in set(paths)
        if path in SOURCE_FILES or path.startswith(SOURCE_PREFIXES)
    ]
    if not selected:
        raise ValueError("no runtime source files found")
    tools = {
        "host": f"{platform.system()}-{platform.machine()}-{platform.mac_ver()[0]}",
        "python": platform.python_version(),
    }
    for name in ("node", "rustc", "uv"):
        tools[name] = subprocess.check_output([name, "--version"], text=True).strip()
    return fingerprint(root, selected, tools)


def inventory(root: Path) -> dict[str, dict[str, str | int]]:
    resources = root / RESOURCES
    names = ["pi-runtime", "embed_assets"]
    if (root / "apps/loupist/python-worker/runtime.json").exists():
        names.append("python-runtime")
    files = {}
    for name in names:
        directory = resources / name
        if directory.is_symlink() or not directory.is_dir():
            raise ValueError(f"missing or linked runtime: {name}")
        for path in sorted(directory.rglob("*")):
            metadata = path.lstat()
            if stat.S_ISDIR(metadata.st_mode):
                continue
            relative = path.relative_to(resources).as_posix()
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
                raise ValueError(f"runtime contains a link or special file: {relative}")
            files[relative] = {
                "sha256": digest(path),
                "mode": stat.S_IMODE(metadata.st_mode),
            }
    if not files:
        raise ValueError("runtime cache is empty")
    return files


def record(root: Path, key: str) -> None:
    value = {"schema": 1, "key": key, "files": inventory(root)}
    # Replace the directory entry rather than following a restored manifest link.
    temporary = root / RESOURCES / f"{MANIFEST}.tmp"
    with temporary.open("x") as stream:
        stream.write(json.dumps(value, sort_keys=True) + "\n")
    temporary.replace(root / RESOURCES / MANIFEST)


def verify(root: Path, key: str) -> None:
    manifest = root / RESOURCES / MANIFEST
    if manifest.is_symlink():
        raise ValueError("linked cache manifest")
    value = json.loads(manifest.read_text())
    if value.get("schema") != 1 or value.get("key") != key:
        raise ValueError("runtime cache source or toolchain changed")
    if value.get("files") != inventory(root):
        raise ValueError("runtime cache files or modes changed")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("key", "record", "verify"))
    parser.add_argument("--key")
    args = parser.parse_args()
    if args.action == "key":
        print(source_key(ROOT))
    elif not args.key:
        parser.error("--key is required for record/verify")
    else:
        {"record": record, "verify": verify}[args.action](ROOT, args.key)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(f"CI runtime cache: {error}")
