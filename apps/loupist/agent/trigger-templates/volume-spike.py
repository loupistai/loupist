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

def day(value):
    text = str(value)[:10]
    return f"{text[:4]}-{text[4:6]}-{text[6:8]}" if len(text) == 8 and text.isdigit() else text

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
    volumes = {day(row[DATE]): float(row[VOLUME]) for row in rows or []
               if isinstance(row, dict) and row.get(DATE) and row.get(VOLUME) not in (None, "")}
    days = sorted(volumes.items())
    if len(days) <= WINDOW:
        unread.append({"name": asset["name"], "why": "no_data"})
        continue
    date, volume = days[-1]
    newest = max(newest or date, date)
    history = sorted(value for _, value in days[-WINDOW - 1:-1])
    middle = len(history) // 2
    median = (history[(len(history) - 1) // 2] + history[middle]) / 2
    if median > 0 and volume > MULTIPLE * median and loupist.state.get(asset["id"]) != date:
        loupist.state[asset["id"]] = date
        spiked.append({"id": asset["id"], "name": asset["name"], "date": date, "volume": volume,
                       "median": median, "ratio": round(volume / median, 1)})

if newest is None:
    raise loupist.Unavailable("no_data")
reading = "; ".join(f"{s['name']} volume {s['ratio']}x its median" for s in spiked) or "No volume spike"
loupist.result(met=bool(spiked), reading=reading[:160], as_of=newest,
               evidence={"multiple": MULTIPLE, "window": WINDOW, "spiked": spiked, "unread": unread})
