---
name: world
description: Use when investigating what this Loupist already knows about entities, events and their relationships, or when checking how saved claims and their sources fit together.
---

# World

World is this Loupist's source-grounded record: entities, events, relationships, dates and attributed claims. A relation may be a forecast, a hypothesis or
disputed; keep its status and source.

Find material with `world_search`, then exact-read it
before citing. Search hits and graph connections are discovery. An empty or
`partial` result does not prove absence, and a Source hit marked `mapPending` is
not in World yet: read the Source.

A `world_read` result's `sourceTargets` gives `readArgs` for each retained original; use
them with `world_read`. A complete retained Source is the original evidence for
that capture; do not reopen its file or URL just to confirm it.

Compare claims on the same entity, period and definition. Keep conflicting
accounts and their attribution visible, distinguish a later correction from what
was known at the cutoff, and keep a forecast labelled as one.

## Time

A claim carries when it held, at the precision its Source gave (a year, a month, a date or an instant), when
that Source was published, and whether it has ended. A claim marked `ended` is history: it tells how things
stood until its `end`, not how they stand. An approximate end was not stated by any Source: a later Source
stated the same thing with a different value, so the earlier claim held no later than that Source's date.

Read one claim's story with `world_read`, `centerClaimId` and `history: true`: `lineage` names the Sources
that first stated it, supported it and ended it, and says whether the end was stated or taken from a later
statement. Read a subject's changes with `world_changes` and `about`, and the World as it stood on a date
with `asOf`. A fact that a later Source does not repeat is still current. Sources published the same day
that disagree stay a conflict: give both.

To add to World, save a Source that says something in words (a filing, an article, a transcript): World building extracts names and relationships after the reply. Price data adds nothing to World. `world_job` reports committed processing only.

Explain findings with attribution, dates, uncertainty and readable source titles.
