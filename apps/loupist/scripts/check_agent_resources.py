#!/usr/bin/env python3
"""Check the staged agent tree against production sources, excluding test modules."""

from __future__ import annotations

import argparse
from pathlib import Path


def production_files(root: Path) -> dict[str, bytes]:
    files = {}
    if not root.is_dir() or root.is_symlink():
        raise ValueError("agent resource directory is missing or linked")
    for path in root.rglob("*"):
        if path.is_symlink():
            raise ValueError("agent resources contain a symbolic link")
        if path.is_file() and not path.name.endswith(".test.mjs"):
            files[path.relative_to(root).as_posix()] = path.read_bytes()
    return files


def check(source: Path, packaged: Path) -> None:
    expected = production_files(source)
    actual = production_files(packaged)
    if any(path.name.endswith(".test.mjs") for path in packaged.rglob("*")):
        raise ValueError("packaged agent contains test modules")
    if expected != actual:
        raise ValueError("packaged agent differs from production sources")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--packaged", required=True, type=Path)
    arguments = parser.parse_args()
    try:
        check(arguments.source, arguments.packaged)
    except ValueError as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
