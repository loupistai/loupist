#!/usr/bin/env python3
"""Validate authored release configuration before signing or a dry run."""

from __future__ import annotations
import json
import plistlib
from pathlib import Path
from check_agent_resources import production_files


def check(app: Path) -> None:
    tauri = app / "src-tauri"
    base = json.loads((tauri / "tauri.conf.json").read_text())
    release = json.loads((tauri / "tauri.release.conf.json").read_text())
    smoke = json.loads((tauri / "tauri.package-smoke.conf.json").read_text())
    info = plistlib.loads((tauri / "Info.plist").read_bytes())
    assert base["productName"] == "Loupist"
    assert base["identifier"] == "com.monarchjuno.loupist"
    assert base["build"]["beforeBuildCommand"] == "npm run build"
    assert base["version"].split("-", 1)[0] == info["CFBundleShortVersionString"]
    assert base["bundle"]["macOS"]["minimumSystemVersion"] == "13.0"
    assert not base["bundle"].get("externalBin") and not release["bundle"].get(
        "externalBin"
    )
    assert base["bundle"]["macOS"].get("hardenedRuntime", True) is True
    assert release["bundle"].get("macOS", {}).get("hardenedRuntime", True) is True
    assert not base["bundle"]["macOS"].get("entitlements") and not release[
        "bundle"
    ].get("macOS", {}).get("entitlements")
    assert release["bundle"]["createUpdaterArtifacts"] is True
    assert release["plugins"]["updater"]["requireSignedVersion"] is True
    resources = {
        "resources/pi-runtime/": "pi-runtime",
        "resources/embed_assets/": "embed_assets",
        "resources/python-runtime/": "python-runtime",
        "../../../LICENSE": "LICENSE",
        "../../../NOTICE": "NOTICE",
        "../THIRD_PARTY_NOTICES.md": "THIRD_PARTY_NOTICES.md",
        **{
            "../agent/" + path: "loupist-agent/" + path
            for path in production_files(app / "agent")
        },
    }
    assert release["bundle"]["resources"] == smoke["bundle"]["resources"] == resources


if __name__ == "__main__":
    check(Path(__file__).resolve().parents[1])
