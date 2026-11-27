"""Read the data sealed for this worker start. Requests are served after exit."""
import collections.abc
from datetime import date, timedelta
import json
import random
import sys
import types

with open("/scratch/.loupist-ledger.json", encoding="utf-8") as file:
    _ledger = json.load(file)
random.seed(_ledger["id"])

class Pending(BaseException):
    pass

class Refused(Exception):
    def __init__(self, code):
        super().__init__(code)
        self.code = code

class Unavailable(Exception):
    def __init__(self, code, retry_after=None):
        super().__init__(code)
        self.code = code
        self.retry_after = retry_after

def _key(call):
    return json.dumps(call, sort_keys=True, separators=(",", ":"), allow_nan=False)

_entries = {_key(entry["call"]): entry["response"] for entry in _ledger["entries"]}

def _lookup(call):
    response = _entries.get(_key(call))
    if response is None:
        raise Refused("limit" if _ledger["final_round"] else "not_exposed")
    if response["status"] == "refused":
        raise Refused(response["code"])
    if response["status"] == "unavailable":
        raise Unavailable(response["code"], response.get("retry_after"))
    # The next lookup returns the same data even if the caller mutates this copy.
    return json.loads(json.dumps(response["value"]))

def gather(calls):
    requests = [{"tool": tool, "arguments": arguments} for tool, arguments in calls]
    if len(requests) > 32:
        raise Refused("limit")
    missing = [request for request in requests if _key(request) not in _entries]
    if missing and not _ledger["final_round"]:
        payload = json.dumps(missing, allow_nan=False).encode("utf-8")
        if len(payload) > 256 * 1024:
            raise Refused("limit")
        with open("/scratch/.loupist-requests.json", "xb") as file:
            file.write(payload)
        raise Pending()
    return [_lookup(request) for request in requests]

def call(tool, **arguments):
    return gather([(tool, arguments)])[0]

def dataset(dataset_id, revision, pointer=None):
    return call("dataset", dataset_id=dataset_id, revision=revision, pointer=pointer)

def followed():
    return call("followed")

def price(asset_id, days=0):
    return call("price", asset_id=asset_id, days=days)

def observation_day(value):
    text = str(value)[:10]
    if len(text) == 8 and text.isdigit():
        text = f"{text[:4]}-{text[4:6]}-{text[6:8]}"
    try:
        return date.fromisoformat(text).isoformat()
    except ValueError:
        return ""


def bar_dates(asset, tool=None, market=None):
    exchange = (asset.get("reference") or {}).get("exchange")
    market = market or ("KRX" if tool == "krx_daily_quote" else exchange) or ""
    dates = _ledger.get("bar_dates", {})
    return dates.get(market.strip().upper()) or dates[""]


def fresh_pair(before, last, asset, market=None):
    latest, previous, prior = bar_dates(asset, market=market)
    return (before, last) in ((prior, previous), (previous, latest)) and before != last


def fresh_bar(day, asset, tool=None):
    latest, previous, _ = bar_dates(asset, tool)
    return day in (latest, previous)


def accept_bar(asset_id, day):
    saved = _module.state.get(asset_id)
    seen = saved if isinstance(saved, dict) else {"met": saved}
    latest = seen.get("latest") or seen.get("met")
    if latest and day < latest:
        return None
    seen["latest"] = day
    _module.state[asset_id] = seen
    return seen


def fresh_item(day):
    today = date.fromisoformat(_ledger["now"][:10])
    return (today - timedelta(days=_ledger.get("check_stale_days") or 5)).isoformat() <= day <= today.isoformat()


def sources(query, limit=10):
    return call("world_search", scope="source", query=query).get("hits", [])[:limit]

def source(id):
    return call("world_read", scope="source", recordId=id)

# Follows `next` for up to `pages` pages. Each page is one read against the call's read limit.
def changes(**arguments):
    pages = arguments.pop("pages", 4)
    try:
        pages = int(pages)
    except (TypeError, ValueError):
        pages = 4
    collected = []
    last = None
    for _ in range(max(pages, 0)):
        last = call("world_changes", **arguments)
        collected.extend(last.get("changes") or [])
        if not last.get("capped"):
            break
        nxt = last.get("next")
        if not nxt:
            break
        arguments["since"] = nxt
    if last is None:
        return {"changes": collected}
    last["changes"] = collected
    return last

def _write_json(name, payload):
    raw = json.dumps(payload, allow_nan=False).encode("utf-8")
    if len(raw) > 64 * 1024:
        raise Refused("limit")
    with open("/scratch/" + name, "w", encoding="utf-8") as file:
        file.write(raw.decode("utf-8"))

# as_of is the date of the newest observation the verdict rests on. The host refuses a
# future date and one older than the check's age limit.
def result(met, reading, as_of, evidence=None):
    if not isinstance(met, bool):
        raise TypeError("met must be true or false")
    if not isinstance(reading, str) or not isinstance(as_of, str):
        raise TypeError("reading and as_of must be strings")
    _write_json(".loupist-verdict.json", {
        "met": met,
        "reading": reading,
        "as_of": as_of,
        "evidence": evidence if isinstance(evidence, dict) else {},
        "state": _module.state if isinstance(_module.state, dict) else {},
    })

def _failure(kind, code, line, retry_after=None):
    try:
        _write_json(".loupist-failure.json", {
            "kind": kind,
            "code": code,
            "line": line,
            "retry_after": retry_after,
        })
    except Exception:
        pass

class _Inputs(collections.abc.Mapping):
    def __getitem__(self, name):
        request = _ledger["inputs"][name]
        return call(request["tool"], **request["arguments"])

    def __iter__(self):
        return iter(_ledger["inputs"])

    def __len__(self):
        return len(_ledger["inputs"])

_state = _ledger.get("state")
_module = types.ModuleType("loupist")
for _member in (Pending, Refused, Unavailable, call, dataset, followed, price, sources, source, gather, result,
                observation_day, fresh_pair, fresh_bar, accept_bar, fresh_item):
    setattr(_module, _member.__name__, _member)
_module.inputs = _Inputs()
_module.args = _ledger["args"]
_module.now = _ledger["now"]
_module.state = _state if isinstance(_state, dict) else {}
sys.modules["loupist"] = _module

with open("/scratch/.loupist-code.py", encoding="utf-8") as file:
    _code = compile(file.read(), "<run_python>", "exec")
try:
    exec(_code, {"__name__": "__main__", "__file__": "<run_python>"})
except Pending:
    pass
except Unavailable as error:
    import traceback

    _failure("unavailable", getattr(error, "code", "no_data"), str(error).splitlines()[0] if str(error) else "no_data", getattr(error, "retry_after", None))
    frames = traceback.StackSummary.extract(
        (frame, line)
        for frame, line in traceback.walk_tb(error.__traceback__)
        if frame.f_globals is not globals()
    )
    if frames:
        print("Traceback (most recent call last):", file=sys.stderr)
        traceback.print_list(frames)
    sys.stderr.writelines(traceback.format_exception_only(type(error), error))
    sys.exit(1)
except Refused as error:
    _failure("refused", getattr(error, "code", "refused"), str(error).splitlines()[0] if str(error) else "refused")
    raise
except Exception as error:
    _failure("error", type(error).__name__, str(error).splitlines()[0] if str(error) else type(error).__name__)
    raise
