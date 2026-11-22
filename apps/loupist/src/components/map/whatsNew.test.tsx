import { render, screen, within } from "@testing-library/react";
import i18n from "@/i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { GrowthEvent, KnowledgeMap } from "../../contracts/knowledge";
import { MapWhatsNew } from "./MapWhatsNew";
import { groupByDay, sinceMs, whatsNewCounts, whatsNewRows } from "./whatsNew";

const NOW = new Date(2026, 9, 31, 12).getTime();
const HOUR = 3_600_000;
const event = (kind: GrowthEvent["kind"], subjectId: string | null, atMs: number, extra: Partial<GrowthEvent> = {}): GrowthEvent => ({
  eventId: `${kind}:${subjectId}:${atMs}`, loupistId: "loupist", atMs, kind, subjectId, recordId: "world", originChatId: null, originMessageId: null,
  actor: "loupist", summary: subjectId ?? "", beforeSummary: null, afterSummary: null, ...extra,
});
const map = {
  nodes: [
    { id: "hanul", label: "Hanul Precision", localNames: ["한울정밀"], kind: "entity", entityKind: "Organization", degree: 2, watched: true },
    { id: "margin", label: "Operating margin", kind: "entity", entityKind: "Metric", degree: 1, watched: false },
    { id: "old", label: "Old name", kind: "entity", entityKind: "Organization", degree: 0, watched: false, archived: true },
    { id: "lens:m", recordId: "lens-method", label: "Test capacity costs", kind: "lens_method", degree: 0, watched: false },
  ],
  edges: [{ id: "c1", kind: "claim", source: "hanul", target: "margin", status: "disputed" }],
  counts: {}, unmatchedWatchlist: [], worldRevisionId: "rev", generatedAtMs: NOW,
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

/** One Farside table read in one reply: a reported net flow per trading day on each name, and two holdings. */
function farsideFixture() {
  const asset = (id: string, label: string, localNames?: string[]) => ({ id, label, localNames, kind: "entity", entityKind: "Asset", degree: 1, watched: true });
  const self = (id: string, subject: string, status = "active") => ({ id, kind: "claim", source: subject, target: subject, relation: "REPORTS", status });
  const farsideMap = {
    nodes: [asset("krw-btc", "KRW-BTC"), asset("btc", "Bitcoin (BTC:CRYPTO)", ["비트코인"]), asset("btc-alias", "BTC", ["비트코인"]), { ...asset("strategy", "Strategy"), entityKind: "Organization" }],
    edges: [
      ...[1, 2, 3, 4, 5, 6, 7].map((day) => self(`flow-${day}`, "krw-btc")),
      self("btc-flow", "btc"),
      self("btc-disputed", "btc", "disputed"),
      // Two nodes that share a name are one asset.
      { id: "alias-flow", kind: "claim", source: "btc", target: "btc-alias", relation: "REPORTS", status: "active" },
      { id: "own-1", kind: "claim", source: "strategy", target: "btc", relation: "OWNS", status: "active" },
      { id: "own-2", kind: "claim", source: "strategy", target: "btc", relation: "OWNS", status: "active" },
      { id: "invest", kind: "claim", source: "strategy", target: "btc", relation: "INVESTED_IN", status: "active" },
    ],
    counts: {}, unmatchedWatchlist: [], worldRevisionId: "rev", generatedAtMs: NOW,
  } as unknown as KnowledgeMap;
  const at = NOW - HOUR;
  const added = (id: string, atMs = at) => event("claim_added", id, atMs, { originMessageId: "answer", summary: `${id} proposition` });
  const events = [
    event("source_retained", null, at, { originMessageId: "answer", summary: "Farside Investors — US spot bitcoin ETF daily net flows" }),
    ...[1, 2, 3, 4, 5, 6].map((day) => added(`flow-${day}`)),
    added("btc-flow"), added("alias-flow"), added("own-1"), added("own-2"), added("invest"),
    event("claim_disputed", "btc-disputed", at, { summary: "Bitcoin ETF flows 120 vs 150" }),
    // A value reported in an earlier publication is another row.
    added("flow-7", at - 2 * HOUR),
  ];
  return { farsideMap, events };
}

it("reads a value reported about one name as a fact about it, and lists identical changes from one publication once", () => {
  const { farsideMap, events } = farsideFixture();
  const rows = whatsNewRows(events, farsideMap, "en");
  expect(rows.map((row) => [row.kind, row.link, row.count])).toEqual([
    ["fact", { a: "KRW-BTC", b: undefined, relation: "REPORTS" }, 6],
    // A value on the node aliased the same is about the same asset.
    ["fact", { a: "Bitcoin (BTC:CRYPTO)", b: undefined, relation: "REPORTS" }, 2],
    ["relationship", { a: "Strategy", b: "Bitcoin (BTC:CRYPTO)", relation: "OWNS" }, 2],
    ["relationship", { a: "Strategy", b: "Bitcoin (BTC:CRYPTO)", relation: "INVESTED_IN" }, 1],
    ["conflict", { a: "Bitcoin (BTC:CRYPTO)", b: undefined, relation: "REPORTS" }, 1],
    ["fact", { a: "KRW-BTC", b: undefined, relation: "REPORTS" }, 1],
  ]);
  // A fact row opens the name its Key facts belong to and carries that name's swatch.
  expect(rows[0]).toMatchObject({ node: { id: "krw-btc" }, category: "market", origin: "Farside Investors — US spot bitcoin ETF daily net flows" });
  expect(whatsNewCounts(rows)).toEqual({ names: 0, links: 2, conflicts: 1 });
});

describe("the What's new drawer", () => {
  afterEach(async () => { await i18n.changeLanguage("en"); });
  const open = async (language: string) => {
    await i18n.changeLanguage(language);
    const { farsideMap, events } = farsideFixture();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "knowledgeGrowthEvents").mockResolvedValue({ events, nextCursor: null });
    render(<MapWhatsNew bridge={bridge} loupistId="loupist" map={farsideMap} lastSeenAtMs={NOW - 30 * HOUR} nowMs={NOW} onOpenRow={vi.fn()} onReplay={vi.fn()} onClose={vi.fn()} />);
    const drawer = await screen.findByRole("dialog");
    await within(drawer).findAllByRole("listitem");
    return within(drawer).getAllByRole("listitem").map((item) => item.querySelector(".map-whats-new-label")!.textContent);
  };

  it("never lists a name related to itself, and says how many values one Source reported", async () => {
    expect(await open("en")).toEqual([
      "6 new facts about KRW-BTC",
      "2 new facts about Bitcoin (BTC:CRYPTO)",
      "Strategy → Bitcoin (BTC:CRYPTO) (owns)",
      "Strategy → Bitcoin (BTC:CRYPTO) (invested in)",
      "Two sources disagree: Bitcoin (BTC:CRYPTO) (reports)",
      "New fact about KRW-BTC",
    ]);
  });

  it("speaks Korean through the names in World", async () => {
    const labels = await open("ko");
    expect(labels[0]).toBe("KRW-BTC에 관한 새 사실 6건");
    expect(labels[1]).toBe("비트코인에 관한 새 사실 2건");
    expect(labels[4]).toBe("출처가 엇갈림: 비트코인 (보고)");
    expect(labels.some((label) => /(\S+) → \1/.test(label!))).toBe(false);
  });
});
