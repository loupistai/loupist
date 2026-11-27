"""A fact this Loupist holds about the followed assets in scope that ended or that Sources contradict, or a Lens section that went out of date, since the last check. The first run records where the feed stands. Pass direction (lower or higher) when the user asked about a figure moving one way, such as lowered guidance: an ended fact whose figure and the figure that replaced it compare is then reported only when it moved that way. One whose figures do not compare is still reported, marked direction_unknown, and the run it starts says that the figures did not show the direction."""
import json
import loupist

KINDS = loupist.args.get("kinds") or ["ended", "conflict", "lens"]
DIRECTION = loupist.args.get("direction")
if DIRECTION not in (None, "lower", "higher"):
    raise ValueError("direction is lower or higher")
NEEDLES = [str(word).casefold() for word in (loupist.args.get("contains") or []) if str(word).strip()]


def scoped_ids():
    ids = set(loupist.args.get("asset_ids") or [])
    names = {name.casefold() for name in loupist.args.get("asset_names") or []}
    scoped = "asset_ids" in loupist.args or "asset_names" in loupist.args
    assets = [asset for asset in loupist.followed()
              if not scoped or asset["id"] in ids or asset["name"].casefold() in names]
    if scoped and not assets:
        raise loupist.Unavailable("no_data")
    return [asset["id"] for asset in assets] or list(ids)


def statement_of(row):
    if row.get("kind") == "lens":
        return " ".join(part for part in (row.get("title"), row.get("sectionKey"), row.get("state")) if part)
    return str(row.get("proposition") or "")


def row_key(row):
    if row.get("kind") == "lens":
        # A later supersession of the same section has a new time.
        return "lens:{record}:{section}:{at}".format(
            record=row.get("recordId"), section=row.get("sectionKey"), at=row.get("recordedAtMs"))
    return "{kind}:{claim}:{episode}:{partner}".format(
        kind=row.get("kind"),
        claim=row.get("claimId"),
        episode=row.get("episodeId"),
        partner=row.get("withClaimId") or "")


def remember(previous, keys, cursor):
    merged = list(previous)
    known = set(merged)
    for key in keys:
        if key not in known:
            merged.append(key)
            known.add(key)
    while merged and len(json.dumps({"cursor": cursor, "seen": merged, "started": True})) > 7000:
        merged.pop(0)
    return merged


def published_on(row):
    source = row.get("source") or {}
    published = source.get("publicationTime") or {}
    text = str(published.get("value") or "")[:10]
    if len(text) == 10:
        return text
    ending = ((row.get("ending") or {}).get("at") or {}).get("value")
    text = str(ending or "")[:10]
    return text if len(text) == 10 else ""


def keep(row):
    if not NEEDLES:
        return True
    text = statement_of(row).casefold()
    return any(needle in text for needle in NEEDLES)


def direction_unknown(row):
    """An ended fact whose figures cannot say which way it moved."""
    return DIRECTION is not None and row.get("kind") == "ended" \
        and not (row.get("figures") or {}).get("comparable")


def moved_as_asked(row):
    """Silence never hides a change: only a comparable figure that moved the
    other way, or not at all, is left out."""
    if DIRECTION is None or row.get("kind") != "ended" or direction_unknown(row):
        return True
    return row["figures"].get("direction") == DIRECTION


def evidence_row(row):
    source = row.get("source") or {}
    published = (source.get("publicationTime") or {}).get("value")
    ending = ((row.get("ending") or {}).get("at") or {}).get("value")
    out = {
        "kind": row.get("kind"),
        "statement": statement_of(row)[:180],
        "dates": [value for value in (published, ending) if value],
        "source": source.get("title") or row.get("title") or "",
    }
    figures = row.get("figures")
    if DIRECTION is not None and figures:
        out["figures"] = {key: figures.get(key) for key in ("from", "to", "direction")}
    if direction_unknown(row):
        out["direction_unknown"] = True
    return out


about = scoped_ids()
if not about:
    raise loupist.Unavailable("no_data")

started = loupist.state.get("started") is True
seen = list(loupist.state.get("seen") or [])
since = loupist.state.get("cursor") or "1970-01-01"
# One page per evaluation. followed(), this read and result() are the three
# worker rounds; a second page in this start is refused and the cursor is lost.
page = loupist.call("world_changes", since=since, about=about, kinds=KINDS)
pending = int(page.get("pending") or 0)
failed = int(page.get("failed") or 0)
last_next = page.get("next")
rows = list(page.get("changes") or [])
rows.extend({**row, "kind": "lens"} for row in (page.get("lens") or []))
rows = [row for row in rows if keep(row)]
capped = bool(page.get("capped")) and bool(last_next)

fresh = []
known = set(seen)
# History already on the feed at approval is the baseline, including a page
# read while jobs are pending or a source has failed. A failed source does not
# freeze later alerts. Pending work after approval waits, so a row that lands
# during it is reported on the next quiet evaluation.
if not started:
    if last_next:
        loupist.state["cursor"] = last_next
        loupist.state["seen"] = remember(seen, (row_key(row) for row in rows), last_next)
    if not capped:
        loupist.state["started"] = True
elif pending == 0:
    fresh = [row for row in rows if row_key(row) not in known and moved_as_asked(row)]
    if last_next:
        loupist.state["cursor"] = last_next
        loupist.state["seen"] = remember(seen, (row_key(row) for row in rows), last_next)

dates = [published_on(row) for row in fresh]
dates = [day for day in dates if day]
reading = "; ".join(statement_of(row) for row in fresh if statement_of(row))[:160]
if not reading:
    reading = "Recorded the starting point" if not started else "No new fact"
evidence = {"rows": [evidence_row(row) for row in fresh[:8]]}
if pending:
    evidence["pending"] = pending
if failed:
    evidence["failed"] = failed
while evidence["rows"] and len(json.dumps(evidence)) > 3500:
    evidence["rows"].pop()
loupist.result(
    met=bool(fresh) and pending == 0,
    reading=reading[:160],
    as_of=max(dates, default=loupist.now[:10]),
    evidence=evidence,
)
