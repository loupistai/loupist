#!/usr/bin/env python3
"""Create or verify the receipt required to publish a stable draft."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path
from urllib.parse import urlparse

from release_profile import load_profile, match_profile, validate_signing_record


STABLE_TAG = re.compile(r"^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")
RC_TAG = re.compile(r"^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)-rc\.([1-9]\d*)$")
REPOSITORY = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")
SOURCE_COMMIT = re.compile(r"^[0-9a-f]{40}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
MACOS_BUNDLE_VERSION = re.compile(r"^[1-9][0-9]*$")


def sha256(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def stable_version(tag: str) -> tuple[int, int, int]:
    match = STABLE_TAG.fullmatch(tag)
    if match is None:
        raise RuntimeError(f"candidate must be a stable vMAJOR.MINOR.PATCH tag: {tag}")
    return tuple(map(int, match.groups()))


def predecessor_version(tag: str) -> tuple[int, int, int, int, int]:
    stable = STABLE_TAG.fullmatch(tag)
    if stable is not None:
        major, minor, patch = map(int, stable.groups())
        return major, minor, patch, 1, 0
    rc = RC_TAG.fullmatch(tag)
    if rc is not None:
        major, minor, patch, sequence = map(int, rc.groups())
        return major, minor, patch, 0, sequence
    raise RuntimeError(
        f"predecessor must be a stable or canonical RC release tag: {tag}"
    )


def validate_release_sequence(
    candidate_tag: str, previous_tag: str, latest_stable_tag: str
) -> None:
    """Require an N-1 candidate path that cannot downgrade the stable feed."""

    candidate_version = stable_version(candidate_tag)
    previous_version = predecessor_version(previous_tag)
    candidate_order = (*candidate_version, 1, 0)
    if previous_version >= candidate_order:
        raise RuntimeError("the qualified installed release must precede the candidate")

    if latest_stable_tag:
        latest_version = stable_version(latest_stable_tag)
        if candidate_version <= latest_version:
            raise RuntimeError(
                "candidate stable release must be strictly newer than the current Latest release"
            )
        if previous_tag != latest_stable_tag:
            raise RuntimeError(
                "after the first stable release, qualification must start from the current Latest release"
            )
        return

    matching_rc = RC_TAG.fullmatch(previous_tag)
    if (
        matching_rc is None
        or tuple(map(int, matching_rc.groups()[:3])) != candidate_version
    ):
        raise RuntimeError(
            "the first stable release must qualify from its matching published RC"
        )


def validate_rc_release_sequence(
    candidate_tag: str,
    published_tags: list[str],
    existing_tags: list[str] | None = None,
) -> None:
    """Allow gaps only for immutable RC tags that never published a release.

    A skipped next-RC endpoint remains absent; installed copies still check the
    stable feed. A missing tag is never evidence of a burned candidate.
    """
    candidate = RC_TAG.fullmatch(candidate_tag)
    if candidate is None:
        raise RuntimeError(
            "release candidate must be a canonical vMAJOR.MINOR.PATCH-rc.N tag"
        )
    base, sequence = candidate.groups()[:3], int(candidate.group(4))
    published = set()
    for tag in published_tags:
        stable = STABLE_TAG.fullmatch(tag)
        if stable and stable.groups() == base:
            raise RuntimeError(
                "cannot publish a release candidate after its matching stable release"
            )
        match = RC_TAG.fullmatch(tag)
        if match and match.groups()[:3] == base:
            published.add(int(match.group(4)))
    if sequence <= max(published, default=0):
        raise RuntimeError("release candidate must be above every published RC")
    tags = set(existing_tags or [])
    prefix = "v" + ".".join(base) + "-rc."
    missing = [
        n
        for n in range(1, sequence)
        if n not in published and prefix + str(n) not in tags
    ]
    if missing:
        raise RuntimeError(
            f"release candidate must be contiguous unless each gap is an existing unpublished tag: expected rc.{missing[0]}"
        )


def evidence_url(value: str, label: str) -> str:
    parsed = urlparse(value)
    if (
        parsed.scheme != "https"
        or not parsed.netloc
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
    ):
        raise RuntimeError(
            f"{label} must be a credential-free HTTPS URL without a query or fragment"
        )
    return value


def positive_integer(value: str, label: str) -> str:
    if not value.isascii() or not value.isdigit() or int(value) < 1:
        raise RuntimeError(f"{label} must be a positive decimal integer")
    return value


def load_metadata(path: Path) -> dict[str, object]:
    if path.is_symlink() or not path.is_file():
        raise RuntimeError("release metadata must be a regular file")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise RuntimeError("release metadata must be a JSON object")
    return value


def candidate_set_sha256(release_metadata: Path) -> str:
    checksums = release_metadata.with_name("SHA256SUMS")
    if (
        checksums.is_symlink()
        or not checksums.is_file()
        or checksums.stat().st_size == 0
    ):
        raise RuntimeError("SHA256SUMS must be a nonempty regular file")
    return sha256(checksums)


def required_candidate_digest(value: str, label: str, expected: str) -> str:
    if not isinstance(value, str) or SHA256.fullmatch(value) is None:
        raise RuntimeError(f"{label} must be a lowercase SHA-256 digest")
    if value != expected:
        raise RuntimeError(f"{label} does not match the candidate set")
    return value


def validate_identity(
    *,
    candidate_tag: str,
    previous_tag: str,
    repository: str,
    source_commit: str,
    release_id: str,
    metadata: dict[str, object],
) -> str:
    candidate_version = stable_version(candidate_tag)
    previous_version = predecessor_version(previous_tag)
    candidate_order = (*candidate_version, 1, 0)
    if previous_version >= candidate_order:
        raise RuntimeError("the qualified installed release must precede the candidate")
    if not REPOSITORY.fullmatch(repository):
        raise RuntimeError("repository must be an owner/name pair")
    if not SOURCE_COMMIT.fullmatch(source_commit):
        raise RuntimeError("source commit must be a lowercase 40-digit SHA-1")
    positive_integer(release_id, "draft release id")
    if (
        metadata.get("schema_version") != 1
        or metadata.get("tag") != candidate_tag
        or metadata.get("version") != candidate_tag.removeprefix("v")
        or metadata.get("repository") != repository
        or metadata.get("source_commit") != source_commit
        or not isinstance(metadata.get("macos_bundle_version"), str)
        or MACOS_BUNDLE_VERSION.fullmatch(metadata["macos_bundle_version"]) is None
    ):
        raise RuntimeError("release metadata identity does not match qualification")
    validate_signing_record(metadata.get("macos_signing"))
    return candidate_tag.removeprefix("v")


UPDATE_METHODS = {
    "candidate-on-isolated-machine",
    "candidate-in-virtual-machine",
    "not-performed-on-candidate",
}
PRODUCT_METHODS = {
    "clean-isolated-machine",
    "clean-virtual-machine",
    "separate-user-account",
    "not-performed",
}


def validate_methods(channel: str, update: str, product: str, unperformed: str) -> None:
    if update not in UPDATE_METHODS or product not in PRODUCT_METHODS:
        raise RuntimeError("unknown qualification method")
    if (
        not isinstance(unperformed, str)
        or any(ord(c) < 32 for c in unperformed)
        or len(unperformed.splitlines()) > 1
    ):
        raise RuntimeError("unperformed checks must be one line of plain text")
    if channel == "stable" and (
        update != "candidate-on-isolated-machine" or product != "clean-isolated-machine"
    ):
        raise RuntimeError("stable channel requires both isolated-machine checks")
    if channel == "beta" and product == "not-performed":
        raise RuntimeError("Beta requires product acceptance")
    if channel == "beta" and not normalized_disclosure(unperformed):
        raise RuntimeError(
            "Beta requires a non-empty unperformed checks line, including when all checks were performed"
        )
    if (
        update != "candidate-on-isolated-machine" or product != "clean-isolated-machine"
    ) and not unperformed.strip():
        raise RuntimeError(
            "unperformed checks are required for these qualification methods"
        )


def check_result(method: str, url: str, digest: str, *, product: bool = False) -> dict:
    absent = method == ("not-performed" if product else "not-performed-on-candidate")
    return {
        "method": method,
        "result": "not_performed" if absent else "passed",
        "evidence_url": evidence_url(
            url, "product acceptance evidence URL" if product else "macOS evidence URL"
        )
        if url or not absent
        else "",
        "candidate_set_sha256": digest,
    }


def write_receipt(arguments: argparse.Namespace) -> int:
    metadata = load_metadata(arguments.release_metadata)
    profile = load_profile(arguments.profile)
    match_profile(metadata.get("macos_signing"), profile)
    candidate_digest = candidate_set_sha256(arguments.release_metadata)
    candidate_version = validate_identity(
        candidate_tag=arguments.candidate_tag,
        previous_tag=arguments.previous_tag,
        repository=arguments.repository,
        source_commit=arguments.source_commit,
        release_id=arguments.release_id,
        metadata=metadata,
    )
    validate_methods(
        profile["channel"],
        arguments.update_check,
        arguments.product_acceptance,
        arguments.unperformed_checks,
    )
    required_candidate_digest(
        arguments.macos_candidate_set_sha256,
        "macOS candidate-set digest",
        candidate_digest,
    )
    receipt = {
        "schema_version": 2,
        "channel": profile["channel"],
        "macos_signing_mode": profile["macos_signing"]["mode"],
        "repository": arguments.repository,
        "candidate_tag": arguments.candidate_tag,
        "candidate_version": candidate_version,
        "qualified_predecessor_tag": arguments.previous_tag,
        "source_commit": arguments.source_commit,
        "draft_release_id": arguments.release_id,
        "release_metadata_sha256": sha256(arguments.release_metadata),
        "candidate_set_sha256": candidate_digest,
        "qualification_workflow_run_id": positive_integer(
            arguments.workflow_run_id, "qualification workflow run id"
        ),
        "unperformed_checks": arguments.unperformed_checks,
        "platforms": {
            "macos-aarch64": check_result(
                arguments.update_check, arguments.macos_evidence_url, candidate_digest
            )
        },
        "product_acceptance": {
            "macos-aarch64": check_result(
                arguments.product_acceptance,
                arguments.macos_product_acceptance_evidence_url,
                candidate_digest,
                product=True,
            )
        },
    }
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(
        json.dumps(receipt, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return 0


def normalized_disclosure(line: str) -> str:
    line = line.strip()
    if line.startswith(("- ", "* ")):
        line = line[2:]
    if line.startswith("Not checked:"):
        line = line.removeprefix("Not checked:")
    return " ".join(line.split())


def verify_receipt(arguments: argparse.Namespace) -> int:
    metadata = load_metadata(arguments.release_metadata)
    profile = load_profile(arguments.profile)
    match_profile(metadata.get("macos_signing"), profile)
    receipt = load_metadata(arguments.receipt)
    required = {
        "schema_version",
        "repository",
        "candidate_tag",
        "candidate_version",
        "qualified_predecessor_tag",
        "source_commit",
        "draft_release_id",
        "release_metadata_sha256",
        "candidate_set_sha256",
        "qualification_workflow_run_id",
        "platforms",
        "product_acceptance",
        "channel",
        "macos_signing_mode",
        "unperformed_checks",
    }
    if set(receipt) != required or receipt["schema_version"] != 2:
        raise RuntimeError("qualification receipt schema is not exactly version 2")
    candidate_version = validate_identity(
        candidate_tag=arguments.candidate_tag,
        previous_tag=receipt["qualified_predecessor_tag"],
        repository=arguments.repository,
        source_commit=arguments.source_commit,
        release_id=arguments.release_id,
        metadata=metadata,
    )
    digest = candidate_set_sha256(arguments.release_metadata)
    expected_scalars = {
        "channel": profile["channel"],
        "macos_signing_mode": profile["macos_signing"]["mode"],
        "repository": arguments.repository,
        "candidate_tag": arguments.candidate_tag,
        "candidate_version": candidate_version,
        "source_commit": arguments.source_commit,
        "draft_release_id": arguments.release_id,
        "candidate_set_sha256": digest,
        "release_metadata_sha256": sha256(arguments.release_metadata),
        "qualification_workflow_run_id": positive_integer(
            arguments.workflow_run_id, "qualification workflow run id"
        ),
    }
    for key, expected in expected_scalars.items():
        if receipt[key] != expected:
            raise RuntimeError(f"qualification receipt field does not match: {key}")
    methods = []
    for section, product in [("platforms", False), ("product_acceptance", True)]:
        results = receipt[section]
        if not isinstance(results, dict) or set(results) != {"macos-aarch64"}:
            raise RuntimeError("qualification receipt must cover macOS Apple Silicon")
        result = results["macos-aarch64"]
        if not isinstance(result, dict) or set(result) != {
            "method",
            "result",
            "evidence_url",
            "candidate_set_sha256",
        }:
            raise RuntimeError("qualification result is malformed")
        methods.append(result["method"])
        required_candidate_digest(
            result["candidate_set_sha256"],
            "product acceptance candidate-set digest"
            if product
            else "macOS candidate-set digest",
            digest,
        )
        if result != check_result(
            result["method"], result["evidence_url"], digest, product=product
        ):
            raise RuntimeError("qualification result differs from the recorded method")
    validate_methods(profile["channel"], *methods, receipt["unperformed_checks"])
    if profile["channel"] == "beta":
        if arguments.release_json is None:
            raise RuntimeError("Beta promotion requires current draft title and notes")
        release = load_metadata(arguments.release_json)
        if not isinstance(release.get("name"), str) or not release["name"].endswith(
            "(Beta)"
        ):
            raise RuntimeError("Beta draft title must end in (Beta)")
        if not isinstance(release.get("body"), str) or normalized_disclosure(
            receipt["unperformed_checks"]
        ) not in {normalized_disclosure(line) for line in release["body"].splitlines()}:
            raise RuntimeError(
                "Beta draft notes must contain the whole unperformed_checks line verbatim after whitespace normalization"
            )
    return 0


def print_digest(arguments: argparse.Namespace) -> int:
    load_metadata(arguments.release_metadata)
    print(candidate_set_sha256(arguments.release_metadata))
    return 0


def validate_sequence(arguments: argparse.Namespace) -> int:
    validate_release_sequence(
        arguments.candidate_tag,
        arguments.previous_tag,
        arguments.latest_stable_tag,
    )
    return 0


def validate_rc_sequence(arguments: argparse.Namespace) -> int:
    if arguments.published_tags.is_symlink() or not arguments.published_tags.is_file():
        raise RuntimeError("published release tags must be a regular file")
    if arguments.existing_tags and (
        arguments.existing_tags.is_symlink() or not arguments.existing_tags.is_file()
    ):
        raise RuntimeError("existing RC tags must be a regular file")
    validate_rc_release_sequence(
        arguments.candidate_tag,
        arguments.published_tags.read_text(encoding="utf-8").splitlines(),
        arguments.existing_tags.read_text(encoding="utf-8").splitlines()
        if arguments.existing_tags
        else [],
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    write = subparsers.add_parser("write")
    write.add_argument("--candidate-tag", required=True)
    write.add_argument("--previous-tag", required=True)
    write.add_argument("--repository", required=True)
    write.add_argument("--source-commit", required=True)
    write.add_argument("--release-id", required=True)
    write.add_argument("--release-metadata", required=True, type=Path)
    write.add_argument("--workflow-run-id", required=True)
    write.add_argument("--macos-evidence-url", default="")
    write.add_argument("--update-check", choices=sorted(UPDATE_METHODS), required=True)
    write.add_argument(
        "--product-acceptance", choices=sorted(PRODUCT_METHODS), required=True
    )
    write.add_argument("--unperformed-checks", default="")
    write.add_argument(
        "--profile",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "release-profile.json",
    )
    write.add_argument("--macos-candidate-set-sha256", required=True)
    write.add_argument("--macos-product-acceptance-evidence-url", required=True)
    write.add_argument("--output", required=True, type=Path)
    write.set_defaults(handler=write_receipt)

    verify = subparsers.add_parser("verify")
    verify.add_argument("--candidate-tag", required=True)
    verify.add_argument("--repository", required=True)
    verify.add_argument("--source-commit", required=True)
    verify.add_argument("--release-id", required=True)
    verify.add_argument("--release-metadata", required=True, type=Path)
    verify.add_argument("--workflow-run-id", required=True)
    verify.add_argument("--receipt", required=True, type=Path)
    verify.add_argument(
        "--profile",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "release-profile.json",
    )
    verify.add_argument("--release-json", type=Path)
    verify.set_defaults(handler=verify_receipt)

    digest = subparsers.add_parser("digest")
    digest.add_argument("--release-metadata", required=True, type=Path)
    digest.set_defaults(handler=print_digest)

    sequence = subparsers.add_parser("sequence")
    sequence.add_argument("--candidate-tag", required=True)
    sequence.add_argument("--previous-tag", required=True)
    sequence.add_argument("--latest-stable-tag", required=True)
    sequence.set_defaults(handler=validate_sequence)

    rc_sequence = subparsers.add_parser("rc-sequence")
    rc_sequence.add_argument("--candidate-tag", required=True)
    rc_sequence.add_argument("--published-tags", required=True, type=Path)
    rc_sequence.add_argument("--existing-tags", type=Path)
    rc_sequence.set_defaults(handler=validate_rc_sequence)

    arguments = parser.parse_args()
    return arguments.handler(arguments)


if __name__ == "__main__":
    raise SystemExit(main())
