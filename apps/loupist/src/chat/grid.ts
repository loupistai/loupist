import i18n from "@/i18n";
import type { AssetsGrid, ChatEvidenceRef, GridCell, GridEmptyCell, GridEmptyReason, GridValueCell } from "../types";
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

const quotedTsv = (text: string) => (/[\t\n\r"]/u.test(text) ? `"${text.replaceAll('"', '""')}"` : text);

/** Tab-separated header and rows for pasting into a spreadsheet; empty cells are blank. */
export function gridTsv(grid: AssetsGrid) {
  const header = [t("grid.asset"), ...grid.columns.map((column) => column.label)];
  const rows = grid.rows.map((row, index) => [row.label, ...grid.columns.map((_, column) => {
    const cell = grid.cells[index]?.[column];
    return cell && isValueCell(cell) ? cell.value : "";
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
        lines.push([...base, "", column.unit, "", cell?.empty ?? "not_checked", "", "", cell?.note ?? ""]);
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
  grid.rows.forEach((row, rowIndex) => {
    const cells = grid.columns.map((_, columnIndex) => {
      const cell = grid.cells[rowIndex]?.[columnIndex];
      if (!cell || isEmptyCell(cell)) return emptyReason(cell?.empty ?? "not_checked");
      const evidence = evidenceRefs.find((item) => item.id === cell.evidence_id);
      footnotes.push(`[^${footnotes.length + 1}]: ${exportFootnote(cell.check, evidence, evidenceRefs)}`);
      return `${cell.value}[^${footnotes.length}]`;
    });
    lines.push(`| ${[row.label, ...cells].map(markdownCell).join(" | ")} |`);
  });
  lines.push("");
  for (const note of grid.notes ?? []) lines.push(note, "");
  if (footnotes.length) lines.push(...footnotes, "");
  lines.push(`_${t("grid.exportNotice")}_`, "");
  return lines.join("\n");
}
