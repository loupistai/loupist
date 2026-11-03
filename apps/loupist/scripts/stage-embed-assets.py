#!/usr/bin/env python3
"""Stage the local World embedding model without a separate runtime."""

from pathlib import Path
import shutil

APP = Path(__file__).resolve().parent.parent
SOURCE = APP.parent.parent / "src/knowledge/embed_assets"
DESTINATION = APP / "src-tauri/resources/embed_assets"
FILES = ("config.json", "model.safetensors", "tokenizer.json", "NOTICE")


def main():
    for name in FILES:
        if not (SOURCE / name).is_file():
            raise SystemExit(f"Missing embedding asset: {SOURCE / name}")
    if DESTINATION.exists():
        shutil.rmtree(DESTINATION)
    DESTINATION.mkdir(parents=True)
    for name in FILES:
        shutil.copyfile(SOURCE / name, DESTINATION / name)
    print("Staged local World embedding assets.")


if __name__ == "__main__":
    main()
