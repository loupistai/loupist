---
name: presentations
description: Read, create or edit .pptx slides with the offline Python runtime, using editable text, tables and shapes.
---
# Presentation files

Use `run_python` with `from pptx import Presentation` for `.pptx` files.
Inspect slide sizes, layouts, placeholders, text and tables before editing a
supplied deck. Use its actual layouts, preserve unrelated slides and write a
separate file under `/workspace/outputs/`. Keep units and source labels beside data.
Legacy `.ppt`, rendering, conversion to PDF and lossless preservation of animations
or specialized embedded content are outside this workflow.

Reopen with `Presentation(path)` and check slide count and changed text/tables.
There is no slide renderer: valid slide XML does not prove good visual layout.
Report the visual check as missing and link the saved file.

On the last slide of decks made for others, include this line in their language:
"Made with AI in Loupist. AI can be wrong. Not a licensed financial adviser."

Figures read with Python or from an attachment's extracted text carry no citation
link: name the file and page, sheet or slide beside each. A UTF-8 text or CSV file up
to 256 KiB can be cited with `workspace_capture`; for a PDF or Office original, ask the
user to add it with Add source, then read the saved Source.

Bundled PDF/chart fonts do not cover Korean, Japanese or Chinese; use Latin labels
or offer Word or Markdown for such content. No renderer checks font fallback.
