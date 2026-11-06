import type { KnowledgeMap, KnowledgeMapEdge, KnowledgeMapNode } from "../contracts/knowledge";
import { prepareFixture, type PreviewBridge } from "./fixtures";

const seen = Date.parse("2026-09-14T00:00:00Z");

const entities: Array<Pick<KnowledgeMapNode, "id" | "label" | "entityKind" | "localNames">> = [
  { id: "core-hanbit", label: "Hanbit Battery", entityKind: "Organization", localNames: ["한빛배터리"] },
  { id: "core-kim", label: "Kim Seo-yeon", entityKind: "Person", localNames: ["김서연"] },
  { id: "core-kosdaq", label: "KOSDAQ", entityKind: "Asset", localNames: ["코스닥"] },
  { id: "core-lfp", label: "LFP cathode material", entityKind: "Product" },
  { id: "core-chungju", label: "Chungju plant", entityKind: "Place", localNames: ["충주 공장"] },
  { id: "core-policy", label: "Clean Vehicle Content Rule", entityKind: "Policy" },
  { id: "core-event", label: "Northgate Motors supply agreement announced 2026-09-14", entityKind: "Event" },
  { id: "core-metric", label: "Hanbit Battery Q2 2026 revenue KRW 123.4 billion", entityKind: "Metric" },
  { id: "core-program", label: "Northgate supply program", entityKind: "Entity" },
  { id: "core-northgate", label: "Northgate Motors", entityKind: "Organization" },
];

/** One claim per pair. COVERS is the open relation: the program is the arrangement the announcement describes. */
const links: Array<[string, string, string]> = [
  ["core-kim", "core-hanbit", "LEADS"],
  ["core-hanbit", "core-lfp", "OFFERS"],
  ["core-hanbit", "core-chungju", "LOCATED_IN"],
  ["core-hanbit", "core-kosdaq", "LISTED_ON"],
  ["core-hanbit", "core-metric", "REPORTS"],
  ["core-policy", "core-hanbit", "AFFECTS"],
  ["core-hanbit", "core-northgate", "SUPPLIES"],
  ["core-event", "core-northgate", "AFFECTS"],
  ["core-program", "core-event", "COVERS"],
];

const count = (total: number) => ({ total, shown: total, truncated: false });

function coreSchemaMap(): KnowledgeMap {
  const degree = new Map<string, number>();
  const edges: KnowledgeMapEdge[] = links.map(([source, target, relation], index) => {
    degree.set(source, (degree.get(source) ?? 0) + 1);
    degree.set(target, (degree.get(target) ?? 0) + 1);
    return { id: `core-claim-${index}`, kind: "claim", source, target, relation, status: "active", firstSeenAtMs: seen };
  });
  const nodes: KnowledgeMapNode[] = entities.map(entity => ({
    id: entity.id, kind: "entity", label: entity.label, localNames: entity.localNames, entityKind: entity.entityKind,
    degree: degree.get(entity.id) ?? 0, firstSeenAtMs: seen, watched: false, held: false,
  }));
  const none = count(0);
  return {
    nodes, edges, unmatchedHoldings: [], unmatchedWatchlist: [],
    counts: {
      entity: count(nodes.length), lens_perspective: { ...none }, lens_principle: { ...none }, lens_method: { ...none }, lens_subject: { ...none }, lens_idea: { ...none }, journal: { ...none },
      claim: count(edges.length), lens_link: { ...none }, applies_to: { ...none }, about: { ...none },
    },
    worldRevisionId: "core-schema-revision", generatedAtMs: seen,
  };
}

export async function prepareCoreSchema(bridge: PreviewBridge) {
  await prepareFixture(bridge);
  bridge.knowledgeMap = async () => coreSchemaMap();
}
