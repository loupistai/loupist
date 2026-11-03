import i18n from "./index";
import { finalSound } from "./koreanParticles";

const numbers = new Map<string, Intl.NumberFormat>();
const dates = new Map<string, Intl.DateTimeFormat>();
const relatives = new Map<string, Intl.RelativeTimeFormat>();
const locale = () => i18n.language === "en-XA" ? "en" : i18n.language || "en";
const cacheKey = (language: string, options: object) => JSON.stringify([language, Object.entries(options).sort(([a], [b]) => a.localeCompare(b))]);
function numberFormatter(options: Intl.NumberFormatOptions) {
  const language = locale();
  const key = cacheKey(language, options);
  if (!numbers.has(key)) numbers.set(key, new Intl.NumberFormat(language, options));
  return numbers.get(key)!;
}
function dateFormatter(options: Intl.DateTimeFormatOptions) {
  const language = locale();
  const key = cacheKey(language, options);
  if (!dates.has(key)) dates.set(key, new Intl.DateTimeFormat(language, options));
  return dates.get(key)!;
}
export const formatNumber = (value: number, options: Intl.NumberFormatOptions = {}) => numberFormatter(options).format(value);
/** Percent input is a fraction: 0.25 renders as 25%. */
export const formatPercent = (value: number, options: Intl.NumberFormatOptions = {}) => formatNumber(value, { ...options, style: "percent" });
export const formatDate = (value: Date | number, options: Intl.DateTimeFormatOptions = {}) => dateFormatter(options.dateStyle || options.timeStyle ? options : { year: "numeric", month: "short", day: "numeric", ...options }).format(value);
export function formatTime(value: Date | number) {
  const formatter = dateFormatter({ hour: "numeric", minute: "2-digit" });
  if (!/^ko(?:-|$)/i.test(locale())) return formatter.format(value);
  // ICU data differences between Node and WebKit can leave Korean day periods in English.
  return formatter.formatToParts(value).map((part) => part.type === "dayPeriod" && (part.value === "AM" || part.value === "PM")
    ? i18n.t(part.value === "AM" ? "common:time.am" : "common:time.pm")
    : part.value).join("");
}
export const formatDateTime = (value: Date | number, options: Intl.DateTimeFormatOptions = {}) => dateFormatter(options.dateStyle || options.timeStyle ? options : { year: "numeric", month: "short", day: "numeric", hour: "numeric", minute: "2-digit", ...options }).format(value);
export function formatRelative(value: number, unit: Intl.RelativeTimeFormatUnit, options: Intl.RelativeTimeFormatOptions = {}) {
  const language = locale();
  const resolved = { numeric: "auto" as const, ...options };
  const key = cacheKey(language, resolved);
  if (!relatives.has(key)) relatives.set(key, new Intl.RelativeTimeFormat(language, resolved));
  return relatives.get(key)!.format(value, unit);
}

/** The Korean subject particle for a name: 이 after a final consonant, 가 after a vowel (`finalSound`). */
export function koreanSubjectParticle(name: string) {
  const sound = finalSound(name.trim());
  return !sound ? "이(가)" : sound === "vowel" ? "가" : "이";
}

/** A name as the subject of a sentence: with 이/가 in Korean, unchanged elsewhere. */
export const subjectName = (name: string, language: string) => (language.toLowerCase().startsWith("ko") ? `${name}${koreanSubjectParticle(name)}` : name);

const isKorean = () => /^ko(?:-|$)/i.test(locale());
/** Korean amount units, largest first. */
const KO_UNITS = [[1e12, "조"], [1e8, "억"], [1e4, "만"]] as const;
/** Korean currency words; other codes stay as their ISO code. */
const KO_CURRENCY: Record<string, string> = { KRW: "원", USD: "달러", JPY: "엔", EUR: "유로", CNY: "위안", GBP: "파운드" };
/** English scale words, the ones answers print ("₩452.3bn"). */
const EN_SCALES = [[1e12, "tn"], [1e9, "bn"], [1e6, "m"]] as const;

function currencySymbol(code: string) {
  try {
    const symbol = new Intl.NumberFormat("en", { style: "currency", currency: code, currencyDisplay: "narrowSymbol" }).formatToParts(0).find((part) => part.type === "currency")?.value ?? code;
    return /^\p{L}+$/u.test(symbol) ? `${symbol} ` : symbol;
  } catch {
    return `${code} `;
  }
}

/**
 * "4,523억 1,200만" (exact) or "4,523억" (rounded to its largest unit) for a
 * non-negative amount. Amounts under 1억 (under 1,000만 when rounded) keep
 * plain digits, as prices are written: "38,200".
 */
function koreanUnits(value: number, exact: boolean, digits = 1) {
  if (value < (exact ? 1e8 : 1e7)) return { text: formatNumber(value, { maximumFractionDigits: 2 }), endsInUnit: false };
  if (exact) {
    const parts: string[] = [];
    let rest = Math.round(value * 100) / 100;
    for (const [size, unit] of KO_UNITS) {
      const count = Math.floor(rest / size);
      if (count > 0) parts.push(`${formatNumber(count)}${unit}`);
      rest -= count * size;
    }
    rest = Math.round(rest * 100) / 100;
    if (rest > 0 || !parts.length) parts.push(formatNumber(rest, { maximumFractionDigits: 2 }));
    return { text: parts.join(" "), endsInUnit: rest <= 0 && parts.length > 0 && value >= 1e4 };
  }
  // Under 10억 one decimal of 억 would drop digits the English scale keeps
  // ("$402m"), so 억 and 만 together: "4억 200만".
  if (value >= 1e8 && value < 1e9) {
    const man = Math.round(value / 1e4);
    if (man < 100_000) {
      const eok = Math.floor(man / 1e4);
      const rest = man % 1e4;
      return { text: rest ? `${formatNumber(eok)}억 ${formatNumber(rest)}만` : `${formatNumber(eok)}억`, endsInUnit: true };
    }
  }
  if (value >= 1e12) {
    let jo = Math.floor(value / 1e12);
    let eok = Math.round((value - jo * 1e12) / 1e8);
    if (eok >= 10_000) { jo += 1; eok = 0; }
    return { text: eok ? `${formatNumber(jo)}조 ${formatNumber(eok)}억` : `${formatNumber(jo)}조`, endsInUnit: true };
  }
  for (const [size, unit] of KO_UNITS.slice(1)) {
    if (value < size) continue;
    const count = value / size;
    const rounded = Number(count.toFixed(count < 100 ? digits : 0));
    // 9,999.6억 rounds to 1조, not "10,000억".
    if (unit === "억" && rounded >= 10_000) return { text: "1조", endsInUnit: true };
    if (unit === "만" && rounded >= 10_000) return { text: "1억", endsInUnit: true };
    return { text: `${formatNumber(count, { maximumFractionDigits: count < 100 ? digits : 0 })}${unit}`, endsInUnit: true };
  }
  return { text: formatNumber(value, { maximumFractionDigits: 2 }), endsInUnit: false };
}

/**
 * An amount of money the way answers print it: "₩452.3bn" / "4,523억 원".
 * `exact` keeps every digit: "₩452,312,000,000" / "4,523억 1,200만 원". Without
 * a currency the scale stays and the symbol is left out. `digits` is how many
 * decimals the scaled number may keep (1 by default: "₩452.3bn"; 2 keeps a
 * guidance range's "$1.25bn").
 */
export function formatAmount(value: number, currency?: string, options: { exact?: boolean; digits?: number } = {}) {
  if (!Number.isFinite(value)) return String(value);
  const code = currency && /^[A-Z]{3}$/u.test(currency) ? currency : undefined;
  const sign = value < 0 ? "-" : "";
  const magnitude = Math.abs(value);
  if (isKorean()) {
    const { text, endsInUnit } = koreanUnits(magnitude, Boolean(options.exact), options.digits);
    if (!code) return `${sign}${text}`;
    const word = KO_CURRENCY[code];
    return word ? `${sign}${text}${endsInUnit ? " " : ""}${word}` : `${sign}${text} ${code}`;
  }
  const symbol = code ? currencySymbol(code) : "";
  if (!options.exact) {
    for (const [size, suffix] of EN_SCALES) {
      if (magnitude >= size) return `${sign}${symbol}${formatNumber(magnitude / size, { maximumFractionDigits: options.digits ?? 1 })}${suffix}`;
    }
  }
  return `${sign}${symbol}${formatNumber(magnitude, { maximumFractionDigits: 2 })}`;
}

/**
 * A count at the scale money uses, ready for a counter word: "34.6m", "9,800".
 * In Korean a counter follows a unit with a space and digits without one, so a
 * count that ends in 조, 억 or 만 keeps a trailing space ("3,456만 " + "주").
 */
export function formatCount(value: number) {
  const text = formatAmount(value);
  return isKorean() && /[조억만]$/u.test(text) ? `${text} ` : text;
}

/** A count of shares: "34.6m shares" / "3,456만 주", "9,800 shares" / "9,800주". */
export const formatShareCount = (value: number) => i18n.t("common:shareCount", { value: formatCount(value) });

/**
 * A range of money, the currency and scale written once where both ends share
 * them: "$1.2–1.25bn" / "12억–12.5억 달러" / "1,100억–1,200억 원".
 */
export function formatAmountRange(low: number, high: number, currency?: string, options: { digits?: number } = {}) {
  const from = formatAmount(low, currency, options);
  const to = formatAmount(high, currency, options);
  if (low < 0 || high < 0) return `${from}–${to}`;
  if (isKorean()) {
    // The currency word follows the higher end only: "12억–12.5억 달러".
    const word = /\s?(?:[^\d\s,.억만조]+)$/u.exec(to)?.[0];
    return word && from.endsWith(word) && from.length > word.length ? `${from.slice(0, -word.length)}–${to}` : `${from}–${to}`;
  }
  const parts = (text: string) => /^(\D*?)([\d.,]+)(tn|bn|m)?$/u.exec(text);
  const a = parts(from);
  const b = parts(to);
  if (a && b && a[1] === b[1] && (a[3] ?? "") === (b[3] ?? "")) return `${a[1]}${a[2]}–${b[2]}${b[3] ?? ""}`;
  return `${from}–${to}`;
}

/**
 * A measured value with its unit, as the app prints it elsewhere: a currency
 * code reads as money ("₩445bn" / "4,450억 원"), "%" as a percent, anything
 * else follows the number.
 */
export function formatMeasure(value: number, unit?: string | null) {
  const trimmed = unit?.trim() ?? "";
  if (/^[A-Z]{3}$/u.test(trimmed)) return formatAmount(value, trimmed);
  if (trimmed === "%") return `${formatNumber(value, { maximumFractionDigits: 2 })}%`;
  return trimmed ? `${formatNumber(value, { maximumFractionDigits: 2 })} ${trimmed}` : formatNumber(value, { maximumFractionDigits: 2 });
}
