---
name: filings
description: Analyze filings, financial statements, earnings releases, guidance changes, or earnings quality, including capital decisions, ownership reports, dividends and investor flows where a connected source reads them. Use for a document-based financial review; skip a price lookup or an isolated calculation with complete supplied inputs.
---

# Filings and earnings review

Identify the requested period and document version before extracting results.
Use the company fiscal calendar: name a fiscal period with its end date, since
fiscal years end in different months, and keep each amount in the currency it
was reported in. A line the filing does not report is missing, not zero. A
historical review needs the requested release and cutoff, not automatically
the newest quarter.

Read [earnings review](references/earnings-review.md) for a results update,
guidance comparison, statement reconciliation, or earnings-quality assessment.
When the figures come from OpenDART or Korea Investment, read
[Korean statements](references/korea-statements.md) first: how those sources
report statement basis and periods, and which reads cover capital decisions,
ownership, dividends and investor flows. Read [filing changes](references/filing-changes.md)
when the question is what a filing changed against the one before it. Size the
review to the question; it need not become a coverage report.

Fetch the exact original through an enabled connection, inspected local file,
or public source under the common evidence rules. Preserve original bytes
separately from OCR or extracted tables. Check page and table coverage;
unresolved extraction is not empty content.

Lead with what changed and why it matters. Compute margins, growth and free
cash flow from the extracted figures with `calculate`, citing the statement.
Distinguish management explanations from your inference and keep unresolved
reconciliation items visible. Consensus is an estimate with its own date and
provider; do not invent a beat or miss without an appropriate comparison.
Retain supplied originals under the common Memory rules and create a report
when the requested outcome needs it.
