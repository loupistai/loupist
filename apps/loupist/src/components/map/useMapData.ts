import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { readKnowledgeMap } from "../../knowledge/mapRead";
import type { LoupistBridge, KnowledgeLocation, WorldGraphView, WorldGraphQuery } from "../../types";
import { readWithWorldRetry } from "../../knowledge/readWithWorldRetry";
import type { KnowledgeMap } from "../../contracts/knowledge";

const WEEK_MS = 7 * 24 * 60 * 60 * 1000;

/** Nodes on the canvas whose first appearance is inside the last seven days. */
export function weeklyDrawnCount(nodes: { firstSeenAtMs?: number }[], now = Date.now()) {
  const start = now - WEEK_MS;
  const count = nodes.filter(node => node.firstSeenAtMs != null && node.firstSeenAtMs >= start && node.firstSeenAtMs <= now).length;
  return Math.min(count, nodes.length);
}

export function useMapData(bridge: LoupistBridge, loupistId: string, refreshKey: unknown, active = true, catalogRevision?: string) {
  const [result, setResult] = useState<{ loupistId: string; map: KnowledgeMap; arrivedAt: number }>();
  const [error, setError] = useState(false);
  const [loading, setLoading] = useState(true);
  const [retryKey, setRetryKey] = useState(0);
  const retry = useCallback(() => setRetryKey(n => n + 1), []);
  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    setLoading(true); setError(false);
    void readWithWorldRetry(() => readKnowledgeMap(bridge, loupistId, { fresh: true }), () => !cancelled).then((map) => {
      if (!cancelled) setResult({ loupistId, map, arrivedAt: performance.now() });
    }).catch(() => { if (!cancelled) setError(true); }).finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [bridge, loupistId, refreshKey, retryKey, active, catalogRevision]);
  return { arrivedAt: result?.loupistId === loupistId ? result.arrivedAt : undefined, map: result?.loupistId === loupistId ? result.map : undefined, loading, error, retry };
}

type WorldRead = Partial<Pick<WorldGraphQuery, "centerNodeId" | "centerClaimId" | "search" | "statuses" | "nodeLimit" | "edgeLimit">>;

/** Live first-page reads. A World page applies the results when its overview has caught up. */
export function useWorldReads(bridge: LoupistBridge, location: KnowledgeLocation, reads: WorldRead[], advance?: () => void) {
  const { loupistId, recordId, revisionId } = location;
  const queryKey = JSON.stringify(reads);
  const key = JSON.stringify([loupistId, recordId, queryKey, revisionId]);
  const [result, setResult] = useState<{ key: string; graphs: WorldGraphView[] }>();
  const [errorKey, setErrorKey] = useState<string>();
  const [retryKey, retry] = useState(0);
  useEffect(() => {
    const queries = JSON.parse(queryKey) as WorldRead[];
    if (!queries.length || !recordId || !revisionId) return;
    let active = true;
    setErrorKey(undefined);
    void readWithWorldRetry(() => Promise.all(queries.map(query => bridge.knowledgeWorldQuery({
      loupistId, recordId, revisionId, search: "", kinds: [], relations: [], bindingRoles: [], statuses: [],
      byteLimit: 524288, nodeLimit: 100, edgeLimit: 100, ...query,
    }))), () => active).then(graphs => {
      if (!active) return;
      if (graphs.some(graph => graph.recordId !== recordId)) {
        setErrorKey(key);
      } else if (advance && graphs.some(graph => graph.revisionId !== revisionId)) {
        const next = graphs[0]?.revisionId;
        if (graphs.every(graph => graph.revisionId === next)) {
          setResult({ key: JSON.stringify([loupistId, recordId, queryKey, next]), graphs });
        }
        advance();
      } else {
        setResult({ key, graphs });
      }
    }).catch(() => { if (active) setErrorKey(key); });
    return () => { active = false; };
  }, [bridge, loupistId, recordId, revisionId, key, queryKey, retryKey, advance]);
  return { graphs: result?.key === key ? result.graphs : undefined, error: errorKey === key, retry: () => retry(n => n + 1) };
}

export function useMapDetail(bridge: LoupistBridge, location: KnowledgeLocation, selectedId?: string, kind: "node" | "claim" = "node", advance?: () => void) {
  const { graphs, error, retry } = useWorldReads(bridge, location, selectedId ? [
    kind === "node" ? { centerNodeId: selectedId } : { centerClaimId: selectedId },
  ] : [], advance);
  return { graph: graphs?.[0], error, retry };
}

/** Restore the inspector's scroll if loading a newly published revision shortened its contents. */
export function useInspectorScroll(id: string, ready: boolean) {
  const ref = useRef<HTMLElement>(null);
  const scroll = useRef(0);
  useLayoutEffect(() => {
    scroll.current = 0;
    if (ref.current) ref.current.scrollTop = 0;
  }, [id]);
  useLayoutEffect(() => {
    if (ready && ref.current) ref.current.scrollTop = scroll.current;
  }, [id, ready]);
  return {
    ref,
    onScroll: () => {
      if (ready && ref.current) scroll.current = ref.current.scrollTop;
    },
  };
}
