import i18n from "@/i18n";
import { formatDate } from "@/i18n/format";

/** The earliest as-of date (finance-point-in-time.md §3.1). */
export const EARLIEST_AS_OF = "1990-01-01";

const pad = (value: number) => String(value).padStart(2, "0");

/** A local calendar day as `YYYY-MM-DD`. */
export const isoDay = (date: Date) => `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;

/** Today on this device, the latest as-of date. */
export const todayIso = (now = new Date()) => isoDay(now);

const ISO_DAY = /^\d{4}-\d{2}-\d{2}$/u;

/** Whether `value` names a real calendar day. */
export function isCalendarDay(value: string) {
  if (!ISO_DAY.test(value)) return false;
  const [year, month, day] = value.split("-").map(Number);
  const date = new Date(Date.UTC(year!, month! - 1, day!));
  return date.getUTCFullYear() === year && date.getUTCMonth() === month! - 1 && date.getUTCDate() === day;
}

/** Why a date cannot be the as-of date, or null when it can. Rust checks again. */
export function asOfProblem(value: string, now = new Date()): "invalid" | "future" | "early" | null {
  if (!isCalendarDay(value)) return "invalid";
  if (value > todayIso(now)) return "future";
  if (value < EARLIEST_AS_OF) return "early";
  return null;
}

/**
 * "Mar 3, 2026" (medium) or "March 3, 2026" (long) for an as-of day, in the
 * interface language. Korean always spells it out ("2026년 3월 3일"): its
 * medium form is numeric ("2026. 3. 3.") and reads poorly before 기준.
 */
export const formatAsOf = (value: string, style: "medium" | "long" = "medium") =>
  isCalendarDay(value) ? formatDate(Date.parse(`${value}T00:00:00Z`), { timeZone: "UTC", dateStyle: /^ko(?:-|$)/iu.test(i18n.language) ? "long" : style }) : value;

/** The panel's quick picks, from today on this device. */
export function quickAsOf(kind: "month" | "yearStart" | "year", now = new Date()) {
  const date = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  if (kind === "month") date.setMonth(date.getMonth() - 1);
  if (kind === "yearStart") date.setMonth(0, 1);
  if (kind === "year") date.setFullYear(date.getFullYear() - 1);
  return isoDay(date);
}

/** `/asof 2026-03-03` typed in the composer, before the cursor; only ISO days are read from text. */
export function typedAsOf(text: string): { date: string; start: number; end: number } | null {
  const match = text.match(/(^|\s)\/asof\s+(\d{4}-\d{2}-\d{2})\s?$/u);
  if (!match || match.index === undefined) return null;
  const start = match.index + match[1]!.length;
  return { date: match[2]!, start, end: text.length };
}

/** A creation timestamp's local calendar day, for Look back. */
export const localDayOf = (ms: number) => isoDay(new Date(ms));
