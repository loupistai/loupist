# PDF workflow

Original Loupist guidance grounded in the pypdf extraction documentation
and local-tool capabilities. See [sources](../../NOTICE.md).

## Read and extract

Inspect page count, rotation, encryption, text coverage, and whether each needed
page has text, images, or both. For text PDFs, use a discovered extractor such
as pdftotext, pypdf, or pdfplumber. Preserve page boundaries and check a visual
sample where reading order, columns, minus signs, or units affect interpretation.

Scans require OCR through an available OCR tool; pypdf does not recognize text
inside an image. Keep OCR-derived text distinct from the original and verify
important figures against the page image. Do not OCR every page when only a
small range is needed. Tables may require cell boundaries and manual alignment;
plain extracted whitespace does not establish correct rows and columns.

Retain source file and page locators alongside extracted values. Inspect merged
headers, repeated page headers, footnotes, decimal separators, scales, and
negative-number conventions in financial tables. Track pages that failed.

## Modify or generate

Use an available PDF library for page selection, merge/split, rotation, metadata,
or form fields. Inspect actual form field names and types before filling them;
verify the written values and appearance afterward. A visible overlaid label
is not an edited form field. Preserve signatures when required and identify
when a requested edit would invalidate them.

For a new PDF, choose an installed layout tool such as ReportLab or an office
export. Use a font supporting the document language and embed it when needed.
Write a separate output and reopen it to check page count and searchable text.

Render affected pages with an installed PDF renderer, such as pdftoppm, and
inspect text clipping, tables, glyphs, pagination, and images. If rendering is
unavailable, report the structural checks performed and the visual limit.
