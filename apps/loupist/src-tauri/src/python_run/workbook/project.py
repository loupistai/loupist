"""Host-owned, read-only workspace preview. sheet_index is supplied by Rust."""
import datetime
import json
import math
import re
import zipfile

from defusedxml.ElementTree import iterparse
from openpyxl import load_workbook
from openpyxl.utils.cell import coordinate_to_tuple, range_boundaries

MAX_ROWS, MAX_COLS, MAX_TEXT = 200, 40, 1024
MAX_SHEETS = 64
# Leave room inside the supervisor's 64 KiB pipe for sheet names and metadata.
MAX_ROW_BYTES = 48 * 1024
BOOK = "workbook.xlsx"


def describe_sheet(archive, sheet):
    # Streaming read-only openpyxl drops merge and formula markers in data_only
    # mode. Inspect the worksheet XML for these and the actual occupied bounds;
    # a producer's optional dimension hint can be missing or stale.
    top = left = bottom = right = None
    formulas = set()
    merged_followers = set()
    with archive.open(sheet._worksheet_path) as source:
        row = col = 0
        for event, node in iterparse(source, events=("start", "end")):
            tag = node.tag.rsplit("}", 1)[-1]
            if event == "start":
                if tag == "row":
                    row = int(node.get("r", row + 1))
                    col = 0
                elif tag == "c":
                    col += 1
                    if "r" in node.attrib:
                        row, col = coordinate_to_tuple(node.attrib["r"])
                    if row < 1 or row > 1048576 or col < 1 or col > 16384:
                        raise ValueError("invalid cell coordinate")
                    if top is None:
                        top = bottom = row
                        left = right = col
                    top, left = min(top, row), min(left, col)
                    bottom, right = max(bottom, row), max(right, col)
                elif tag == "f" and top is not None and top <= row < top + MAX_ROWS:
                    # Columns can begin farther left later in the sheet.
                    if left <= col < left + MAX_COLS:
                        formulas.add((row, col))
                elif tag == "mergeCell":
                    lo_col, lo_row, hi_col, hi_row = range_boundaries(node.attrib["ref"])
                    if top is not None:
                        if hi_row > 1048576 or hi_col > 16384:
                            raise ValueError("invalid merge bounds")
                        bottom, right = max(bottom, hi_row), max(right, hi_col)
                        for r in range(max(lo_row, top), min(hi_row + 1, top + MAX_ROWS)):
                            for c in range(max(lo_col, left), min(hi_col + 1, left + MAX_COLS)):
                                if (r, c) != (lo_row, lo_col):
                                    merged_followers.add((r, c))
            else:
                node.clear()
    return top, left, bottom, right, formulas, merged_followers


def cell_value(cell, formula):
    value = cell.value
    fmt = getattr(cell, "number_format", None)
    if value is None:
        kind, text = "empty", ""
    elif cell.data_type == "e":
        kind, text = "error", str(value)
    elif isinstance(value, bool):
        kind, text = "boolean", "TRUE" if value else "FALSE"
    elif isinstance(value, (datetime.date, datetime.time, datetime.timedelta)):
        kind = "date"
        text = value.isoformat() if hasattr(value, "isoformat") else str(value)
        patterns = {"yyyy-mm-dd": "%Y-%m-%d", "mm/dd/yyyy": "%m/%d/%Y",
                    "dd/mm/yyyy": "%d/%m/%Y", "yyyy-mm-dd h:mm:ss": "%Y-%m-%d %-H:%M:%S",
                    "yyyy-mm-dd hh:mm:ss": "%Y-%m-%d %H:%M:%S", "hh:mm:ss": "%H:%M:%S"}
        if hasattr(value, "strftime") and (fmt or "").lower() in patterns:
            text = value.strftime(patterns[fmt.lower()])
    elif isinstance(value, (int, float)):
        kind, text = "number", str(value)
        pattern = re.fullmatch(r"(#,##)?0(?:\.(0{1,10}))?(%)?", fmt or "")
        if pattern and math.isfinite(value):
            text = format(value, ("," if pattern[1] else "") + "." +
                          str(len(pattern[2] or "")) + ("%" if pattern[3] else "f"))
    else:
        kind, text = "text", str(value)
    result = {"kind": kind, "display": text[:MAX_TEXT], "formula": formula}
    if fmt and fmt != "General":
        result["number_format"] = fmt[:MAX_TEXT]
    return result, len(text) > MAX_TEXT or len(fmt or "") > MAX_TEXT


def project():
    book = load_workbook(BOOK, read_only=True, data_only=True, keep_links=False, keep_vba=False)
    try:
        names = book.sheetnames
        if len(names) > MAX_SHEETS or any(len(s.encode("utf-8")) > 124 for s in names):
            return {"status": "limit_exceeded"}
        sheets = [{"name": s, "hidden": book[s].sheet_state != "visible",
                   "worksheet": hasattr(book[s], "iter_rows")} for s in names]
        if sheet_index is not None and not 0 <= sheet_index < len(names):
            return {"status": "unknown_sheet"}
        indices = ([sheet_index] if sheet_index is not None else
                   [i for i, s in enumerate(sheets) if s["worksheet"] and not s["hidden"]])
        selected = None
        bounds = (None, None, None, None, set(), set())
        with zipfile.ZipFile(BOOK) as archive:
            for i in indices:
                if sheets[i]["hidden"] or not sheets[i]["worksheet"]:
                    return {"status": "unsupported_sheet"}
                current = describe_sheet(archive, book[names[i]])
                if current[0] is not None or selected is None:
                    selected, bounds = i, current
                if current[0] is not None:
                    break
        top, left, bottom, right, formulas, merged_followers = bounds
        rows = []
        used = 0
        clipped = False
        total_rows = bottom - top + 1 if top is not None else 0
        total_cols = right - left + 1 if left is not None else 0
        if total_rows:
            values = book[names[selected]].iter_rows(min_row=top, min_col=left,
                max_row=min(bottom, top + MAX_ROWS - 1), max_col=min(right, left + MAX_COLS - 1))
            for r, cells in enumerate(values, top):
                row = []
                for c, cell in enumerate(cells, left):
                    if (r, c) in merged_followers:
                        item, short = {"kind": "empty", "display": "", "formula": False}, False
                    else:
                        item, short = cell_value(cell, (r, c) in formulas)
                    size = len(json.dumps(item, ensure_ascii=False, separators=(",", ":")).encode("utf-8")) + 1
                    if used + size + 2 > MAX_ROW_BYTES:
                        clipped = True
                        break
                    row.append(item)
                    used += size
                    clipped |= short
                if len(row) < len(cells) and rows:
                    break
                if row:
                    rows.append(row)
                    used += 2
                if len(row) < len(cells):
                    break
        return {"status": "ready", "sheets": sheets, "selected_sheet": selected,
                "first_row": top, "first_column": left, "rows_total": total_rows,
                "columns_total": total_cols, "rows": rows,
                "truncated": clipped or len(rows) < total_rows or total_cols > MAX_COLS,
                "omitted": ["charts", "images", "visual_styles", "comments", "macros"]}
    finally:
        book.close()


if __name__ == "__main__":
    try:
        result = project()
    except (MemoryError, OverflowError):
        result = {"status": "limit_exceeded"}
    except Exception:
        result = {"status": "invalid_workbook"}
    print(json.dumps(result, ensure_ascii=False, separators=(",", ":")))
