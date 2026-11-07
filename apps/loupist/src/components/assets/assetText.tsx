/**
 * Sentences and source chips for asset rows. Rust computes every row and its
 * facts; this only words them in the user's language. Shared by the asset
 * page, the morning message, briefings and the Map inspector.
 */
import i18n from "@/i18n";
import { formatAmount, formatCount, formatDate, formatNumber } from "@/i18n/format";
import type { ChatEvidenceRef } from "../../types";
import type { AssetCapitalFacts, AssetChangeRow, AssetDilution, AssetEvidence, AssetFlowRow, AssetGlanceRow, LineCrossing, Verdict } from "../../contracts/assets";
import { useCallback } from "react";
import { CitationChip } from "../chat/CitationChip";
import { OpenProvider, useOpenLayer } from "../navigation/Open";
import english from "@/i18n/locales/en/assets.json";

const t = i18n.getFixedT(null, "assets");
/** Keys chosen at runtime from Rust-provided kinds. */
const tDynamic = t as unknown as (key: string, options?: Record<string, unknown>) => string;

export const verdictLabel = (verdict: Verdict) => tDynamic(`verdict.${verdict}`);
export const verdictStep = (verdict: Verdict) => ({ holding: 1, weakening: 2, challenged: 3, broken: 4 })[verdict];

/** A calendar day (YYYY-MM-DD) as "Oct 30", without a time-zone shift. */
export function shortDay(day: string | undefined) {
  if (!day) return "";
  const [year, month, date] = day.slice(0, 10).split("-").map(Number);
  if (!year || !month || !date) return day;
  return formatDate(new Date(year, month - 1, date), { year: undefined, month: "short", day: "numeric" });
}

const num = (value: unknown) => (typeof value === "number" && Number.isFinite(value) ? value : undefined);
const text = (value: unknown) => (typeof value === "string" ? value : "");
const MINUS = "−";

/** A signed change with its unit ("−3.1%", "+25 bp"). */
export function signed(value: number, unit: string, digits = 1) {
  const magnitude = formatNumber(Math.abs(value), { maximumFractionDigits: digits });
  const sign = value > 0 ? "+" : value < 0 ? MINUS : "";
  return unit === "bp" ? `${sign}${magnitude} bp` : `${sign}${magnitude}${unit}`;
}

/** A value in its own currency, never converted. */
export function amount(value: number, currency?: unknown) {
  const code = typeof currency === "string" && /^[A-Z]{3}$/.test(currency) ? currency : undefined;
  return code ? formatNumber(value, { style: "currency", currency: code, maximumFractionDigits: value % 1 === 0 ? 0 : 2 }) : formatNumber(value, { maximumFractionDigits: 2 });
}

function metricValue(value: number, unit: string) {
  return unit === "%" ? `${formatNumber(value, { maximumFractionDigits: 1 })}%` : formatNumber(value, { notation: Math.abs(value) >= 1e6 ? "compact" : "standard", maximumFractionDigits: 1 }) + (unit && unit !== "%" ? ` ${unit}` : "");
}

/** An expense ratio reported as a fraction (0.0009) or a percent (0.09). */
function ratio(value: number) {
  return `${formatNumber(value < 0.02 ? value * 100 : value, { maximumFractionDigits: 2 })}%`;
}

/**
 * A reason that follows a dash or a separator mid-sentence starts in lower case
 * ("Weakening since Oct 26 — the quarterly report shows…"). Only words that are
 * never names are lowered; anything else keeps its capital.
 */
const SENTENCE_START = /^(?:The|A|An|This|That|These|Those|It|Its|Their|They|There|Our|Your|Both|Each|Every|No|Now|Margins?|Revenue|Sales|Operating|Prices?|Shares|Results|Crossed)\b/u;
export const midSentence = (reason: string) => reason.replace(SENTENCE_START, word => word.toLowerCase());

/** A parenthetical such as "(₩41,200 → ₩39,920)" never wraps inside. */
const together = (sentence: string) => sentence.replace(/\([^()]*\)/gu, group => group.replace(/ /gu, "\u00a0"));

type FilingFormKey = "annual" | "halfYear" | "quarterly" | "majorEvent" | "insiderOwnership" | "largeHolding" | "largestShareholder" | "audit" | "preliminaryResults";
/** OpenDART report names (`report_nm`), most specific first. */
const DART_FORMS: [RegExp, FilingFormKey][] = [
  [/사업보고서/u, "annual"], [/반기보고서/u, "halfYear"], [/분기보고서/u, "quarterly"], [/주요사항보고서/u, "majorEvent"],
  [/임원[ㆍ·]?주요주주특정증권등소유상황보고서/u, "insiderOwnership"], [/주식등의\s?대량보유상황보고서/u, "largeHolding"],
  [/최대주주등소유주식변동신고서/u, "largestShareholder"], [/(?:연결)?감사보고서/u, "audit"],
  [/(?:연결재무제표기준)?영업\(잠정\)실적(?:\(공정공시\))?/u, "preliminaryResults"],
];

/**
 * A DART report name in the reader's language: "분기보고서 (2026.09)" reads
 * "Quarterly report (2026.09)" in English and stays as filed in Korean. A
 * correction tag ("[기재정정]") marks it amended. Undefined for other names.
 */
export function dartReport(name: string): { form: string; name: string; key: FilingFormKey; amended: boolean; rest: string } | undefined {
  for (const [pattern, key] of DART_FORMS) {
    const match = pattern.exec(name);
    if (!match) continue;
    const before = name.slice(0, match.index);
    // Only correction tags may precede the report type; anything else stays as filed.
    if (!/^(?:\s*\[[^\]]*\])*\s*$/u.test(before)) return undefined;
    const base = t(`filingForm.${key}`);
    const form = before.includes("정정") ? t("filingForm.amended", { form: base }) : base;
    const rest = name.slice(match.index + match[0].length).trim();
    return { form, name: rest ? `${form} ${rest}` : form, key, amended: before.includes("정정"), rest };
  }
  return undefined;
}

/** SEC periodic forms and the report each one is. */
const SEC_FORMS: Record<string, "quarterly" | "annual"> = { "10-Q": "quarterly", "10-K": "annual", "20-F": "annual" };

/**
 * The report a periodic SEC filing is, when its title only names that kind of
 * report ("Quarterly report" for a 10-Q): the provider's English title then reads
 * in the user's language. Undefined for any other form or a specific title.
 */
export function secReport(form: string, title: string): "quarterly" | "annual" | undefined {
  const key = SEC_FORMS[form.trim().toUpperCase()];
  if (!key) return undefined;
  const generic = english.filingForm[key].toLowerCase();
  const plain = title.trim().toLowerCase();
  return plain === generic ? key : undefined;
}

/** "Quarterly report (2026.09) filed", "10-Q filed: Quarterly report": the form once, never twice. */
function filingSentence(facts: Record<string, unknown>) {
  const rawForm = text(facts.form);
  const rawTitle = text(facts.title);
  const titled = dartReport(rawTitle);
  const form = (rawForm && dartReport(rawForm)?.form) || rawForm || titled?.form || "";
  const sec = secReport(rawForm, rawTitle);
  const title = titled?.name ?? (sec ? t(`filingForm.${sec}`) : rawTitle);
  if (!form) return title;
  if (!title || title.includes(form)) return t("row.filedForm", { form: title || form });
  return t("row.filing", { form, title });
}

/** A crossed line's reason, in the user's language; it always follows a dash or a separator. */
export function crossedReason(line: LineCrossing, factDate: string) {
  return t("lines.crossedReason", { claim: line.claim, value: lineValue(line.observed, line.unit), date: shortDay(factDate) });
}

export function lineValue(observed: number, unit?: string) {
  if (!unit) return formatNumber(observed, { maximumFractionDigits: 2 });
  return unit === "%" ? `${formatNumber(observed, { maximumFractionDigits: 2 })}%` : `${formatNumber(observed, { maximumFractionDigits: 2 })} ${unit}`;
}

/** A capital decision's dilution as the row prints it: "8.7%". */
export const dilutionText = (dilution: AssetDilution) => `${formatNumber(dilution.value, { maximumFractionDigits: 1 })}%`;

/**
 * One Korean capital decision (주요사항보고) in plain words, money in the app's
 * one format ("₩30bn" / "300억 원"). OpenDART's rights offering decision has no
 * issue price or payment date, so that row points to the notice. Bonds that can
 * be reset down to a floor give the most shares at that floor, as Rust
 * calculated it; without a floor, the filed share at the starting price.
 */
function capitalSentence(facts: Partial<AssetCapitalFacts>, dilutionMark?: string) {
  const won = (value: number | null | undefined) => (typeof value === "number" ? formatAmount(value, "KRW") : "—");
  const dilution = dilutionMark ?? (facts.dilution ? dilutionText(facts.dilution) : "—");
  switch (facts.decision) {
    case "rights_offering": {
      const shares = formatCount(facts.shares ?? 0);
      return typeof facts.price === "number" && facts.paymentDate
        ? t("korea.rights", { shares, price: won(facts.price), dilution, date: shortDay(facts.paymentDate) })
        : t("korea.rightsNotice", { shares, dilution });
    }
    case "convertible_bonds":
    case "bonds_with_warrants": {
      const bonds = facts.decision === "convertible_bonds" ? "convertible" : "warrants";
      const bond = { amount: won(facts.amount), price: won(facts.conversionPrice), floor: won(facts.floor), dilution };
      const floored = typeof facts.floor === "number";
      const variant = !facts.dilution ? "NoRatio" : floored && facts.dilution.at === "floor" ? "" : floored ? "Filed" : "Fixed";
      return tDynamic(`korea.${bonds}${variant}`, bond);
    }
    case "buyback": return t("korea.buyback", { amount: won(facts.amount), date: shortDay(facts.by) });
    default: return "";
  }
}

/**
 * The sentence for one change row. `dilution` replaces a capital row's dilution
 * figure, so the page can put its calculation chip there.
 */
export function rowSentence(row: Pick<AssetChangeRow, "kind" | "facts" | "factDate">, options: { dilution?: string } = {}) {
  const facts = row.facts;
  switch (row.kind) {
    case "capital": return capitalSentence(facts as Partial<AssetCapitalFacts>, options.dilution);
    case "filing": return filingSentence(facts);
    case "results": {
      const metricKey = text(facts.metric);
      const metric = metricKey === "operating_margin" || metricKey === "revenue" ? t(`metric.${metricKey}`) : metricKey;
      const unit = text(facts.unit);
      const value = metricValue(num(facts.value) ?? 0, unit);
      const prior = num(facts.prior);
      return prior === undefined
        ? t("row.resultsFirst", { metric, value, period: text(facts.period) })
        : t("row.results", { metric, value, period: text(facts.period), prior: metricValue(prior, unit) });
    }
    case "earnings": return t("row.earnings", { period: text(facts.period), date: shortDay(text(facts.date)) });
    case "dividend": return recordDateText(facts) ?? t("row.dividend", { amount: amount(num(facts.amount) ?? 0, facts.currency), date: shortDay(text(facts.exDate)) });
    case "price": return together(t("row.price", { change: signed(num(facts.change) ?? 0, "%"), date: shortDay(text(facts.fromDate)), from: amount(num(facts.from) ?? 0, facts.currency), to: amount(num(facts.to) ?? 0, facts.currency) }));
    case "rates": return t("row.rates", { series: text(facts.series), change: signed(num(facts.change) ?? 0, "bp", 0), date: shortDay(text(facts.fromDate)) });
    case "holdings": {
      const names = (key: string) => (Array.isArray(facts[key]) ? (facts[key] as unknown[]).map(text).filter(Boolean) : []);
      return [names("entered").length ? t("row.holdingsIn", { names: names("entered").join(", ") }) : "", names("left").length ? t("row.holdingsOut", { names: names("left").join(", ") }) : ""].filter(Boolean).join(" · ");
    }
    case "fees": return t("row.fees", { from: ratio(num(facts.prior) ?? 0), to: ratio(num(facts.ratio) ?? 0) });
    case "positioning": return t("row.positioning", { change: signed(num(facts.change) ?? 0, "", 0) });
    case "map": return text(facts.summary);
    case "source": return text(facts.title);
    case "insiders": return t(facts.buyNotChecked ? "row.insidersReports" : "row.insiders", { count: num(facts.count) ?? 0, from: shortDay(text(facts.from)), to: shortDay(text(facts.to)) });
    case "holders": return text(facts.holder) && text(facts.ratio)
      ? t("row.holders", { holder: text(facts.holder), ratio: text(facts.ratio), before: text(facts.before) || "—" })
      : filingSentence(facts);
    case "flows": return t(facts.direction === "out" ? "row.flowsOut" : "row.flowsIn", { date: shortDay(text(facts.to) || row.factDate) });
    case "thesis": {
      const line = facts.line as LineCrossing | null | undefined;
      const reason = line?.crossed ? crossedReason(line, row.factDate) : midSentence(text(facts.reason));
      const from = text(facts.from) as Verdict;
      return t("row.move", { from: from ? verdictLabel(from) : "", to: verdictLabel(text(facts.to) as Verdict), reason });
    }
  }
}

/**
 * A Korean dividend (OpenDART) has no ex-date: its record date (배당기준일)
 * from the decision notice, where to find it, or the amount alone.
 * Undefined for a dividend that names its ex-date.
 */
function recordDateText(facts: Record<string, unknown>) {
  if (text(facts.exDate)) return undefined;
  const value = amount(num(facts.amount) ?? 0, facts.currency);
  if (text(facts.recordDate)) return t("korea.recordDate", { amount: value, date: shortDay(text(facts.recordDate)) });
  return facts.recordInNotice === true ? `${value} · ${t("korea.inNotice")}` : value;
}

/** A private-use marker for a flows row's value inside its translated sentence. */
export const FLOW_VALUE = "";
const signWord = (value: number, spoken: string) => (value > 0 ? t("korea.plus", { value: spoken }) : value < 0 ? t("korea.minus", { value: spoken }) : spoken);

/**
 * One investor-flows row: net buying in won with its sign ("+₩12.3bn" /
 * "+123억 원"; below zero it is net selling), the short-selling share of
 * volume, or the lending-balance change to its own date. Flows are not
 * prices, so they carry no direction color; the spoken form says what the
 * sign means ("net selling 4.1 billion won").
 */
export function flowText(row: AssetFlowRow, to: string): { label: string; sentence: string; value: string; spoken: string } {
  const label = tDynamic(`korea.${row.key}`);
  if (row.unit === "KRW") {
    const sign = row.value > 0 ? "+" : row.value < 0 ? MINUS : "";
    const words = t("korea.won", { value: formatNumber(Math.abs(row.value), { notation: "compact", compactDisplay: "long", maximumFractionDigits: 1 }) });
    const spoken = row.value < 0 ? t("korea.netSelling", { value: words }) : row.value > 0 ? t("korea.netBuyingValue", { value: words }) : words;
    return { label, sentence: FLOW_VALUE, value: `${sign}${formatAmount(Math.abs(row.value), "KRW")}`, spoken };
  }
  const percent = formatNumber(Math.abs(row.value), { style: "unit", unit: "percent", unitDisplay: "long", maximumFractionDigits: 1 });
  if (row.key === "shortSelling") return { label, sentence: t("korea.shortShare", { value: FLOW_VALUE }), value: `${formatNumber(row.value, { maximumFractionDigits: 1 })}%`, spoken: percent };
  return { label, sentence: t("korea.lendingTo", { value: FLOW_VALUE, date: shortDay(row.to ?? to) }), value: signed(row.value, "%"), spoken: signWord(row.value, percent) };
}

/** A change row outside the page, where no date column stands beside it: "Oct 26 · …". */
export const datedSentence = (row: Pick<AssetChangeRow, "kind" | "facts" | "factDate">) => `${shortDay(row.factDate)} · ${rowSentence(row)}`;

/** The check kind behind an "At a glance" row, so a failed check marks its value as stale. */
export const GLANCE_KIND: Record<AssetGlanceRow["key"], string> = {
  lastFiling: "filings", lastResults: "results", nextEarnings: "earnings", dividend: "dividends",
  price: "price", drawdown: "price", expenseRatio: "fees", level: "level",
};

/** The value column of an "At a glance" row. */
export function glanceValue(row: AssetGlanceRow) {
  const facts = row.facts;
  switch (row.key) {
    case "lastFiling": {
      const form = text(facts.form);
      const name = (form && (dartReport(form)?.form ?? form)) || dartReport(text(facts.title))?.form || text(facts.title);
      return [name, shortDay(row.date)].filter(Boolean).join(" · ");
    }
    case "lastResults": return [text(facts.period), shortDay(row.date)].filter(Boolean).join(" · ");
    // A Korean legal deadline is never an expected date; an IR notice's date is the company's own.
    case "nextEarnings": {
      const date = shortDay(text(facts.date));
      if (facts.dueBy || facts.source === "legal_deadline") return t("glance.dueBy", { date });
      return facts.source === "ir_notice" ? t("glance.nextConfirmed", { date }) : t("glance.nextExpected", { date });
    }
    case "dividend": return recordDateText(facts) ?? `${amount(num(facts.amount) ?? 0, facts.currency)} · ${t("glance.exDate", { date: shortDay(text(facts.exDate)) })}`;
    case "price": return together(t("row.price", { change: signed(num(facts.change) ?? 0, "%"), date: shortDay(text(facts.fromDate)), from: amount(num(facts.from) ?? 0, facts.currency), to: amount(num(facts.to) ?? 0, facts.currency) }));
    case "drawdown": return signed(num(facts.change) ?? 0, "%");
    case "expenseRatio": return ratio(num(facts.ratio) ?? 0);
    case "level": return `${formatNumber(num(facts.value) ?? 0, { maximumFractionDigits: 2 })}${text(facts.unit) === "%" ? "%" : ""}`;
  }
}

/** The kind label for a check or a not-yet kind from Rust. */
export const checkKind = (kind: string) => tDynamic(`checks.kind.${kind}`);

/** Evidence as the citation card expects it; the chip opens the original when there is one. */
/** An asset row's source as a chat citation, so it opens the way answer citations do. */
export function assetCitation(evidence: AssetEvidence, index: number): ChatEvidenceRef {
  return {
    id: `${evidence.citationUrl ?? evidence.url ?? evidence.recordId ?? evidence.worldId ?? evidence.title}:${index}`,
    title: evidence.title,
    kind: evidence.worldId || evidence.recordId ? "saved_context" : "original",
    url: evidence.url,
    provider: evidence.provider,
    tool_name: evidence.tool,
    retrieved_at: evidence.retrievedAt ?? "",
    published_at: evidence.date,
    excerpt: "",
    excerpt_truncated: false,
    content_sha256: "",
    warnings: [],
  };
}

const host = (url?: string) => {
  try { return url ? new URL(url).hostname.replace(/^www\./, "") : ""; } catch { return ""; }
};

/** Opens one source of an asset row; `citation` is the same source as a chat citation. */
export type EvidenceOpener = (evidence: AssetEvidence, citation: ChatEvidenceRef) => void;

/** What makes two pieces of evidence the same source: its link, record, Map claim or dataset row. */
export function evidenceKey(evidence: AssetEvidence) {
  const url = evidence.citationUrl ?? evidence.url;
  if (url) return `url:${url}`;
  if (evidence.recordId) return `record:${evidence.recordId}`;
  if (evidence.worldId) return `world:${evidence.worldId}`;
  if (evidence.dataset) return `dataset:${evidence.dataset.dataset_id}:${evidence.dataset.revision}:${evidence.pointer ?? ""}:${evidence.date ?? ""}`;
  return `source:${evidence.tool}:${evidence.title}:${evidence.date ?? ""}`;
}

/** The number a source carries everywhere on one page or in one message. */
export type CitationNumbers = (evidence: AssetEvidence) => number;

/**
 * Numbers sources in reading order: each source gets one number however many
 * rows cite it, and numbering never restarts within the page or message.
 * Pass the evidence in display order so numbers read 1, 2, 3 down the page.
 */
export function citationNumbers(inOrder: Iterable<readonly AssetEvidence[]> = []): CitationNumbers {
  const numbers = new Map<string, number>();
  const numberOf = (evidence: AssetEvidence) => {
    const key = evidenceKey(evidence);
    let number = numbers.get(key);
    if (number === undefined) { number = numbers.size + 1; numbers.set(key, number); }
    return number;
  };
  for (const group of inOrder) for (const evidence of group) numberOf(evidence);
  return numberOf;
}

/**
 * One numbered source. A plain click goes to the surface's own opener; Cmd-click
 * (Ctrl elsewhere) or a middle click opens the source page in the system browser.
 */
function SourceChip({ item, number, onOpen }: { item: AssetEvidence; number: number; onOpen: EvidenceOpener }) {
  const evidence = assetCitation(item, number);
  const open = useOpenLayer(useCallback(() => () => onOpen(item, evidence), [evidence, item, onOpen]));
  return <OpenProvider open={open}>
    <CitationChip evidence={evidence} label={i18n.t("chat:evidence.numberedSource", { number, title: item.title })} title={item.title} source={host(item.url) || item.provider || item.title}
      to={{ kind: "evidence", evidence }}>{number}</CitationChip>
  </OpenProvider>;
}

/** Numbered source chips for a row; a source cited twice in the row shows once. */
export function SourceChips({ evidence, numbers, onOpen }: { evidence: readonly AssetEvidence[]; numbers: CitationNumbers; onOpen: EvidenceOpener }) {
  const seen = new Set<number>();
  return <>{evidence.map((item) => {
    const number = numbers(item);
    if (seen.has(number)) return null;
    seen.add(number);
    return <SourceChip key={`${number}:${item.title}`} item={item} number={number} onOpen={onOpen} />;
  })}</>;
}
