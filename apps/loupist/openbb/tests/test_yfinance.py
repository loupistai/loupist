"""Exercise the pinned Yahoo fetcher and the MCP history route without network."""

import asyncio
from pathlib import Path
import subprocess
import sys


def test_yahoo_daily_chart_close_and_split_boundary(tmp_path):
    subprocess.run([sys.executable, __file__, str(tmp_path)], check=True, timeout=90)


def exercise(scratch):
    from loupist_openbb.bootstrap import (
        Bootstrap,
        BootstrapSettings,
        configure_scratch_environment,
    )

    configure_scratch_environment(scratch)
    from fastmcp import Client
    import pandas as pd
    import yfinance as yf
    from loupist_openbb.manifest import load_runtime_manifest, resolve_network_hosts
    from loupist_openbb.server import create_server

    # Yahoo's chart close on the requested day differs from its dividend-adjusted close.
    dates = pd.to_datetime(["2024-01-02", "2024-01-03"])
    dates.name = "Date"
    bars = pd.DataFrame(
        {
            "Open": [187.15, 184.22],
            "High": [188.44, 185.88],
            "Low": [183.89, 183.43],
            "Close": [185.64, 184.25],
            "Adj Close": [183.75, 182.38],
            "Volume": [82488700, 58414500],
            "Stock Splits": [0.0, 0.0],
            "Dividends": [0.0, 0.0],
        },
        index=dates,
    )

    def download(**kwargs):
        assert kwargs["actions"] and not kwargs["auto_adjust"] and kwargs["end"] is None
        data = bars.copy()
        data.columns = pd.MultiIndex.from_product(
            [["AAPL"], data.columns], names=["Ticker", "Price"]
        )
        return data

    yf.download = download
    hosts = resolve_network_hosts({"yfinance"}, load_runtime_manifest())
    server = create_server(
        Bootstrap(
            "yahoo-prices",
            scratch,
            {},
            BootstrapSettings(
                ("equity",),
                ("yfinance",),
                tuple(hosts),
                {},
            ),
        )
    )

    async def check():
        tool = "equity_price_historical"
        query = {
            "provider": "yfinance",
            "symbol": "AAPL",
            "start_date": "2024-01-02",
            "end_date": "2024-01-02",
            "interval": "1d",
            "adjustment": "unadjusted",
        }
        async with Client(server) as client:
            await client.call_tool("activate_tools", {"tool_names": [tool]})
            card = next(t for t in await client.list_tools() if t.name == tool)
            assert (
                "unadjusted"
                in card.inputSchema["properties"]["adjustment"]["yfinance"]["choices"]
            )
            result = await client.call_tool(tool, query, raise_on_error=False)
            assert not result.is_error, result
            assert [
                (r["date"], r["close"]) for r in result.structured_content["results"]
            ] == [("2024-01-02", 185.64)]
            # After a later split, the same chart Close is on a different share basis.
            bars.loc[dates[1], "Stock Splits"] = 4.0
            for params in [query, {**query, "interval": "1W"}]:
                refusal = await client.call_tool(tool, params, raise_on_error=False)
                assert refusal.is_error
                assert refusal.structured_content["loupist_error"]["supported"] == [
                    "splits_only",
                    "splits_and_dividends",
                ]
            bars.drop(columns="Stock Splits", inplace=True)
            missing = await client.call_tool(tool, query, raise_on_error=False)
            assert (
                missing.is_error
                and missing.structured_content["loupist_error"]["kind"]
                == "unsupported_adjustment"
            )
            supported = await client.call_tool(
                tool, {**query, "adjustment": "splits_only"}
            )
            assert supported.structured_content["results"][0]["close"] == 185.64

    asyncio.run(check())


if __name__ == "__main__":
    exercise(Path(sys.argv[1]))
