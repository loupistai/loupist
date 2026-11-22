import { useMemo } from "react";
import type { LoupistBridge, KnowledgeLocation } from "../../types";
import type { KnowledgeMap } from "../../contracts/knowledge";
import { claimDegree, projectClaim } from "../../knowledge/mapItems";

import { useWorldReads } from "./useMapData";

/** Exact-read briefing pointers omitted by the default bounded Map projection. */
export function useConflictHighlights(bridge: LoupistBridge, location: KnowledgeLocation, base?: KnowledgeMap, advance?: () => void) {
  const ids = useMemo(() => [...new Set(location.view?.highlightEntityIds ?? [])].slice(0, 20), [location.view?.highlightEntityIds]);
  const missing = useMemo(() => ids.filter(id => !base?.nodes.some(node => node.id === id) || !base.edges.some(edge => edge.status === "disputed" && (edge.source === id || edge.target === id))), [ids, base]);
  const { graphs, error, retry } = useWorldReads(bridge, location, base ? missing.map(centerNodeId => ({ centerNodeId, statuses: ["disputed"] })) : [], advance);
  const map = useMemo(() => {
    if (!base || !graphs) return base;
    const nodes = new Map(base.nodes.map(node => [node.id, node]));
    const edges = new Map(base.edges.map(edge => [edge.id, edge]));
    for (const graph of graphs) {
      for (const node of graph.nodes) if (ids.includes(node.id) && !nodes.has(node.id)) nodes.set(node.id, { id: node.id, label: node.name, kind: "entity", entityKind: node.kind, degree: claimDegree(graph.edges, node.id), watched: false });
      for (const { claim } of graph.edges) {
        if (claim.status !== "disputed") continue;
        for (const edge of projectClaim(claim)) {
          if (nodes.has(edge.source) && nodes.has(edge.target)) edges.set(edge.id, edge);
        }
      }
    }
    return { ...base, nodes: [...nodes.values()], edges: [...edges.values()] };
  }, [base, graphs, ids]);
  return { map, error, retry };
}
