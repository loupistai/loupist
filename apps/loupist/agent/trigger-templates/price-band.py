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
        points = loupist.price(asset["id"], days=7)["points"]
    except (loupist.Unavailable, loupist.Refused) as error:
        unread.append({"name": asset["name"], "why": error.code})
        continue
    if len(points) < 2:
        unread.append({"name": asset["name"], "why": "no_data"})
        continue
    before, last = points[-2], points[-1]
    newest = max(newest or last["date"], last["date"])
    change = (last["value"] / before["value"] - 1) * 100
    if abs(change) > BAND and loupist.state.get(asset["id"]) != last["date"]:
        loupist.state[asset["id"]] = last["date"]
        moved.append({"id": asset["id"], "name": asset["name"], "change_percent": round(change, 2),
                      "from": before, "to": last})

if newest is None:
    raise loupist.Unavailable("no_data")
reading = "; ".join(f"{m['name']} {m['change_percent']:+.1f}%" for m in moved) or "No asset beyond the band"
loupist.result(met=bool(moved), reading=reading[:160], as_of=newest,
               evidence={"band_percent": BAND, "moved": moved, "unread": unread})
