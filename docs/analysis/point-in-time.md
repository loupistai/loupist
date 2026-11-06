# Point-in-time answers

An as-of date answers only from what was known by then. The composer control sends `as_of` and never puts the date in the question text.

## Point-in-time answers

A Chat turn can carry an as-of date ([`/asof`](#composer-date-control),
[turn authority](../runtime/chat-turns.md#turn-authority-and-finalization)). Its answer then
uses only what was published or recorded by the end of that calendar day: each
tool and Memory read applies the cutoff in Rust, and the turn envelope tells the
model not to add later facts from its own knowledge.

| Under a cutoff | Rule |
| --- | --- |
| Web pages | Publication on or before the date; later or undated pages fail with `as_of_later` ([Data and provenance](datasets.md#data-and-provenance)) |
| DART, KRX, KIS, World Bank, DefiLlama, OpenBB | Dated rows only; current-only routes fail with `as_of_unsupported`, a read with nothing dated by then with `as_of_empty` |
| Saved datasets (`dataset_describe`, `dataset_query`, `dataset_export`) and `calculate` | Only revisions fetched under a cutoff on or before the date |
| Saved charts | `artifact_read` and `chart_query` need the chart's rows drawn from such a revision; otherwise `as_of_unsupported` asks for a fresh read and a new chart |
| Tables (`grid_publish`) | Every cell must be known by then: a cell citing a local file fails with `as_of_unsupported`, later material with `as_of_later`; the table keeps the date, and `artifact_read` of a table needs one built under a cutoff on or before it |
| Saved reports | A Markdown artifact written after the date is left out (`as_of_later`) |
| Saved Memory | [Memory under a cutoff](../knowledge/memory.md#memory-under-a-cutoff) |
| What the user follows | Watched names only, by symbol and without flags; holdings never reach the model |
| Principles | Apply as they are today |

Writes: `source_save` keeps only material whose publication date is on or before
the cutoff; later material fails with `as_of_later` and undated material with an
`as_of_undated` argument error on `publicationTime`. `note_save`, `principle_save`
and `journal_save` save as usual, dated today. `thesis_update` and
`earnings_snapshot_save` are refused and no asset observations are recorded ([asset pages](../knowledge/asset-pages.md#asset-pages));
`journal_review` waits for its check date ([Journal](../knowledge/sources-notes.md#working-notes-and-journal)).

The answer's `as_of_report` lists refused and empty reads and counts the Memory left
out by id, once per turn: facts (World relationships), sources, notes (notes, Journal
entries, chat messages and reports) and methods (methods and the standing
perspective). `conversation_memory` note and message searches count only later items
that would otherwise have been among the results shown.


## Composer date control

`/asof` turns the menu into a date panel: a year, month and day in the app language, from 1990-01-01 to today, One
month ago, Start of this year and One year ago, then Cancel and Set date (Enter
sets; Escape returns to the `/` menu it came from). Typing `/asof 2026-03-03` and
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
