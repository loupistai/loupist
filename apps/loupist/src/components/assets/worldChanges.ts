import type { TFunction } from "i18next";
import type { TimePoint } from "../../contracts/knowledge";
import { dateValue } from "../../knowledge/presentation";
import { validityLine } from "../map/claimFacts";

export type PassageTarget = { recordId: string; revisionId?: string; spanId?: string; sourceId?: string };

/** One row the change feed can show: a fact, a disagreement, or a Lens section. */
export type ChangeRow = {
  id: string;
  group: "ended" | "added" | "disagreement" | "lens";
  proposition: string;
  validAt: TimePoint | null;
  invalidAt: TimePoint | null;
  published?: string;
  sourceTitle?: string;
  passage?: PassageTarget;
  lensRecordId?: string;
};

export type ChangeGroups = {
  ended: ChangeRow[];
  added: ChangeRow[];
  disagreements: ChangeRow[];
  lens: ChangeRow[];
};

const GROUPS = ["ended", "added", "disagreement", "lens"] as const;

function objectOf(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : undefined;
}

function text(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function timePoint(value: unknown): TimePoint | null {
  const row = objectOf(value);
  if (!row || typeof row.precision !== "string") return null;
  return { precision: row.precision as TimePoint["precision"], value: text(row.value), approximate: row.approximate === true };
}

function published(value: unknown): string | undefined {
  const direct = text(value);
  if (direct) return direct;
  return text(objectOf(value)?.value);
}

/** A feed row from `changes`, or nothing when it is not one of the groups the page lists. */
export function parseChange(value: unknown, index: number): ChangeRow | undefined {
  const row = objectOf(value);
  if (!row) return undefined;
  const kind = text(row.kind);
  const group = kind === "ended" ? "ended" : kind === "added" ? "added" : kind === "conflict" || kind === "disputed" ? "disagreement" : undefined;
  const proposition = text(row.proposition);
  if (!group || !proposition) return undefined;
  const source = objectOf(row.source);
  const args = objectOf(source?.readArgs);
  const recordId = text(args?.recordId);
  return {
    id: text(row.claimId) ?? `${group}:${index}`,
    group,
    proposition,
    validAt: timePoint(row.validAt),
    invalidAt: timePoint(row.invalidAt),
    published: published(source?.publicationTime),
    sourceTitle: text(source?.title),
    passage: recordId ? { recordId, revisionId: text(args?.revisionId), spanId: text(args?.spanId) } : undefined,
  };
}

/** A Lens section the feed separated from the claim rows. */
export function parseLens(value: unknown, index: number): ChangeRow | undefined {
  const row = objectOf(value);
  if (!row) return undefined;
  const state = text(row.state);
  if (state !== "superseded" && state !== "contested") return undefined;
  const page = text(row.title) ?? "";
  const section = text(row.sectionKey) ?? "";
  if (!page && !section) return undefined;
  return {
    id: `${text(row.recordId) ?? "lens"}:${section}:${index}`,
    group: "lens",
    proposition: section || page,
    validAt: null,
    invalidAt: null,
    sourceTitle: page || undefined,
    lensRecordId: text(row.recordId),
  };
}

export function groupChanges(rows: readonly ChangeRow[]): ChangeGroups {
  const groups: ChangeGroups = { ended: [], added: [], disagreements: [], lens: [] };
  for (const row of rows) {
    if (row.group === "ended") groups.ended.push(row);
    else if (row.group === "added") groups.added.push(row);
    else if (row.group === "disagreement") groups.disagreements.push(row);
    else groups.lens.push(row);
  }
  return groups;
}

export function rowsFromFeed(changes: readonly unknown[], lens: readonly unknown[] = []): ChangeRow[] {
  return [...changes.flatMap((row, index) => { const parsed = parseChange(row, index); return parsed ? [parsed] : []; }), ...lens.flatMap((row, index) => { const parsed = parseLens(row, index); return parsed ? [parsed] : []; })];
}

/** Section 8's validity line, then the publication date when the feed has one. */
export function changeLine(row: ChangeRow, t: TFunction<"knowledge">, publishedLabel: (date: string) => string, now = Date.now()) {
  if (row.group === "lens") return row.sourceTitle && row.sourceTitle !== row.proposition ? `${row.sourceTitle} · ${row.proposition}` : row.proposition;
  const validity = validityLine({ validAt: row.validAt, invalidAt: row.invalidAt }, t, now);
  return row.published ? `${validity} · ${publishedLabel(dateValue(row.published))}` : validity;
}

export const groupOrder = GROUPS;
