"""Any followed asset whose latest close moved more than band_percent from the close before it."""
import loupist

BAND = float(loupist.args.get("band_percent", 5))
ids = set(loupist.args.get("asset_ids") or [])
names = {name.casefold() for name in loupist.args.get("asset_names") or []}
scoped = "asset_ids" in loupist.args or "asset_names" in loupist.args
moved, unread, newest = [], [], None

assets = [asset for asset in loupist.followed()
          if not scoped or asset["id"] in ids or asset["name"].casefold() in names]
try:
    loupist.gather([("price", {"asset_id": asset["id"], "days": 7}) for asset in assets])
except (loupist.Unavailable, loupist.Refused):
    pass  # every read is fetched at once; each one's failure is named below

for asset in assets:
    try:
        quote = loupist.price(asset["id"], days=7)
        points = quote["points"]
    except (loupist.Unavailable, loupist.Refused) as error:
        unread.append({"name": asset["name"], "why": error.code})
        continue
    if len(points) < 2:
        unread.append({"name": asset["name"], "why": "no_data"})
        continue
    before, last = points[-2], points[-1]
    if not loupist.fresh_pair(before["date"], last["date"], asset, quote.get("market")):
        unread.append({"name": asset["name"], "date": last["date"], "why": "stale"})
        continue
    if before["value"] <= 0 or last["value"] <= 0:
        unread.append({"name": asset["name"], "why": "no_data"})
        continue
    seen = loupist.accept_bar(asset["id"], last["date"])
    if seen is None:
        unread.append({"name": asset["name"], "date": last["date"], "why": "stale"})
        continue
    newest = max(newest or last["date"], last["date"])
    delta = last["value"] - before["value"]
    if abs(delta) * 100 > BAND * before["value"] and seen.get("met") != last["date"]:
        seen["met"] = last["date"]
        moved.append({"id": asset["id"], "name": asset["name"], "change_percent": round(delta / before["value"] * 100, 2),
                      "from": before, "to": last})

if newest is None:
    raise loupist.Unavailable("stale" if any(row["why"] == "stale" for row in unread) else "no_data")
reading = "; ".join(f"{m['name']} {m['change_percent']:+.1f}%" for m in moved) or "No asset beyond the band"
if unread:
    reading += " · Unavailable: " + ", ".join(row["name"] for row in unread)
loupist.result(met=bool(moved), reading=reading[:160], as_of=max((row["to"]["date"] for row in moved), default=newest),
               evidence={"band_percent": BAND, "moved": moved, "unread": unread})
