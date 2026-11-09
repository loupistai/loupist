"""Staged workspace tools fail closed when rg is missing, linked, or altered."""

import dataclasses
import io
import os
import tarfile
import tempfile
import unittest
import urllib.error
from pathlib import Path
from unittest.mock import call, patch

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
                    releases={
                        TARGET: (
                            "tar.gz",
                            stage.digest(encoded),
                            tool.releases[TARGET][2],
                        )
                    },
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

    def test_transient_download_uses_pinned_asset_api_then_verifies(self):
        tool = stage.TOOLS[0]
        payload = self.archives[tool.name].read_bytes()
        failure = urllib.error.HTTPError(
            tool.release_url(TARGET), 504, "timeout", {}, io.BytesIO()
        )
        with (
            patch.object(
                stage.urllib.request,
                "urlopen",
                side_effect=[failure, io.BytesIO(payload)],
            ) as opened,
            patch.object(stage.time, "sleep") as sleep,
        ):
            stage.stage_workspace_tools(self.runtime, TARGET)
        self.assertEqual(opened.call_count, 2)
        request = opened.call_args.args[0]
        self.assertEqual(
            request.full_url,
            "https://api.github.com/repos/BurntSushi/ripgrep/releases/assets/478118851",
        )
        self.assertEqual(request.get_header("Accept"), "application/octet-stream")
        self.assertEqual(opened.call_args.kwargs, {"timeout": 60})
        sleep.assert_called_once_with(1)
        stage.verify_workspace_tools(self.runtime, TARGET)

    def test_download_failures_preserve_runtime_and_have_bounded_retries(self):
        before = {path.name: path.read_bytes() for path in self.tools.iterdir()}
        for codes in ([502, 503, 504], [404], [504, 404], [403]):
            with (
                self.subTest(codes=codes),
                patch.object(
                    stage.urllib.request,
                    "urlopen",
                    side_effect=[
                        urllib.error.HTTPError(
                            "https://github.com/fixture",
                            code,
                            "failed",
                            {},
                            io.BytesIO(),
                        )
                        for code in codes
                    ],
                ) as opened,
                patch.object(stage.time, "sleep") as sleep,
            ):
                with self.assertRaises(urllib.error.HTTPError):
                    stage.stage_workspace_tools(self.runtime, TARGET)
                self.assertEqual(opened.call_count, len(codes))
                self.assertEqual(
                    sleep.call_args_list,
                    [call(2**index) for index in range(len(codes) - 1)],
                )
                self.assertEqual(
                    before,
                    {path.name: path.read_bytes() for path in self.tools.iterdir()},
                )

    def test_asset_api_response_still_requires_the_pinned_digest(self):
        before = (self.tools / "rg").read_bytes()
        with (
            patch.object(
                stage.urllib.request,
                "urlopen",
                side_effect=[
                    urllib.error.HTTPError(
                        "https://github.com/fixture", 504, "timeout", {}, io.BytesIO()
                    ),
                    io.BytesIO(b'{"not": "the archive"}'),
                ],
            ) as opened,
            patch.object(stage.time, "sleep"),
        ):
            with self.assertRaisesRegex(ValueError, "archive checksum mismatch"):
                stage.stage_workspace_tools(self.runtime, TARGET)
        self.assertEqual(opened.call_count, 2)
        self.assertEqual((self.tools / "rg").read_bytes(), before)

    def test_success_downloads_once_and_local_archive_never_downloads(self):
        payload = self.archives["rg"].read_bytes()
        with patch.object(
            stage.urllib.request, "urlopen", return_value=io.BytesIO(payload)
        ) as opened:
            stage.stage_workspace_tools(self.runtime, TARGET)
            opened.assert_called_once_with(
                stage.TOOLS[0].release_url(TARGET), timeout=60
            )
        with patch.object(
            stage.urllib.request, "urlopen", side_effect=AssertionError("network")
        ):
            stage.stage_workspace_tools(self.runtime, TARGET, self.archives)


if __name__ == "__main__":
    unittest.main()
