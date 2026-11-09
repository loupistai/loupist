#!/usr/bin/env python3
"""Build the trusted Rust worker and stage a pinned offline WASI scientific stack."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import platform
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
import zipfile

APP = Path(__file__).resolve().parents[1]
WORKER = APP / "python-worker"
CRATE_LICENSES = Path(__file__).resolve().parent / "python-worker-crate-licenses.json"
PIN = json.loads((WORKER / "runtime.json").read_text())
RUNTIME = APP / "src-tauri/resources/python-runtime"
BINARY = (
    "loupist-python-worker.exe" if sys.platform == "win32" else "loupist-python-worker"
)
MARKER = ".runtime.json"
STAGING_BUDGET = 256 * 1024 * 1024
WORKER_ENTITLEMENT = "com.apple.security.cs.allow-unsigned-executable-memory"
REQUIRED_PACKAGES = (
    "numpy",
    "pandas",
    "scipy",
    "matplotlib",
    "statsmodels",
    "scikit-learn",
)
LICENSE_ALIASES = {"sqlite-autoconf": "sqlite"}
ZIP_MODULES = {
    "numpy": "numpy/__init__.pyc",
    "pandas": "pandas/__init__.pyc",
    "scipy": "scipy/__init__.pyc",
    "matplotlib": "matplotlib/__init__.pyc",
    "statsmodels": "statsmodels/__init__.pyc",
    "scikit-learn": "sklearn/__init__.pyc",
}
SMOKE_CODE = (
    "import numpy, pandas, scipy, matplotlib, statsmodels, sklearn\n"
    "print('loupist-python-ok')\n"
)
HOST_MACHINES = {
    ("darwin", "arm64"): "arm64",
    ("linux", "x86_64"): "x86_64",
    ("linux", "aarch64"): "aarch64",
    ("linux", "arm64"): "aarch64",
    ("win32", "AMD64"): "x86_64",
    ("win32", "ARM64"): "aarch64",
}


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def source_digest() -> str:
    value = hashlib.sha256()
    for path in sorted(
        [
            WORKER / "Cargo.toml",
            WORKER / "Cargo.lock",
            WORKER / "runtime.json",
            WORKER / "Entitlements.plist",
            *WORKER.glob("src/*.rs"),
        ]
    ):
        value.update(path.relative_to(WORKER).as_posix().encode())
        value.update(path.read_bytes())
    value.update(subprocess.check_output(["rustc", "--version"]))
    return value.hexdigest()


def manifest(root: Path) -> dict[str, str]:
    result = {}
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise ValueError("Python runtime contains a symlink")
        if path.is_file() and path.name != MARKER:
            result[path.relative_to(root).as_posix()] = digest(path)
    return result


def packaged_distributions(text: str) -> list[str]:
    names = []
    for line in text.splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[0] == "package":
            names.append(parts[1])
    return names


def lock_packages(text: str) -> set[tuple[str, str]]:
    """Registry packages in a Cargo.lock. Lockfile v4 has no license field."""
    packages: set[tuple[str, str]] = set()
    name = version = source = None

    def finish() -> None:
        nonlocal name, version, source
        if name and version and source and source.startswith("registry+"):
            packages.add((name, version))
        name = version = source = None

    for line in text.splitlines():
        if line.startswith("[[package]]"):
            finish()
        elif line.startswith("name = "):
            name = line.split("=", 1)[1].strip().strip('"')
        elif line.startswith("version = "):
            version = line.split("=", 1)[1].strip().strip('"')
        elif line.startswith("source = "):
            source = line.split("=", 1)[1].strip().strip('"')
    finish()
    return packages


def assert_license_set(
    locked: set[tuple[str, str]], notice: list[dict[str, str]]
) -> None:
    recorded = {
        (item["name"], item["version"]): item.get("license", "") for item in notice
    }
    missing = sorted(locked - set(recorded))
    extra = sorted(set(recorded) - locked)
    blank = sorted(
        key for key, license_name in recorded.items() if not license_name.strip()
    )
    if missing or extra or blank:
        detail = []
        if missing:
            detail.append(
                "missing " + ", ".join(f"{name} {version}" for name, version in missing)
            )
        if extra:
            detail.append(
                "extra " + ", ".join(f"{name} {version}" for name, version in extra)
            )
        if blank:
            detail.append(
                "blank " + ", ".join(f"{name} {version}" for name, version in blank)
            )
        raise ValueError(
            "Rust crate license notice does not match Cargo.lock: " + "; ".join(detail)
        )


def assert_rust_crate_licenses(root: Path) -> None:
    staged = (root / "rust-crate-licenses.json").read_bytes()
    if staged != CRATE_LICENSES.read_bytes():
        raise ValueError(
            "Staged Rust crate license notice does not match the source notice"
        )
    notice = json.loads(staged)
    locked = lock_packages((APP / "python-worker" / "Cargo.lock").read_text())
    assert_license_set(locked, notice["packages"])


def assert_licenses(root: Path) -> None:
    names = packaged_distributions((root / "MANIFEST").read_text())
    missing = [
        name
        for name in names
        if not (root / "LICENSES" / f"{LICENSE_ALIASES.get(name, name)}.txt").is_file()
    ]
    if missing:
        raise ValueError(
            "Python runtime is missing dependency licenses: " + ", ".join(missing)
        )
    absent = [name for name in REQUIRED_PACKAGES if name not in names]
    if absent:
        raise ValueError("Python runtime MANIFEST omits " + ", ".join(absent))
    for name in REQUIRED_PACKAGES:
        license_path = root / "LICENSES" / f"{name}.txt"
        if license_path.stat().st_size == 0:
            raise ValueError(f"Python runtime license is empty: {name}")


def assert_offline_packages(root: Path) -> None:
    with zipfile.ZipFile(root / "lib/python315.zip") as stdlib:
        names = set(stdlib.namelist())
    missing = [
        ZIP_MODULES[name]
        for name in REQUIRED_PACKAGES
        if ZIP_MODULES[name] not in names
    ]
    if missing:
        raise ValueError("Offline Python runtime is missing " + ", ".join(missing))


def executable_machine(path: Path) -> str:
    header = path.read_bytes()[:64]
    if header.startswith(b"\x7fELF"):
        machine = struct.unpack_from("<H", header, 18)[0]
        return {62: "x86_64", 183: "aarch64"}.get(machine, f"elf-{machine}")
    if len(header) >= 8 and struct.unpack_from("<I", header, 0)[0] == 0xFEEDFACF:
        cpu = struct.unpack_from("<I", header, 4)[0]
        return {0x0100000C: "arm64", 0x01000007: "x86_64"}.get(cpu, f"macho-{cpu}")
    if header.startswith(b"MZ") and len(header) >= 64:
        blob = path.read_bytes()
        pe_offset = struct.unpack_from("<I", header, 0x3C)[0]
        if pe_offset + 6 <= len(blob) and blob[pe_offset : pe_offset + 4] == b"PE\0\0":
            machine = struct.unpack_from("<H", blob, pe_offset + 4)[0]
            return {0x8664: "x86_64", 0xAA64: "aarch64"}.get(machine, f"pe-{machine}")
    raise ValueError("Python worker is not a supported executable")


def assert_architecture(root: Path) -> None:
    expected = HOST_MACHINES.get((sys.platform, platform.machine()))
    if expected is None:
        raise ValueError("Python worker staging does not support this host")
    actual = executable_machine(root / BINARY)
    if actual != expected:
        raise ValueError(
            f"Python worker architecture is {actual}, host requires {expected}"
        )
    if not (root / "python-snapshot.wasm").read_bytes().startswith(b"\0asm"):
        raise ValueError("Pinned Python snapshot is not a WebAssembly module")


def verify(root: Path, source: str | None = None) -> None:
    record = json.loads((root / MARKER).read_text())
    if record.get("version") != PIN["version"] or record.get("files") != manifest(root):
        raise ValueError("Python runtime files changed; restage the pinned runtime")
    for original, installed in (
        ("python-snapshot.wasm", "python-snapshot.wasm"),
        ("python-stdlib.zip", "lib/python315.zip"),
    ):
        if record["files"].get(installed) != PIN["files"][original]:
            raise ValueError("Python runtime does not match its release pin")
    if not (root / BINARY).is_file() or not (root / "mpl-data/matplotlibrc").is_file():
        raise ValueError("Python worker or matplotlib data is missing")
    if source is not None and source != record.get("worker_source"):
        raise ValueError("Python worker source changed")
    assert_licenses(root)
    assert_offline_packages(root)
    assert_architecture(root)
    assert_rust_crate_licenses(root)


def codesign_command(executable: Path) -> list[str]:
    identity = os.environ.get("APPLE_SIGNING_IDENTITY") or "-"
    command = [
        "codesign",
        "--force",
        "--sign",
        identity,
        "--options",
        "runtime",
        "--entitlements",
        str(WORKER / "Entitlements.plist"),
    ]
    if identity != "-":
        command.append("--timestamp")
    command.append(str(executable))
    return command


def codesign_text(executable: Path, *args: str) -> str:
    result = subprocess.run(
        ["codesign", *args, str(executable)],
        check=False,
        capture_output=True,
        text=True,
    )
    detail = result.stdout + result.stderr
    if result.returncode != 0:
        raise ValueError(detail.strip() or "codesign failed")
    return detail


def entitlement_is_true(text: str, key: str) -> bool:
    """Accept the XML plist and the current codesign entitlement dump."""
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if key not in line:
            continue
        window = "\n".join(lines[index : index + 5])
        if "<false" in window or "[Bool] false" in window:
            return False
        if "<true" in window or "[Bool] true" in window:
            return True
    return False


def require_worker_signature(executable: Path) -> None:
    flags = ""
    for line in codesign_text(executable, "-d", "-vv").splitlines():
        if "flags=" in line:
            flags = line
            break
    if "runtime" not in flags:
        raise ValueError("Python worker is not signed with the hardened runtime")
    entitlements = codesign_text(executable, "-d", "--entitlements", "-")
    if not entitlement_is_true(entitlements, WORKER_ENTITLEMENT):
        raise ValueError("Python worker is missing its executable-memory entitlement")
    if "allow-jit" in entitlements:
        raise ValueError("Python worker signature grants allow-jit")


def apply_distribution_signature(root: Path) -> None:
    """An unsigned cache is reusable, but a requested signer must be applied."""
    if sys.platform != "darwin" or os.environ.get("APPLE_SIGNING_IDENTITY", "-") in (
        "",
        "-",
    ):
        return
    executable = root / BINARY
    subprocess.run(codesign_command(executable), check=True)
    require_worker_signature(executable)
    record = json.loads((root / MARKER).read_text())
    record["files"] = manifest(root)
    (root / MARKER).write_text(json.dumps(record, sort_keys=True) + "\n")


def require_app_without_worker_entitlement(executable: Path) -> None:
    entitlements = codesign_text(executable, "-d", "--entitlements", "-")
    if WORKER_ENTITLEMENT in entitlements:
        raise ValueError(
            "Loupist app binary has the Python worker executable-memory entitlement"
        )


def safe_parts(name: str) -> tuple[str, ...]:
    path = PurePosixPath(name)
    if (
        path.is_absolute()
        or not path.parts
        or any(part in (".", "..") for part in path.parts)
    ):
        raise ValueError("Invalid runtime archive path")
    return path.parts


def unpack(archive: Path, destination: Path) -> None:
    if digest(archive) != PIN["archive_sha256"]:
        raise ValueError("WASI Python archive checksum mismatch")
    prefix = "wasi-python-" + PIN["version"]
    total = 0
    with tarfile.open(archive, "r:xz") as bundle:
        for member in bundle:
            parts = safe_parts(member.name)
            if parts[0] != prefix or member.isdir():
                continue
            if not member.isfile() or member.size < 0:
                raise ValueError("WASI Python archive contains a non-regular file")
            relative = PurePosixPath(*parts[1:])
            if str(relative) == "python.wasm":
                continue  # Only the initialized reactor is shipped.
            total += member.size
            if total > STAGING_BUDGET:
                raise ValueError("WASI Python archive exceeds the staging budget")
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            with bundle.extractfile(member) as source, target.open("wb") as output:
                shutil.copyfileobj(source, output)
    for name, expected in PIN["files"].items():
        if digest(destination / name) != expected:
            raise ValueError("WASI Python asset checksum mismatch")
    (destination / "lib").mkdir()
    (destination / "python-stdlib.zip").rename(destination / "lib/python315.zip")
    with zipfile.ZipFile(destination / "lib/python315.zip") as stdlib:
        data = stdlib.read("matplotlib/mpl-data.zip")
    with zipfile.ZipFile(io.BytesIO(data)) as data_zip:
        for entry in data_zip.infolist():
            parts = safe_parts(entry.filename)
            if entry.is_dir():
                continue
            total += entry.file_size
            if total > STAGING_BUDGET:
                raise ValueError("Matplotlib data exceeds the staging budget")
            target = destination / "mpl-data" / Path(*parts)
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data_zip.read(entry))


def download_archive(destination: Path) -> None:
    for attempt in range(3):
        try:
            with (
                urllib.request.urlopen(PIN["url"], timeout=120) as response,
                destination.open("wb") as output,
            ):
                shutil.copyfileobj(response, output)
            return
        except urllib.error.HTTPError as error:
            if error.code not in (502, 503, 504) or attempt == 2:
                raise
            error.close()
            time.sleep(2**attempt)


def stage(root: Path, archive: Path | None, source: str) -> None:
    subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "--locked",
            "--manifest-path",
            str(WORKER / "Cargo.toml"),
        ],
        check=True,
    )
    root.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix="python-stage-", dir=root.parent
    ) as temporary:
        stage_root = Path(temporary) / "runtime"
        stage_root.mkdir()
        if archive is None:
            archive = Path(temporary) / "python.tar.xz"
            download_archive(archive)
        unpack(archive, stage_root)
        executable = stage_root / BINARY
        shutil.copy2(WORKER / "target/release" / BINARY, executable)
        executable.chmod(0o755)
        if sys.platform == "darwin":
            subprocess.run(codesign_command(executable), check=True)
        shutil.copy2(CRATE_LICENSES, stage_root / "rust-crate-licenses.json")
        (stage_root / MARKER).write_text(
            json.dumps(
                {
                    "version": PIN["version"],
                    "worker_source": source,
                    "files": manifest(stage_root),
                },
                sort_keys=True,
            )
            + "\n"
        )
        verify(stage_root, source)
        if sys.platform == "darwin":
            require_worker_signature(executable)
        if root.exists():
            shutil.rmtree(root)
        stage_root.rename(root)


def smoke(root: Path) -> None:
    verify(root, source_digest())
    if sys.platform == "darwin":
        require_worker_signature(root / BINARY)
    with tempfile.TemporaryDirectory(prefix="python-smoke-") as temporary:
        base = Path(temporary)
        for name in ("workspace", "attachments", "scratch"):
            (base / "snapshot" / name).mkdir(parents=True)
        # Matplotlib writes its config here. The guest path is /scratch/matplotlib.
        (base / "snapshot" / "scratch" / "matplotlib").mkdir()
        (base / "cache").mkdir()
        request = {
            "code": SMOKE_CODE,
            "timeout_seconds": 120,
            "runtime": str((root).resolve()),
            "cache": str((base / "cache").resolve()),
            "snapshot": str((base / "snapshot").resolve()),
            "writable": [],
        }
        result = subprocess.run(
            [root / BINARY],
            input=json.dumps(request).encode(),
            capture_output=True,
            timeout=180,
            check=False,
        )
    if result.returncode != 0 or b"loupist-python-ok" not in result.stdout:
        detail = (result.stderr or result.stdout).decode(errors="replace")[-2000:]
        raise ValueError(f"Python worker smoke failed ({result.returncode}): {detail}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, default=RUNTIME)
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("--ensure", action="store_true")
    parser.add_argument("--require-signature", action="store_true")
    parser.add_argument("--smoke", action="store_true")
    parser.add_argument("--app-binary", type=Path)
    args = parser.parse_args()
    checked = False
    if args.verify:
        verify(args.runtime, source_digest())
        checked = True
    if args.require_signature:
        require_worker_signature(args.runtime / BINARY)
        checked = True
    if args.app_binary:
        require_app_without_worker_entitlement(args.app_binary)
        checked = True
    if args.smoke:
        smoke(args.runtime)
        checked = True
    if not checked:
        source = source_digest()
        if args.ensure:
            try:
                verify(args.runtime, source)
                if sys.platform == "darwin":
                    require_worker_signature(args.runtime / BINARY)
                    apply_distribution_signature(args.runtime)
                    verify(args.runtime, source)
                print("Pinned offline Python runtime is current.")
                return
            except (OSError, ValueError, KeyError):
                pass
        stage(args.runtime, args.archive, source)
    print("Pinned offline Python runtime verified.")


if __name__ == "__main__":
    main()
