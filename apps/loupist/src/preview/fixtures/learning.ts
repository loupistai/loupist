/** Preview fixture: one World and Lens history for the learning captures. */
import { companies, prepareFixture, PreviewBridge, previewEdges, previewWorldPage, revision } from "../fixtures";

/** One coherent World/Lens history for the learning UI capture matrix. */
export async function prepareLearningFixture(bridge: PreviewBridge, evidenceCount = 6) {
  await prepareFixture(bridge);
  const title = "Test whether capacity costs are temporary";
  const refinedTitle = "Check cash conversion across a capacity ramp";
  const at = Date.now() - 3600000;
  const reviews: import("../../contracts/knowledge").LearningReview[] = [
    { reviewId: "learning-created", status: "created", lensRecordId: "map-lens-2", evidenceCount, createdAtMs: at, rationale: "Across three companies, low utilization and temporary qualification costs preceded margin recovery. Compare cash conversion before calling the pressure temporary." },
    { reviewId: "learning-refined", status: "refined", lensRecordId: "map-lens-1", evidenceCount: 4, createdAtMs: at - 86400000, rationale: "Inventory rose faster than sales, so the method now checks working capital." },
    { reviewId: "learning-deferred", status: "deferred", evidenceCount: 0, createdAtMs: at - 2 * 86400000, reason: "insufficient_turns" },
    { reviewId: "learning-failed", status: "failed", evidenceCount: 0, createdAtMs: at - 3 * 86400000, reason: "not_sent" },
  ];
  bridge.learningReviewList = async ({ limit }) => structuredClone(reviews.slice(0, limit ?? 50));
  const catalog = await bridge.knowledgeCatalog({ loupistId: "loupist-quality" });
  catalog.lenses.push(...[title, refinedTitle].map((name, index) => ({ ...revision, recordId: `map-lens-${2 - index}`, title: name, lensKind: "method" as const, kind: "lens" as const, learned: true, updatedAtMs: at - index * 86400000, objects: [] })));
  bridge.knowledgeCatalog = async ({ loupistId }) => structuredClone({ ...catalog, loupistId });
  const baseMap = await bridge.knowledgeMap({ loupistId: "loupist-quality" });
  baseMap.nodes = baseMap.nodes.filter(node => node.kind !== "lens_principle");
  baseMap.counts.lens_principle = { shown: 0, total: 0, truncated: false };
  const learned = baseMap.nodes.find(node => node.id === "map-lens-2")!;
  learned.label = title; learned.learnedAtMs = at; learned.learnedRationale = reviews[0].rationale!;
  const refined = baseMap.nodes.find(node => node.id === "map-lens-1")!;
  refined.label = refinedTitle; refined.learnedAtMs = at - 86400000; refined.learnedRationale = reviews[1].rationale!;
  const evidenceNodes = baseMap.nodes.filter(node => node.kind === "entity").slice(0, 6);
  evidenceNodes.forEach((node, i) => { node.label = companies[i]; });
  const statements = ["Operating margin fell during a new production line’s qualification.", "Utilization reached 68% as qualification costs began to decline.", "Inventory days fell as utilization recovered in the next quarter.", "Temporary ramp labor accounted for most of the margin decline.", "Cash conversion improved alongside higher utilization.", "Pricing remained stable while ramp costs and scrap declined."];
  const claims = statements.slice(0, evidenceCount).map((proposition, i) => ({ ...previewEdges[i], id: `learning-claim-${i}`, endpoints: [{ nodeId: evidenceNodes[i].id, role: "subject" }], claim: { ...previewEdges[i].claim, id: `learning-claim-${i}`, proposition, endpoints: [{ nodeId: evidenceNodes[i].id, role: "subject" }], publicationTime: { precision: "date" as const, value: `2026-10-${20 + i}` } } }));
  bridge.knowledgeWorldQuery = async request => structuredClone({ recordId: request.recordId, ...revision, revisionId: request.revisionId, nodes: evidenceNodes.map(node => ({ id: node.id, name: node.label, aliases: [], description: "", kind: "Organization", sourceRole: "organization", properties: {} })), edges: claims, coverage: "sealed_snapshot" as const, truncated: false, ...previewWorldPage });
  bridge.knowledgeMap = async () => structuredClone(baseMap);
  const originalRead = bridge.knowledgeLensRead.bind(bridge);
  bridge.knowledgeLensRead = async request => ({ ...await originalRead(request), title: catalog.lenses.find(lens => lens.recordId === request.recordId)?.title ?? title, kind: request.recordId.startsWith("map-lens-") ? "method" : "perspective" });
  const growth: import("../../contracts/knowledge").GrowthEvent[] = [
    { eventId: "learning-revision", loupistId: catalog.loupistId, recordId: learned.recordId!, subjectId: learned.recordId!, kind: "lens_revised", atMs: at, actor: "loupist", originChatId: null, originMessageId: null, summary: "Added cash confirmation", beforeSummary: "Compare utilization and margin across the ramp. Separate qualification costs from recurring expenses.", afterSummary: "Compare utilization, margins and inventory days. Confirm recovery with operating cash flow, and test whether pricing or product mix explains the change.", rationale: "Margin recovery alone did not establish that demand had turned into cash.", origin: { kind: "learning_review", reviewId: reviews[0].reviewId } },
    { eventId: "learning-prior", loupistId: catalog.loupistId, recordId: learned.recordId!, subjectId: learned.recordId!, kind: "lens_revised", atMs: at - 86400000, actor: "loupist", originChatId: null, originMessageId: null, summary: "Separated ramp costs", beforeSummary: "Track reported margins.", afterSummary: "Compare utilization and margin across the ramp.", rationale: "Three companies reported temporary qualification costs." },
    { eventId: "learning-original", loupistId: catalog.loupistId, recordId: learned.recordId!, subjectId: learned.recordId!, kind: "lens_created", atMs: at - 2 * 86400000, actor: "loupist", originChatId: null, originMessageId: null, summary: title, beforeSummary: null, afterSummary: "Track reported margins." },
  ];
  bridge.knowledgeGrowthEvents = async () => ({ events: structuredClone(growth), nextCursor: null });
  return reviews[0];
}
