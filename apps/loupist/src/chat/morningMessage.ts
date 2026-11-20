import type { GrowthEvent, JournalRecord, KnowledgeMap, KnowledgeMapEdge, KnowledgeMapNode } from "../contracts/knowledge";
import type { InboxItem } from "../contracts/scheduled";
import type { AssetRow } from "../contracts/assets";
import { claimIdForEdge, entityLabel, oneName } from "../knowledge/mapItems";
import { isMetricNode } from "../knowledge/mapKinds";

const DAY_MS = 24 * 60 * 60_000;
const HOUR_MS = 60 * 60_000;
/** Bot posts its news only after the person was away this long, */
export const BOT_NEWS_AWAY_MS = 4 * HOUR_MS;
/** or on the first open of a new local day once they were away at least this long. */
export const BOT_NEWS_NEW_DAY_MS = HOUR_MS;
const MAX_CHECKS = 3;
const MAX_QUESTIONS = 3;
/** A focus written as a sentence reads badly inside "What matters in … this week?". */
const MAX_FOCUS_IN_QUESTION = 48;

export type MorningInputs = {
  language: string;
  nowMs: number;
  /** Undefined on the first visit. */
  lastSeenAtMs?: number;
  /**
   * Bot's news: only what the person follows (briefings and checks), and questions
   * that do not come from the Map. The Map shows its own growth.
   */
  bot?: boolean;
  map?: KnowledgeMap | null;
  /** Growth events since `lastSeenAtMs` (all of them on the first visit). */
  events?: readonly GrowthEvent[] | null;
  briefing?: InboxItem | null;
  journal?: readonly JournalRecord[] | null;
  /** Followed assets, from Rust `assetsList`. */
  watchlist?: readonly Pick<AssetRow, "assetId" | "name">[] | null;
  focus?: string;
};

export type CheckBullet = { journalId: string; claim: string; metric: string; byDate: string; byDateMs: number; daysLeft: number };
export type QuestionTemplate = "changed" | "linked" | "explains" | "check" | "conflict" | "briefing" | "focus" | "learned" | "general" | "markets" | "risks";
export type ReasonKey = "new" | "relation" | "source" | "check" | "conflict" | "watching";
export type MorningQuestion = {
  template: QuestionTemplate;
  params: Record<string, string>;
  reason?: { key: ReasonKey; params: Record<string, string> };
};
export type MorningMessage = {
  /** Every line the message shows: the briefing and each check. */
  count: number;
  briefing?: InboxItem;
  checks: CheckBullet[];
  questions: MorningQuestion[];
};

/** The calendar day `byDate` (YYYY-MM-DD) in local time, or NaN. */
export function localDayMs(byDate: string) {
  const match = /^(\d{4})-(\d{2})-(\d{2})/.exec(byDate);
  return match ? new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3])).getTime() : Number.NaN;
}

/** A claim quoted inside a sentence keeps its words, not its full stop. */
const quotable = (text: string) => text.trim().replace(/[.。…]+$/u, "").trim();

const startOfDay = (ms: number) => { const day = new Date(ms); day.setHours(0, 0, 0, 0); return day.getTime(); };

/** Whole days from today to `byDate`; negative when the date has passed. */
export function daysUntil(byDate: string, nowMs: number) {
  const due = localDayMs(byDate);
  return Number.isFinite(due) ? Math.round((due - startOfDay(nowMs)) / DAY_MS) : Number.NaN;
}

/** A summary that opens with its own date ("October 28: …", "10월 28일: …") repeats the bullet's date. */
const LEADING_DATE = /^(?:(?:jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\.? \d{1,2}(?:, \d{4})?|\d{4}-\d{2}-\d{2}|(?:\d{4}년 )?\d{1,2}월 \d{1,2}일)\s*[:–—-]\s*/iu;

/** The first sentence of a run summary, without a leading date; the Briefings tab shows the rest. */
export function firstSentence(text: string) {
  const trimmed = text.trim().replace(LEADING_DATE, "");
  const match = /^(.+?[.!?。])(\s|$)/su.exec(trimmed);
  return (match ? match[1] : trimmed).trim();
}

const entityNodes = (map?: KnowledgeMap | null) => new Map((map?.nodes ?? []).filter((node) => node.kind === "entity" && !node.archived).map((node) => [node.id, node]));

function claimEdges(map?: KnowledgeMap | null) {
  const byClaim = new Map<string, KnowledgeMapEdge>();
  for (const edge of map?.edges ?? []) if (edge.kind === "claim") byClaim.set(claimIdForEdge(edge), edge);
  return byClaim;
}

/** Newest first; schema and truncation markers are not things a person reads as news. The Map's What's new shares this rule. */
export const relevantEvents = (events?: readonly GrowthEvent[] | null) =>
  [...(events ?? [])].filter((event) => !event.kind.startsWith("schema_") && event.kind !== "growth_truncated").sort((a, b) => b.atMs - a.atMs);

/** Open dated checks, soonest first. */
function pendingChecks(journal: readonly JournalRecord[] | null | undefined, nowMs: number): CheckBullet[] {
  return (journal ?? [])
    .flatMap((entry): CheckBullet[] => {
      const check = entry.check;
      // An undated line is read on its asset page, not reviewed on a day.
      const byDate = check?.byDate;
      if (entry.archived || !check || !byDate || entry.outcome) return [];
      const byDateMs = localDayMs(byDate);
      if (!Number.isFinite(byDateMs)) return [];
      return [{ journalId: entry.journalId, claim: quotable(check.claim || entry.judgment), metric: quotable(check.metric || check.claim || entry.judgment), byDate, byDateMs, daysLeft: daysUntil(byDate, nowMs) }];
    })
    .sort((a, b) => a.byDateMs - b.byDateMs)
    .slice(0, MAX_CHECKS);
}

/** Template candidates in priority order (spec today-onboarding.md §3.3). */
function questionCandidates(input: MorningInputs, events: GrowthEvent[], nodes: Map<string, KnowledgeMapNode>, claims: Map<string, KnowledgeMapEdge>, checks: CheckBullet[], sinceLabel: string, withDiff: boolean) {
  // Bot asks about what the person follows; questions built from Map entries belong to the Map and research sessions.
  if (input.bot) {
    const candidates: (MorningQuestion & { subject: string })[] = checks.map((check) => ({ template: "check", subject: `check:${check.journalId}`, params: { metric: check.metric }, reason: { key: "check", params: { date: check.byDate } } }));
    for (const entry of input.watchlist ?? []) candidates.push({ template: "briefing", subject: `asset:${entry.assetId}`, params: { symbol: entry.name }, reason: { key: "watching", params: {} } });
    for (const template of ["general", "markets", "risks"] as const) candidates.push({ template, subject: template, params: {} });
    return candidates;
  }
  const name = (node: KnowledgeMapNode) => entityLabel(node, input.language);
  const candidates: (MorningQuestion & { subject: string })[] = [];
  if (withDiff) {
    const touched = new Set<string>();
    for (const event of events) {
      if (!event.subjectId) continue;
      if (event.kind === "entity_added") touched.add(event.subjectId);
      const edge = event.kind === "claim_added" ? claims.get(event.subjectId) : undefined;
      if (edge) { touched.add(edge.source); touched.add(edge.target); }
    }
    for (const id of touched) {
      const node = nodes.get(id);
      if (node?.watched) candidates.push({ template: "changed", subject: node.id, params: { name: name(node), date: sinceLabel }, reason: { key: "new", params: {} } });
    }
    // The user's own list of what they follow is not where a reported value came from.
    const sourceTitle = events.find((event) => event.kind === "source_retained" && !event.titleKey && event.summary.trim())?.summary.trim();
    for (const event of events) {
      if (event.kind !== "claim_added" || !event.subjectId) continue;
      const edge = claims.get(event.subjectId);
      const a = edge ? nodes.get(edge.source) : undefined;
      const b = edge ? nodes.get(edge.target) : undefined;
      // A value reported about one name names it twice: there is no link to ask about.
      if (!edge || !a || !b || oneName(a, b, input.language)) continue;
      const metric = isMetricNode(a) ? a : isMetricNode(b) ? b : undefined;
      if (metric) candidates.push({ template: "explains", subject: metric.id, params: { subject: name(metric) }, reason: sourceTitle ? { key: "source", params: { title: sourceTitle } } : { key: "relation", params: {} } });
      else candidates.push({ template: "linked", subject: a.id, params: { a: name(a), b: name(b) }, reason: { key: "relation", params: {} } });
    }
    for (const check of checks) candidates.push({ template: "check", subject: `check:${check.journalId}`, params: { metric: check.metric }, reason: { key: "check", params: { date: check.byDate } } });
    for (const event of events) {
      if (event.kind !== "claim_disputed" || !event.subjectId) continue;
      const edge = claims.get(event.subjectId);
      const endpoints = edge ? [nodes.get(edge.source), nodes.get(edge.target)].filter((node): node is KnowledgeMapNode => Boolean(node)) : [];
      const subject = endpoints.find((node) => !isMetricNode(node)) ?? endpoints[0];
      if (subject) candidates.push({ template: "conflict", subject: subject.id, params: { name: name(subject) }, reason: { key: "conflict", params: {} } });
    }
  }
  for (const entry of input.watchlist ?? []) candidates.push({ template: "briefing", subject: `asset:${entry.assetId}`, params: { symbol: entry.name }, reason: { key: "watching", params: {} } });
  const focus = input.focus?.trim();
  if (focus && focus.length <= MAX_FOCUS_IN_QUESTION && !/[.!?。]$/u.test(focus)) candidates.push({ template: "focus", subject: "focus", params: { focus } });
  if (nodes.size) candidates.push({ template: "learned", subject: "learned", params: {} });
  // General questions keep the list at three when the Loupist has little to point at yet.
  for (const template of ["general", "markets", "risks"] as const) candidates.push({ template, subject: template, params: {} });
  // "Linked" and "explains" share the relationship reason; keep the priority order by template.
  const order: QuestionTemplate[] = ["changed", "linked", "explains", "check", "conflict", "briefing", "focus", "learned", "general", "markets", "risks"];
  return candidates.sort((a, b) => order.indexOf(a.template) - order.indexOf(b.template));
}

function pickQuestions(candidates: (MorningQuestion & { subject: string })[]) {
  const usedTemplates = new Set<QuestionTemplate>();
  const usedSubjects = new Set<string>();
  const picked: MorningQuestion[] = [];
  for (const candidate of candidates) {
    if (picked.length >= MAX_QUESTIONS) break;
    if (usedTemplates.has(candidate.template) || usedSubjects.has(candidate.subject)) continue;
    usedTemplates.add(candidate.template);
    usedSubjects.add(candidate.subject);
    const { subject: _subject, ...question } = candidate;
    picked.push(question);
  }
  return picked;
}

/**
 * Whether Bot posts its news now: never while the person is using the app. They
 * must have been away (since `lastActiveAtMs`, the later of the last counted
 * visit's end and their last message) for four hours, or for an hour across
 * midnight, which makes this the first open of a new local day.
 */
export function botNewsDue(lastActiveAtMs: number | undefined, nowMs: number) {
  if (lastActiveAtMs === undefined) return false;
  const away = nowMs - lastActiveAtMs;
  return away >= BOT_NEWS_AWAY_MS || (away >= BOT_NEWS_NEW_DAY_MS && startOfDay(lastActiveAtMs) < startOfDay(nowMs));
}

/**
 * The Loupist's news from data already on this computer: the latest briefing,
 * Journal checks, and three questions. Deterministic; no model call.
 */
export function morningMessage(input: MorningInputs, sinceLabel = ""): MorningMessage {
  const nodes = entityNodes(input.map);
  const claims = claimEdges(input.map);
  const events = relevantEvents(input.events);
  const firstDay = input.lastSeenAtMs === undefined;
  const briefing = input.briefing && input.briefing.status === "succeeded" && (firstDay || (input.briefing.finishedAtMs ?? input.briefing.startedAtMs) > (input.lastSeenAtMs ?? 0) || input.briefing.readAtMs === null)
    ? input.briefing : undefined;
  const checks = pendingChecks(input.journal, input.nowMs);
  const withDiff = !firstDay && events.length > 0;
  const questions = pickQuestions(questionCandidates(input, events, nodes, claims, checks, sinceLabel, withDiff));
  const count = (briefing ? 1 : 0) + checks.length;
  return { count, briefing, checks, questions };
}

/** "2:07 AM" earlier today, "Tuesday" within the last six days, else "Oct 21". */
export function sinceDayOptions(sinceMs: number, nowMs: number): Intl.DateTimeFormatOptions {
  const days = startOfDay(nowMs) - startOfDay(sinceMs);
  if (days <= 0) return { year: undefined, month: undefined, day: undefined, hour: "numeric", minute: "2-digit" };
  return days <= 6 * DAY_MS
    ? { weekday: "long", year: undefined, month: undefined, day: undefined }
    : { year: undefined, month: "short", day: "numeric" };
}
