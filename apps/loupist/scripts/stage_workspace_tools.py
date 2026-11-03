#!/usr/bin/env python3
"""Stage the pinned offline fd dependency used by Pi's native find tool."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request
import zipfile
from pathlib import Path


FD_VERSION = "10.5.0"
FD_RELEASES = {
    "aarch64-apple-darwin": (
        "tar.gz",
        "b67e1836c468e42e411984b56e52fa7abec08c2bd22c867398e7cc134aac5e12",
    ),
    "x86_64-pc-windows-msvc": (
        "zip",
        "a227701b8551c35a9931d9f6da75503cf86d88e182d71fb849a70864c5d57cd7",
    ),
}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def executable_name(target: str) -> str:
    return "fd.exe" if "windows" in target else "fd"


def verify_workspace_tools(
    runtime: Path, target: str, *, record_signature: bool = False
) -> None:
    tools = runtime / "workspace-tools"
    manifest_path = tools / "manifest.json"
    manifest = json.loads(manifest_path.read_text())
    if (
        manifest.get("version"),
        manifest.get("target"),
        manifest.get("archive_sha256"),
    ) != (FD_VERSION, target, FD_RELEASES[target][1]):
        raise ValueError("staged fd release identity does not match its pin")
    executable = tools / executable_name(target)
    if executable.is_symlink() or not executable.is_file():
        raise ValueError("staged fd executable is missing or linked")
    actual = digest(executable.read_bytes())
    if not record_signature and actual != manifest.get("executable_sha256"):
        raise ValueError("staged fd executable differs from its verified digest")
    version = subprocess.check_output(
        [str(executable), "--version"], text=True, timeout=10
    ).strip()
    if version != f"fd {FD_VERSION}":
        raise ValueError("staged fd version does not match its pin")
    for name in ["LICENSE-MIT", "LICENSE-APACHE"]:
        if not (tools / name).is_file():
            raise ValueError(f"staged fd license is missing: {name}")
    if record_signature:
        manifest["executable_sha256"] = actual
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")


def stage_workspace_tools(
    runtime: Path, target: str, archive: Path | None = None
) -> None:
    extension, archive_digest = FD_RELEASES[target]
    prefix = f"fd-v{FD_VERSION}-{target}"
    if archive:
        encoded = archive.read_bytes()
    else:
        url = f"https://github.com/sharkdp/fd/releases/download/v{FD_VERSION}/{prefix}.{extension}"
        with urllib.request.urlopen(url, timeout=60) as response:
            encoded = response.read(16 * 1024 * 1024 + 1)
    if digest(encoded) != archive_digest:
        raise ValueError("fd archive checksum mismatch")
    names = [executable_name(target), "LICENSE-MIT", "LICENSE-APACHE"]
    # Read only exact, regular pinned members; never extract archive paths.
    files: dict[str, bytes] = {}
    if extension == "zip":
        with zipfile.ZipFile(io.BytesIO(encoded)) as source:
            for name in names:
                files[name] = source.read(f"{prefix}/{name}")
    else:
        with tarfile.open(fileobj=io.BytesIO(encoded), mode="r:gz") as source:
            for name in names:
                member = source.getmember(f"{prefix}/{name}")
                if not member.isfile():
                    raise ValueError("fd archive member is not a regular file")
                stream = source.extractfile(member)
                assert stream is not None
                files[name] = stream.read()
    runtime.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix=".workspace-tools-", dir=runtime
    ) as temporary:
        temporary_root = Path(temporary)
        prepared = temporary_root / "workspace-tools"
        prepared.mkdir()
        for name, data in files.items():
            (prepared / name).write_bytes(data)
        (prepared / executable_name(target)).chmod(0o755)
        (prepared / "manifest.json").write_text(
            json.dumps(
                {
                    "version": FD_VERSION,
                    "target": target,
                    "archive_sha256": archive_digest,
                    "executable_sha256": digest(files[executable_name(target)]),
                },
                indent=2,
            )
            + "\n"
        )
        verify_workspace_tools(temporary_root, target)
        destination = runtime / "workspace-tools"
        previous = temporary_root / "previous"
        if destination.exists():
            destination.rename(previous)
        try:
            prepared.rename(destination)
        except BaseException:
            if previous.exists():
                previous.rename(destination)
            raise
        if previous.exists():
            shutil.rmtree(previous)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, required=True)
    parser.add_argument("--target", choices=FD_RELEASES, required=True)
    parser.add_argument(
        "--archive", type=Path, default=os.environ.get("LOUPIST_FD_ARCHIVE")
    )
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--verify", action="store_true")
    modes.add_argument("--record-signature", action="store_true")
    args = parser.parse_args()
    if args.verify or args.record_signature:
        verify_workspace_tools(
            args.runtime, args.target, record_signature=args.record_signature
        )
    else:
        stage_workspace_tools(args.runtime, args.target, args.archive)
    print(f"Verified offline fd {FD_VERSION} for {args.target}.")


if __name__ == "__main__":
    main()
