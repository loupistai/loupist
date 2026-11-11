/** Preview fixture: the Journal and its research notes. */
import { FIXTURE_TIME, prepareFixture, PreviewBridge, previewKo } from "../fixtures";

export async function prepareJournalFixture(bridge: PreviewBridge) {
  await prepareFixture(bridge);
  const entries: import("../../types").JournalRecord[] = [
    { judgment: "I think Hanul’s gross margin will stay above 55% through Q1.", reasoning: "Capacity utilization is recovering while qualification costs are easing.", check: { claim: "Gross margin stays above 55%", metric: "Gross margin", direction: "above", threshold: 55, unit: "%", byDate: "2027-03-31", confidence: 70 } },
    { judgment: "I expect inventory days to fall below 50 days.", reasoning: "The new orders should reduce the finished goods backlog.", check: { claim: "Inventory days fall below 50", metric: "Inventory days", direction: "below", threshold: 50, unit: " days", byDate: "2026-10-01" }, outcome: { observed: 54, observedAt: "2026-10-02", result: "did_not", note: "The quarterly filing reported 54 inventory days." } },
    { judgment: "I think operating margin will exceed 16%.", reasoning: "Higher shipments could spread fixed costs across more units.", check: { claim: "Operating margin exceeds 16%", metric: "Operating margin", direction: "above", threshold: 16, unit: "%", byDate: "2026-10-01" }, outcome: { observed: 14.8, observedAt: "2026-10-02", result: "did_not", note: "Qualification costs continued during the quarter." } },
  ].map((entry, index) => ({ ...entry, loupistId: "loupist-quality", journalId: `preview-journal-${index}`, researchId: "", originChatId: "preview-chat", entities: [], createdAtMs: Math.min(FIXTURE_TIME - (index + 1) * 86400000, Date.parse(entry.check.byDate) - 86400000), updatedAtMs: FIXTURE_TIME, archived: false, inputDigest: "preview", inputs: { sources: [], worlds: [], lenses: [], earlier: [] } })) as import("../../types").JournalRecord[];
  if (previewKo()) for (const [index, copy] of [
    ["Hanul의 매출총이익률이 1분기까지 55% 위로 유지될 것 같아요.", "가동률이 회복되고 인증 비용은 줄고 있어요."],
    ["재고일수가 50일 아래로 줄어들 것 같아요.", "새 주문이 완제품 재고를 줄일 것 같아요."],
    ["영업이익률이 16%를 넘을 것 같아요.", "출하가 늘면 고정비가 더 많은 수량에 나뉘어요."],
  ].entries()) {
    entries[index].judgment = copy[0];
    entries[index].reasoning = copy[1];
    if (entries[index].outcome?.note) entries[index].outcome.note = index === 1 ? "분기 보고서의 재고일수는 54일이에요." : "분기 동안 인증 비용이 이어졌어요.";
  }
  bridge.journalList = async () => structuredClone(entries);
  const read = bridge.knowledgeJournalRead.bind(bridge);
  bridge.knowledgeJournalRead = async request => structuredClone(entries.find(entry => entry.journalId === request.journalId) ?? await read(request));
}
