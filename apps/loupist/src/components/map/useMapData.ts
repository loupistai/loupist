import { useEffect, useState } from "react";
import { readKnowledgeMap } from "../../knowledge/mapRead";
import type { LoupistBridge, KnowledgeLocation, WorldGraphView } from "../../types";
import type { KnowledgeMap } from "../../contracts/knowledge";

const WEEK_MS = 7 * 24 * 60 * 60 * 1000;

/** Nodes on the canvas whose first appearance is inside the last seven days. */
export function weeklyDrawnCount(nodes: { firstSeenAtMs?: number }[], now = Date.now()) {
  const start = now - WEEK_MS;
  const count = nodes.filter(node => node.firstSeenAtMs != null && node.firstSeenAtMs >= start && node.firstSeenAtMs <= now).length;
  return Math.min(count, nodes.length);
}

export function useMapData(bridge: LoupistBridge, loupistId: string, refreshKey: unknown, active = true) {
  const [result, setResult] = useState<{ loupistId: string; map: KnowledgeMap; arrivedAt: number }>();
  const [error, setError] = useState(false);
  const [loading, setLoading] = useState(true);
  const [retryKey, setRetryKey] = useState(0);
  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    setLoading(true); setError(false);
    void readKnowledgeMap(bridge, loupistId, { fresh: true }).then((map) => {
      if (!cancelled) setResult({ loupistId, map, arrivedAt: performance.now() });
    }).catch(() => { if (!cancelled) setError(true); }).finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [bridge, loupistId, refreshKey, retryKey, active]);
  return { arrivedAt: result?.loupistId === loupistId ? result.arrivedAt : undefined, map: result?.loupistId === loupistId ? result.map : undefined, loading, error, retry: () => setRetryKey((n) => n + 1) };
}

export function useMapDetail(bridge: LoupistBridge, location: KnowledgeLocation, selectedId?: string, kind: "node" | "claim" = "node") {
  const { loupistId, recordId, revisionId } = location;
  const key = JSON.stringify([loupistId, recordId, revisionId, selectedId, kind]);
  const [result, setResult] = useState<{ key: string; graph: WorldGraphView }>();
  const [errorKey, setErrorKey] = useState<string>();
  const [retryKey, retry] = useState(0);
  useEffect(() => {
    if (!selectedId || !recordId || !revisionId) return;
    let cancelled = false;
    setErrorKey(undefined);
    void bridge.knowledgeWorldQuery({ loupistId, recordId, revisionId, ...(kind === "node" ? { centerNodeId: selectedId } : { centerClaimId: selectedId }), search: "", kinds: [], relations: [], bindingRoles: [], statuses: [], byteLimit: 524288, nodeLimit: 100, edgeLimit: 100 }).then((graph) => {
      if (!cancelled && graph.recordId === recordId && graph.revisionId === revisionId) setResult({ key, graph });
    }).catch(() => { if (!cancelled) setErrorKey(key); });
    return () => { cancelled = true; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` names the query inputs.
  }, [bridge, key, retryKey]);
  return { graph: result?.key === key ? result.graph : undefined, error: errorKey === key, retry: () => retry((n) => n + 1) };
}
