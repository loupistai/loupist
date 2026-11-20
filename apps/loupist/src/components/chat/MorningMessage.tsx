import { formatDate, formatNumber, formatTime } from "@/i18n/format";
import i18n, { useTranslation, Trans } from "@/i18n";
import { useEffect, useId, useMemo, useState, type ReactNode } from "react";
import { ArrowRightIcon } from "lucide-react";
import type { LoupistBridge, LoupistTemplateId } from "../../types";
import type { GrowthEvent, JournalRecord, KnowledgeMap } from "../../contracts/knowledge";
import type { InboxItem } from "../../contracts/scheduled";
import type { AssetRow } from "../../contracts/assets";
import { lastSeenAt, useCurrentVisit } from "../../app/loupistVisit";
import { readKnowledgeMap } from "../../knowledge/mapRead";
import { botNewsDue, daysUntil, firstSentence, localDayMs, morningMessage, sinceDayOptions, type CheckBullet, type MorningQuestion } from "../../chat/morningMessage";
import { useGo } from "../navigation/Open";
import { BotPostBody } from "./BotPost";
import "@/styles/bot-cards.css";
import "@/styles/briefings.css";

const t = i18n.getFixedT(null, "chat");
/** The Briefings tab owns the name of the scheduled briefing. */
const tBriefings = i18n.getFixedT(null, "briefings");
const tDynamic = t as unknown as (key: string, options?: Record<string, unknown>) => string;
const MAX_EVENT_PAGES = 4;

type Load<T> = { status: "loading" | "done" | "failed"; value?: T };

/** A new `key` starts over; a new `revision` reads again behind the value already shown. */
function useLoad<T>(read: () => Promise<T>, key: string, revision = 0): Load<T> {
  const [state, setState] = useState<Load<T> & { key: string }>({ key, status: "loading" });
  useEffect(() => {
    let current = true;
    setState((previous) => previous.key === key ? previous : { key, status: "loading" });
    void read().then((value) => { if (current) setState({ key, status: "done", value }); })
      .catch(() => { if (current) setState((previous) => previous.key === key && previous.status === "done" ? previous : { key, status: "failed" }); });
    return () => { current = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `read` closes over the same inputs as `key`.
  }, [key, revision]);
  return state.key === key ? state : { status: "loading" };
}

const shortDate = (ms: number) => formatDate(ms, { year: undefined, month: "short", day: "numeric" });

type Props = {
  bridge: LoupistBridge;
  loupistId: string;
  loupistName: string;
  /** Puts the question into the composer; it never sends. */
  onAsk: (question: string) => void;
  /**
   * `news`: closes Bot's conversation after an absence when a briefing arrived or
   * a check is due, otherwise nothing.
   * `questions`: an empty research session offers only the questions.
   */
  mode?: "news" | "questions";
  /** When the person last wrote in this conversation: news waits for a real absence after it. */
  lastWroteAtMs?: number;
  /** Runs already posted in the Thread: their briefing is not repeated as a bullet (one message per event). */
  deliveredRunIds?: readonly string[];
  /** Called once every section has loaded, so the Thread can keep its end in view. */
  onSettled?: () => void;
  /** The template the Loupist was made from: its subject can be a question. */
  template?: LoupistTemplateId;
  /** Bumped each time Bot is shown again: the followed assets are read again. */
  refreshRevision?: number;
};

/**
 * Bot's news after an absence, or an empty research session's questions, from data
 * on this computer: the latest briefing, Journal checks, and three questions. It
 * is not saved; the person's next message answers it.
 */
export function MorningMessage({ bridge, loupistId, loupistName, onAsk, mode = "news", lastWroteAtMs, deliveredRunIds, onSettled, template, refreshRevision = 0 }: Props) {
  const go = useGo();
  const openJournal = (journalId: string, title: string) => go?.({ kind: "memory", location: { loupistId, surface: "note", objectId: journalId, selection: { kind: "journal", journalId } }, title });
  const { i18n: instance } = useTranslation("chat");
  const language = instance.language;
  const reasonIds = useId();
  const news = mode === "news";
  const visit = useCurrentVisit(loupistId);
  // News counts from the later of the last counted visit and the person's last message here. The
  // absence ends where this visit began, and a visit in which the person has sent a message is work, not a return.
  const [opened] = useState(() => {
    const seen = lastSeenAt(loupistId);
    const active = news && lastWroteAtMs !== undefined && (seen === undefined || lastWroteAtMs > seen) ? lastWroteAtMs : seen;
    const nowMs = Date.now();
    return { nowMs, lastSeenAtMs: active, due: !news || (!visit?.active && botNewsDue(active, visit?.openedAtMs ?? nowMs)) };
  });
  const { nowMs, lastSeenAtMs, due } = opened;
  // A message sent while the news was still being read keeps it from appearing; news already shown stays.
  const working = news && visit?.active === true;
  const [shown, setShown] = useState(false);

  // Bot's news leaves the Map's growth to the Map; only a research session's questions read it.
  const map = useLoad<KnowledgeMap | null>(async () => due ? await readKnowledgeMap(bridge, loupistId) : null, `map:${loupistId}:${due}`);
  const events = useLoad<GrowthEvent[]>(async () => {
    if (news) return [];
    const all: GrowthEvent[] = [];
    let cursor: string | undefined;
    for (let page = 0; page < MAX_EVENT_PAGES; page += 1) {
      const result = await bridge.knowledgeGrowthEvents({ loupistId, sinceMs: lastSeenAtMs === undefined ? undefined : lastSeenAtMs + 1, limit: 5000, cursor });
      all.push(...result.events);
      cursor = result.nextCursor ?? undefined;
      if (!cursor) break;
    }
    return all;
  }, `events:${loupistId}:${news}:${lastSeenAtMs ?? "first"}`);
  const inbox = useLoad<InboxItem | null>(async () => {
    if (!due) return null;
    const page = await bridge.inboxList({ loupistId, limit: 10 });
    return page.items.find((item) => item.kind === "briefing" && item.status === "succeeded") ?? null;
  }, `inbox:${loupistId}:${due}`);
  const journal = useLoad<JournalRecord[]>(async () => due ? await bridge.journalList({ loupistId }) : [], `journal:${loupistId}:${due}`);
  const watchlist = useLoad<AssetRow[]>(async () => due ? await bridge.assetsList({ loupistId }) : [], `watchlist:${loupistId}:${due}`, refreshRevision);

  const sinceLabel = lastSeenAtMs === undefined ? "" : formatDate(lastSeenAtMs, sinceDayOptions(lastSeenAtMs, nowMs));
  const briefing = inbox.value && deliveredRunIds?.includes(inbox.value.runId) ? null : inbox.value;
  // A free-text focus reads badly inside "What matters in … this week?"; a preset names its subject.
  const focusSubject = template && template !== "custom" ? tDynamic(`today.focusSubject.${template}`) : undefined;
  const message = useMemo(() => morningMessage({
    language, nowMs, lastSeenAtMs, focus: focusSubject, bot: news,
    map: map.value, events: events.value, briefing, journal: journal.value, watchlist: watchlist.value,
  }, sinceLabel), [language, nowMs, lastSeenAtMs, focusSubject, news, map.value, events.value, briefing, journal.value, watchlist.value, sinceLabel]);

  const allSettled = [map, events, inbox, journal, watchlist].every((load) => load.status !== "loading");
  const showing = news && due && allSettled && message.count > 0 && (shown || !working);
  useEffect(() => { if (showing) setShown(true); }, [showing]);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- reports settling once; `onSettled` is the parent's inline callback.
  useEffect(() => { if (allSettled) onSettled?.(); }, [allSettled]);

  const ask = allSettled && message.questions.length ? <section className="morning-ask" aria-label={t("today.youCouldAsk")}>
    <p className={news ? "morning-ask-label" : "sr-only"}>{t("today.youCouldAsk")}</p>
    <ul>{message.questions.map((question, index) => {
      const text = questionText(question);
      const reason = reasonText(question, nowMs);
      const reasonId = `${reasonIds}-${index}`;
      // The question is the name; why it is offered is the description.
      return <li key={`${question.template}:${text}`}>
        <button type="button" className="morning-question" aria-describedby={reason ? reasonId : undefined} onClick={() => onAsk(text)}>
          <span>{text}</span>
          {reason ? <small id={reasonId} aria-hidden="true">{reason}</small> : null}
          <ArrowRightIcon className="morning-question-go" aria-hidden="true" />
        </button>
      </li>;
    })}</ul>
  </section> : null;
  if (!news) return ask ? <div className="morning-ask-only"><h2 className="morning-greeting">{t("today.askGreeting")}</h2>{ask}</div> : null;
  // Nothing while the person is using the app, and nothing when nothing they follow has news.
  if (!showing) return null;

  const bullets: { key: string; node: ReactNode }[] = [];
  if (message.briefing) {
    const run = message.briefing;
    bullets.push({ key: "briefing", node: <>
      <span>{t("today.briefingBullet", { title: tBriefings("morning"), date: `${shortDate(run.finishedAtMs ?? run.startedAtMs)}, ${formatTime(run.finishedAtMs ?? run.startedAtMs)}` })} {firstSentence(run.summary)}</span>
      {go ? <button type="button" className="morning-link" onClick={() => go({ kind: "briefing", runId: run.runId })}>{t("today.open")}</button> : null}
    </> });
  }
  for (const check of message.checks) bullets.push({ key: `check:${check.journalId}`, node: <CheckLine check={check} onReview={() => openJournal(check.journalId, check.claim)} /> });
  // Korean has no singular plural form, and "한 가지" reads better than "1가지".
  const lead = message.count === 1 ? t("today.leadOne", { day: sinceLabel })
    : t("today.lead", { count: message.count, formattedCount: formatNumber(message.count), day: sinceLabel });
  return (
    <article className="message assistant morning-message" data-bot-post="" aria-label={t("today.label", { loupist: loupistName })}>
      <BotPostBody loupistName={loupistName} at={nowMs}>
        <p className="morning-lead bot-bubble">{lead}</p>
        {/* One card: the briefing that arrived and the checks coming due. */}
        <div className="bot-card morning-card">
          <ul className="morning-bullets">{bullets.map((bullet) => <li key={bullet.key}>{bullet.node}</li>)}</ul>
        </div>
        {ask}
      </BotPostBody>
    </article>
  );
}

function CheckLine({ check, onReview }: { check: CheckBullet; onReview: () => void }) {
  const textId = useId();
  const date = shortDate(check.byDateMs);
  // Only "Overdue" takes the warning color; the sentence stays readable.
  const text = check.daysLeft < 0
    ? <Trans ns="chat" i18nKey="today.checkLate" values={{ claim: check.claim, date }} components={[<span className="morning-late" />]} />
    : check.daysLeft === 0 ? t("today.checkDueToday", { claim: check.claim }) : t("today.checkDue", { claim: check.claim, date });
  // Each Review says which check it opens: the line beside it describes it.
  return <>
    <span id={textId}>{text}</span>
    <button type="button" className="morning-link" aria-label={t("today.reviewCheck")} aria-describedby={textId} onClick={onReview}>{t("today.review")}</button>
  </>;
}

function questionText(question: MorningQuestion) {
  return tDynamic(`today.q.${question.template}`, question.params);
}

function reasonText(question: MorningQuestion, nowMs: number) {
  const reason = question.reason;
  if (!reason) return "";
  if (reason.key === "check") {
    const due = localDayMs(reason.params.date ?? "");
    const date = Number.isFinite(due) ? shortDate(due) : reason.params.date;
    // A check past its date reads as the news bullet above it does, never "to check by" a past day.
    return t(daysUntil(reason.params.date ?? "", nowMs) < 0 ? "today.reason.checkLate" : "today.reason.check", { date });
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
