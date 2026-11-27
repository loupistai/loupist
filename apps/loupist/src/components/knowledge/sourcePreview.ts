import i18n from "../../i18n";
import { formatNumber } from "../../i18n/format";
import { compactOrigin, sourceOrigin } from "../../knowledge/hostSource";

/** Rows and period from a catalog `table`, when the host has started sending one. */
export type SourceListTable = {
  statement?: string;
  rows?: number;
  totalRows?: number;
  period?: string;
};

const COLUMN_PART = 48;

/** The catalog summary of a structured Source is its column names joined by " · ". */
export function columnListSummary(summary: string | undefined) {
  if (!summary?.includes(" · ")) return false;
  const parts = summary.split(" · ").map(part => part.trim());
  return parts.length >= 2 && parts.every(part => part.length > 0 && part.length <= COLUMN_PART && !/[.!?]$/.test(part) && !part.includes("\n"));
}

export function sourceListTable(value: object): SourceListTable | undefined {
  const table = (value as { table?: unknown }).table;
  if (!table || typeof table !== "object") return undefined;
  const row = table as Record<string, unknown>;
  const statement = typeof row.statement === "string" ? row.statement.trim() : undefined;
  const rows = typeof row.rows === "number" ? row.rows : typeof row.totalRows === "number" ? row.totalRows : undefined;
  const period = typeof row.period === "string" ? row.period.trim() : undefined;
  if (!statement && rows == null && !period) return undefined;
  return { statement, rows, period };
}

/**
 * One line under a Library title. A structured Source names what it is, and
 * its rows and period when the catalog row carries them, instead of column names.
 */
export function sourcePreviewLine(source: { summary?: string; originLabel?: string; titleKey?: "followed"; table?: SourceListTable }) {
  const table = source.table;
  const structured = !!table || columnListSummary(source.summary);
  if (!structured) return source.summary;
  const what = table?.statement || compactOrigin(sourceOrigin(source)) || i18n.t("knowledge:sourceWorkspace.structuredData");
  const parts = [what];
  if (table?.rows != null) parts.push(i18n.t("knowledge:sourceWorkspace.previewRows", { count: table.rows, formattedCount: formatNumber(table.rows) }));
  if (table?.period) parts.push(table.period);
  return parts.join(" · ");
}
