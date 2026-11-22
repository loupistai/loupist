import { useEffect, useMemo, useState } from "react";
import type { LoupistBridge, KnowledgeLocation, WorldGraphView } from "../../types";
import type { KnowledgeMap } from "../../contracts/knowledge";
import { projectClaim } from "../../knowledge/mapItems";

/** Exact-read briefing pointers omitted by the default bounded Map projection. */
export function useConflictHighlights(bridge: LoupistBridge, location: KnowledgeLocation, base?: KnowledgeMap) {
  const [result, setResult] = useState<{ key: string; graphs: WorldGraphView[] }>();
  const [errorKey, setErrorKey] = useState<string>();
  const [retryKey, retry] = useState(0);
  const ids = useMemo(() => [...new Set(location.view?.highlightEntityIds ?? [])].slice(0, 20), [location.view?.highlightEntityIds]);
  const missing = useMemo(() => ids.filter(id => !base?.nodes.some(node => node.id === id) || !base.edges.some(edge => edge.status === "disputed" && (edge.source === id || edge.target === id))), [ids, base]);
  const key = JSON.stringify([location.loupistId, location.recordId, location.revisionId, missing]);
  useEffect(() => {
    if (!base || !missing.length || !location.recordId || !location.revisionId) return;
    let current = true;
    setErrorKey(undefined);
    void Promise.all(missing.map(centerNodeId => bridge.knowledgeWorldQuery({ loupistId: location.loupistId, recordId: location.recordId!, revisionId: location.revisionId!, centerNodeId, search: "", kinds: [], relations: [], bindingRoles: [], statuses: ["disputed"], nodeLimit: 100, edgeLimit: 100, byteLimit: 524288 }))).then(graphs => {
      if (current) setResult({ key, graphs: graphs.filter(graph => graph.recordId === location.recordId && graph.revisionId === location.revisionId) });
    }).catch(() => { if (current) setErrorKey(key); });
    return () => { current = false; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` names the location and the missing centres; the query runs once per key.
  }, [bridge, key, !!base, retryKey]);
  const map = useMemo(() => {
    if (!base || result?.key !== key) return base;
    const nodes = new Map(base.nodes.map(node => [node.id, node]));
    const edges = new Map(base.edges.map(edge => [edge.id, edge]));
    for (const graph of result.graphs) {
      for (const node of graph.nodes) if (ids.includes(node.id) && !nodes.has(node.id)) nodes.set(node.id, { id: node.id, label: node.name, kind: "entity", entityKind: node.kind, degree: graph.edges.filter(edge => edge.endpoints.some(end => end.nodeId === node.id)).length, watched: false });
      for (const { claim } of graph.edges) {
        if (claim.status !== "disputed") continue;
        for (const edge of projectClaim(claim)) {
          if (nodes.has(edge.source) && nodes.has(edge.target)) edges.set(edge.id, edge);
        }
      }
    }
    return { ...base, nodes: [...nodes.values()], edges: [...edges.values()] };
  }, [base, result, key, ids]);
  return { map, error: errorKey === key, retry: () => retry(value => value + 1) };
}
