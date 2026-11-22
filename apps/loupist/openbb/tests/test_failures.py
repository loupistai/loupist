from __future__ import annotations

import asyncio
import json
import sys
from pathlib import Path
from urllib.parse import parse_qs, urlsplit
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
    PAID_ALPHA_PLAN,
    INVALID_ARGUMENTS,
    FRED_INVALID_ARGUMENTS,
    FRED_INVALID_SYMBOL,
    InvalidProviderArguments,
    AlphaVantagePaidPlanRequired,
    CredentialRejectedError,
    credential_rejected,
    provider_refusal,
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
            "plan": AlphaVantagePaidPlanRequired(),
            "dns": socket.gaierror(
                -2, "private-hostname.example?api_key=private-canary"
            ),
            "connect": ConnectionRefusedError("private-canary"),
            "timeout": TimeoutError("private-canary"),
            "argument": ValueError("private-canary"),
            "rejected": InvalidProviderArguments(FRED_INVALID_SYMBOL),
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
        async with httpx.AsyncClient(
            transport=httpx.ASGITransport(app=isolated), base_url="http://fixture"
        ) as rest:
            invalid = await rest.get(
                "/api/v1/equity/price/quote", params={"provider": "fixture"}
            )
            assert invalid.status_code == 422
            assert invalid.json() == INVALID_ARGUMENTS
        async with Client(server) as client:
            await client.call_tool(
                "activate_tools", {"tool_names": ["equity_price_quote"]}
            )
            for failure in ["dns", "connect", "timeout", "plan"]:
                result = await client.call_tool(
                    "equity_price_quote",
                    {"provider": "fixture", "failure": failure},
                    raise_on_error=False,
                )
                assert result.is_error
                assert result.structured_content == (
                    PAID_ALPHA_PLAN if failure == "plan" else OFFLINE
                )
                assert "private-canary" not in str(result)
            error = await client.call_tool(
                "equity_price_quote",
                {"provider": "fixture", "failure": "argument"},
                raise_on_error=False,
            )
            assert error.is_error and error.structured_content != OFFLINE
            rejected = await client.call_tool(
                "equity_price_quote",
                {"provider": "fixture", "failure": "rejected"},
                raise_on_error=False,
            )
            assert rejected.is_error
            assert rejected.structured_content == FRED_INVALID_SYMBOL
            result = await client.call_tool(
                "equity_price_quote",
                {"provider": "fixture", "failure": "ok"},
            )
            assert not result.is_error
            assert result.structured_content["results"] == [{"price": 12}]

    asyncio.run(exercise())


class _ProviderResponse:
    def __init__(self, status: int, payload: object) -> None:
        self.status = status
        self.reason = "OK" if status == 200 else "Unauthorized"
        self.headers: dict[str, str] = {}
        self._payload = payload

    async def json(self) -> object:
        return self._payload


def _fred_series_with(
    monkeypatch: pytest.MonkeyPatch, status: int, payload: object, arguments=None
):
    from openbb_core.provider.utils import helpers as provider_helpers
    from openbb_fred.models.series import FredSeriesFetcher
    from openbb_fred.utils import rate_limiter

    from loupist_openbb.server import apply_fred_error_guard

    async def scripted(url: str, response_callback=None, **_kwargs):
        assert "private-canary" in url
        body = payload(url) if callable(payload) else payload
        return await response_callback(_ProviderResponse(status, body), None)

    monkeypatch.setattr(provider_helpers, "amake_request", scripted)
    monkeypatch.setattr(
        rate_limiter,
        "_check_payload_rate_limit",
        rate_limiter._check_payload_rate_limit,
    )
    monkeypatch.setattr(rate_limiter, "MIN_INTERVAL_SECONDS", 0)
    rate_limiter.cache_clear()
    apply_fred_error_guard()
    query = FredSeriesFetcher.transform_query(
        arguments or {"symbol": "GDP", "limit": 1}
    )
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
    assert provider_refusal(raised.value) == credential_rejected(reason)
    assert "private-canary" not in str(raised.value)
    assert "32 character" not in str(raised.value)


@pytest.mark.parametrize(
    ("status", "message", "expected"),
    [
        (400, "Bad Request.  The series does not exist.", FRED_INVALID_SYMBOL),
        (400, "Bad Request. private-canary", FRED_INVALID_ARGUMENTS),
        (500, "Server Error. private-canary", None),
    ],
)
def test_fred_request_errors_keep_their_category_without_provider_text(
    monkeypatch: pytest.MonkeyPatch,
    status: int,
    message: str,
    expected: dict | None,
) -> None:
    with pytest.raises((OpenBBError, InvalidProviderArguments)) as raised:
        _fred_series_with(
            monkeypatch,
            status,
            {"error_code": status, "error_message": message},
        )
    assert provider_refusal(raised.value) == expected
    assert "private-canary" not in str(raised.value)


@pytest.mark.parametrize("status", [200, 400])
def test_sec_error_is_classified_before_the_fetcher_reads_rows(monkeypatch, status):
    from openbb_core.provider.utils import helpers as provider_helpers
    from openbb_sec.utils import definitions, form4
    from loupist_openbb.server import apply_sec_contact_email

    response = _ProviderResponse(
        status,
        {
            "status": 400,
            "error": {"type": "parsing_exception", "reason": "private-canary"},
        },
    )

    async def scripted(_url, *, response_callback, **_kwargs):
        return await response_callback(response, None)

    monkeypatch.setattr(provider_helpers, "amake_request", scripted)
    # Restore all patched module attributes after using the real wrapper.
    with monkeypatch.context() as patches:
        for name, module in tuple(sys.modules.items()):
            for attr in ("amake_request", "make_request"):
                if (
                    name.startswith("openbb_sec.") or module is provider_helpers
                ) and hasattr(module, attr):
                    patches.setattr(module, attr, getattr(module, attr))
        for headers in (
            definitions.HEADERS,
            definitions.SEC_HEADERS,
            form4.SEC_HEADERS,
        ):
            patches.setitem(headers, "User-Agent", headers.get("User-Agent"))
        apply_sec_contact_email("fixture@example.test")
        with pytest.raises(InvalidProviderArguments) as raised:
            asyncio.run(
                provider_helpers.amake_request(
                    "https://efts.sec.gov/LATEST/search-index"
                )
            )
        assert provider_refusal(raised.value) == INVALID_ARGUMENTS
        assert "private-canary" not in str(raised.value)


def test_fred_bundled_probe_skips_the_missing_first_gdp_observation(monkeypatch):
    root = Path(__file__).resolve().parents[2]
    descriptor = root / "marketplace/plugins/openbb/connectors/fred.macro.json"
    arguments = json.loads(descriptor.read_text())["runtime"]["verification_probe"][
        "arguments"
    ]

    def response(url):
        if urlsplit(url).path == "/fred/series":
            return {"seriess": [{"title": "Gross Domestic Product"}]}
        query = parse_qs(urlsplit(url).query)
        rows = [("1946-01-01", "."), ("2024-01-01", "28793.191")]
        observations = [
            {"date": date, "value": value, "realtime_start": date, "realtime_end": date}
            for date, value in rows
            if query.get("observation_start", [""])[0]
            <= date
            <= query.get("observation_end", ["9999"])[0]
        ]
        return {"observations": observations[: int(query["limit"][0])]}

    results = _fred_series_with(monkeypatch, 200, response, arguments)
    assert results[0]["GDP"]["data"] == {"2024-01-01": 28793.191}


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


@pytest.mark.parametrize("authorized", [True, False])
def test_econdb_probe_checks_the_supplied_token_without_cache(monkeypatch, authorized):
    from openbb_core.provider.utils.errors import EmptyDataError
    from openbb_econdb.models.economic_indicators import EconDbEconomicIndicatorsFetcher
    from openbb_econdb.utils import helpers
    from loupist_openbb.manifest import load_runtime_manifest

    descriptor = (
        Path(__file__).resolve().parents[2]
        / "marketplace/plugins/openbb/connectors/econdb.macro.json"
    )
    probe = json.loads(descriptor.read_text())["runtime"]["verification_probe"]
    manifest = next(
        p for p in load_runtime_manifest()["providers"] if p["id"] == "econdb"
    )
    assert (
        probe["tool_name"]
        == manifest["verification_probe"]["tool"]
        == "economy_indicators"
    )
    assert probe["arguments"] == manifest["verification_probe"]["arguments"]
    record = {"ticker": "CPIUS", "data": {"dates": ["2024-01-01"], "values": [308.4]}}

    async def request(url, response_callback, **kwargs):
        query = parse_qs(urlsplit(url).query)
        assert query["ticker"] == ["[CPIUS]"]
        assert query["token"] == ["fixture"]
        assert "session" not in kwargs
        response = _ProviderResponse(
            200 if authorized else 401, {"results": [record]} if authorized else {}
        )
        return await response_callback(response, None)

    monkeypatch.setattr(helpers, "amake_request", request)
    query = EconDbEconomicIndicatorsFetcher.transform_query(probe["arguments"])
    result = EconDbEconomicIndicatorsFetcher.aextract_data(
        query, {"econdb_api_key": "fixture"}
    )
    if authorized:
        assert asyncio.run(result) == [record]
    else:
        with pytest.warns(UserWarning, match="401"), pytest.raises(EmptyDataError):
            asyncio.run(result)
