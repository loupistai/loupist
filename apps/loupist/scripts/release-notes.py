#!/usr/bin/env python3
"""Render truthful default release notes from the tag's signing profile."""

from __future__ import annotations
import argparse
import re
from pathlib import Path
from release_profile import load_profile

ROOT = Path(__file__).resolve().parents[3]


def render(version: str, profile: dict, root: Path = ROOT) -> str:
    if (
        re.fullmatch(
            r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-rc\.[1-9]\d*)?", version
        )
        is None
    ):
        raise ValueError("release version must be canonical")
    lines = (root / "CHANGELOG.md").read_text().splitlines()
    heading = next(
        (
            n
            for n, line in enumerate(lines)
            if re.fullmatch(r"## " + re.escape(version) + r" - \d{4}-\d{2}-\d{2}", line)
        ),
        None,
    )
    if heading is None:
        heading = next(
            (n for n, line in enumerate(lines) if line == "## Unreleased"), None
        )
    end = (
        next(
            (n for n in range(heading + 1, len(lines)) if lines[n].startswith("## ")),
            len(lines),
        )
        if heading is not None
        else 0
    )
    template = (
        root / ".github/release-notes" / (profile["macos_signing"]["mode"] + ".md")
    ).read_text()
    values = {
        "version": version,
        "channel_suffix": " (Beta)" if profile["channel"] == "beta" else "",
        "certificate_sha256": profile["macos_signing"]["certificate_sha256"] or "",
        "changelog": "\n".join(lines[heading + 1 : end]).strip()
        if heading is not None
        else "See CHANGELOG.md for the release history.",
    }
    for key, value in values.items():
        template = template.replace("{{" + key + "}}", value)
    return template


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    args.output.write_text(render(args.version, load_profile()))
