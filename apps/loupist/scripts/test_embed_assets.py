"""Embedding staging derives the pinned table and rejects tampering or substitution."""

import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import struct
import tempfile
import unittest
from unittest import mock

from check_embed_assets import FILES, MODEL_FILES, verify

SPEC = importlib.util.spec_from_file_location(
    "stage_embed_assets", Path(__file__).with_name("stage-embed-assets.py")
)
stage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stage)


def float32(value):
    return struct.unpack("<f", struct.pack("<f", value))[0]


def safetensors(rows):
    """A float32 `embeddings` table in safetensors layout."""
    data = struct.pack(
        f"<{len(rows) * len(rows[0])}f", *[v for row in rows for v in row]
    )
    header = {
        "embeddings": {
            "dtype": "F32",
            "shape": [len(rows), len(rows[0])],
            "data_offsets": [0, len(data)],
        }
    }
    encoded = json.dumps(header).encode()
    return struct.pack("<Q", len(encoded)) + encoded + data


def sha256(data):
    return hashlib.sha256(data).hexdigest()


class EmbeddingBundleTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source = self.root / "source"
        self.packaged = self.root / "packaged"
        self.source.mkdir()
        self.packaged.mkdir()
        (self.source / "NOTICE").write_text("Model notice\n")
        for name in FILES:
            (self.packaged / name).write_bytes(
                (self.source / "NOTICE").read_bytes()
                if name == "NOTICE"
                else name.encode()
            )
        digests = {name: sha256(name.encode()) for name in MODEL_FILES}
        self.write_pin(digests, digests)

    def write_pin(self, upstream, staged):
        pin = {
            "model": "example/model",
            "revision": "0123456789abcdef",
            "upstream": upstream,
            "staged": staged,
        }
        (self.source / "pin.json").write_text(json.dumps(pin))

    def test_matching_assets_pass_and_altered_bytes_fail(self):
        verify(self.source, self.packaged)
        (self.packaged / "model.safetensors").write_bytes(b"altered model")
        with self.assertRaisesRegex(ValueError, "differs from its pin"):
            verify(self.source, self.packaged)
        (self.packaged / "model.safetensors").write_bytes(b"model.safetensors")
        (self.packaged / "NOTICE").write_text("Another notice\n")
        with self.assertRaisesRegex(ValueError, "differs from source: NOTICE"):
            verify(self.source, self.packaged)

    def test_missing_extra_and_empty_assets_fail(self):
        model = self.packaged / "model.safetensors"
        model.unlink()
        with self.assertRaisesRegex(ValueError, "inventory"):
            verify(self.source, self.packaged)
        model.write_bytes(b"")
        with self.assertRaisesRegex(ValueError, "nonempty regular file"):
            verify(self.source, self.packaged)
        model.write_bytes(b"model.safetensors")
        (self.packaged / "unexpected-library").write_bytes(b"extra")
        with self.assertRaisesRegex(ValueError, "inventory"):
            verify(self.source, self.packaged)
        (self.packaged / "unexpected-library").unlink()
        (self.source / "model.safetensors").write_bytes(b"not a source file")
        with self.assertRaisesRegex(ValueError, "inventory"):
            verify(self.source, self.packaged)

    def test_a_pin_without_every_model_file_fails(self):
        self.write_pin({"config.json": "0" * 64}, {"config.json": "0" * 64})
        with self.assertRaisesRegex(ValueError, "three model files"):
            verify(self.source, self.packaged)

    def test_matching_bytes_through_a_symlink_fail(self):
        model = self.packaged / "model.safetensors"
        model.unlink()
        (self.root / "model.safetensors").write_bytes(b"model.safetensors")
        model.symlink_to(self.root / "model.safetensors")
        with self.assertRaisesRegex(ValueError, "regular file"):
            verify(self.source, self.packaged)
        directory_link = self.root / "linked"
        directory_link.symlink_to(self.packaged, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "ordinary directory"):
            verify(self.source, directory_link)

    def test_each_row_is_stored_as_int8_with_its_own_float32_scale(self):
        rows = [[0.5, -0.25, 0.125, 0.0], [0.0, 0.0, 0.0, 0.0], [-3.0, 1.0, 0.3, 2.9]]
        upstream = self.root / "upstream.safetensors"
        upstream.write_bytes(safetensors(rows))
        derived = self.root / "derived.safetensors"
        stage.derive_int8(upstream, derived)
        data = derived.read_bytes()
        size = struct.unpack("<Q", data[:8])[0]
        self.assertEqual(size % 8, 0)
        header = json.loads(data[8 : 8 + size])
        self.assertEqual(header["embeddings"]["dtype"], "I8")
        self.assertEqual(header["embeddings"]["shape"], [3, 4])
        self.assertEqual(header["weights"]["dtype"], "F32")
        body = data[8 + size :]
        levels = struct.unpack("<12b", body[:12])
        scales = struct.unpack("<3f", body[12:24])
        for row, values in enumerate(rows):
            scale = float32(max(abs(value) for value in values) / 127.0)
            self.assertEqual(scales[row], scale)
            expected = [
                round(float32(value / scale)) if scale else 0 for value in values
            ]
            self.assertEqual(list(levels[row * 4 : row * 4 + 4]), expected)
        self.assertEqual(levels[8], -127)

    def test_staging_uses_the_pinned_revision_and_refuses_other_bytes(self):
        rows = [[0.5, -0.25], [0.75, 1.0]]
        upstream = {
            "config.json": b'{"normalize": true}',
            "tokenizer.json": b'{"model": {}}',
            "model.safetensors": safetensors(rows),
        }
        derived = self.root / "derived.safetensors"
        (self.root / "table.safetensors").write_bytes(upstream["model.safetensors"])
        stage.derive_int8(self.root / "table.safetensors", derived)
        staged = dict(upstream, **{"model.safetensors": derived.read_bytes()})
        self.write_pin(
            {name: sha256(upstream[name]) for name in MODEL_FILES},
            {name: sha256(staged[name]) for name in MODEL_FILES},
        )
        requested = []

        def download(url, destination):
            requested.append(url)
            destination.write_bytes(upstream[url.rsplit("/", 1)[1]])

        destination = self.root / "resources/embed_assets"
        with mock.patch.object(stage, "download", download):
            stage.stage(self.source, destination, None)
        verify(self.source, destination)
        self.assertEqual(
            sorted(requested),
            [
                f"https://huggingface.co/example/model/resolve/0123456789abcdef/{name}"
                for name in sorted(MODEL_FILES)
            ],
        )
        local = self.root / "local"
        local.mkdir()
        for name, data in upstream.items():
            (local / name).write_bytes(data)
        (local / "tokenizer.json").write_bytes(b'{"model": {"other": 1}}')
        shutil.rmtree(destination)
        with self.assertRaisesRegex(SystemExit, "tokenizer.json"):
            stage.stage(self.source, destination, local)
        self.assertFalse(destination.exists())


if __name__ == "__main__":
    unittest.main()
