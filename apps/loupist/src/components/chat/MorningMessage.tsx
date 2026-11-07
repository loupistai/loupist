import { formatDate, formatNumber, formatTime } from "@/i18n/format";
import i18n, { useTranslation, Trans } from "@/i18n";
import { useEffect, useId, useMemo, useState, type ReactNode } from "react";
import type { ChatEvidenceRef, LoupistBridge, LoupistCapabilityBinding, LoupistTemplateId } from "../../types";
import type { GrowthEvent, JournalRecord, KnowledgeMap } from "../../contracts/knowledge";
import type { InboxItem } from "../../contracts/scheduled";
import type { WatchlistEntry } from "../../contracts/userContext";
import { lastSeenAt } from "../../app/loupistVisit";
import { firstSentence, localDayMs, morningMessage, sinceDayOptions, type CheckBullet, type FollowedBullet, type MorningQuestion } from "../../chat/morningMessage";
import type { AssetDelta, AssetEvidence, UpcomingAsset, UpcomingEvent } from "../../contracts/assets";
import type { OpenAssetPage } from "../assets/assetOpener";
import { useGo } from "../navigation/Open";
import { hostSourceDestination } from "../../chat/navigation";
import { VerdictChip } from "../assets/VerdictChip";
import { citationNumbers, datedSentence, SourceChips } from "../assets/assetText";
import { localAssetName } from "../assets/assetName";
import { datedWatchSentence, newestFirst } from "../assets/watchText";
import type { WatchEvent } from "../../contracts/watch";
import { useSeenAfter } from "./ThreadWatch";

const t = i18n.getFixedT(null, "chat");
/** The Briefings tab owns the name of the scheduled briefing. */
const tBriefings = i18n.getFixedT(null, "briefings");
/** Template and reason keys are chosen at runtime; their parameters come from `morningMessage`. */
const tDynamic = t as unknown as (key: string, options?: Record<string, unknown>) => string;
/** A section that is still loading after this long shows a placeholder line. */
const SLOW_MS = 400;
const MAX_EVENT_PAGES = 4;

/**
 * The connections an asset-class template works best with
 * (finance-crypto-and-templates.md §3.4). Its first morning message names the
 * ones this Loupist does not have yet; names are the providers' own.
 */
const WORKS_BEST_WITH: Partial<Record<LoupistTemplateId, readonly { id: string; name: string }[]>> = {
  crypto: [{ id: "tiingo.market-data", name: "Tiingo" }],
  macro: [{ id: "fred.macro", name: "FRED" }],
  commodities: [{ id: "eia.energy", name: "EIA" }],
};

type Load<T> = { status: "loading" | "done" | "failed"; value?: T };

function useLoad<T>(read: () => Promise<T>, key: string): Load<T> {
  const [state, setState] = useState<Load<T> & { key: string }>({ key, status: "loading" });
  useEffect(() => {
    let current = true;
    setState({ key, status: "loading" });
    void read().then((value) => { if (current) setState({ key, status: "done", value }); })
      .catch(() => { if (current) setState({ key, status: "failed" }); });
    return () => { current = false; };
    // `read` closes over the same inputs as `key`.
  }, [key]);
  return state.key === key ? state : { status: "loading" };
}

function useSlow(loading: boolean) {
  const [slow, setSlow] = useState(false);
  useEffect(() => {
    if (!loading) { setSlow(false); return; }
    const timer = window.setTimeout(() => setSlow(true), SLOW_MS);
    return () => window.clearTimeout(timer);
  }, [loading]);
  return loading && slow;
}

const shortDate = (ms: number) => formatDate(ms, { year: undefined, month: "short", day: "numeric" });

type Props = {
  bridge: LoupistBridge;
  loupistId: string;
  loupistName: string;
  focus?: string;
  /** Puts the question into the composer; it never sends. */
  onAsk: (question: string) => void;
  /**
   * `full`: the empty home Thread's landing. `news`: closes the home Thread
   * when something changed since the last visit, otherwise nothing.
   * `questions`: an empty topic Chat offers only the questions.
   */
  mode?: "full" | "news" | "questions";
  /** Runs already posted in the Thread: their briefing is not repeated as a bullet (one message per event). */
  deliveredRunIds?: readonly string[];
  /** Called once every section has loaded, so the Thread can keep its end in view. */
  onSettled?: () => void;
  /** The chat answers as of this date: the event strip, which looks ahead from today, is left out. */
  asOf?: string;
  /** The template the Loupist was made from: a class template's first message names the connections it works best with. */
  template?: LoupistTemplateId;
};

/**
 * The empty Chat's landing: the Loupist's morning message. What changed since the
 * last visit, the latest briefing, Journal checks, and three questions, all
 * from data on this computer. It is not saved; the first message replaces it.
 */
export function MorningMessage({ bridge, loupistId, loupistName, focus, onAsk, mode = "full", deliveredRunIds, onSettled, asOf, template }: Props) {
  const go = useGo();
  const openAsset: OpenAssetPage | null = go ? (target) => go({ kind: "asset", target }) : null;
  const openMap = () => go?.({ kind: "page", location: { loupistId, surface: "world", view: { followCurrent: true, whatsNew: true } } });
  const openJournal = (journalId: string, title: string) => go?.({ kind: "memory", location: { loupistId, surface: "note", objectId: journalId, selection: { kind: "journal", journalId } }, title });
  const { i18n: instance } = useTranslation("chat");
  const language = instance.language;
  const reasonIds = useId();
  const [opened] = useState(() => ({ nowMs: Date.now(), lastSeenAtMs: lastSeenAt(loupistId) }));
  const { nowMs, lastSeenAtMs } = opened;

  const map = useLoad<KnowledgeMap>(() => bridge.knowledgeMap({ loupistId, maxNodes: 1000 }), `map:${loupistId}`);
  const events = useLoad<GrowthEvent[]>(async () => {
    const all: GrowthEvent[] = [];
    let cursor: string | undefined;
    for (let page = 0; page < MAX_EVENT_PAGES; page += 1) {
      const result = await bridge.knowledgeGrowthEvents({ loupistId, sinceMs: lastSeenAtMs === undefined ? undefined : lastSeenAtMs + 1, limit: 5000, cursor });
      all.push(...result.events);
      cursor = result.nextCursor ?? undefined;
      if (!cursor) break;
    }
    return all;
  }, `events:${loupistId}:${lastSeenAtMs ?? "first"}`);
  const inbox = useLoad<InboxItem | null>(async () => {
    const page = await bridge.inboxList({ loupistId, limit: 10 });
    return page.items.find((item) => item.kind === "briefing" && item.status === "succeeded") ?? null;
  }, `inbox:${loupistId}`);
  const journal = useLoad<JournalRecord[]>(() => bridge.journalList({ loupistId }), `journal:${loupistId}`);
  const watchlist = useLoad<WatchlistEntry[]>(() => bridge.watchlistList(), "watchlist");
  // Followed assets change only "since" a visit; a first visit has none.
  const assets = useLoad<AssetDelta[]>(async () => lastSeenAtMs === undefined ? [] : (await bridge.assetDelta?.({ loupistId, sinceMs: lastSeenAtMs }).catch(() => [])) ?? [], `assets:${loupistId}:${lastSeenAtMs ?? "first"}`);
  // Alerts the first check of the day found go here instead of a separate Thread message.
  const alerts = useLoad<WatchEvent[]>(async () => lastSeenAtMs === undefined ? [] : (await bridge.watchEventsList?.({ loupistId, sinceMs: lastSeenAtMs, limit: 50 }).catch(() => [])) ?? [], `alerts:${loupistId}:${lastSeenAtMs ?? "first"}`);
  // The event strip looks ahead from today, so a topic Chat's questions and an as-of chat go without it.
  const withStrip = mode !== "questions" && !asOf;
  const upcoming = useLoad<UpcomingEvent[]>(async () => withStrip ? (await bridge.upcomingEvents?.({ loupistId }).catch(() => [])) ?? [] : [], `upcoming:${loupistId}:${withStrip}`);
  // A class Loupist's first message names, once, the connections it works best with that it does not have.
  const recommended = mode === "full" && lastSeenAtMs === undefined && template ? WORKS_BEST_WITH[template] : undefined;
  const connections = useLoad<LoupistCapabilityBinding[] | null>(async () => recommended ? await bridge.loupistCapabilityList(loupistId).catch(() => null) : null, `connections:${loupistId}:${recommended ? template : ""}`);
  const missingConnections = recommended && connections.value
    ? recommended.filter((entry) => !connections.value!.some((binding) => binding.entry_id === entry.id && binding.enabled && binding.available))
    : [];

  const sinceLabel = lastSeenAtMs === undefined ? "" : formatDate(lastSeenAtMs, sinceDayOptions(lastSeenAtMs, nowMs));
  const deliveredKey = deliveredRunIds?.join(",") ?? "";
  const briefing = inbox.value && deliveredRunIds?.includes(inbox.value.runId) ? null : inbox.value;
  const message = useMemo(() => morningMessage({
    language, nowMs, lastSeenAtMs, focus,
    map: map.value, events: events.value, briefing, journal: journal.value, watchlist: watchlist.value, assets: assets.value, watchEvents: alerts.value, upcoming: upcoming.value,
  }, sinceLabel), [language, nowMs, lastSeenAtMs, focus, map.value, events.value, briefing, deliveredKey, journal.value, watchlist.value, assets.value, alerts.value, upcoming.value, sinceLabel]);

  const changedLoading = map.status === "loading" || events.status === "loading";
  const allSettled = [map, events, inbox, journal, watchlist, assets, alerts, upcoming, connections].every((load) => load.status !== "loading");
  // One name per asset in this language, from the Map and pages already read for this message.
  const nameOf = (asset: Pick<AssetDelta, "assetId" | "symbol" | "market" | "name" | "entityId">) => localAssetName(asset, { nodes: map.value?.nodes, pages: assets.value }, language);
  // Alerts shown in the bullet for a second are seen and leave Needs attention.
  useSeenAfter(bridge, loupistId, (message.followed?.alerts ?? []).map((event) => event.eventId), mode !== "questions");
  useEffect(() => { if (allSettled) onSettled?.(); }, [allSettled]);
  const changedSlow = useSlow(changedLoading);
  const briefingSlow = useSlow(inbox.status === "loading");
  const checksSlow = useSlow(journal.status === "loading");
  const leadSlow = useSlow(!allSettled);

  const lead = (() => {
    switch (message.lead.kind) {
      // Korean has no singular plural form, and "한 가지" reads better than "1가지".
      case "count": return message.lead.count === 1 ? t("today.leadOne", { day: sinceLabel })
        : t("today.lead", { count: message.lead.count, formattedCount: formatNumber(message.lead.count), day: sinceLabel });
      case "toLook": return message.lead.count === 1 ? t("today.leadToLookOne")
        : t("today.leadToLook", { count: message.lead.count, formattedCount: formatNumber(message.lead.count) });
      case "firstDay": return t("today.leadFirstDay");
      case "nothingNew": return t("today.leadNothingNew", { day: sinceLabel });
      case "empty": return t("today.leadEmpty");
    }
  })();

  const bullets: { key: string; node: ReactNode }[] = [];
  if (changedLoading) { if (changedSlow) bullets.push({ key: "changed", node: <span className="morning-skeleton" /> }); }
  else if (message.changed) {
    const { names, links, chips, more } = message.changed;
    const namesPart = names ? t("today.namesPart", { count: names, formattedCount: formatNumber(names) }) : "";
    const linksPart = links ? t("today.linksPart", { count: links, formattedCount: formatNumber(links) }) : "";
    bullets.push({ key: "changed", node: <>
      <span>{namesPart && linksPart ? t("today.changedBoth", { names: namesPart, links: linksPart }) : t("today.changedOne", { part: namesPart || linksPart })}</span>
      {chips.length ? <span className="morning-chips">
        {chips.map((chip) => <button type="button" className="morning-name-chip" key={chip.id} onClick={() => go?.({ kind: "page", location: { loupistId, surface: "world", recordId: map.value?.nodes.find((node) => node.id === chip.id)?.recordId, objectId: chip.id, selection: { kind: "node", id: chip.id } } })}>
          <i className={`map-swatch is-${chip.swatch}`} aria-hidden="true" />{chip.label}
        </button>)}
        {more ? <button type="button" className="morning-name-chip" onClick={openMap}>{t("today.moreNames", { formattedCount: formatNumber(more) })}</button> : null}
      </span> : null}
      <button type="button" className="morning-link" data-line="" onClick={openMap}>{t("today.seeAllInMap")}</button>
    </> });
  }
  if (message.followed) {
    bullets.push({ key: "followed", node: <FollowedLine bullet={message.followed} loupistId={loupistId} loupistName={loupistName} nameOf={nameOf} onOpenAsset={openAsset} /> });
  }
  if (inbox.status === "loading") { if (briefingSlow) bullets.push({ key: "briefing", node: <span className="morning-skeleton" /> }); }
  else if (message.briefing) {
    const run = message.briefing;
    bullets.push({ key: "briefing", node: <>
      <span>{t("today.briefingBullet", { title: tBriefings("morning"), date: `${shortDate(run.finishedAtMs ?? run.startedAtMs)}, ${formatTime(run.finishedAtMs ?? run.startedAtMs)}` })} {firstSentence(run.summary)}</span>
      {go ? <button type="button" className="morning-link" onClick={() => go({ kind: "briefing", runId: run.runId })}>{t("today.open")}</button> : null}
    </> });
  }
  // A check due soon belongs to the strip, so the bullets wait for it rather than move a line away.
  if (journal.status === "loading" || upcoming.status === "loading") { if (checksSlow) bullets.push({ key: "checks", node: <span className="morning-skeleton" /> }); }
  else for (const check of message.checks) bullets.push({ key: `check:${check.journalId}`, node: <CheckLine check={check} onReview={() => openJournal(check.journalId, check.claim)} /> });
  if (missingConnections.length) {
    bullets.push({ key: "worksBest", node: <span data-works-best="">
      <Trans ns="chat" i18nKey="today.worksBest" values={{ connections: missingConnections.map((entry) => entry.name).join(", ") }}
        components={[go ? <button type="button" className="morning-link" onClick={() => go({ kind: "settings", section: "connections" })} /> : <span />]} />
    </span> });
  }

  const ask = allSettled && message.questions.length ? <section className="morning-ask" aria-label={t("today.youCouldAsk")}>
    <p className="morning-ask-label">{t("today.youCouldAsk")}</p>
    <ul>{message.questions.map((question, index) => {
      const text = questionText(question);
      const reason = reasonText(question);
      const reasonId = `${reasonIds}-${index}`;
      // The question is the name; why it is offered is the description.
      return <li key={`${question.template}:${text}`}>
        <button type="button" className="morning-question" aria-describedby={reason ? reasonId : undefined} onClick={() => onAsk(text)}>
          <span>{text}</span>
          {reason ? <small id={reasonId} aria-hidden="true">{reason}</small> : null}
        </button>
      </li>;
    })}</ul>
  </section> : null;
  if (mode === "questions") return ask ? <div className="morning-ask-only">{ask}</div> : null;
  // In a Thread with history, the message appears only when there is news.
  if (mode === "news" && (!allSettled || message.lead.kind !== "count")) return null;
  return (
    <article className="message assistant morning-message" aria-label={t("today.label", { loupist: loupistName })} aria-busy={!allSettled}>
      <div className="message-meta">
        <span>{loupistName}</span>
        <time>{formatTime(nowMs)}</time>
      </div>
      {/* Shown once the names it uses are read, so a Korean name never replaces an English one in place. */}
      {message.upcoming.length && map.status !== "loading" && assets.status !== "loading" ? <UpcomingStrip events={message.upcoming} loupistId={loupistId} nameOf={nameOf} onOpenAsset={openAsset} onOpenJournal={openJournal} /> : null}
      {allSettled ? <p className="morning-lead">{lead}</p> : leadSlow ? <p className="morning-lead"><span className="morning-skeleton" /></p> : null}
      {bullets.length ? <ul className="morning-bullets">{bullets.map((bullet) => <li key={bullet.key}>{bullet.node}</li>)}</ul> : null}
      {ask}
    </article>
  );
}

/** "What you follow:" up to three assets, each with its verdict and most material row, dated and numbered across the message. */
function FollowedLine({ bullet, loupistId, loupistName, nameOf, onOpenAsset }: {
  bullet: FollowedBullet; loupistId: string; loupistName: string; nameOf: (delta: AssetDelta) => string;
  onOpenAsset: OpenAssetPage | null;
}) {
  const go = useGo();
  const open = (assetId: string, name?: string) => onOpenAsset?.({ loupistId, assetId, name });
  const openEvidence = (assetId: string) => (_source: AssetEvidence, citation: ChatEvidenceRef) => go?.(hostSourceDestination(citation, { loupistId, assetId }));
  const numbers = citationNumbers([...bullet.rows.map(({ row }) => row.evidence), ...bullet.alerts.map((event) => event.evidence)]);
  return <>
    <span>{t("today.assetsBullet")}</span>
    <ul className="morning-followed">
      {bullet.rows.map(({ delta, row }) => {
        const name = nameOf(delta);
        return <li key={delta.assetId}>
          {onOpenAsset ? <button type="button" className="morning-link morning-asset-name" onClick={() => open(delta.assetId, name)}>{name}</button> : <strong>{name}</strong>}
          {delta.verdict ? <VerdictChip verdict={delta.verdict} loupist={loupistName} /> : null}
          <span>{datedSentence(row)} <SourceChips evidence={row.evidence} numbers={numbers} onOpen={openEvidence(delta.assetId)} /></span>
        </li>;
      })}
      {newestFirst(bullet.alerts).map((event) => {
        const name = nameOf({ assetId: event.assetId, symbol: event.symbol, market: event.market, name: event.name, class: "equity", rows: [], moved: false, crossed: false });
        return <li key={event.eventId} className="morning-alert" data-kind={event.kind}>
          <span>{datedWatchSentence(event, name)} <SourceChips evidence={event.evidence} numbers={numbers} onOpen={openEvidence(event.assetId)} /></span>
          {onOpenAsset ? <button type="button" className="morning-link" aria-label={`${i18n.t("assets:page.open")} · ${name}`} onClick={() => open(event.assetId, name)}>{i18n.t("assets:page.open")}</button> : null}
        </li>;
      })}
    </ul>
    {bullet.more && bullet.nextAssetId && onOpenAsset ? <button type="button" className="morning-link" onClick={() => open(bullet.nextAssetId!)}>{t("today.assetsMore", { formattedCount: formatNumber(bullet.more) })}</button> : null}
  </>;
}

/** "Tue, Nov 4" / "11월 4일 (화)": the weekday says how soon without a relative word. */
const upcomingDay = (date: string) => {
  const day = localDayMs(date);
  return Number.isFinite(day) ? formatDate(day, { weekday: "short", year: undefined, month: "short", day: "numeric" }) : date;
};

/** The event in words; a results date says, as the asset page does, how sure it is. */
function upcomingText(event: UpcomingEvent) {
  switch (event.kind) {
    case "results": return event.dateSource === "legal_deadline" ? t("today.upcoming.resultsDue")
      : event.dateSource === "ir_notice" ? t("today.upcoming.resultsConfirmed") : t("today.upcoming.resultsExpected");
    case "exDividend": return t("today.upcoming.exDividend");
    case "recordDate": return t("today.upcoming.recordDate");
    case "check": return t("today.upcoming.check", { claim: event.claim });
  }
}

/**
 * The event strip above the lead (W15.9e): at most five dated events ahead,
 * soonest first, one line each (date, asset, event). An asset's event opens its
 * page (results on the results card); a check opens its Journal entry.
 */
function UpcomingStrip({ events, loupistId, nameOf, onOpenAsset, onOpenJournal }: {
  events: readonly UpcomingEvent[]; loupistId: string; nameOf: (asset: UpcomingAsset) => string;
  onOpenAsset: OpenAssetPage | null; onOpenJournal: (journalId: string, title: string) => void;
}) {
  const labelId = useId();
  return <section className="morning-upcoming" aria-labelledby={labelId}>
    <p className="morning-upcoming-label" id={labelId}>{t("today.upcoming.label")}</p>
    <ul>{events.map((event) => {
      const name = event.asset ? nameOf(event.asset) : "";
      const what = upcomingText(event);
      const date = upcomingDay(event.date);
      const open = event.kind === "check" ? () => onOpenJournal(event.journalId, event.claim)
        : onOpenAsset ? () => onOpenAsset({ loupistId, assetId: event.asset.assetId, name, ...(event.kind === "results" ? { section: "results" as const, eventId: event.eventId } : {}) })
          : undefined;
      const cells = <>
        <time dateTime={event.date}>{date}</time>{" "}
        {name ? <><span className="morning-upcoming-name">{name}</span>{" "}</> : null}
        <span className="morning-upcoming-event" data-wide={name ? undefined : ""}>{what}</span>
      </>;
      const key = `${event.kind}:${event.kind === "check" ? event.journalId : event.asset.assetId}:${event.date}`;
      // The row reads as one sentence; the full text stays available when a narrow window cuts it.
      const label = [date, name, what].filter(Boolean).join(" · ");
      return <li key={key} data-kind={event.kind}>
        {open ? <button type="button" className="morning-upcoming-row" title={label} onClick={open}>{cells}</button>
          : <span className="morning-upcoming-row" title={label}>{cells}</span>}
      </li>;
    })}</ul>
  </section>;
}

function CheckLine({ check, onReview }: { check: CheckBullet; onReview: () => void }) {
  const date = shortDate(check.byDateMs);
  // Only "Overdue" takes the warning color; the sentence stays readable.
  const text = check.daysLeft < 0
    ? <Trans ns="chat" i18nKey="today.checkLate" values={{ claim: check.claim, date }} components={[<span className="morning-late" />]} />
    : check.daysLeft === 0 ? t("today.checkDueToday", { claim: check.claim }) : t("today.checkDue", { claim: check.claim, date });
  return <>
    <span>{text}</span>
    <button type="button" className="morning-link" aria-label={t("today.reviewCheck")} onClick={onReview}>{t("today.review")}</button>
  </>;
}

function questionText(question: MorningQuestion) {
  return tDynamic(`today.q.${question.template}`, question.params);
}

function reasonText(question: MorningQuestion) {
  const reason = question.reason;
  if (!reason) return "";
  if (reason.key === "check") {
    const due = localDayMs(reason.params.date ?? "");
    return t("today.reason.check", { date: Number.isFinite(due) ? shortDate(due) : reason.params.date });
  }
  // A long source title is cut at a word, never mid-word, so the reason stays readable at narrow widths.
  if (reason.key === "source") return t("today.reason.source", { title: wordTrimmed(reason.params.title ?? "", 40) });
  return tDynamic(`today.reason.${reason.key}`, reason.params);
}

/** `text` cut to at most `limit` characters at a word boundary, with an ellipsis when cut. */
export function wordTrimmed(text: string, limit: number) {
  const clean = text.replace(/\s+/gu, " ").trim();
  if ([...clean].length <= limit) return clean;
  const cut = [...clean].slice(0, limit).join("");
  const space = cut.lastIndexOf(" ");
  return `${(space > limit / 2 ? cut.slice(0, space) : cut).replace(/[\s,.;:–—-]+$/u, "")}…`;
}
