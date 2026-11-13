#!/usr/bin/env python3
import base64
import hashlib
import os
import plistlib
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import macos_release_signing as signing
import release_credentials as credentials
from release_profile import load_profile, MODE_SECRETS, UPDATER_SECRETS


def result(stdout="", stderr="", code=0):
    return subprocess.CompletedProcess([], code, stdout, stderr)


class SigningTests(unittest.TestCase):
    def test_sidecar_arguments_and_main_last(self):
        for mode in ("developer-id", "self-signed"):
            identity = "A" * 40
            command = signing.sidecar_command(Path("/pi"), mode, identity, pi=True)
            self.assertEqual("--timestamp" in command, mode == "developer-id")
            self.assertEqual("runtime" in command, mode == "developer-id")
            self.assertEqual("--entitlements" in command, mode == "developer-id")
        root = Path("/runtime")
        for kind, executable in [("pi", "loupist-pi"), ("openbb", "loupist-openbb")]:
            with (
                patch.object(
                    signing,
                    "macho_files",
                    return_value=[root / executable, root / "library.node"],
                ),
                patch.object(signing, "run") as run,
            ):
                signing.sign_runtime(root, kind, "self-signed", "A" * 40)
                self.assertEqual(
                    run.call_args_list[-1].args[-1], str(root / executable)
                )
                self.assertTrue(
                    all(
                        "--options" not in call.args
                        and "--entitlements" not in call.args
                        for call in run.call_args_list
                    )
                )

    def test_strict_entitlements_runtime_and_signer_requirements(self):
        path = Path("/binary")
        for runtime, expected in [
            (True, {signing.WORKER_ENTITLEMENT: True}),
            (False, {}),
        ]:
            with (
                patch.object(
                    signing,
                    "details",
                    return_value="flags=0x10000(runtime)"
                    if runtime
                    else "flags=0x0(none)",
                ),
                patch.object(signing, "entitlements", return_value=expected),
            ):
                signing.require_flags(path, runtime, expected)
                with self.assertRaises(ValueError):
                    signing.require_flags(path, not runtime, expected)
        with (
            patch.object(signing, "details", return_value="flags=0x10000(runtime)"),
            patch.object(
                signing, "entitlements", return_value={signing.WORKER_ENTITLEMENT: True}
            ),
        ):
            with self.assertRaises(ValueError):
                signing.require_flags(path, True, {})
        with patch.object(
            signing, "run", return_value=result("TeamIdentifier=not set\n")
        ) as run:
            signing.verify_binary(path, "self-signed", "A" * 40, None)
            self.assertIn(
                '=certificate leaf = H"' + "A" * 40 + '"', run.call_args_list[0].args
            )
        with patch.object(
            signing,
            "run",
            return_value=result(stdout=plistlib.dumps({"extra": True}).decode()),
        ):
            self.assertEqual(signing.entitlements(path), {"extra": True})
            with patch.object(signing, "run", return_value=result()) as xml_call:
                signing.entitlements(path)
                self.assertIn("--xml", xml_call.call_args.args)
                self.assertIn("-", xml_call.call_args.args)
                self.assertNotIn(":-", xml_call.call_args.args)

    def test_all_runtime_files_are_verified_with_only_worker_hardened_for_beta(self):
        pi, worker = Path("/pi"), Path("/worker")
        files = [
            pi / "loupist-pi",
            pi / "native/addon.node",
            pi / "openbb/library.dylib",
        ]
        with (
            patch.object(signing, "macho_files", return_value=files),
            patch.object(signing, "verify_binary") as verify,
            patch.object(signing, "require_flags") as flags,
        ):
            signing.verify_runtime(pi, worker, "self-signed", "A" * 40)
            self.assertEqual(verify.call_count, 4)
            self.assertEqual(
                [call.args[1] for call in flags.call_args_list],
                [False, False, False, True],
            )
            self.assertEqual(
                flags.call_args_list[-1].args[2], {signing.WORKER_ENTITLEMENT: True}
            )

    def test_missing_ticket_is_positive_evidence_not_any_tool_error(self):
        with patch.object(
            signing,
            "run",
            return_value=result(
                stderr="does not have a ticket stapled to it.", code=65
            ),
        ):
            signing.require_ticket(Path("/image"), False)
            with self.assertRaises(ValueError):
                signing.require_ticket(Path("/image"), True)
        for response in [result(), result(stderr="network unavailable", code=1)]:
            with patch.object(signing, "run", return_value=response):
                with self.assertRaises(ValueError):
                    signing.require_ticket(Path("/image"), False)

    def test_mode_specific_credentials_and_no_apple_notary_environment_export(self):
        for mode in MODE_SECRETS:
            names = UPDATER_SECRETS | MODE_SECRETS[mode]
            with patch.dict(os.environ, {key: "fixture" for key in names}, clear=True):
                values = credentials.require(mode)
                self.assertFalse(
                    set(values)
                    & {
                        "APPLE_ID",
                        "APPLE_PASSWORD",
                        "APPLE_TEAM_ID",
                        "APPLE_CERTIFICATE",
                        "APPLE_API_KEY",
                        "APPLE_API_ISSUER",
                    }
                )
                os.environ.pop("LOUPIST_UPDATER_PUBLIC_KEY")
                with self.assertRaises(ValueError):
                    credentials.require(mode)

    def test_import_uses_untrusted_identity_listing_and_pins_both_hashes(self):
        leaf = b"fixture public DER"
        sha1, sha256 = (
            hashlib.sha1(leaf).hexdigest().upper(),
            hashlib.sha256(leaf).hexdigest().upper(),
        )
        pem = (
            "-----BEGIN CERTIFICATE-----\n"
            + base64.b64encode(leaf).decode()
            + "\n-----END CERTIFICATE-----\n"
        )
        profile = {
            "macos_signing": {
                "mode": "self-signed",
                "certificate_sha1": sha1,
                "certificate_sha256": sha256,
            }
        }

        def run(*args, **kwargs):
            if "import" in args:
                certificate = Path(args[2])
                self.assertEqual(certificate.stat().st_mode & 0o777, 0o600)
            if "find-identity" in args:
                return result(
                    f'  1) {sha1} "Identity"\n  1) {sha1.lower()} "Identity"\n'
                )
            return result(pem if "find-certificate" in args else "")

        with (
            tempfile.TemporaryDirectory() as directory,
            patch.dict(
                os.environ,
                {
                    "LOUPIST_CERTIFICATE": " \n"
                    + base64.b64encode(b"p12").decode()
                    + "\n",
                    "LOUPIST_CERTIFICATE_PASSWORD": "test",
                },
            ),
            patch.object(credentials, "export"),
            patch.object(credentials, "run", side_effect=run) as calls,
        ):
            values = credentials.import_certificate(
                Path(directory), profile, dry_run=False
            )
            self.assertEqual(values["APPLE_SIGNING_IDENTITY"], sha1)
            self.assertFalse((Path(directory) / "loupist-signing.p12").exists())
            identity_call = next(
                call.args
                for call in calls.call_args_list
                if "find-identity" in call.args
            )
            self.assertNotIn("-v", identity_call)
            self.assertTrue(
                any("list-keychain" in call.args for call in calls.call_args_list)
            )
            profile["macos_signing"]["certificate_sha256"] = "0" * 64
            with self.assertRaises(ValueError):
                credentials.import_certificate(Path(directory), profile, dry_run=False)
            self.assertIn(
                "LOUPIST_EPHEMERAL_CERTIFICATE_SHA1",
                credentials.import_certificate(Path(directory), profile, dry_run=True),
            )

    def test_dry_run_generator_uses_libressl_and_base64_roundtrip(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "ephemeral"

            def run(*args, **kwargs):
                if "req" in args:
                    (root / "certificate.pem").write_text("public certificate")
                if "pkcs12" in args:
                    (root / "certificate.p12").write_bytes(b"fixture p12")
                if "npm" in args:
                    (root / "updater.key").write_text("private fixture")
                    (root / "updater.key.pub").write_text("public fixture")
                return result()

            with patch.object(credentials, "run", side_effect=run) as calls:
                values = credentials.ephemeral(root)
            self.assertEqual(
                base64.b64decode(values["LOUPIST_CERTIFICATE"]), b"fixture p12"
            )
            self.assertEqual(values["LOUPIST_UPDATER_PUBLIC_KEY"], "public fixture")
            self.assertTrue(
                all(
                    call.args[0] == "/usr/bin/openssl"
                    for call in calls.call_args_list[:2]
                )
            )
            self.assertIn("--ci", calls.call_args_list[-1].args)

    def test_staging_shell_entrypoints_delegate_to_the_tested_signing_policy(self):
        # Security invariant: no inline shell signing path may bypass this policy.
        scripts = Path(__file__).resolve().parent
        for name, command in [
            ("stage-pi-macos-arm64.sh", "sign-runtime --kind pi"),
            ("stage-openbb-macos-arm64.sh", "sign-runtime --kind openbb"),
            ("check-package-prerequisites.sh", "verify-runtime"),
        ]:
            text = (scripts / name).read_text()
            self.assertIn('macos_release_signing.py" ' + command, text)
            self.assertNotIn("codesign --force", text)
        text = (scripts / "stage-openbb-macos-arm64.sh").read_text()
        self.assertIn(
            '"${LOUPIST_MACOS_SIGNING_MODE:-developer-id}" = developer-id', text
        )
        self.assertLess(
            text.index("materialize_bundle_symlinks.py"),
            text.index("sign-runtime --kind openbb"),
        )

    def test_bundle_records_developer_id_facts_strictly(self):
        app, dmg = Path("/Loupist.app"), Path("/fixture.dmg")
        main = app / "Contents/MacOS/loupist"
        worker = app / "Contents/Resources/python-runtime/loupist-python-worker"
        profile = load_profile()
        profile["macos_signing"] = {
            key: "developer-id" if key == "mode" else None
            for key in profile["macos_signing"]
        }
        requirement = 'identifier "com.monarchjuno.loupist" and anchor apple generic and certificate leaf[subject.OU] = ABCDE12345'
        with (
            patch.object(signing, "run", return_value=result()),
            patch.object(signing, "verify_runtime"),
            patch.object(signing, "verify_binary") as verify,
            patch.object(signing, "require_flags") as flags,
            patch.object(signing, "macho_files", return_value=[main, worker]),
            patch.object(signing, "require_ticket", return_value="ticket") as ticket,
            patch.object(signing, "certificate_hash", return_value="A" * 64),
            patch.object(signing, "designated_requirement", return_value=requirement),
        ):
            record = signing.verify_bundle(app, dmg, profile, team="ABCDE12345")
            self.assertEqual(verify.call_count, 3)
            self.assertEqual(record["mode"], "developer-id")
            self.assertIs(record["notarized"], True)
            self.assertEqual(record["certificate_sha256"], "A" * 64)
            self.assertEqual(ticket.call_count, 2)
            self.assertTrue(
                any(call.args == (main, True, {}) for call in flags.call_args_list)
            )

    def test_credential_command_failure_does_not_print_password_arguments(self):
        with patch.object(credentials.subprocess, "run", return_value=result(code=1)):
            with self.assertRaises(RuntimeError) as error:
                credentials.run("security", "import", "-P", "private-password-fixture")
        self.assertNotIn("private-password-fixture", str(error.exception))


if __name__ == "__main__":
    unittest.main()
