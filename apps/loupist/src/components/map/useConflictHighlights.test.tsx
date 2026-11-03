import { renderHook, waitFor } from "@testing-library/react";
import { PreviewBridge, prepareConflictsFixture } from "../../preview/fixtures";
import { useConflictHighlights } from "./useConflictHighlights";
it("recovers conflict nodes and edges omitted by the bounded Map using exact reads", async () => {
  const bridge = new PreviewBridge({delay_ms:0}); await prepareConflictsFixture(bridge);
  const full = await bridge.knowledgeMap({loupistId:"loupist-quality"});
  const base = {...full,nodes:full.nodes.filter(node=>node.id!=="preview-node-1"),edges:full.edges.filter(edge=>edge.source!=="preview-node-1" && edge.target!=="preview-node-1")};
  const read = vi.spyOn(bridge,"knowledgeWorldQuery");
  const {result} = renderHook(()=>useConflictHighlights(bridge,{loupistId:"loupist-quality",surface:"world",recordId:"preview-world",revisionId:"preview-revision",view:{highlightEntityIds:["preview-node-0","preview-node-1"]}},base));
  await waitFor(()=>expect(result.current.map?.nodes.some(node=>node.id==="preview-node-1")).toBe(true));
  expect(result.current.map?.edges.filter(edge=>edge.status==="disputed")).toHaveLength(2);
  expect(read).toHaveBeenCalledWith(expect.objectContaining({centerNodeId:"preview-node-1",statuses:["disputed"],recordId:"preview-world",revisionId:"preview-revision"}));
});
