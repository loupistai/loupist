#!/usr/bin/env python3
import copy
import hashlib
import json
import runpy
import tempfile
import unittest
from pathlib import Path

from release_profile import load_profile, match_profile, validate_signing_record

SCRIPTS = Path(__file__).resolve().parent


class ReleaseProfileTests(unittest.TestCase):
    def test_profile_is_closed_and_certificate_pins_match_public_material(self):
        import base64

        profile = load_profile()
        pem = (SCRIPTS.parent / "release-signing-certificate.pem").read_text()
        der = base64.b64decode(
            "".join(line for line in pem.splitlines() if not line.startswith("-----"))
        )
        self.assertEqual(
            hashlib.sha1(der).hexdigest().upper(),
            profile["macos_signing"]["certificate_sha1"],
        )
        self.assertEqual(
            hashlib.sha256(der).hexdigest().upper(),
            profile["macos_signing"]["certificate_sha256"],
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            for change in [
                lambda v: v.update(channel="nightly"),
                lambda v: v.update(extra=True),
                lambda v: v["macos_signing"].update(mode="unknown"),
                lambda v: v["macos_signing"].update(certificate_sha1="A" * 39),
                lambda v: v["macos_signing"].update(certificate_sha256="x" * 64),
                lambda v: v["macos_signing"].update(certificate_common_name=""),
            ]:
                value = copy.deepcopy(profile)
                change(value)
                path.write_text(json.dumps(value))
                with self.assertRaises(ValueError):
                    load_profile(path)
            for mode in ("developer-id", "ad-hoc"):
                value = copy.deepcopy(profile)
                value["macos_signing"] = {
                    key: mode if key == "mode" else None
                    for key in value["macos_signing"]
                }
                path.write_text(json.dumps(value))
                self.assertEqual(load_profile(path), value)

    def test_signing_records_enforce_each_mode_and_tag_policy(self):
        profile = load_profile()
        policy = profile["macos_signing"]
        signed = {
            "mode": "self-signed",
            "notarized": False,
            "team_id": None,
            "certificate_sha256": policy["certificate_sha256"],
            "designated_requirement": 'identifier "com.monarchjuno.loupist" and certificate root = H"'
            + policy["certificate_sha1"]
            + '"',
        }
        match_profile(signed, profile)
        with self.assertRaises(ValueError):
            match_profile(
                {
                    **signed,
                    "designated_requirement": signed["designated_requirement"].replace(
                        policy["certificate_sha1"], "F" * 40
                    ),
                },
                profile,
            )
        with self.assertRaises(ValueError):
            validate_signing_record({**signed, "mode": "dry-run-ephemeral"})
        validate_signing_record(
            {**signed, "mode": "dry-run-ephemeral"}, allow_dry_run=True
        )
        adhoc = {
            "mode": "ad-hoc",
            "notarized": False,
            "team_id": None,
            "certificate_sha256": None,
            "designated_requirement": 'cdhash H"' + "a" * 40 + '"',
        }
        validate_signing_record(adhoc)
        with self.assertRaises(ValueError):
            validate_signing_record({**adhoc, "certificate_sha256": "F" * 64})
        developer = {
            **signed,
            "mode": "developer-id",
            "notarized": True,
            "team_id": "ABCDE12345",
            "designated_requirement": 'identifier "com.monarchjuno.loupist" and anchor apple generic and certificate leaf[subject.OU] = ABCDE12345',
        }
        validate_signing_record(developer)
        with self.assertRaises(ValueError):
            validate_signing_record({**developer, "notarized": False})

    def test_release_configuration_checks_the_real_production_resource_map(self):
        module = runpy.run_path(str(SCRIPTS / "check-release-configuration.py"))
        module["check"](SCRIPTS.parent)

    def test_notes_render_each_mode_without_claiming_apple_signing_for_beta(self):
        render = runpy.run_path(str(SCRIPTS / "release-notes.py"))["render"]
        profile = load_profile()
        beta = render("0.1.0-rc.1", profile)
        self.assertIn("Loupist 0.1.0-rc.1 (Beta)", beta)
        self.assertIn("Not notarized", beta)
        self.assertNotIn("{{", beta)
        for mode in ("developer-id", "ad-hoc"):
            changed = copy.deepcopy(profile)
            changed["macos_signing"]["mode"] = mode
            notes = render("0.1.0-rc.1", changed)
            self.assertIn("Developer ID" if mode == "developer-id" else "ad-hoc", notes)

    def test_notes_use_dated_changelog_and_do_not_burn_rcs_without_unreleased(self):
        render = runpy.run_path(str(SCRIPTS / "release-notes.py"))["render"]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            templates = root / ".github/release-notes"
            templates.mkdir(parents=True)
            (templates / "self-signed.md").write_text(
                "{{version}}{{channel_suffix}}\n{{changelog}}"
            )
            changelog = root / "CHANGELOG.md"
            changelog.write_text(
                "## 0.1.0 - 2026-11-13\n\n- Release feature\n\n## 0.0.9 - 2026-11-12\n- Old feature"
            )
            self.assertIn("Release feature", render("0.1.0", load_profile(), root))
            self.assertNotIn("Old feature", render("0.1.0", load_profile(), root))
            self.assertIn(
                "See CHANGELOG.md", render("0.1.1-rc.1", load_profile(), root)
            )


if __name__ == "__main__":
    unittest.main()
