#!/usr/bin/env python3
"""Verify staged local embedding assets against their pinned digests."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

FILES = frozenset(("config.json", "model.safetensors", "tokenizer.json", "NOTICE"))
# The repository keeps the notice and the pins; the model files are staged.
SOURCE_FILES = frozenset(("NOTICE", "pin.json"))
MODEL_FILES = FILES - {"NOTICE"}


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def ordinary_files(root: Path, inventory: frozenset[str]) -> None:
    if root.is_symlink() or not root.is_dir():
        raise ValueError("Embedding assets must be an ordinary directory")
    if {entry.name for entry in root.iterdir()} != inventory:
        raise ValueError(
            f"Embedding asset inventory differs from the {len(inventory)} required files"
        )
    for name in inventory:
        path = root / name
        if path.is_symlink() or not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"Embedding asset must be a nonempty regular file: {name}")


def load_pin(source: Path) -> dict:
    ordinary_files(source, SOURCE_FILES)
    pin = json.loads((source / "pin.json").read_text())
    for key in ("upstream", "staged"):
        if not isinstance(pin.get(key), dict) or set(pin[key]) != MODEL_FILES:
            raise ValueError(f"Embedding pin must list the three model files: {key}")
    if not pin.get("model") or not pin.get("revision"):
        raise ValueError("Embedding pin must name the model and its revision")
    return pin


def verify(source: Path, packaged: Path) -> None:
    pin = load_pin(source)
    ordinary_files(packaged, FILES)
    if (source / "NOTICE").read_bytes() != (packaged / "NOTICE").read_bytes():
        raise ValueError("Packaged embedding asset differs from source: NOTICE")
    for name in sorted(MODEL_FILES):
        if digest(packaged / name) != pin["staged"][name]:
            raise ValueError(f"Packaged embedding asset differs from its pin: {name}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--packaged", type=Path, required=True)
    args = parser.parse_args()
    try:
        verify(args.source, args.packaged)
    except ValueError as error:
        raise SystemExit(str(error)) from error
    print("Packaged embedding assets match the pinned inventory and digests.")


if __name__ == "__main__":
    main()
