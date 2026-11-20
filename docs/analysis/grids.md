# Grids and conflict cards

A published grid checks each cell and supports sorting and export for asset
comparisons. The conflict layout puts the sources side by side. Prose and
Markdown tables can also compare assets or explain a disagreement. In any form,
cite comparable values with their periods and bases, explain gaps, and give the
reason for using one source or for leaving the choice open.

### Grid cells

`grid_publish` (`commands/tool_executor/grid.rs`; stored as a `grid` artifact,
`loupist-grid/1`, by `chat/grid.rs`) checks every value the way it checks a number
link, and model text never sets a status. A value cell `{value, citation_url}` must cite a
result of this turn; its value is read in its column's unit (a bare number in a percent
or multiple column is that unit, and a small integer still counts) and gets an ordinary
`NumberCheck`, while a date or text value is looked for as written in the cited excerpt
or payload (`matched` or `not_found`). The result returns `cell_checks`, `empty_cells`
and, when a cell `differs` or is `not_found`, a hint to fix it.

Layout `assets` takes 2–12 rows (symbol, market and label, each identity once; an
exchange-style identity also names its asset page, opened with the stored id,
pair and name as of the table's message or displayed revision, see
[saved links](../knowledge/asset-pages.md#saved-links)) and 1–8 columns (id, label, optional
period and basis, unit `amount`, `percent`, `multiple`, `number` or `date`), cells in
row-then-column order and at most three notes. An empty cell names `not_reported`,
`not_applicable`, `unavailable` or `not_checked` with an optional note; it is never a
zero. Layout `conflict` takes 2–6 reports, each citing a different source (`says` ≤ 120
characters; a number in it is checked against that source), a reason (`basis`,
`period`, `preliminary_vs_final`, `estimate_vs_reported`, `currency_or_unit`,
`adjusted_vs_reported`, `timing`, `unclear`), an explanation of at most 280 characters,
`used` (a report index, or null when the answer cannot tell) with an optional reason,
and optionally `world_entity_id`, which must be a node this turn's `world_read` delivered;
reports that agree within rounding, or say the same words, are refused. A report's date
comes from its evidence's publication date, the matched row's filing date or the
result's own, never retrieval time. A stored grid is at most 256 KiB. A turn may publish
four distinct documents, charts or tables, saved only with the completed reply.
A revision uses the edit token from its delivered publish result or `artifact_read`
and the whole table. Revising a delivered draft replaces it at the same revision
number without using another slot; only the final draft is stored. `chat_artifact_read` returns the
grid's evidence from its source message, so chips and exports work after a reload.
Exports keep sources: CSV with sources has one row per cell (asset, symbol, market,
measure, period, basis, value, unit, currency, status, source, source date, and the
source URL or a calculated cell's formula with its inputs' sources), UTF-8 with a byte
order mark and the AI notice row last; the Markdown table adds a footnote per value.
A money value with more than two decimals is shown, copied and written to the Markdown
table rounded to two ("₩32.765719조" reads "₩32.77조", "$394.3281bn" "$394.33bn"); its
number chip then says it was rounded and gives the exact source value, and CSV with
sources keeps the value as published. Within one column the values the table rounds show
one number of decimals, the most any money value in that column shows (at most two), so
"₩43.600123조" reads "₩43.60조" under "₩47.21조" rather than "₩43.6조"; a value the model
wrote with two decimals or fewer stays exactly as written (`gridDecimals`, `grid.ts`).
Percents, multiples and other values show as written.
A measure column is 112–168 px wide and never narrower than its widest value or period
line, so a long amount ("333,605,938,000,000원") never runs into the next column.
When the columns do not fit, a table with more measures than assets turns, with
measures as rows and assets as columns; any other table scrolls sideways with the
next column peeking in.

## Code

- Grids and conflict cards: `src-tauri/src/chat/grid.rs`, `src-tauri/src/commands/tool_executor/grid.rs`, `src-tauri/src/commands/records.rs`, `src/chat/grid.ts`, `src/components/chat/GridCard.tsx`
