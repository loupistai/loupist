/** Preview fixtures for the empty Chat's morning message. */
import { MIN_VISIT_MS, recordVisit } from "../../app/loupistVisit";
import { mockAssetId } from "../../bridge/mock/assets";
import { companies, FIXTURE_TIME, prepareFixture, PreviewBridge, previewKo, sourceTitles } from "../fixtures";

/**
 * The home Thread's messages as written `days` before the fixture's clock. Bot's news waits for a
 * real absence after the person's last message, and the stored conversation is dated at the clock.
 */
export function writtenEarlier(bridge: PreviewBridge, days: number) {
  const select = bridge.loupistSelect.bind(bridge);
  const shift = (at: string) => new Date(Date.parse(at) - days * 86_400_000).toISOString();
  bridge.loupistSelect = async loupistId => {
    const workspace = await select(loupistId);
    return { ...workspace, threads: workspace.threads.map(thread => thread.home ? { ...thread, messages: thread.messages.map(message => ({ ...message, created_at: shift(message.created_at) })) } : thread) };
  };
}

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
  bridge.assets.installAsset({ assetId: mockAssetId("Hanul Precision"), name: ko ? "한울정밀" : "Hanul Precision", ticker: "HNP", exchange: "KS", createdAtMs: FIXTURE_TIME - 30 * 86_400_000 });
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
