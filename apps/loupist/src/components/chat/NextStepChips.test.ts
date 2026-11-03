import type { GrowthEvent, KnowledgeMap } from "../../contracts/knowledge";
import { nextStepQuestions } from "./NextStepChips";

const node = (id: string, label: string, extra: Record<string, unknown> = {}) => ({ id, label, kind: "entity", degree: 1, watched: false, held: false, ...extra });
const map = { nodes: [node("hanul", "Hanul Precision"), node("morrow", "Morrow Components"), node("margin", "Operating margin", { entityKind: "Metric" }), node("tsmc", "TSMC")],
  edges: [{ id: "c1", kind: "claim", source: "hanul", target: "margin" }], worldRevisionId: "rev" } as unknown as KnowledgeMap;
const event = (kind: GrowthEvent["kind"], subjectId: string): GrowthEvent => ({ eventId: `${kind}:${subjectId}`, loupistId: "g", atMs: 1, kind, subjectId, originChatId: "chat", originMessageId: "answer", recordId: "world", actor: "loupist", summary: "", beforeSummary: null, afterSummary: null });

it("asks about a conflict first, then the names an answer added, at most three and never a metric", () => {
  const questions = nextStepQuestions(map, [event("entity_added", "margin"), event("entity_added", "morrow"), event("claim_disputed", "c1"), event("entity_added", "hanul"), event("entity_added", "tsmc")], "en");
  expect(questions).toEqual(["Which side is right about Hanul Precision?", "Tell me more about Morrow Components", "Tell me more about TSMC"]);
  expect(nextStepQuestions(map, [], "en")).toEqual([]);
  expect(nextStepQuestions(null, [event("entity_added", "hanul")], "en")).toEqual([]);
});
