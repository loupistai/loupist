/**
 * Words and values for number popovers (finance-show-the-math.md §2.2–2.3):
 * plain, localized names for a calculation's operation and inputs and for a
 * data row's fields, and amounts printed the way answers print them. Rust's
 * check decides every status; this only words what it stored.
 */
import i18n from "@/i18n";
import { formatAmount, formatNumber } from "@/i18n/format";
import type { CalcInputUnit, ChatEvidenceRef, NumberCheck } from "../types";
import type { CalcOperation } from "@/contracts/generated";

const t = i18n.getFixedT(null, "chat");
/** Keys chosen at runtime from host-provided names, so not typed literals. */
const tDynamic = t as unknown as (key: string, options?: Record<string, unknown>) => string;

type Operation = "percentChange" | "cagr" | "growthSeries" | "returnsSummary" | "movingAverage" | "ratio" | "margin" | "payoutRatio" | "netDebtToEbitda" | "freeCashFlow" | "dcfGrid" | "reverseDcf" | "convert";

/**
 * Plain-words keys by the host's operation id (`calculations.rs` `Operation`). Answers
 * saved before the id was kept show the host's English title instead.
 */
const OPERATIONS: Partial<Record<CalcOperation, Operation>> = {
  percent_change: "percentChange", cagr: "cagr", growth_series: "growthSeries", returns_summary: "returnsSummary",
  moving_average: "movingAverage", ratio: "ratio", margin: "margin", payout_ratio: "payoutRatio",
  net_debt_to_ebitda: "netDebtToEbitda", free_cash_flow: "freeCashFlow", dcf_grid: "dcfGrid", reverse_dcf: "reverseDcf",
  convert: "convert",
};

/** A host operation ("reverse_dcf") in plain, localized words; undefined for any other operation. */
export function operationName(id: CalcOperation | undefined) {
  const operation = id && OPERATIONS[id];
  return operation ? tDynamic(`numberChip.operation.${operation}`) : undefined;
}

/** Plain financial terms, by the words a label or a data field may use (English or Korean). */
const TERM_WORDS: Record<string, string[]> = {
  revenue: ["revenue", "revenues", "sales", "net sales", "total revenue", "매출", "매출액"],
  operatingIncome: ["operating income", "operating profit", "영업이익"],
  netIncome: ["net income", "net profit", "net earnings", "당기순이익", "순이익"],
  grossProfit: ["gross profit", "매출총이익"],
  profit: ["profit"],
  ebitda: ["ebitda"],
  freeCashFlow: ["free cash flow", "fcf", "잉여현금흐름"],
  operatingCashFlow: ["operating cash flow", "ocf", "영업현금흐름"],
  capex: ["capex", "capital expenditure", "capital expenditures", "설비투자"],
  dividends: ["dividends", "dividends paid", "배당금"],
  earnings: ["earnings"],
  totalDebt: ["total debt", "debt", "총차입금"],
  cash: ["cash", "cash and equivalents", "현금"],
  netDebt: ["net debt", "순차입금"],
  cashFlow: ["cash flow", "현금흐름"],
  shares: ["shares", "shares outstanding", "발행주식수"],
  price: ["price", "share price", "주가"],
  from: ["from"], to: ["to"], start: ["start"], end: ["end"],
  numerator: ["numerator"], denominator: ["denominator"], years: ["years"], growth: ["growth", "growth pct"],
  referenceGrowth: ["reference growth"], fee: ["fee"], contribution: ["contribution"], target: ["target"],
};
const TERMS = new Map(Object.entries(TERM_WORDS).flatMap(([term, words]) => words.map((word) => [word, term] as const)));

/** "FY2025", "2025", "Q3 2025", "2025Q3" at the end of a label. */
const PERIOD = /\s((?:FY|CY)\s?\d{2,4}|\d{4}(?:\s?(?:Q[1-4]|H[12]))?|(?:Q[1-4]|H[12]|[12]H)\s?(?:FY)?\s?\d{2,4})$/iu;

const normalizedWords = (text: string) => text.trim().toLowerCase().replace(/[_\s]+/gu, " ");
const termOf = (text: string) => TERMS.get(normalizedWords(text));
const term = (key: string) => tDynamic(`numberChip.term.${key}`);

/** "snake_case" → "Snake case", for a name the app has no word for. */
export function humanize(key: string) {
  const words = key.replace(/[_-]+/gu, " ").trim();
  return words ? `${words[0]!.toUpperCase()}${words.slice(1)}` : key;
}

/** A label in the reader's words when it names a known term, with any period kept: "revenue FY2025" → "매출 FY2025". */
export function plainLabel(label: string): { text: string; period?: string } {
  const whole = termOf(label);
  if (whole) return { text: term(whole) };
  const period = label.match(PERIOD);
  const base = period ? label.slice(0, period.index) : "";
  const known = base ? termOf(base) : undefined;
  if (known && period) return { text: `${term(known)} ${period[1]}`, period: period[1] };
  return { text: label, ...(period ? { period: period[1] } : {}) };
}

const parse = (text: string) => {
  const value = Number(text.replace(/[,\s]/gu, "").replace("−", "-"));
  return Number.isFinite(value) ? value : undefined;
};
const decimals = (text: string) => text.match(/\.(\d+)/u)?.[1]?.length ?? 0;

const currencyAliases: Record<string, string> = {
  "₩": "KRW", "￦": "KRW", 원: "KRW", WON: "KRW",
  "$": "USD", "US$": "USD", "미국 달러": "USD", 달러: "USD", DOLLAR: "USD", DOLLARS: "USD",
  "¥": "JPY", "￥": "JPY", 円: "JPY", 엔: "JPY", YEN: "JPY",
  "€": "EUR", 유로: "EUR", EURO: "EUR", EUROS: "EUR",
  "£": "GBP", 파운드: "GBP", POUND: "GBP", POUNDS: "GBP",
  RMB: "CNY", 人民币: "CNY", 人民幣: "CNY", 元: "CNY", 위안: "CNY", YUAN: "CNY",
  "NT$": "TWD", "대만 달러": "TWD", 新台幣: "TWD", 新台币: "TWD",
  "HK$": "HKD", "홍콩 달러": "HKD", "A$": "AUD", "C$": "CAD", "NZ$": "NZD", "S$": "SGD",
};
const symbolAliases = Object.keys(currencyAliases)
  .filter(token => /[^A-Z]/u.test(token))
  .sort((a, b) => b.length - a.length)
  .map(token => token.replace(/[.*+?^$\{\}()|[\]\\]/gu, "\\$&").replace(" ", "\\s+"));
const currencyTokens = new RegExp([...symbolAliases, "[a-z]+"].join("|"), "giu");
const isoCurrencyNames = new Intl.DisplayNames(["en"], { type: "currency", fallback: "none" });

function currencyToken(text: string) {
  const token = text.toUpperCase().replace(/\s+/gu, " ");
  if (Object.hasOwn(currencyAliases, token)) return currencyAliases[token];
  return /^[A-Z]{3}$/u.test(token) && isoCurrencyNames.of(token) ? token : undefined;
}

/** The currency a printed amount names, provided it names only one. */
export function currencyOfText(text: string, explicitOnly = false): string | undefined {
  const currencies = new Set([...text.matchAll(currencyTokens)].flatMap(([token]) => {
    if (explicitOnly && /^(?:[$¥￥元])$/iu.test(token)) return [];
    return currencyToken(token) ?? [];
  }));
  return currencies.size === 1 ? [...currencies][0] : undefined;
}

/** Use a single currency marker for recognizing the shape of a streaming number link. */
export function normalizeCurrencyMarkers(text: string): string {
  return text.replace(currencyTokens, token => currencyToken(token) ? "$" : token);
}

/** The currency of an evidence's amounts: from the answer's own number links, else one the retained data names. */
export function evidenceCurrency(evidence: ChatEvidenceRef, checks: readonly NumberCheck[]) {
  for (const check of checks) {
    if (check.evidence_id !== evidence.id || check.kind !== "data") continue;
    const currency = currencyOfText(check.text);
    if (currency) return currency;
  }
  const codes = new Set([...evidence.excerpt.matchAll(/"(?:unit|currency|currency_code|currency_unit)"\s*:\s*"([A-Z]{3})"/gu)].map((match) => match[1]!));
  return codes.size === 1 ? [...codes][0] : undefined;
}

const PERIOD_FIELDS = ["period", "fiscal_year", "bsns_year"];
const periodOf = (check: NumberCheck) => check.fields?.find((field) => PERIOD_FIELDS.includes(field.key) && !field.matched)?.value;

/**
 * An input printed in the unit Rust gave it (`CalcInputUnit`): 38.1 in KRW
 * billions reads "₩38.1bn" / "381억 원", a percent "8.4%", a count "34,560,000".
 */
export function inputValue(display: string, unit: CalcInputUnit | undefined) {
  const value = parse(display);
  if (value === undefined) return display;
  const plain = formatNumber(value, { maximumFractionDigits: Math.max(2, decimals(display)) });
  switch (unit?.kind) {
    case "percent": return `${formatNumber(value, { maximumFractionDigits: 2 })}%`;
    case "points": return t("numberChip.unit.points", { value: plain });
    case "basis_points": return t("numberChip.unit.basisPoints", { value: plain });
    case "multiple": return t("numberChip.unit.multiple", { value: plain });
    case "price": return unit.currency ? formatAmount(value, unit.currency, { exact: true }) : plain;
    case "amount": return unit.currency || unit.scale ? formatAmount(value * 10 ** (unit.scale ?? 0), unit.currency) : plain;
    default: return unit?.text ? `${plain} ${unit.text}` : plain;
  }
}

export type CalcInputView = {
  name: string; label: string; value: string; evidenceId?: string;
  /** A conversion's amount whose currency Rust took from the request because its cited source states none. */
  declaredCurrency?: true;
  /** A conversion's rate: its source is dated by publication only, never by the day it was read. */
  publishedDateOnly?: true;
};
export type CalcView = { title: string; formulas: string[]; inputs: CalcInputView[]; value?: string };

/**
 * A calculated number's popover in the reader's words: "Operating margin ·
 * FY2025", inputs named by the label their formula prints ("Operating income")
 * and printed in the unit Rust gave them, and formula labels translated where
 * the app knows the term.
 */
export function calcView(check: NumberCheck, checks: readonly NumberCheck[]): CalcView {
  const calc = check.calc!;
  const operation = calc.operation && OPERATIONS[calc.operation];
  // Answers saved before inputs carried labels name them by their input.
  const labels = Object.fromEntries(calc.inputs.map((input) => [input.name, input.label || humanize(input.name)]));
  const periods: string[] = [];
  const addPeriod = (period: string | undefined) => { if (period && !periods.includes(period)) periods.push(period); };
  const sourcePeriods: string[] = [];
  const conversion = calc.operation === "convert";
  // A conversion input the record holds no value for has no row; a stored "0" is a value.
  const shown = conversion ? calc.inputs.filter((input) => typeof input.display === "string" && input.display.trim() !== "") : calc.inputs;
  const inputs = shown.map((input): CalcInputView => {
    const label = plainLabel(labels[input.name]!);
    addPeriod(label.period);
    // Only the host's explicit marker on a conversion's amount; never inferred from a label, source or rate.
    const declared = conversion && input.name === "amount" && input.currency_basis === "declared";
    return { name: input.name, label: label.text, value: inputValue(input.display, input.unit), evidenceId: input.evidence_id,
      ...(declared ? { declaredCurrency: true } : {}), ...(conversion && input.name === "rate" ? { publishedDateOnly: true } : {}) };
  });
  if (!periods.length) {
    // Periods of the answer's own data numbers from the same sources.
    for (const input of calc.inputs) {
      for (const other of checks) if (input.evidence_id && other.evidence_id === input.evidence_id && other.kind === "data") {
        const period = periodOf(other);
        if (period && !sourcePeriods.includes(period)) sourcePeriods.push(period);
      }
    }
    if (sourcePeriods.length === 1) addPeriod(sourcePeriods[0]);
  }
  const marginTerm = operation === "margin" ? (labels[calc.inputs[0]?.name ?? ""] ? termOf(labels[calc.inputs[0]!.name]!) : undefined) : undefined;
  const operationName = !operation ? calc.title
    : marginTerm === "operatingIncome" ? tDynamic("numberChip.operation.operatingMargin")
    : marginTerm === "netIncome" ? tDynamic("numberChip.operation.netMargin")
    : marginTerm === "grossProfit" ? tDynamic("numberChip.operation.grossMargin")
    : tDynamic(`numberChip.operation.${operation}`);
  const period = periods.length === 2 ? `${periods[0]} → ${periods[1]}` : periods.length === 1 ? periods[0] : undefined;
  // Formula labels in the reader's words; numbers and symbols stay as the host wrote them.
  const replacements = Object.values(labels).map((label) => [label, plainLabel(label).text] as const)
    .filter(([label, plain]) => plain.toLowerCase() !== label.toLowerCase())
    .sort((a, b) => b[0].length - a[0].length);
  // A conversion's formula is its retained receipt and is printed as the host wrote it.
  const formulas = conversion ? calc.formulas : calc.formulas.map((formula) => {
    if (!replacements.length) return formula;
    const at = formula.indexOf(" = ");
    let left = at < 0 ? formula : formula.slice(0, at);
    for (const [label, plain] of replacements) left = left.split(label).join(plain);
    return at < 0 ? left : `${left}${formula.slice(at)}`;
  });
  return {
    title: period ? `${operationName} · ${period}` : operationName,
    formulas,
    inputs,
    value: calc.grid ? gridRange(check, checks) : check.text,
  };
}

/** A grid has no single value: the range of the answer's numbers from it ("4.1%–7.8%"), or nothing. */
function gridRange(check: NumberCheck, checks: readonly NumberCheck[]) {
  const values = checks.filter((other) => other.evidence_id === check.evidence_id && other.calc?.grid)
    .map((other) => ({ text: other.text, value: parse(other.text.replace(/[^\d.,−-]/gu, "")) }))
    .filter((item): item is { text: string; value: number } => item.value !== undefined)
    .sort((a, b) => a.value - b.value);
  if (values.length < 2) return undefined;
  const low = values[0]!.text;
  const high = values.at(-1)!.text;
  return low === high ? undefined : `${low}–${high}`;
}

const FIELD_LABELS: Record<string, string> = {
  canonical_concept: "item", concept: "item", account_nm: "item", account: "item", item: "item", metric: "item", line_item: "item",
  period: "period", fiscal_year: "period", bsns_year: "period", date: "date",
  value: "value", amount: "value", thstrm_amount: "value",
  unit: "unit", currency: "unit", fs_div: "basis", basis: "basis",
};
/**
 * Market-data price, valuation and results fields (a bundled quote tool, `asset_page_read`):
 * the label key under `numberChip.quoteField`, and whether the value is a percent.
 */
const QUOTE_FIELDS: Record<string, { label: string; percent?: true; price?: true }> = {
  last: { label: "last", price: true }, last_price: { label: "last", price: true }, close: { label: "close", price: true },
  previous_close: { label: "previousClose", price: true }, change: { label: "change", price: true }, change_pct: { label: "changePct", percent: true },
  // Older reads stored a signed distance under this key, so the bare key keeps its older label.
  high_52w: { label: "high52", price: true }, low_52w: { label: "low52", price: true }, below_52w_high_pct: { label: "below52High", percent: true },
  // Keys only the host's by-direction magnitudes use.
  above_52w_high_pct: { label: "pctAbove52High", percent: true },
  below_ma50_pct: { label: "pctBelowMa50", percent: true }, above_ma50_pct: { label: "pctAboveMa50", percent: true },
  below_ma200_pct: { label: "pctBelowMa200", percent: true }, above_ma200_pct: { label: "pctAboveMa200", percent: true },
  distance_from_52w_high_pct: { label: "distanceFrom52High", percent: true }, price_to_book_multiple: { label: "priceToBook" }, ev_ebitda_multiple: { label: "evEbitda" },
  ma50: { label: "ma50", price: true }, ma200: { label: "ma200", price: true },
  from_ma50_pct: { label: "fromMa50", percent: true }, distance_from_ma50_pct: { label: "fromMa50", percent: true },
  from_ma200_pct: { label: "fromMa200", percent: true }, distance_from_ma200_pct: { label: "fromMa200", percent: true },
  market_cap: { label: "marketCap", price: true }, pe: { label: "pe" }, pe_multiple: { label: "pe" }, forward_pe: { label: "forwardPe" }, forward_pe_multiple: { label: "forwardPe" },
  operating_margin_pct: { label: "operatingMargin", percent: true }, gross_margin_pct: { label: "grossMargin", percent: true },
  revenue_yoy_pct: { label: "revenueYoy", percent: true }, revenue_growth_yoy_pct: { label: "revenueYoy", percent: true },
  revenue_qoq_pct: { label: "revenueQoq", percent: true }, revenue_growth_qoq_pct: { label: "revenueQoq", percent: true },
};
const ENTITY_FIELDS = ["corp_name", "entity", "company", "name"];
const BASIS: Record<string, string> = { CFS: "consolidated", OFS: "separate" };

export type DataRowView = { key: string; label: string; value: string; matched: boolean };

/** "10.27%", the sign as stored; a zero is "0%", never "-0%". */
const percentText = (value: number) => `${formatNumber(value === 0 ? 0 : value, { maximumFractionDigits: 2 })}%`;

/**
 * A data number's row in plain words: "Item · Revenue", "Amount · ₩452,312,000,000".
 * The company row is left out when the title already names it.
 */
export function dataRows(check: NumberCheck, evidence: ChatEvidenceRef, checks: readonly NumberCheck[], title: string): DataRowView[] {
  const fields = check.fields ?? [];
  const unit = fields.find(field => field.key === "unit")?.value.trim() || undefined;
  const statedCurrency = fields.find(field => field.key === "currency")?.value.trim() || undefined;
  const currency = currencyOfText(statedCurrency ?? unit ?? evidenceCurrency(evidence, checks) ?? "");
  const valueUnit = unit && !currency && fields.some(field => FIELD_LABELS[field.key] === "value" && parse(field.value) !== undefined) ? unit : undefined;
  return fields.flatMap((field) => {
    if (valueUnit && field.key === "unit") return [];
    if (ENTITY_FIELDS.includes(field.key) && title.includes(field.value)) return [];
    const kind = FIELD_LABELS[field.key];
    const quote = kind ? undefined : QUOTE_FIELDS[field.key];
    const label = kind ? tDynamic(`numberChip.field.${kind === "value" && currency ? "amount" : kind}`) : quote ? tDynamic(`numberChip.quoteField.${quote.label}`) : humanize(field.key);
    const number = parse(field.value);
    let value = field.value;
    if (quote && number !== undefined) {
      value = quote.percent ? percentText(number)
        : quote.price && currency ? formatAmount(number, currency, { exact: true }) : formatNumber(number, { maximumFractionDigits: 6 });
    } else if (kind === "item") value = termOf(field.value) ? term(termOf(field.value)!) : humanize(field.value);
    else if (kind === "basis" && BASIS[field.value]) value = tDynamic(`numberChip.basis.${BASIS[field.value]}`);
    else if ((kind === "value" || field.matched) && kind !== "period" && kind !== "unit" &&
      !/(?:^|_)(?:date|day|year|period|code|id)(?:_|$)/i.test(field.key) && number !== undefined) {
      value = currency ? formatAmount(number, currency, { exact: true }) : formatNumber(number, { maximumFractionDigits: 6 });
      if (valueUnit && kind === "value") value = `${value} ${valueUnit}`;
    }
    return [{ key: field.key, label, value, matched: Boolean(field.matched) }];
  });
}

const KO_UNITS: Record<string, number> = { 조: 1e12, 억: 1e8, 만: 1e4 };
const EN_SCALES: Record<string, number> = { tn: 1e12, trillion: 1e12, t: 1e12, bn: 1e9, billion: 1e9, b: 1e9, m: 1e6, million: 1e6, k: 1e3 };

/**
 * The value a printed number stands for, scale words included: "₩452.3bn" →
 * 452,300,000,000, "4,523억 원" → 452,300,000,000, "8.42%" → 8.42. Undefined when
 * the text is not one number.
 */
export function printedValue(text: string): number | undefined {
  const plain = text.replace(/[−–]/gu, "-").replace(/\s+/gu, " ").trim();
  if (/[조억만]/u.test(plain)) {
    let total = 0;
    let seen = false;
    for (const match of plain.matchAll(/(-?[\d,]+(?:\.\d+)?)\s*(조|억|만)?/gu)) {
      const value = parse(match[1]!);
      if (value === undefined) return undefined;
      total += value * (match[2] ? KO_UNITS[match[2]]! : 1);
      seen = true;
    }
    return seen ? total : undefined;
  }
  const match = /^[^\d-]*(-?[\d,]+(?:\.\d+)?)\s*([A-Za-z]+)?[^\dA-Za-z]*$/u.exec(plain);
  if (!match) return undefined;
  const value = parse(match[1]!);
  if (value === undefined) return undefined;
  const word = match[2]?.toLowerCase();
  if (!word || /^[A-Z]{3}$/u.test(match[2]!)) return value;
  const scale = EN_SCALES[word];
  return scale === undefined ? undefined : value * scale;
}

/** A "rounded" number whose printed value is the source value exactly ("₩452.3bn" for 452,300,000,000) is not called rounded. */
export function shownExactly(check: Pick<NumberCheck, "text" | "exact">) {
  const exact = check.exact === undefined ? undefined : parse(check.exact);
  const shown = printedValue(check.text);
  if (exact === undefined || shown === undefined) return false;
  return Math.abs(shown - exact) <= Math.max(1, Math.abs(exact)) * 1e-9;
}

/** "Rounded from ₩452,312,000,000": the exact value in the same words as the row. */
export function exactAmount(check: NumberCheck, evidence: ChatEvidenceRef, checks: readonly NumberCheck[]) {
  const exact = check.exact ?? "";
  const number = parse(exact);
  if (number === undefined) return exact;
  if (/%/u.test(check.text)) return exact;
  const currency = evidenceCurrency(evidence, checks);
  return currency ? formatAmount(number, currency, { exact: true }) : formatNumber(number, { maximumFractionDigits: 6 });
}

export function scaleLabel(scale: number) {
  const known = ([0, 3, 4, 6, 8, 9, 12] as const).find(value => value === scale);
  return known === undefined ? t("figureWarning.scales.other", { factor: formatNumber(10 ** scale) })
    : t(`figureWarning.scales.${known}`);
}

/** Explain an exact rescaling with the original amount in its currency's base unit. */
export function matchedConversion(check: NumberCheck) {
  const conversion = check.scale_conversion;
  if (check.status !== "matched" || !conversion || !check.exact) return undefined;
  const amount = parse(check.exact);
  const sourceValue = amount === undefined ? check.exact
    : formatNumber(amount * 10 ** conversion.source_scale, { maximumFractionDigits: 12 });
  const currency = conversion.currency && amount !== undefined ? ` ${conversion.currency}` : "";
  return t("numberChip.matchedScaleConversion", { sourceValue, currency, scale: scaleLabel(conversion.answer_scale) });
}
