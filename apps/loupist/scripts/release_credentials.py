#!/usr/bin/env python3
"""Require mode-specific credentials, or generate disposable dry-run keys, then import."""

from __future__ import annotations

import argparse
import base64
import hashlib
import os
import re
import secrets
import subprocess
from pathlib import Path

from release_profile import MODE_SECRETS, UPDATER_SECRETS, load_profile


def run(*args: str, **kwargs) -> subprocess.CompletedProcess:
    result = subprocess.run(
        list(map(str, args)), check=False, capture_output=True, text=True, **kwargs
    )
    if result.returncode:
        # Never include the command arguments: security/openssl take passwords.
        raise RuntimeError(f"{args[0]} failed during release credential setup")
    return result


def export(values: dict[str, str], *, sensitive: bool = False) -> None:
    with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as output:
        for key, value in values.items():
            if sensitive:
                masked = (
                    value.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
                )
                print(f"::add-mask::{masked}")
            delimiter = "LOUPIST_" + secrets.token_hex(16)
            output.write(f"{key}<<{delimiter}\n{value}\n{delimiter}\n")


def require(mode: str) -> dict[str, str]:
    required = UPDATER_SECRETS | MODE_SECRETS[mode]
    missing = sorted(name for name in required if not os.environ.get(name))
    if missing:
        raise ValueError("Required release credential is absent: " + ", ".join(missing))
    values = {
        "LOUPIST_UPDATER_PUBLIC_KEY": os.environ["LOUPIST_UPDATER_PUBLIC_KEY"],
        "TAURI_SIGNING_PRIVATE_KEY": os.environ["LOUPIST_TAURI_SIGNING_PRIVATE_KEY"],
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD": os.environ[
            "LOUPIST_TAURI_SIGNING_PRIVATE_KEY_PASSWORD"
        ],
    }
    if mode == "self-signed":
        values |= {
            "LOUPIST_CERTIFICATE": os.environ["LOUPIST_SELF_SIGNED_CERTIFICATE"],
            "LOUPIST_CERTIFICATE_PASSWORD": os.environ[
                "LOUPIST_SELF_SIGNED_CERTIFICATE_PASSWORD"
            ],
        }
    elif mode == "developer-id":
        values |= {
            "LOUPIST_CERTIFICATE": os.environ["LOUPIST_APPLE_CERTIFICATE"],
            "LOUPIST_CERTIFICATE_PASSWORD": os.environ[
                "LOUPIST_APPLE_CERTIFICATE_PASSWORD"
            ],
            "APPLE_SIGNING_IDENTITY": os.environ["LOUPIST_APPLE_SIGNING_IDENTITY"],
        }
    else:
        values["APPLE_SIGNING_IDENTITY"] = "-"
    return values


def ephemeral(root: Path) -> dict[str, str]:
    root.mkdir(mode=0o700)
    config = root / "codesign.cnf"
    config.write_text("""[req]
distinguished_name=dn
x509_extensions=ext
prompt=no
[dn]
CN=Loupist Dry Run Ephemeral
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
        "Loupist Dry Run Ephemeral",
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


def import_certificate(root: Path, profile: dict, *, dry_run: bool) -> dict[str, str]:
    certificate, keychain = (
        root / "loupist-signing.p12",
        root / "loupist-signing.keychain-db",
    )
    certificate.write_bytes(
        base64.b64decode(os.environ["LOUPIST_CERTIFICATE"], validate=True)
    )
    certificate.chmod(0o600)
    password = secrets.token_hex(30)
    run("security", "create-keychain", "-p", password, keychain)
    # Export immediately so the workflow's always() cleanup also handles a failed import.
    export({"LOUPIST_SIGNING_KEYCHAIN": str(keychain)})
    run("security", "set-keychain-settings", "-lut", "21600", keychain)
    run("security", "unlock-keychain", "-p", password, keychain)
    run(
        "security",
        "import",
        certificate,
        "-P",
        os.environ["LOUPIST_CERTIFICATE_PASSWORD"],
        "-A",
        "-t",
        "cert",
        "-f",
        "pkcs12",
        "-k",
        keychain,
    )
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
    hashes = re.findall(r"^\s*\d+\) ([A-Fa-f0-9]{40}) ", identities, re.MULTILINE)
    if len(hashes) != 1:
        raise ValueError(
            "signing keychain must contain exactly one code-signing identity"
        )
    pem = run("security", "find-certificate", "-a", "-p", keychain).stdout
    if pem.count("-----BEGIN CERTIFICATE-----") < 1:
        raise ValueError("imported code-signing certificate is absent")
    # Select the leaf matching the identity, not a CA certificate in a Developer ID chain.
    certificates = re.findall(
        r"-----BEGIN CERTIFICATE-----\s*(.*?)\s*-----END CERTIFICATE-----",
        pem,
        re.DOTALL,
    )
    der = [base64.b64decode(re.sub(r"\s", "", item)) for item in certificates]
    leaves = [
        item
        for item in der
        if hashlib.sha1(item).hexdigest().upper() == hashes[0].upper()
    ]
    if len(leaves) != 1:
        raise ValueError("identity and imported certificate do not match")
    sha1, sha256 = hashes[0].upper(), hashlib.sha256(leaves[0]).hexdigest().upper()
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


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["require", "ephemeral", "import"])
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    profile = load_profile()
    root = Path(os.environ["RUNNER_TEMP"])
    if args.command == "require":
        export(require(profile["macos_signing"]["mode"]), sensitive=True)
    elif args.command == "ephemeral":
        export(ephemeral(root / "loupist-ephemeral"), sensitive=True)
        export({"LOUPIST_MACOS_SIGNING_MODE": "self-signed"})
    else:
        export(import_certificate(root, profile, dry_run=args.dry_run))


if __name__ == "__main__":
    main()
