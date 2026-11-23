import { useEffect, useState } from "react";
import type { TFunction } from "i18next";
import type { GrowthEvent, KnowledgeMap, KnowledgeMapEdge, KnowledgeMapNode, WorldGrowthSource } from "../../contracts/knowledge";
import type { LoupistBridge } from "../../types";
import { readWithWorldRetry } from "../../knowledge/readWithWorldRetry";
import { commandErrorCode } from "../../lib/errors";
import { relevantEvents } from "../../chat/morningMessage";
import { claimIdForEdge, entityLabel, oneName } from "../../knowledge/mapItems";
import { nodeCategory } from "../../knowledge/mapKinds";
import { relationName } from "./relations";

const DAY_MS = 86_400_000;
/** A Source with more rows than this in the range shown is one expandable group. */
const SOURCE_GROUP_THRESHOLD = 3;

/** What changed in the Loupist's knowledge, in the rows the What's new drawer lists. A `fact` is a value reported about one name. */
export type WhatsNewKind = "name" | "relationship" | "fact" | "conflict" | "method" | "principle" | "subject" | "idea";
export type WhatsNewRow = {
  id: string;
  kind: WhatsNewKind;
  atMs: number;
  label: string;
  /** Category swatch for a name, or for the name a fact is about. */
  category?: string;
  node?: KnowledgeMapNode;
  edge?: KnowledgeMapEdge;
  claimId?: string;
  /** The World name a name row adds, which a bounded overview may leave out. */
  nameId?: string;
  recordId?: string;
  publicationKey: string;
  source?: WorldGrowthSource;
  /** New facts folded into a new name's row in the same publication. */
  facts?: number;
  factSubjectIds?: string[];
  /** Title of a Source retained in the same reply, for historical events. */
  origin?: string;
  /** A claim's names in the interface language and its relation, when they are on the Map. A value reported about one name has no `b`. */
  link?: { a: string; b?: string; relation?: string };
  /** Identical changes this row stands for: the same kind, ends, relation and Source in one publication. */
  count: number;
  /** Archived or withdrawn since: the row stays but opens nothing. */
  gone: boolean;
};

/** A changed claim in words: "A → B (relation)", or "A (relation)" for a value reported about one name. */
export function whatsNewLinkText(link: NonNullable<WhatsNewRow["link"]>, t: TFunction<"knowledge">) {
  const relation = link.relation ? relationName(link.relation, t) : undefined;
  if (link.b === undefined) return relation ? t("map.newLinkOne", { a: link.a, relation }) : link.a;
  return relation ? t("map.newLink", { a: link.a, b: link.b, relation }) : t("map.newLinkPlain", { a: link.a, b: link.b });
}

/** Since-choices offered in the drawer. `lastVisit` exists only after a counted visit. */
export type WhatsNewSince = { kind: "lastVisit" } | { kind: "days"; days: 7 | 30 } | { kind: "date"; dayMs: number };

export function sinceMs(choice: WhatsNewSince, lastSeenAtMs: number | undefined, nowMs: number) {
  if (choice.kind === "lastVisit") return lastSeenAtMs;
  if (choice.kind === "days") return nowMs - choice.days * DAY_MS;
  return choice.dayMs;
}

function lensKind(event: GrowthEvent, node: KnowledgeMapNode | undefined): "method" | "principle" | "subject" | "idea" | undefined {
  if (node?.kind === "lens_method") return "method";
  if (node?.kind === "lens_principle") return "principle";
  if (node?.kind === "lens_subject") return "subject";
  if (node?.kind === "lens_idea") return "idea";
  if (node) return undefined;
  // Unlinked Lens entries may be missing from a bounded Map; the event origin still tells them apart.
  if (typeof event.origin === "string") return "principle";
  if (event.origin && "kind" in event.origin && event.origin.kind === "learning_review") return "method";
  return undefined;
}

/**
 * World's one rule for "new" in a range that starts after `sinceMs` (the growth read's
 * exclusive start): first seen after it. What's new and World's weekly count share it.
 */
export const firstSeenAfter = (firstSeenAtMs: number | null | undefined, sinceMs: number | undefined) =>
  sinceMs === undefined || firstSeenAtMs == null || firstSeenAtMs > sinceMs;

/**
 * Rows newest first: names, relationships, reported values, conflicting
 * evidence, and Lens methods or principles. One row per subject; schema and
 * truncation markers are left out, as in the morning message. A claim whose
 * ends are one name is a value reported about it, not a relationship. Changes
 * one publication made that read the same (kind, ends, relation and Source)
 * are one row; `count` says how many.
 *
 * A name or claim is new in the range when it was first seen after `sinceMs`
 * (its first appearance in World, not just in these events) and exists now. Its
 * row belongs to the change that first added it in the range, so a later Source
 * adding facts about it shows those facts, not the name again. One that has
 * left World stays as a dimmed row and is not new. A bounded overview leaves
 * some names and claims out: those are taken to exist, first seen at their
 * first change in the range.
 */
export function whatsNewRows(events: readonly GrowthEvent[] | null | undefined, map: KnowledgeMap | null | undefined, language: string, sinceMs?: number): WhatsNewRow[] {
  const nodes = new Map((map?.nodes ?? []).map((node) => [node.id, node]));
  const lenses = new Map((map?.nodes ?? []).filter((node) => node.recordId && node.kind !== "entity").map((node) => [node.recordId!, node]));
  const claims = new Map<string, KnowledgeMapEdge>();
  for (const edge of map?.edges ?? []) if ((edge.kind === "claim" || edge.kind === "fact") && !claims.has(claimIdForEdge(edge))) claims.set(claimIdForEdge(edge), edge);
  const namesBounded = !!map?.counts?.entity?.truncated;
  const claimsBounded = !!(map?.counts?.claim?.truncated || map?.counts?.fact?.truncated);
  const ordered = relevantEvents(events);
  // The first add of each subject in the range carries it; a later add of the same subject is not news.
  const firstAdd = new Map<string, GrowthEvent>();
  for (let index = ordered.length - 1; index >= 0; index -= 1) {
    const event = ordered[index]!;
    if ((event.kind === "entity_added" || event.kind === "claim_added") && event.subjectId && !firstAdd.has(`${event.kind}:${event.subjectId}`)) firstAdd.set(`${event.kind}:${event.subjectId}`, event);
  }
  const sources = new Map<string, string>();
  for (const event of ordered) if (event.kind === "source_retained" && event.originMessageId && event.summary.trim() && !sources.has(event.originMessageId)) sources.set(event.originMessageId, event.summary.trim());
  const rows: WhatsNewRow[] = [];
  const nameKey = (publicationKey: string, id: string) => JSON.stringify([publicationKey, id]);
  const names = new Map<string, WhatsNewRow>();
  const seen = new Set<string>();
  const alike = new Map<string, WhatsNewRow>();
  for (const event of ordered) {
    const origin = event.originMessageId ? sources.get(event.originMessageId) : undefined;
    const source = event.publication?.source;
    const publicationKey = JSON.stringify(event.publication
      ? [event.publication.revisionId, source?.recordId, source?.revisionId]
      : [event.recordId, event.atMs, event.originMessageId]);
    const base = { id: event.eventId, atMs: event.atMs, origin, count: 1, publicationKey, source };
    if (event.kind === "entity_added" && event.subjectId) {
      if (firstAdd.get(`entity_added:${event.subjectId}`) !== event) continue;
      const node = nodes.get(event.subjectId);
      // A name World already had before the range came back, so it is not new.
      if (node && !firstSeenAfter(node.firstSeenAtMs, sinceMs)) continue;
      const gone = node ? node.kind !== "entity" || !!node.archived : !namesBounded;
      const row: WhatsNewRow = { ...base, kind: "name", label: node ? entityLabel(node, language) : event.summary, category: node ? nodeCategory(node) : "other", node: gone ? undefined : node, nameId: event.subjectId, gone };
      names.set(nameKey(publicationKey, event.subjectId), row);
      rows.push(row);
    } else if ((event.kind === "claim_added" || event.kind === "claim_disputed") && event.subjectId) {
      const changed = event.kind === "claim_added" ? "added" : "disputed";
      if (changed === "added" ? firstAdd.get(`claim_added:${event.subjectId}`) !== event : seen.has(`disputed:${event.subjectId}`)) continue;
      seen.add(`${changed}:${event.subjectId}`);
      const edge = claims.get(event.subjectId);
      if (changed === "added" && edge && !firstSeenAfter(edge.firstSeenAtMs, sinceMs)) continue;
      // The stored proposition is English; the row speaks the interface language through the names and relation.
      const factSubjectId = event.publication?.factSubjectId;
      const a = nodes.get(edge?.source ?? factSubjectId ?? "");
      const b = edge ? nodes.get(edge.target) : undefined;
      const single = !!factSubjectId || edge?.kind === "fact" || (!!a && !!b && oneName(a, b, language));
      const kind = changed === "disputed" ? "conflict" : single ? "fact" : "relationship";
      const link = a && (single || b) ? { a: entityLabel(a, language), b: single || !b ? undefined : entityLabel(b, language), relation: edge?.relation } : undefined;
      // Rows that would read the same, from one publication, are one row.
      const ends = factSubjectId ?? (edge ? single ? edge.source : `${edge.source}>${edge.target}` : event.summary);
      const key = JSON.stringify([kind, ends, edge?.relation ?? "", publicationKey]);
      const same = alike.get(key);
      if (same) { same.count += 1; continue; }
      const factSubjectIds = single ? [...new Set([factSubjectId, edge?.source, edge?.target].filter((id): id is string => !!id))] : undefined;
      const gone = (!edge && !claimsBounded && !(kind === "fact" && a && !a.archived)) || (kind !== "conflict" && edge?.status === "withdrawn");
      const row: WhatsNewRow = { ...base, kind, label: event.summary, link, claimId: event.subjectId, edge, gone, factSubjectIds,
        ...(single && a ? { node: gone || a.archived ? undefined : a, category: nodeCategory(a) } : {}) };
      alike.set(key, row);
      rows.push(row);
    } else if (event.kind === "lens_created" && event.recordId) {
      const node = lenses.get(event.recordId);
      const kind = lensKind(event, node);
      if (!kind || seen.has(`lens:${event.recordId}`)) continue;
      seen.add(`lens:${event.recordId}`);
      rows.push({ ...base, kind, label: node?.label ?? event.summary, recordId: event.recordId, node, gone: !!node?.archived });
    }
  }
  return rows.filter(row => {
    const name = row.kind === "fact" ? row.factSubjectIds?.map(id => names.get(nameKey(row.publicationKey, id))).find(Boolean) : undefined;
    if (!name) return true;
    name.facts = (name.facts ?? 0) + row.count;
    return false;
  });
}

export type WhatsNewSourceGroup = {
  kind: "source";
  id: string;
  atMs: number;
  source: WorldGrowthSource;
  rows: WhatsNewRow[];
  names: number;
  facts: number;
};
export type WhatsNewEntry = WhatsNewRow | WhatsNewSourceGroup;

const sourceKey = (source: WorldGrowthSource) => JSON.stringify([source.recordId, source.sourceId]);

/**
 * Collapse each Source into one group across the publications in `rows`
 * (newest first): a Source processed in several chunks is one group whose
 * counts add up, dated by its latest change, with its rows newest first.
 */
export function whatsNewEntries(rows: readonly WhatsNewRow[], language: string): WhatsNewEntry[] {
  const bySource = new Map<string, WhatsNewRow[]>();
  for (const row of rows) {
    if (!row.source) continue;
    const key = sourceKey(row.source);
    const changes = bySource.get(key);
    if (changes) changes.push(row);
    else bySource.set(key, [row]);
  }
  const order = new Intl.Collator(language);
  return rows.flatMap<WhatsNewEntry>(row => {
    const source = row.source;
    const key = source && sourceKey(source);
    const changes = key ? bySource.get(key) : undefined;
    if (!source || !changes || changes.length <= SOURCE_GROUP_THRESHOLD) return [row];
    if (changes[0] !== row) return [];
    return [{
      kind: "source", id: `source:${key}`, atMs: row.atMs, source,
      rows: [...changes].sort((a, b) => b.atMs - a.atMs
        || Number(b.kind === "name") - Number(a.kind === "name")
        || order.compare(a.link?.a ?? a.label, b.link?.a ?? b.label)),
      names: changes.filter(item => item.kind === "name" && !item.gone).length,
      facts: changes.reduce((total, item) => total + (item.kind === "fact" ? item.count : item.facts ?? 0), 0),
    }];
  });
}

/**
 * The drawer's totals for its range, over every row (a Source group adds up its
 * rows): new names, new relationships and new disagreements, each counted once.
 * A row that has left World is listed dimmed and not counted.
 */
export function whatsNewTotals(rows: readonly WhatsNewRow[]) {
  const sum = (kind: WhatsNewKind) => rows.reduce((total, row) => total + (row.kind === kind && !row.gone ? row.count : 0), 0);
  return { names: sum("name"), links: sum("relationship"), conflicts: sum("conflict") };
}

const startOfDay = (ms: number) => { const day = new Date(ms); day.setHours(0, 0, 0, 0); return day.getTime(); };

/** Rows grouped by local calendar day, newest day first. `daysAgo` is 0 for today. */
export function groupByDay<T extends { atMs: number }>(rows: readonly T[], nowMs: number) {
  const today = startOfDay(nowMs);
  const groups: { dayMs: number; daysAgo: number; rows: T[] }[] = [];
  for (const row of rows) {
    const dayMs = startOfDay(row.atMs);
    const group = groups.at(-1);
    if (group?.dayMs === dayMs) group.rows.push(row);
    else groups.push({ dayMs, daysAgo: Math.round((today - dayMs) / DAY_MS), rows: [row] });
  }
  return groups;
}

export type GrowthLoad = { status: "loading" | "done" | "failed"; events: GrowthEvent[] };

/** Growth events since `since` (all of them when undefined); `enabled` false skips the read. */
export function useGrowthSince(bridge: LoupistBridge, loupistId: string, since: number | undefined, enabled: boolean, untilMs?: number, revisionId?: string | null, advance?: () => void): GrowthLoad & { retry: () => void } {
  const key = `${loupistId}:${since ?? "all"}`;
  const [attempt, setAttempt] = useState(0);
  const [state, setState] = useState<GrowthLoad & { key: string }>({ key, status: "loading", events: [] });
  useEffect(() => {
    if (!enabled) return;
    let current = true;
    setState(previous => previous.key === key && previous.status === "done" ? previous : { key, status: "loading", events: [] });
    void readWithWorldRetry(async () => {
      const sinceMs = since === undefined ? undefined : Math.max(0, Math.floor(since)) + 1;
      if (sinceMs !== undefined && untilMs !== undefined && sinceMs > untilMs) return [];
      const all: GrowthEvent[] = [];
      let cursor: string | undefined;
      // A publication can start on one page and end on another. Publish the
      // list once all pages for this pinned revision have been collected.
      do {
        const result = await bridge.knowledgeGrowthEvents({ loupistId, sinceMs, untilMs, worldRevisionId: revisionId ?? undefined, limit: 5000, cursor });
        if (!current) return [];
        all.push(...result.events);
        const next = result.nextCursor ?? undefined;
        if (next !== undefined && next === cursor) throw new Error("Growth event cursor did not advance");
        cursor = next;
      } while (cursor);
      return all;
    }, () => current).then((events) => { if (current) setState({ key, status: "done", events }); })
      .catch(cause => {
        if (!current) return;
        if (advance && commandErrorCode(cause) === "conflict") advance();
        else setState({ key, status: "failed", events: [] });
      });
    return () => { current = false; };
  }, [bridge, loupistId, key, since, enabled, attempt, untilMs, revisionId, advance]);
  const load = state.key === key ? state : { status: "loading" as const, events: [] };
  return { status: load.status, events: load.events, retry: () => setAttempt((value) => value + 1) };
}
