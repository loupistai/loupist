"""Typed provider failures across OpenBB's REST and MCP boundaries.

Only exception types and our private REST markers are inspected. Provider text,
URLs and credentials never enter the MCP diagnostic: an unreachable provider is
``offline`` and a provider that refuses the configured key is
``credential_rejected`` with one fixed reason code.
"""

from __future__ import annotations

import socket
from collections.abc import Callable
from typing import Any
from urllib.error import URLError

OFFLINE = {"loupist_error": {"kind": "offline"}}
CREDENTIAL_REASONS = frozenset({"key_format", "key_not_accepted"})


def credential_rejected(reason: str) -> dict[str, Any]:
    if reason not in CREDENTIAL_REASONS:
        raise ValueError("unknown credential rejection reason")
    return {"loupist_error": {"kind": "credential_rejected", "reason": reason}}


class CredentialRejectedError(Exception):
    """The provider answered and refused the configured key.

    Carries only a fixed reason code, never the provider's text or the key.
    """

    def __init__(self, reason: str) -> None:
        credential_rejected(reason)
        super().__init__("The provider rejected the configured key.")
        self.reason = reason


def _causes(error: BaseException) -> list[BaseException]:
    causes: list[BaseException] = []
    original = getattr(error, "original", None)
    if isinstance(original, BaseException):
        causes.append(original)
    causes.extend(
        cause for cause in (error.__cause__, error.__context__) if cause is not None
    )
    # OpenBB can wrap an exception as an argument without an explicit cause.
    causes.extend(arg for arg in error.args if isinstance(arg, BaseException))
    return causes


def credential_rejection(error: BaseException) -> dict[str, Any] | None:
    """Return the fixed MCP diagnostic when ``error`` is a key rejection."""

    import httpx

    pending = [error]
    seen: set[int] = set()
    while pending and len(seen) < 32:
        current = pending.pop()
        if id(current) in seen:
            continue
        seen.add(id(current))
        if isinstance(current, CredentialRejectedError):
            return credential_rejected(current.reason)
        if isinstance(current, httpx.HTTPStatusError):
            try:
                body = current.response.json()
            except ValueError:
                body = None
            marker = body.get("loupist_error") if isinstance(body, dict) else None
            if (
                isinstance(marker, dict)
                and marker.get("kind") == "credential_rejected"
                and marker.get("reason") in CREDENTIAL_REASONS
            ):
                return credential_rejected(marker["reason"])
        pending.extend(_causes(current))
    return None


def is_offline_error(error: BaseException) -> bool:
    import aiohttp
    import httpx
    import requests
    from curl_cffi import CurlError
    from curl_cffi.const import CurlECode

    pending = [error]
    seen: set[int] = set()
    while pending and len(seen) < 32:
        current = pending.pop()
        if id(current) in seen:
            continue
        seen.add(id(current))
        # TLS and HTTP rejections are reachable services, not offline reads.
        if isinstance(current, (requests.exceptions.SSLError, aiohttp.ClientSSLError)):
            continue
        if isinstance(
            current,
            (
                socket.gaierror,
                ConnectionError,
                TimeoutError,
                requests.exceptions.ConnectionError,
                requests.exceptions.Timeout,
                aiohttp.ClientConnectionError,
                httpx.ConnectError,
                httpx.TimeoutException,
            ),
        ):
            return True
        if isinstance(current, CurlError) and current.code in {
            CurlECode.COULDNT_RESOLVE_PROXY,
            CurlECode.COULDNT_RESOLVE_HOST,
            CurlECode.COULDNT_CONNECT,
            CurlECode.OPERATION_TIMEDOUT,
        }:
            return True
        if isinstance(current, httpx.HTTPStatusError):
            try:
                if current.response.json() == OFFLINE:
                    return True
            except ValueError:
                pass
        if isinstance(current, URLError) and isinstance(current.reason, BaseException):
            pending.append(current.reason)
        original = getattr(current, "original", None)
        if isinstance(original, BaseException):
            pending.append(original)
        pending.extend(
            cause
            for cause in (current.__cause__, current.__context__)
            if cause is not None
        )
        # OpenBB can wrap an exception as an argument without an explicit cause.
        pending.extend(arg for arg in current.args if isinstance(arg, BaseException))
    return False


def classify_rest_failures(app: Any) -> None:
    """Keep OpenBB's handlers, replacing only unreachable-provider failures."""
    from starlette.responses import JSONResponse

    def wrap(handler: Callable[..., Any]) -> Callable[..., Any]:
        async def classified(request: Any, error: Exception) -> Any:
            if is_offline_error(error):
                return JSONResponse(OFFLINE, status_code=503)
            if rejection := credential_rejection(error):
                return JSONResponse(rejection, status_code=401)
            return await handler(request, error)

        return classified

    app.exception_handlers = {
        kind: wrap(handler) for kind, handler in app.exception_handlers.items()
    }
    # Also cover a plain FastAPI app (used in contract tests). OpenBB already
    # installs its own Exception handler, whose other behavior stays intact.
    if Exception not in app.exception_handlers:

        async def unhandled(request: Any, error: Exception) -> Any:
            if is_offline_error(error):
                return JSONResponse(OFFLINE, status_code=503)
            if rejection := credential_rejection(error):
                return JSONResponse(rejection, status_code=401)
            raise error

        app.exception_handlers[Exception] = unhandled
    app.middleware_stack = None


async def call_tool_with_failures(
    run: Callable[..., Any], arguments: dict[str, Any]
) -> Any:
    from fastmcp.tools.tool import ToolResult

    try:
        return await run(arguments)
    except Exception as error:
        if is_offline_error(error):
            return ToolResult(
                content="The data provider could not be reached.",
                structured_content=OFFLINE,
                is_error=True,
            )
        if rejection := credential_rejection(error):
            return ToolResult(
                content="The data provider rejected the configured key.",
                structured_content=rejection,
                is_error=True,
            )
        raise
