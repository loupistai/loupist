#!/usr/bin/env python3
import copy
import json
import runpy
import tempfile
import unittest
from pathlib import Path

from release_profile import load_profile, match_profile, validate_signing_record

SCRIPTS = Path(__file__).resolve().parent


class ReleaseProfileTests(unittest.TestCase):
    def test_beta_uses_an_unpinned_developer_id_leaf(self):
        profile = load_profile(rehearsal=True)
        self.assertEqual(profile["channel"], "beta")
        self.assertEqual(
            profile["macos_signing"],
            {
                "mode": "developer-id",
                "team_id": profile["macos_signing"]["team_id"],
                "certificate_common_name": None,
                "certificate_sha1": None,
                "certificate_sha256": None,
            },
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            legacy = copy.deepcopy(profile)
            legacy["macos_signing"].update(
                mode="self-signed",
                certificate_common_name="fixture",
                certificate_sha1="A" * 40,
                certificate_sha256="B" * 64,
            )
            legacy["macos_signing"].pop("team_id")
            path.write_text(json.dumps(legacy))
            self.assertIsNone(load_profile(path)["macos_signing"]["team_id"])
            profile["macos_signing"]["team_id"] = "TEAMID0000"
            path.write_text(json.dumps(profile))
            with self.assertRaisesRegex(
                ValueError,
                r"placeholder.*apps/loupist/release-profile\.json.*installation\.md",
            ):
                load_profile(path)
            profile["macos_signing"]["team_id"] = "ABCDE12345"
            for change in [
                lambda v: v["macos_signing"].pop("team_id"),
                lambda v: v["macos_signing"].update(team_id="invalid"),
                lambda v: v.update(channel="nightly"),
                lambda v: v.update(extra=True),
                lambda v: v["macos_signing"].update(mode="unknown"),
                lambda v: v["macos_signing"].update(mode="ad-hoc"),
                lambda v: v["macos_signing"].update(certificate_sha1="A" * 39),
                lambda v: v["macos_signing"].update(certificate_sha256="x" * 64),
                lambda v: v["macos_signing"].update(certificate_common_name=""),
            ]:
                value = copy.deepcopy(profile)
                change(value)
                path.write_text(json.dumps(value))
                with self.assertRaises(ValueError):
                    load_profile(path)

    def test_signing_records_enforce_each_mode_and_tag_policy(self):
        profile = load_profile(rehearsal=True)
        policy = profile["macos_signing"]
        policy.update(
            mode="self-signed",
            certificate_common_name="fixture",
            certificate_sha1="A" * 40,
            certificate_sha256="B" * 64,
        )
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
        with self.assertRaises(ValueError):
            validate_signing_record(adhoc)
        with self.assertRaises(ValueError):
            validate_signing_record({**adhoc, "certificate_sha256": "F" * 64})
        developer = {
            **signed,
            "mode": "developer-id",
            "notarized": True,
            "team_id": "ABCDE12345",
            "designated_requirement": 'identifier "com.monarchjuno.loupist" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */ and certificate leaf[field.1.2.840.113635.100.6.1.13] /* exists */ and certificate leaf[subject.OU] = ABCDE12345',
        }
        validate_signing_record(developer)
        policy.update(mode="developer-id", team_id="OTHER12345")
        with self.assertRaisesRegex(ValueError, "team differs"):
            match_profile(developer, profile)
        with self.assertRaises(ValueError):
            validate_signing_record({**developer, "notarized": False})

    def test_every_localized_publisher_line_matches_the_team_pin(self):
        check = runpy.run_path(str(SCRIPTS / "check-release-configuration.py"))[
            "check_publisher_guide"
        ]
        team = "ABCDE12345"
        actual = "TeamIdentifier=" + team
        section = "`" + actual + "`\n\ncodesign -dvv /Applications/Loupist.app\n"
        guide = "## English\n" + section + "## 한국어\n" + section
        check(guide, team)
        for bad in (
            guide.replace(actual, "TeamIdentifier=TEAMID0000", 1),
            guide.replace(actual, "TeamIdentifier=WRONG00000"),
            guide.replace(actual, "", 1),
            guide + "\n`TeamIdentifier=OTHER98765`",
        ):
            with (
                self.subTest(bad=bad),
                self.assertRaisesRegex(ValueError, "installation.md"),
            ):
                check(bad, team)

    def test_release_configuration_checks_the_real_production_resource_map(self):
        module = runpy.run_path(str(SCRIPTS / "check-release-configuration.py"))
        module["check"](SCRIPTS.parent)

    def test_notes_render_each_mode_without_claiming_apple_signing_for_beta(self):
        render = runpy.run_path(str(SCRIPTS / "release-notes.py"))["render"]
        profile = load_profile(rehearsal=True)
        profile["macos_signing"].update(mode="self-signed", certificate_sha256="A" * 64)
        beta = render("0.1.0-rc.1", profile)
        self.assertIn("Loupist 0.1.0-rc.1 (Beta)", beta)
        self.assertIn("Not notarized", beta)
        self.assertNotIn("{{", beta)
        self.assertIn("release candidate for testing", beta)
        self.assertNotIn("[product acceptance", beta)
        self.assertNotIn("[the complete unperformed_checks", beta)
        stable = render("0.1.0", profile)
        self.assertIn("[the complete unperformed_checks", stable)
        for mode in ("developer-id",):
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
            (templates / "developer-id.md").write_text(
                "{{version}}{{channel_suffix}}\n{{changelog}}"
            )
            changelog = root / "CHANGELOG.md"
            changelog.write_text(
                "## 0.1.0 - 2026-11-13\n\n- Release feature\n\n## 0.0.9 - 2026-11-12\n- Old feature"
            )
            self.assertIn(
                "Release feature", render("0.1.0", load_profile(rehearsal=True), root)
            )
            self.assertNotIn(
                "Old feature", render("0.1.0", load_profile(rehearsal=True), root)
            )
            self.assertIn(
                "See CHANGELOG.md",
                render("0.1.1-rc.1", load_profile(rehearsal=True), root),
            )


if __name__ == "__main__":
    unittest.main()
