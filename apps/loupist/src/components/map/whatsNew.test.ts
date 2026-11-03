import type { GrowthEvent, KnowledgeMap } from "../../contracts/knowledge";
import { groupByDay, sinceMs, whatsNewCounts, whatsNewRows } from "./whatsNew";

const NOW = new Date(2026, 9, 31, 12).getTime();
const HOUR = 3_600_000;
const event = (kind: GrowthEvent["kind"], subjectId: string | null, atMs: number, extra: Partial<GrowthEvent> = {}): GrowthEvent => ({
  eventId: `${kind}:${subjectId}:${atMs}`, loupistId: "loupist", atMs, kind, subjectId, recordId: "world", originChatId: null, originMessageId: null,
  actor: "loupist", summary: subjectId ?? "", beforeSummary: null, afterSummary: null, ...extra,
});
const map = {
  nodes: [
    { id: "hanul", label: "Hanul Precision", localNames: ["한울정밀"], kind: "entity", entityKind: "Organization", degree: 2, watched: true, held: false },
    { id: "margin", label: "Operating margin", kind: "entity", entityKind: "Metric", degree: 1, watched: false, held: false },
    { id: "old", label: "Old name", kind: "entity", entityKind: "Organization", degree: 0, watched: false, held: false, archived: true },
    { id: "lens:m", recordId: "lens-method", label: "Test capacity costs", kind: "lens_method", degree: 0, watched: false, held: false },
  ],
  edges: [{ id: "c1", kind: "claim", source: "hanul", target: "margin", status: "disputed" }],
  counts: {}, unmatchedHoldings: [], unmatchedWatchlist: [], worldRevisionId: "rev", generatedAtMs: NOW,
} as unknown as KnowledgeMap;

it("turns growth events into one row per subject, newest first, without schema or truncation markers", () => {
  const rows = whatsNewRows([
    event("entity_added", "hanul", NOW - 3 * HOUR, { originMessageId: "answer" }),
    event("entity_added", "hanul", NOW - 4 * HOUR),
    event("source_retained", null, NOW - 3 * HOUR, { originMessageId: "answer", summary: "Q3 operating review" }),
    event("claim_added", "c1", NOW - 2 * HOUR, { summary: "Hanul's margin fell" }),
    event("claim_disputed", "c1", NOW - HOUR, { summary: "Inventory days 54 vs 48" }),
    event("lens_created", null, NOW - 30 * HOUR, { recordId: "lens-method", summary: "Test capacity costs" }),
    event("lens_created", null, NOW - 31 * HOUR, { recordId: "lens-principle", origin: "chat", summary: "Never chase momentum" }),
    event("entity_added", "old", NOW - 50 * HOUR, { summary: "Old name" }),
    event("schema_extended", null, NOW),
    event("growth_truncated", null, NOW),
  ], map, "ko");
  expect(rows.map((row) => [row.kind, row.label, row.gone])).toEqual([
    ["conflict", "Inventory days 54 vs 48", false],
    ["relationship", "Hanul's margin fell", false],
    ["name", "한울정밀", false],
    ["method", "Test capacity costs", false],
    ["principle", "Never chase momentum", false],
    ["name", "Old name", true],
  ]);
  expect(rows.find((row) => row.kind === "name")?.origin).toBe("Q3 operating review");
  // The stored proposition is English; relationship and conflict rows speak through the names on the Map.
  expect(rows.find((row) => row.kind === "relationship")?.link).toEqual({ a: "한울정밀", b: "Operating margin", relation: undefined });
  expect(rows.find((row) => row.kind === "conflict")?.link?.a).toBe("한울정밀");
  expect(whatsNewCounts(rows)).toEqual({ names: 2, links: 1, conflicts: 1 });
});

it("groups rows by local day and resolves since-choices", () => {
  const rows = whatsNewRows([event("entity_added", "hanul", NOW - HOUR), event("entity_added", "margin", NOW - 26 * HOUR)], map, "en");
  expect(groupByDay(rows, NOW).map((group) => [group.daysAgo, group.rows.length])).toEqual([[0, 1], [1, 1]]);
  expect(sinceMs({ kind: "lastVisit" }, 42, NOW)).toBe(42);
  expect(sinceMs({ kind: "days", days: 7 }, 42, NOW)).toBe(NOW - 7 * 24 * HOUR);
  expect(sinceMs({ kind: "date", dayMs: 7 }, 42, NOW)).toBe(7);
});
