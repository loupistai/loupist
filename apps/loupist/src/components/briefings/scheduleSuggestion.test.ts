import type { GrowthEvent } from "../../contracts/knowledge";
import { scheduleSuggestion, SUGGESTION_SNOOZE_MS } from "./scheduleSuggestion";

const NOW = Date.parse("2026-10-31T00:00:00Z");
const disputed = (subjectId: string, atMs: number): GrowthEvent => ({ eventId: `d-${subjectId}-${atMs}`, loupistId: "g", atMs, kind: "claim_disputed", subjectId, recordId: "w", originChatId: null, originMessageId: null, actor: "loupist", summary: "", beforeSummary: null, afterSummary: null });
const follow = (symbol: string, addedAtMs: number) => ({ symbol, market: "KRX", displayName: null, addedAtMs });

it("suggests a special briefing for conflicts since the last briefing, counting each claim once", () => {
  const events = [disputed("c1", NOW - 1000), disputed("c1", NOW - 500), disputed("c2", NOW - 200), disputed("old", NOW - 10 * 86_400_000)];
  expect(scheduleSuggestion({ nowMs: NOW, lastBriefingAtMs: NOW - 86_400_000, events, watchlist: [], dismissed: {} })).toEqual({ kind: "conflict", count: 2, names: [] });
});

it("falls back to newly followed assets, needs three of them and a previous briefing", () => {
  const watchlist = [follow("A", NOW - 100), follow("B", NOW - 200), follow("C", NOW - 300), follow("D", NOW - 9 * 86_400_000)];
  expect(scheduleSuggestion({ nowMs: NOW, lastBriefingAtMs: NOW - 86_400_000, events: [], watchlist, dismissed: {} })).toEqual({ kind: "watchlist", count: 3, names: ["A", "B", "C"] });
  expect(scheduleSuggestion({ nowMs: NOW, lastBriefingAtMs: NOW - 86_400_000, events: [], watchlist: watchlist.slice(0, 2), dismissed: {} })).toBeNull();
  expect(scheduleSuggestion({ nowMs: NOW, events: [], watchlist, dismissed: {} })).toBeNull();
});

it("keeps a dismissed kind away for two weeks", () => {
  const events = [disputed("c1", NOW - 1000)];
  expect(scheduleSuggestion({ nowMs: NOW, lastBriefingAtMs: NOW - 86_400_000, events, watchlist: [], dismissed: { conflict: NOW + SUGGESTION_SNOOZE_MS } })).toBeNull();
  expect(scheduleSuggestion({ nowMs: NOW + SUGGESTION_SNOOZE_MS + 1, lastBriefingAtMs: NOW - 86_400_000, events, watchlist: [], dismissed: { conflict: NOW + SUGGESTION_SNOOZE_MS } })).toEqual({ kind: "conflict", count: 1, names: [] });
});

it("names what a suggestion is about: the asset of each conflict, or the newly followed assets", () => {
  const events = [disputed("c1", NOW - 1000), disputed("c2", NOW - 500), disputed("c3", NOW - 200)];
  const nameOf = (event: GrowthEvent) => ({ c1: "Hanul Precision", c2: "Hanul Precision", c3: "Morrow Components" } as Record<string, string>)[event.subjectId ?? ""];
  expect(scheduleSuggestion({ nowMs: NOW, lastBriefingAtMs: NOW - 86_400_000, events, watchlist: [], dismissed: {}, nameOf })).toEqual({ kind: "conflict", count: 3, names: ["Hanul Precision", "Morrow Components"] });
  const watchlist = [{ ...follow("005930", NOW - 100), displayName: "Samsung Electronics" }, follow("000660", NOW - 200), follow("AAPL", NOW - 300), follow("MSFT", NOW - 400)];
  expect(scheduleSuggestion({ nowMs: NOW, lastBriefingAtMs: NOW - 86_400_000, events: [], watchlist, dismissed: {} })?.names).toEqual(["Samsung Electronics", "000660", "AAPL"]);
});
