#!/usr/bin/env python3
"""Exercise the release asset assembly contract with disposable fixtures."""

from __future__ import annotations

import hashlib
import json
import os
import runpy
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from release_asset_contract import (  # noqa: E402
    canonical_asset_names,
    download_aliases,
    updater_artifact_names,
)

SCRIPTS = Path(__file__).resolve().parent
VERSION = "0.0.1"
TAG = f"v{VERSION}"
REPOSITORY = "loupistai/loupist"
COMMIT = "0123456789abcdef0123456789abcdef01234567"
RELEASE = ["--version", VERSION, "--tag", TAG, "--repository", REPOSITORY]
DMG = f"Loupist-{VERSION}-aarch64-apple-darwin.dmg"
UPDATER = f"Loupist-{VERSION}-darwin-aarch64.app.tar.gz"
QUALIFY = ["--candidate-tag", TAG, "--repository", REPOSITORY]
QUALIFY += ["--source-commit", COMMIT, "--release-id", "99", "--workflow-run-id", "100"]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path: Path, value: object) -> None:
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    )


class ReleaseAssetsContractTest(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.assets = self.root / "release-assets"

    def script(self, name: str, *arguments: str, ok: bool = True):
        completed = subprocess.run(
            [sys.executable, str(SCRIPTS / name), *map(str, arguments)],
            check=False,
            text=True,
            capture_output=True,
        )
        if ok:
            self.assertEqual(completed.returncode, 0, completed.stderr)
        return completed

    def refused(self, message: str, name: str, *arguments: str) -> None:
        completed = self.script(name, *arguments, ok=False)
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn(message, completed.stderr)

    def assemble(self) -> Path:
        self.assets.mkdir()
        for name, contents in {
            DMG: b"fixture dmg\n",
            UPDATER: b"fixture mac updater\n",
            f"{UPDATER}.sig": b"fixture mac signature\n",
            f"Loupist-{VERSION}.spdx.json": b'{"spdxVersion": "SPDX-2.3"}',
        }.items():
            (self.assets / name).write_bytes(contents)
        manifest = ["--output", self.assets / "latest.json"]
        self.script(
            "generate-update-manifest.py",
            *RELEASE,
            *["--pub-date", "2026-08-25T00:00:00Z", "--assets", self.assets],
            *manifest,
        )
        self.script(
            "finalize-release-assets.py",
            *RELEASE,
            *["--source-commit", COMMIT, "--workflow-run-id", "42"],
            *["--macos-bundle-version", "17", "--assets", self.assets],
        )
        return self.assets

    def rewrite_checksums(self) -> None:
        names = sorted(p.name for p in self.assets.iterdir() if p.name != "SHA256SUMS")
        sums = "".join(f"{sha256(self.assets / name)}  {name}\n" for name in names)
        (self.assets / "SHA256SUMS").write_text(sums)

    def test_shared_vocabulary_matches_the_published_asset_contract(self) -> None:
        spdx = f"Loupist-{VERSION}.spdx.json"
        expected = [DMG, UPDATER, f"{UPDATER}.sig", spdx, "latest.json"]
        self.assertEqual(canonical_asset_names(VERSION), expected)
        self.assertEqual(updater_artifact_names(VERSION), {"darwin-aarch64": UPDATER})
        aliases = {"Loupist-macOS-arm64.dmg": DMG}
        self.assertEqual(download_aliases(VERSION), aliases)

    def test_attestation_gate_accepts_the_assembled_set_only(self) -> None:
        assets = self.assemble()
        stub = self.root / "bin"
        stub.mkdir()
        gh = stub / "gh"
        gh.write_text("#!/bin/sh\nexit 0\n")
        gh.chmod(0o755)
        env = os.environ.copy()
        env["PATH"] = f"{stub}{os.pathsep}{env.get('PATH', '')}"

        def gate() -> subprocess.CompletedProcess[str]:
            return subprocess.run(
                [
                    "bash",
                    str(SCRIPTS / "verify-release-attestations.sh"),
                    str(assets),
                    REPOSITORY,
                    COMMIT,
                    f"refs/tags/{TAG}",
                ],
                check=False,
                capture_output=True,
                text=True,
                env=env,
            )

        accepted = gate()
        self.assertEqual(accepted.returncode, 0, accepted.stderr)
        extra = assets / "Loupist-extra.bin"
        extra.write_bytes(b"extra\n")
        rejected = gate()
        self.assertNotEqual(rejected.returncode, 0)
        self.assertIn("Closed release set", rejected.stderr)
        extra.unlink()
        self.assertEqual(gate().returncode, 0)
        latest = assets / "latest.json"
        displaced = assets / "latest-wrong.json"
        latest.rename(displaced)
        renamed = gate()
        self.assertNotEqual(renamed.returncode, 0)
        self.assertIn("Closed release set", renamed.stderr)
        displaced.rename(latest)
        self.assertEqual(gate().returncode, 0)

    def test_prepare_normalizes_macos_and_windows_bundle_outputs(self) -> None:
        bundles = {
            "macos": {
                "dmg/fixture.dmg": b"dmg",
                "macos/fixture.app.tar.gz": b"updater",
                "macos/fixture.app.tar.gz.sig": b"signature",
            },
            "windows": {
                "nsis/fixture-setup.exe": b"installer",
                "nsis/fixture-setup.exe.sig": b"signature",
            },
        }
        windows = f"Loupist-{VERSION}-x86_64-pc-windows-msvc-setup.exe"
        expected = {
            "macos": set(canonical_asset_names(VERSION)[:3]),
            "windows": {windows, f"{windows}.sig"},
        }
        for platform, files in bundles.items():
            bundle, output = self.root / f"{platform}-bundle", self.root / platform
            for name, contents in files.items():
                (bundle / name).parent.mkdir(parents=True, exist_ok=True)
                (bundle / name).write_bytes(contents)
            self.script(
                "prepare-release-assets.py",
                *["--platform", platform, "--version", VERSION],
                *["--bundle-root", bundle, "--output", output],
            )
            names = {path.name for path in output.iterdir()}
            self.assertEqual(names, expected[platform])

    def test_complete_candidate_verifies_and_serves_only_updater_assets(self) -> None:
        assets = self.assemble()
        self.script(
            "verify-release-assets.py",
            *RELEASE,
            "--source-commit",
            COMMIT,
            "--assets",
            assets,
        )
        manifest = json.loads((assets / "latest.json").read_text())
        self.assertEqual(manifest["version"], VERSION)
        self.assertEqual(set(manifest["platforms"]), {"darwin-aarch64"})
        signature = manifest["platforms"]["darwin-aarch64"]["signature"]
        self.assertEqual(signature, "fixture mac signature")
        metadata = json.loads((assets / "RELEASE-METADATA.json").read_text())
        self.assertEqual(metadata["source_commit"], COMMIT)
        self.assertEqual(metadata["macos_bundle_version"], "17")
        alias = (assets / "Loupist-macOS-arm64.dmg").read_bytes()
        self.assertEqual(alias, (assets / DMG).read_bytes())
        server = runpy.run_path(str(SCRIPTS / "serve-update-candidate.py"))
        prefix, resolved = f"/{REPOSITORY}/releases", assets.resolve()
        self.assertEqual(
            server["routes"](assets, REPOSITORY),
            {
                f"{prefix}/latest/download/latest.json": resolved / "latest.json",
                f"{prefix}/download/{TAG}/{UPDATER}": resolved / UPDATER,
            },
        )

    def test_verifier_rejects_changed_asset_sets_and_tampered_updater_urls(
        self,
    ) -> None:
        verify = ["verify-release-assets.py", *RELEASE, "--assets", self.assets]
        for mutation in ("missing macos signature", "extra windows installer"):
            with self.subTest(mutation):
                self.assemble()
                if mutation == "missing macos signature":
                    (self.assets / f"{UPDATER}.sig").unlink()
                else:
                    (self.assets / "Loupist-Windows-x64.exe").write_bytes(b"x")
                self.rewrite_checksums()
                self.refused("release asset set differs", *verify)
                for path in self.assets.iterdir():
                    path.unlink()
                self.assets.rmdir()
        self.assemble()
        manifest_path = self.assets / "latest.json"
        manifest = json.loads(manifest_path.read_text())
        url = "https://example.invalid/incorrect-updater.tar.gz"
        manifest["platforms"]["darwin-aarch64"]["url"] = url
        write_json(manifest_path, manifest)
        metadata_path = self.assets / "RELEASE-METADATA.json"
        metadata = json.loads(metadata_path.read_text())
        metadata["artifacts"]["latest.json"]["sha256"] = sha256(manifest_path)
        write_json(metadata_path, metadata)
        self.rewrite_checksums()
        message = "updater manifest URLs or signatures do not match the assets"
        self.refused(message, *verify, "--source-commit", COMMIT)

    def test_qualification_receipt_seals_the_verified_candidate_set(self) -> None:
        metadata = ["--release-metadata", self.assemble() / "RELEASE-METADATA.json"]
        receipt = self.root / "qualification.json"
        qualification = "release-qualification.py"
        digest = self.script(qualification, "digest", *metadata).stdout.strip()
        acceptance = "https://evidence.example.test/macos-product-acceptance"
        self.script(
            qualification,
            *["write", *QUALIFY, *metadata, "--previous-tag", "v0.0.1-rc.1"],
            *["--macos-evidence-url", "https://evidence.example.test/macos"],
            *["--macos-candidate-set-sha256", digest],
            *["--macos-product-acceptance-evidence-url", acceptance],
            *["--output", receipt],
        )
        sealed = json.loads(receipt.read_text())
        self.assertEqual(set(sealed["platforms"]), {"macos-aarch64"})
        self.assertEqual(
            sealed["product_acceptance"],
            {
                "macos-aarch64": {
                    "result": "passed",
                    "evidence_url": acceptance,
                    "candidate_set_sha256": digest,
                }
            },
        )
        verify = [qualification, "verify", *QUALIFY, *metadata, "--receipt", receipt]
        self.script(*verify)
        sealed["product_acceptance"]["macos-aarch64"]["candidate_set_sha256"] = "0" * 64
        write_json(receipt, sealed)
        self.refused("product acceptance candidate-set digest", *verify)

    def test_promotion_requires_the_candidate_tag_workflow_ref(self) -> None:
        workflow = SCRIPTS.parents[2] / ".github/workflows/promote-release.yml"
        text = workflow.read_text()
        self.assertIn('test "$GITHUB_REF_TYPE" = tag', text)
        self.assertIn('test "$GITHUB_REF_NAME" = "$CANDIDATE_TAG"', text)
        self.assertIn('git rev-parse "$GITHUB_SHA^{commit}"', text)

    def test_stable_qualification_sequence_requires_latest_after_first_release(
        self,
    ) -> None:
        def sequence(previous: str, latest: str) -> list[str]:
            return [
                "release-qualification.py",
                "sequence",
                "--candidate-tag",
                "v0.0.1",
            ] + [
                "--previous-tag",
                previous,
                "--latest-stable-tag",
                latest,
            ]

        self.script(*sequence("v0.0.1-rc.1", ""))
        self.script(*sequence("v0.0.0", "v0.0.0"))
        message = "must start from the current Latest release"
        self.refused(message, *sequence("v0.0.1-rc.1", "v0.0.0"))
        self.refused("must be strictly newer", *sequence("v0.0.0", "v0.0.1"))

    def test_release_candidates_must_form_a_contiguous_update_line(self) -> None:
        published = self.root / "published-tags.txt"

        def rc_sequence(candidate: str, tags: list[str]) -> list[str]:
            published.write_text("\n".join(tags) + "\n")
            return ["release-qualification.py", "rc-sequence"] + [
                "--candidate-tag",
                candidate,
                "--published-tags",
                str(published),
            ]

        self.script(*rc_sequence("v1.2.3-rc.1", []))
        self.script(*rc_sequence("v1.2.3-rc.2", ["v1.2.3-rc.1", "v9.9.9-rc.7"]))
        for message, candidate, tags in (
            ("expected rc.2", "v1.2.3-rc.3", ["v1.2.3-rc.1"]),
            ("must be contiguous", "v1.2.3-rc.4", ["v1.2.3-rc.1", "v1.2.3-rc.3"]),
            ("after its matching stable", "v1.2.3-rc.1", ["v1.2.3"]),
            ("canonical", "v1.2.3-rc.0", []),
        ):
            with self.subTest(message):
                self.refused(message, *rc_sequence(candidate, tags))


if __name__ == "__main__":
    unittest.main()
