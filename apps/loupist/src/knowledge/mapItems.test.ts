import { claimIdForEdge, entityLabel, matchesMapQuery } from "./mapItems";
import type { KnowledgeMapEdge } from "../contracts/knowledge";

const named = { label: "Hanbit Battery", localNames: ["한빛배터리", "東京"] };
it("shows and finds a Hangul alias for Korean and the canonical name otherwise", () => {
  expect(entityLabel(named, "ko")).toBe("한빛배터리");
  expect(entityLabel(named, "ko-KR")).toBe("한빛배터리");
  expect(entityLabel({ label: "Hanbit Battery", localNames: ["412340", "Hanbit"] }, "ko")).toBe("Hanbit Battery");
  expect(entityLabel({ label: "Hanbit Battery" }, "ko")).toBe("Hanbit Battery");
  expect(entityLabel(named, "en")).toBe("Hanbit Battery");
  expect(entityLabel(named, "en-US")).toBe("Hanbit Battery");
  expect(matchesMapQuery(named, "한빛")).toBe(true);
  expect(matchesMapQuery(named, "HANBIT")).toBe(true);
  expect(matchesMapQuery({ label: "Hanbit Battery" }, "한빛")).toBe(false);
});

it("reads the claim behind a projected edge and keeps any other edge id", () => {
  const edge = (id: string, target: string) => ({ id, source: "a", target, kind: "claim" }) as KnowledgeMapEdge;
  expect(claimIdForEdge(edge('claim-1:"node-b"', "node-b"))).toBe("claim-1");
  expect(claimIdForEdge(edge("claim-2", "node-b"))).toBe("claim-2");
});
