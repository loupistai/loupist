from __future__ import annotations

import hashlib
import json
import re
from importlib.metadata import entry_points, version
from pathlib import Path

from loupist_openbb.manifest import (
    load_runtime_manifest,
    resolve_network_hosts,
    resolve_providerless_tool_policy,
)

_HOSTNAME = re.compile(
    r"^(?=.{1,253}\Z)(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+"
    r"[a-z](?:[a-z0-9-]{0,61}[a-z0-9])?$"
)


def _normalized_provider_id(entry_point_name: str) -> str:
    return {
        "famafranch": "famafrench",
        "us_eia": "eia",
    }.get(entry_point_name, entry_point_name)


def test_manifest_matches_pinned_lock_packages_and_provider_entry_points() -> None:
    manifest = load_runtime_manifest()
    lock = Path(__file__).resolve().parents[1] / "uv.lock"
    assert manifest["uv_lock_sha256"] == hashlib.sha256(lock.read_bytes()).hexdigest()
    assert manifest["security"]["read_only"] is True

    for package, expected in manifest["packages"].items():
        assert version(package) == expected

    providers = manifest["providers"]
    provider_ids = [provider["id"] for provider in providers]
    assert len(provider_ids) == len(set(provider_ids))
    discovered = {
        _normalized_provider_id(item.name): (item.dist.name, item.dist.version)
        for item in entry_points(group="openbb_provider_extension")
    }
    declared = {
        provider["id"]: (provider["package"], provider["version"])
        for provider in providers
    }
    assert declared == discovered


def test_manifest_grants_are_provider_explicit_and_exact() -> None:
    manifest = load_runtime_manifest()
    credential_keys: set[str] = set()
    for provider in manifest["providers"]:
        mapping = provider["credential_mapping"]
        # A credential belongs to exactly one provider grant.
        assert credential_keys.isdisjoint(mapping.values())
        credential_keys.update(mapping.values())
        probe = provider.get("verification_probe")
        if probe:
            assert probe["arguments"]["provider"] == provider["id"]
        hosts = provider["network_hosts"]
        assert hosts == sorted(set(hosts))
        assert all(_HOSTNAME.fullmatch(host) for host in hosts)

    local_tools, implicit_provider = resolve_providerless_tool_policy(manifest)
    assert local_tools
    assert implicit_provider
    assert local_tools.isdisjoint(implicit_provider)


def test_marketplace_openbb_hosts_exactly_match_provider_unions() -> None:
    manifest = load_runtime_manifest()
    connectors_root = (
        Path(__file__).resolve().parents[2]
        / "marketplace"
        / "plugins"
        / "openbb"
        / "connectors"
    )
    declared_provider_ids: set[str] = set()
    for path in sorted(connectors_root.glob("*.json")):
        entry = json.loads(path.read_text(encoding="utf-8"))
        runtime = entry["runtime"]
        assert runtime["server_id"] == "openbb"
        provider_ids = set(runtime["provider_ids"])
        declared_provider_ids.update(provider_ids)
        assert set(entry["permissions"]["network_hosts"]) == resolve_network_hosts(
            provider_ids, manifest
        )

    assert declared_provider_ids == {
        provider["id"] for provider in manifest["providers"]
    }
