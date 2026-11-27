# Point-in-time answers

An as-of date answers only from what was known by then. The composer control sends `as_of` and never puts the date in the question text.

## Point-in-time answers

A Chat turn can carry an as-of date ([`/asof`](#composer-date-control),
[turn authority](../runtime/chat-turns.md#turn-authority-and-finalization)). Its answer then
uses only what was published or recorded by the end of that calendar day: each
tool and Memory read applies the cutoff in Rust, and the turn envelope tells the
model not to add later facts from its own knowledge.
The day ends at midnight on the user's clock, as `as_of.end_of_day` states. A
published instant counts on the user's date at that instant (`local_time.rs`), and
a date written without a time counts as written. In Seoul, a page published at
00:30 on the 23rd is after a cutoff on the 22nd although it is still the 22nd in
UTC (`record_date` in `tool_executor/market.rs`).

| Under a cutoff | Rule |
| --- | --- |
| Web pages | Publication on or before the date; later or undated pages fail with `as_of_later` ([Data and provenance](datasets.md#data-and-provenance)) |
| DART, KRX, KIS, World Bank, DefiLlama, OpenBB | Dated rows only; current-only routes fail with `as_of_unsupported`, a read with nothing dated by then with `as_of_empty` |
| Saved datasets (`dataset_describe`, `dataset_query`, `dataset_export`) and `calculate` | Only revisions fetched under a cutoff on or before the date |
| Saved charts | `artifact_read` and `chart_query` need the chart's rows drawn from such a revision; otherwise `as_of_unsupported` asks for a fresh read and a new chart |
| Tables (`grid_publish`) | Every cell must be known by then: a cell citing a local file fails with `as_of_unsupported`, later material with `as_of_later`; the table keeps the date, and `artifact_read` of a table needs one built under a cutoff on or before it |
| Saved reports | A Markdown artifact written after the date is left out (`as_of_later`) |
| Saved Memory | [Memory under a cutoff](../knowledge/memory.md#memory-under-a-cutoff) |
| What the user follows | Saved followed names and references, with their price-read state |
| Principles | Apply as they are today |

A World ending requires a known notice and an effective date within the cutoff.
An April 1 answer keeps a CFO current when a May filing first reports a March
departure, or when a March notice puts the departure in June.

Every World reader under a cutoff answers as the World that read only the Sources
known by then would: the World read and its hits, a claim's lineage, the change feed
(both axes, and inside the asset page), a filing comparison and its receipt, and a
Lens page with its section states. One projection decides (`as_of.rs`): a claim's
evidence, ending, conflicts, status, attribution, assertion time and first reading
come from the Sources the cutoff knows, and a label never names a Source it does not.
A dispute or withdrawal counts when the cutoff knows the Source that wrote it, or on
the day it was recorded when no Source did (`transition_known`). World stores one such
event per status change, and today a status changes only when a claim is imported
disputed or removed. The first producer of disputes by reading, the user's "dispute"
correction of a claim (`knowledge_world_claim_correct`, master plan 9.5, 0.1.9), must
write an event for every dispute, a second dispute of a disputed claim included, so a
cutoff that knows only one of its witnesses still shows the claim disputed.
The feed's rows, a comparison's groups, a lineage and the Lens section transitions
are that projection replayed reading by reading (`Replay`); until the first reading
of a Source the cutoff does not know, the stored history is that World's history,
and a reading of a Source a Memory reset removed still counts in it. An
ending that took effect after the reading that established it is a feed row on the
day it took effect. A Lens page that existed by the cutoff reads as it stood: each
section as the Lens log had it, its state from the state it was filed with (a filing
supersedes without a dated event, so this is no feed row) and the dated Lens events,
then from the replay's seals, where an end that took effect between two readings is
met at the next one; a page made later is left out with `as_of_later`.
World's As of control ([World as of a date](../ui/map-lens.md#world-as-of-a-date))
reads through the same cutoff. `asOf` on `knowledge_world_query`, `knowledge_map`,
`knowledge_world_changes` (a `since` read) and `knowledge_world_claim_history` takes a
calendar day from 1990-01-01 to today, as a chat turn's date does (`Cutoff::requested`);
a later day fails with `as_of_future`. The World view and the claim history are the
chat reads above; the map runs its unchanged code on the projected snapshot
(`world_as_of`), with Lens pages, Journal entries and method learning recorded by then,
and both World reads return `asOfReport {facts, sources}`: the relationships with no
evidence published by then, and the Sources World cites with no evidence that was.
`WorldGraphQuery.asOf` is the one cutoff field; `publishedBy` was removed.
The cutoff oracle (`commands/knowledge/as_of_oracle.rs`) checks every reader against
that World on generated Worlds; its reader table must name every Memory read tool and
`knowledge_*` command.

Writes: `source_save` keeps only material whose publication date is on or before
the cutoff; later material fails with `as_of_later` and undated material with an
`as_of_undated` argument error on `publicationTime`. `note_save`, `principle_save`
and `journal_save` save as usual, dated today. `thesis_update` and
`price_read_save` require a current turn ([asset pages](../knowledge/asset-pages.md#asset-pages));
`journal_review` waits for its check date ([Journal](../knowledge/sources-notes.md#working-notes-and-journal)).

The answer's `as_of_report` lists refused and empty reads and counts the Memory left
out by id, once per turn: facts (World relationships), sources, notes (notes, Journal
entries, chat messages and reports) and methods (methods and the standing
perspective). `conversation_memory` note and message searches count only later items
that would otherwise have been among the results shown.


## Composer date control

`/asof` turns the menu into a date panel: a year, month and day in the app language, from 1990-01-01 to today, One
month ago, Start of this year and One year ago, then Cancel and Set date. Enter opens
the focused dropdown and chooses the highlighted item. Escape closes an open dropdown,
then returns to the `/` menu. Typing `/asof 2026-03-03` and
Space or Enter sets the date directly; a date out of range opens the panel with its
error. The date is the first chip in the tray and never folds: its label reopens the
panel, × removes it, and Backspace in an empty composer removes it after every other
chip. It changes the placeholder, stays in the Chat's draft for follow-ups until
removed, and is sent as `as_of`, never as selected context. Queued messages keep
their own date and editing a sent message reopens it with its date. The request
shows an "As of {date}" tag above its bubble and the answer a "Known as of {date}"
tag in its meta line. In the answer, a step refused for the date reads "Can't look
back to {date} · {tool}" with a neutral mark and explains why when expanded; number
popovers add "Known as of {date}"; chart cards add the date after their title; and a note
between the answer and its receipt lists the reads that could not show the date, the
reads with nothing dated by then and what the Loupist learned later, from
`as_of_report` (four names per list, then "+n more"; no note when nothing is left to
say).

## Code

- As-of answers: `src-tauri/src/commands/chat_runtime/setup.rs`, `src-tauri/src/commands/knowledge/as_of.rs`, `src-tauri/src/commands/tool_executor/as_of_turn.rs`, `src-tauri/src/agent_harness.rs`, `src/chat/asOf.ts`, `src/components/chat/ComposerAsOf.tsx`, `src/components/chat/AsOfNote.tsx`, `src/components/knowledge/lookBack.ts`
- Publication day on the user's calendar: `src-tauri/src/knowledge/world_filter.rs` (`published_by`, `publication_day`)
- Oracles: `src-tauri/src/commands/knowledge/as_of_oracle.rs` (cutoff), `src-tauri/src/commands/tool_executor/taint_oracle.rs` (third-party text and standing writes)
