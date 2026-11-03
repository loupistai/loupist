import type { GrowthEvent, JournalRecord, KnowledgeMap, KnowledgeMapEdge, KnowledgeMapNode } from "../contracts/knowledge";
import type { InboxItem } from "../contracts/scheduled";
import type { AssetChangeRow, AssetDelta } from "../contracts/assets";
import { nodeColorKey } from "../components/map/kinds";
import { daysUntil, firstSentence, followedBullet, morningMessage, type MorningInputs } from "./morningMessage";

const assetRow = (kind: AssetChangeRow["kind"], factDate: string, material = true): AssetChangeRow =>
  ({ id: `${kind}:${factDate}`, kind, factDate, recordedAtMs: 0, facts: {}, evidence: [{ tool: "dart_filing_search", title: "OpenDART", url: "https://dart.fss.or.kr/" }], new: true, material });
const delta = (assetId: string, rows: AssetChangeRow[], flags: Partial<AssetDelta> = {}): AssetDelta =>
  ({ assetId, symbol: assetId.toUpperCase(), market: "KRX", name: assetId, class: "equity", rows, moved: false, crossed: false, ...flags });

describe("what you follow in the morning message", () => {
  it("orders assets by a moved read, a crossed line, a filing or results row, then a material price move", () => {
    const bullet = followedBullet([
      delta("price-only", [assetRow("price", "2026-10-30")]),
      delta("filing", [assetRow("price", "2026-10-30"), assetRow("filing", "2026-10-29")]),
      delta("crossed", [assetRow("thesis", "2026-10-28")], { moved: true, crossed: true }),
      delta("quiet", [assetRow("price", "2026-10-30", false)]),
      delta("moved", [assetRow("results", "2026-10-29"), assetRow("thesis", "2026-10-29")], { moved: true }),
    ])!;
    expect(bullet.rows.map(item => item.delta.assetId)).toEqual(["moved", "crossed", "filing"]);
    // Each asset shows its most material row: the move, then the filing before the price.
    expect(bullet.rows.map(item => item.row.kind)).toEqual(["thesis", "thesis", "filing"]);
    expect(bullet).toMatchObject({ more: 1, nextAssetId: "price-only" });
  });

  it("is omitted on a first visit and when nothing material changed, and counts toward the lead", () => {
    const quiet = [delta("quiet", [assetRow("price", "2026-10-30", false)])];
    expect(followedBullet(quiet)).toBeUndefined();
    const moved = [delta("moved", [assetRow("thesis", "2026-10-30")], { moved: true })];
    expect(morningMessage({ ...base, assets: moved }).lead).toEqual({ kind: "count", count: 1, sinceMs: LAST_SEEN });
    expect(morningMessage({ ...base, lastSeenAtMs: undefined, assets: moved }).followed).toBeUndefined();
  });
});

const NOW = new Date(2026, 9, 31, 9, 0).getTime();
const HOUR = 3_600_000;
const LAST_SEEN = NOW - 3 * 24 * HOUR;

const node = (id: string, label: string, extra: Partial<KnowledgeMapNode> = {}): KnowledgeMapNode =>
  ({ id, kind: "entity", label, entityKind: "Organization", degree: 1, watched: false, held: false, ...extra });
const claim = (id: string, source: string, target: string): KnowledgeMapEdge => ({ id, kind: "claim", source, target, relation: "supplies" });
const map = (nodes: KnowledgeMapNode[], edges: KnowledgeMapEdge[] = []) => ({ nodes, edges } as unknown as KnowledgeMap);
let sequence = 0;
const event = (kind: GrowthEvent["kind"], subjectId: string | null, atMs = LAST_SEEN + HOUR, summary = ""): GrowthEvent => ({
  eventId: `event-${sequence++}`, loupistId: "loupist", atMs, kind, recordId: null, subjectId, originChatId: null, originMessageId: null,
  actor: "loupist", summary, beforeSummary: null, afterSummary: null,
});
const check = (journalId: string, byDate: string, extra: Partial<JournalRecord> = {}): JournalRecord => ({
  journalId, researchId: journalId, loupistId: "loupist", judgment: `Judgment ${journalId}`, reasoning: "", entities: [],
  check: { claim: `Claim ${journalId}`, metric: `metric ${journalId}`, direction: "below", threshold: 50, byDate },
  createdAtMs: 0, updatedAtMs: 0, archived: false, inputs: { sources: [], worlds: [] } as unknown as JournalRecord["inputs"], inputDigest: "",
  ...extra,
});
const briefing = (extra: Partial<InboxItem> = {}): InboxItem => ({
  runId: "run-1", taskId: "task", kind: "briefing", title: "Morning briefing", status: "succeeded", summary: "Margins fell. More follows.",
  startedAtMs: NOW - HOUR, finishedAtMs: NOW - HOUR + 1000, readAtMs: null, threadId: null, messageId: null, skippedTools: [],
  ...extra,
} as InboxItem);

const hanul = node("hanul", "Hanul Precision", { watched: true });
const morrow = node("morrow", "Morrow Components");
const margin = node("margin", "Operating margin", { entityKind: "Metric" });
const base: MorningInputs = { language: "en", nowMs: NOW, lastSeenAtMs: LAST_SEEN, map: map([hanul, morrow, margin], [claim("c1", "morrow", "hanul"), claim("c2", "hanul", "margin")]) };

describe("morning message", () => {
  it("summarizes what changed since the last visit with companies before metrics", () => {
    const message = morningMessage({ ...base, events: [
      event("entity_added", "margin", LAST_SEEN + HOUR), event("entity_added", "hanul", LAST_SEEN + 2 * HOUR),
      event("entity_added", "hanul", LAST_SEEN + 3 * HOUR), event("claim_added", "c1"), event("schema_extended", null),
      event("entity_added", "archived-or-missing"),
    ] }, "Tuesday");
    // Each chip takes the color the Map canvas fills its node with: the metric keeps the receded metric mark.
    expect(message.changed).toEqual({ names: 2, links: 1, more: 0, chips: [
      { id: "hanul", label: "Hanul Precision", swatch: nodeColorKey(hanul) },
      { id: "margin", label: "Operating margin", swatch: nodeColorKey(margin) },
    ] });
    expect(nodeColorKey(margin)).toBe("metric");
    expect(message.lead).toEqual({ kind: "count", count: 1, sinceMs: LAST_SEEN });
  });

  it("caps name chips at five and counts the rest", () => {
    const nodes = Array.from({ length: 7 }, (_, index) => node(`n${index}`, `Company ${index}`));
    const message = morningMessage({ ...base, map: map(nodes), events: nodes.map((item) => event("entity_added", item.id)) });
    expect(message.changed?.chips).toHaveLength(5);
    expect(message.changed?.more).toBe(2);
  });

  it("picks three questions in priority order without repeating a template or subject", () => {
    const message = morningMessage({ ...base,
      events: [event("entity_added", "hanul"), event("claim_added", "c1"), event("claim_added", "c2"), event("source_retained", null, LAST_SEEN + HOUR, "Q3 operating review")],
      journal: [check("j1", "2026-11-04")],
      watchlist: [{ symbol: "AAPL", market: "US", displayName: "Apple", addedAtMs: 0 }],
    }, "Tuesday");
    expect(message.questions).toEqual([
      { template: "changed", params: { name: "Hanul Precision", date: "Tuesday" }, reason: { key: "new", params: {} } },
      { template: "linked", params: { a: "Morrow Components", b: "Hanul Precision" }, reason: { key: "relation", params: {} } },
      { template: "explains", params: { subject: "Operating margin" }, reason: { key: "source", params: { title: "Q3 operating review" } } },
    ]);
  });

  it("asks about conflicts and checks once the diff runs out", () => {
    const message = morningMessage({ ...base, events: [event("claim_disputed", "c2")], journal: [check("j1", "2026-11-04")] });
    expect(message.questions.map((question) => question.template)).toEqual(["check", "conflict", "learned"]);
    expect(message.questions[1]?.params).toEqual({ name: "Hanul Precision" });
  });

  it("starts a first visit from the watchlist, the focus and what the Loupist knows", () => {
    const message = morningMessage({ ...base, lastSeenAtMs: undefined, events: [event("entity_added", "hanul")], focus: "Korean semiconductors",
      watchlist: [{ symbol: "005930", market: "KR", displayName: null, addedAtMs: 0 }] });
    expect(message.lead).toEqual({ kind: "toLook", count: 1 });
    expect(message.questions).toEqual([
      { template: "briefing", params: { symbol: "005930" }, reason: { key: "watching", params: {} } },
      { template: "focus", params: { focus: "Korean semiconductors" } },
      { template: "learned", params: {} },
    ]);
  });

  it("leaves a focus written as a sentence out of the questions", () => {
    const message = morningMessage({ ...base, focus: "Find durable compounders with honest management." });
    expect(message.questions.map((question) => question.template)).toEqual(["learned", "general", "markets"]);
  });

  it("says nothing new when nothing changed and the briefing was already read", () => {
    const message = morningMessage({ ...base, events: [], briefing: briefing({ finishedAtMs: LAST_SEEN - HOUR, readAtMs: LAST_SEEN }) });
    expect(message).toMatchObject({ lead: { kind: "nothingNew", sinceMs: LAST_SEEN }, changed: undefined, briefing: undefined, checks: [] });
  });

  it("drops a date the briefing summary repeats at its start", () => {
    expect(firstSentence("October 28: Hanul Precision reported higher costs. More follows.")).toBe("Hanul Precision reported higher costs.");
    expect(firstSentence("10월 28일: 한울정밀의 비용이 늘었어요. 이어서")).toBe("한울정밀의 비용이 늘었어요.");
    expect(firstSentence("Q3: revenue grew 11%.")).toBe("Q3: revenue grew 11%.");
  });

  it("quotes a check claim without its full stop", () => {
    const entry = check("j1", "2026-11-04");
    const message = morningMessage({ ...base, journal: [{ ...entry, check: { ...entry.check!, claim: "Margin stays above 30%.", metric: "" } }] });
    expect(message.checks[0]).toMatchObject({ claim: "Margin stays above 30%", metric: "Margin stays above 30%" });
  });

  it("asks general questions so a new Loupist still offers three", () => {
    const message = morningMessage({ language: "en", nowMs: NOW, map: map([]) });
    expect(message.questions.map((question) => question.template)).toEqual(["general", "markets", "risks"]);
  });

  it("asks what the user is researching when the Loupist knows nothing yet", () => {
    expect(morningMessage({ language: "en", nowMs: NOW, map: map([]) }).lead).toEqual({ kind: "empty" });
    expect(morningMessage({ language: "en", nowMs: NOW, map: map([]), watchlist: [{ symbol: "AAPL", market: "US", displayName: null, addedAtMs: 0 }] }).lead).toEqual({ kind: "firstDay" });
  });

  it("shows the latest succeeded briefing when it is new or unread", () => {
    expect(morningMessage({ ...base, briefing: briefing() }).briefing?.runId).toBe("run-1");
    expect(morningMessage({ ...base, briefing: briefing({ finishedAtMs: LAST_SEEN - HOUR, readAtMs: null }) }).briefing?.runId).toBe("run-1");
    expect(morningMessage({ ...base, briefing: briefing({ status: "failed" }) }).briefing).toBeUndefined();
  });

  it("lists up to three pending checks, soonest first", () => {
    const message = morningMessage({ ...base, journal: [
      check("later", "2026-12-01"), check("late", "2026-10-29"), check("today", "2026-10-31"), check("soon", "2026-11-02"),
      check("done", "2026-10-01", { outcome: { observed: 40, observedAt: "2026-10-01", result: "came_true" } }),
      check("archived", "2026-10-01", { archived: true }), check("none", "2026-10-01", { check: null }),
    ] });
    expect(message.checks.map((item) => [item.journalId, item.daysLeft])).toEqual([["late", -2], ["today", 0], ["soon", 2]]);
  });
});

describe("morning message helpers", () => {
  it("counts whole calendar days to a check date", () => {
    expect(daysUntil("2026-10-31", NOW)).toBe(0);
    expect(daysUntil("2026-11-01", new Date(2026, 9, 31, 23, 59).getTime())).toBe(1);
    expect(daysUntil("not a date", NOW)).toBeNaN();
  });

  it("keeps the first sentence of a run summary", () => {
    expect(firstSentence("Margins fell. More follows.")).toBe("Margins fell.");
    expect(firstSentence("이익률이 낮아졌어요. 다음 분기를 봐요.")).toBe("이익률이 낮아졌어요.");
    expect(firstSentence("No full stop")).toBe("No full stop");
  });
});
