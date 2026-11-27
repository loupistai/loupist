"""A row in a list that was not there before: a filing index, a release calendar. The first run records the list."""
import json
import loupist

TOOL = loupist.args["tool"]
CALL = loupist.args.get("call") or {}
ROWS = loupist.args.get("rows") or "rows"
ID = loupist.args.get("id_field") or "id"
DATE = loupist.args.get("date_field") or "date"
TITLE = loupist.args.get("title_field") or "title"

day = loupist.observation_day

payload = loupist.call(TOOL, **CALL)
for key in ROWS.split("."):
    payload = payload.get(key) if isinstance(payload, dict) else None
if not isinstance(payload, list):
    raise loupist.Unavailable("no_data")
rows = [row for row in payload if isinstance(row, dict) and row.get(ID) not in (None, "")]

first = "seen" not in loupist.state
seen = set(loupist.state.get("seen") or [])
dated = [(day(row.get(DATE)), row) for row in rows]
current = [(date, row) for date, row in dated if loupist.fresh_item(date)]
if rows and not current and not first:
    raise loupist.Unavailable("stale")
fresh = [(date, row) for date, row in current if not first and str(row[ID]) not in seen]
dates = [date for date, _ in current]
listed = [str(row[ID]) for _, row in sorted(current, key=lambda item: item[0], reverse=True)]
kept = list(dict.fromkeys([*listed, *(loupist.state.get("seen") or [])]))
while len(json.dumps(kept)) > 6000:  # the app keeps at most 8 KiB of state; the oldest go first
    kept.pop()
loupist.state["seen"] = kept
loupist.state.pop("since", None)

as_of = max((date for date, _ in fresh), default=max(dates, default=loupist.now[:10]))
reading = "; ".join(str(row.get(TITLE) or row[ID]) for _, row in fresh[:4]) or (
    f"Recorded {len(rows)} listed items" if first else "No new item")
loupist.result(met=bool(fresh), reading=reading[:160], as_of=as_of,
               evidence={"tool": TOOL, "new": [row for _, row in fresh[:8]], "listed": len(rows),
                         "unavailable": len(rows) - len(current)})
