# Pack add and remove

Opening a pack reviews it in memory. Add creates a Loupist through the ordinary creation path. Remove takes back only what that pack added.

Opening a pack (a github.com repository, folder or gist link, a ZIP or a folder)
validates it and holds it in memory for review for an hour, at most four at once;
nothing touches a Loupist. A rejected pack names one reason (`notPack`, `newer`,
`tooLarge`, `unsafe`, `damaged`, `spoof`) and adds nothing. Review before adding shows
the profile, each perspective, method and principle as read-only Markdown with its scan
flags and lines. Its typed manifest also exposes the license id, `licenseUrl`,
`redistributable` and `credits` entries with each origin's name, author, license, URL
and version: review links the license name to `licenseUrl` with its plain line,
or, when not redistributable, says "For your use only. You can't share these items
again." A Credits list names each `credits` origin (linked when it has a URL) with its
author, version and license. Review includes the map's counts and Source hosts, flagged map
records (50 listed, the rest counted) and flags on the pack's own name and author. A pack on the community
index's blocked list can be reviewed but not added (`safety_blocked`).

Add creates a new Loupist through the ordinary creation path (name and tone chosen in
review; the pack's focus only when the scan found nothing in it; a known avatar) or adds
to an existing one, whose principles and perspective stay as they are (the pack's
perspective is not offered). One transaction records the import, its items as pending
suggestions and its map; a failure rolls back and deletes a Loupist Add created. Adding
changes no bindings, tool approvals, Skill enablement, Memory permissions, schedules,
model settings or instructions.

Suggestions sit at the top of Your principles and Methods on "What {Loupist} knows"
until accepted or skipped item by item. Clean items can be accepted together; an item
with a scan finding only on its own (`flagged_one_at_a_time`). Accept checks the stored
body against its hash and creates an ordinary Lens method (its title made unique with
"· {pack}"), a principle with origin `shared`, or a new Loupist's standing perspective;
a principle the Loupist already holds is marked skipped.

The map merges in the same transaction without a model call. A running World job
refuses it (`world_busy`); Add first waits up to five minutes for one. Each pack Source
becomes a reference Source (`imported_contributor`, provider `shared_pack:<import>`)
whose retained text is the pack's Source record and the claims citing it; it is never
sent for extraction. A pack name matches an existing name by identifier (ticker with
exchange, CIK, DART corp code, ISIN), then by name or alias of the same kind, and is
otherwise added. A relationship identical to one on the map (relation, ends and
proposition) adds the pack's Source to it. A pack `REPORTS` relationship with the same
ends and period, the same unit and currency and another value is added as the pack's own
disputed claim with the note "Differs from a value already on your map"; the user's own
claims never change. The merged map must fit the derived index's load bounds (10,000
names, 10,000 relationships, 5,000 cited spans). A load of the new snapshot into the
derived index is queued ahead of extraction ([Runs](../runtime/scheduled-runs.md#runs)), so the next
extraction keeps the shared content.

Remove what it added takes back the pack's own relationships, the support it added to
others (a relationship left without a Source goes), the names nothing else uses and its
Sources' place in the World, then queues the same load; a running World job refuses it.
Source records are immutable, so the pack's Sources are hidden from the Library, search
and reads. Pending suggestions are dropped; accepted items stay unless the user also
archives them (a standing perspective is never archived).

`packOrigin` names the pack behind an accepted Lens item, or a name or relationship a
pack added, while that pack is not removed; the Lens reader and Map inspector show
"Shared · {pack} by {author}" with a link to Shared Loupists you added on "What
{Loupist} knows". That section lists each pack with Remove and, when the community index
is configured and the pack came from a link, Report (the index's GitHub issue form with
only the pack link filled in). Add also copies the pack's files to
`.loupist/packs/<import>/<version>/`, so they travel with the folder. A pack the index
removed shows a banner on its row with the reason.
