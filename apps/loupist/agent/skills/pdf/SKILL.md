---
name: pdf
description: Extract text, assemble pages or generate text PDFs with the offline Python runtime. Scans and visual PDF editing are outside this workflow.
---
# PDF files

Use `run_python` with `pypdf.PdfReader` for page counts and text extraction,
`pypdf.PdfWriter` for page selection, copying, merging and rotation, pdfplumber
for tables and positioned text (verify row/column alignment), and
`reportlab.pdfgen.canvas.Canvas` for new text PDFs. Preserve page locators with
extracted figures. Empty text does not prove an empty page; scans need OCR,
which this runtime cannot perform. Extracted whitespace does not establish table cells.

Keep the original and write a distinct file under `/workspace/outputs/`. Reopen
with `PdfReader` and check page count and searchable text. There is no page
renderer: report that visual layout, image content and glyph appearance were not
verified. Use only fonts already available and suitable for the requested language;
do not claim arbitrary form editing, signature preservation or PDF-to-Word conversion.

Link the saved file. End PDFs made for others with this line in their language:
"Made with AI in Loupist. AI can be wrong. Not a licensed financial adviser."

To cite figures from an input file, ask the user to attach it to Chat (or fetch
a PDF URL). Python extraction is working material, not citable Source evidence.

Bundled PDF/chart fonts do not cover Korean, Japanese or Chinese; use Latin labels
or offer Word or Markdown for such content. No renderer checks font fallback.
