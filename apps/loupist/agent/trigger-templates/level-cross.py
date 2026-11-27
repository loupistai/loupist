"""A followed asset whose latest close crossed the level set for it, in either direction."""
import loupist

LEVELS = {asset_id: float(level) for asset_id, level in (loupist.args.get("levels") or {}).items()}
crossed, unread, newest = [], [], None

assets = [asset for asset in loupist.followed() if asset["id"] in LEVELS]
try:
    loupist.gather([("price", {"asset_id": asset["id"], "days": 7}) for asset in assets])
except (loupist.Unavailable, loupist.Refused):
    pass  # every read is fetched at once; each one's failure is named below

for asset in assets:
    level = LEVELS[asset["id"]]
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
    seen = loupist.accept_bar(asset["id"], last["date"])
    if seen is None:
        unread.append({"name": asset["name"], "date": last["date"], "why": "stale"})
        continue
    newest = max(newest or last["date"], last["date"])
    side = "above" if last["value"] >= level else "below"
    was = "above" if before["value"] >= level else "below"
    if side != was and seen.get("met") != last["date"]:
        seen["met"] = last["date"]
        crossed.append({"id": asset["id"], "name": asset["name"], "side": side, "level": level,
                        "from": before, "to": last})

if newest is None:
    raise loupist.Unavailable("stale" if any(row["why"] == "stale" for row in unread) else "no_data")
reading = "; ".join(f"{c['name']} {c['side']} {c['level']:g}" for c in crossed) or "No level crossed"
if unread:
    reading += " · Unavailable: " + ", ".join(row["name"] for row in unread)
loupist.result(met=bool(crossed), reading=reading[:160], as_of=max((row["to"]["date"] for row in crossed), default=newest),
               evidence={"levels": LEVELS, "crossed": crossed, "unread": unread})
