---
name: spreadsheets
description: Read or write .xlsx workbooks and CSV/TSV tables with the offline Python runtime, including stored formulas and basic formatting.
---
# Spreadsheet files

Use `run_python` with `openpyxl.load_workbook` and `openpyxl.Workbook` for `.xlsx`;
use `pandas.read_csv` and `DataFrame.to_csv` for CSV/TSV (set `sep="\t"` for TSV).
Inspect sheet names, ranges, formulas and styles before editing. Preserve unrelated
cells, label units and sources, and write a separate file under `/workspace/outputs/`.
Use XlsxWriter for new formatted workbooks and native charts. Read legacy `.xls`
with `xlrd.open_workbook(path, use_mmap=False)`; WASI has no memory mapping.
Macros are not run and specialized features may not survive a save. CSV/TSV stores
values only, without sheets, styles or formulas.

Reopen the result and check representative cells, formulas and dimensions.
openpyxl stores formulas but does not calculate them; `data_only=True` returns
possibly stale or absent caches. Independently check decisive results with
`calculate` or Python. There is no spreadsheet engine or sheet renderer: report
missing recalculation and visual checks. Link the saved file.

Figures read with Python or from an attachment's extracted text carry no citation
link: name the file and page, sheet or slide beside each. A UTF-8 text or CSV file up
to 256 KiB can be cited with `workspace_capture`; for a PDF or Office original, ask the
user to add it with Add source, then read the saved Source.
