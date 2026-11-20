"""Validate caller-selected destinations before OpenBB receives credentials."""

import re
from collections.abc import Awaitable, Callable, Iterator
from contextlib import contextmanager
from contextvars import ContextVar
from functools import wraps
from typing import Any
from urllib.parse import SplitResult, parse_qs, urlsplit

CONGRESS_API = frozenset({"api.congress.gov"})
SEC_FILINGS = frozenset({"www.sec.gov", "sec.gov"})
BILL_DOCUMENTS = frozenset({"www.congress.gov", "www.govinfo.gov"})
AMENDMENT_DOCUMENTS = frozenset({"www.congress.gov"})
_MEMBERSHIP = "https://unitedstates.github.io/congress-legislators/committee-membership-current.json"
_RESOURCE = {
    "uscongress_bill_info": "bill",
    "uscongress_bill_text_urls": "bill",
    "uscongress_amendment_info": "amendment",
    "uscongress_amendment_text_urls": "amendment",
}
_TYPES = {
    "bill": r"(?:hr|s|hjres|sjres|hconres|sconres|hres|sres)",
    "amendment": r"(?:hamdt|samdt|suamdt)",
}
_DOCUMENTS = {
    "uscongress_bill_text": BILL_DOCUMENTS,
    "uscongress_amendment_text": AMENDMENT_DOCUMENTS,
}
_SEC = {"regulators_sec_filing_headers", "regulators_sec_htm_file"}
_ACTIVE_TOOL: ContextVar[tuple[str, frozenset[str] | None]] = ContextVar(
    "destination_checked_tool", default=("", None)
)


class InvalidDestination(ValueError):
    def __init__(self, field: str):
        super().__init__("invalid_destination")
        self.field = field


def _url(value: object, hosts: frozenset[str], field: str) -> SplitResult:
    if not isinstance(value, str) or any(
        c.isspace() or ord(c) < 32 or ord(c) == 127 or c == "\\" for c in value
    ):
        raise InvalidDestination(field)
    try:
        parsed = urlsplit(value)
        # Compare the complete authority. Only an exact host, optionally with
        # the default port, can proceed to another URL parser.
        if (
            parsed.scheme == "https"
            and parsed.netloc.lower().removesuffix(":443") in hosts
        ):
            return parsed
    except ValueError:
        pass
    raise InvalidDestination(field)


def checked_arguments(tool: str, args: dict[str, Any]) -> dict[str, Any]:
    if resource := _RESOURCE.get(tool):
        field = resource + "_url"
        value = args.get(field)
        if not isinstance(value, str):
            raise InvalidDestination(field)
        identifier = value.removeprefix("/")
        if "://" in value:
            parsed = _url(value, CONGRESS_API, field)
            prefix = f"/v3/{resource}/"
            if (
                not parsed.path.startswith(prefix)
                or parsed.query not in ("", "format=json")
                or parsed.fragment
            ):
                raise InvalidDestination(field)
            identifier = parsed.path[len(prefix) :]
        if not re.fullmatch(
            rf"[0-9]+/{_TYPES[resource]}/[0-9]+",
            identifier,
            flags=re.ASCII | re.IGNORECASE,
        ):
            raise InvalidDestination(field)
        # Upstream constructs its own fixed API URL from this identifier.
        return {**args, field: identifier.lower()}
    if hosts := _DOCUMENTS.get(tool):
        field = "congress_gov"
        value = args.get(field)
        if isinstance(value, dict):
            if set(value) != {"urls"}:
                raise InvalidDestination(field)
            value = value["urls"]
        if isinstance(value, str):
            value = value.split(",")
        if not isinstance(value, list) or not value:
            raise InvalidDestination(field)
        for url in value:
            _url(url, hosts, field)
    if tool in _SEC:
        _url(args.get("url"), SEC_FILINGS, "url")
    return args


@contextmanager
def tool_destinations(
    tool: str, provider_hosts: frozenset[str] | None = None
) -> Iterator[None]:
    token = _ACTIVE_TOOL.set((tool, provider_hosts))
    try:
        yield
    finally:
        _ACTIVE_TOOL.reset(token)


def guard_tool_destinations(
    tool: Any,
    run: Callable[[dict[str, Any]], Awaitable[Any]],
    implicit_provider: str | None,
    provider_hosts: dict[str, frozenset[str]] | None,
) -> Callable[[dict[str, Any]], Awaitable[Any]]:
    from fastmcp.tools.tool import ToolResult

    name = tool.name
    schema = tool.parameters.get("properties", {}).get("provider", {})
    default = implicit_provider or schema.get("const") or schema.get("default")

    @wraps(run)
    async def guarded(arguments: dict[str, Any]) -> Any:
        try:
            arguments = checked_arguments(name, arguments)
        except InvalidDestination as error:
            return ToolResult(
                content="invalid_destination",
                structured_content={
                    "loupist_error": {
                        "kind": "invalid_destination",
                        "field": error.field,
                    }
                },
                is_error=True,
            )
        provider = arguments.get("provider") or default
        hosts = (
            (provider_hosts or {}).get(provider) if isinstance(provider, str) else None
        )
        with tool_destinations(name, hosts):
            return await run(arguments)

    return guarded


def check_tool_destination(url: str, host: str) -> None:
    tool, hosts = _ACTIVE_TOOL.get()
    if tool in _SEC:
        hosts = SEC_FILINGS
    elif tool.startswith("uscongress_"):
        if tool == "uscongress_committee_info" and url == _MEMBERSHIP:
            return
        if (
            tool
            in {"uscongress_committee_documents", "uscongress_committee_document_urls"}
            and host == "www.congress.gov"
        ):
            parsed = urlsplit(url)
            if (
                parsed.path in {"/", "/search"}
                and not parsed.fragment
                and set(parse_qs(parsed.query, keep_blank_values=True))
                <= {"q", "pageSize", "page"}
            ):
                return
        hosts = _DOCUMENTS.get(tool, CONGRESS_API)
    if hosts is not None and host not in hosts:
        raise PermissionError("Request outside the tool's provider origins")
