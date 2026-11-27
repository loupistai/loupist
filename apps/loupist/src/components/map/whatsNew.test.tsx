import { act, render, screen, within } from "@testing-library/react";
import i18n from "@/i18n";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { GrowthEvent, KnowledgeMap } from "../../contracts/knowledge";
import { MapWhatsNew } from "./MapWhatsNew";
import { firstSeenAfter, groupByDay, sinceMs, whatsNewEntries, whatsNewLinkText, whatsNewRows, whatsNewTotals } from "./whatsNew";

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
    // A name added twice in the range belongs to its first add.
    event("entity_added", "hanul", NOW - 3 * HOUR),
    event("entity_added", "hanul", NOW - 4 * HOUR, { originMessageId: "answer" }),
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
  expect(rows.find((row) => row.kind === "name")?.atMs).toBe(NOW - 4 * HOUR);
  // A name that has left World stays dimmed and is not new.
  expect(whatsNewTotals(rows)).toEqual({ names: 1, links: 1, conflicts: 1 });
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
  // Totals count claims, so the two OWNS claims on one row are two relationships.
  expect(whatsNewTotals(rows)).toEqual({ names: 0, links: 3, conflicts: 1 });
});

it("writes a changed claim's relation in the interface language, and an unlisted one in plain English", () => {
  const ko = i18n.getFixedT("ko", "knowledge");
  const en = i18n.getFixedT("en", "knowledge");
  expect(whatsNewLinkText({ a: "테슬라", b: "나스닥", relation: "LISTS_ON" }, ko)).toBe("테슬라 → 나스닥 (상장 거래소)");
  expect(whatsNewLinkText({ a: "Tesla", b: "Nasdaq", relation: "LISTS_ON" }, en)).toBe("Tesla → Nasdaq (lists on)");
  expect(whatsNewLinkText({ a: "테슬라", b: "CES", relation: "DEMOED_AT" }, ko)).toBe("테슬라 → CES (demoed at)");
  expect(whatsNewLinkText({ a: "테슬라", relation: "LOCATED_IN" }, ko)).toBe("테슬라 (소재지)");
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

it("uses the collapsed row threshold per Source after combining names and facts", () => {
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
  // The header adds up every row: the group's four names and the two names outside it.
  expect(whatsNewTotals(rows)).toEqual({ names: 6, links: 0, conflicts: 0 });
  expect(whatsNewEntries(rows.filter(row => ["Beta", "Gamma", "Zenith"].includes(row.label)), "en")).toHaveLength(3);
});

it("keeps one group for a Source published in several chunks, newest change first", () => {
  const { newsMap, namesAdded, facts, source, at } = sourcesFixture();
  // The same Source: its first chunk added four names, a later chunk five minutes on added two more and a fact.
  const later = { revisionId: "world-c", source: { ...source, revisionId: "source-rev-a2" } };
  const chunked = [
    ...namesAdded.slice(0, 4), facts[0]!, facts[2]!,
    ...namesAdded.slice(4).map(item => ({ ...item, eventId: `${item.eventId}:later`, atMs: at + 5 * 60_000, publication: later })),
    { ...facts[3]!, eventId: "margin:later", atMs: at + 5 * 60_000, publication: { ...later, factSubjectId: "Echo" } },
  ];
  const entries = whatsNewEntries(whatsNewRows(chunked, newsMap, "en"), "en");
  expect(entries).toHaveLength(1);
  const group = entries[0]!;
  if (group.kind !== "source") throw new Error("Expected one Source group");
  expect(group).toMatchObject({ atMs: at + 5 * 60_000, names: 6, facts: 3, source: later.source });
  expect(group.rows.map(row => [row.atMs - at, row.link?.a ?? row.label])).toEqual([
    [300_000, "Echo"], [300_000, "Fable"],
    [0, "Beta"], [0, "Delta"], [0, "Gamma"], [0, "Zenith"], [0, "Hanul Precision"],
  ]);
  // The toolbar badge counts entries, so the Source counts once; the header counts its names.
  expect(entries).toHaveLength(1);
  expect(whatsNewTotals(group.rows)).toEqual({ names: 6, links: 0, conflicts: 0 });
  // Its disclosure keeps its identity when a further chunk arrives.
  const next = { revisionId: "world-d", source };
  const more = whatsNewEntries(whatsNewRows([...chunked, event("entity_added", "hanul", at + 9 * 60_000, { publication: next })], newsMap, "en"), "en");
  expect(more.map(entry => [entry.kind, entry.id, entry.atMs - at])).toEqual([["source", group.id, 540_000]]);
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
  expect(view.container.querySelector(".map-whats-new-summary")).toHaveTextContent("+6 in World");
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
  expect(view.container.querySelector(".map-whats-new-summary")).toHaveTextContent("+6 in World");
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

/**
 * One week of two Sources, as Y-009 saw it: Source A in two chunks adds five names; Source B
 * later re-adds one of them with facts, adds one more name and reports values about earlier
 * ones. Hanul existed before the range; Old name was added and has left World.
 */
function weekFixture() {
  const { newsMap, at } = sourcesFixture();
  const since = at - DAY_MS;
  const a = { recordId: "pe", revisionId: "pe-1", sourceId: "pe", title: "Automakers by P/E ratio" };
  const b = { recordId: "cap", revisionId: "cap-1", sourceId: "cap", title: "Largest automakers by market cap" };
  const chunk1 = { revisionId: "world-1", source: a }, chunk2 = { revisionId: "world-2", source: a }, later = { revisionId: "world-3", source: b };
  const added = (id: string, atMs: number, publication: GrowthEvent["publication"]) => event("entity_added", id, atMs, { eventId: `${id}:${atMs}`, publication });
  const fact = (id: string, name: string, atMs: number) => event("claim_added", id, atMs, { publication: { ...later, factSubjectId: name } });
  const map: KnowledgeMap = {
    ...newsMap,
    nodes: newsMap.nodes.map(node => node.id === "hanul" ? { ...node, firstSeenAtMs: since - DAY_MS } : node.id === "Zenith" ? { ...node, firstSeenAtMs: at } : node),
  };
  const events = [
    ...["Zenith", "Gamma", "Beta", "Delta"].map(id => added(id, at, chunk1)),
    added("Echo", at + 5 * 60_000, chunk2),
    // Source B: Zenith again (the same World name), a new Fable, Hanul back from before the range.
    added("Zenith", at + 10 * 60_000, later), added("Fable", at + 10 * 60_000, later), added("hanul", at + 10 * 60_000, later),
    fact("zenith-sales", "Zenith", at + 10 * 60_000), fact("zenith-cap", "Zenith", at + 10 * 60_000),
    fact("gamma-cap", "Gamma", at + 10 * 60_000), fact("beta-cap", "Beta", at + 10 * 60_000),
    added("old", at + 10 * 60_000, later),
  ];
  return { map, events, since, a, b };
}
const DAY_MS = 24 * HOUR;

it("counts a name once, in the group of the Source that first added it, and a later Source's values as facts", () => {
  const { map, events, since, a, b } = weekFixture();
  const rows = whatsNewRows(events, map, "en", since);
  const entries = whatsNewEntries(rows, "en");
  const groups = entries.filter(entry => entry.kind === "source");
  expect(groups.map(group => [group.source.title, group.names, group.facts])).toEqual([[b.title, 1, 4], [a.title, 5, 0]]);
  // Zenith is new once, in A; in B it gains two facts.
  expect(rows.filter(row => row.kind === "name" && row.label === "Zenith").map(row => row.source?.title)).toEqual([a.title]);
  expect(rows.find(row => row.kind === "fact" && row.link?.a === "Zenith")).toMatchObject({ count: 2, source: b });
  // Hanul existed before the range, so it is not new; Old name left World, so it is listed dimmed and not counted.
  expect(rows.some(row => row.kind === "name" && row.label === "Hanul Precision")).toBe(false);
  expect(rows.find(row => row.label === "Old name")).toMatchObject({ kind: "name", gone: true });
  // The header is the sum of the groups and the rows outside them.
  expect(whatsNewTotals(rows).names).toBe(6);
  expect(groups.reduce((total, group) => total + group.names, 0)).toBe(6);
});

it("calls a name new only when World first saw it after the range's start", () => {
  expect(firstSeenAfter(10, 9)).toBe(true);
  expect(firstSeenAfter(9, 9)).toBe(false);
  expect(firstSeenAfter(undefined, 9)).toBe(true);
  expect(firstSeenAfter(5, undefined)).toBe(true);
});

it("lists an ended fact, a disagreement and a superseded Lens section with their Source", async () => {
  const source = { recordId: "source-record", revisionId: "source-rev", sourceId: "filing", title: "Q3 filing" };
  const publication = { revisionId: "world-rev", source };
  const events = [
    event("claim_ended" as GrowthEvent["kind"], "old-claim", NOW, { summary: "Margin was 14%", publication }),
    event("claim_conflict" as GrowthEvent["kind"], "split-claim", NOW - HOUR, { summary: "Capacity rose 18%", publication }),
    event("lens_superseded" as GrowthEvent["kind"], null, NOW - 2 * HOUR, { recordId: "lens:m", summary: "Leadership", beforeSummary: "Test capacity costs", publication }),
  ];
  const rows = whatsNewRows(events, map, "en");
  expect(rows.map(row => [row.kind, row.label, row.page, row.section, row.source?.title])).toEqual([
    ["ended", "Margin was 14%", undefined, undefined, "Q3 filing"],
    ["disagree", "Capacity rose 18%", undefined, undefined, "Q3 filing"],
    ["superseded", "Leadership", "Test capacity costs", "Leadership", "Q3 filing"],
  ]);
  const ended = [0, 1, 2, 3].map(index => event("claim_ended" as GrowthEvent["kind"], `ended-${index}`, NOW - index, { summary: `Ended fact ${index}`, publication }));
  const entries = whatsNewEntries(whatsNewRows(ended, map, "en"), "en");
  expect(entries).toHaveLength(1);
  expect(entries[0]).toMatchObject({ kind: "source", source });
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "knowledgeGrowthEvents").mockResolvedValue({ events, nextCursor: null });
  render(<MapWhatsNew bridge={bridge} loupistId="loupist" map={map} lastSeenAtMs={NOW - 1} nowMs={NOW} onOpenRow={vi.fn()} onReplay={vi.fn()} onClose={vi.fn()} />);
  expect(await screen.findByText("Ended: Margin was 14%")).toBeVisible();
  expect(screen.getByText("Sources disagree: Capacity rose 18%")).toBeVisible();
  expect(screen.getByText("Lens section superseded: Test capacity costs · Leadership")).toBeVisible();
});

it("counts a name a bounded overview left out, and opens it by id; an absent name in a complete overview has left World", () => {
  const { map, events, since } = weekFixture();
  const far = event("entity_added", "far-away", since + HOUR, { summary: "Far Away Motors" });
  const bounded = { ...map, counts: { ...map.counts, entity: { total: 2000, shown: 1000, truncated: true } } } as KnowledgeMap;
  const outside = whatsNewRows([...events, far], bounded, "en", since).find(row => row.label === "Far Away Motors");
  expect(outside).toMatchObject({ kind: "name", gone: false, nameId: "far-away" });
  expect(whatsNewTotals(whatsNewRows([...events, far], bounded, "en", since)).names).toBe(7);
  expect(whatsNewRows([...events, far], map, "en", since).find(row => row.label === "Far Away Motors")).toMatchObject({ gone: true });
  expect(whatsNewTotals(whatsNewRows([...events, far], map, "en", since)).names).toBe(6);
});

it("totals the whole range whichever page an event arrived on", async () => {
  const { map, events, since, a, b } = weekFixture();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  // Pages split each Source and put B's re-add of Zenith before A's first chunk finishes arriving.
  const pages = [events.slice(5, 8), events.slice(0, 2), events.slice(8), events.slice(2, 5)];
  const read = vi.spyOn(bridge, "knowledgeGrowthEvents").mockImplementation(async ({ cursor }) => {
    const page = cursor ? Number(cursor) : 0;
    return { events: pages[page]!, nextCursor: page < pages.length - 1 ? String(page + 1) : null };
  });
  const view = render(<MapWhatsNew bridge={bridge} loupistId="loupist" map={map} lastSeenAtMs={since} nowMs={NOW} onOpenRow={vi.fn()} onReplay={vi.fn()} onClose={vi.fn()} />);
  await screen.findByRole("button", { name: new RegExp(a.title) });
  expect(read).toHaveBeenCalledTimes(4);
  expect(view.container.querySelector(".map-whats-new-summary")).toHaveTextContent("+6 in World");
  expect(within(screen.getByRole("button", { name: new RegExp(a.title) })).getByText("5 new names")).toBeVisible();
  expect(within(screen.getByRole("button", { name: new RegExp(b.title) })).getByText("1 new name · 4 new facts")).toBeVisible();
});
