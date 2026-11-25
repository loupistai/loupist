"""A row in a list that was not there before: a filing index, a release calendar. The first run records the list."""
import json
import loupist

TOOL = loupist.args["tool"]
CALL = loupist.args.get("call") or {}
ROWS = loupist.args.get("rows") or "rows"
ID = loupist.args.get("id_field") or "id"
DATE = loupist.args.get("date_field") or "date"
TITLE = loupist.args.get("title_field") or "title"

def day(value):
    text = str(value)[:10]
    return f"{text[:4]}-{text[4:6]}-{text[6:8]}" if len(text) == 8 and text.isdigit() else text

payload = loupist.call(TOOL, **CALL)
for key in ROWS.split("."):
    payload = payload.get(key) if isinstance(payload, dict) else None
if not isinstance(payload, list):
    raise loupist.Unavailable("no_data")
rows = [row for row in payload if isinstance(row, dict) and row.get(ID) not in (None, "")]

first = "seen" not in loupist.state
seen = set(loupist.state.get("seen") or [])
since = loupist.state.get("since")
# An id that is new but dated before the newest date already seen is an old row coming back.
fresh = [row for row in rows if str(row[ID]) not in seen
         and not (since and row.get(DATE) and day(row[DATE]) < since)]
if first:
    fresh = []  # what is already listed is the baseline, not news

dates = [day(row[DATE]) for row in rows if row.get(DATE)]
kept = [str(row[ID]) for row in sorted(rows, key=lambda row: day(row.get(DATE) or ""), reverse=True)]
while len(json.dumps(kept)) > 6000:  # the app keeps at most 8 KiB of state; the oldest go first
    kept.pop()
loupist.state["seen"] = kept
loupist.state["since"] = max([since or "", *dates]) or None

as_of = max(dates) if dates else loupist.now[:10]
reading = "; ".join(str(row.get(TITLE) or row[ID]) for row in fresh[:4]) or (
    f"Recorded {len(rows)} listed items" if first else "No new item")
loupist.result(met=bool(fresh), reading=reading[:160], as_of=as_of,
               evidence={"tool": TOOL, "new": fresh[:8], "listed": len(rows)})
