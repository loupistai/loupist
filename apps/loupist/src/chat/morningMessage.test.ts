import type { GrowthEvent, JournalRecord, KnowledgeMap, KnowledgeMapEdge, KnowledgeMapNode } from "../contracts/knowledge";
import type { InboxItem } from "../contracts/scheduled";
import { botNewsDue, daysUntil, firstSentence, morningMessage, sinceDayOptions, type MorningInputs } from "./morningMessage";
import { formatDate } from "../i18n/format";
import { mockAssetId } from "../bridge/mock/assets";

const NOW = new Date(2026, 9, 31, 9, 0).getTime();
const HOUR = 3_600_000;
const LAST_SEEN = NOW - 3 * 24 * HOUR;

const node = (id: string, label: string, extra: Partial<KnowledgeMapNode> = {}): KnowledgeMapNode =>
  ({ id, kind: "entity", label, entityKind: "Organization", degree: 1, watched: false, ...extra });
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
  originChatId: null, createdAtMs: 0, updatedAtMs: 0, archived: false, inputs: { sources: [], worlds: [] } as unknown as JournalRecord["inputs"], inputDigest: "",
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
  it("counts every line it shows: the briefing and each check; the Map's growth is not news in Bot", () => {
    const message = morningMessage({ ...base, bot: true, events: [event("entity_added", "hanul"), event("claim_added", "c1")],
      briefing: briefing(), journal: [check("j1", "2026-11-04")] });
    expect(message.count).toBe(1 + 1);
  });

  it("asks what changed only about a name the user follows, even when an older host still marks a name held", () => {
    // Hosts before holdings left the app sent \`held\`; it carries no meaning now.
    const legacy = { ...morrow, held: true } as KnowledgeMapNode;
    const message = morningMessage({ ...base, map: map([hanul, legacy, margin]), events: [event("entity_added", "morrow"), event("entity_added", "hanul")] }, "Tuesday");
    expect(message.questions.filter((question) => question.template === "changed").map((question) => question.params)).toEqual([{ name: "Hanul Precision", date: "Tuesday" }]);
  });

  it("asks Bot nothing built from the Map", () => {
    const message = morningMessage({ ...base, bot: true, events: [event("entity_added", "hanul"), event("claim_added", "c1"), event("claim_disputed", "c2")],
      watchlist: [{ assetId: mockAssetId("Apple"), name: "Apple" }] }, "Tuesday");
    expect(message.questions.map((question) => question.template)).toEqual(["briefing", "general", "markets"]);
  });

  it("picks three questions in priority order without repeating a template or subject", () => {
    const message = morningMessage({ ...base,
      events: [event("entity_added", "hanul"), event("claim_added", "c1"), event("claim_added", "c2"), event("source_retained", null, LAST_SEEN + HOUR, "Q3 operating review")],
      journal: [check("j1", "2026-11-04")],
      watchlist: [{ assetId: mockAssetId("Apple"), name: "Apple" }],
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
      watchlist: [{ assetId: mockAssetId("005930"), name: "005930" }] });
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
    expect(message).toMatchObject({ count: 0, briefing: undefined, checks: [] });
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

  it("shows the latest succeeded briefing when it is new or unread", () => {
    expect(morningMessage({ ...base, briefing: briefing() }).briefing?.runId).toBe("run-1");
    expect(morningMessage({ ...base, briefing: briefing({ finishedAtMs: LAST_SEEN - HOUR, readAtMs: null }) }).briefing?.runId).toBe("run-1");
    expect(morningMessage({ ...base, briefing: briefing({ status: "failed" }) }).briefing).toBeUndefined();
  });

  it("lists up to three pending checks, soonest first", () => {
    const message = morningMessage({ ...base, journal: [
      check("later", "2026-12-01"), check("late", "2026-10-29"), check("today", "2026-10-31"), check("soon", "2026-11-02"),
      check("done", "2026-10-01", { outcome: { observed: 40, observedAt: "2026-10-01", result: "came_true" } }),
      check("archived", "2026-10-01", { archived: true }), check("none", "2026-10-01", { check: undefined }),
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

it("names a visit earlier today by its time, not today's weekday", () => {
  const now = new Date(2026, 10, 7, 15, 30).getTime();
  const since = (ms: number) => formatDate(ms, sinceDayOptions(ms, now));
  expect(since(new Date(2026, 10, 7, 2, 7).getTime())).toBe("2:07 AM");
  expect(since(new Date(2026, 10, 5, 9, 0).getTime())).toBe("Thursday");
  expect(since(new Date(2026, 9, 21, 9, 0).getTime())).toBe("Oct 21");
});

it("posts Bot's news only after a real absence or on a new day", () => {
  const at = (day: number, hour: number, minute = 0) => new Date(2026, 9, day, hour, minute).getTime();
  expect(botNewsDue(undefined, at(31, 9))).toBe(false);
  expect(botNewsDue(at(31, 8, 27), at(31, 9))).toBe(false);
  expect(botNewsDue(at(31, 5, 1), at(31, 9))).toBe(false);
  expect(botNewsDue(at(31, 5), at(31, 9))).toBe(true);
  // Across midnight, an hour away is the first open of the new day; ten minutes is still the same sitting.
  expect(botNewsDue(at(30, 23, 50), at(31, 0, 0))).toBe(false);
  expect(botNewsDue(at(30, 23), at(31, 0, 10))).toBe(true);
});
