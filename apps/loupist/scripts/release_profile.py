#!/usr/bin/env python3
"""The reviewed, tag-bound release policy and signing facts."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

PROFILE = Path(__file__).resolve().parents[1] / "release-profile.json"
MODES = {"developer-id", "self-signed"}
UPDATER_SECRETS = {
    "LOUPIST_UPDATER_PUBLIC_KEY",
    "LOUPIST_TAURI_SIGNING_PRIVATE_KEY",
    "LOUPIST_TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
}
MODE_SECRETS = {
    "developer-id": {
        "LOUPIST_APPLE_" + name
        for name in (
            "CERTIFICATE",
            "CERTIFICATE_PASSWORD",
            "SIGNING_IDENTITY",
            "ID",
            "PASSWORD",
            "TEAM_ID",
        )
    },
    "self-signed": {
        "LOUPIST_SELF_SIGNED_CERTIFICATE",
        "LOUPIST_SELF_SIGNED_CERTIFICATE_PASSWORD",
    },
}
ALL_SECRETS = UPDATER_SECRETS | set().union(*MODE_SECRETS.values())
IDENTIFIER = "com.monarchjuno.loupist"


def fingerprint(value: object, length: int) -> bool:
    return (
        isinstance(value, str)
        and re.fullmatch(r"[0-9A-F]{%d}" % length, value) is not None
    )


def load_profile(path: Path = PROFILE) -> dict:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict) or set(value) != {"channel", "macos_signing"}:
        raise ValueError(
            "release profile must contain exactly channel and macos_signing"
        )
    if value["channel"] not in {"beta", "stable"}:
        raise ValueError("release channel must be beta or stable")
    signing = value["macos_signing"]
    if not isinstance(signing, dict) or set(signing) != {
        "mode",
        "certificate_common_name",
        "certificate_sha1",
        "certificate_sha256",
    }:
        raise ValueError("release profile signing fields are invalid")
    mode = signing["mode"]
    if mode not in MODES:
        raise ValueError("unknown macOS signing mode")
    fields = [
        signing[key]
        for key in ("certificate_common_name", "certificate_sha1", "certificate_sha256")
    ]
    if mode == "developer-id" and all(field is None for field in fields):
        pass
    elif (
        not isinstance(fields[0], str)
        or not fields[0].strip()
        or not fingerprint(fields[1], 40)
        or not fingerprint(fields[2], 64)
    ):
        raise ValueError(
            "certificate name and uppercase SHA-1/SHA-256 fingerprints are required"
        )
    return value


def validate_signing_record(
    value: object,
    *,
    allow_dry_run: bool = False,
    expected_certificate: str | None = None,
) -> dict:
    if not isinstance(value, dict) or set(value) != {
        "mode",
        "notarized",
        "team_id",
        "certificate_sha256",
        "designated_requirement",
    }:
        raise ValueError("macOS signing record has unexpected fields")
    mode = value["mode"]
    if mode not in MODES | ({"dry-run-ephemeral"} if allow_dry_run else set()):
        raise ValueError("macOS signing mode is invalid or dry-run is not allowed")
    requirement = value["designated_requirement"]
    if not isinstance(requirement, str) or not requirement or "\n" in requirement:
        raise ValueError("macOS designated requirement must be one nonempty line")
    if not fingerprint(value["certificate_sha256"], 64):
        raise ValueError("macOS signing certificate SHA-256 is invalid")
    if mode in {"self-signed", "dry-run-ephemeral"}:
        if value["notarized"] is not False or value["team_id"] is not None:
            raise ValueError("self-signed signing must have no notarization or team")
        if (
            re.fullmatch(
                r'identifier "'
                + re.escape(IDENTIFIER)
                + r'" and certificate root = H"[0-9a-fA-F]{40}"',
                requirement,
            )
            is None
        ):
            raise ValueError(
                "self-signed designated requirement must pin the certificate root"
            )
    elif (
        value["notarized"] is not True
        or not isinstance(value["team_id"], str)
        or re.fullmatch(r"[A-Z0-9]{10}", value["team_id"]) is None
        or f'identifier "{IDENTIFIER}"' not in requirement
        or "anchor apple generic" not in requirement
        or value["team_id"] not in requirement
    ):
        raise ValueError(
            "Developer ID signing must record notarization and its Apple team requirement"
        )
    if expected_certificate and value["certificate_sha256"] != expected_certificate:
        raise ValueError("macOS signing certificate differs from the release profile")
    return value


def match_profile(record: dict, profile: dict) -> None:
    validate_signing_record(
        record, expected_certificate=profile["macos_signing"]["certificate_sha256"]
    )
    signing = profile["macos_signing"]
    if record["mode"] != signing["mode"]:
        raise ValueError(
            "metadata signing mode differs from the tagged release profile"
        )
    if (
        signing["mode"] == "self-signed"
        and record["designated_requirement"].upper()
        != (
            f'identifier "{IDENTIFIER}" and certificate root = H"{signing["certificate_sha1"]}"'
        ).upper()
    ):
        raise ValueError(
            "metadata certificate root differs from the tagged release profile"
        )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", type=Path, default=PROFILE)
    parser.add_argument(
        "--field",
        choices=["channel", "signing_mode", "certificate_sha1", "certificate_sha256"],
    )
    args = parser.parse_args()
    profile = load_profile(args.profile)
    fields = {
        "channel": profile["channel"],
        "signing_mode": profile["macos_signing"]["mode"],
        "certificate_sha1": profile["macos_signing"]["certificate_sha1"],
        "certificate_sha256": profile["macos_signing"]["certificate_sha256"],
    }
    if args.field:
        print(fields[args.field] or "")
    else:
        for key, value in fields.items():
            print(f"{key}={value or ''}")


if __name__ == "__main__":
    main()
