from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import plistlib
import subprocess
import tempfile
import unittest


APP_ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = APP_ROOT / "e2e/macos_dev_app.py"
SPEC = importlib.util.spec_from_file_location("macos_dev_app", MODULE_PATH)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class DevelopmentAppTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="loupist bundle ")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = self.root / "target/debug/loupist"
        self.binary.parent.mkdir(parents=True)
        self.binary.write_text('#!/bin/sh\nprintf "%s" "$1"\nexit 17\n')
        self.binary.chmod(0o700)
        self.icon = self.root / "icons/icon.icns"
        self.icon.parent.mkdir()
        self.icon.write_bytes((APP_ROOT / "src-tauri/icons/icon.icns").read_bytes())
        (self.root / "tauri.conf.json").write_text(
            json.dumps(
                {
                    "identifier": "com.monarchjuno.loupist",
                    "productName": "Loupist",
                    "version": "0.0.1",
                    "bundle": {"icon": ["icons/32x32.png", "icons/icon.icns"]},
                }
            )
        )

    def prepare(self, override="{}"):
        return MODULE.prepare_bundle(self.binary, self.root, override)

    def test_real_executable_preserves_arguments_exit_code_and_spaces(self):
        executable = self.prepare()
        result = subprocess.run(
            [executable, "an argument with spaces"], capture_output=True, text=True
        )
        self.assertEqual(result.returncode, 17)
        self.assertEqual(result.stdout, "an argument with spaces")
        info = plistlib.loads((executable.parents[1] / "Info.plist").read_bytes())
        self.assertEqual(
            info["CFBundleIdentifier"], "com.monarchjuno.loupist.development"
        )
        self.assertEqual(info["CFBundleExecutable"], executable.name)
        self.assertEqual(info["CFBundlePackageType"], "APPL")
        self.assertEqual(self.binary.read_bytes(), executable.read_bytes())

    def test_dev_and_e2e_bundles_use_the_packaged_icon(self):
        e2e = (APP_ROOT / "src-tauri/tauri.e2e.conf.json").read_text()
        for override in ("{}", e2e):
            with self.subTest(override=override):
                contents = self.prepare(override).parents[1]
                info = plistlib.loads((contents / "Info.plist").read_bytes())
                self.assertEqual(info["CFBundleIconFile"], "icon.icns")
                self.assertEqual(
                    (contents / "Resources" / info["CFBundleIconFile"]).read_bytes(),
                    self.icon.read_bytes(),
                )

    def test_manual_profile_keeps_separate_bundle_and_compiled_identity(self):
        first = self.prepare()
        manual = self.prepare(
            json.dumps({"identifier": "com.monarchjuno.loupist.qa-preview"})
        )
        self.assertNotEqual(first, manual)
        info = plistlib.loads((manual.parents[1] / "Info.plist").read_bytes())
        self.assertEqual(
            info["CFBundleIdentifier"], "com.monarchjuno.loupist.qa-preview"
        )

    def test_rebuild_replaces_copy_without_truncating_prior_executable(self):
        executable = self.prepare()
        with executable.open("rb") as old:
            self.binary.write_text("#!/bin/sh\nexit 19\n")
            self.prepare()
            self.assertIn(b"exit 17", old.read())
        self.assertEqual(subprocess.run([executable]).returncode, 19)

    def test_refuses_foreign_directory_and_profile_path_escape(self):
        executable = self.prepare()
        (executable.parents[2] / ".loupist-dev-bundle.json").unlink()
        with self.assertRaisesRegex(ValueError, "unowned"):
            self.prepare()
        with self.assertRaisesRegex(ValueError, "identifier"):
            self.prepare('{"identifier":"../../another-app"}')

    def test_refuses_symlinked_output_and_release_executable(self):
        outside = self.root / "outside"
        outside.mkdir()
        (self.binary.parent / "loupist-dev-apps").symlink_to(
            outside, target_is_directory=True
        )
        with self.assertRaisesRegex(ValueError, "real owned"):
            self.prepare()
        self.assertEqual(list(outside.iterdir()), [])
        with self.assertRaisesRegex(ValueError, "debug/loupist"):
            MODULE.prepare_bundle(self.root / "target/release/loupist", self.root)


if __name__ == "__main__":
    unittest.main()
