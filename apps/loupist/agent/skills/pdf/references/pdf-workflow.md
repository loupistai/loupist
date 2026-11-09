# PDF workflow

Original Loupist guidance grounded in the pypdf extraction documentation.
See [sources](../../NOTICE.md). `run_python` does not include pypdf, pdfplumber,
or a host extractor, and it cannot install packages. Use the file tools, and
say so when one of those is required.

## Read and extract

Inspect page count, rotation, encryption, text coverage, and whether each needed
page has text, images, or both. For text PDFs, read the file with the file
tools. Preserve page boundaries and check a visual
sample where reading order, columns, minus signs, or units affect interpretation.

Scans need OCR. `run_python` has no OCR tool and pypdf is not installed, so it
cannot recognize text inside an image. Say so. Keep OCR-derived text distinct from the original and verify
important figures against the page image. Do not OCR every page when only a
small range is needed. Tables may require cell boundaries and manual alignment;
plain extracted whitespace does not establish correct rows and columns.

Retain source file and page locators alongside extracted values. Inspect merged
headers, repeated page headers, footnotes, decimal separators, scales, and
negative-number conventions in financial tables. Track pages that failed.

## Modify or generate

Page selection, merge/split, rotation, metadata, and form fields need a PDF
library that `run_python` does not include. Say so, and use the file tools for
what they can inspect. Inspect actual form field names and types before filling them;
verify the written values and appearance afterward. A visible overlaid label
is not an edited form field. Preserve signatures when required and identify
when a requested edit would invalidate them.

A new PDF needs a layout library that `run_python` does not include. Say so
instead of installing one. Use a font supporting the document language and embed it when needed.
Write a separate output and reopen it to check page count and searchable text.

`run_python` cannot render pages with a host PDF tool. Report the structural
checks performed and the visual limit.
