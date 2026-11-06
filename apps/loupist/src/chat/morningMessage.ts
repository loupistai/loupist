import type { GrowthEvent, JournalRecord, KnowledgeMap, KnowledgeMapEdge, KnowledgeMapNode } from "../contracts/knowledge";
import type { InboxItem } from "../contracts/scheduled";
import type { WatchlistEntry } from "../contracts/userContext";
import type { AssetChangeRow, AssetDelta, UpcomingEvent } from "../contracts/assets";
import type { WatchEvent } from "../contracts/watch";
import { claimIdForEdge } from "../components/map/MapList";
import { isMetricNode, nodeColorKey } from "../components/map/kinds";
import { entityLabel } from "../components/map/labels";

const DAY_MS = 24 * 60 * 60_000;
const MAX_NAME_CHIPS = 5;
const MAX_CHECKS = 3;
/** Rust sends at most five; the strip never grows past that. */
const MAX_UPCOMING = 5;
const MAX_QUESTIONS = 3;
/** A focus written as a sentence reads badly inside "What matters in … this week?". */
const MAX_FOCUS_IN_QUESTION = 48;

export type MorningInputs = {
  language: string;
  nowMs: number;
  /** Undefined on the first visit: the message speaks about where the Loupist starts. */
  lastSeenAtMs?: number;
  map?: KnowledgeMap | null;
  /** Growth events since `lastSeenAtMs` (all of them on the first visit). */
  events?: readonly GrowthEvent[] | null;
  briefing?: InboxItem | null;
  journal?: readonly JournalRecord[] | null;
  watchlist?: readonly WatchlistEntry[] | null;
  focus?: string;
  /** Followed assets' deltas since the last visit, from Rust `assetDelta`. */
  assets?: readonly AssetDelta[] | null;
  /** Watcher alerts Rust delivered to this message instead of a Thread message (`deliveredIn: "morning"`). */
  watchEvents?: readonly WatchEvent[] | null;
  /** The event strip, as Rust `upcomingEvents` ordered it; none in an as-of chat. */
  upcoming?: readonly UpcomingEvent[] | null;
};

/** `swatch` is the node's color on the Map, so a chip and its mark match. */
export type NameChip = { id: string; label: string; swatch: string };
export type ChangedBullet = { names: number; links: number; chips: NameChip[]; more: number };
export type CheckBullet = { journalId: string; claim: string; metric: string; byDate: string; byDateMs: number; daysLeft: number };
export type QuestionTemplate = "changed" | "linked" | "explains" | "check" | "conflict" | "briefing" | "focus" | "learned" | "general" | "markets" | "risks";
export type ReasonKey = "new" | "relation" | "source" | "check" | "conflict" | "watching";
export type MorningQuestion = {
  template: QuestionTemplate;
  params: Record<string, string>;
  reason?: { key: ReasonKey; params: Record<string, string> };
};
export type MorningLead =
  | { kind: "count"; count: number; sinceMs: number }
  /** The first visit with something to show: a count, since there is no "since". */
  | { kind: "toLook"; count: number }
  | { kind: "firstDay" }
  | { kind: "nothingNew"; sinceMs: number }
  | { kind: "empty" };
export type FollowedRow = { delta: AssetDelta; row: AssetChangeRow };
/** `alerts`: watcher events found by the first check of the day, shown here instead of a Thread message. */
export type FollowedBullet = { rows: FollowedRow[]; more: number; nextAssetId?: string; alerts: WatchEvent[] };
export type MorningMessage = {
  /** At most five events ahead, soonest first, shown above the lead. */
  upcoming: UpcomingEvent[];
  lead: MorningLead;
  changed?: ChangedBullet;
  followed?: FollowedBullet;
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

function changedBullet(events: GrowthEvent[], nodes: Map<string, KnowledgeMapNode>, claims: Map<string, KnowledgeMapEdge>, language: string): ChangedBullet | undefined {
  const names: KnowledgeMapNode[] = [];
  const seenNames = new Set<string>();
  const links = new Set<string>();
  for (const event of events) {
    if (!event.subjectId) continue;
    if (event.kind === "entity_added") {
      const node = nodes.get(event.subjectId);
      if (node && !seenNames.has(node.id)) { seenNames.add(node.id); names.push(node); }
    } else if (event.kind === "claim_added" && claims.has(event.subjectId)) links.add(event.subjectId);
  }
  if (!names.length && !links.size) return undefined;
  // Metrics recede on the Map; the chips show the companies, products and people first.
  const ordered = [...names.filter((node) => !isMetricNode(node)), ...names.filter((node) => isMetricNode(node))];
  const chips = ordered.slice(0, MAX_NAME_CHIPS).map((node) => ({ id: node.id, label: entityLabel(node, language), swatch: nodeColorKey(node) }));
  return { names: names.length, links: links.size, chips, more: Math.max(0, names.length - chips.length) };
}

/** Open dated checks, soonest first; `shown` ones are already a row of the event strip. */
function pendingChecks(journal: readonly JournalRecord[] | null | undefined, nowMs: number, shown: ReadonlySet<string> = new Set()): CheckBullet[] {
  return (journal ?? [])
    .flatMap((entry): CheckBullet[] => {
      const check = entry.check;
      // An undated line is read on its asset page, not reviewed on a day.
      const byDate = check?.byDate;
      if (entry.archived || !check || !byDate || entry.outcome || shown.has(entry.journalId)) return [];
      const byDateMs = localDayMs(byDate);
      if (!Number.isFinite(byDateMs)) return [];
      return [{ journalId: entry.journalId, claim: quotable(check.claim || entry.judgment), metric: quotable(check.metric || check.claim || entry.judgment), byDate, byDateMs, daysLeft: daysUntil(byDate, nowMs) }];
    })
    .sort((a, b) => a.byDateMs - b.byDateMs)
    .slice(0, MAX_CHECKS);
}

/** Template candidates in priority order (spec today-onboarding.md §3.3). */
function questionCandidates(input: MorningInputs, events: GrowthEvent[], nodes: Map<string, KnowledgeMapNode>, claims: Map<string, KnowledgeMapEdge>, checks: CheckBullet[], sinceLabel: string, withDiff: boolean) {
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
      if (node && (node.watched || node.held)) candidates.push({ template: "changed", subject: node.id, params: { name: name(node), date: sinceLabel }, reason: { key: "new", params: {} } });
    }
    // The user's own list of what they follow is not where a reported value came from.
    const sourceTitle = events.find((event) => event.kind === "source_retained" && !event.titleKey && event.summary.trim())?.summary.trim();
    for (const event of events) {
      if (event.kind !== "claim_added" || !event.subjectId) continue;
      const edge = claims.get(event.subjectId);
      const a = edge ? nodes.get(edge.source) : undefined;
      const b = edge ? nodes.get(edge.target) : undefined;
      if (!edge || !a || !b) continue;
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
  for (const entry of input.watchlist ?? []) candidates.push({ template: "briefing", subject: `symbol:${entry.symbol}:${entry.market}`, params: { symbol: entry.displayName?.trim() || entry.symbol }, reason: { key: "watching", params: {} } });
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

const MAX_FOLLOWED = 3;
const ROW_ORDER = ["thesis", "filing", "results", "earnings", "dividend", "price", "rates", "holdings", "fees", "positioning", "map", "source"];

/** Spec §2.4 over Rust's flags: a moved read, a crossed line, a filing or results row, a material price move. */
export function followedRank(delta: AssetDelta) {
  const has = (kinds: string[]) => delta.rows.some((row) => row.material && kinds.includes(row.kind));
  if (delta.moved && !delta.crossed) return 0;
  if (delta.crossed) return 1;
  if (has(["filing", "results"])) return 2;
  if (has(["price", "rates"])) return 3;
  return 4;
}

/** Up to three followed assets with their most material row since the last visit. */
export function followedBullet(deltas: readonly AssetDelta[] | null | undefined, watchEvents: readonly WatchEvent[] | null | undefined = []): FollowedBullet | undefined {
  const ordered = (deltas ?? [])
    .map((delta) => ({ delta, rows: delta.rows.filter((row) => row.material) }))
    .filter((item) => item.rows.length)
    .sort((a, b) => followedRank(a.delta) - followedRank(b.delta) || b.rows[0]!.factDate.localeCompare(a.rows[0]!.factDate));
  const alerts = (watchEvents ?? []).filter((event) => event.deliveredIn === "morning" && !event.seenAtMs).slice(0, MAX_FOLLOWED);
  if (!ordered.length) return alerts.length ? { rows: [], more: 0, alerts } : undefined;
  const rows = ordered.slice(0, MAX_FOLLOWED).map(({ delta, rows }) => ({
    delta,
    row: [...rows].sort((a, b) => ROW_ORDER.indexOf(a.kind) - ROW_ORDER.indexOf(b.kind) || b.factDate.localeCompare(a.factDate))[0]!,
  }));
  return { rows, more: Math.max(0, ordered.length - rows.length), nextAssetId: ordered[MAX_FOLLOWED]?.delta.assetId, alerts };
}

/**
 * The Loupist's morning message from data already on this computer: what changed
 * since the last visit, the latest briefing, Journal checks, and three
 * questions. Deterministic; no model call.
 */
export function morningMessage(input: MorningInputs, sinceLabel = ""): MorningMessage {
  const nodes = entityNodes(input.map);
  const claims = claimEdges(input.map);
  const events = relevantEvents(input.events);
  const firstDay = input.lastSeenAtMs === undefined;
  const changed = changedBullet(events, nodes, claims, input.language);
  const briefing = input.briefing && input.briefing.status === "succeeded" && (firstDay || (input.briefing.finishedAtMs ?? input.briefing.startedAtMs) > (input.lastSeenAtMs ?? 0) || input.briefing.readAtMs === null)
    ? input.briefing : undefined;
  const upcoming = (input.upcoming ?? []).slice(0, MAX_UPCOMING);
  // One line per event: a check due in the strip is not repeated as a bullet, nor counted as news.
  const checks = pendingChecks(input.journal, input.nowMs, new Set(upcoming.flatMap((event) => event.kind === "check" ? [event.journalId] : [])));
  // On a first visit there is no "since"; the asset pages show their own first look.
  const followed = firstDay ? undefined : followedBullet(input.assets, input.watchEvents);
  const withDiff = !firstDay && events.length > 0;
  const questions = pickQuestions(questionCandidates(input, events, nodes, claims, pendingChecks(input.journal, input.nowMs), sinceLabel, withDiff));
  const count = (changed ? 1 : 0) + (followed ? 1 : 0) + (briefing ? 1 : 0) + checks.length;
  const lead: MorningLead = !nodes.size && !(input.watchlist ?? []).length && !count && !upcoming.length
    ? { kind: "empty" }
    : firstDay ? (count ? { kind: "toLook", count } : { kind: "firstDay" })
      : count ? { kind: "count", count, sinceMs: input.lastSeenAtMs! }
        : { kind: "nothingNew", sinceMs: input.lastSeenAtMs! };
  return { upcoming, lead, changed, followed, briefing, checks, questions };
}

/** "2:07 AM" earlier today, "Tuesday" within the last six days, else "Oct 21". */
export function sinceDayOptions(sinceMs: number, nowMs: number): Intl.DateTimeFormatOptions {
  const days = startOfDay(nowMs) - startOfDay(sinceMs);
  if (days <= 0) return { year: undefined, month: undefined, day: undefined, hour: "numeric", minute: "2-digit" };
  return days <= 6 * DAY_MS
    ? { weekday: "long", year: undefined, month: undefined, day: undefined }
    : { year: undefined, month: "short", day: "numeric" };
}
