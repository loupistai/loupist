---
name: documents
description: Create, inspect, or edit Word documents and templates (.docx/.dotx), including tables, styles, headers, citations, and revisions. Use for Word-file deliverables; plain chat prose needs no document workflow.
---

# Word documents

Inspect the input document or template before editing. Read
[Word workflow](references/docx-workflow.md) for document structure,
format-preserving edits, conversion, and page verification.

`run_python` cannot install packages and does not include office libraries such
as python-docx. Use the file tools to inspect and write the document, and say
so when a named library is unavailable. Match the user template, page settings, language, and output format.
Preserve document structure and unrelated content; a text-only extraction is
not a complete representation of tables, headers, fields, or revisions.

For authored or changed pages, `run_python` cannot render with an office engine.
Validate structure and disclose the visual check still missing. Link the saved
workspace deliverable; do not claim the app has opened a native Word preview.

End a document you write for others with this line in the document's language:
"Made with AI in Loupist. AI can be wrong. Not a licensed financial adviser."
