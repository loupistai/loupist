from __future__ import annotations

import subprocess
import sys
from pathlib import Path


def test_keyless_statement_reads_validate_and_fetch_through_mcp(tmp_path: Path) -> None:
    # OpenBB's process-wide settings and request guards must stay isolated from
    # other tests. Replace only Yahoo extraction so actual query validation runs.
    tmp_path.chmod(0o700)
    script = """
import asyncio
import sys
from contextlib import ExitStack
from pathlib import Path
from unittest.mock import patch

from loupist_openbb.bootstrap import Bootstrap, BootstrapSettings, configure_scratch_environment
from loupist_openbb.manifest import load_runtime_manifest, resolve_network_hosts

scratch = Path(sys.argv[1])
configure_scratch_environment(scratch)

from fastmcp import Client
from openbb_core.provider.utils.errors import EmptyDataError
from openbb_yfinance.models.balance_sheet import YFinanceBalanceSheetFetcher
from openbb_yfinance.models.cash_flow import YFinanceCashFlowStatementFetcher
from openbb_yfinance.models.income_statement import YFinanceIncomeStatementFetcher

from loupist_openbb.server import create_server

manifest = load_runtime_manifest()
providers = {
    p["id"] for p in manifest["providers"] if p["keyless"] and not p.get("config_mapping")
}
server = create_server(Bootstrap(
    run_id="keyless-statements",
    scratch_dir=scratch,
    credentials={},
    settings=BootstrapSettings(
        allowed_categories=tuple(manifest["allowed_categories"]),
        enabled_provider_ids=tuple(sorted(providers)),
        allowed_network_hosts=tuple(sorted(resolve_network_hosts(providers, manifest))),
        provider_config={},
    ),
))
fetchers = {
    "equity_fundamental_income": YFinanceIncomeStatementFetcher,
    "equity_fundamental_balance": YFinanceBalanceSheetFetcher,
    "equity_fundamental_cash": YFinanceCashFlowStatementFetcher,
}
calls = []

def extract(query, credentials, **kwargs):
    assert not credentials
    calls.append((query.symbol, query.period, query.limit))
    if query.symbol == "NO-DATA":
        raise EmptyDataError()
    return [
        {"period_ending": f"{2020 + i}-12-31 00:00:00"}
        for i in range(query.limit)
    ]

async def check():
    async with Client(server) as client:
        await client.call_tool("activate_tools", {"tool_names": list(fetchers)})
        inventory = {tool.name: tool for tool in await client.list_tools()}
        for name in fetchers:
            fields = inventory[name].inputSchema["properties"]
            assert fields["provider"]["enum"] == ["yfinance"], (name, fields["provider"])
            assert fields["limit"]["yfinance"]["maximum"] == 5
            assert fields["period"]["yfinance"]["choices"] == ["annual", "quarter"]

            for symbol, period, limit, field, expected in [
                ("TSLA", "annual", 6, "limit", {"minimum": 0, "maximum": 5}),
                ("TSLA", "quarter", 8, "limit", {"minimum": 0, "maximum": 5}),
                ("GM", "annual", 6, "limit", {"minimum": 0, "maximum": 5}),
                ("TSLA", "ttm", 5, "period", {"allowed_values": ["annual", "quarter"]}),
            ]:
                before = len(calls)
                result = await client.call_tool(
                    name, dict(provider="yfinance", symbol=symbol, period=period, limit=limit),
                    raise_on_error=False,
                )
                assert result.is_error
                assert result.structured_content == {"loupist_error": {
                    "kind": "invalid_arguments", "provider": "yfinance", "field": field, **expected
                }}, (name, symbol, period, limit, result)
                assert len(calls) == before

            for symbol, period, limit in [
                ("MSFT", "annual", 3), ("TSLA", "annual", 5), ("TSLA", "quarter", 5),
            ]:
                result = await client.call_tool(
                    name, dict(provider="yfinance", symbol=symbol, period=period, limit=limit)
                )
                assert calls[-1] == (symbol, period, limit)
                assert result.structured_content["provider"] == "yfinance"
                assert len(result.structured_content["results"]) == limit

            result = await client.call_tool(
                name, dict(provider="yfinance", symbol="NO-DATA", period="annual", limit=3),
                raise_on_error=False,
            )
            assert result.is_error
            assert result.structured_content == {"loupist_error": {
                "kind": "empty_result", "provider": "yfinance"
            }}

with ExitStack() as patches:
    for fetcher in fetchers.values():
        patches.enter_context(patch.object(fetcher, "extract_data", side_effect=extract))
    asyncio.run(check())
"""
    completed = subprocess.run(
        [sys.executable, "-c", script, str(tmp_path.resolve())],
        check=False,
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert completed.returncode == 0, completed.stderr
