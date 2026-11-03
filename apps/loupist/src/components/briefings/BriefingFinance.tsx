import i18n, { useTranslation } from "@/i18n";
import type { LoupistBridge } from "../../types";
import type { AssetEvidence } from "../../contracts/assets";
import type { BriefingContext, BriefingEarnings } from "../../contracts/scheduled";
import type { WatchEvent } from "../../contracts/watch";
import { Button } from "../ui/button";
import { useAssetPageOpener } from "../assets/assetOpener";
import { useAssetNames } from "../assets/assetName";
import { citationNumbers, shortDay, SourceChips } from "../assets/assetText";
import { EarningsMiniTable } from "../assets/EarningsCard";
import { aheadDay, aheadDividendSentence, datedWatchSentence, newestFirst } from "../assets/watchText";
import "@/styles/watch.css";
import "@/styles/earnings.css";

const t = i18n.getFixedT(null, "briefings");
const DAY_MS = 86_400_000;
/** Alerts that join "What you follow"; results and record dates look ahead instead. */
const FOLLOW_KINDS = new Set<WatchEvent["kind"]>(["filings", "insiders", "holders", "flows"]);

const reported = (event: BriefingEarnings) => event.status === "reported" || event.status === "final";
/** Whole days from today to a calendar day. */
function daysAhead(day: string) {
  const [year, month, date] = day.slice(0, 10).split("-").map(Number);
  if (!year || !month || !date) return Number.NaN;
  const today = new Date(); today.setHours(0, 0, 0, 0);
  return Math.round((new Date(year, month - 1, date).getTime() - today.getTime()) / DAY_MS);
}

/** Dividend record dates within seven days, for "Looking ahead". */
const aheadRecords = (context: BriefingContext) => (context.watchEvents ?? []).filter(event => event.kind === "dividends" && aheadDay(event) !== undefined && daysAhead(aheadDay(event)!) >= 0 && daysAhead(aheadDay(event)!) <= 7);

/**
 * One number per source across the briefing's results, alerts and the record
 * dates ahead, so "Looking ahead" continues the numbering of "What you follow".
 */
function briefingNumbers(context: BriefingContext) {
  const results = (context.earnings ?? []).filter(reported);
  const alerts = newestFirst((context.watchEvents ?? []).filter(event => FOLLOW_KINDS.has(event.kind)));
  return citationNumbers([
    ...results.flatMap(event => event.measures.slice(0, 2).flatMap(measure => [...measure.expected.map(value => value.evidence), ...measure.reported.map(value => value.evidence)])),
    ...alerts.map(event => event.evidence),
    ...aheadRecords(context).map(event => event.evidence),
  ]);
}

/** Whether the briefing's "What you follow" has rows from results or alerts. */
export const hasFollowRows = (context: BriefingContext) =>
  (context.earnings ?? []).some(reported) || (context.watchEvents ?? []).some(event => FOLLOW_KINDS.has(event.kind));

/**
 * "What you follow" rows a briefing adds (finance-earnings-snapshot.md §2.2,
 * finance-watchers.md §2.4): reported results with the two-measure table, and
 * alerts since the previous briefing. Rust computed both at run start.
 */
export function BriefingFollowRows({ context, bridge, loupistId, onOpenLink }: { context: BriefingContext; bridge?: LoupistBridge; loupistId: string; onOpenLink: (url: string) => void }) {
  useTranslation(["briefings", "assets", "chat"]);
  const open = useAssetPageOpener();
  const names = useAssetNames(bridge, loupistId);
  const results = (context.earnings ?? []).filter(reported);
  const alerts = newestFirst((context.watchEvents ?? []).filter(event => FOLLOW_KINDS.has(event.kind)));
  if (!results.length && !alerts.length) return null;
  const numbers = briefingNumbers(context);
  const openEvidence = (assetId: string, name: string) => (evidence: AssetEvidence) => {
    if (evidence.url) onOpenLink(evidence.url);
    else open?.({ loupistId, assetId, name });
  };
  return <>
    {results.length ? <ul className="briefing-earnings">{results.map(event => {
      const name = names(event);
      return <li key={event.eventId} data-earnings-event={event.eventId}>
        <p>{i18n.t("chat:thread.earningsReported", { name, period: event.period })}</p>
        <EarningsMiniTable measures={event.measures} numbers={numbers} onOpenEvidence={openEvidence(event.assetId, name)} />
        {open ? <Button type="button" variant="link" size="sm" className="briefing-row-link" onClick={() => open({ loupistId, assetId: event.assetId, name, section: "results", eventId: event.eventId })}>{i18n.t("assets:earnings.seeBeforeAfter")}</Button> : null}
      </li>;
    })}</ul> : null}
    {alerts.length ? <ul className="briefing-watch-rows">{alerts.map(event => {
      const name = names(event);
      return <li key={event.eventId}>{datedWatchSentence(event, name)} <SourceChips evidence={event.evidence} numbers={numbers} onOpen={openEvidence(event.assetId, name)} /></li>;
    })}</ul> : null}
  </>;
}

/** "Looking ahead": results dates and dividend record dates within seven days, each record date with its source. */
export function BriefingAheadRows({ context, bridge, loupistId, onOpenLink }: { context: BriefingContext; bridge?: LoupistBridge; loupistId: string; onOpenLink?: (url: string) => void }) {
  useTranslation(["briefings", "chat"]);
  const open = useAssetPageOpener();
  const names = useAssetNames(bridge, loupistId);
  const results = (context.earnings ?? []).filter(event => !reported(event) && daysAhead(event.reportDate) >= 0 && daysAhead(event.reportDate) <= 7);
  const records = aheadRecords(context);
  const numbers = briefingNumbers(context);
  const openEvidence = (event: WatchEvent) => (evidence: AssetEvidence) => {
    if (evidence.url && onOpenLink) onOpenLink(evidence.url);
    else open?.({ loupistId, assetId: event.assetId, name: names(event) });
  };
  return <>
    {results.map(event => <li key={event.eventId} data-ahead="results">{names(event)} · {t(event.captured ? "ahead.resultsCaptured" : "ahead.results", { date: shortDay(event.reportDate) })}</li>)}
    {records.map(event => <li key={event.eventId} data-ahead="dividend">{aheadDividendSentence(event, names(event))} <SourceChips evidence={event.evidence} numbers={numbers} onOpen={openEvidence(event)} /></li>)}
  </>;
}

/** Whether "Looking ahead" has results or record dates to list. */
export const hasAheadRows = (context: BriefingContext) =>
  (context.earnings ?? []).some(event => !reported(event) && daysAhead(event.reportDate) >= 0 && daysAhead(event.reportDate) <= 7)
  || aheadRecords(context).length > 0;
