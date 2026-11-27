import type { TimePoint } from "../../contracts/knowledge";

export type HistoryEpisode = {
  title?: string;
  date?: string;
  recordId?: string;
  revisionId?: string;
  spanId?: string;
};

function objectOf(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : undefined;
}

function text(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function dated(value: unknown): string | undefined {
  const direct = text(value);
  if (direct) return direct;
  const row = objectOf(value);
  return text(row?.value);
}

/** One episode line from `knowledge_world_claim_history`. */
export function historyEpisode(value: unknown): HistoryEpisode | undefined {
  const row = objectOf(value);
  if (!row) return undefined;
  const args = objectOf(row.readArgs);
  const end = dated(row.end);
  return {
    title: text(row.sourceTitle),
    date: end ?? dated(row.publicationTime),
    recordId: text(args?.recordId),
    revisionId: text(args?.revisionId),
    spanId: text(args?.spanId),
  };
}

export type { TimePoint };
