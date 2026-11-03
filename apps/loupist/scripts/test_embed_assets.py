"""Retained embedding bundle checks reject tampering and path substitution."""

from pathlib import Path
import shutil
import tempfile
import unittest

from check_embed_assets import FILES, verify


class EmbeddingBundleTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source = self.root / "source"
        self.packaged = self.root / "packaged"
        self.source.mkdir()
        for name in FILES:
            (self.source / name).write_bytes(name.encode())
        shutil.copytree(self.source, self.packaged)

    def test_matching_assets_pass_and_altered_bytes_fail(self):
        verify(self.source, self.packaged)
        (self.packaged / "model.safetensors").write_bytes(b"altered model")
        with self.assertRaisesRegex(ValueError, "differs from source"):
            verify(self.source, self.packaged)

    def test_missing_extra_and_empty_assets_fail(self):
        model = self.packaged / "model.safetensors"
        model.unlink()
        with self.assertRaisesRegex(ValueError, "inventory"):
            verify(self.source, self.packaged)
        model.write_bytes(b"")
        with self.assertRaisesRegex(ValueError, "nonempty regular file"):
            verify(self.source, self.packaged)
        shutil.copyfile(self.source / model.name, model)
        (self.packaged / "unexpected-library").write_bytes(b"extra")
        with self.assertRaisesRegex(ValueError, "inventory"):
            verify(self.source, self.packaged)

    def test_matching_bytes_through_a_symlink_fail(self):
        model = self.packaged / "model.safetensors"
        model.unlink()
        model.symlink_to(self.source / model.name)
        with self.assertRaisesRegex(ValueError, "regular file"):
            verify(self.source, self.packaged)
        directory_link = self.root / "linked"
        directory_link.symlink_to(self.source, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "ordinary directory"):
            verify(self.source, directory_link)


if __name__ == "__main__":
    unittest.main()
