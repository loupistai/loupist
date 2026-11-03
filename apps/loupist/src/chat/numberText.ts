/**
 * Words and values for number popovers (finance-show-the-math.md §2.2–2.3):
 * plain, localized names for a calculation's operation and inputs and for a
 * data row's fields, and amounts printed the way answers print them. Rust's
 * check decides every status; this only words what it stored.
 */
import i18n from "@/i18n";
import { formatAmount, formatNumber } from "@/i18n/format";
import type { CalcInputUnit, ChatEvidenceRef, NumberCheck } from "../types";

const t = i18n.getFixedT(null, "chat");
/** Keys chosen at runtime from host-provided names, so not typed literals. */
const tDynamic = t as unknown as (key: string, options?: Record<string, unknown>) => string;

type Operation = "percentChange" | "cagr" | "growthSeries" | "returnsSummary" | "movingAverage" | "ratio" | "margin" | "payoutRatio" | "netDebtToEbitda" | "freeCashFlow" | "dcfGrid" | "reverseDcf";

/** The host's operation titles (`calculations.rs`), a stable English contract. */
const OPERATIONS: Record<string, Operation> = {
  "Percent change": "percentChange", "Compound annual growth": "cagr", "Period growth": "growthSeries",
  "Return and risk summary": "returnsSummary", "Moving average": "movingAverage", Ratio: "ratio", Margin: "margin",
  "Payout ratio": "payoutRatio", "Net debt to EBITDA": "netDebtToEbitda", "Free cash flow": "freeCashFlow",
  "DCF sensitivity grid": "dcfGrid", "Reverse DCF grid": "reverseDcf",
};

/** A host operation title ("Reverse DCF grid") in plain, localized words; undefined for any other title. */
export function operationName(title: string) {
  const operation = OPERATIONS[title];
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

/** The currency a printed number names: ₩, 원, KRW… */
export function currencyOfText(text: string): string | undefined {
  if (/[₩￦]|원|\bKRW\b/iu.test(text)) return "KRW";
  if (/\$|\bUSD\b|달러/iu.test(text)) return "USD";
  if (/€|\bEUR\b|유로/iu.test(text)) return "EUR";
  if (/£|\bGBP\b/iu.test(text)) return "GBP";
  if (/¥|\bJPY\b|엔/iu.test(text)) return "JPY";
  return undefined;
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

export type CalcInputView = { name: string; label: string; value: string; evidenceId?: string };
export type CalcView = { title: string; formulas: string[]; inputs: CalcInputView[]; value?: string };

/**
 * A calculated number's popover in the reader's words: "Operating margin ·
 * FY2025", inputs named by the label their formula prints ("Operating income")
 * and printed in the unit Rust gave them, and formula labels translated where
 * the app knows the term.
 */
export function calcView(check: NumberCheck, checks: readonly NumberCheck[]): CalcView {
  const calc = check.calc!;
  const operation = OPERATIONS[calc.title];
  // Answers saved before inputs carried labels name them by their input.
  const labels = Object.fromEntries(calc.inputs.map((input) => [input.name, input.label || humanize(input.name)]));
  const periods: string[] = [];
  const addPeriod = (period: string | undefined) => { if (period && !periods.includes(period)) periods.push(period); };
  const sourcePeriods: string[] = [];
  const inputs = calc.inputs.map((input): CalcInputView => {
    const label = plainLabel(labels[input.name]!);
    addPeriod(label.period);
    return { name: input.name, label: label.text, value: inputValue(input.display, input.unit), evidenceId: input.evidence_id };
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
  const formulas = calc.formulas.map((formula) => {
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
  period: "period", fiscal_year: "period", bsns_year: "period",
  value: "amount", amount: "amount", thstrm_amount: "amount",
  unit: "unit", currency: "unit", fs_div: "basis", basis: "basis",
};
const ENTITY_FIELDS = ["corp_name", "entity", "company", "name"];
const BASIS: Record<string, string> = { CFS: "consolidated", OFS: "separate" };

export type DataRowView = { key: string; label: string; value: string; matched: boolean };

/**
 * A data number's row in plain words: "Item · Revenue", "Amount · ₩452,312,000,000".
 * The company row is left out when the title already names it.
 */
export function dataRows(check: NumberCheck, evidence: ChatEvidenceRef, checks: readonly NumberCheck[], title: string): DataRowView[] {
  const currency = evidenceCurrency(evidence, checks) ?? currencyOfText(check.fields?.find((field) => FIELD_LABELS[field.key] === "unit")?.value ?? "");
  return (check.fields ?? []).flatMap((field) => {
    if (ENTITY_FIELDS.includes(field.key) && title.includes(field.value)) return [];
    const kind = FIELD_LABELS[field.key];
    const label = kind ? tDynamic(`numberChip.field.${kind}`) : humanize(field.key);
    const number = parse(field.value);
    let value = field.value;
    if (kind === "item") value = termOf(field.value) ? term(termOf(field.value)!) : humanize(field.value);
    else if (kind === "basis" && BASIS[field.value]) value = tDynamic(`numberChip.basis.${BASIS[field.value]}`);
    else if (kind !== "period" && kind !== "unit" && number !== undefined) {
      value = (kind === "amount" || field.matched) && currency ? formatAmount(number, currency, { exact: true }) : formatNumber(number, { maximumFractionDigits: 6 });
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
