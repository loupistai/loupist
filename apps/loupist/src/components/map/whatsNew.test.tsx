import { act, render, screen, within } from "@testing-library/react";
import i18n from "@/i18n";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { GrowthEvent, KnowledgeMap } from "../../contracts/knowledge";
import { MapWhatsNew } from "./MapWhatsNew";
import { groupByDay, sinceMs, whatsNewCounts, whatsNewEntries, whatsNewRows } from "./whatsNew";

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
  const self = (id: string, subject: string, status = "active") => ({ id, kind: "fact", source: subject, target: subject, relation: "REPORTS", status });
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

function sourcesFixture() {
  const at = NOW - HOUR;
  const source = { recordId: "source-a", revisionId: "source-rev-a", sourceId: "a", title: "Automaker valuation multiples" };
  const short = { ...source, recordId: "source-b", revisionId: "source-rev-b", sourceId: "b", title: "Motorcycle report" };
  const update = { revisionId: "world-a", source };
  const secondUpdate = { revisionId: "world-b", source: short };
  const names = ["Zenith", "Gamma", "Beta", "Delta", "Echo", "Fable"];
  const newsMap: KnowledgeMap = {
    ...map,
    nodes: [...map.nodes, ...names.map(label => ({ id: label, label, kind: "entity" as const, entityKind: "Organization", degree: 0, watched: false }))],
  };
  const namesAdded = names.map((name, index) => event("entity_added", name, at, {
    publication: index < 4 ? update : secondUpdate, originMessageId: "the-same-reply",
  }));
  const fact = (name: string, id: string, publication = update) => event("claim_added", id, at, {
    publication: { ...publication, factSubjectId: name }, originMessageId: "the-same-reply",
  });
  const facts = [fact("Zenith", "sales"), fact("Zenith", "cap"), fact("hanul", "income"), fact("Echo", "margin", secondUpdate)];
  return { newsMap, source, short, namesAdded, facts, at };
}

it("combines a new name and all its facts from the same publication even off the projection", () => {
  const { newsMap, namesAdded, facts } = sourcesFixture();
  const separate = { ...facts[0]!, eventId: "another", subjectId: "another", publication: { ...facts[0]!.publication!, revisionId: "later" } };
  const rows = whatsNewRows([...namesAdded, ...facts, separate], newsMap, "en");
  expect(rows.filter(row => row.facts).map(row => [row.label, row.facts])).toEqual([["Zenith", 2], ["Echo", 1]]);
  expect(rows.filter(row => row.kind === "fact").map(row => [row.link?.a, row.count])).toEqual([["Hanul Precision", 1], ["Zenith", 1]]);
});

it("uses the collapsed row threshold per publication and Source after combining names and facts", () => {
  const { newsMap, namesAdded, facts, source, short } = sourcesFixture();
  const rows = whatsNewRows([...namesAdded, ...facts], newsMap, "en");
  const entries = whatsNewEntries(rows, "en");
  expect(entries).toHaveLength(3);
  const group = entries[0]!;
  expect(group).toMatchObject({ kind: "source", source, names: 4, facts: 3 });
  if (group.kind !== "source") throw new Error("Expected one Source group");
  expect(group.rows.map(row => [row.kind, row.link?.a ?? row.label])).toEqual([
    ["name", "Beta"], ["name", "Delta"], ["name", "Gamma"], ["name", "Zenith"], ["fact", "Hanul Precision"],
  ]);
  expect(entries.slice(1)).toEqual(expect.arrayContaining([expect.objectContaining({ kind: "name", source: short, facts: 1 })]));
  expect(whatsNewCounts(entries)).toEqual({ names: 3, links: 0, conflicts: 0 });
  expect(whatsNewEntries(rows.filter(row => ["Beta", "Gamma", "Zenith"].includes(row.label)), "en")).toHaveLength(3);
});

it("never groups by a shared Source title, timestamp or message without matching identity", () => {
  const { newsMap, namesAdded, source } = sourcesFixture();
  const separate = namesAdded.map((item, index) => ({ ...item,
    publication: { revisionId: "world-a", source: index < 3 ? source : { ...source, recordId: "different-record" } },
  }));
  expect(whatsNewEntries(whatsNewRows(separate, newsMap, "en"), "en")).toHaveLength(6);
  const historical = namesAdded.map(item => ({ ...item, publication: undefined }));
  expect(whatsNewEntries(whatsNewRows(historical, newsMap, "en"), "en")).toHaveLength(6);
});

it("carries a Source across five pages, keeps disclosure open after refresh and opens its Source and children", async () => {
  const user = userEvent.setup();
  const { newsMap, namesAdded, facts, source, at } = sourcesFixture();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const pages = [[...facts], [namesAdded[0]!], [namesAdded[1]!], [namesAdded[2]!], namesAdded.slice(3)];
  const read = vi.spyOn(bridge, "knowledgeGrowthEvents").mockImplementation(async ({ cursor }) => {
    const page = cursor ? Number(cursor) : 0;
    return { events: pages[page]!, nextCursor: page < pages.length - 1 ? String(page + 1) : null };
  });
  const onOpenSource = vi.fn(), onOpenRow = vi.fn(), onReplay = vi.fn();
  const props = { bridge, loupistId: "loupist", lastSeenAtMs: at - 1, nowMs: NOW, onOpenSource, onOpenRow, onReplay, onClose: vi.fn() };
  const view = render(<MapWhatsNew {...props} map={newsMap} />);
  const show = await screen.findByRole("button", { name: "Show the 5 changes from this source" });
  expect(read).toHaveBeenCalledTimes(5);
  expect(view.container.querySelectorAll('[data-kind="source"]')).toHaveLength(1);
  expect(screen.getByText("4 new names · 3 new facts")).toBeVisible();
  expect(view.container.querySelector(".map-whats-new-summary")).toHaveTextContent("+3 in World");
  expect(screen.queryByRole("list", { name: source.title })).toBeNull();
  const title = screen.getByRole("button", { name: new RegExp(source.title) });
  title.focus();
  await user.tab();
  expect(show).toHaveFocus();
  await user.keyboard("{Enter}");
  const list = screen.getByRole("list", { name: source.title });
  expect(within(list).getAllByRole("listitem")).toHaveLength(5);
  expect(within(list).getByText("Company · 2 new facts")).toBeVisible();
  expect(screen.getByText("Company · 1 new fact")).toBeVisible();
  expect(view.container.querySelector(".map-whats-new-summary")).toHaveTextContent("+3 in World");
  await act(async () => view.rerender(<MapWhatsNew {...props} map={{ ...newsMap, worldRevisionId: "world-newer" }} />));
  expect(screen.getByRole("list", { name: source.title })).toBeVisible();
  await user.click(within(list).getByRole("button", { name: /Zenith/ }));
  expect(onOpenRow).toHaveBeenCalledWith(expect.objectContaining({ kind: "name", label: "Zenith" }));
  const hide = screen.getByRole("button", { name: "Hide the changes from this source" });
  expect(hide).toHaveAttribute("aria-expanded", "true");
  await user.click(hide);
  expect(screen.queryByRole("list", { name: source.title })).toBeNull();
  await user.click(title);
  expect(onOpenSource).toHaveBeenCalledWith(source);
  await user.click(screen.getByRole("button", { name: "Replay these" }));
  expect(onReplay).toHaveBeenCalledWith(at - 1);
});
