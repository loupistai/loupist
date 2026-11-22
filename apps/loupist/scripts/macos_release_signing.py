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
import time
from pathlib import Path

from release_profile import check_team, IDENTIFIER, MODES, load_profile, match_profile

WORKER_ENTITLEMENT = "com.apple.security.cs.allow-unsigned-executable-memory"
PI_ENTITLEMENTS = {"com.apple.security.cs.allow-jit": True, WORKER_ENTITLEMENT: True}
APP_ROOT = Path(__file__).resolve().parents[1]


def run(*command: str, check: bool = True) -> subprocess.CompletedProcess:
    # `file -b` and `codesign -d` can print bytes that are not UTF-8 (a copyright
    # sign in a description, a path in another encoding); never fail on them.
    result = subprocess.run(
        list(map(str, command)),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if check and result.returncode:
        raise RuntimeError(f"{command[0]} failed: {result.stdout}{result.stderr}")
    return result


def sign_code(command: list[str]) -> None:
    for attempt in range(3):
        try:
            run(*command)
            return
        except RuntimeError as error:
            if (
                "--timestamp" not in command
                or "timestamp" not in str(error).lower()
                or attempt == 2
            ):
                raise
            print(
                f"Timestamp failed for {command[-1]}, retrying ({attempt + 2}/3)",
                flush=True,
            )
            time.sleep(2 ** (attempt + 1))


def signing_mode() -> str | None:
    mode = os.environ.get("LOUPIST_MACOS_SIGNING_MODE") or None
    if mode is not None and mode not in MODES:
        raise ValueError("unknown LOUPIST_MACOS_SIGNING_MODE")
    return mode


def sidecar_command(
    path: Path, mode: str, identity: str, *, pi: bool = False, timestamp: bool = True
) -> list[str]:
    if (
        mode not in MODES
        or not identity
        or (mode == "self-signed" and not re.fullmatch("[A-Fa-f0-9]{40}", identity))
    ):
        raise ValueError("sidecar signing requires a valid mode and identity")
    command = ["codesign", "--force", "--sign", identity]
    if mode == "developer-id":
        command += [
            "--options",
            "runtime",
            "--timestamp" if timestamp else "--timestamp=none",
        ]
        if pi:
            command += [
                "--entitlements",
                str(APP_ROOT / "src-tauri/PiEntitlements.plist"),
            ]
    return command + [str(path)]


def macho_files(root: Path) -> list[Path]:
    # Only ask file(1) about possible Mach-O headers. A universal binary
    # and a Java class can share a magic number.
    magic = {
        bytes.fromhex(value)
        for value in (
            "feedface",
            "cefaedfe",
            "feedfacf",
            "cffaedfe",
            "cafebabe",
            "bebafeca",
            "cafebabf",
            "bfbafeca",
        )
    }
    files = []
    for path in root.rglob("*"):
        if path.is_symlink() or not path.is_file():
            continue
        with path.open("rb") as stream:
            if stream.read(4) not in magic:
                continue
        if "Mach-O" in run("file", "-b", str(path)).stdout:
            files.append(path)
    return sorted(files)


def external_libraries(path: Path) -> list[str]:
    output = run("otool", "-arch", "all", "-l", str(path)).stdout
    # LC_ID_DYLIB names the library itself. Only load commands are dependencies.
    dependencies = re.findall(
        r"^\s*cmd LC_(?:LOAD|LOAD_WEAK|REEXPORT|LOAD_UPWARD|LAZY_LOAD)_DYLIB\n"
        r"\s*cmdsize \d+\n\s*name (/[^\n]+?) \(offset",
        output,
        re.MULTILINE,
    )
    return sorted(
        {
            name
            for name in dependencies
            if not name.startswith(("/System/Library/", "/usr/lib/"))
        }
    )


def nested_bundles(root: Path, files: list[Path]) -> set[Path]:
    extensions = {".framework", ".app", ".appex", ".xpc", ".bundle", ".plugin"}
    return {
        parent
        for path in files
        for parent in path.relative_to(root).parents
        if parent.suffix in extensions
    }


def sign_runtime(
    root: Path, kind: str, mode: str, identity: str, *, timestamp: bool = True
) -> None:
    main = root / ("loupist-pi" if kind == "pi" else "loupist-openbb")
    files = macho_files(root)
    if main not in files:
        raise ValueError(f"missing Mach-O main executable: {main}")
    targets = set(files) | {root / path for path in nested_bundles(root, files)}
    targets.remove(main)
    for path in sorted(targets, key=lambda path: (-len(path.parts), path)) + [main]:
        sign_code(
            sidecar_command(
                path,
                mode,
                identity,
                pi=kind == "pi" and path == main,
                timestamp=timestamp,
            )
        )


def details(path: Path) -> str:
    result = run("codesign", "-d", "-vv", str(path))
    return result.stdout + result.stderr


def entitlements(path: Path) -> dict:
    result = run("codesign", "-d", "--entitlements", "-", "--xml", str(path))
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


def verify_binary(
    path: Path, mode: str, sha1: str | None, team: str | None, *, timestamp: bool = True
) -> None:
    command = ["codesign", "--verify", "--strict", "--verbose=2"]
    if mode == "self-signed":
        if not sha1 or re.fullmatch("[A-Fa-f0-9]{40}", sha1) is None:
            raise ValueError("self-signed verification requires the pinned SHA-1")
        command += ["-R", f'=certificate leaf = H"{sha1}"']
    elif mode == "developer-id":
        team = check_team(team)
        command += [
            "-R",
            "=anchor apple generic and certificate leaf[field.1.2.840.113635.100.6.1.13] exists"
            f' and certificate leaf[subject.OU] = "{team}"',
        ]
    else:
        raise ValueError("unknown macOS signing mode")
    run(*command, str(path))
    text = details(path)
    if mode == "developer-id":
        if f"TeamIdentifier={team}" not in text.splitlines():
            raise ValueError(f"wrong Apple team: {path}")
        if timestamp and not re.search(r"^Timestamp=.+$", text, re.MULTILINE):
            raise ValueError(f"missing secure timestamp: {path}")
    if mode == "self-signed" and "TeamIdentifier=not set" not in text:
        raise ValueError(f"self-signed binary unexpectedly has an Apple team: {path}")


def verify_runtime(
    pi: Path, worker: Path, mode: str, sha1: str | None, team: str | None = None
) -> None:
    files = macho_files(pi)
    if pi / "loupist-pi" not in files:
        raise ValueError("Pi executable is missing from runtime")
    for path in files + [worker]:
        if libraries := external_libraries(path):
            raise ValueError(
                f"non-system absolute library dependencies in {path}: {libraries}"
            )
        verify_binary(path, mode, sha1, team)
        verify_flags(path, mode, pi, worker)


def verify_flags(
    path: Path, mode: str, pi: Path, worker: Path, main: Path | None = None
) -> None:
    expected = {}
    if path == worker:
        expected = {WORKER_ENTITLEMENT: True}
    elif mode == "developer-id" and path == pi / "loupist-pi":
        expected = PI_ENTITLEMENTS
    require_flags(path, mode == "developer-id" or path in (main, worker), expected)


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


NOTARY_PROFILE = "loupist-release"


def notarize(upload: Path, staple_target: Path) -> None:
    auth = [
        "--keychain-profile",
        NOTARY_PROFILE,
        "--keychain",
        os.environ["LOUPIST_SIGNING_KEYCHAIN"],
    ]
    submitted = run(
        "xcrun",
        "notarytool",
        "submit",
        str(upload),
        *auth,
        "--output-format",
        "json",
        check=False,
    )
    if submitted.stderr:
        print(submitted.stderr, flush=True)
    try:
        result = json.loads(submitted.stdout)
    except ValueError:
        raise ValueError(
            f"notarytool returned no submission record: {submitted.stdout}"
        ) from None
    submission = result.get("id")
    if submitted.returncode or not submission:
        raise ValueError(f"Apple submission failed: {result}")
    print(
        f"Apple submission {submission} for {upload.name}. Recover with notarytool info/log {submission}.",
        flush=True,
    )
    waited = run(
        "xcrun",
        "notarytool",
        "wait",
        submission,
        *auth,
        "--timeout",
        "80m",
        "--output-format",
        "json",
        check=False,
    )
    if waited.stderr:
        print(waited.stderr, flush=True)
    try:
        status = json.loads(waited.stdout).get("status")
    except ValueError:
        status = waited.stdout or "no final status"
    if waited.returncode or status != "Accepted":
        log = run("xcrun", "notarytool", "log", submission, *auth, check=False)
        print(log.stdout + log.stderr, flush=True)
        raise ValueError(
            f"Apple submission {submission} did not complete as Accepted: {status}"
        )
    run("xcrun", "stapler", "staple", str(staple_target))
    require_ticket(staple_target, True)


def verify_updater(archive: Path, app: Path, notarized: bool) -> None:
    # Match the signed CodeDirectory, which seals the bundle resources, and
    # independently check the ticket survived the updater's tar round trip.
    match = re.search(
        r"^CDHash=([a-fA-F0-9]{40})$",
        run("codesign", "-d", "-vvv", str(app)).stderr,
        re.MULTILINE,
    )
    if not match:
        raise ValueError("missing reference app CodeDirectory hash")
    with tempfile.TemporaryDirectory(prefix="loupist-updater-") as directory:
        run("/usr/bin/tar", "-xzf", str(archive), "-C", directory)
        root = Path(directory)
        unpacked = root / app.name
        if (
            list(root.iterdir()) != [unpacked]
            or unpacked.is_symlink()
            or not unpacked.is_dir()
        ):
            raise ValueError("updater must contain exactly the release app")
        run(
            "codesign",
            "--verify",
            "--deep",
            "--strict",
            "-R",
            f'=cdhash H"{match[1]}"',
            str(unpacked),
        )
        if notarized:
            require_ticket(unpacked, True)


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
    if mode == "developer-id":
        pinned = check_team(policy["team_id"])
        if team and team != pinned:
            raise ValueError("expected Apple team differs from the release profile")
        team = pinned
    run("hdiutil", "verify", str(dmg))
    run("codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app))
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
    contents = app / "Contents"
    files = macho_files(contents)
    if not {main, worker, pi / "loupist-pi", pi / "openbb/loupist-openbb"}.issubset(
        files
    ):
        raise ValueError("a required app or runtime executable is missing")
    for path in files:
        verify_binary(path, mode, sha1, team)
        verify_flags(path, mode, pi, worker, main)
    for bundle in nested_bundles(contents, files):
        verify_binary(contents / bundle, mode, sha1, team)
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
        "certificate_sha256": certificate_hash(app),
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


def report_differences(expected: object, actual: object, prefix: str = "") -> list[str]:
    if isinstance(expected, dict) and isinstance(actual, dict):
        changes = []
        for key in sorted(expected.keys() | actual.keys()):
            field = f"{prefix}.{key}" if prefix else key
            if key not in expected or key not in actual:
                changes.append(field)
            else:
                changes.extend(report_differences(expected[key], actual[key], field))
        return changes
    return (
        [prefix or "<root>"]
        if type(expected) is not type(actual) or expected != actual
        else []
    )


def compare_reports(expected: Path, actual: Path) -> None:
    changes = report_differences(
        json.loads(expected.read_text()), json.loads(actual.read_text())
    )
    if changes:
        raise ValueError(
            "Built and mounted signing reports differ in these fields:\n- "
            + "\n- ".join(changes)
        )
    print("Built and mounted signing reports match.")


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
    bundle.add_argument("--updater", type=Path)
    compare = sub.add_parser("compare-reports")
    compare.add_argument("--expected", type=Path, required=True)
    compare.add_argument("--actual", type=Path, required=True)
    args = parser.parse_args()
    if args.command in {"sign-runtime", "verify-runtime"}:
        mode = signing_mode()
        if not mode:
            parser.error("distribution signing requires LOUPIST_MACOS_SIGNING_MODE")
        team = None
        if mode == "developer-id":
            policy = load_profile()["macos_signing"]
            team = check_team(policy["team_id"])
            if os.environ.get("APPLE_TEAM_ID", team) != team:
                parser.error("staging Apple team differs from the release profile")
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
            team,
        )
    elif args.command == "compare-reports":
        compare_reports(args.expected, args.actual)
    else:
        options = {
            "rehearsal": bool(os.environ.get("LOUPIST_EPHEMERAL_CERTIFICATE_SHA1"))
        }
        profile = (
            load_profile(args.profile, **options)
            if args.profile
            else load_profile(**options)
        )
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
        if args.updater:
            verify_updater(args.updater, args.app, record["notarized"])
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
