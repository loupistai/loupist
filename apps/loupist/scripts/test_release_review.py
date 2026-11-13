#!/usr/bin/env python3
"""Regression cases for release review boundaries and failed-job reruns."""

import contextlib
import io
import os
from pathlib import Path
import re
import runpy
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import release_credentials as credentials

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[2]


class ReviewTests(unittest.TestCase):
    def test_private_material_cannot_enter_github_env_or_masks(self):
        with tempfile.TemporaryDirectory() as directory:
            env_file = Path(directory) / "github-env"
            with (
                patch.dict(os.environ, {"GITHUB_ENV": str(env_file)}),
                contextlib.redirect_stdout(io.StringIO()) as printed,
            ):
                for name in (
                    "TAURI_SIGNING_PRIVATE_KEY",
                    "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
                    "LOUPIST_CERTIFICATE",
                    "LOUPIST_CERTIFICATE_PASSWORD",
                ):
                    with self.assertRaises(ValueError):
                        credentials.export({name: "private-fixture"})
                credentials.export({"APPLE_SIGNING_IDENTITY": "-"})
            self.assertNotIn("private-fixture", env_file.read_text())
            self.assertNotIn("::add-mask::-", printed.getvalue())

    def test_disposable_material_is_private_and_scoped_to_build(self):
        values = {
            "LOUPIST_CERTIFICATE": "p12-fixture",
            "LOUPIST_CERTIFICATE_PASSWORD": "p12-password",
            "TAURI_SIGNING_PRIVATE_KEY": "updater-private-fixture",
            "TAURI_SIGNING_PRIVATE_KEY_PASSWORD": "updater-password",
            "LOUPIST_UPDATER_PUBLIC_KEY": "public-fixture",
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)

            def generate(path):
                path.mkdir(mode=0o700)
                for name in ("certificate.key", "certificate.p12", "updater.key"):
                    (path / name).write_text("private")
                return values

            with (
                patch.dict(
                    os.environ,
                    {
                        "GITHUB_ENV": str(root / "github-env"),
                        "LOUPIST_TAURI_RELEASE_CONFIG": "public-config",
                    },
                    clear=True,
                ),
                patch.object(credentials, "ephemeral", side_effect=generate),
            ):
                credentials.persist_ephemeral(root)
                handoff = credentials.ephemeral_file(root)
                self.assertEqual(handoff.stat().st_mode & 0o777, 0o600)
                self.assertNotIn("private-fixture", (root / "github-env").read_text())
                self.assertFalse((handoff.parent / "certificate.p12").exists())
                with patch.object(credentials, "run") as build:
                    credentials.build(root, dry_run=True)
                environment = build.call_args.kwargs["env"]
                self.assertEqual(
                    environment["TAURI_SIGNING_PRIVATE_KEY"],
                    values["TAURI_SIGNING_PRIVATE_KEY"],
                )
                self.assertNotIn("LOUPIST_CERTIFICATE", environment)
                self.assertNotIn("LOUPIST_CERTIFICATE_PASSWORD", environment)
                self.assertNotIn("TAURI_SIGNING_PRIVATE_KEY", os.environ)
                handoff.chmod(0o644)
                with self.assertRaises(ValueError):
                    credentials.load_ephemeral(root)

    def test_rc_failed_job_rerun_rechecks_sequence_before_publication(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        publication = workflow.split(
            "      - name: Publish RC or stage stable draft\n", 1
        )[1]
        body = publication.split("        run: |\n", 1)[1]
        body = "\n".join(line[10:] for line in body.splitlines())
        body = re.sub(r"\$\{\{.*?\}\}", "v1.2.3-rc.2", body)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "bin"
            binary.mkdir()
            gh = binary / "gh"
            git = binary / "git"
            gh.write_text(
                '#!/bin/sh\ncase "$1 $2" in\n"release view") exit 1;;\n"api --paginate") printf "v1.2.3-rc.1\\nv1.2.3-rc.3\\n";;\n"release create") touch "$RUNNER_TEMP/published";;\n*) exit 99;;\nesac\n'
            )
            git.write_text(
                '#!/bin/sh\nprintf "v1.2.3-rc.1\\nv1.2.3-rc.2\\nv1.2.3-rc.3\\n"\n'
            )
            gh.chmod(0o755)
            git.chmod(0o755)
            environment = dict(
                os.environ,
                PATH=str(binary) + os.pathsep + os.environ["PATH"],
                RUNNER_TEMP=str(root),
                GITHUB_REPOSITORY="loupistai/loupist",
                GITHUB_REF_NAME="v1.2.3-rc.2",
                RELEASE_VERSION="1.2.3-rc.2",
                PRERELEASE="true",
                RELEASE_CHANNEL="beta",
            )
            result = subprocess.run(
                ["bash", "-c", body],
                cwd=ROOT,
                env=environment,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("above every published RC", result.stderr)
            self.assertFalse((root / "published").exists())

    def test_beta_omissions_require_whole_disclosure_line_and_cannot_be_empty(self):
        module = runpy.run_path(str(SCRIPTS / "release-qualification.py"))
        validate = module["validate_methods"]
        normalize = module["normalized_disclosure"]
        with self.assertRaises(RuntimeError):
            validate(
                "beta", "candidate-on-isolated-machine", "clean-isolated-machine", ""
            )
        with self.assertRaises(RuntimeError):
            validate(
                "beta",
                "candidate-on-isolated-machine",
                "clean-isolated-machine",
                "- Not checked:",
            )
        self.assertEqual(
            normalize("- Not checked:  Candidate update  was omitted. "),
            normalize("Candidate update was omitted."),
        )
        self.assertNotEqual(normalize("a"), normalize("Beta candidate accepted"))
        self.assertNotEqual(
            normalize("Candidate update was omitted."),
            normalize("Only part: Candidate update was omitted."),
        )

    def test_release_guards_use_mounted_app_and_real_host_entrypoint(self):
        # Security invariants: release preflight must reach the app's release-only gate.
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertIn('--app "$MOUNTED_APP" --dmg "$DMG"', workflow)
        self.assertIn("--require-distribution-trust", workflow)
        self.assertIn("'true' && 1 || 14", workflow)
        retentions = re.findall(r"retention-days: (.+)", workflow)
        self.assertEqual(len(retentions), 3)
        self.assertTrue(all("'true' && 1 || 14" in value for value in retentions))
        self.assertIn(
            "cmp -s signing-report.json mounted-signing-report.json", workflow
        )
        checker = (SCRIPTS / "check-macos-app-bundle.sh").read_text()
        self.assertIn('"$MACOS_DIR/loupist" --check-bundled-helpers', checker)
        qualification = (
            ROOT / ".github/workflows/release-qualification.yml"
        ).read_text()
        self.assertIn(
            '"$APP/Contents/MacOS/loupist" --check-bundled-helpers', qualification
        )
        source = (
            (ROOT / "apps/loupist/src-tauri/src/lib.rs")
            .read_text()
            .split("pub fn run() {", 1)[1]
        )
        self.assertLess(
            source.index("bundle_self_test::requested"),
            source.index("tauri::generate_context!"),
        )
        for name in (
            "Build and sign unnotarized Loupist",
            "Build, sign, and notarize Loupist with Developer ID",
        ):
            step = workflow.split("      - name: " + name + "\n", 1)[1].split(
                "\n      - ", 1
            )[0]
            self.assertIn(
                "TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.LOUPIST_TAURI_SIGNING_PRIVATE_KEY }}",
                step,
            )
        for name in (
            "Stage and sign sidecars and OpenBB runtime",
            "Configure embedded updater",
            "Verify signatures, notarization, and updater artifacts",
        ):
            step = workflow.split("      - name: " + name + "\n", 1)[1].split(
                "\n      - ", 1
            )[0]
            self.assertNotIn("TAURI_SIGNING_PRIVATE_KEY:", step)
            self.assertNotIn("LOUPIST_CERTIFICATE:", step)


if __name__ == "__main__":
    unittest.main()
