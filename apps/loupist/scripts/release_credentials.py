#!/usr/bin/env python3
"""Require mode-specific credentials, or generate disposable dry-run keys, then import."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import re
import secrets
import subprocess
import sys
from pathlib import Path

from release_profile import (
    ALL_SECRETS,
    MODE_SECRETS,
    UPDATER_SECRETS,
    check_team,
    load_profile,
)


def run(*args: str, **kwargs) -> subprocess.CompletedProcess:
    capture = kwargs.pop("capture_output", True)
    result = subprocess.run(
        list(map(str, args)),
        check=False,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        **kwargs,
    )
    if result.stderr and (result.returncode or not capture):
        # Include disposable passwords too: these can come from stdin, an
        # OpenSSL pass: argument, the keychain CLI or the child-only environment.
        names = ALL_SECRETS | {
            "APPLE_ID",
            "APPLE_PASSWORD",
            "LOUPIST_CERTIFICATE",
            "LOUPIST_CERTIFICATE_PASSWORD",
            "TAURI_SIGNING_PRIVATE_KEY",
            "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
        }
        env = kwargs.get("env", os.environ)
        private = [env.get(name, "") for name in names]
        private.append(kwargs.get("input", "").rstrip("\n"))
        flags = {"-p", "-P", "-passout"}
        if args[:2] == ("security", "set-key-partition-list"):
            flags.add("-k")
        for flag, value in zip(args, args[1:]):
            if flag in flags:
                private.append(
                    str(value).removeprefix("pass:")
                    if flag == "-passout"
                    else str(value)
                )
        diagnostic = result.stderr
        for value in sorted(
            {line for value in private for line in value.splitlines() if line},
            key=len,
            reverse=True,
        ):
            diagnostic = diagnostic.replace(value, "[REDACTED]")
        print(diagnostic.rstrip(), file=sys.stderr)
    if result.returncode:
        raise RuntimeError(f"{args[0]} failed during release credential setup")
    return result


PUBLIC_EXPORTS = {
    "LOUPIST_UPDATER_PUBLIC_KEY",
    "APPLE_SIGNING_IDENTITY",
    "LOUPIST_SIGNING_KEYCHAIN",
    "LOUPIST_EPHEMERAL_CERTIFICATE_SHA1",
}


def export(values: dict[str, str]) -> None:
    if set(values) - PUBLIC_EXPORTS:
        raise ValueError("private release material must not be exported to GITHUB_ENV")
    with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as output:
        for key, value in values.items():
            delimiter = "LOUPIST_" + secrets.token_hex(16)
            output.write(f"{key}<<{delimiter}\n{value}\n{delimiter}\n")


def require(mode: str) -> dict[str, str]:
    required = UPDATER_SECRETS | MODE_SECRETS[mode]
    missing = sorted(name for name in required if not os.environ.get(name))
    if missing:
        raise ValueError("Required release credential is absent: " + ", ".join(missing))
    return {"LOUPIST_UPDATER_PUBLIC_KEY": os.environ["LOUPIST_UPDATER_PUBLIC_KEY"]}


DRY_TEAM = "DRYRUN1234"
DRY_IDENTITY = f"Loupist Dry Run Ephemeral ({DRY_TEAM})"


def ephemeral(root: Path) -> dict[str, str]:
    root.mkdir(mode=0o700)
    config = root / "codesign.cnf"
    config.write_text(f"""[req]
distinguished_name=dn
x509_extensions=ext
prompt=no
[dn]
CN={DRY_IDENTITY}
OU={DRY_TEAM}
O=Loupist
[ext]
basicConstraints=critical,CA:false
keyUsage=critical,digitalSignature
extendedKeyUsage=critical,codeSigning
subjectKeyIdentifier=hash
""")
    certificate, key, p12 = [
        root / name
        for name in ("certificate.pem", "certificate.key", "certificate.p12")
    ]
    password = secrets.token_hex(30)
    run(
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
    run(
        "/usr/bin/openssl",
        "pkcs12",
        "-export",
        "-inkey",
        key,
        "-in",
        certificate,
        "-name",
        DRY_IDENTITY,
        "-out",
        p12,
        "-passout",
        "pass:" + password,
    )
    updater = root / "updater.key"
    updater_password = secrets.token_hex(30)
    run(
        "npm",
        "exec",
        "--",
        "tauri",
        "signer",
        "generate",
        "--ci",
        "-p",
        updater_password,
        "-w",
        updater,
        cwd=Path(__file__).resolve().parents[1],
    )
    return {
        "LOUPIST_CERTIFICATE": base64.b64encode(p12.read_bytes()).decode(),
        "LOUPIST_CERTIFICATE_PASSWORD": password,
        "TAURI_SIGNING_PRIVATE_KEY": updater.read_text().strip(),
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD": updater_password,
        "LOUPIST_UPDATER_PUBLIC_KEY": updater.with_suffix(".key.pub")
        .read_text()
        .strip(),
    }


def check_apple_chain(
    keychain: Path, sha1: str, listing: str, *, dry_run: bool
) -> None:
    def valid() -> bool:
        identities = run(
            "security", "find-identity", "-v", "-p", "codesigning", keychain
        ).stdout
        return bool(
            re.search(rf'^\s*\d+\) {sha1} "', identities, re.MULTILINE | re.IGNORECASE)
        )

    if valid() and not dry_run:
        return
    # A .p12 can contain only the leaf and private key. Supply the public
    # intermediates in the job keychain without changing any trust settings.
    for name, digest in {
        "DeveloperIDCA": "7afc9d01a62f03a2de9637936d4afe68090d2de18d03f29c88cfb0b1ba63587f",
        "DeveloperIDG2CA": "f16cd3c54c7f83cea4bf1a3e6a0819c8aaa8e4a1528fd144715f350643d2df3a",
    }.items():
        if f"SHA-256 hash: {digest}".lower() in listing.lower():
            continue
        public_cert = keychain.with_name(name + ".cer")
        run(
            "curl",
            "--fail",
            "--silent",
            "--show-error",
            "--retry",
            "3",
            "--output",
            public_cert,
            f"https://www.apple.com/certificateauthority/{name}.cer",
        )
        if hashlib.sha256(public_cert.read_bytes()).hexdigest() != digest:
            raise ValueError(
                "Apple Developer ID intermediate certificate fingerprint mismatch"
            )
        run("security", "add-certificates", "-k", keychain, public_cert)
        if not dry_run and valid():
            return
    if dry_run:
        if valid():
            raise ValueError(
                "disposable rehearsal certificate unexpectedly has a trusted chain"
            )
        return
    raise ValueError(
        "imported Developer ID identity is invalid or its Apple chain is incomplete"
    )


def import_certificate(root: Path, profile: dict, *, dry_run: bool) -> dict[str, str]:
    certificate, keychain = (
        root / "loupist-signing.p12",
        root / "loupist-signing.keychain-db",
    )
    encoded = re.sub(r"\s", "", os.environ["LOUPIST_CERTIFICATE"])
    decoded = base64.b64decode(encoded, validate=True)
    with open(
        certificate, "xb", opener=lambda name, flags: os.open(name, flags, 0o600)
    ) as output:
        output.write(decoded)
    password = secrets.token_hex(30)
    run("security", "create-keychain", "-p", password, keychain)
    # Export immediately so the workflow's always() cleanup also handles a failed import.
    export({"LOUPIST_SIGNING_KEYCHAIN": str(keychain)})
    run("security", "set-keychain-settings", "-lut", "21600", keychain)
    run("security", "unlock-keychain", "-p", password, keychain)
    try:
        # Tauri and all app/helper/DMG signing use codesign. Never make the
        # imported key extractable or add every application to its import ACL.
        run(
            "security",
            "import",
            certificate,
            "-P",
            os.environ["LOUPIST_CERTIFICATE_PASSWORD"],
            "-x",
            "-T",
            "/usr/bin/codesign",
            "-t",
            "cert",
            "-f",
            "pkcs12",
            "-k",
            keychain,
        )
    finally:
        certificate.unlink(missing_ok=True)
    run(
        "security",
        "set-key-partition-list",
        "-S",
        "apple-tool:,apple:,codesign:",
        "-s",
        "-k",
        password,
        keychain,
    )
    run("security", "list-keychain", "-d", "user", "-s", keychain)
    identities = run("security", "find-identity", "-p", "codesigning", keychain).stdout
    hashes = sorted(
        {
            value.upper()
            for value in re.findall(
                r"^\s*\d+\) ([A-Fa-f0-9]{40}) ", identities, re.MULTILINE
            )
        }
    )
    if len(hashes) != 1:
        raise ValueError(
            "signing keychain must contain exactly one code-signing identity"
        )
    # -Z prints each certificate's SHA-256 and SHA-1 before its PEM. The SHA-1 is
    # Apple's identity identifier (find-identity, codesign); it is taken from the
    # tool, never computed here. The SHA-256 computed below is the security pin.
    listing = run("security", "find-certificate", "-a", "-Z", "-p", keychain).stdout
    if listing.count("-----BEGIN CERTIFICATE-----") < 1:
        raise ValueError("imported code-signing certificate is absent")
    certificates = re.findall(
        r"^(?:SHA-256 hash: ([A-Fa-f0-9]{64})\s*\n)?SHA-1 hash: ([A-Fa-f0-9]{40})\s*\n"
        r"-----BEGIN CERTIFICATE-----\s*(.*?)\s*-----END CERTIFICATE-----",
        listing,
        re.MULTILINE | re.DOTALL,
    )
    if len(certificates) != listing.count("-----BEGIN CERTIFICATE-----"):
        raise ValueError("imported certificate listing is not recognized")
    # Select the leaf matching the identity, not a CA certificate in a Developer ID chain.
    leaves = [
        (listed_sha256, base64.b64decode(re.sub(r"\s", "", body), validate=True))
        for listed_sha256, listed_sha1, body in certificates
        if listed_sha1.upper() == hashes[0]
    ]
    if len(leaves) != 1:
        raise ValueError("identity and imported certificate do not match")
    listed_sha256, leaf = leaves[0]
    sha1, sha256 = hashes[0], hashlib.sha256(leaf).hexdigest().upper()
    if listed_sha256 and listed_sha256.upper() != sha256:
        raise ValueError("identity and imported certificate do not match")
    if profile["macos_signing"]["mode"] == "developer-id":
        # security reports the common name next to the fingerprint of the
        # certificate whose private key it located in the temporary keychain.
        names = {
            name
            for fingerprint, name in re.findall(
                r'^\s*\d+\) ([A-Fa-f0-9]{40}) "([^"\n]+)"', identities, re.MULTILINE
            )
            if fingerprint.upper() == sha1
        }
        expected = (
            DRY_IDENTITY if dry_run else os.environ["LOUPIST_APPLE_SIGNING_IDENTITY"]
        )
        team = DRY_TEAM if dry_run else check_team(profile["macos_signing"]["team_id"])
        # Match the actual leaf's OU independently of the human-readable name.
        pem = (
            "-----BEGIN CERTIFICATE-----\n"
            + base64.encodebytes(leaf).decode()
            + "-----END CERTIFICATE-----\n"
        )
        subject = run(
            "/usr/bin/openssl",
            "x509",
            "-subject",
            "-nameopt",
            "multiline",
            "-noout",
            input=pem,
        ).stdout
        units = re.findall(
            r"^\s*organizationalUnitName\s*= (.+)$", subject, re.MULTILINE
        )
        if (
            names != {expected}
            or units != [team]
            or (
                not dry_run
                and (
                    not expected.startswith("Developer ID Application: ")
                    or team != os.environ["LOUPIST_APPLE_TEAM_ID"]
                )
            )
            or not expected.endswith(f" ({team})")
        ):
            raise ValueError(
                "imported identity does not match the configured Developer ID Application name and team"
            )
        check_apple_chain(keychain, sha1, listing, dry_run=dry_run)
    if not dry_run:
        policy = profile["macos_signing"]
        if policy["certificate_sha1"] and (
            sha1 != policy["certificate_sha1"] or sha256 != policy["certificate_sha256"]
        ):
            raise ValueError(
                "imported certificate fingerprints differ from release profile"
            )
        if policy["mode"] == "self-signed" and not policy["certificate_sha1"]:
            raise ValueError("self-signed certificate must be pinned")
    return {
        "APPLE_SIGNING_IDENTITY": sha1,
        **({"LOUPIST_EPHEMERAL_CERTIFICATE_SHA1": sha1} if dry_run else {}),
    }


def ephemeral_file(root: Path) -> Path:
    return root / "loupist-ephemeral/credentials.json"


def persist_ephemeral(root: Path) -> None:
    directory = root / "loupist-ephemeral"
    values = ephemeral(directory)
    path = ephemeral_file(root)
    with open(
        path,
        "x",
        encoding="utf-8",
        opener=lambda name, flags: os.open(name, flags, 0o600),
    ) as output:
        json.dump(values, output)
    # Only the bounded private handoff file is needed by import/build steps.
    for name in ("certificate.key", "certificate.p12", "updater.key"):
        (directory / name).unlink(missing_ok=True)
    export({"LOUPIST_UPDATER_PUBLIC_KEY": values["LOUPIST_UPDATER_PUBLIC_KEY"]})


def load_ephemeral(root: Path) -> dict[str, str]:
    path = ephemeral_file(root)
    if (
        path.is_symlink()
        or not path.is_file()
        or path.stat().st_mode & 0o077
        or path.stat().st_size > 1024 * 1024
    ):
        raise ValueError("dry-run credentials must be a private regular file")
    return json.loads(path.read_text(encoding="utf-8"))


def updater_environment(root: Path, *, dry_run: bool) -> dict[str, str]:
    names = ("TAURI_SIGNING_PRIVATE_KEY", "TAURI_SIGNING_PRIVATE_KEY_PASSWORD")
    try:
        values = load_ephemeral(root) if dry_run else os.environ
        environment = dict(os.environ)
        for name in names:
            if not values.get(name):
                raise ValueError(f"missing updater build credential: {name}")
            environment[name] = values[name]
    finally:
        # The child receives only its step-scoped environment. No later step
        # needs the private handoff, even when validation or the build fails.
        if dry_run:
            ephemeral_file(root).unlink(missing_ok=True)
    return environment


def build(root: Path, *, dry_run: bool) -> None:
    environment = updater_environment(root, dry_run=dry_run)
    run(
        "npm",
        "exec",
        "--",
        "tauri",
        "build",
        "--target",
        "aarch64-apple-darwin",
        "--bundles",
        "app,dmg",
        "--config",
        os.environ["LOUPIST_TAURI_RELEASE_CONFIG"],
        env=environment,
        cwd=Path(__file__).resolve().parents[1],
        capture_output=False,
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "command",
        choices=[
            "require",
            "ephemeral",
            "import",
            "build",
            "store-notary",
            "sign-updater",
        ],
    )
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--version")
    args = parser.parse_args()
    profile = load_profile(rehearsal=args.dry_run or args.command == "ephemeral")
    root = Path(os.environ["RUNNER_TEMP"])
    if args.command == "require":
        export(require(profile["macos_signing"]["mode"]))
    elif args.command == "ephemeral":
        persist_ephemeral(root)
    elif args.command == "store-notary":
        from macos_release_signing import NOTARY_PROFILE

        keychain = os.environ["LOUPIST_SIGNING_KEYCHAIN"]
        if args.dry_run:
            password, apple_id, team = (
                "dummy-notary-password",
                "rehearsal@example.invalid",
                DRY_TEAM,
            )
        else:
            password = os.environ.pop("APPLE_PASSWORD")
            apple_id, team = os.environ["APPLE_ID"], profile["macos_signing"]["team_id"]
        run(
            "xcrun",
            "notarytool",
            "store-credentials",
            NOTARY_PROFILE,
            "--keychain",
            keychain,
            "--apple-id",
            apple_id,
            "--team-id",
            team,
            *(["--no-validate"] if args.dry_run else []),
            input=password + "\n",
            start_new_session=True,
        )
        if args.dry_run:
            # Look up metadata only, restricted to the named profile in this job's keychain.
            run(
                "security",
                "find-generic-password",
                "-a",
                f"com.apple.gke.notary.tool.saved-creds.{NOTARY_PROFILE}",
                "-s",
                "appSpecificPassword",
                keychain,
            )
    elif args.command == "sign-updater":
        if not args.archive or not args.version:
            parser.error("sign-updater requires --archive and --version")
        run(
            "npm",
            "exec",
            "--",
            "tauri",
            "signer",
            "sign",
            "--app-version",
            args.version,
            args.archive.resolve(),
            env=updater_environment(root, dry_run=args.dry_run),
            cwd=Path(__file__).resolve().parents[1],
        )
    elif args.command == "build":
        build(root, dry_run=args.dry_run)
    else:
        if args.dry_run:
            values = load_ephemeral(root)
            for name in ("LOUPIST_CERTIFICATE", "LOUPIST_CERTIFICATE_PASSWORD"):
                os.environ[name] = values.pop(name)
            # Remove certificate handoff bytes before any staging helper runs.
            ephemeral_file(root).write_text(json.dumps(values), encoding="utf-8")
        export(import_certificate(root, profile, dry_run=args.dry_run))


if __name__ == "__main__":
    main()
