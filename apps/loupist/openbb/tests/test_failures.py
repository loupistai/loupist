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

from loupist_openbb.failures import (
    OFFLINE,
    CredentialRejectedError,
    credential_rejected,
    credential_rejection,
    is_offline_error,
)
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


class _FredResponse:
    def __init__(self, status: int, payload: object) -> None:
        self.status = status
        self.headers: dict[str, str] = {}
        self._payload = payload

    async def json(self) -> object:
        return self._payload


def _fred_series_with(monkeypatch: pytest.MonkeyPatch, status: int, payload: object):
    from openbb_core.provider.utils import helpers as provider_helpers
    from openbb_fred.models.series import FredSeriesFetcher
    from openbb_fred.utils import rate_limiter

    from loupist_openbb.server import apply_fred_error_guard

    async def scripted(url: str, response_callback=None, **_kwargs):
        assert "private-canary" in url
        return await response_callback(_FredResponse(status, payload), None)

    monkeypatch.setattr(provider_helpers, "amake_request", scripted)
    monkeypatch.setattr(
        rate_limiter,
        "_check_payload_rate_limit",
        rate_limiter._check_payload_rate_limit,
    )
    monkeypatch.setattr(rate_limiter, "MIN_INTERVAL_SECONDS", 0)
    rate_limiter.cache_clear()
    apply_fred_error_guard()
    query = FredSeriesFetcher.transform_query({"symbol": "GDP", "limit": 1})
    return asyncio.run(
        FredSeriesFetcher.aextract_data(query, {"fred_api_key": "private-canary"})
    )


@pytest.mark.parametrize(
    ("status", "message", "reason"),
    [
        (
            400,
            "Bad Request.  The value for variable api_key is not a 32 character "
            "alpha-numeric lower-case string.",
            "key_format",
        ),
        (
            400,
            "Bad Request.  The value for variable api_key is not registered.",
            "key_not_accepted",
        ),
        (
            403,
            "Forbidden.  The value for variable api_key is invalid.",
            "key_not_accepted",
        ),
    ],
)
def test_fred_key_errors_fail_with_a_fixed_reason_instead_of_an_empty_series(
    monkeypatch: pytest.MonkeyPatch, status: int, message: str, reason: str
) -> None:
    with pytest.raises(Exception) as raised:
        _fred_series_with(
            monkeypatch, status, {"error_code": status, "error_message": message}
        )
    assert credential_rejection(raised.value) == credential_rejected(reason)
    assert "private-canary" not in str(raised.value)
    assert "32 character" not in str(raised.value)


def test_fred_other_errors_fail_without_provider_text(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    with pytest.raises(OpenBBError) as raised:
        _fred_series_with(
            monkeypatch,
            400,
            {"error_code": 400, "error_message": "Bad Request. private-canary"},
        )
    assert credential_rejection(raised.value) is None
    assert "private-canary" not in str(raised.value)


def test_fred_accepted_key_still_returns_observations(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    payload = {
        "observations": [
            {
                "date": "1947-01-01",
                "value": "243.164",
                "realtime_start": "1947-01-01",
                "realtime_end": "1947-01-01",
            }
        ],
        "seriess": [{"title": "Gross Domestic Product"}],
    }
    results = _fred_series_with(monkeypatch, 200, payload)
    assert results and results[0]["GDP"]["data"]


def test_key_rejection_crosses_openbb_rest_and_mcp_as_a_fixed_reason() -> None:
    app = FastAPI()
    AppLoader.add_exception_handlers(app)

    @app.get("/api/v1/economy/fred_series")
    async def series(provider: str, reason: str) -> dict:
        try:
            raise CredentialRejectedError(reason)
        except CredentialRejectedError as error:
            # FRED's fetcher re-wraps foreign errors with their text.
            raise OpenBBError("private-canary " + str(error)) from error

    isolated = retain_read_only_routes(app, {"economy"}, "/api/v1")
    settings = MCPSettings(
        name="Rejection contract",
        default_tool_categories=[],
        allowed_tool_categories=["economy"],
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
                "activate_tools", {"tool_names": ["economy_fred_series"]}
            )
            for reason in ["key_format", "key_not_accepted"]:
                result = await client.call_tool(
                    "economy_fred_series",
                    {"provider": "fred", "reason": reason},
                    raise_on_error=False,
                )
                assert result.is_error
                assert result.structured_content == credential_rejected(reason)
                assert "private-canary" not in str(result)

    asyncio.run(exercise())


def test_rejection_reasons_are_a_closed_set() -> None:
    with pytest.raises(ValueError):
        CredentialRejectedError("provider said private-canary")
