from __future__ import annotations

import hashlib
import os
import runpy
from importlib.metadata import distributions as installed_distributions
from pathlib import Path

import pytest
from packaging.utils import canonicalize_name

from loupist_openbb.materialize import materialize
from license_bundle import (
    ARCHIVE_DIRECTORY,
    LicenseBundleError,
    build_license_archive,
    locked_runtime_closure,
    validate_license_archive,
)


def test_pyinstaller_hook_collects_runtime_data_and_omits_tests(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    import importlib.metadata
    import PyInstaller.utils.hooks as hooks

    monkeypatch.setattr(
        importlib.metadata,
        "packages_distributions",
        lambda: {
            "openbb": ["openbb"],
            "random_user_agent": ["random-user-agent"],
        },
    )
    collected: list[str] = []
    copied: list[str] = []

    def collect(package: str, *, include_py_files: bool) -> tuple:
        assert include_py_files
        collected.append(package)
        if package == "random_user_agent":
            return ([("runtime.dat", package)], [], [package])
        return (
            [
                ("runtime.dat", package),
                ("test.dat", f"{package}/tests"),
                ("skill.md", "openbb_mcp_server/skills"),
            ],
            [],
            [package, f"{package}.tests", f"{package}.test_example"],
        )

    def metadata(distribution: str) -> list[tuple[str, str]]:
        copied.append(distribution)
        return [(f"{distribution}.dist-info", f"{distribution}.dist-info")]

    monkeypatch.setattr(hooks, "collect_all", collect)
    monkeypatch.setattr(hooks, "copy_metadata", metadata)
    project_root = Path(__file__).resolve().parents[1]
    frozen = runpy.run_path(str(project_root / "hooks" / "hook-openbb.py"))
    assert collected == ["openbb", "random_user_agent"]
    assert "random-user-agent" in copied
    assert ("runtime.dat", "random_user_agent") in frozen["datas"]
    assert ("openbb.dist-info", "openbb.dist-info") in frozen["datas"]
    assert ("test.dat", "openbb/tests") not in frozen["datas"]
    assert ("skill.md", "openbb_mcp_server/skills") not in frozen["datas"]
    assert "openbb" in frozen["hiddenimports"]
    assert "openbb.tests" not in frozen["hiddenimports"]
    assert "openbb.test_example" not in frozen["hiddenimports"]


@pytest.mark.skipif(os.name == "nt", reason="Windows staging has no bundle symlinks")
def test_materialize_replaces_internal_file_and_directory_symlinks(
    tmp_path: Path,
) -> None:
    (tmp_path / "library.dylib").write_bytes(b"library")
    (tmp_path / "alias.dylib").symlink_to("library.dylib")
    (tmp_path / "framework-resources").mkdir()
    (tmp_path / "framework-resources" / "library").write_bytes(b"directory library")
    (tmp_path / "Resources").symlink_to("framework-resources", target_is_directory=True)

    materialize(tmp_path)

    assert not (tmp_path / "alias.dylib").is_symlink()
    assert (tmp_path / "alias.dylib").read_bytes() == b"library"
    assert not (tmp_path / "Resources").is_symlink()
    assert (tmp_path / "Resources" / "library").read_bytes() == b"directory library"


@pytest.mark.skipif(os.name == "nt", reason="Windows staging has no bundle symlinks")
def test_materialize_rejects_escaping_symlink(tmp_path: Path) -> None:
    outside = tmp_path.parent / f"{tmp_path.name}-outside"
    outside.write_bytes(b"outside")
    link = tmp_path / "escape"
    link.symlink_to(outside)

    with pytest.raises(SystemExit, match="escapes its bundle"):
        materialize(tmp_path)


def test_runtime_license_archive_covers_metadata_and_toolchains(
    tmp_path: Path,
) -> None:
    project_root = Path(__file__).resolve().parents[1]
    archive_root = tmp_path / ARCHIVE_DIRECTORY
    manifest = build_license_archive(project_root, archive_root)
    validate_license_archive(
        archive_root,
        expected_lock=(project_root / "uv.lock").read_bytes(),
    )

    distributions = {entry["name"]: entry for entry in manifest["distributions"]}
    installed = {
        canonicalize_name(item.metadata["Name"]) for item in installed_distributions()
    }
    assert (
        set(distributions)
        == set(locked_runtime_closure(project_root / "uv.lock")) & installed
    )
    assert set(distributions) >= {
        "certifi",
        "cryptography",
        "numpy",
        "openbb",
        "openbb-core",
        "openbb-mcp-server",
        "scipy",
    }
    for entry in distributions.values():
        assert entry["license_files"]
        for declared in entry["declared_license_files"]:
            assert set(declared["files"]) <= set(entry["license_files"])
    assert distributions["certifi"]["license_files"]

    repository_license_digest = hashlib.sha256(
        (project_root.parents[2] / "LICENSE").read_bytes()
    ).hexdigest()
    records = {record["path"]: record for record in manifest["files"]}
    openbb_entries = [
        entry
        for name, entry in distributions.items()
        if name == "openbb" or name.startswith("openbb-")
    ]
    assert all(
        entry["effective_license"] == "AGPL-3.0-only" for entry in openbb_entries
    )
    assert all(
        any(
            records[path]["sha256"] == repository_license_digest
            for path in entry["license_files"]
        )
        for entry in openbb_entries
    )
    assert {entry["name"] for entry in manifest["toolchain"]} == {
        "cpython",
        "pyinstaller",
    }
    assert all("direct_url.json" not in record["path"] for record in manifest["files"])

    certifi_license = archive_root / distributions["certifi"]["license_files"][0]
    certifi_license.write_bytes(certifi_license.read_bytes() + b"tampered")
    with pytest.raises(LicenseBundleError, match="digest is invalid"):
        validate_license_archive(archive_root)
