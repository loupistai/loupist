import { useCallback, useEffect, useRef, useState } from "react";
import type { GraphCamera, KnowledgeLocation } from "../types";
import { backKnowledgeLocation, initialKnowledgeNavigation, openKnowledgeLocation, patchKnowledgeLocation, type KnowledgeNavigation } from "../knowledge/location";

type LoupistNavigation = { navigation: KnowledgeNavigation; visited: boolean };

/** Each Loupist keeps its own views and Reader trail for the lifetime of the app. */
export function useKnowledgeNavigation(loupistId: string | null, active: boolean) {
  const [loupists, setLoupists] = useState<Record<string, LoupistNavigation>>({});
  const currentLoupist = useRef(loupistId);
  currentLoupist.current = loupistId;
  useEffect(() => {
    if (!loupistId) return;
    setLoupists((current) => {
      const saved = current[loupistId];
      if (saved && (saved.visited || !active)) return current;
      return { ...current, [loupistId]: { navigation: saved?.navigation ?? initialKnowledgeNavigation(loupistId), visited: active || Boolean(saved?.visited) } };
    });
  }, [loupistId, active]);
  const patch = useCallback((next: KnowledgeLocation, camera?: GraphCamera) => {
    if (next.loupistId !== currentLoupist.current) return;
    setLoupists((current) => ({ ...current, [next.loupistId]: { visited: true,
      navigation: patchKnowledgeLocation(current[next.loupistId]?.navigation ?? initialKnowledgeNavigation(next.loupistId), next, camera) } }));
  }, []);
  const open = useCallback((next: KnowledgeLocation, camera?: GraphCamera) => {
    if (next.loupistId !== currentLoupist.current) return;
    setLoupists((current) => ({ ...current, [next.loupistId]: { visited: true,
      navigation: openKnowledgeLocation(current[next.loupistId]?.navigation ?? initialKnowledgeNavigation(next.loupistId), next, camera) } }));
  }, []);
  const back = useCallback(() => {
    const id = currentLoupist.current;
    if (!id) return;
    setLoupists((current) => current[id] ? { ...current, [id]: { ...current[id], navigation: backKnowledgeLocation(current[id].navigation) } } : current);
  }, []);
  const current = loupistId ? loupists[loupistId] : undefined;
  return { navigation: current?.navigation ?? null, visited: current?.visited ?? false, patch, open, back };
}
