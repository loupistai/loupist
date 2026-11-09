---
name: spreadsheets
description: Read, build, or edit spreadsheet files such as .xlsx and .xlsm, including formulas, financial models, formatting, and charts. Use when workbook behavior matters; ordinary CSV analysis can use data-analysis.
---

# Spreadsheet files

Inspect the workbook, sheet names, used ranges, formulas, and requested cells.
Read [workbook workflow](references/workbook-workflow.md) for formula-aware
edits, preserving workbook features, recalculation, and verification.

`run_python` cannot install packages and does not include spreadsheet libraries
such as openpyxl. Use the file tools, and say so when a named library is
unavailable. Reuse the supplied template and preserve
unrelated sheets, formulas, styles, and workbook features. Keep model inputs,
assumptions, and derived cells distinct. Reference inputs rather than repeating
hardcoded values through a reusable financial model.

`run_python` cannot recalculate a workbook with a spreadsheet engine. A successful
save or cached value alone does not establish recalculation. Check decisive
outputs independently and report calculation limits. Pair with an available
financial Skill only when its analytical workflow is needed.
