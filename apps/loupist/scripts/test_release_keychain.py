#!/usr/bin/env python3
"""Exercise the production key import with disposable macOS signatures only."""

import base64
import hashlib
import os
from pathlib import Path
import plistlib
import re
import shutil
import ssl
import subprocess
import sys
import tempfile
import unittest

import macos_release_signing as signing
import release_credentials as credentials
import stage_python_runtime

TOOLS = (
    "/usr/bin/security",
    "/usr/bin/codesign",
    "/usr/bin/openssl",
    "/usr/bin/clang",
    "/usr/bin/hdiutil",
)


def fixture(root: Path) -> None:
    keychain = root / "loupist-signing.keychain-db"
    try:
        config = root / "certificate.cnf"
        config.write_text("""[req]
distinguished_name=dn
x509_extensions=ext
prompt=no
[dn]
CN=Loupist Nonextractable Fixture
O=Loupist
[ext]
basicConstraints=critical,CA:false
keyUsage=critical,digitalSignature
extendedKeyUsage=critical,codeSigning
subjectKeyIdentifier=hash
""")
        key, certificate, p12 = [
            root / name for name in ("private.pem", "certificate.pem", "fixture.p12")
        ]
        credentials.run(
            "/usr/bin/openssl",
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-sha256",
            "-days",
            "2",
            "-nodes",
            "-keyout",
            key,
            "-out",
            certificate,
            "-config",
            config,
        )
        credentials.run(
            "/usr/bin/openssl",
            "pkcs12",
            "-export",
            "-inkey",
            key,
            "-in",
            certificate,
            "-out",
            p12,
            "-passout",
            "pass:fixture",
        )
        der = ssl.PEM_cert_to_DER_cert(certificate.read_text())
        # Apple's SHA-1 identity identifier comes from the X.509 tool, not Python.
        fingerprint = credentials.run(
            "/usr/bin/openssl",
            "x509",
            "-in",
            certificate,
            "-noout",
            "-fingerprint",
            "-sha1",
        ).stdout
        match = re.fullmatch(
            r"SHA1 Fingerprint=((?:[0-9A-F]{2}:){19}[0-9A-F]{2})\s*",
            fingerprint,
            re.IGNORECASE,
        )
        assert match, "openssl did not report a SHA-1 fingerprint"
        sha1 = match.group(1).replace(":", "").upper()
        profile = {
            "macos_signing": {
                "mode": "self-signed",
                "certificate_sha1": sha1,
                "certificate_sha256": hashlib.sha256(der).hexdigest().upper(),
            }
        }
        os.environ.update(
            LOUPIST_CERTIFICATE=base64.b64encode(p12.read_bytes()).decode(),
            LOUPIST_CERTIFICATE_PASSWORD="fixture",
            GITHUB_ENV=str(root / "github-env"),
        )
        imported = credentials.import_certificate(root, profile, dry_run=False)
        assert imported["APPLE_SIGNING_IDENTITY"] == sha1
        assert not (root / "loupist-signing.p12").exists()
        os.environ.update(
            APPLE_SIGNING_IDENTITY=sha1, LOUPIST_MACOS_SIGNING_MODE="self-signed"
        )

        source = root / "fixture.c"
        source.write_text("int main(void) { return 0; }\n")
        binary = root / "fixture"
        credentials.run("/usr/bin/clang", source, "-o", binary)
        payload = root / "payload"
        app = payload / "Loupist.app"
        main = app / "Contents/MacOS/loupist"
        pi = app / "Contents/Resources/pi-runtime"
        openbb = pi / "openbb"
        worker = app / "Contents/Resources/python-runtime/loupist-python-worker"
        for path in (main, pi / "loupist-pi", worker):
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(binary, path)
        (app / "Contents/Info.plist").write_bytes(
            plistlib.dumps(
                {
                    "CFBundleIdentifier": "com.monarchjuno.loupist",
                    "CFBundleExecutable": "loupist",
                    "CFBundlePackageType": "APPL",
                    "CFBundleVersion": "1",
                    "CFBundleShortVersionString": "0.1.0",
                }
            )
        )
        signing.sign_runtime(pi, "pi", "self-signed", sha1)
        openbb.mkdir()
        shutil.copy2(binary, openbb / "loupist-openbb")
        signing.sign_runtime(openbb, "openbb", "self-signed", sha1)
        signing.run(*stage_python_runtime.codesign_command(worker))
        signing.run(
            "/usr/bin/codesign",
            "--force",
            "--sign",
            sha1,
            "--options",
            "runtime",
            str(app),
        )
        signing.verify_runtime(pi, worker, "self-signed", sha1)
        signing.require_flags(main, True, {})
        signing.run("/usr/bin/codesign", "--verify", "--deep", "--strict", str(app))
        dmg = root / "fixture.dmg"
        credentials.run(
            "/usr/bin/hdiutil",
            "create",
            "-volname",
            "LoupistFixture",
            "-srcfolder",
            payload,
            "-ov",
            "-format",
            "UDZO",
            dmg,
        )
        signing.run("/usr/bin/codesign", "--force", "--sign", sha1, str(dmg))
        signing.verify_binary(dmg, "self-signed", sha1, None)
        # -P is an archive-wrapping password, not the keychain password. No
        # keychain password or interactive input is provided to the export.
        exported = subprocess.run(
            [
                "/usr/bin/security",
                "export",
                "-k",
                str(keychain),
                "-t",
                "identities",
                "-f",
                "pkcs12",
                "-P",
                "fixture-export",
            ],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=20,
        )
        assert exported.returncode != 0, "private signing key was exportable"
        assert not exported.stdout, "export emitted private material"
        diagnostic = exported.stderr.decode(errors="replace").lower()
        assert (
            "cannot be retrieved" in diagnostic
            or "not extractable" in diagnostic
            or "not exportable" in diagnostic
        ), "export failed for an unexpected reason"
        print(
            "PASS: SHA-1 app, Pi, OpenBB, worker and DMG signatures; private-key export refused"
        )
    finally:
        if keychain.exists():
            credentials.run("/usr/bin/security", "delete-keychain", keychain)


class NonextractableKeyTests(unittest.TestCase):
    @unittest.skipUnless(
        sys.platform == "darwin" and all(Path(tool).is_file() for tool in TOOLS),
        "requires macOS signing tools and clang",
    )
    def test_signing_works_and_private_key_export_is_refused(self):
        with tempfile.TemporaryDirectory(prefix="loupist-keychain-test-") as directory:
            root = Path(directory)
            home = root / "home"
            (home / "Library/Preferences").mkdir(parents=True)
            (home / "Library/Keychains").mkdir()
            environment = dict(
                os.environ,
                HOME=str(home),
                CFFIXED_USER_HOME=str(home),
                PATH="/usr/bin:/bin:/usr/sbin:/sbin",
                LANG="C",
                LC_ALL="C",
            )
            environment.pop("GITHUB_STEP_SUMMARY", None)
            result = subprocess.run(
                [
                    sys.executable,
                    str(Path(__file__).resolve()),
                    "--fixture-root",
                    str(root),
                ],
                env=environment,
                capture_output=True,
                text=True,
                timeout=120,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("private-key export refused", result.stdout)


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--fixture-root":
        fixture(Path(sys.argv[2]))
    else:
        unittest.main()
