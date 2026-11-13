import { vega } from "vega-embed";
import { amountScale, currencyName, formatAmount, formatDate, formatNumber } from "@/i18n/format";
import i18n from "@/i18n";

/**
 * How an analytic chart prints its numbers and periods, in the interface
 * language and at the scales the app prints amounts with (`formatAmount`).
 * Vega evaluates these as expression functions, so the axes, the data labels,
 * the tooltips and "Save as PNG…" (the same view drawn again) all agree.
 */

const t = i18n.getFixedT(null, "chat");

/** d3's tick step for about `count` ticks over [lo, hi]: 1, 2 or 5 × a power of ten. */
function tickStep(lo: number, hi: number, count: number) {
  const raw = Math.abs(hi - lo) / Math.max(1, count);
  if (!(raw > 0) || !Number.isFinite(raw)) return 1;
  const power = 10 ** Math.floor(Math.log10(raw));
  const error = raw / power;
  return power * (error >= Math.sqrt(50) ? 10 : error >= Math.sqrt(10) ? 5 : error >= Math.sqrt(2) ? 2 : 1);
}

/**
 * A value axis's tick: "300조", "1.5조" / "300tn", "1.5tn" when `abbreviate`
 * and the axis reaches the scales amounts use, plain localized digits below
 * them ("38,200", "0.05"). `lo` and `hi` are the extent the axis covers and
 * `count` the ticks it asks for; the precision is that of a step for twice as
 * many ticks, trailing zeros dropped, so adjacent ticks always read apart.
 */
export function axisTick(value: number, lo: number, hi: number, count: number, abbreviate: boolean): string {
  const step = tickStep(lo, hi, count * 2);
  const top = Math.max(Math.abs(Math.floor(lo / step) * step), Math.abs(Math.ceil(hi / step) * step));
  const { size, suffix } = abbreviate ? amountScale(top) : { size: 1, suffix: "" };
  if (value === 0) return formatNumber(0);
  const decimals = Math.min(20, Math.max(0, Math.ceil(-Math.log10(step / size) - 1e-9)));
  return `${formatNumber(value / size, { maximumFractionDigits: decimals })}${suffix}`;
}

/**
 * A value printed on its mark: in the unit the answer uses ("302.2조", "302.2tn")
 * when large; otherwise its digits with up to two decimals ("71,000.5"), or three
 * significant ones under 1 ("0.123"), so a price or a ratio keeps its decimals.
 */
export function dataLabel(value: unknown, abbreviate: boolean): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return value == null ? "" : String(value);
  const { size, suffix } = abbreviate ? amountScale(value) : { size: 1, suffix: "" };
  if (size > 1) return `${formatNumber(value / size, { maximumFractionDigits: 1 })}${suffix}`;
  return Math.abs(value) >= 1 ? formatNumber(value, { maximumFractionDigits: 2 }) : formatNumber(value, { maximumSignificantDigits: 3 });
}

/** A value as stored, for the tooltip: every digit, in the currency when the dataset names one ("302조 1,234억 원"). */
export function exactValue(value: unknown, currency: unknown): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return value == null ? "" : String(value);
  return typeof currency === "string" && currency ? formatAmount(value, currency, { exact: true }) : formatNumber(value, { maximumFractionDigits: 12 });
}

export type DateGrain = "year" | "month" | "day";
const YEAR_RANGE = [1800, 2200] as const;
/** An integer that reads as a calendar year. */
export const isYear = (value: unknown) => typeof value === "number" && Number.isInteger(value) && value >= YEAR_RANGE[0] && value <= YEAR_RANGE[1];

/** A date in the app's format: "2023"; "Jan 2024" / "2024년 1월"; "Jan 31" / "1월 31일". A bare year is printed as given. */
export function dateLabel(value: unknown, options: unknown): string {
  if (isYear(value)) return String(value);
  const time = value instanceof Date ? value.getTime() : typeof value === "number" ? value : typeof value === "string" ? Date.parse(value) : Number.NaN;
  if (!Number.isFinite(time)) return value == null ? "" : String(value);
  const { grain = "day", utc = true } = (options ?? {}) as { grain?: DateGrain; utc?: boolean };
  const timeZone = utc ? "UTC" : undefined;
  if (grain === "year") return String(utc ? new Date(time).getUTCFullYear() : new Date(time).getFullYear());
  if (grain === "month") return formatDate(time, { timeZone, year: "numeric", month: "short", day: undefined });
  return formatDate(time, { timeZone, year: undefined, month: "short", day: "numeric" });
}

/** A folded series key as its legend reads it. */
export function seriesLabel(key: unknown, labels: unknown): string {
  const text = key == null ? "" : String(key);
  return labels && typeof labels === "object" && text in labels ? String((labels as Record<string, unknown>)[text]) : text;
}

const FUNCTIONS = { loupistTick: axisTick, loupistLabel: dataLabel, loupistExact: exactValue, loupistDate: dateLabel, loupistSeries: seriesLabel };
for (const [name, fn] of Object.entries(FUNCTIONS)) vega.expressionFunction(name, fn);

// A currency the column names: "revenue_krw", "Revenue (USD)", "매출(원)", "$".
const CURRENCY_CODE = /(?:^|[^A-Za-z])(KRW|USD|EUR|JPY|CNY|GBP)(?![A-Za-z])/iu;
const CURRENCY_SIGN: Array<[RegExp, string]> = [[/₩|￦|원/u, "KRW"], [/\$|달러/u, "USD"], [/€|유로/u, "EUR"], [/¥|엔/u, "JPY"], [/위안/u, "CNY"], [/£|파운드/u, "GBP"]];
// A scale written into a column id: "revenue_krw_trillion", "sales_usd_bn".
const SCALE_WORD = /_(trillions?|tn|billions?|bn|millions?|mn|thousands?)$/iu;
const SCALE_NAMES = { tn: ["tn", "조"], bn: ["bn", "십억"], m: ["m", "백만"], k: ["k", "천"] } as const;

const currencyIn = (text: string) => CURRENCY_CODE.exec(text)?.[1]?.toUpperCase() ?? CURRENCY_SIGN.find(([pattern]) => pattern.test(text))?.[1];

/** The currency (and a scale its id writes in) a value column holds, and whether its label already says so. */
export function columnUnit(id: string, label: string): { currency?: string; scale?: keyof typeof SCALE_NAMES; named: boolean } {
  const named = Boolean(currencyIn(label));
  const currency = currencyIn(label) ?? currencyIn(id.replaceAll("_", " "));
  const word = SCALE_WORD.exec(id)?.[1]?.toLowerCase();
  const scale = !word ? undefined : word.startsWith("t") && word !== "thousand" && word !== "thousands" ? "tn" : word.startsWith("b") ? "bn" : word.startsWith("m") ? "m" : "k";
  return { currency, scale, named };
}

/** The unit as a title names it once: "KRW", "KRW bn" / "원", "십억 원"; empty when the dataset names none. */
export function unitText(unit: ReturnType<typeof columnUnit>): string {
  if (!unit.currency && !unit.scale) return "";
  const korean = /^ko(?:-|$)/iu.test(i18n.language);
  const scale = unit.scale ? SCALE_NAMES[unit.scale][korean ? 1 : 0] : "";
  return t("chartAxis.unit", { currency: unit.currency ? currencyName(unit.currency) : "", scale }).replace(/\s+/gu, " ").trim();
}
