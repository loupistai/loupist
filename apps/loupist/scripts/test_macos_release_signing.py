#!/usr/bin/env python3
import base64
import hashlib
import json
import os
import plistlib
import contextlib
import io
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
    def test_report_comparison_identifies_changed_missing_and_typed_fields(self):
        expected = {
            "macos_signing": {
                "mode": "self-signed",
                "notarized": False,
                "certificate_sha256": "A" * 64,
            },
            "updater_public_key": "public",
        }
        actual = {
            "macos_signing": {
                "mode": "developer-id",
                "notarized": 0,
                "team_id": "TEAM",
            },
            "updater_public_key": "public",
        }
        fields = [
            "macos_signing.certificate_sha256",
            "macos_signing.mode",
            "macos_signing.notarized",
            "macos_signing.team_id",
        ]
        self.assertEqual(signing.report_differences(expected, actual), fields)
        with tempfile.TemporaryDirectory() as directory:
            original, mounted = [
                Path(directory) / name for name in ("built.json", "mounted.json")
            ]
            original.write_text(json.dumps(expected))
            mounted.write_text(json.dumps(actual))
            command = [
                os.sys.executable,
                str(Path(signing.__file__)),
                "compare-reports",
                "--expected",
                str(original),
                "--actual",
                str(mounted),
            ]
            mismatched = subprocess.run(command, capture_output=True, text=True)
            self.assertNotEqual(mismatched.returncode, 0)
            self.assertIn("Built and mounted signing reports differ", mismatched.stderr)
            for field in fields:
                self.assertIn("- " + field, mismatched.stderr)
            mounted.write_text(json.dumps(expected, indent=2, sort_keys=True))
            matched = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(matched.returncode, 0, matched.stderr)
            self.assertIn("reports match", matched.stdout)

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
            patch.object(signing, "external_libraries", return_value=[]),
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

    def test_staging_refuses_external_library_install_paths(self):
        pi, worker = Path("/runtime"), Path("/worker")
        with (
            patch.object(signing, "macho_files", return_value=[pi / "loupist-pi"]),
            patch.object(
                signing,
                "external_libraries",
                return_value=["/opt/build/libexample.dylib"],
            ),
        ):
            with self.assertRaisesRegex(ValueError, "/opt/build/libexample"):
                signing.verify_runtime(pi, worker, "developer-id", None, "ABCDE12345")

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
        leaf, chain = b"fixture public DER", b"fixture CA DER"
        # `security` reports Apple's SHA-1 identity identifier; the fixture only
        # needs it to be consistent across the identity and certificate listings.
        sha1, ca_sha1 = "A1" * 20, "C0" * 20
        sha256 = hashlib.sha256(leaf).hexdigest().upper()

        def entry(identifier, der, listed_sha256=None):
            return (
                (f"SHA-256 hash: {listed_sha256}\n" if listed_sha256 else "")
                + f"SHA-1 hash: {identifier}\n-----BEGIN CERTIFICATE-----\n"
                + base64.b64encode(der).decode()
                + "\n-----END CERTIFICATE-----\n"
            )

        ca = entry(ca_sha1, chain, hashlib.sha256(chain).hexdigest().upper())
        listing = {"value": entry(sha1, leaf, sha256) + ca}
        identities = {
            "value": f'  1) {sha1} "Identity"\n  1) {sha1.lower()} "Identity"\n'
        }
        profile = {
            "macos_signing": {
                "mode": "self-signed",
                "certificate_sha1": sha1,
                "certificate_sha256": sha256,
            }
        }

        def run(*args, **kwargs):
            if "import" in args:
                self.assertIn("-x", args)
                self.assertNotIn("-A", args)
                self.assertEqual(args[args.index("-T") + 1], "/usr/bin/codesign")
                self.assertEqual(args.count("-T"), 1)
                certificate = Path(args[2])
                self.assertEqual(certificate.stat().st_mode & 0o777, 0o600)
            if "x509" in args:
                return result("    organizationalUnitName    = ABCDE12345\n")
            if "find-identity" in args:
                return result(identities["value"])
            if "find-certificate" in args:
                self.assertIn("-Z", args)
                return result(listing["value"])
            return result()

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
            # A listing without SHA-256 lines still pins the computed SHA-256.
            listing["value"] = entry(sha1, leaf) + entry(ca_sha1, chain)
            self.assertEqual(
                credentials.import_certificate(Path(directory), profile, dry_run=False),
                {"APPLE_SIGNING_IDENTITY": sha1},
            )

            def refuses(message, *, dry_run=False):
                with self.assertRaisesRegex(ValueError, message):
                    credentials.import_certificate(
                        Path(directory), profile, dry_run=dry_run
                    )

            good_listing, good_identities = (
                entry(sha1, leaf, sha256) + ca,
                identities["value"],
            )
            for bad_identities in (
                "",
                f'  1) {sha1} "Identity"\n  2) {ca_sha1} "Other"\n',
            ):
                identities["value"] = bad_identities
                refuses("exactly one code-signing identity", dry_run=True)
            identities["value"] = good_identities
            for bad_listing, message in (
                ("", "certificate is absent"),
                (ca, "do not match"),
                (entry(sha1, leaf) + entry(sha1, chain), "do not match"),
                (entry(sha1, leaf, "0" * 64), "do not match"),
                (
                    "-----BEGIN CERTIFICATE-----\n"
                    + base64.b64encode(leaf).decode()
                    + "\n-----END CERTIFICATE-----\n",
                    "not recognized",
                ),
            ):
                listing["value"] = bad_listing
                refuses(message, dry_run=True)
            listing["value"] = good_listing
            profile["macos_signing"]["certificate_sha1"] = ca_sha1
            refuses("differ from release profile")
            profile["macos_signing"]["certificate_sha1"] = sha1
            profile["macos_signing"]["certificate_sha256"] = "0" * 64
            refuses("differ from release profile")
            self.assertIn(
                "LOUPIST_EPHEMERAL_CERTIFICATE_SHA1",
                credentials.import_certificate(Path(directory), profile, dry_run=True),
            )
            profile["macos_signing"].update(
                certificate_sha1=None, certificate_sha256=None
            )
            refuses("must be pinned")
            profile["macos_signing"].update(mode="developer-id", team_id="ABCDE12345")
            name = "Developer ID Application: Example (ABCDE12345)"
            identities["value"] = f'  1) {sha1} "{name}"\n'
            with patch.dict(
                os.environ,
                LOUPIST_APPLE_SIGNING_IDENTITY=name,
                LOUPIST_APPLE_TEAM_ID="ABCDE12345",
            ):
                self.assertEqual(
                    credentials.import_certificate(
                        Path(directory), profile, dry_run=False
                    ),
                    {"APPLE_SIGNING_IDENTITY": sha1},
                )
                os.environ["LOUPIST_APPLE_TEAM_ID"] = "OTHER12345"
                refuses("name and team")
                os.environ["LOUPIST_APPLE_TEAM_ID"] = "ABCDE12345"
                os.environ["LOUPIST_APPLE_SIGNING_IDENTITY"] = name.replace(
                    "Example", "Other"
                )
                refuses("name and team")

    def test_rehearsal_import_checks_the_disposable_leaf_and_intermediate_pins(self):
        with tempfile.TemporaryDirectory() as directory:
            keychain = Path(directory) / "temporary.keychain-db"

            def wrong_download(*args, **kwargs):
                if "curl" in args:
                    Path(args[args.index("--output") + 1]).write_bytes(
                        b"changed intermediate"
                    )
                return result()

            with patch.object(credentials, "run", side_effect=wrong_download):
                with self.assertRaisesRegex(
                    ValueError, "intermediate certificate fingerprint mismatch"
                ):
                    credentials.check_apple_chain(keychain, "A" * 40, "", dry_run=True)

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

    def test_tool_output_that_is_not_utf8_does_not_stop_a_check(self):
        # The first dry run died decoding `file -b` output with a 0xa9 byte.
        result = signing.run("/usr/bin/printf", "Mach-O \\251 universal")
        self.assertIn("Mach-O", result.stdout)
        self.assertIn("universal", result.stdout)

    def test_nested_bundles_are_signed_inside_out_after_materialization(self):
        root = Path("/runtime")
        framework = root / "Helper.app/Contents/Frameworks/Python.framework"
        main = root / "loupist-openbb"
        inner = framework / "Versions/3.12/Python"
        with (
            patch.object(signing, "macho_files", return_value=[main, inner]),
            patch.object(signing, "run") as run,
        ):
            signing.sign_runtime(root, "openbb", "developer-id", "fixture")
            self.assertEqual(
                [call.args[-1] for call in run.call_args_list],
                list(map(str, [inner, framework, root / "Helper.app", main])),
            )
            for call in run.call_args_list:
                self.assertIn("--timestamp", call.args)
                self.assertIn("runtime", call.args)
                self.assertNotIn("--entitlements", call.args)

    def test_rejected_dmg_prints_the_notary_log_even_on_nonzero_exit(self):
        for code in (0, 1):
            with (
                patch.dict(
                    os.environ,
                    LOUPIST_SIGNING_KEYCHAIN="/tmp/fixture.keychain",
                ),
                patch.object(
                    signing,
                    "run",
                    side_effect=[
                        result('{"id":"submission"}'),
                        result('{"status":"Invalid"}', code=code),
                        result("bad signature at path"),
                    ],
                ) as run,
                contextlib.redirect_stdout(io.StringIO()) as printed,
            ):
                with self.assertRaisesRegex(ValueError, "Invalid"):
                    signing.notarize(Path("test.dmg"), Path("test.dmg"))
                self.assertEqual(
                    run.call_args.args[:4], ("xcrun", "notarytool", "log", "submission")
                )
                self.assertIn("bad signature at path", printed.getvalue())
                self.assertNotIn("fixture", printed.getvalue())

    def test_timestamp_retries_only_timestamp_failures(self):
        command = ["codesign", "--timestamp", "fixture"]
        with (
            patch.object(signing.time, "sleep"),
            patch.object(
                signing,
                "run",
                side_effect=[RuntimeError("timestamp service unavailable"), result()],
            ) as run,
        ):
            signing.sign_code(command)
            self.assertEqual(run.call_count, 2)
        for message, count in [
            ("timestamp service unavailable", 3),
            ("invalid identity", 1),
        ]:
            with (
                patch.object(signing.time, "sleep"),
                patch.object(signing, "run", side_effect=RuntimeError(message)) as run,
                self.assertRaises(RuntimeError),
            ):
                try:
                    signing.sign_code(command)
                finally:
                    self.assertEqual(run.call_count, count)

    def test_developer_id_requires_a_secure_timestamp_and_expected_team(self):
        path = Path("/runtime/worker")
        for listing, accepted in [
            ("TeamIdentifier=ABCDE12345\nTimestamp=today", True),
            ("TeamIdentifier=OTHER12345\nTimestamp=today", False),
            ("TeamIdentifier=ABCDE12345\nSigned Time=today", False),
        ]:
            with patch.object(
                signing, "run", return_value=result(stderr=listing)
            ) as run:
                if accepted:
                    signing.verify_binary(path, "developer-id", None, "ABCDE12345")
                    requirement = run.call_args_list[0].args[-2]
                    self.assertTrue(
                        requirement.startswith("=anchor apple generic and ")
                    )
                    self.assertIn("1.2.840.113635.100.6.1.13", requirement)
                else:
                    with self.assertRaises(ValueError):
                        signing.verify_binary(path, "developer-id", None, "ABCDE12345")

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
        self.assertLess(
            text.index("materialize_bundle_symlinks.py"),
            text.index("sign-runtime --kind openbb"),
        )

    def test_bundle_records_developer_id_facts_strictly(self):
        app, dmg = Path("/Loupist.app"), Path("/fixture.dmg")
        main = app / "Contents/MacOS/loupist"
        worker = app / "Contents/Resources/python-runtime/loupist-python-worker"
        profile = load_profile(rehearsal=True)
        profile["macos_signing"] = {
            key: "developer-id" if key == "mode" else None
            for key in profile["macos_signing"]
        }
        profile["macos_signing"]["team_id"] = "ABCDE12345"
        requirement = 'identifier "com.monarchjuno.loupist" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */ and certificate leaf[field.1.2.840.113635.100.6.1.13] /* exists */ and certificate leaf[subject.OU] = ABCDE12345'
        with (
            patch.object(signing, "run", return_value=result()),
            patch.object(signing, "verify_runtime"),
            patch.object(signing, "verify_binary") as verify,
            patch.object(signing, "require_flags") as flags,
            patch.object(
                signing,
                "macho_files",
                return_value=[
                    main,
                    worker,
                    app / "Contents/Resources/pi-runtime/loupist-pi",
                    app / "Contents/Resources/pi-runtime/openbb/loupist-openbb",
                ],
            ),
            patch.object(signing, "require_ticket", return_value="ticket") as ticket,
            patch.object(signing, "certificate_hash", return_value="A" * 64),
            patch.object(signing, "designated_requirement", return_value=requirement),
        ):
            record = signing.verify_bundle(app, dmg, profile, team="ABCDE12345")
            self.assertEqual(verify.call_count, 5)
            self.assertEqual(record["mode"], "developer-id")
            self.assertIs(record["notarized"], True)
            self.assertEqual(record["certificate_sha256"], "A" * 64)
            self.assertEqual(ticket.call_count, 2)
            self.assertTrue(
                any(call.args == (main, True, {}) for call in flags.call_args_list)
            )

    def test_credential_command_failure_does_not_print_password_arguments(self):
        message = "permission denied: secret-argument secret-input secret-child"
        with (
            patch.object(
                credentials.subprocess,
                "run",
                return_value=result(stderr=message, code=1),
            ),
            contextlib.redirect_stderr(io.StringIO()) as printed,
        ):
            with self.assertRaises(RuntimeError) as error:
                credentials.run(
                    "security",
                    "import",
                    "-P",
                    "secret-argument",
                    input="secret-input\n",
                    env={"TAURI_SIGNING_PRIVATE_KEY": "secret-child"},
                )
        self.assertEqual(
            printed.getvalue(), "permission denied: [REDACTED] [REDACTED] [REDACTED]\n"
        )
        self.assertNotIn("secret-", str(error.exception))


if __name__ == "__main__":
    unittest.main()
