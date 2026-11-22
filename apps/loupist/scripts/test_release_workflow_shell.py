"""Shell pitfalls in the release workflows that only show on the real path."""

import os
import re
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path

WORKFLOWS = Path(__file__).resolve().parents[3] / ".github/workflows"
NAMES = ("release.yml", "release-qualification.yml", "promote-release.yml")


def unguarded_empty_array_expansions(text: str) -> list[str]:
    """Expansions of an array that may still be empty, without the `+` guard.

    macOS runners use bash 3.2: under `set -u`, "${args[@]}" of an empty array
    is an unbound variable. The first release candidate's build failed on it,
    because only the dry run ever put an element in the array.
    """
    found = []
    maybe_empty: set[str] = set()
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("#"):
            continue
        assigned = re.match(r"(\w+)=\((.*)$", stripped)
        if assigned:
            name, rest = assigned.groups()
            if rest.strip() == ")":
                maybe_empty.add(name)
            else:
                maybe_empty.discard(name)
            continue
        for name in maybe_empty:
            guarded = r"\$\{" + name + r'\[@\]\+"\$\{' + name + r'\[@\]\}"\}'
            if re.search(r'"\$\{' + name + r'\[@\]\}"', re.sub(guarded, "", stripped)):
                found.append(stripped)
    return found


class ReleaseWorkflowShellTests(unittest.TestCase):
    def test_published_tag_sequence_check_only_applies_to_rcs(self):
        workflow = (WORKFLOWS / "release.yml").read_text()
        validation = workflow.split("\n  validate:\n", 1)[1]
        validation = validation.split("\n  full-ci:\n", 1)[0]
        published_tag_checks = [
            step
            for step in validation.split("\n      - ")
            if "/releases?per_page=" in step
        ]
        self.assertEqual(len(published_tag_checks), 1)
        (rc_check,) = published_tag_checks
        self.assertIn("steps.identity.outputs.prerelease == 'true'", rc_check)
        self.assertIn("release-qualification.py rc-sequence", rc_check)

    def test_no_possibly_empty_array_is_expanded_without_a_guard(self):
        for name in NAMES:
            with self.subTest(workflow=name):
                text = (WORKFLOWS / name).read_text()
                self.assertEqual(unguarded_empty_array_expansions(text), [])

    def test_failed_keychain_deletion_warns_after_other_cleanup(self):
        workflow = (WORKFLOWS / "release.yml").read_text()
        step = workflow.split("      - name: Remove temporary signing keychain\n")[
            1
        ].split("\n\n")[0]
        cleanup = textwrap.dedent(step.split("        run: |\n")[1])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "loupist-ephemeral").mkdir()
            (root / "loupist-ephemeral/credentials.json").touch()
            for name in ("loupist-signing.p12", "loupist-tauri-release.json"):
                (root / name).touch()
            # Fail the test if the deletion is attempted while any handoff exists.
            fake_security = (
                'security() { [[ "$1" == delete-keychain && "$2" == fixture ]] || exit 2; '
                '[[ -z "$(ls -A "$RUNNER_TEMP")" ]] || exit 3; return 1; }\n'
            )
            result = subprocess.run(
                ["/bin/bash", "-eu", "-c", fake_security + cleanup],
                text=True,
                capture_output=True,
                env={
                    **os.environ,
                    "RUNNER_TEMP": directory,
                    "LOUPIST_SIGNING_KEYCHAIN": "fixture",
                },
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("::warning::", result.stdout)
            self.assertEqual(list(root.iterdir()), [])

    def test_publication_does_not_depend_on_the_implicit_success_check(self):
        # Without a status function GitHub adds success(), which also counts
        # ancestor jobs; every tag run has a skipped job inside the full CI, so
        # the publish job would be skipped after a green build and assembly.
        text = (WORKFLOWS / "release.yml").read_text()
        publish = text.split("\n  publish:\n", 1)[1].split("\n    runs-on:", 1)[0]
        condition = next(
            line.strip()
            for line in publish.splitlines()
            if line.strip().startswith("if:")
        )
        self.assertIn("!cancelled()", condition)
        self.assertIn("needs.validate.result == 'success'", condition)
        self.assertIn("needs.assemble.result == 'success'", condition)
        self.assertIn("needs.validate.outputs.dry_run != 'true'", condition)
        qualification = (WORKFLOWS / "release-qualification.yml").read_text()
        block = qualification.split("      unperformed_checks:\n", 1)[1].split(
            "type:", 1
        )[0]
        self.assertIn("required: true", block)

    def test_the_detector_sees_the_failing_form_and_accepts_the_guarded_one(self):
        failing = 'args=()\nif x; then args+=(--dry-run); fi\ntool build "${args[@]}"\n'
        self.assertEqual(
            unguarded_empty_array_expansions(failing), ['tool build "${args[@]}"']
        )
        guarded = 'args=()\ntool build ${args[@]+"${args[@]}"} --assets x\n'
        self.assertEqual(unguarded_empty_array_expansions(guarded), [])
        filled = 'args=(--profile p)\ntool verify "${args[@]}"\n'
        self.assertEqual(unguarded_empty_array_expansions(filled), [])


if __name__ == "__main__":
    unittest.main()
