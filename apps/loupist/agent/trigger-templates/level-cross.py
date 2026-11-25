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
        points = loupist.price(asset["id"], days=7)["points"]
    except (loupist.Unavailable, loupist.Refused) as error:
        unread.append({"name": asset["name"], "why": error.code})
        continue
    if len(points) < 2:
        unread.append({"name": asset["name"], "why": "no_data"})
        continue
    before, last = points[-2], points[-1]
    newest = max(newest or last["date"], last["date"])
    side = "above" if last["value"] >= level else "below"
    seen = loupist.state.get(asset["id"]) or {}
    was = seen.get("side") or ("above" if before["value"] >= level else "below")
    if side != was and seen.get("met") != last["date"]:
        seen["met"] = last["date"]
        crossed.append({"id": asset["id"], "name": asset["name"], "side": side, "level": level,
                        "from": before, "to": last})
    seen["side"] = side
    loupist.state[asset["id"]] = seen

if newest is None:
    raise loupist.Unavailable("no_data")
reading = "; ".join(f"{c['name']} {c['side']} {c['level']:g}" for c in crossed) or "No level crossed"
loupist.result(met=bool(crossed), reading=reading[:160], as_of=newest,
               evidence={"levels": LEVELS, "crossed": crossed, "unread": unread})
