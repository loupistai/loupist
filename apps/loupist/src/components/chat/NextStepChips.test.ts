import type { GrowthEvent, KnowledgeMap } from "../../contracts/knowledge";
import { nextStepQuestions } from "./NextStepChips";

const node = (id: string, label: string, extra: Record<string, unknown> = {}) => ({ id, label, kind: "entity", degree: 1, watched: false, ...extra });
const company = (id: string, label: string, extra: Record<string, unknown> = {}) => node(id, label, { entityKind: "Organization", ...extra });
const map = { nodes: [
  company("hanul", "Hanul Precision", { watched: true }), company("morrow", "Morrow Components", { held: true }) /* an older host's flag; it carries no meaning now */, company("tsmc", "TSMC", { watched: true }),
  company("unfollowed", "Quiet Holdings"), node("margin", "Operating margin", { entityKind: "Metric" }),
  node("usd", "USD", { entityKind: "Asset" }), node("california", "California", { entityKind: "Place" }), node("semis", "Semiconductors", { entityKind: "Segment" }),
  node("mystery", "Something", { entityKind: "Gadget" }), node("unclassed", "Thing"), company("listed-ext", "Extension Co", { watched: true, categoryOverride: "company", kindLabels: { en: "Fund" } }),
], edges: [{ id: "c1", kind: "claim", source: "hanul", target: "margin" }, { id: "c2", kind: "claim", source: "usd", target: "california" }], worldRevisionId: "rev" } as unknown as KnowledgeMap;
const event = (kind: GrowthEvent["kind"], subjectId: string): GrowthEvent => ({ eventId: `${kind}:${subjectId}`, loupistId: "g", atMs: 1, kind, subjectId, originChatId: "chat", originMessageId: "answer", recordId: "world", actor: "loupist", summary: "", beforeSummary: null, afterSummary: null });

it("names a conflict first, then the followed companies an answer added, as noun phrases, at most three and never a metric", () => {
  const questions = nextStepQuestions(map, [event("entity_added", "margin"), event("entity_added", "morrow"), event("claim_disputed", "c1"), event("entity_added", "hanul"), event("entity_added", "tsmc")], "en");
  expect(questions).toEqual(["Which report on Hanul Precision holds", "More on TSMC"]);
  expect(nextStepQuestions(map, [], "en")).toEqual([]);
  expect(nextStepQuestions(null, [event("entity_added", "hanul")], "en")).toEqual([]);
});

it("offers nothing for a currency, place, sector, unknown kind or a company the user does not follow", () => {
  const added = ["usd", "california", "semis", "mystery", "unclassed", "unfollowed", "listed-ext"].map((id) => event("entity_added", id));
  expect(nextStepQuestions(map, added, "en")).toEqual([]);
  expect(nextStepQuestions(map, [event("claim_disputed", "c2")], "en")).toEqual([]);
});
