/** Preview fixture: paired attributed reports for the Map inspector and briefing links. */
import type { GraphEdge } from "../../types";
import { prepareFixture, PreviewBridge, previewEdges, previewNodes, previewWorldPage, revision, sourceTitles } from "../fixtures";

/** Paired attributed reports, shared by the inspector and briefing navigation previews. */
export async function prepareConflictsFixture(bridge: PreviewBridge) {
  await prepareFixture(bridge, "answer");
  const map = await bridge.knowledgeMap({ loupistId: "loupist-quality" });
  const nodes = structuredClone(previewNodes);
  const endpoints = [{ nodeId: nodes[0].id, role: "subject" }, { nodeId: nodes[1].id, role: "object" }];
  const edges = ["Hanul Precision expanded supplier capacity by 18% in the third quarter.", "Morrow Components reports unchanged capacity for Hanul Precision in the same quarter."].map((proposition, index): GraphEdge => ({
    id: `conflict-${index}`, relation: "SUPPLIES", endpoints, status: "disputed",
    claim: { id: `conflict-${index}`, relation: "SUPPLIES", endpoints, status: "disputed", proposition, attribution: sourceTitles[index], qualifiers: [], evidence: [{ spanId: `conflict-span-${index}`, role: "supports", attribution: sourceTitles[index] }], publicationTime: { precision: "date", value: `2026-10-${27 - index}` }, recordedAtMs: null, effectiveTime: null, validAt: null, invalidAt: null, expiredAt: null },
  }));
  map.nodes = nodes.map(node => ({ id: node.id, label: node.name, kind: "entity", entityKind: node.kind, degree: 3, watched: node.id === nodes[0].id, held: false }));
  map.edges = [...previewEdges.slice(2), ...edges].map(edge => ({ id: edge.id, kind: "claim", source: edge.endpoints[0].nodeId, target: edge.endpoints[1].nodeId, status: edge.status, relation: edge.relation }));
  map.worldRevisionId = "preview-conflicts";
  map.counts.entity = { total: nodes.length, shown: nodes.length, truncated: false };
  map.counts.claim = { total: map.edges.length, shown: map.edges.length, truncated: false };
  bridge.knowledgeMap = async () => structuredClone(map);
  bridge.knowledgeWorldQuery = async request => structuredClone({ recordId: request.recordId, ...revision, revisionId: request.revisionId, nodes, edges, coverage: "sealed_snapshot", truncated: false, ...previewWorldPage, sourceTargets: Object.fromEntries(edges.map((_, index) => [`conflict-span-${index}`, { recordId: "preview-sources", revisionId: revision.revisionId, sourceId: `preview-source-${index}`, spanId: `conflict-span-${index}`, worldRecordId: request.recordId, worldRevisionId: request.revisionId }])) });
  const inbox = bridge.inboxList.bind(bridge);
  bridge.inboxList = async request => { const page = await inbox(request); return { ...page, items: page.items.map(item => item.briefingContext ? { ...item, briefingContext: { ...item.briefingContext, disputedEntityIds: endpoints.map(endpoint => endpoint.nodeId) } } : item) }; };
  const runs = bridge.scheduledTaskRuns.bind(bridge);
  bridge.scheduledTaskRuns = async request => (await runs(request)).map(run => run.briefing_context ? { ...run, briefing_context: { ...run.briefing_context, disputedEntityIds: endpoints.map(endpoint => endpoint.nodeId) } } : run);
}
