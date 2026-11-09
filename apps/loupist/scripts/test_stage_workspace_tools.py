"""Staged workspace tools fail closed when rg is missing, linked, or altered."""

import dataclasses
import io
import os
import tarfile
import tempfile
import unittest
from pathlib import Path

import stage_workspace_tools as stage

TARGET = "aarch64-apple-darwin"


def fake_archive(tool: stage.Tool) -> bytes:
    prefix = tool.prefix(TARGET)
    members = {
        tool.executable(TARGET): (
            f"#!/bin/sh\necho '{tool.banner} (rev fake)'\n".encode(),
            0o755,
        ),
        **{name: (name.encode(), 0o644) for name in tool.licenses},
    }
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
        for name, (data, mode) in members.items():
            info = tarfile.TarInfo(f"{prefix}/{name}")
            info.size, info.mode = len(data), mode
            archive.addfile(info, io.BytesIO(data))
    return buffer.getvalue()


class WorkspaceToolsTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.archives, tools = {}, []
        for tool in stage.TOOLS:
            encoded = fake_archive(tool)
            path = self.root / f"{tool.name}.tar.gz"
            path.write_bytes(encoded)
            self.archives[tool.name] = path
            tools.append(
                dataclasses.replace(
                    tool,
                    releases={TARGET: ("tar.gz", stage.digest(encoded))},
                )
            )
        original = stage.TOOLS
        stage.TOOLS = tuple(tools)
        self.addCleanup(setattr, stage, "TOOLS", original)
        self.runtime = self.root / "runtime"
        stage.stage_workspace_tools(self.runtime, TARGET, self.archives)
        self.tools = self.runtime / "workspace-tools"

    def test_stages_ripgrep_and_verifies(self):
        stage.verify_workspace_tools(self.runtime, TARGET)
        for name in ("rg",):
            self.assertTrue(os.access(self.tools / name, os.X_OK))

    def test_real_pins_include_linux(self):
        self.assertIn("x86_64-unknown-linux-musl", stage.TARGETS)

    def test_missing_rg_fails(self):
        (self.tools / "rg").unlink()
        with self.assertRaisesRegex(ValueError, "rg executable is missing"):
            stage.verify_workspace_tools(self.runtime, TARGET)

    def test_linked_rg_fails(self):
        (self.tools / "rg").unlink()
        (self.tools / "rg").symlink_to(self.tools / "rg-UNLICENSE")
        with self.assertRaisesRegex(ValueError, "rg executable is missing or linked"):
            stage.verify_workspace_tools(self.runtime, TARGET)

    def test_altered_rg_fails(self):
        (self.tools / "rg").write_text("#!/bin/sh\necho 'ripgrep 15.2.0'\n# x\n")
        with self.assertRaisesRegex(ValueError, "rg executable differs"):
            stage.verify_workspace_tools(self.runtime, TARGET)

    def test_wrong_rg_version_fails_even_when_recorded(self):
        (self.tools / "rg").write_text("#!/bin/sh\necho 'ripgrep 1.0.0'\n")
        with self.assertRaisesRegex(ValueError, "rg version does not match"):
            stage.verify_workspace_tools(self.runtime, TARGET, record_signature=True)

    def test_missing_rg_manifest_entry_fails(self):
        manifest = self.tools / "manifest.json"
        manifest.write_text(manifest.read_text().replace('"rg"', '"other"'))
        with self.assertRaisesRegex(ValueError, "rg release identity"):
            stage.verify_workspace_tools(self.runtime, TARGET)

    def test_missing_rg_license_fails(self):
        (self.tools / "rg-UNLICENSE").unlink()
        with self.assertRaisesRegex(ValueError, "rg license is missing"):
            stage.verify_workspace_tools(self.runtime, TARGET)

    def test_archive_with_wrong_digest_is_rejected(self):
        bad = self.root / "bad.tar.gz"
        bad.write_bytes(b"not the pinned archive")
        with self.assertRaisesRegex(ValueError, "rg archive checksum mismatch"):
            stage.stage_workspace_tools(
                self.root / "other", TARGET, {**self.archives, "rg": bad}
            )


if __name__ == "__main__":
    unittest.main()
