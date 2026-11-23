"""Exercise the real Alpha Vantage MCP tool with a local HTTP response fixture."""

import asyncio
import json
from datetime import date
from pathlib import Path
import subprocess
import sys
from urllib.parse import parse_qs, urlsplit


def test_alpha_vantage_daily_prices_and_plan_refusals(tmp_path):
    subprocess.run([sys.executable, __file__, str(tmp_path)], check=True, timeout=90)


def check_daily_prices(scratch):
    from loupist_openbb.bootstrap import (
        Bootstrap,
        BootstrapSettings,
        configure_scratch_environment,
    )

    configure_scratch_environment(scratch)

    from fastmcp import Client
    from openbb_core.provider.utils import helpers
    from loupist_openbb.manifest import load_runtime_manifest
    from loupist_openbb.server import create_server

    requests = []
    day = date.today().isoformat()
    raw = f"timestamp,open,high,low,close,volume\n{day},1,3,1,2,12\n".encode()
    premium = json.dumps({"Information": "This is a premium endpoint."}).encode()
    # Alpha Vantage's reply to a free key's second request within a second,
    # recorded on 2026-11-23. It ends by offering premium endpoints.
    burst = json.dumps(
        {
            "Information": "Thank you for using Alpha Vantage! Please consider spreading out your free API requests more sparingly (1 request per second). You may subscribe to any of the premium plans at https://www.alphavantage.co/premium/ to lift the free key rate limit (25 requests per day), raise the per-second burst limit, and instantly unlock all premium endpoints"
        },
        indent=4,
    ).encode()
    bursts = []

    class Response:
        def __init__(self, body):
            self.body = body

        async def json(self):
            return json.loads(self.body)

        async def read(self):
            return self.body

    async def request(url, response_callback, **kwargs):
        parts = urlsplit(url)
        assert parts.netloc == "www.alphavantage.co"
        query = parse_qs(parts.query)
        assert query["apikey"] == ["fixture"]
        mode = (query["function"][0], query["outputsize"][0])
        requests.append(mode)
        body = raw if mode == ("TIME_SERIES_DAILY", "compact") else premium
        if bursts:
            body = bursts.pop()
        return await response_callback(Response(body), None)

    helpers.amake_request = request
    mcp = create_server(
        Bootstrap(
            run_id="price-history",
            scratch_dir=scratch,
            credentials={"alpha_vantage_api_key": "fixture"},
            settings=BootstrapSettings(
                allowed_categories=("equity",),
                enabled_provider_ids=("alpha_vantage",),
                allowed_network_hosts=("www.alphavantage.co",),
                provider_config={},
            ),
        )
    )
    path = (
        Path(__file__).resolve().parents[2]
        / "marketplace/plugins/openbb/connectors/alpha-vantage.market-data.json"
    )
    probe = json.loads(path.read_text())["runtime"]["verification_probe"]
    manifest_probe = next(
        p["verification_probe"]
        for p in load_runtime_manifest()["providers"]
        if p["id"] == "alpha_vantage"
    )
    tool = probe["tool_name"]
    assert (
        probe["arguments"] == manifest_probe["arguments"]
        and tool == manifest_probe["tool"]
    )

    async def run():
        nonlocal premium
        base = {"symbol": "AAPL", "provider": "alpha_vantage"}
        async with Client(mcp) as client:
            await client.call_tool("activate_tools", {"tool_names": [tool]})
            for params in [probe["arguments"], base]:
                result = await client.call_tool(tool, params)
                assert result.structured_content["results"][0]["close"] == 2
            adjusted = {**base, "adjustment": "splits_and_dividends"}
            for params in [
                adjusted,
                {**base, "interval": "1W", "adjustment": "unadjusted"},
            ]:
                refusal = await client.call_tool(tool, params, raise_on_error=False)
                assert refusal.is_error and refusal.structured_content == {
                    "loupist_error": {"kind": "alpha_vantage_paid_plan_required"}
                }
            # One burst notice is retried after a second; two in a row are a
            # rate limit, not a paid plan.
            started = asyncio.get_running_loop().time()
            bursts.append(burst)
            result = await client.call_tool(tool, base)
            assert result.structured_content["results"][0]["close"] == 2
            assert asyncio.get_running_loop().time() - started >= 1
            bursts.extend([burst, burst])
            refusal = await client.call_tool(tool, base, raise_on_error=False)
            assert refusal.is_error and refusal.structured_content == {
                "loupist_error": {"kind": "rate_limited", "provider": "alpha_vantage"}
            }
            premium = (
                "timestamp,open,high,low,close,adjusted_close,volume,dividend_amount,split_coefficient\n"
                f"{day},1,3,1,2,2,12,0,1\n"
            ).encode()
            result = await client.call_tool(tool, adjusted)
            assert result.structured_content["results"][0]["close"] == 2
        assert requests == [
            ("TIME_SERIES_DAILY", "compact"),
            ("TIME_SERIES_DAILY", "compact"),
            ("TIME_SERIES_DAILY_ADJUSTED", "full"),
            ("TIME_SERIES_DAILY", "full"),
            *[("TIME_SERIES_DAILY", "compact")] * 4,
            ("TIME_SERIES_DAILY_ADJUSTED", "full"),
        ]

    asyncio.run(run())


if __name__ == "__main__":
    check_daily_prices(Path(sys.argv[1]))
