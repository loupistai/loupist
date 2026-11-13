import i18n from "@/i18n";
import type { AssetsGrid, ChatEvidenceRef, GridCell, GridEmptyCell, GridEmptyReason, GridValueCell } from "../types";
import { citedPlainText } from "./citedText";
import { sourceIdentityLabel } from "./evidence";
import { exportFootnote, needsChecking } from "./numberChecks";

/**
 * Presentation helpers for grid answers (finance-grid-answers.md §2.2, §3.3).
 * Rust stores each value, its check and its base-unit sort key; nothing here
 * recomputes a value or a status.
 */
const t = i18n.getFixedT(null, "chat");

export const isValueCell = (cell: GridCell): cell is GridValueCell => "value" in cell;
export const isEmptyCell = (cell: GridCell): cell is GridEmptyCell => "empty" in cell;

const EMPTY_KEYS: Record<GridEmptyReason, "notReported" | "notApplicable" | "unavailable" | "notChecked"> = {
  not_reported: "notReported", not_applicable: "notApplicable", unavailable: "unavailable", not_checked: "notChecked",
};

/** "Not reported", "Doesn't apply", "Couldn't get" or "Not checked" in the interface language. */
export const emptyReason = (reason: GridEmptyReason) => t(`grid.empty.${EMPTY_KEYS[reason]}`);

/** Sort state of one column: high to low, then low to high, then the original order. */
export type GridSort = { column: number; direction: "desc" | "asc" } | null;

export function nextSort(current: GridSort, column: number): GridSort {
  if (!current || current.column !== column) return { column, direction: "desc" };
  return current.direction === "desc" ? { column, direction: "asc" } : null;
}

/** Row order for a sort: values by Rust's sort key (dates and text by their printed value); empty cells always last. */
export function sortedRows(grid: AssetsGrid, sort: GridSort): number[] {
  const order = grid.rows.map((_, index) => index);
  if (!sort) return order;
  const sign = sort.direction === "desc" ? -1 : 1;
  const key = (row: number) => {
    const cell = grid.cells[row]?.[sort.column];
    if (!cell || !isValueCell(cell)) return null;
    return cell.sort ?? cell.value;
  };
  return order.sort((left, right) => {
    const a = key(left);
    const b = key(right);
    if (a === null || b === null) return a === null && b === null ? left - right : a === null ? 1 : -1;
    const compared = typeof a === "number" && typeof b === "number" ? a - b : String(a).localeCompare(String(b));
    return compared === 0 ? left - right : sign * compared;
  });
}

/** Linked value cells and cells to check, for the caption. */
export function gridCounts(grid: AssetsGrid) {
  const values = grid.cells.flat().filter(isValueCell);
  return { linked: values.length, toCheck: values.filter((cell) => needsChecking(cell.check)).length };
}

// One number in an amount, with more than two decimals: "₩32.765719조", "$394.3281bn".
const LONG_AMOUNT = /^([^\d]*?)(\d[\d,]*)\.(\d{3,})([^\d]*)$/u;
const CURRENCY_MARK = /[$₩€£¥￦]|원|달러|엔|유로|\b[A-Z]{3}\b/u;

// One number in an amount, with two decimals or fewer, as the model wrote it: "₩47.21조", "$1.2bn".
const SHORT_AMOUNT = /^[^\d]*?\d[\d,]*(?:\.(\d{1,2}))?[^\d]*$/u;

/** A money value's long tail rounded to two places, without trailing zeros: the fewest decimals it needs. */
function roundedAmount(text: string) {
  if (!CURRENCY_MARK.test(text)) return null;
  const match = LONG_AMOUNT.exec(text);
  if (!match) return null;
  const [, before, whole, fraction, after] = match;
  const rounded = Number(`${whole!.replaceAll(",", "")}.${fraction}`).toFixed(2).replace(/\.?0+$/u, "");
  const [integer, decimals = ""] = rounded.split(".");
  // Keep the model's grouping: a comma-grouped whole part stays grouped.
  const grouped = whole!.includes(",") ? integer!.replace(/\B(?=(\d{3})+(?!\d))/gu, ",") : integer!;
  return { before: before!, integer: grouped, decimals, after: after! };
}

/** The decimals one money value shows on its own: as rounded, or as the model wrote it; 0 for anything else. */
function amountDecimals(text: string): number {
  const rounded = roundedAmount(text);
  if (rounded) return rounded.decimals.length;
  return CURRENCY_MARK.test(text) ? SHORT_AMOUNT.exec(text)?.[1]?.length ?? 0 : 0;
}

/**
 * How many decimals the values a table rounds show in one column: the most any
 * money value in the column shows (at most two), so the column reads evenly
 * ("₩43.60조" under "₩47.21조", never "₩43.6조"). Values the model wrote
 * with two decimals or fewer count, but are themselves shown as written.
 */
export function columnDecimals(values: readonly string[]): number {
  return Math.min(2, Math.max(0, ...values.map(amountDecimals)));
}

/** `columnDecimals` for each column of a grid's value cells. */
export function gridDecimals(grid: AssetsGrid): number[] {
  return grid.columns.map((_, column) => columnDecimals(grid.cells.flatMap((row) => {
    const cell = row[column];
    return cell && isValueCell(cell) ? [cell.value] : [];
  })));
}

/**
 * A money value as a table shows it: the model's own words, with a long tail of
 * decimals rounded to two ("₩32.765719조" → "₩32.77조", "$394.3281bn" →
 * "$394.33bn"), as the answer prints amounts, padded to its column's
 * `decimals` ("₩43.600123조" → "₩43.60조" beside "₩47.21조"). Anything else
 * (two decimals or fewer, a percent, a multiple, a 조-and-억 amount) is shown
 * as written. The exact value stays in the cell's number chip.
 */
export function shownAmount(text: string, decimals = 0): string {
  const rounded = roundedAmount(text);
  if (!rounded) return text;
  const tail = rounded.decimals.padEnd(Math.min(2, decimals), "0");
  return `${rounded.before}${rounded.integer}${tail ? `.${tail}` : ""}${rounded.after}`;
}

/** A value cell's check as its chip reads it: about the shown text, so a rounded tail offers the exact value. */
export function shownCheck(cell: GridValueCell, decimals = 0): GridValueCell["check"] {
  const shown = shownAmount(cell.value, decimals);
  if (shown === cell.value) return cell.check;
  return { ...cell.check, text: shown, status: cell.check.status === "matched" ? "rounded" : cell.check.status };
}

const quotedTsv =(text: string) => (/[\t\n\r"]/u.test(text) ? `"${text.replaceAll('"', '""')}"` : text);

/** Tab-separated header and rows for pasting into a spreadsheet; empty cells are blank. */
export function gridTsv(grid: AssetsGrid) {
  const decimals = gridDecimals(grid);
  const header = [t("grid.asset"), ...grid.columns.map((column) => column.label)];
  const rows = grid.rows.map((row, index) => [row.label, ...grid.columns.map((_, column) => {
    const cell = grid.cells[index]?.[column];
    return cell && isValueCell(cell) ? shownAmount(cell.value, decimals[column]) : "";
  })]);
  return [header, ...rows].map((line) => line.map(quotedTsv).join("\t")).join("\n");
}

const CSV_HEADER = ["asset", "symbol", "market", "measure", "period", "basis", "value", "unit", "currency", "status", "source", "source_date", "source_url_or_formula"];

const csvCell = (value: string) => (/[",\n\r]/u.test(value) ? `"${value.replaceAll('"', '""')}"` : value);

/** The currency a printed amount names, as an ISO code; blank when it names none. */
export function currencyOf(value: string): string {
  const text = value.trim();
  const code = /\b(KRW|USD|EUR|JPY|GBP|CNY|HKD|TWD|SGD|AUD|CAD|CHF)\b/u.exec(text.toUpperCase());
  if (code) return code[1]!;
  if (/[₩￦]|원/u.test(text)) return "KRW";
  if (/€|유로/u.test(text)) return "EUR";
  if (/£/u.test(text)) return "GBP";
  if (/¥|엔/u.test(text)) return "JPY";
  if (/\$|달러/u.test(text)) return "USD";
  return "";
}

/** A model-written note for a file: each citation target becomes its source's name. */
const noteText = (note: string, evidenceRefs: readonly ChatEvidenceRef[]) => citedPlainText(note, (id) => {
  const evidence = evidenceRefs.find((item) => item.id === id);
  return evidence ? evidence.provider ?? sourceIdentityLabel(evidence) : t("citedNote.source");
});

const isoDay = (evidence: ChatEvidenceRef | undefined) => (evidence?.published_at ?? evidence?.retrieved_at ?? "").slice(0, 10);

/**
 * "CSV with sources": one row per cell (tidy) with its source or formula, the
 * notice row last, UTF-8 with a byte-order mark so spreadsheet apps read Hangul.
 * Headers stay English (data file convention).
 */
export function gridCsv(grid: AssetsGrid, evidenceRefs: readonly ChatEvidenceRef[]) {
  const lines = [CSV_HEADER];
  grid.rows.forEach((row, rowIndex) => {
    grid.columns.forEach((column, columnIndex) => {
      const cell = grid.cells[rowIndex]?.[columnIndex];
      const base = [row.label, row.symbol, row.market, column.label, column.period ?? "", column.basis ?? ""];
      if (!cell || isEmptyCell(cell)) {
        lines.push([...base, "", column.unit, "", cell?.empty ?? "not_checked", "", "", cell?.note ? noteText(cell.note, evidenceRefs) : ""]);
        return;
      }
      const evidence = evidenceRefs.find((item) => item.id === cell.evidence_id);
      const currency = column.unit === "amount" ? currencyOf(cell.value) : "";
      const calc = cell.check.calc;
      if (calc) {
        // A calculated cell names its formula with the inputs substituted, and the inputs' sources.
        const inputs = calc.inputs.flatMap((input) => evidenceRefs.filter((item) => item.id === input.evidence_id));
        const sources = [...new Set(inputs.map((item) => item.provider ?? sourceIdentityLabel(item)))].join("; ");
        lines.push([...base, cell.value, column.unit, currency, cell.check.status, sources, isoDay(inputs[0]), calcEquation(calc)]);
        return;
      }
      const source = evidence ? evidence.provider ?? sourceIdentityLabel(evidence) : "";
      lines.push([...base, cell.value, column.unit, currency, cell.check.status, source, isoDay(evidence), evidence?.url ?? ""]);
    });
  });
  const csv = lines.map((line) => line.map(csvCell).join(","));
  // The notice row is quoted as a sentence even without a comma.
  csv.push(`Note,,,,,,,,,,"${t("grid.exportNotice").replaceAll('"', '""')}",,`);
  return `﻿${csv.join("\r\n")}\r\n`;
}

/** "Operating margin = 38.1 / 452.3 = 8.42%": the calculation's title with its first formula's substituted values. */
export function calcEquation(calc: NonNullable<GridValueCell["check"]["calc"]>) {
  const formula = calc.formulas[0] ?? "";
  const at = formula.indexOf(" = ");
  return at < 0 ? formula : `${calc.title}${formula.slice(at)}`;
}

const markdownCell = (text: string) => text.replaceAll("|", "\\|").replace(/\s+/gu, " ");

/** "Markdown table": the grid as shown, a footnote per value with its source or formula, notes, then the AI notice. */
export function gridMarkdown(title: string, grid: AssetsGrid, evidenceRefs: readonly ChatEvidenceRef[]) {
  const lines = [`# ${title}`, ""];
  if (grid.subtitle) lines.push(grid.subtitle, "");
  const header = [t("grid.asset"), ...grid.columns.map((column) => [column.label, column.period].filter(Boolean).join(" · "))];
  lines.push(`| ${header.map(markdownCell).join(" | ")} |`, `| ${header.map((_, index) => (index === 0 ? "---" : "---:")).join(" | ")} |`);
  const footnotes: string[] = [];
  const decimals = gridDecimals(grid);
  grid.rows.forEach((row, rowIndex) => {
    const cells = grid.columns.map((_, columnIndex) => {
      const cell = grid.cells[rowIndex]?.[columnIndex];
      if (!cell || isEmptyCell(cell)) return emptyReason(cell?.empty ?? "not_checked");
      const evidence = evidenceRefs.find((item) => item.id === cell.evidence_id);
      footnotes.push(`[^${footnotes.length + 1}]: ${exportFootnote(cell.check, evidence, evidenceRefs)}`);
      return `${shownAmount(cell.value, decimals[columnIndex])}[^${footnotes.length}]`;
    });
    lines.push(`| ${[row.label, ...cells].map(markdownCell).join(" | ")} |`);
  });
  lines.push("");
  for (const note of grid.notes ?? []) lines.push(noteText(note, evidenceRefs), "");
  if (footnotes.length) lines.push(...footnotes, "");
  lines.push(`_${t("grid.exportNotice")}_`, "");
  return lines.join("\n");
}
