---
name: word-documents
description: Read, create or edit .docx Word files with the offline Python runtime. Use for Word deliverables, not ordinary chat prose.
---
# Word documents

Use `run_python` with `from docx import Document` to read and write `.docx`.
Inspect paragraphs, tables, styles, sections and headers before editing a supplied
file. Preserve unrelated content and use its styles; replacing paragraph text
can erase run formatting. Write a separate file under `/workspace/outputs/`.

Reopen with `Document(path)` and check the changed text, tables and sections.
The runtime has no Word renderer or conversion engine: do not promise PDF
conversion, page images or a visual layout check. Legacy `.doc`, tracked-change
editing and lossless preservation of specialized Word features are outside this
workflow. State any relevant preservation or visual limit.

Link the saved file. End documents made for others with this line in their
language: "Made with AI in Loupist. AI can be wrong. Not a licensed financial adviser."

To cite figures from an input file, ask the user to attach it to Chat. Python extraction is working material, not citable Source evidence.
