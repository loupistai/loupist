# Thesis and the read

Reasons and lines are Journal entries on the asset page. A move is staged until the reply commits, and Undo restores only the latest Loupist move.

### Thesis and the read

The user's reasons ("why you follow it") and lines ("what would change your mind")
are Journal entries tagged `thesis {assetId, role: reason | line}`. A line needs a
check whose `byDate` is optional. An undated line is never due: Journal reviews, due
pointers, the morning message and Needs attention skip it, and its reading lives on
the asset page. The page's form writes through `journal_create`/`journal_update` on
an existing page. Chat's `journal_save` names the page by
`thesis {symbol, market, role}`, needs no Source, World or Lens read, and creates the
page with the turn. Archiving an entry removes it from the thesis.

The read is the Loupist's view, stored on the page and never in the Journal:
Holding up → Weakening → Challenged → Broken (stored as `holding`, so it never reads
as ownership). It exists while at least one active reason does and starts at Holding
up on the first reason's day. `thesis_update` moves it exactly one step, citing 1–3
of the turn's results: each must carry a date (a calculation takes its inputs'
latest) and at least one must be an original read (a filing, data result or fetched
page) or a `calculate` over one. The move's fact date is the latest evidence date. A
line reading (`lineReading {journalId, observed}`) counts only after Rust finds that
exact number in a cited original, read in the line's unit
([number matching](../analysis/number-checks.md#number-matching)). The reading is recorded; a strict
crossing moves the read to Broken from any step, with the reason "Crossed your line:
{claim} ({value}, {date})". Rust refuses an update while the user has no active
reason, for two steps or the same step, without a reason of 1–280 characters, with
an unknown `reasonId`, undated or only non-original evidence, with saving or Memory
off, in an as-of turn, and as a second update for the same asset in one reply
(reserved when the tool runs and enforced again at commit).

The user can Undo only the latest move, and only a Loupist move, by its move id
(`asset_verdict_undo`): a newer move is a conflict, a refresh in between is not. Undo
restores the previous step and records the user's own move. After an Undo, moving
the read the same way again, including by a line crossing, needs evidence dated
after the undone move's fact date; the other direction is unconstrained. Every
Loupist move and Undo writes a `thesis_moved` [growth event](growth.md#growth-event-log). A
crossed line also posts one Bot message and a Needs attention row until the
page is opened ([Architecture](asset-pages.md#asset-pages)).
