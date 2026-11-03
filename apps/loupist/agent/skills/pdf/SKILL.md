---
name: pdf
description: Read, extract, edit, assemble, fill, or generate PDF files, including scanned pages and financial tables. Use when PDF structure, OCR, forms, or page layout matter.
---

# PDF files

Inspect the actual document and requested page range before choosing an
extraction or editing route. Read [PDF workflow](references/pdf-workflow.md)
for text/table extraction, OCR, forms, assembly, or generation.

Use discovered local tools and project-scoped dependencies. Preserve the
original and write requested changes to a distinct output unless replacing it
is part of the task. Verify text and page coverage; an empty extraction is not
proof of an empty document. Keep page locators with extracted financial values.

For a generated or edited PDF, check the resulting page count and content and
render affected pages for visual inspection when a renderer is available.
Report any unresolved extraction or layout limitation accurately. A file write
alone does not establish a valid or visually checked PDF.

End a PDF you generate for others with this line in the document's language:
"Made with AI in Loupist. AI can be wrong. Not a licensed financial adviser."
