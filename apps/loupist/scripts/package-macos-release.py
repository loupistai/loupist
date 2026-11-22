#!/usr/bin/env python3
"""Seal the app, staple it before packaging, then sign and staple its disk image."""

import argparse
import os
import tempfile
from pathlib import Path

from macos_release_signing import (
    notarize,
    run,
    sign_code,
    verify_binary,
    verify_updater,
)
from release_profile import load_profile


def package(root: Path, *, dry_run: bool) -> None:
    root = root.resolve()
    app = root / "macos/Loupist.app"
    if not app.is_dir():
        raise ValueError(f"missing built app at {app}")
    identity = os.environ["APPLE_SIGNING_IDENTITY"]
    if dry_run != (os.environ.get("LOUPIST_EPHEMERAL_CERTIFICATE_SHA1") == identity):
        raise ValueError("rehearsal requires its disposable signing identity")
    policy = load_profile(rehearsal=dry_run)["macos_signing"]
    if policy["mode"] != "developer-id":
        raise ValueError("post-build notarization requires the Developer ID profile")
    sign = [
        "codesign",
        "--force",
        "--sign",
        identity,
        "--timestamp=none" if dry_run else "--timestamp",
    ]
    sign_code([*sign, "--options", "runtime", str(app)])
    mode = "self-signed" if dry_run else "developer-id"
    verify_binary(app, mode, identity, policy["team_id"])
    run("codesign", "--verify", "--deep", "--strict", str(app))
    archive = app.with_name(app.name + ".tar.gz")
    dmg = root / "dmg/Loupist.dmg"
    dmg.parent.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="loupist-package-") as directory:
        temporary = Path(directory)
        if not dry_run:
            upload = temporary / "Loupist.zip"
            run("ditto", "-c", "-k", "--keepParent", str(app), str(upload))
            notarize(upload, app)
            upload.unlink()
        run(
            "/usr/bin/tar",
            "--no-mac-metadata",
            "-czf",
            str(archive),
            "-C",
            str(app.parent),
            app.name,
        )
        verify_updater(archive, app, not dry_run)
        volume = temporary / "volume"
        volume.mkdir()
        run("ditto", str(app), str(volume / app.name))
        (volume / "Applications").symlink_to("/Applications")
        run(
            "hdiutil",
            "create",
            "-volname",
            "Loupist",
            "-srcfolder",
            str(volume),
            "-format",
            "UDZO",
            "-fs",
            "HFS+",
            str(dmg),
        )
    sign_code([*sign, str(dmg)])
    verify_binary(dmg, mode, identity, policy["team_id"])
    if not dry_run:
        notarize(dmg, dmg)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle_root", type=Path)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    package(args.bundle_root, dry_run=args.dry_run)
