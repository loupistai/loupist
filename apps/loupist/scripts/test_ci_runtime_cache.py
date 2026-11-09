from __future__ import annotations

import importlib.util
import os
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "ci_runtime_cache", Path(__file__).with_name("ci_runtime_cache.py")
)
CACHE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CACHE)


class RuntimeCacheTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.resources = self.root / CACHE.RESOURCES
        for name in ("pi-runtime", "embed_assets"):
            directory = self.resources / name
            directory.mkdir(parents=True)
            (directory / "asset").write_bytes(b"pinned contents")
        self.asset = self.resources / "pi-runtime/asset"

    def test_source_and_toolchain_changes_invalidate_key(self):
        original = CACHE.fingerprint(
            self.root, [str(self.asset.relative_to(self.root))], {"rust": "1"}
        )
        changed_tool = CACHE.fingerprint(
            self.root, [str(self.asset.relative_to(self.root))], {"rust": "2"}
        )
        self.asset.write_bytes(b"new source")
        changed_source = CACHE.fingerprint(
            self.root, [str(self.asset.relative_to(self.root))], {"rust": "1"}
        )
        self.assertEqual(len({original, changed_source, changed_tool}), 3)

    def test_verifies_every_file_and_mode(self):
        CACHE.record(self.root, "source")
        CACHE.verify(self.root, "source")
        with self.assertRaisesRegex(ValueError, "source or toolchain"):
            CACHE.verify(self.root, "changed")
        for mutation in (
            lambda: self.asset.write_bytes(b"tampered"),
            lambda: self.asset.chmod(0o755),
            lambda: self.asset.unlink(),
            lambda: (self.resources / "pi-runtime/extra").write_bytes(b"new"),
        ):
            self.asset.write_bytes(b"pinned contents")
            self.asset.chmod(0o644)
            CACHE.record(self.root, "source")
            mutation()
            with self.assertRaises(ValueError):
                CACHE.verify(self.root, "source")

    def test_rejects_links_before_runtime_execution(self):
        for kind in ("symlink", "hardlink", "directory"):
            with self.subTest(kind=kind):
                link = self.resources / "pi-runtime/link"
                if kind == "hardlink":
                    os.link(self.asset, link)
                else:
                    link.symlink_to(self.root if kind == "directory" else self.asset)
                with self.assertRaises(ValueError):
                    CACHE.inventory(self.root)
                link.unlink()

    def test_python_runtime_required_when_worker_is_part_of_source(self):
        pin = self.root / "apps/loupist/python-worker/runtime.json"
        pin.parent.mkdir(parents=True)
        pin.write_text("{}")
        with self.assertRaisesRegex(ValueError, "python-runtime"):
            CACHE.inventory(self.root)
        runtime = self.resources / "python-runtime"
        runtime.mkdir()
        (runtime / "worker").write_bytes(b"worker")
        CACHE.record(self.root, "source")
        CACHE.verify(self.root, "source")


if __name__ == "__main__":
    unittest.main()
