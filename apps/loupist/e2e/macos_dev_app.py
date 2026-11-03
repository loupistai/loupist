#!/usr/bin/env python3
"""Run a debug binary inside an identifiable macOS app, without detaching it.

Cargo's target runner calls this after compilation, on every Tauri dev restart.
The app keeps its compiled profile, dev server, resources and native delegates.
No installed application or user data is modified.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import stat
import sys
import tempfile


def real_directory(path: Path) -> None:
    path.mkdir(mode=0o700, exist_ok=True)
    metadata = path.lstat()
    if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid():
        raise ValueError(
            f"Development bundle directory is not a real owned directory: {path}"
        )


def prepare_bundle(binary: Path, manifest: Path, override: str = "{}") -> Path:
    binary = binary.absolute()
    if binary.name != "loupist" or binary.parent.name != "debug":
        raise ValueError(
            "The macOS development runner accepts only a debug/loupist binary"
        )
    if binary.is_symlink() or not binary.is_file():
        raise ValueError("The development executable must be a real file")
    config = json.loads((manifest / "tauri.conf.json").read_text())
    config.update(json.loads(override))
    identifier = config["identifier"]
    if not re.fullmatch(r"com\.monarchjuno\.loupist(?:\.[a-z0-9-]+)*", identifier):
        raise ValueError("Unexpected development app identifier")
    # The normal debug profile still uses .loupist/development in Rust. Its OS
    # identity must not resolve to the separately installed release application.
    bundle_id = (
        identifier
        if identifier != "com.monarchjuno.loupist"
        else f"{identifier}.development"
    )
    parent = binary.parent / "loupist-dev-apps"
    real_directory(parent)
    profile = parent / bundle_id
    real_directory(profile)
    bundle = profile / "Loupist Dev.app"
    marker = bundle / ".loupist-dev-bundle.json"
    ownership = {"schema": 1, "binary": str(binary), "identifier": identifier}
    if bundle.exists() or bundle.is_symlink():
        if bundle.is_symlink() or marker.is_symlink() or not marker.is_file():
            raise ValueError("Refusing to replace an unowned development app bundle")
        if json.loads(marker.read_text()) != ownership:
            raise ValueError("Development app bundle belongs to another build")
    else:
        bundle.mkdir(mode=0o700)
        marker.write_text(json.dumps(ownership) + "\n")
    real_directory(bundle / "Contents")
    real_directory(bundle / "Contents/MacOS")
    destination = bundle / "Contents/MacOS/loupist"
    # Replace atomically. A previous process can retain its executable inode
    # until Cargo's watcher stops it; never truncate an executable in place.
    fd, temporary = tempfile.mkstemp(prefix=".loupist-", dir=destination.parent)
    os.close(fd)
    try:
        shutil.copy2(binary, temporary)
        os.replace(temporary, destination)
    finally:
        Path(temporary).unlink(missing_ok=True)
    info = bundle / "Contents/Info.plist"
    if info.is_symlink():
        raise ValueError("Development bundle Info.plist cannot be a symlink")
    label = f"{config.get('productName', 'Loupist')} Dev"
    info.write_bytes(
        plistlib.dumps(
            {
                "CFBundleIdentifier": bundle_id,
                "CFBundleName": label,
                "CFBundleDisplayName": label,
                "CFBundleExecutable": "loupist",
                "CFBundlePackageType": "APPL",
                "CFBundleShortVersionString": config.get("version", "0.0.0"),
                "CFBundleVersion": "1",
                "NSHighResolutionCapable": True,
                "LSMinimumSystemVersion": "13.0",
            }
        )
    )
    return destination


def main() -> None:
    if sys.platform != "darwin" or len(sys.argv) < 2:
        raise ValueError(
            "Usage on macOS: macos_dev_app.py <debug/loupist> [app arguments]"
        )
    executable = prepare_bundle(
        Path(sys.argv[1]), Path.cwd(), os.environ.get("TAURI_CONFIG", "{}")
    )
    print(f"Loupist development bundle: {executable.parents[2]}", flush=True)
    # exec preserves Cargo's ownership, signals and inherited GUI lease. `open`
    # would detach and let the watcher/launcher lose the actual native process.
    os.execv(str(executable), [str(executable), *sys.argv[2:]])


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"macOS development app: {error}", file=sys.stderr)
        sys.exit(1)
