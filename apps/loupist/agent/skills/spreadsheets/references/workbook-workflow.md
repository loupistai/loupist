# Workbook workflow

Original Loupist guidance grounded in openpyxl's formula documentation.
See [sources](../../NOTICE.md).

## Inspect and edit

Identify input and output sheets, range boundaries, formulas, named ranges,
merged cells, tables, links, and requested edits. Preserve templates and
unrelated content. For macro-enabled workbooks, inspect preservation support
and keep executable content inert; do not execute macros to read a workbook.
A CSV is a values-only table, not a workbook with formulas or multiple sheets.

Keep actual data, assumptions, and derived formulas distinguishable. Use cell
references for a model expected to change with its assumptions. Label currency,
scale, time basis, and source/assumption provenance. Preserve exact financial
precision where necessary and avoid treating missing inputs as zero.

Inspect library limitations before saving workbooks with charts, external links,
pivots, or specialized features; verify retained features after reopening. Use
a distinct output unless replacement is authorized.

## Recalculate and validate

openpyxl writes formulas but does not evaluate them. Loading with data_only
reads cached results, which may be absent or stale. Use an installed Excel or
LibreOffice engine to recalculate a copy when the workflow needs evaluated
formulas. Do not claim fresh results from a save or cached values alone.

Check formula errors, representative cell references, totals, dimensions, and
scenario/sensitivity consistency. Independently recompute decisive outputs.
For a formatted deliverable, inspect a rendered sheet or print export when
possible. State which calculations and visual checks were actually completed.
