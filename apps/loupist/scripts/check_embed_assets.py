#!/usr/bin/env python3
"""Verify the packaged local embedding model against its source assets."""

from __future__ import annotations

import argparse
from pathlib import Path

FILES = frozenset(("config.json", "model.safetensors", "tokenizer.json", "NOTICE"))


def verify(source: Path, packaged: Path) -> None:
    for root in (source, packaged):
        if root.is_symlink() or not root.is_dir():
            raise ValueError("Embedding assets must be an ordinary directory")
        if {entry.name for entry in root.iterdir()} != FILES:
            raise ValueError(
                "Embedding asset inventory differs from the four required files"
            )
        for name in FILES:
            path = root / name
            if path.is_symlink() or not path.is_file() or path.stat().st_size == 0:
                raise ValueError(
                    f"Embedding asset must be a nonempty regular file: {name}"
                )
    for name in FILES:
        if (source / name).read_bytes() != (packaged / name).read_bytes():
            raise ValueError(f"Packaged embedding asset differs from source: {name}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--packaged", type=Path, required=True)
    args = parser.parse_args()
    try:
        verify(args.source, args.packaged)
    except ValueError as error:
        raise SystemExit(str(error)) from error
    print("Packaged embedding assets match the source inventory and bytes.")


if __name__ == "__main__":
    main()
