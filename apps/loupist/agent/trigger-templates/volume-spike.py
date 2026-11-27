"""A followed asset whose latest daily volume is more than `multiple` times its median over `window` days."""
import loupist

TOOL = loupist.args["tool"]
CALLS = loupist.args.get("calls") or {}
ROWS = loupist.args.get("rows") or "rows"
DATE = loupist.args.get("date_field") or "date"
VOLUME = loupist.args.get("volume_field") or "volume"
MULTIPLE = float(loupist.args.get("multiple", 3))
WINDOW = int(loupist.args.get("window", 20))
if WINDOW < 1:
    raise ValueError("window must be positive")
spiked, unread, newest = [], [], None

def rows_of(payload):
    for key in ROWS.split("."):
        payload = payload.get(key) if isinstance(payload, dict) else None
    return payload if isinstance(payload, list) else None

assets = [asset for asset in loupist.followed() if asset["id"] in CALLS]
try:
    loupist.gather([(TOOL, CALLS[asset["id"]]) for asset in assets])
except (loupist.Unavailable, loupist.Refused):
    pass  # every read is fetched at once; each one's failure is named below

for asset in assets:
    try:
        rows = rows_of(loupist.call(TOOL, **CALLS[asset["id"]]))
    except (loupist.Unavailable, loupist.Refused) as error:
        unread.append({"name": asset["name"], "why": error.code})
        continue
    volumes = {}
    for row in rows or []:
        if not isinstance(row, dict) or row.get(VOLUME) in (None, ""):
            continue
        day = loupist.observation_day(row.get(DATE))
        if day:
            volumes[day] = float(row[VOLUME])
    days = sorted(volumes.items())
    if len(days) <= WINDOW:
        unread.append({"name": asset["name"], "why": "no_data"})
        continue
    date, volume = days[-1]
    if not loupist.fresh_bar(date, asset, TOOL):
        unread.append({"name": asset["name"], "date": date, "why": "stale"})
        continue
    seen = loupist.accept_bar(asset["id"], date)
    if seen is None:
        unread.append({"name": asset["name"], "date": date, "why": "stale"})
        continue
    newest = max(newest or date, date)
    history = sorted(value for _, value in days[-WINDOW - 1:-1])
    middle = len(history) // 2
    median = (history[(len(history) - 1) // 2] + history[middle]) / 2
    if median > 0 and volume > MULTIPLE * median and seen.get("met") != date:
        seen["met"] = date
        spiked.append({"id": asset["id"], "name": asset["name"], "date": date, "volume": volume,
                       "median": median, "ratio": round(volume / median, 1)})

if newest is None:
    raise loupist.Unavailable("stale" if any(row["why"] == "stale" for row in unread) else "no_data")
reading = "; ".join(f"{s['name']} volume {s['ratio']}x its median" for s in spiked) or "No volume spike"
if unread:
    reading += " · Unavailable: " + ", ".join(row["name"] for row in unread)
loupist.result(met=bool(spiked), reading=reading[:160], as_of=max((row["date"] for row in spiked), default=newest),
               evidence={"multiple": MULTIPLE, "window": WINDOW, "spiked": spiked, "unread": unread})
