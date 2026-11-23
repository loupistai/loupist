#!/usr/bin/env python3
"""Opt-in live replay of the bundled source sidecar's unconfigured inventory."""

from __future__ import annotations

import argparse
import asyncio
import json
import math
import runpy
import sys
import tempfile
from collections import Counter, defaultdict
from contextlib import suppress
from datetime import date, timedelta
from pathlib import Path
from typing import Any

SCRIPTS = Path(__file__).resolve().parent
APP = SCRIPTS.parent

WINDOWED_TOOLS = {
    "crypto_price_historical",
    "currency_price_historical",
    "derivatives_futures_historical",
    "equity_price_historical",
    "etf_historical",
    "index_price_historical",
    "economy_calendar",
    "equity_calendar_dividend",
    "equity_calendar_earnings",
    "equity_calendar_ipo",
}
# Use documented minimum inputs where there is no natural equity symbol.
TOOL_INPUTS = {
    "cftc_cot": {"code": "067651", "limit": 3},
    "cftc_cot_search": {"query": "CRUDE OIL"},
    "commodity_psd_report": {"commodity": "wheat", "year": 2024, "month": 10},
    "commodity_weather_bulletins": {"year": 2025, "month": 10, "week": 1},
    "crypto_price_historical": {"symbol": "BTC-USD"},
    "currency_price_historical": {"symbol": "EURUSD=X"},
    "economy_available_indicators": {"query": "inflation", "dataflows": "CPI"},
    "economy_indicators": {"symbol": "CPI", "country": "USA", "limit": 12},
    "economy_survey_bls_search": {"query": "unemployment"},
    "equity_fundamental_dividends": {
        "start_date": "2025-01-01",
        "end_date": "2025-12-31",
    },
    "equity_search": {"query": "Microsoft"},
    "etf_search": {"query": "XIU"},
    "imf_utils_get_dataflow_dimensions": {"dataflow_id": "CPI"},
    "imf_utils_presentation_table": {
        "dataflow_group": "cpi",
        "table": "cpi",
        "country": "JPN",
        "frequency": "M",
    },
    "index_search": {"query": "S&P 500"},
    "news_company": {"limit": 5},
}
PROVIDER_INPUTS = {
    ("derivatives_futures_curve", "cboe"): {"symbol": "VX_EOD"},
    ("derivatives_futures_curve", "deribit"): {"symbol": "BTC"},
    ("derivatives_futures_curve", "yfinance"): {"symbol": "CL"},
    ("derivatives_futures_historical", "deribit"): {"symbol": "BTC-PERPETUAL"},
    ("derivatives_futures_historical", "yfinance"): {"symbol": "CL=F"},
    ("derivatives_futures_info", "deribit"): {"symbol": "BTC-PERPETUAL"},
    ("derivatives_options_chains", "deribit"): {"symbol": "BTC"},
    ("equity_search", "tmx"): {"query": "Royal Bank"},
}
# Keep the original fixtures available for before-and-after comparison.
PROVIDER_SPECIFIC_INPUTS = {
    ("commodity_psd_report", "government_us"): {"commodity": "grain"},
    ("economy_balance_of_payments", "ecb"): {"country": "united_states"},
    ("economy_direction_of_trade", "imf"): {"country": "USA", "counterpart": "CAN"},
    ("economy_indicators", "imf"): {
        "symbol": "CPI::H_CPI_BY_COMPONENT",
        "frequency": "month",
    },
    ("economy_survey_bls_search", "bls"): {"category": "lfs"},
    ("index_constituents", "cboe"): {"symbol": "BUK100P"},
    ("index_constituents", "tmx"): {"symbol": "^TSX"},
    ("index_sectors", "tmx"): {"symbol": "^TSX"},
}


def arguments_for(
    card: dict[str, Any], provider: str, as_of: date, original: bool
) -> dict[str, Any]:
    tool = card["name"]
    schema = card["inputSchema"]
    properties = schema.get("properties", {})
    arguments = {"provider": provider} if "provider" in properties else {}
    if "symbol" in properties and tool not in {
        "economy_available_indicators",
        "equity_calendar_ipo",
    }:
        symbol = "RY" if provider == "tmx" else "MSFT"
        if tool.startswith("etf_"):
            symbol = "XIU" if provider == "tmx" else "SPY"
        if tool.startswith("index_"):
            symbol = {"tmx": "TSX", "yfinance": "^GSPC"}.get(provider, "SPX")
        arguments["symbol"] = symbol
    if tool in WINDOWED_TOOLS:
        arguments["start_date"] = (as_of - timedelta(days=23)).isoformat()
        arguments["end_date"] = (as_of - timedelta(days=3)).isoformat()
    if tool.startswith("famafrench_"):
        arguments.update(start_date="2023-01-01", end_date="2024-01-01")
    if tool == "quantitative_capm":
        arguments["target"] = "asset"
        arguments["data"] = [
            {
                "date": (date(2022, 1, 1) + timedelta(days=31 * i)).isoformat(),
                "asset": 0.003 + 0.007 * math.sin(0.7 * i),
            }
            for i in range(36)
        ]
    arguments.update(TOOL_INPUTS.get(tool, {}))
    arguments.update(PROVIDER_INPUTS.get((tool, provider), {}))
    if not original:
        arguments.update(PROVIDER_SPECIFIC_INPUTS.get((tool, provider), {}))
    if missing := set(schema.get("required", ())) - arguments.keys():
        raise ValueError(
            f"{tool}/{provider} requires replay fixtures for {sorted(missing)}"
        )
    return arguments


class StdioSession:
    def __init__(self, process: asyncio.subprocess.Process):
        self.process = process
        self.pending: dict[int, asyncio.Future] = {}
        self.sequence = 0
        self.read_error: RuntimeError | None = None

    async def read(self) -> None:
        assert self.process.stdout is not None
        try:
            while line := await self.process.stdout.readline():
                try:
                    message = json.loads(line)
                except json.JSONDecodeError:
                    # A dependency may print a status line outside the MCP stream.
                    continue
                if not isinstance(message, dict):
                    continue
                request = self.pending.get(message.get("id"))
                if request is not None and not request.done():
                    request.set_result(message)
        except (OSError, ValueError) as error:
            self.read_error = RuntimeError(f"OpenBB stdout read failed: {error}")
        else:
            self.read_error = RuntimeError("OpenBB closed its stdout")
        for request in self.pending.values():
            if not request.done():
                request.set_exception(self.read_error)

    async def send(
        self, method: str, params: dict, request_id: int | None = None
    ) -> None:
        assert self.process.stdin is not None
        frame = {"jsonrpc": "2.0", "method": method, "params": params}
        if request_id is not None:
            frame["id"] = request_id
        await self.write(frame)

    async def write(self, frame: dict) -> None:
        assert self.process.stdin is not None
        self.process.stdin.write((json.dumps(frame) + "\n").encode())
        await self.process.stdin.drain()

    async def request(self, method: str, params: dict, timeout: float = 120) -> dict:
        if self.read_error is not None:
            raise self.read_error
        self.sequence += 1
        request_id = self.sequence
        pending = asyncio.get_running_loop().create_future()
        self.pending[request_id] = pending
        try:
            await self.send(method, params, request_id)
            return await asyncio.wait_for(pending, timeout)
        except TimeoutError:
            await self.send(
                "notifications/cancelled",
                {"requestId": request_id, "reason": "audit request deadline"},
            )
            raise
        finally:
            self.pending.pop(request_id)

    async def tools(self, settings: dict, scratch: str, protocol: str) -> list[dict]:
        await self.write(
            {
                "type": "loupist.bootstrap",
                "protocol_version": 1,
                "run_id": "keyless-replay",
                "scratch_dir": scratch,
                "credentials": {},
                "settings": settings,
            }
        )
        initialization = await self.request(
            "initialize",
            {
                "protocolVersion": protocol,
                "capabilities": {},
                "clientInfo": {"name": "keyless audit", "version": "1"},
            },
        )
        if "error" in initialization:
            raise RuntimeError("OpenBB failed to initialize")
        await self.send("notifications/initialized", {})
        categories = await self.call("available_categories", {})
        for category in categories["result"]["structuredContent"]["result"]:
            activated = await self.call(
                "activate_category", {"category": category["name"]}
            )
            if activated["result"].get("isError"):
                raise RuntimeError(f"Could not activate {category['name']}")
        tools = []
        params = {}
        while True:
            result = (await self.request("tools/list", params))["result"]
            tools.extend(result["tools"])
            if not (cursor := result.get("nextCursor")):
                return tools
            params = {"cursor": cursor}

    async def call(self, name: str, arguments: dict, timeout: float = 120) -> dict:
        return await self.request(
            "tools/call", {"name": name, "arguments": arguments}, timeout
        )


def outcome(response: dict) -> dict:
    result = response.get("result", {})
    structured = result.get("structuredContent") or {}
    if result.get("isError") or "error" in response:
        if marker := structured.get("loupist_error"):
            return {"status": marker["kind"], "field": marker.get("field")}
        # Do not place exception messages, URLs, or data bodies in the report.
        text = " ".join(
            part.get("text", "") for part in result.get("content", ())
        ).lower()
        if "output validation error" in text:
            return {"status": "output_validation"}
        if (
            "validation error" in text
            or response.get("error", {}).get("code") == -32602
        ):
            return {"status": "invalid_arguments"}
        return {"status": "masked_provider_error"}
    data = structured.get("results", structured.get("result", structured))
    count = len(data) if isinstance(data, (dict, list, str)) else None
    if data is None or count == 0:
        return {"status": "empty_result", "row_count": count}
    return {"status": "ok", "row_count": count}


async def replay(options: argparse.Namespace) -> None:
    smoke = runpy.run_path(str(SCRIPTS / "check-sidecars.py"))
    manifest = json.loads((APP / "openbb/runtime-manifest.json").read_text())
    keyless = {
        provider["id"]: provider
        for provider in manifest["providers"]
        if provider["keyless"] and not provider.get("config_mapping")
    }
    policy = manifest["providerless_tool_policy"]
    env = smoke["openbb_environment"]()
    env["PYTHONPATH"] = str(APP / "openbb/src")
    settings = {
        "allowed_categories": manifest["allowed_categories"],
        "enabled_provider_ids": sorted(keyless),
        "allowed_network_hosts": sorted(
            {host for p in keyless.values() for host in p["network_hosts"]}
        ),
        "provider_config": {},
    }
    with tempfile.TemporaryDirectory(prefix="loupist-keyless-replay-") as scratch:
        process = await asyncio.create_subprocess_exec(
            sys.executable,
            "-c",
            "from loupist_openbb.server import main; raise SystemExit(main())",
            cwd=scratch,
            env=env,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            # Long series appear both in structured JSON and in MCP's text
            # content. This source audit does not exercise the host frame cap.
            limit=256 * 1024 * 1024,
        )
        session = StdioSession(process)
        reader = asyncio.create_task(session.read())
        try:
            cards = await session.tools(
                settings, scratch, smoke["MCP_PROTOCOL_VERSION"]
            )
            await replay_cards(session, cards, policy, keyless.keys(), options)
        finally:
            if process.returncode is None:
                with suppress(ProcessLookupError):
                    process.kill()
            await process.wait()
            reader.cancel()
            with suppress(asyncio.CancelledError):
                await reader


async def replay_cards(
    session: StdioSession,
    cards: list[dict],
    policy: dict,
    keyless: Any,
    options: argparse.Namespace,
) -> None:
    batches = defaultdict(list)
    alternatives = {}
    local = 0
    for card in sorted(cards, key=lambda card: card["name"]):
        name = card["name"]
        if name in policy["local_tools"]:
            local += 1
            continue
        provider = card["inputSchema"].get("properties", {}).get("provider")
        implicit = policy["implicit_provider"].get(name)
        choices = provider["enum"] if provider else ([implicit] if implicit else [])
        alternatives[name] = choices
        for choice in choices:
            if choice not in keyless:
                raise RuntimeError(f"Unconfigured discovery exposed {choice}")
            batches[choice].append(
                (
                    name,
                    arguments_for(card, choice, options.as_of, options.original_inputs),
                )
            )
    totals = Counter()
    slots = asyncio.Semaphore(5)

    async def run_batch(provider: str, batch: list[tuple[str, dict]]) -> None:
        # Keep each provider sequential and bound concurrency across providers.
        async with slots:
            for name, arguments in batch:
                try:
                    result = outcome(
                        await session.call(name, arguments, options.timeout)
                    )
                except TimeoutError:
                    result = {"status": "timeout"}
                totals[result["status"]] += 1
                if result["status"] == "masked_provider_error":
                    result["alternatives"] = [
                        choice for choice in alternatives[name] if choice != provider
                    ]
                print(
                    json.dumps({"tool": name, "provider": provider, **result}),
                    flush=True,
                )

    await asyncio.gather(
        *(
            run_batch(provider, batch)
            for provider, batch in sorted(
                batches.items(), key=lambda item: -len(item[1])
            )
        )
    )
    print(json.dumps({"totals": totals, "pairs": sum(totals.values()), "local": local}))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--allow-network",
        action="store_true",
        help="Permit calls to bundled public provider endpoints",
    )
    parser.add_argument(
        "--original-inputs",
        action="store_true",
        help="Use the v0.1.4 audit inputs, including known invalid arguments",
    )
    parser.add_argument(
        "--as-of",
        type=date.fromisoformat,
        default=date.today(),
        help="End reference for bounded fixture dates (YYYY-MM-DD)",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=35,
        help="Per-call deadline in seconds (default: 35)",
    )
    options = parser.parse_args()
    if not options.allow_network:
        parser.error("live replay requires --allow-network")
    if options.timeout <= 0:
        parser.error("--timeout must be positive")
    asyncio.run(replay(options))


if __name__ == "__main__":
    main()
