"""Process-local HTTPS egress guard for the bundled OpenBB runtime.

The native host supplies the exact union of hosts granted for this run.  The
wrapper validates that union against the signed runtime manifest before this
module is installed. Patches cover OpenBB's helpers and the HTTP clients used
by the pinned provider set. The frozen, signed OpenBB dependency closure is a
trusted part of the product boundary; this process-local guard is not a claim
that hostile native code or a raw socket is OS-sandboxed.
"""

from __future__ import annotations

import os
import sys
from collections.abc import Callable, Iterable
from contextlib import contextmanager
from contextvars import ContextVar
from functools import wraps
from typing import Any, TypeVar, cast
from urllib.parse import urlsplit

from loupist_openbb.destinations import check_tool_destination

_REDIRECT_HOST = ContextVar("credential_redirect_host", default=None)
_NETWORK_ORIGINAL = "__loupist_network_original__"
_PROXY_ENVIRONMENT = (
    "ALL_PROXY",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "all_proxy",
    "http_proxy",
    "https_proxy",
    "no_proxy",
)
_T = TypeVar("_T", bound=Callable[..., Any])


def validate_https_url(value: object, allowed_hosts: frozenset[str]) -> str:
    """Return an authorized URL or fail before any request is attempted."""

    if hasattr(value, "full_url"):
        value = getattr(value, "full_url")
    url = str(value)
    try:
        parsed = urlsplit(url)
        port = parsed.port
    except ValueError as error:
        raise PermissionError("OpenBB request URL is invalid") from error
    host = (parsed.hostname or "").lower()
    if parsed.scheme.lower() != "https":
        raise PermissionError("OpenBB network policy permits HTTPS only")
    if parsed.username is not None or parsed.password is not None:
        raise PermissionError("OpenBB network policy rejects URL credentials")
    if port not in {None, 443}:
        raise PermissionError("OpenBB network policy permits HTTPS port 443 only")
    if host not in allowed_hosts:
        raise PermissionError(
            f"OpenBB network host is not enabled: {host or '<missing>'}"
        )
    check_tool_destination(url, host)
    origin = _REDIRECT_HOST.get()
    if origin is not None and host != origin:
        raise PermissionError("Cross-origin redirect from a credentialed provider")
    return url


@contextmanager
def _redirect_chain(url: str, credential_hosts: frozenset[str]):
    host = urlsplit(url).hostname
    token = _REDIRECT_HOST.set(host) if host in credential_hosts else None
    try:
        yield
    finally:
        if token is not None:
            _REDIRECT_HOST.reset(token)


def _original(function: _T) -> _T:
    return cast(_T, getattr(function, _NETWORK_ORIGINAL, function))


def _mark(function: _T, original: _T) -> _T:
    setattr(function, _NETWORK_ORIGINAL, original)
    return function


def _reject_proxy_options(kwargs: dict[str, Any]) -> None:
    for key in ("proxy", "proxies", "proxy_auth", "doh_url"):
        if kwargs.get(key) not in (None, "", False, (), [], {}):
            raise PermissionError("OpenBB network policy rejects explicit proxies")


def _patch_openbb_helpers(allowed_hosts: frozenset[str]) -> None:
    from openbb_core.provider.utils import helpers as provider_helpers

    previous_async = provider_helpers.amake_request
    previous_sync = provider_helpers.make_request
    original_async = _original(previous_async)
    original_sync = _original(previous_sync)

    @wraps(original_async)
    async def guarded_async(url: object, *args: Any, **kwargs: Any) -> Any:
        validate_https_url(url, allowed_hosts)
        _reject_proxy_options(kwargs)
        return await original_async(url, *args, **kwargs)

    @wraps(original_sync)
    def guarded_sync(url: object, *args: Any, **kwargs: Any) -> Any:
        validate_https_url(url, allowed_hosts)
        _reject_proxy_options(kwargs)
        return original_sync(url, *args, **kwargs)

    _mark(guarded_async, original_async)
    _mark(guarded_sync, original_sync)
    provider_helpers.amake_request = guarded_async
    provider_helpers.make_request = guarded_sync

    # Provider packages frequently bind these functions during import.  Patch
    # only identity-equal aliases; vendor modules and installed files are not
    # modified.
    for module_name, module in tuple(sys.modules.items()):
        if not module_name.startswith("openbb_") or module is None:
            continue
        if getattr(module, "amake_request", None) in {
            previous_async,
            original_async,
        }:
            setattr(module, "amake_request", guarded_async)
        if getattr(module, "make_request", None) in {previous_sync, original_sync}:
            setattr(module, "make_request", guarded_sync)


def _patch_requests(
    allowed_hosts: frozenset[str], credential_hosts: frozenset[str]
) -> None:
    import requests

    previous = requests.sessions.Session.send
    original = _original(previous)

    @wraps(original)
    def guarded(self: Any, request: Any, **kwargs: Any) -> Any:
        validate_https_url(request.url, allowed_hosts)
        _reject_proxy_options(kwargs)
        with _redirect_chain(request.url, credential_hosts):
            return original(self, request, **kwargs)

    requests.sessions.Session.send = _mark(guarded, original)


def _patch_aiohttp(
    allowed_hosts: frozenset[str], credential_hosts: frozenset[str]
) -> None:
    import aiohttp

    start = _original(aiohttp.ClientSession._request)

    @wraps(start)
    async def request(self, method, str_or_url, **kwargs):
        url = validate_https_url(self._build_url(str_or_url), allowed_hosts)
        with _redirect_chain(url, credential_hosts):
            return await start(self, method, str_or_url, **kwargs)

    aiohttp.ClientSession._request = _mark(request, start)
    previous = aiohttp.ClientRequest.__init__
    original = _original(previous)

    @wraps(original)
    def guarded(self: Any, method: str, url: object, *args: Any, **kwargs: Any) -> None:
        validate_https_url(url, allowed_hosts)
        _reject_proxy_options(kwargs)
        original(self, method, url, *args, **kwargs)

    aiohttp.ClientRequest.__init__ = _mark(guarded, original)


def _patch_httpx(
    allowed_hosts: frozenset[str], credential_hosts: frozenset[str]
) -> None:
    import httpx

    # httpx builds each redirect request from the preceding response.
    from httpx._client import BaseClient

    build = _original(BaseClient._build_redirect_request)

    @wraps(build)
    def build_redirect(self, request, response):
        next_request = build(self, request, response)
        if not isinstance(self._transport, httpx.ASGITransport):
            with _redirect_chain(str(request.url), credential_hosts):
                validate_https_url(next_request.url, allowed_hosts)
        return next_request

    BaseClient._build_redirect_request = _mark(build_redirect, build)
    previous_sync = httpx.Client._send_single_request
    original_sync = _original(previous_sync)

    @wraps(original_sync)
    def guarded_sync(self: Any, request: Any) -> Any:
        if isinstance(self._transport, httpx.ASGITransport):
            return original_sync(self, request)
        validate_https_url(request.url, allowed_hosts)
        return original_sync(self, request)

    previous_async = httpx.AsyncClient._send_single_request
    original_async = _original(previous_async)

    @wraps(original_async)
    async def guarded_async(self: Any, request: Any) -> Any:
        if isinstance(self._transport, httpx.ASGITransport):
            return await original_async(self, request)
        validate_https_url(request.url, allowed_hosts)
        return await original_async(self, request)

    httpx.Client._send_single_request = _mark(guarded_sync, original_sync)
    httpx.AsyncClient._send_single_request = _mark(guarded_async, original_async)


def _patch_urllib(
    allowed_hosts: frozenset[str], credential_hosts: frozenset[str]
) -> None:
    import urllib.request

    previous = urllib.request.OpenerDirector.open
    original = _original(previous)

    @wraps(original)
    def guarded(
        self: Any, fullurl: object, data: Any = None, timeout: Any = None
    ) -> Any:
        url = validate_https_url(fullurl, allowed_hosts)
        with _redirect_chain(url, credential_hosts):
            if timeout is None:
                return original(self, fullurl, data)
            return original(self, fullurl, data, timeout)

    urllib.request.OpenerDirector.open = _mark(guarded, original)


def _patch_curl_cffi(allowed_hosts: frozenset[str]) -> None:
    from curl_cffi import requests as curl_requests

    previous_sync = curl_requests.Session.request
    original_sync = _original(previous_sync)

    @wraps(original_sync)
    def guarded_sync(
        self: Any, method: object, url: object, *args: Any, **kwargs: Any
    ) -> Any:
        validate_https_url(url, allowed_hosts)
        _reject_proxy_options(kwargs)
        # libcurl follows redirects below Python, where the target hostname
        # cannot be inspected. Fail closed; pinned yfinance endpoints do not
        # require redirects in the supported keyless calls.
        kwargs["allow_redirects"] = False
        return original_sync(self, method, url, *args, **kwargs)

    previous_async = curl_requests.AsyncSession.request
    original_async = _original(previous_async)

    @wraps(original_async)
    async def guarded_async(
        self: Any, method: object, url: object, *args: Any, **kwargs: Any
    ) -> Any:
        validate_https_url(url, allowed_hosts)
        _reject_proxy_options(kwargs)
        kwargs["allow_redirects"] = False
        return await original_async(self, method, url, *args, **kwargs)

    curl_requests.Session.request = _mark(guarded_sync, original_sync)
    curl_requests.AsyncSession.request = _mark(guarded_async, original_async)


def apply_network_policy(
    hosts: Iterable[str], credential_hosts: frozenset[str] = frozenset()
) -> frozenset[str]:
    """Install HTTPS exact-host guards for every client in the pinned runtime."""

    allowed_hosts = frozenset(hosts)
    for name in _PROXY_ENVIRONMENT:
        os.environ.pop(name, None)
    _patch_openbb_helpers(allowed_hosts)
    _patch_requests(allowed_hosts, credential_hosts)
    _patch_aiohttp(allowed_hosts, credential_hosts)
    _patch_httpx(allowed_hosts, credential_hosts)
    _patch_urllib(allowed_hosts, credential_hosts)
    _patch_curl_cffi(allowed_hosts)
    return allowed_hosts
