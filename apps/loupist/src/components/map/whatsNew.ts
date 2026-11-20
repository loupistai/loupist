import { useEffect, useState } from "react";
import type { TFunction } from "i18next";
import type { GrowthEvent, KnowledgeMap, KnowledgeMapEdge, KnowledgeMapNode } from "../../contracts/knowledge";
import type { LoupistBridge } from "../../types";
import { relevantEvents } from "../../chat/morningMessage";
import { claimIdForEdge, entityLabel, oneName } from "../../knowledge/mapItems";
import { nodeCategory } from "../../knowledge/mapKinds";
import { relationName } from "./relations";

const DAY_MS = 86_400_000;
const MAX_EVENT_PAGES = 4;

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
  recordId?: string;
  /** Title of a source retained in the same reply, when there is one. */
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
 * Rows newest first: names, relationships, reported values, conflicting
 * evidence, and Lens methods or principles. One row per subject; schema and
 * truncation markers are left out, as in the morning message. A claim whose
 * ends are one name is a value reported about it, not a relationship. Changes
 * one publication made that read the same (kind, ends, relation and Source)
 * are one row; `count` says how many.
 */
export function whatsNewRows(events: readonly GrowthEvent[] | null | undefined, map: KnowledgeMap | null | undefined, language: string): WhatsNewRow[] {
  const nodes = new Map((map?.nodes ?? []).map((node) => [node.id, node]));
  const lenses = new Map((map?.nodes ?? []).filter((node) => node.recordId && node.kind !== "entity").map((node) => [node.recordId!, node]));
  const claims = new Map<string, KnowledgeMapEdge>();
  for (const edge of map?.edges ?? []) if (edge.kind === "claim" && !claims.has(claimIdForEdge(edge))) claims.set(claimIdForEdge(edge), edge);
  const ordered = relevantEvents(events);
  const sources = new Map<string, string>();
  for (const event of ordered) if (event.kind === "source_retained" && event.originMessageId && event.summary.trim() && !sources.has(event.originMessageId)) sources.set(event.originMessageId, event.summary.trim());
  const rows: WhatsNewRow[] = [];
  const seen = new Set<string>();
  const alike = new Map<string, WhatsNewRow>();
  for (const event of ordered) {
    const origin = event.originMessageId ? sources.get(event.originMessageId) : undefined;
    const base = { id: event.eventId, atMs: event.atMs, origin, count: 1 };
    if (event.kind === "entity_added" && event.subjectId) {
      if (seen.has(`name:${event.subjectId}`)) continue;
      seen.add(`name:${event.subjectId}`);
      const node = nodes.get(event.subjectId);
      const gone = !node || node.kind !== "entity" || !!node.archived;
      rows.push({ ...base, kind: "name", label: node ? entityLabel(node, language) : event.summary, category: node ? nodeCategory(node) : "other", node: gone ? undefined : node, gone });
    } else if ((event.kind === "claim_added" || event.kind === "claim_disputed") && event.subjectId) {
      const changed = event.kind === "claim_added" ? "added" : "disputed";
      if (seen.has(`${changed}:${event.subjectId}`)) continue;
      seen.add(`${changed}:${event.subjectId}`);
      const edge = claims.get(event.subjectId);
      // The stored proposition is English; the row speaks the interface language through the names and relation.
      const a = edge ? nodes.get(edge.source) : undefined, b = edge ? nodes.get(edge.target) : undefined;
      const single = !!a && !!b && oneName(a, b, language);
      const kind = changed === "disputed" ? "conflict" : single ? "fact" : "relationship";
      const link = a && b ? { a: entityLabel(a, language), b: single ? undefined : entityLabel(b, language), relation: edge?.relation } : undefined;
      // Rows that would read the same, from one publication, are one row. A fact's ends are its one subject.
      const ends = !edge ? event.summary : single ? edge.source : `${edge.source}>${edge.target}`;
      const key = [kind, ends, edge?.relation ?? "", event.originMessageId ?? "", event.atMs].join("\u0000");
      const same = alike.get(key);
      if (same) { same.count += 1; continue; }
      const gone = !edge || (kind !== "conflict" && edge.status === "withdrawn");
      const row: WhatsNewRow = { ...base, kind, label: event.summary, link, claimId: event.subjectId, edge, gone,
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
  return rows;
}

export function whatsNewCounts(rows: readonly WhatsNewRow[]) {
  return {
    names: rows.filter((row) => row.kind === "name").length,
    links: rows.filter((row) => row.kind === "relationship").length,
    conflicts: rows.filter((row) => row.kind === "conflict").length,
  };
}

const startOfDay = (ms: number) => { const day = new Date(ms); day.setHours(0, 0, 0, 0); return day.getTime(); };

/** Rows grouped by local calendar day, newest day first. `daysAgo` is 0 for today. */
export function groupByDay(rows: readonly WhatsNewRow[], nowMs: number) {
  const today = startOfDay(nowMs);
  const groups: { dayMs: number; daysAgo: number; rows: WhatsNewRow[] }[] = [];
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
export function useGrowthSince(bridge: LoupistBridge, loupistId: string, since: number | undefined, enabled: boolean, refreshKey?: unknown): GrowthLoad & { retry: () => void } {
  const key = `${loupistId}:${since ?? "all"}`;
  const [attempt, setAttempt] = useState(0);
  const [state, setState] = useState<GrowthLoad & { key: string }>({ key, status: "loading", events: [] });
  useEffect(() => {
    if (!enabled) return;
    let current = true;
    setState({ key, status: "loading", events: [] });
    void (async () => {
      const all: GrowthEvent[] = [];
      let cursor: string | undefined;
      for (let page = 0; page < MAX_EVENT_PAGES; page += 1) {
        const result = await bridge.knowledgeGrowthEvents({ loupistId, sinceMs: since === undefined ? undefined : Math.max(0, Math.floor(since)) + 1, limit: 5000, cursor });
        all.push(...result.events);
        cursor = result.nextCursor ?? undefined;
        if (!cursor) break;
      }
      return all;
    })().then((events) => { if (current) setState({ key, status: "done", events }); })
      .catch(() => { if (current) setState({ key, status: "failed", events: [] }); });
    return () => { current = false; };
  }, [bridge, loupistId, key, since, enabled, attempt, refreshKey]);
  const load = state.key === key ? state : { status: "loading" as const, events: [] };
  return { status: load.status, events: load.events, retry: () => setAttempt((value) => value + 1) };
}
