import i18n from "../i18n";
import { formatDate, formatDateTime, formatNumber } from "../i18n/format";
import type { ClaimStatus, Coverage, TimePoint } from "../contracts/knowledge";

/** Preserve source precision and avoid shifting a date-only value across time zones. */
export function dateValue(value: string): string {
  if (/^\d{4}$/.test(value)) return formatDate(new Date(`${value}-01-01T00:00:00Z`), { year: "numeric", month: undefined, day: undefined, timeZone: "UTC" });
  if (/^\d{4}-\d{2}$/.test(value)) {
    const month = new Date(`${value}-01T00:00:00Z`);
    return Number.isNaN(month.getTime()) ? value : formatDate(month, { year: "numeric", month: "short", day: undefined, timeZone: "UTC" });
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return /^\d{4}-\d{2}-\d{2}$/.test(value)
    ? formatDate(date, { timeZone: "UTC" })
    : formatDateTime(date);
}

export function timePointValue(time?: TimePoint): string {
  if (!time || time.precision === "unknown" || !time.value) return i18n.t("knowledge:presentation.unknownTime");
  const date = dateValue(time.value);
  return time.approximate ? i18n.t("knowledge:presentation.approximateTime", { date }) : date;
}

export function spanSelectorLabel(selector: string): string {
  const trimmed = selector.trim();
  const lines = (start: number, end: number) => start === end
    ? i18n.t("knowledge:presentation.line", { line: formatNumber(start) })
    : i18n.t("knowledge:presentation.lineRange", { start: formatNumber(start), end: formatNumber(end) });
  if (trimmed.startsWith("{")) {
    try {
      const value = JSON.parse(trimmed) as Record<string, unknown>;
      if (value.kind === "utf8_line_range" && typeof value.startLine === "number" && typeof value.endLine === "number") return lines(value.startLine, value.endLine);
      if (value.kind === "row" && typeof value.index === "number") return i18n.t("knowledge:presentation.row", { index: formatNumber(value.index) });
      if (value.kind === "multi") {
        const count = Array.isArray(value.documents) ? value.documents.length : 0;
        return count ? i18n.t("knowledge:presentation.documents", { count, countText: formatNumber(count) }) : i18n.t("knowledge:presentation.multipleDocuments");
      }
    } catch { /* Unstructured source selectors have no display label. */ }
  }
  const match = trimmed.match(/^utf8_line_range:(\d+)-(\d+)$/);
  return match ? lines(Number(match[1]), Number(match[2])) : "";
}

export const claimStatusLabel = (status: ClaimStatus) => metadataLabel({
  active: i18n.t("knowledge:presentation.active"),
  disputed: i18n.t("knowledge:presentation.disputed"),
  withdrawn: i18n.t("knowledge:presentation.withdrawn"),
}, status);

/** Newer hosts can return metadata values a reader has no label for yet. */
export function metadataLabel(labels: Record<string, string>, value: string): string {
  return Object.hasOwn(labels, value) ? labels[value]! : i18n.t("knowledge:presentation.unspecified");
}

export function coverageLabel(value: string): string {
  const sourceCoverage = {
    included_full: i18n.t("knowledge:sourceWorkspace.fullDocument"),
    included_excerpt: i18n.t("knowledge:sourceWorkspace.excerpt"),
    upstream_truncated: i18n.t("knowledge:sourceWorkspace.incompleteSourceText"),
    metadata_only: i18n.t("knowledge:sourceWorkspace.sourceDetailsOnly"),
    omitted: i18n.t("knowledge:sourceWorkspace.contentNotSaved"),
  } satisfies Record<Coverage, string>;
  return metadataLabel({
    ...sourceCoverage,
    complete: i18n.t("knowledge:presentation.fullResult"),
    bounded_sample: i18n.t("knowledge:presentation.limitedSample"),
    untrusted_web: i18n.t("knowledge:presentation.unverifiedWeb"),
    snippet: i18n.t("knowledge:sourceWorkspace.excerpt"),
    live_generation: i18n.t("knowledge:presentation.currentMap"),
    sealed_snapshot: i18n.t("knowledge:presentation.savedSnapshot"),
  }, value);
}
