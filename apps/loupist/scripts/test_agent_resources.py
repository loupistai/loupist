"""Exercise both exact production resource lists without building an app."""
import json
import shutil
import tempfile
import unittest
from pathlib import Path

from check_agent_resources import check, production_files

APP = Path(__file__).resolve().parents[1]


class AgentResourcesTest(unittest.TestCase):
    def test_renderer_has_no_unused_notification_or_asset_grant(self):
        capability = json.loads((APP / "src-tauri/capabilities/default.json").read_text())
        self.assertNotIn("notification:default", capability["permissions"])
        config = json.loads((APP / "src-tauri/tauri.conf.json").read_text())
        self.assertNotIn("asset:", config["app"]["security"]["csp"].split())

    def test_package_smoke_uses_the_same_production_resources_as_release(self):
        configs = [
            json.loads((APP / "src-tauri" / name).read_text())
            for name in ("tauri.release.conf.json", "tauri.package-smoke.conf.json")
        ]
        self.assertEqual(configs[0]["bundle"]["resources"], configs[1]["bundle"]["resources"])
        self.assertIs(configs[1]["bundle"]["createUpdaterArtifacts"], False)

    def test_packaged_resources_stage_exactly_production_agent_files(self):
        for name in ("tauri.release.conf.json", "tauri.package-smoke.conf.json"):
            with self.subTest(config=name):
                self.check_staged_resources(name)

    def check_staged_resources(self, config):
        mappings = json.loads((APP / "src-tauri" / config).read_text())["bundle"]["resources"]
        with tempfile.TemporaryDirectory() as temporary:
            resources = Path(temporary)
            for source, destination in mappings.items():
                if not destination.startswith("loupist-agent/"):
                    continue
                target = resources / destination
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(APP / "src-tauri" / source, target)
            packaged = resources / "loupist-agent"
            check(APP / "agent", packaged)
            self.assertTrue(production_files(packaged))
            for mutation in ("test", "extra", "changed", "missing"):
                with self.subTest(mutation=mutation):
                    file = packaged / "loupist-extension.mjs"
                    original = file.read_bytes()
                    extra = packaged / ("fixture.test.mjs" if mutation == "test" else "extra.mjs")
                    if mutation in ("test", "extra"):
                        extra.write_text("fixture")
                    elif mutation == "changed":
                        file.write_text("changed")
                    else:
                        file.unlink()
                    with self.assertRaises(ValueError):
                        check(APP / "agent", packaged)
                    if extra.exists():
                        extra.unlink()
                    file.write_bytes(original)


if __name__ == "__main__":
    unittest.main()
