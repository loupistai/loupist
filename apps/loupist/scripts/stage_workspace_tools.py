#!/usr/bin/env python3
"""Stage the pinned offline ripgrep used on admitted native grep descriptors."""

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
import time
import urllib.error
import urllib.request
import zipfile
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Tool:
    name: str
    version: str
    repository: str
    tag: str
    archive_prefix: str
    banner: str
    # target -> (archive extension, archive SHA-256, GitHub release asset ID)
    releases: dict[str, tuple[str, str, int]]
    # archive member -> staged name, besides the executable
    licenses: dict[str, str]

    def prefix(self, target: str) -> str:
        return self.archive_prefix.format(version=self.version, target=target)

    def release_url(self, target: str) -> str:
        return (
            f"https://github.com/{self.repository}/releases/download/{self.tag}/"
            f"{self.prefix(target)}.{self.releases[target][0]}"
        )

    def executable(self, target: str) -> str:
        return f"{self.name}.exe" if "windows" in target else self.name


TOOLS = (
    Tool(
        "rg",
        "15.2.0",
        "BurntSushi/ripgrep",
        "15.2.0",
        "ripgrep-{version}-{target}",
        "ripgrep 15.2.0",
        {
            "x86_64-unknown-linux-musl": (
                "tar.gz",
                "33e15bcf1624b25cdd2a55813a47a2f95dbe126268203e76aa6a585d1e7b149c",
                478120097,
            ),
            "aarch64-apple-darwin": (
                "tar.gz",
                "3750b2e93f37e0c692657da574d7019a101c0084da05a790c83fd335bad973e4",
                478118851,
            ),
            "x86_64-pc-windows-msvc": (
                "zip",
                "71b2fef860abe467217a538ff31de02f5258807c0129f771846f87bd029aafc5",
                478119643,
            ),
        },
        {"LICENSE-MIT": "rg-LICENSE-MIT", "UNLICENSE": "rg-UNLICENSE"},
    ),
)
TARGETS = tuple(TOOLS[0].releases)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def verify_workspace_tools(
    runtime: Path, target: str, *, record_signature: bool = False
) -> None:
    tools = runtime / "workspace-tools"
    manifest_path = tools / "manifest.json"
    manifest = json.loads(manifest_path.read_text())
    for tool in TOOLS:
        entry = manifest.get(tool.name)
        if not isinstance(entry, dict) or (
            entry.get("version"),
            entry.get("target"),
            entry.get("archive_sha256"),
        ) != (tool.version, target, tool.releases[target][1]):
            raise ValueError(
                f"staged {tool.name} release identity does not match its pin"
            )
        executable = tools / tool.executable(target)
        if executable.is_symlink() or not executable.is_file():
            raise ValueError(f"staged {tool.name} executable is missing or linked")
        actual = digest(executable.read_bytes())
        if not record_signature and actual != entry.get("executable_sha256"):
            raise ValueError(
                f"staged {tool.name} executable differs from its verified digest"
            )
        version = (
            subprocess.check_output(
                [str(executable), "--version"], text=True, timeout=10
            )
            .splitlines()[0]
            .strip()
        )
        if version != tool.banner and not version.startswith(f"{tool.banner} ("):
            raise ValueError(f"staged {tool.name} version does not match its pin")
        for name in tool.licenses.values():
            if not (tools / name).is_file():
                raise ValueError(f"staged {tool.name} license is missing: {name}")
        if record_signature:
            entry["executable_sha256"] = actual
    if record_signature:
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")


def read_members(tool: Tool, target: str, encoded: bytes) -> dict[str, bytes]:
    extension = tool.releases[target][0]
    prefix = tool.prefix(target)
    executable = tool.executable(target)
    # Read only exact, regular pinned members; never extract archive paths.
    members = {executable: executable, **tool.licenses}
    files: dict[str, bytes] = {}
    if extension == "zip":
        with zipfile.ZipFile(io.BytesIO(encoded)) as source:
            for name, staged in members.items():
                files[staged] = source.read(f"{prefix}/{name}")
    else:
        with tarfile.open(fileobj=io.BytesIO(encoded), mode="r:gz") as source:
            for name, staged in members.items():
                member = source.getmember(f"{prefix}/{name}")
                if not member.isfile():
                    raise ValueError(
                        f"{tool.name} archive member is not a regular file"
                    )
                stream = source.extractfile(member)
                assert stream is not None
                files[staged] = stream.read()
    return files


def download_archive(tool: Tool, target: str) -> bytes:
    request: str | urllib.request.Request = tool.release_url(target)
    for attempt in range(3):
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                return response.read(16 * 1024 * 1024 + 1)
        except urllib.error.HTTPError as error:
            if error.code not in (502, 503, 504) or attempt == 2:
                raise
            error.close()
            # GitHub's download edge can fail while its asset API still serves
            # the same pinned bytes. No authentication or alternate mirror.
            request = urllib.request.Request(
                f"https://api.github.com/repos/{tool.repository}/releases/assets/"
                f"{tool.releases[target][2]}",
                headers={"Accept": "application/octet-stream"},
            )
            time.sleep(2**attempt)
    raise AssertionError("unreachable download retry state")


def stage_workspace_tools(
    runtime: Path, target: str, archives: dict[str, Path] | None = None
) -> None:
    archives = archives or {}
    files: dict[str, bytes] = {}
    manifest: dict[str, dict[str, str]] = {}
    for tool in TOOLS:
        archive_digest = tool.releases[target][1]
        if tool.name in archives:
            encoded = archives[tool.name].read_bytes()
        else:
            encoded = download_archive(tool, target)
        if digest(encoded) != archive_digest:
            raise ValueError(f"{tool.name} archive checksum mismatch")
        members = read_members(tool, target, encoded)
        files.update(members)
        manifest[tool.name] = {
            "version": tool.version,
            "target": target,
            "archive_sha256": archive_digest,
            "executable_sha256": digest(members[tool.executable(target)]),
        }
    runtime.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix=".workspace-tools-", dir=runtime
    ) as temporary:
        temporary_root = Path(temporary)
        prepared = temporary_root / "workspace-tools"
        prepared.mkdir()
        for name, data in files.items():
            (prepared / name).write_bytes(data)
        for tool in TOOLS:
            (prepared / tool.executable(target)).chmod(0o755)
        (prepared / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
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
    parser.add_argument("--target", choices=TARGETS, required=True)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--verify", action="store_true")
    modes.add_argument("--ensure", action="store_true")
    modes.add_argument("--record-signature", action="store_true")
    args = parser.parse_args()
    if args.verify or args.record_signature:
        verify_workspace_tools(
            args.runtime, args.target, record_signature=args.record_signature
        )
    else:
        if args.ensure:
            try:
                verify_workspace_tools(args.runtime, args.target)
                print(f"Verified offline workspace tools for {args.target}.")
                return
            except (OSError, ValueError, subprocess.SubprocessError):
                pass
        archives = {
            tool.name: Path(path)
            for tool in TOOLS
            if (path := os.environ.get(f"LOUPIST_{tool.name.upper()}_ARCHIVE"))
        }
        stage_workspace_tools(args.runtime, args.target, archives)
    versions = ", ".join(tool.banner for tool in TOOLS)
    print(f"Verified offline workspace tools ({versions}) for {args.target}.")


if __name__ == "__main__":
    main()
