from __future__ import annotations

import asyncio
import socket
from urllib.error import URLError

import aiohttp
import httpx
import pytest
import requests
from curl_cffi import CurlError
from curl_cffi.const import CurlECode
from fastapi import FastAPI
from fastmcp import Client
from openbb_core.app.model.abstract.error import OpenBBError
from openbb_core.api.app_loader import AppLoader
from openbb_mcp_server.app.app import create_mcp_server
from openbb_mcp_server.models.settings import MCPSettings

from loupist_openbb.failures import OFFLINE, is_offline_error
from loupist_openbb.server import (
    mark_registered_tools_read_only,
    retain_read_only_routes,
)


@pytest.mark.parametrize(
    "error",
    [
        socket.gaierror(-2, "private hostname"),
        ConnectionRefusedError("private address"),
        TimeoutError("private URL"),
        requests.exceptions.ConnectTimeout("private URL"),
        requests.exceptions.ConnectionError("private URL"),
        aiohttp.ClientConnectionError("private URL"),
        httpx.ConnectError("private URL"),
        httpx.ReadTimeout("private URL"),
        URLError(socket.gaierror(-2, "private hostname")),
        CurlError("private URL", CurlECode.COULDNT_RESOLVE_HOST),
        CurlError("private URL", CurlECode.COULDNT_CONNECT),
        CurlError("private URL", CurlECode.OPERATION_TIMEDOUT),
    ],
)
def test_typed_transport_errors_survive_provider_wrapping(error: Exception) -> None:
    assert is_offline_error(OpenBBError(error))


@pytest.mark.parametrize(
    "error",
    [
        ValueError("DNS failed, timed out"),
        PermissionError("connection denied"),
        requests.exceptions.HTTPError("HTTP 429"),
        requests.exceptions.SSLError("certificate"),
        CurlError("certificate", CurlECode.PEER_FAILED_VERIFICATION),
        httpx.HTTPStatusError(
            "HTTP 503",
            request=httpx.Request("GET", "https://fixture.example"),
            response=httpx.Response(503, json={"error": "unavailable"}),
        ),
    ],
)
def test_reachable_errors_and_provider_text_are_not_offline(error: Exception) -> None:
    assert not is_offline_error(error)


def test_network_failure_crosses_openbb_rest_and_mcp_without_private_text() -> None:
    app = FastAPI()
    AppLoader.add_exception_handlers(app)

    @app.get("/api/v1/equity/price/quote")
    async def quote(provider: str, failure: str) -> dict:
        if failure == "ok":
            return {"provider": provider, "results": [{"price": 12}]}
        errors = {
            "dns": socket.gaierror(
                -2, "private-hostname.example?api_key=private-canary"
            ),
            "connect": ConnectionRefusedError("private-canary"),
            "timeout": TimeoutError("private-canary"),
            "argument": ValueError("private-canary"),
        }
        raise OpenBBError(errors[failure]) from errors[failure]

    isolated = retain_read_only_routes(app, {"equity"}, "/api/v1")
    settings = MCPSettings(
        name="Offline contract",
        default_tool_categories=[],
        allowed_tool_categories=["equity"],
        enable_tool_discovery=True,
        describe_responses=False,
        mask_error_details=True,
        instructions=None,
        system_prompt_file=None,
        server_prompts_file=None,
        default_skills_dir=None,
        skills_providers=None,
        dependencies=None,
    )
    server = create_mcp_server(settings, isolated)
    mark_registered_tools_read_only(server, {})

    async def exercise() -> None:
        async with Client(server) as client:
            await client.call_tool(
                "activate_tools", {"tool_names": ["equity_price_quote"]}
            )
            for failure in ["dns", "connect", "timeout"]:
                result = await client.call_tool(
                    "equity_price_quote",
                    {"provider": "fixture", "failure": failure},
                    raise_on_error=False,
                )
                assert result.is_error
                assert result.structured_content == OFFLINE
                assert "private-canary" not in str(result)
            error = await client.call_tool(
                "equity_price_quote",
                {"provider": "fixture", "failure": "argument"},
                raise_on_error=False,
            )
            assert error.is_error and error.structured_content != OFFLINE
            result = await client.call_tool(
                "equity_price_quote",
                {"provider": "fixture", "failure": "ok"},
            )
            assert not result.is_error
            assert result.structured_content["results"] == [{"price": 12}]

    asyncio.run(exercise())
