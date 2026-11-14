"""Shell pitfalls in the release workflows that only show on the real path."""

import re
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
    def test_no_possibly_empty_array_is_expanded_without_a_guard(self):
        for name in NAMES:
            with self.subTest(workflow=name):
                text = (WORKFLOWS / name).read_text()
                self.assertEqual(unguarded_empty_array_expansions(text), [])

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
