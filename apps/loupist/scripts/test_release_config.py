#!/usr/bin/env python3
"""Exercise the release-only Tauri configuration and macOS build counters."""

from __future__ import annotations

import json
import os
import plistlib
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
RELEASES = "https://github.com/loupistai/loupist/releases"
STABLE_ENDPOINT = f"{RELEASES}/latest/download/latest.json"
NEXT_RC_ENDPOINT = f"{RELEASES}/download/v1.2.3-rc.5/latest.json"


def run(script: str, *arguments: object, env: dict | None = None):
    return subprocess.run(
        [sys.executable, str(SCRIPTS / script), *map(str, arguments)],
        check=False,
        capture_output=True,
        text=True,
        env=env,
    )


class ReleaseScriptTest(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def assert_refused(self, completed, message: str) -> None:
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn(message, completed.stderr)


class ReleaseConfigTest(ReleaseScriptTest):
    def create(self, platform: str, *arguments: str, version: str | None = "1.2.3"):
        release = self.root / "tauri.release.conf.json"
        updater = {"windows": {"installMode": "passive"}}
        overlay = {
            "bundle": {"createUpdaterArtifacts": True},
            "plugins": {"updater": updater},
        }
        release.write_text(json.dumps(overlay))
        # An inherited endpoint variable must never redirect updates.
        env = {
            **os.environ,
            "LOUPIST_UPDATER_PUBLIC_KEY": "fixture-updater-public-key",
            "LOUPIST_UPDATE_ENDPOINTS": '["https://attacker.example.invalid/latest.json"]',
            "LOUPIST_WINDOWS_CERTIFICATE_THUMBPRINT": "A" * 40,
            "LOUPIST_WINDOWS_TIMESTAMP_URL": "https://timestamp.example.test",
        }
        version_arguments = [] if version is None else ["--version", version]
        return run(
            "create-release-config.py",
            *["--platform", platform, *version_arguments, *arguments],
            *["--release-config", release, "--output", self.root / "generated.json"],
            env=env,
        )

    def created(self, platform: str, *arguments: str, version: str = "1.2.3"):
        completed = self.create(platform, *arguments, version=version)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        return json.loads((self.root / "generated.json").read_text())

    def test_versions_derive_canonical_update_endpoints(self) -> None:
        stable = self.created("macos", "--macos-bundle-version", "17")
        self.assertEqual(stable["bundle"]["macOS"]["bundleVersion"], "17")
        self.assertEqual(stable["plugins"]["updater"]["endpoints"], [STABLE_ENDPOINT])
        self.assertIs(stable["plugins"]["updater"]["requireSignedVersion"], True)
        rc = self.created("windows", version="1.2.3-rc.4")
        endpoints = rc["plugins"]["updater"]["endpoints"]
        self.assertEqual(endpoints, [NEXT_RC_ENDPOINT, STABLE_ENDPOINT])
        self.assertIs(rc["plugins"]["updater"]["requireSignedVersion"], True)
        self.assertNotIn("macOS", rc["bundle"])

    def test_invalid_versions_and_build_counters_are_refused(self) -> None:
        self.assert_refused(self.create("windows", version=None), "--version")
        for version in (
            "1.2",
            "01.2.3",
            "1.2.3-beta.1",
            "1.2.3-rc.0",
            "1.2.3-rc.١",
            "1.2.3+build.1",
        ):
            with self.subTest(version=version):
                self.assert_refused(
                    self.create("windows", version=version), "--version"
                )
        for value in (None, "", "0", "-1", "01", "1.2", "counter"):
            with self.subTest(counter=value):
                counter = [] if value is None else ["--macos-bundle-version", value]
                completed = self.create("macos", *counter)
                self.assert_refused(completed, "--macos-bundle-version")
        windows = self.create("windows", "--macos-bundle-version", "17")
        self.assert_refused(windows, "valid only for macos")


class MacosBundleVersionTest(ReleaseScriptTest):
    def test_packaged_release_build_uses_the_explicit_counter(self) -> None:
        config = {"version": "0.0.1-rc.1", "bundle": {"macOS": {"bundleVersion": "1"}}}
        (self.root / "tauri.conf.json").write_text(json.dumps(config))
        for name, value in (
            ("Info.plist", {"CFBundleShortVersionString": "0.0.1"}),
            (
                "packaged-Info.plist",
                {"CFBundleShortVersionString": "0.0.1", "CFBundleVersion": "17"},
            ),
        ):
            with (self.root / name).open("wb") as output:
                plistlib.dump(value, output)

        def check(*arguments: str):
            return run(
                "check-macos-bundle-version.py",
                *["--tauri-config", self.root / "tauri.conf.json"],
                *["--source-plist", self.root / "Info.plist"],
                *["--bundle-plist", self.root / "packaged-Info.plist", *arguments],
            )

        self.assert_refused(check(), "expected macOS build version")
        self.assertEqual(check("--expected-bundle-version", "17").returncode, 0)
        mismatched = check("--expected-bundle-version", "18")
        self.assert_refused(mismatched, "expected macOS build version")

    def test_next_counter_advances_beyond_every_retained_release(self) -> None:
        metadata = self.root / "metadata"
        metadata.mkdir()

        def allocate(run_number: str):
            arguments = ["--release-metadata-directory", metadata]
            return run(
                "next-macos-bundle-version.py", "--run-number", run_number, *arguments
            )

        def retain(name: str, value: dict) -> None:
            (metadata / name).mkdir()
            (metadata / name / "RELEASE-METADATA.json").write_text(json.dumps(value))

        self.assertEqual(allocate("17").stdout, "17\n")
        retain("older-rc", {"schema_version": 1, "macos_bundle_version": "17"})
        retain("newer-stable", {"schema_version": 1, "macos_bundle_version": "53"})
        self.assertEqual(allocate("42").stdout, "54\n")
        for index, invalid in enumerate(
            (
                {"schema_version": 2, "macos_bundle_version": "1"},
                {"schema_version": 1, "macos_bundle_version": "01"},
            )
        ):
            with self.subTest(metadata=invalid):
                retain(f"invalid-{index}", invalid)
                self.assert_refused(allocate("17"), "release metadata")
                (metadata / f"invalid-{index}" / "RELEASE-METADATA.json").unlink()
                (metadata / f"invalid-{index}").rmdir()


if __name__ == "__main__":
    unittest.main()
