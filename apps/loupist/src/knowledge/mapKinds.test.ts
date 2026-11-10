import { entityCategory, extensionKindLabel, isMetricNode, nodeCategory, nodeRadius } from "./mapKinds";
import type { KnowledgeMapNode } from "../contracts/knowledge";
const node: KnowledgeMapNode = { id: "one", label: "One", kind: "entity", degree: 0, watched: false, held: false };
it("maps core entity kinds to legend categories and everything else to Other", () => {
  const cases = [["ORGANIZATION", "company"], ["  Person ", "person"], ["product", "product"], ["Technology", "technology"], [" Segment ", "industry"], ["Asset", "market"], ["metric", "event"], ["company", "other"], ["new-kind", "other"], [undefined, "other"]] as const;
  for (const [kind, expected] of cases) expect(entityCategory(kind)).toBe(expected);
});
it("uses an extension category and its language label", () => {
  const extended = { ...node, entityKind: "ProcessNode", categoryOverride: "product" as const, kindLabels: { en: "Process node", ko: "공정 노드" } };
  expect(nodeCategory(extended)).toBe("product");
  expect(nodeCategory({ ...node, entityKind: "ProcessNode" })).toBe("other");
  expect(extensionKindLabel(extended.kindLabels, "ko-KR", "Product")).toBe("공정 노드");
  expect(extensionKindLabel({ en: "Process node" }, "ko", "Product")).toBe("Process node");
  expect(extensionKindLabel(undefined, "en", "Product")).toBe("Product");
});
it("bounds radii and draws only a core metric as a fixed small dot", () => {
  expect(nodeRadius(node)).toBe(5);
  expect(nodeRadius({ ...node, degree: 10000 })).toBe(16);
  const metric = { ...node, entityKind: " Metric ", degree: 100 };
  expect(isMetricNode(metric)).toBe(true);
  expect(nodeRadius(metric)).toBe(4);
  const extension = { ...node, entityKind: "metric", categoryOverride: "event" as const, kindLabels: { en: "Capacity plan" }, degree: 4 };
  expect(isMetricNode(extension)).toBe(false);
  expect(nodeRadius(extension)).toBe(8);
});
