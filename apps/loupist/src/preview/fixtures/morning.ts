/** Preview fixtures for the empty Chat's morning message and its event strip. */
import { MIN_VISIT_MS, recordVisit } from "../../app/loupistVisit";
import { mockAssetId } from "../../bridge/mock/assets";
import { companies, FIXTURE_TIME, prepareFixture, PreviewBridge, previewKo, sourceTitles } from "../fixtures";

/**
 * The empty Chat's morning message. "morning": two days since the last visit with new names,
 * relationships, an unread briefing and two Journal checks; "first-day": a new Loupist's seed anchors;
 * "nothing-changed": a visit yesterday and nothing since.
 */
export async function prepareMorningFixture(bridge: PreviewBridge, state: "morning" | "first-day" | "nothing-changed") {
  await prepareFixture(bridge);
  const ko = previewKo();
  const hour = 3_600_000;
  const lastSeenAtMs = state === "morning" ? FIXTURE_TIME - 2 * 86_400_000 : state === "nothing-changed" ? FIXTURE_TIME - 86_400_000 : undefined;
  if (lastSeenAtMs !== undefined) recordVisit("loupist-quality", lastSeenAtMs - MIN_VISIT_MS, lastSeenAtMs);
  const localNames: Record<string, string[]> = { "map-entity-00": ["한울정밀"], "map-entity-04": ["은빛소재"], "map-entity-29": ["영업이익률"] };
  const map = bridge.knowledgeMap.bind(bridge);
  bridge.knowledgeMap = async request => {
    const result = await map(request);
    for (const node of result.nodes) {
      const index = Number(/^map-entity-(\d+)$/.exec(node.id)?.[1] ?? Number.NaN);
      if (index < companies.length) { node.label = companies[index]!; node.localNames = localNames[node.id]; }
      if (localNames[node.id] && index >= companies.length) node.localNames = localNames[node.id];
    }
    return result;
  };
  const event = (kind: import("../../contracts/knowledge").GrowthEvent["kind"], subjectId: string | null, atMs: number, summary = ""): import("../../contracts/knowledge").GrowthEvent =>
    ({ eventId: `morning-${kind}-${subjectId ?? atMs}`, loupistId: "loupist-quality", atMs, kind, subjectId, recordId: "preview-world", originChatId: null, originMessageId: null, actor: "loupist", summary, beforeSummary: null, afterSummary: null });
  const events = state === "morning" ? [
    event("entity_added", "map-entity-00", FIXTURE_TIME - hour), event("claim_added", "map-claim-04", FIXTURE_TIME - 2 * hour),
    event("entity_added", "map-entity-05", FIXTURE_TIME - 3 * hour), event("entity_added", "map-entity-29", FIXTURE_TIME - 4 * hour),
    event("claim_added", "map-claim-28", FIXTURE_TIME - 5 * hour), ...["03", "04", "06", "07"].map((id, i) => event("entity_added", `map-entity-${id}`, FIXTURE_TIME - (6 + i) * hour)),
    event("source_retained", null, FIXTURE_TIME - 10 * hour, ko ? "한울정밀 2026년 3분기 실적 설명자료" : sourceTitles[1]), event("schema_extended", null, FIXTURE_TIME - 11 * hour),
  ] : state === "first-day" ? ["00", "01", "02", "04", "29"].map((id, i) => event("entity_added", `map-entity-${id}`, FIXTURE_TIME - 10 * 60_000 - i)) : [];
  bridge.knowledgeGrowthEvents = async request => ({ events: events.filter(item => request.sinceMs == null || item.atMs >= request.sinceMs).sort((a, b) => a.atMs - b.atMs), nextCursor: null });
  const watchlist = [{ symbol: "HNP", market: "KRX", displayName: ko ? "한울정밀" : "Hanul Precision", addedAtMs: FIXTURE_TIME - 30 * 86_400_000 }];
  bridge.watchlistList = async () => structuredClone(watchlist);
  if (state !== "morning") {
    // A briefing already read before the last visit is not news.
    const inbox = bridge.inboxList.bind(bridge);
    bridge.inboxList = async request => {
      const page = await inbox(request);
      return { ...page, items: state === "first-day" ? [] : page.items.filter(item => item.runId === "preview-briefing-3") };
    };
    bridge.journalList = async () => [];
  } else {
    const checks: [string, string, string][] = ko
      ? [["한울정밀의 재고일수가 50일 아래로 내려간다", "재고일수", "2026-10-26"], ["한울정밀의 매출총이익률이 55% 이상을 유지한다", "매출총이익률", "2026-11-14"]]
      : [["Hanul’s inventory days fall below 50", "inventory days", "2026-10-26"], ["Hanul’s gross margin stays above 55%", "gross margin", "2026-11-14"]];
    bridge.journalList = async () => checks.map(([claim, metric, byDate], index) => ({
      journalId: `preview-morning-check-${index}`, researchId: "", loupistId: "loupist-quality", judgment: claim, reasoning: "", entities: [],
      check: { claim, metric, direction: index ? "above" : "below", threshold: index ? 55 : 50, byDate }, originChatId: "preview-chat",
      createdAtMs: FIXTURE_TIME - 30 * 86_400_000, updatedAtMs: FIXTURE_TIME - 30 * 86_400_000, archived: false, inputDigest: "preview",
      inputs: { sources: [], worlds: [], lenses: [], earlier: [] },
    })) as import("../../types").JournalRecord[];
  }
  if (state === "first-day") {
    const philosophy = ko ? "산업 공급망" : "industrial supply chains";
    const loupists = bridge.loupistList.bind(bridge);
    bridge.loupistList = async () => (await loupists()).map(loupist => loupist.id === "loupist-quality" ? { ...loupist, philosophy } : loupist);
    const select = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => { const workspace = await select(loupistId); return loupistId === "loupist-quality" ? { ...workspace, loupist: { ...workspace.loupist, philosophy } } : workspace; };
  }
}

/**
 * The morning message's event strip (W15.9e) on the fixture day (Wed Oct 28):
 * five events in the next two weeks, as Rust orders them, or nothing ahead.
 */
export async function prepareUpcomingFixture(bridge: PreviewBridge, state: "events" | "none") {
  await prepareMorningFixture(bridge, "morning");
  if (state === "none") { bridge.assets.installUpcoming("loupist-quality", []); return; }
  const ko = previewKo();
  const asset = (symbol: string, market: string, name: string, entityId: string) => ({ assetId: mockAssetId(symbol, market), symbol, market, name, entityId });
  const hanul = asset("HNP", "KRX", "Hanul Precision", "map-entity-00");
  const eunbit = asset("EBM", "KRX", "Eunbit Materials", "map-entity-04");
  const backlog = ko ? "모로우의 수주 잔고가 계속 늘어난다" : "Morrow’s order backlog keeps growing";
  // The check in the strip is the Journal's own; the message does not repeat it as a bullet.
  const journal = bridge.journalList.bind(bridge);
  bridge.journalList = async request => [...await journal(request), {
    journalId: "preview-upcoming-check", researchId: "", loupistId: "loupist-quality", judgment: backlog, reasoning: "", entities: [],
    check: { claim: backlog, metric: ko ? "수주 잔고" : "order backlog", direction: "above", threshold: 0, byDate: "2026-11-03" }, originChatId: "preview-chat",
    createdAtMs: FIXTURE_TIME - 20 * 86_400_000, updatedAtMs: FIXTURE_TIME - 20 * 86_400_000, archived: false, inputDigest: "preview",
    inputs: { sources: [], worlds: [], lenses: [], earlier: [] },
  } as import("../../types").JournalRecord];
  bridge.assets.installUpcoming("loupist-quality", [
    { kind: "results", date: "2026-10-29", asset: asset("MRRW", "NASDAQ", "Morrow Components", "map-entity-01"), dateSource: "calendar", eventId: "ern_0123456789abcdef", period: "Q3 2026" },
    { kind: "exDividend", date: "2026-10-30", asset: asset("BLHV", "NYSE", "Bluehaven Logistics", "map-entity-02") },
    { kind: "results", date: "2026-11-02", asset: hanul, dateSource: "ir_notice", eventId: "ern_1123456789abcdef", period: "Q3 2026" },
    { kind: "check", date: "2026-11-03", journalId: "preview-upcoming-check", claim: backlog },
    { kind: "recordDate", date: "2026-11-09", asset: eunbit },
  ]);
}
