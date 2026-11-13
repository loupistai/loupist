#!/usr/bin/env python3
"""Per-executable signing and strict verification shared by staging and release preflight."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import plistlib
import re
import subprocess
import tempfile
from pathlib import Path

from release_profile import IDENTIFIER, MODES, load_profile, match_profile

WORKER_ENTITLEMENT = "com.apple.security.cs.allow-unsigned-executable-memory"
PI_ENTITLEMENTS = {"com.apple.security.cs.allow-jit": True, WORKER_ENTITLEMENT: True}
APP_ROOT = Path(__file__).resolve().parents[1]


def run(*command: str, check: bool = True) -> subprocess.CompletedProcess:
    result = subprocess.run(list(map(str, command)), capture_output=True, text=True)
    if check and result.returncode:
        raise RuntimeError(f"{command[0]} failed: {result.stdout}{result.stderr}")
    return result


def signing_mode() -> str:
    mode = os.environ.get("LOUPIST_MACOS_SIGNING_MODE") or "developer-id"
    if mode not in MODES:
        raise ValueError("unknown LOUPIST_MACOS_SIGNING_MODE")
    return mode


def sidecar_command(
    path: Path, mode: str, identity: str, *, pi: bool = False
) -> list[str]:
    if (
        mode not in MODES
        or not identity
        or (mode == "self-signed" and not re.fullmatch("[A-Fa-f0-9]{40}", identity))
    ):
        raise ValueError("sidecar signing requires a valid mode and identity")
    command = ["codesign", "--force", "--sign", identity]
    if mode == "developer-id":
        command += ["--options", "runtime", "--timestamp"]
        if pi:
            command += [
                "--entitlements",
                str(APP_ROOT / "src-tauri/PiEntitlements.plist"),
            ]
    return command + [str(path)]


def macho_files(root: Path) -> list[Path]:
    return sorted(
        path
        for path in root.rglob("*")
        if path.is_file()
        and not path.is_symlink()
        and "Mach-O" in run("file", "-b", str(path)).stdout
    )


def sign_runtime(root: Path, kind: str, mode: str, identity: str) -> None:
    main = root / ("loupist-pi" if kind == "pi" else "loupist-openbb")
    files = macho_files(root)
    if main not in files:
        raise ValueError(f"missing Mach-O main executable: {main}")
    for path in [path for path in files if path != main] + [main]:
        run(*sidecar_command(path, mode, identity, pi=kind == "pi" and path == main))


def details(path: Path) -> str:
    result = run("codesign", "-d", "-vv", str(path))
    return result.stdout + result.stderr


def entitlements(path: Path) -> dict:
    result = run("codesign", "-d", "--entitlements", ":-", str(path))
    text = result.stdout
    if not text.strip():
        return {}
    # codesign diagnostics belong on stderr, never accept unparsed entitlement data.
    value = plistlib.loads(text.encode())
    if not isinstance(value, dict):
        raise ValueError(f"invalid entitlements: {path}")
    return value


def require_flags(path: Path, runtime: bool, expected_entitlements: dict) -> str:
    text = details(path)
    flags = re.search(r"flags=0x[0-9a-f]+\(([^)]*)\)", text)
    if flags is None or ("runtime" in flags.group(1).split(",")) != runtime:
        raise ValueError(f"wrong hardened-runtime flags: {path}")
    if entitlements(path) != expected_entitlements:
        raise ValueError(f"wrong entitlements: {path}")
    return text


def verify_binary(path: Path, mode: str, sha1: str | None, team: str | None) -> None:
    command = ["codesign", "--verify", "--strict", "--verbose=2"]
    if mode == "self-signed":
        if not sha1 or re.fullmatch("[A-Fa-f0-9]{40}", sha1) is None:
            raise ValueError("self-signed verification requires the pinned SHA-1")
        command += ["-R", f'=certificate leaf = H"{sha1}"']
    elif mode == "developer-id":
        command += ["-R", "anchor apple generic"]
    run(*command, str(path))
    text = details(path)
    if mode == "developer-id" and team and f"TeamIdentifier={team}\n" not in text:
        raise ValueError(f"wrong Apple team: {path}")
    if mode == "self-signed" and "TeamIdentifier=not set" not in text:
        raise ValueError(f"self-signed binary unexpectedly has an Apple team: {path}")


def verify_runtime(
    pi: Path, worker: Path, mode: str, sha1: str | None, team: str | None = None
) -> None:
    files = macho_files(pi)
    if pi / "loupist-pi" not in files:
        raise ValueError("Pi executable is missing from runtime")
    for path in files + [worker]:
        verify_binary(path, mode, sha1, team)
        if path == worker:
            require_flags(path, True, {WORKER_ENTITLEMENT: True})
        elif mode in {"self-signed", "ad-hoc"}:
            require_flags(path, False, {})
        elif path == pi / "loupist-pi":
            require_flags(path, True, PI_ENTITLEMENTS)


def designated_requirement(app: Path) -> str:
    result = run("codesign", "-d", "-r-", str(app))
    lines = [
        line.removeprefix("designated => ")
        for line in (result.stdout + result.stderr).splitlines()
        if line.startswith("designated => ")
    ]
    if len(lines) != 1:
        raise ValueError("app must have one designated requirement")
    return lines[0]


def certificate_hash(app: Path) -> str:
    with tempfile.TemporaryDirectory(prefix="loupist-public-cert-") as directory:
        prefix = str(Path(directory) / "certificate")
        run("codesign", "-d", "--extract-certificates=" + prefix, str(app))
        return hashlib.sha256(Path(prefix + "0").read_bytes()).hexdigest().upper()


def require_ticket(path: Path, notarized: bool) -> str:
    result = run("xcrun", "stapler", "validate", str(path), check=False)
    text = result.stdout + result.stderr
    if notarized and result.returncode != 0:
        raise ValueError(f"notarization ticket missing: {text}")
    if not notarized:
        if result.returncode == 0:
            raise ValueError("unnotarized mode unexpectedly has a notarization ticket")
        if "does not have a ticket stapled" not in text:
            raise ValueError(
                f"could not establish absence of notarization ticket: {text}"
            )
    return text


def verify_bundle(
    app: Path,
    dmg: Path,
    profile: dict,
    *,
    team: str | None = None,
    ephemeral_sha1: str | None = None,
) -> dict:
    policy = profile["macos_signing"]
    mode = "self-signed" if ephemeral_sha1 else policy["mode"]
    sha1 = ephemeral_sha1 or policy["certificate_sha1"]
    run("hdiutil", "verify", str(dmg))
    run("codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app))
    if mode != "ad-hoc":
        verify_binary(dmg, mode, sha1, team)
    if mode == "self-signed":
        run(
            "codesign",
            "--verify",
            "--strict",
            "-R",
            f'=identifier "{IDENTIFIER}" and certificate leaf = H"{sha1}"',
            str(app),
        )
    if mode == "developer-id" and not team:
        raise ValueError("Developer ID verification requires an expected Apple team")
    main = app / "Contents/MacOS/loupist"
    worker = app / "Contents/Resources/python-runtime/loupist-python-worker"
    pi = app / "Contents/Resources/pi-runtime"
    verify_runtime(pi, worker, mode, sha1, team)
    for path in macho_files(app / "Contents"):
        verify_binary(path, mode, sha1, team)
        if mode in {"self-signed", "ad-hoc"}:
            require_flags(
                path,
                path in {main, worker},
                {WORKER_ENTITLEMENT: True} if path == worker else {},
            )
    require_flags(main, True, {})
    ticket = require_ticket(dmg, mode == "developer-id")
    if mode == "developer-id":
        require_ticket(app, True)
    else:
        assessment = run(
            "spctl", "--assess", "--type", "execute", "-vv", str(app), check=False
        )
        status = run("spctl", "--status", check=False)
        summary = (
            "### Not notarized: Gatekeeper rejects this build, as recorded\n\n```text\n"
            + ticket
            + status.stdout
            + status.stderr
            + assessment.stdout
            + assessment.stderr
            + "\n```\n"
        )
        print(summary)
        if os.environ.get("GITHUB_STEP_SUMMARY"):
            with open(
                os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8"
            ) as output:
                output.write(summary)
    record = {
        "mode": "dry-run-ephemeral" if ephemeral_sha1 else mode,
        "notarized": mode == "developer-id",
        "team_id": team if mode == "developer-id" else None,
        "certificate_sha256": None if mode == "ad-hoc" else certificate_hash(app),
        "designated_requirement": designated_requirement(app),
    }
    if not ephemeral_sha1:
        match_profile(record, profile)
    else:
        from release_profile import validate_signing_record

        validate_signing_record(record, allow_dry_run=True)
        if sha1.upper() not in record["designated_requirement"].upper():
            raise ValueError(
                "ephemeral designated requirement does not pin its certificate"
            )
    return record


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sign = sub.add_parser("sign-runtime")
    sign.add_argument("--runtime", type=Path, required=True)
    sign.add_argument("--kind", choices=["pi", "openbb"], required=True)
    runtime = sub.add_parser("verify-runtime")
    runtime.add_argument("--pi", type=Path, required=True)
    runtime.add_argument("--worker", type=Path, required=True)
    bundle = sub.add_parser("verify-bundle")
    bundle.add_argument("--app", type=Path, required=True)
    bundle.add_argument("--dmg", type=Path, required=True)
    bundle.add_argument("--profile", type=Path)
    bundle.add_argument("--metadata", type=Path)
    bundle.add_argument("--release-config", type=Path)
    bundle.add_argument("--report", type=Path)
    args = parser.parse_args()
    if args.command == "sign-runtime":
        sign_runtime(
            args.runtime,
            args.kind,
            signing_mode(),
            os.environ["APPLE_SIGNING_IDENTITY"],
        )
    elif args.command == "verify-runtime":
        verify_runtime(
            args.pi,
            args.worker,
            signing_mode(),
            os.environ.get("APPLE_SIGNING_IDENTITY"),
        )
    else:
        profile = load_profile(args.profile) if args.profile else load_profile()
        if args.metadata:
            metadata = json.loads(args.metadata.read_text())
            match_profile(metadata["macos_signing"], profile)
        record = verify_bundle(
            args.app,
            args.dmg,
            profile,
            team=os.environ.get("APPLE_TEAM_ID")
            or (metadata["macos_signing"]["team_id"] if args.metadata else None),
            ephemeral_sha1=os.environ.get("LOUPIST_EPHEMERAL_CERTIFICATE_SHA1"),
        )
        if args.metadata and record != metadata["macos_signing"]:
            raise ValueError("mounted app signing facts differ from metadata")
        if args.report:
            config = json.loads(args.release_config.read_text())
            args.report.write_text(
                json.dumps(
                    {
                        "macos_signing": record,
                        "updater_public_key": config["plugins"]["updater"]["pubkey"],
                    },
                    indent=2,
                )
                + "\n"
            )


if __name__ == "__main__":
    main()
