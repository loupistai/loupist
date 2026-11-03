/**
 * Sentences for watcher alerts (finance-watchers.md §4). Rust detects each
 * event from structured rows and dates it; this only words it in the user's
 * language. Shared by the Thread, the morning message, briefings and Schedule.
 */
import i18n from "@/i18n";
import { formatNumber, formatPercent } from "@/i18n/format";
import type { WatchEvent, WatchKind } from "../../contracts/watch";
import { amount, dartReport, secReport, shortDay } from "./assetText";

const t = i18n.getFixedT(null, "chat");
const tAssets = i18n.getFixedT(null, "assets");
const tBriefings = i18n.getFixedT(null, "briefings");
const tDynamic = tAssets as unknown as (key: string, options?: Record<string, unknown>) => string;

const text = (value: unknown) => (typeof value === "string" ? value : "");
const num = (value: unknown) => (typeof value === "number" && Number.isFinite(value) ? value : undefined);

/** The kind's switch label ("New filings"). */
export const watchKindLabel = (kind: WatchKind) => tDynamic(`watch.kind.${kind}`);

/** The kind's column label in the Watching matrix and its last-alert cell ("5% holders"). */
export const watchKindColumn = (kind: WatchKind) => tBriefings(`watch.col.${kind}`);

/**
 * The title and form of a filing in the reader's language ("Quarterly report"
 * for a DART name). A form the title already names is not repeated: the
 * disclosure system stands in its place.
 */
function filingParts(facts: Record<string, unknown>, evidence: WatchEvent["evidence"]) {
  const rawTitle = text(facts.title);
  const titled = dartReport(rawTitle);
  const rawForm = text(facts.form);
  const form = (rawForm && (dartReport(rawForm)?.form ?? rawForm)) || titled?.form || "";
  // A periodic report reads as one in a sentence: "filed a quarterly report".
  const sec = secReport(rawForm, rawTitle);
  const periodic = titled && !titled.amended && PERIODIC.has(titled.key) ? titled : undefined;
  const title = periodic ? [tDynamic(`filingArticle.${periodic.key}`), periodic.rest].filter(Boolean).join(" ")
    : sec ? tDynamic(`filingArticle.${sec}`)
      : titled?.name ?? rawTitle;
  const dart = Boolean(titled) || evidence.some(item => item.tool.startsWith("dart_"));
  if (!title) return { title: form || tAssets("kind.filing"), form: dart ? "DART" : tAssets("kind.filing") };
  return { title, form: !form || title.includes(form) || (periodic && form === periodic.form) ? (dart ? "DART" : tAssets("kind.filing")) : form };
}

const PERIODIC = new Set(["quarterly", "halfYear", "annual"]);

/**
 * One sentence for one alert, at most two with a number (concept §3).
 * `dated`: the row already shows the alert's date beside it, so a filing does not repeat it.
 */
export function watchSentence(event: WatchEvent, name: string, options: { dated?: boolean } = {}) {
  const facts = event.facts;
  switch (event.kind) {
    case "filings": {
      const { title, form } = filingParts(facts, event.evidence);
      return options.dated ? t("watchMessage.filingShort", { name, title, form }) : t("watchMessage.filing", { name, title, form, date: shortDay(event.factDate) });
    }
    case "results": {
      const date = shortDay(text(facts.date) || event.factDate);
      const period = text(facts.period);
      return period ? t("watchMessage.results", { name, period, date }) : t("watchMessage.resultsNoPeriod", { name, date });
    }
    case "insiders": return t(facts.buyNotChecked ? "watchMessage.insidersReports" : "watchMessage.insiders", { name, count: num(facts.count) ?? 0, from: shortDay(text(facts.from)), to: shortDay(text(facts.to) || event.factDate) });
    case "holders": {
      if (text(facts.holder) && text(facts.ratio)) return t("watchMessage.holders", { name, holder: text(facts.holder), ratio: text(facts.ratio), before: text(facts.before) || "—" });
      const { title, form } = filingParts(facts, event.evidence);
      return options.dated ? t("watchMessage.filingShort", { name, title, form }) : t("watchMessage.filing", { name, title, form, date: shortDay(event.factDate) });
    }
    case "flows": return t(facts.direction === "out" ? "watchMessage.flowsOut" : "watchMessage.flowsIn", { name, date: shortDay(text(facts.to) || event.factDate) });
    case "dividends": return t("watchMessage.dividend", { name, amount: num(facts.amount) === undefined ? "—" : amount(num(facts.amount)!, facts.currency), date: shortDay(text(facts.recordDate) || text(facts.exDate) || event.factDate) });
    case "cryptoMove":
    case "futuresMove": {
      const change = num(facts.change) ?? 0;
      const key = `${facts.window === "day" ? "moveDay" : "moveWeek"}${change < 0 ? "Down" : "Up"}` as const;
      return t(`watchMessage.${key}`, { name, change: share(change), date: shortDay(text(facts.to) || event.factDate) });
    }
    case "drawdown": return t("watchMessage.drawdown", { name, drawdown: share(num(facts.drawdown) ?? 0), date: shortDay(text(facts.to) || event.factDate) });
    // A peg is read to four decimals ($0.9731).
    case "depeg": return t("watchMessage.depeg", { name, price: num(facts.close) === undefined ? "—" : formatNumber(num(facts.close)!, { style: "currency", currency: "USD", minimumFractionDigits: 2, maximumFractionDigits: 4 }), date: shortDay(text(facts.to) || event.factDate) });
    case "tvl": return t("watchMessage.tvl", { name, target: text(facts.target) || name, change: share(num(facts.change) ?? 0), date: shortDay(text(facts.to) || event.factDate) });
    case "positioning": return t(facts.direction === "short" ? "watchMessage.positioningShort" : "watchMessage.positioningLong", { name, date: shortDay(text(facts.to) || event.factDate) });
    case "oilInventory": {
      const change = num(facts.change);
      const date = shortDay(text(facts.to) || event.factDate);
      if (change === undefined) return t("watchMessage.oilInventory", { name, date });
      // EIA states stocks in thousand barrels.
      const barrels = formatNumber(Math.abs(change) * 1000, { notation: "compact", maximumFractionDigits: 1 });
      return t(change < 0 ? "watchMessage.oilInventoryDown" : "watchMessage.oilInventoryUp", { name, amount: barrels, date });
    }
  }
}

/** A change as an unsigned percentage ("12.4%"); the sentence says which way. */
const share = (fraction: number) => formatPercent(Math.abs(fraction), { maximumFractionDigits: 1 });

/** "Oct 27 · Morrow Components filed a quarterly report (10-Q).": an alert with its fact date in front. */
export const datedWatchSentence = (event: WatchEvent, name: string) => `${shortDay(event.factDate)} · ${watchSentence(event, name, { dated: true })}`;

/** Newest fact first; alerts of one day keep the order Rust detected them in. */
export const newestFirst = <T extends Pick<WatchEvent, "factDate">>(events: readonly T[]) => [...events].sort((a, b) => b.factDate.localeCompare(a.factDate));

/** A dividend still ahead, in the future tense: "holders on record Nov 2 will get ₩300 per share". */
export function aheadDividendSentence(event: WatchEvent, name: string) {
  const value = num(event.facts.amount);
  return tBriefings("ahead.recordDate", { name, amount: value === undefined ? "—" : amount(value, event.facts.currency), date: shortDay(text(event.facts.recordDate) || text(event.facts.exDate) || event.factDate) });
}

/** The calendar day an alert looks ahead to: a results date or a dividend record date. */
export function aheadDay(event: WatchEvent) {
  if (event.kind === "results") return text(event.facts.date) || undefined;
  if (event.kind === "dividends") return text(event.facts.recordDate) || undefined;
  return undefined;
}
