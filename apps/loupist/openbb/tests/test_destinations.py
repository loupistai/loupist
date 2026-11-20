"""Verify the advertised tools at the credential boundary without outbound I/O."""

import asyncio
import subprocess
import sys
from pathlib import Path
from unittest.mock import Mock

import pytest

from loupist_openbb.destinations import (
    InvalidDestination,
    checked_arguments,
    tool_destinations,
)
from loupist_openbb.network import validate_https_url

BILL = "https://api.congress.gov/v3/bill/118/hr/1"
INVALID_BILLS = [
    "https://api.stlouisfed.org/v3/bill/118/hr/1",
    "http://api.congress.gov/v3/bill/118/hr/1",
    "https://api.congress.gov@evil.example/v3/bill/118/hr/1",
    "https://user@api.congress.gov/v3/bill/118/hr/1",
    "https://api.congress.gov:444/v3/bill/118/hr/1",
    "https://api.congress.gov/redirect",
    BILL + "?next=elsewhere",
    "//api.congress.gov/v3/bill/118/hr/1",
    "118/hr/../1",
]


@pytest.mark.parametrize("value", INVALID_BILLS)
def test_rejected_bill_argument(value):
    with pytest.raises(InvalidDestination, match="invalid_destination"):
        checked_arguments("uscongress_bill_info", {"bill_url": value})


@pytest.mark.parametrize(
    "value",
    ["118/hr/1", BILL, BILL + "?format=json", BILL.replace(".gov/", ".gov:443/")],
)
def test_bill_identifier_is_canonical(value):
    assert checked_arguments("uscongress_bill_info", {"bill_url": value}) == {
        "bill_url": "118/hr/1"
    }


@pytest.mark.parametrize(
    ("tool", "field", "value"),
    [
        ("uscongress_bill_text_urls", "bill_url", INVALID_BILLS[0]),
        ("uscongress_amendment_info", "amendment_url", INVALID_BILLS[0]),
        ("uscongress_amendment_text_urls", "amendment_url", INVALID_BILLS[0]),
        ("uscongress_bill_text", "congress_gov", {"urls": [INVALID_BILLS[0]]}),
        ("uscongress_amendment_text", "congress_gov", [INVALID_BILLS[0]]),
        ("regulators_sec_filing_headers", "url", INVALID_BILLS[0]),
        ("regulators_sec_htm_file", "url", INVALID_BILLS[0]),
    ],
)
def test_other_user_selected_destinations_are_provider_bound(tool, field, value):
    with pytest.raises(InvalidDestination):
        checked_arguments(tool, {field: value})


@pytest.mark.parametrize(
    ("tool", "field", "value"),
    [
        ("uscongress_amendment_info", "amendment_url", "119/hamdt/2"),
        (
            "uscongress_bill_text",
            "congress_gov",
            {"urls": ["https://www.govinfo.gov/a.pdf"]},
        ),
        (
            "uscongress_amendment_text",
            "congress_gov",
            ["https://www.congress.gov/a.pdf"],
        ),
        ("regulators_sec_htm_file", "url", "https://www.sec.gov/Archives/filing.htm"),
    ],
)
def test_document_read_keeps_its_official_input(tool, field, value):
    arguments = {field: value}
    assert checked_arguments(tool, arguments) == arguments


def test_committee_supplement_and_news_filter_keep_their_own_destinations():
    hosts = frozenset({"api.congress.gov", "www.congress.gov", "api.tiingo.com"})
    search = "https://www.congress.gov/search?q=agriculture&page=1&pageSize=10"
    with tool_destinations("uscongress_committee_documents"):
        assert validate_https_url(search, hosts) == search
        with pytest.raises(PermissionError):
            validate_https_url(search + "&api_key=fixture", hosts)
    with tool_destinations("news_company", frozenset({"api.tiingo.com"})):
        assert validate_https_url("https://api.tiingo.com/", hosts)
        with pytest.raises(PermissionError):
            validate_https_url(BILL, hosts)


def test_real_mcp_bill_read_stays_on_its_api(tmp_path):
    subprocess.run([sys.executable, __file__, str(tmp_path)], check=True, timeout=90)


async def check_redirects():
    import urllib.request
    from contextlib import nullcontext
    from email.message import Message
    from io import BytesIO
    from urllib.response import addinfourl

    import aiohttp
    import httpx
    import requests

    for location in [BILL + "/next", "https://api.stlouisfed.org/next"]:
        same_origin = location.startswith(BILL)
        seen = []
        expected = [BILL, location] if same_origin else [BILL]

        def outcome():
            return nullcontext() if same_origin else pytest.raises(PermissionError)

        async def reply(request, handler):
            seen.append(str(request.url))
            return Mock(
                status=302 if len(seen) == 1 else 200,
                method="GET",
                headers={"Location": location},
                _raw_cookie_headers=(),
                connection=None,
            )

        async with aiohttp.ClientSession(middlewares=(reply,)) as client:
            with outcome():
                assert (await client.get(BILL)).status == 200
        assert seen == expected
        seen.clear()

        def response(request):
            seen.append(str(request.url))
            return httpx.Response(
                302 if len(seen) == 1 else 200, headers={"Location": location}
            )

        async with httpx.AsyncClient(
            transport=httpx.MockTransport(response), follow_redirects=True
        ) as client:
            with outcome():
                assert (await client.get(BILL)).status_code == 200
        assert seen == expected
        seen.clear()

        class RecordedAdapter(requests.adapters.BaseAdapter):
            def send(self, request, **kwargs):
                seen.append(request.url)
                response = requests.Response()
                response.status_code = 302 if len(seen) == 1 else 200
                response.url = request.url
                response.request = request
                response.headers["Location"] = location
                response._content = b""
                return response

            def close(self):
                pass

        with requests.Session() as session:
            session.trust_env = False
            session.mount("https://", RecordedAdapter())
            with outcome():
                assert session.get(BILL).status_code == 200
        assert seen == expected
        seen.clear()

        class RecordedHTTPS(urllib.request.HTTPSHandler):
            def https_open(self, request):
                seen.append(request.full_url)
                headers = Message()
                headers["Location"] = location
                response = addinfourl(
                    BytesIO(), headers, request.full_url, 302 if len(seen) == 1 else 200
                )
                response.msg = "recorded"
                return response

        opener = urllib.request.build_opener(RecordedHTTPS())
        with outcome():
            with opener.open(BILL) as response:
                assert response.status == 200
        assert seen == expected


def check_mcp(scratch):
    from loupist_openbb.bootstrap import (
        Bootstrap,
        BootstrapSettings,
        configure_scratch_environment,
    )

    configure_scratch_environment(scratch)
    from fastmcp import Client
    from openbb_core.provider.utils import helpers

    from loupist_openbb.manifest import load_runtime_manifest, resolve_network_hosts
    from loupist_openbb.server import create_server

    requested = []
    bill = {"title": "Lower Energy Costs Act", "introducedDate": "2023-03-14"}

    async def recorded(url, **kwargs):
        requested.append(url)
        assert url == BILL + "?format=json&api_key=fixture"
        return {"bill": bill.copy()}

    helpers.amake_request = recorded
    providers = {"congress_gov", "fred"}
    hosts = resolve_network_hosts(providers, load_runtime_manifest())
    server = create_server(
        Bootstrap(
            "test",
            scratch,
            {"congress_gov_api_key": "fixture"},
            BootstrapSettings(
                ("uscongress", "economy"), tuple(providers), tuple(hosts), {}
            ),
        )
    )

    async def check():
        await check_redirects()
        tool = "uscongress_bill_info"
        async with Client(server) as client:
            await client.call_tool("activate_tools", {"tool_names": [tool]})
            for value in ["118/hr/1", BILL]:
                result = await client.call_tool(
                    tool, {"provider": "congress_gov", "bill_url": value}
                )
                assert (
                    result.structured_content["results"]["raw_data"]["introducedDate"]
                    == "2023-03-14"
                )
            assert len(requested) == 2
            # A provider response can supply a follow-up link independently of
            # the caller's validated bill identifier.
            bill["subjects"] = {
                "count": 1,
                "url": "https://api.stlouisfed.org/subjects?format=json",
            }
            result = await client.call_tool(
                tool,
                {"provider": "congress_gov", "bill_url": "118/hr/1"},
                raise_on_error=False,
            )
            assert result.is_error
            assert requested == [BILL + "?format=json&api_key=fixture"] * 3
            requested.clear()
            for value in INVALID_BILLS:
                result = await client.call_tool(
                    tool,
                    {"provider": "congress_gov", "bill_url": value},
                    raise_on_error=False,
                )
                assert result.is_error
                assert result.structured_content == {
                    "loupist_error": {
                        "kind": "invalid_destination",
                        "field": "bill_url",
                    }
                }
            assert not requested

    asyncio.run(check())


if __name__ == "__main__":
    check_mcp(Path(sys.argv[1]))
