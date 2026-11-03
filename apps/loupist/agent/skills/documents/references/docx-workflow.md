# Word workflow

Original Loupist guidance grounded in python-docx's document model.
See [sources](../../NOTICE.md).

## Inspect and preserve

Identify paragraphs, runs, tables, sections, page geometry, headers/footers,
styles, images, fields, hyperlinks, and any tracked revisions relevant to the
edit. Text inside tables or headers is not covered by the paragraph body alone.
A DOCX package also contains relationships and linked parts; preserve their
integrity when an edit needs lower-level XML handling.

Use an existing template when supplied. For targeted edits, change the relevant
runs or elements without replacing all paragraph text and losing formatting.
Check the selected library's support before a round trip involving comments,
tracked changes, embedded objects, or other specialized features. Keep a copy
of the original and verify those features after saving.

## Author and verify

Use paragraph and heading styles, proper list structures, explicit section
settings, and tables with readable widths. Include source titles and links for
material financial figures under the common evidence rules. Keep headings with
the following paragraph and inspect page breaks around large tables.

Reopen the saved file and check text, table contents, images, and section count.
For page-quality claims, export a copy through an installed office engine and
inspect the rendered pages. Verify Korean and other non-Latin glyphs, headers,
footers, long tables, page numbering, and links. A successful python-docx save
is not a rendered layout check. Report unsupported features or missing rendering.
