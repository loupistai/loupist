"""Pinned Python runtime staging rejects drift, network, and a bad signature split."""

from __future__ import annotations

import hashlib
import io
import json
import os
import re
import struct
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

import check_runner
import stage_python_runtime as stage

APP_ENTITLEMENT = "com.apple.security.cs.allow-unsigned-executable-memory"
REAL_PIN = stage.PIN
REAL_WORKER = stage.WORKER


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def arm64_macho() -> bytes:
    return struct.pack("<IIIIIIII", 0xFEEDFACF, 0x0100000C, 0, 2, 0, 0, 0, 0)


def elf_x86_64() -> bytes:
    header = bytearray(64)
    header[0:4] = b"\x7fELF"
    header[4] = 2
    header[5] = 1
    struct.pack_into("<H", header, 18, 62)
    return bytes(header)


def pe_x86_64() -> bytes:
    blob = bytearray(128)
    blob[0:2] = b"MZ"
    struct.pack_into("<I", blob, 0x3C, 0x40)
    blob[0x40:0x44] = b"PE\0\0"
    struct.pack_into("<H", blob, 0x44, 0x8664)
    return bytes(blob)


def mpl_zip() -> bytes:
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as archive:
        archive.writestr("matplotlibrc", "backend: Agg\n")
    return buffer.getvalue()


def stdlib_zip() -> bytes:
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as archive:
        archive.writestr("matplotlib/mpl-data.zip", mpl_zip())
        for name in stage.ZIP_MODULES.values():
            archive.writestr(name, b"offline")
    return buffer.getvalue()


def manifest_text() -> str:
    lines = ["python 3.15.2"]
    lines.extend(
        f"package {name} 1" for name in (*stage.REQUIRED_PACKAGES, "sqlite-autoconf")
    )
    return "\n".join(lines) + "\n"


class PythonRuntimeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.worker = self.root / "worker"
        self.worker.mkdir()
        (self.worker / "Entitlements.plist").write_text(
            (stage.WORKER / "Entitlements.plist").read_text()
        )
        wasm = b"\0asmfake"
        stdlib = stdlib_zip()
        self.pin = {
            "version": "9.9.9-test",
            "url": "https://example.invalid/python.tar.xz",
            "archive_sha256": "",
            "files": {
                "python-snapshot.wasm": sha256(wasm),
                "python-stdlib.zip": sha256(stdlib),
            },
        }
        self.archive = self.root / "python.tar.xz"
        self.write_archive(self.archive, wasm, stdlib, manifest_text())
        self.pin["archive_sha256"] = sha256(self.archive.read_bytes())
        self.addCleanup(setattr, stage, "PIN", stage.PIN)
        self.addCleanup(setattr, stage, "WORKER", stage.WORKER)
        self.addCleanup(setattr, stage, "STAGING_BUDGET", stage.STAGING_BUDGET)
        stage.PIN = self.pin
        stage.WORKER = self.worker

    def write_archive(
        self, path: Path, wasm: bytes, stdlib: bytes, manifest: str
    ) -> None:
        prefix = "wasi-python-" + self.pin["version"]
        members = {
            "python-snapshot.wasm": wasm,
            "python-stdlib.zip": stdlib,
            "python.wasm": b"do-not-ship",
            "MANIFEST": manifest.encode(),
        }
        for name in (*stage.REQUIRED_PACKAGES, "sqlite"):
            members[f"LICENSES/{name}.txt"] = b"license text\n"
        with tarfile.open(path, "w:xz") as archive:
            for name, data in members.items():
                info = tarfile.TarInfo(f"{prefix}/{name}")
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))

    def run_processes(self, binary: bytes):
        signed = []

        def run(command, check=False, capture_output=False, text=False, **_kwargs):
            command = [str(part) for part in command]
            if command[0] == "cargo":
                target = stage.WORKER / "target/release" / stage.BINARY
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(binary)
                return subprocess.CompletedProcess(command, 0, "", "")
            if command[0] == "codesign" and "--force" in command:
                signed.append(command)
                path = Path(command[-1])
                path.write_bytes(path.read_bytes() + b"SIGNED")
                return subprocess.CompletedProcess(command, 0, "", "")
            if command[0] == "codesign" and "--entitlements" in command:
                body = f"{APP_ENTITLEMENT}\n<true/>\n"
                return subprocess.CompletedProcess(command, 0, body, "")
            if command[0] == "codesign" and "-vv" in command:
                detail = "flags=0x10002(adhoc,runtime)\n"
                return subprocess.CompletedProcess(command, 0, "", detail)
            raise AssertionError(command)

        return signed, run

    def stage_runtime(self, platform_name: str, machine: str, binary: bytes) -> Path:
        destination = self.root / f"runtime-{platform_name}"
        signed, run = self.run_processes(binary)
        with (
            patch.dict(os.environ, {"APPLE_SIGNING_IDENTITY": ""}),
            patch.object(stage.sys, "platform", platform_name),
            patch.object(stage.platform, "machine", return_value=machine),
            patch.object(stage, "source_digest", return_value="source-a"),
            patch.object(stage.subprocess, "run", side_effect=run),
            patch.object(
                stage.urllib.request, "urlopen", side_effect=AssertionError("network")
            ),
        ):
            stage.stage(destination, self.archive, "source-a")
        self.signed = signed
        return destination

    def test_stage_records_the_signed_worker_and_does_not_download(self):
        destination = self.stage_runtime("darwin", "arm64", arm64_macho())
        worker = destination / stage.BINARY
        record = json.loads((destination / MARKER).read_text())
        self.assertTrue(worker.read_bytes().endswith(b"SIGNED"))
        self.assertEqual(record["files"][stage.BINARY], stage.digest(worker))
        self.assertNotEqual(stage.digest(worker), sha256(arm64_macho()))
        self.assertEqual(record["worker_source"], "source-a")
        self.assertEqual(record["version"], self.pin["version"])
        self.assertFalse((destination / "python.wasm").exists())
        self.assertTrue((destination / "mpl-data/matplotlibrc").is_file())
        self.assertEqual(
            self.signed[0][:8],
            [
                "codesign",
                "--force",
                "--sign",
                "-",
                "--options",
                "runtime",
                "--entitlements",
                str(self.worker / "Entitlements.plist"),
            ],
        )
        destination.joinpath("mpl-data/matplotlibrc").write_text("changed\n")
        with (
            patch.object(stage.sys, "platform", "darwin"),
            patch.object(stage.platform, "machine", return_value="arm64"),
            self.assertRaisesRegex(ValueError, "files changed"),
        ):
            stage.verify(destination, "source-a")

    def test_reusing_an_unsigned_runtime_applies_the_requested_signer(self):
        destination = self.stage_runtime("darwin", "arm64", arm64_macho())
        old_digest = stage.digest(destination / stage.BINARY)
        signed, run = self.run_processes(arm64_macho())
        with (
            patch.dict(
                os.environ,
                {"APPLE_SIGNING_IDENTITY": "Developer ID Application: Example (TEST)"},
            ),
            patch.object(stage.sys, "platform", "darwin"),
            patch.object(stage.platform, "machine", return_value="arm64"),
            patch.object(stage.subprocess, "run", side_effect=run),
        ):
            stage.apply_distribution_signature(destination)
            stage.verify(destination, "source-a")
        self.assertEqual(signed[0][3], "Developer ID Application: Example (TEST)")
        self.assertIn("--timestamp", signed[0])
        self.assertNotEqual(stage.digest(destination / stage.BINARY), old_digest)
        record = json.loads((destination / MARKER).read_text())
        self.assertEqual(
            record["files"][stage.BINARY], stage.digest(destination / stage.BINARY)
        )

    def test_linux_stage_does_not_sign(self):
        destination = self.stage_runtime("linux", "x86_64", elf_x86_64())
        self.assertEqual(self.signed, [])
        self.assertFalse((destination / stage.BINARY).read_bytes().endswith(b"SIGNED"))
        with (
            patch.object(stage.sys, "platform", "linux"),
            patch.object(stage.platform, "machine", return_value="x86_64"),
        ):
            stage.verify(destination, "source-a")

    def test_windows_stage_accepts_an_x64_worker_without_signing(self):
        destination = self.stage_runtime("win32", "AMD64", pe_x86_64())
        self.assertEqual(self.signed, [])
        with (
            patch.object(stage.sys, "platform", "win32"),
            patch.object(stage.platform, "machine", return_value="AMD64"),
        ):
            stage.verify(destination, "source-a")

    def test_second_stage_replaces_a_partial_tree(self):
        destination = self.stage_runtime("linux", "x86_64", elf_x86_64())
        (destination / "leftover").write_text("partial")
        again = self.stage_runtime("linux", "x86_64", elf_x86_64())
        self.assertEqual(again, destination)
        self.assertFalse((destination / "leftover").exists())
        with (
            patch.object(stage.sys, "platform", "linux"),
            patch.object(stage.platform, "machine", return_value="x86_64"),
        ):
            stage.verify(destination, "source-a")

    def test_archive_checksum_mismatch_is_rejected(self):
        self.pin["archive_sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "archive checksum mismatch"):
            stage.unpack(self.archive, self.root / "bad")

    def test_symlink_and_parent_paths_are_rejected(self):
        prefix = "wasi-python-" + self.pin["version"]
        linked = self.root / "linked.tar.xz"
        with tarfile.open(linked, "w:xz") as archive:
            info = tarfile.TarInfo(f"{prefix}/escape")
            info.type = tarfile.SYMTYPE
            info.linkname = "python-snapshot.wasm"
            archive.addfile(info)
        self.pin["archive_sha256"] = sha256(linked.read_bytes())
        with self.assertRaisesRegex(ValueError, "non-regular file"):
            stage.unpack(linked, self.root / "linked")
        escaped = self.root / "escaped.tar.xz"
        with tarfile.open(escaped, "w:xz") as archive:
            info = tarfile.TarInfo(f"{prefix}/../outside")
            info.size = 1
            archive.addfile(info, io.BytesIO(b"x"))
        self.pin["archive_sha256"] = sha256(escaped.read_bytes())
        with self.assertRaisesRegex(ValueError, "Invalid runtime archive path"):
            stage.unpack(escaped, self.root / "escaped")

    def test_over_budget_archive_is_rejected(self):
        stage.STAGING_BUDGET = 4
        with self.assertRaisesRegex(ValueError, "staging budget"):
            stage.unpack(self.archive, self.root / "budget")

    def test_missing_archive_downloads_once(self):
        payload = self.archive.read_bytes()

        class Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *_args):
                return False

        calls = []

        def urlopen(url, timeout):
            calls.append((url, timeout))
            return Response(payload)

        signed, run = self.run_processes(elf_x86_64())
        with (
            patch.object(stage.sys, "platform", "linux"),
            patch.object(stage.platform, "machine", return_value="x86_64"),
            patch.object(stage, "source_digest", return_value="source-a"),
            patch.object(stage.subprocess, "run", side_effect=run),
            patch.object(stage.urllib.request, "urlopen", side_effect=urlopen),
        ):
            stage.stage(self.root / "downloaded", None, "source-a")
        self.assertEqual(calls, [(self.pin["url"], 120)])
        self.assertEqual(signed, [])

    def test_removed_license_fails_verification(self):
        destination = self.stage_runtime("linux", "x86_64", elf_x86_64())
        (destination / "LICENSES/numpy.txt").unlink()
        record = json.loads((destination / MARKER).read_text())
        record["files"] = stage.manifest(destination)
        (destination / MARKER).write_text(json.dumps(record))
        with self.assertRaisesRegex(ValueError, "missing dependency licenses: numpy"):
            stage.verify(destination)

    def test_setup_and_rust_checks_stage_then_verify(self):
        setup = check_runner.setup_steps("rust", [])
        self.assertEqual(
            [item["name"] for item in setup],
            ["install-cargo", "install-python-runtime"],
        )
        self.assertIn("--ensure", setup[1]["command"])
        installed = [item["name"] for item in check_runner.rust_plan(True)]
        checked = [item["name"] for item in check_runner.rust_plan()]
        self.assertEqual(installed[0], "install-python-runtime")
        self.assertIn("python-runtime-verify", checked)
        self.assertNotIn("install-python-runtime", checked)
        self.assertIn("--verify", check_runner.rust_plan()[-1]["command"])

    def test_package_configs_keep_the_entitlement_on_the_worker(self):
        repo = stage.APP.parents[1]
        for name in ("tauri.release.conf.json", "tauri.package-smoke.conf.json"):
            config = json.loads((stage.APP / "src-tauri" / name).read_text())
            self.assertEqual(
                config["bundle"]["resources"]["resources/python-runtime/"],
                "python-runtime",
            )
            self.assertNotIn(APP_ENTITLEMENT, json.dumps(config))
        entitlements = (REAL_WORKER / "Entitlements.plist").read_text()
        self.assertIn(APP_ENTITLEMENT, entitlements)
        self.assertNotIn("allow-jit", entitlements)
        self.assertIn("python-runtime/", (repo / ".gitignore").read_text())
        self.assertIn("python-worker/target/", (repo / ".gitignore").read_text())
        workflow = (repo / ".github/workflows/ci.yml").read_text()
        for job in ("verify-rust", "verify-rust-macos"):
            # Each OS verifies the offline runtime on exactly one staged leg.
            # The full Rust-plan coverage is checked in test_ci_change_scope.
            block = workflow.split(f"  {job}:\n", 1)[1]
            block = re.split(r"\n  [a-z][a-z0-9-]*:\n", block, maxsplit=1)[0]
            self.assertEqual(block.count("python-runtime-verify"), 1)
            leg = block.split("python-runtime-verify", 1)[1].split("- leg:", 1)[0]
            self.assertIn('runtime: "true"', leg)
            self.assertIn("if: matrix.runtime == 'true'", block)
            self.assertIn("run: sh apps/loupist/scripts/setup.sh", block)
        self.assertIn(
            "python3 apps/loupist/scripts/stage_python_runtime.py --ensure",
            workflow,
        )
        native = workflow.split("native-e2e-linux:", 1)[1].split(
            "verify-rust-macos:", 1
        )[0]
        self.assertIn("stage_python_runtime.py --ensure", native)
        self.assertNotIn(
            "sudo",
            native.split("Stage the pinned offline Python runtime", 1)[1].split(
                "Run native", 1
            )[0],
        )
        release = (repo / ".github/workflows/release.yml").read_text()
        self.assertIn('"resources/python-runtime/": "python-runtime"', release)
        macos = (stage.APP / "scripts/stage-macos-arm64.sh").read_text()
        self.assertIn("stage_python_runtime.py", macos)
        bundle = (stage.APP / "scripts/check-macos-app-bundle.sh").read_text()
        self.assertIn("--smoke", bundle)
        self.assertIn("--app-binary", bundle)
        prerequisites = (
            stage.APP / "scripts/check-package-prerequisites.sh"
        ).read_text()
        self.assertIn("--require-signature", prerequisites)
        cargo = (REAL_WORKER / "Cargo.toml").read_text()
        self.assertIn('version = "=50.0.1"', cargo)
        windows = (stage.APP / "scripts/stage-windows-x64.ps1").read_text()
        self.assertIn("stage_python_runtime.py", windows)

    def test_registry_packages_need_a_license_notice(self):
        lock = "\n".join(
            [
                "version = 4",
                "[[package]]",
                'name = "demo"',
                'version = "1.2.3"',
                'source = "registry+https://github.com/rust-lang/crates.io-index"',
                "[[package]]",
                'name = "loupist-python-worker"',
                'version = "0.1.0"',
            ]
        )
        locked = stage.lock_packages(lock)
        self.assertEqual(locked, {("demo", "1.2.3")})
        stage.assert_license_set(
            locked, [{"name": "demo", "version": "1.2.3", "license": "MIT"}]
        )
        with self.assertRaisesRegex(ValueError, "missing demo 1.2.3"):
            stage.assert_license_set(locked, [])
        with self.assertRaisesRegex(ValueError, "blank demo 1.2.3"):
            stage.assert_license_set(
                locked, [{"name": "demo", "version": "1.2.3", "license": " "}]
            )

    def test_worker_crate_notice_covers_the_lock_without_using_cargo_lock_licenses(
        self,
    ):
        notice = json.loads(stage.CRATE_LICENSES.read_text())
        locked = stage.lock_packages(
            (stage.APP / "python-worker" / "Cargo.lock").read_text()
        )
        stage.assert_license_set(locked, notice["packages"])
        self.assertNotIn(
            "license", (stage.APP / "python-worker" / "Cargo.lock").read_text()
        )
        self.assertGreater(len(locked), 100)

    def test_verify_rejects_a_different_worker_source(self):
        destination = self.stage_runtime("linux", "x86_64", elf_x86_64())
        with (
            patch.object(stage.sys, "platform", "linux"),
            patch.object(stage.platform, "machine", return_value="x86_64"),
            self.assertRaisesRegex(ValueError, "source changed"),
        ):
            stage.verify(destination, "source-b")

    def test_codesign_bool_dump_counts_as_the_worker_entitlement(self):
        dump = (
            "[Dict]\n"
            "\t[Key] com.apple.security.cs.allow-unsigned-executable-memory\n"
            "\t[Value]\n"
            "\t\t[Bool] true\n"
        )
        self.assertTrue(stage.entitlement_is_true(dump, APP_ENTITLEMENT))
        denied = dump.replace("[Bool] true", "[Bool] false")
        self.assertFalse(stage.entitlement_is_true(denied, APP_ENTITLEMENT))

    def test_probe_archive_matches_the_pin_when_present(self):
        archive = Path("/tmp/loupist-python-probe/python.tar.xz")
        if not archive.is_file():
            self.skipTest("local probe archive is not on this machine")
        self.assertEqual(stage.digest(archive), REAL_PIN["archive_sha256"])

    def test_staged_tree_matches_its_manifest_when_present(self):
        if not (stage.RUNTIME / stage.MARKER).is_file():
            self.skipTest("runtime is staged by setup")
        stage.PIN = REAL_PIN
        stage.WORKER = REAL_WORKER
        stage.verify(stage.RUNTIME, stage.source_digest())
        if sys.platform == "darwin":
            stage.require_worker_signature(stage.RUNTIME / stage.BINARY)

    def test_baseline_app_lacks_the_worker_entitlement_when_present(self):
        binary = (
            stage.APP
            / "src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Loupist.app/Contents/MacOS/loupist"
        )
        if sys.platform != "darwin" or not binary.is_file():
            self.skipTest("baseline app is local macOS evidence")
        stage.require_app_without_worker_entitlement(binary)


MARKER = stage.MARKER


if __name__ == "__main__":
    unittest.main()
