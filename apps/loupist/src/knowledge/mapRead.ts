import type { LoupistBridge } from "../types";
import type { KnowledgeMap } from "../contracts/knowledge";

/** The node budget every whole-Map reader asks for. */
export const MAP_READ_NODES = 1000;

const inFlight = new WeakMap<LoupistBridge, Map<string, Promise<KnowledgeMap>>>();

/**
 * Reads a Loupist's whole Map. Surfaces that mount together (the Map, Memory, Schedule and
 * asset names) share one read while it is in flight; a read that starts after it settles
 * asks the host again, so no surface sees an older Map than a fresh read would return.
 * `fresh` always asks the host (after a change) and becomes the read later callers share.
 * Callers treat the result as read-only.
 */
export function readKnowledgeMap(bridge: LoupistBridge, loupistId: string, { fresh = false } = {}): Promise<KnowledgeMap> {
  const byLoupist = inFlight.get(bridge) ?? new Map<string, Promise<KnowledgeMap>>();
  inFlight.set(bridge, byLoupist);
  const pending = byLoupist.get(loupistId);
  if (pending && !fresh) return pending;
  const read = bridge.knowledgeMap({ loupistId, maxNodes: MAP_READ_NODES });
  byLoupist.set(loupistId, read);
  const settle = () => { if (byLoupist.get(loupistId) === read) byLoupist.delete(loupistId); };
  read.then(settle, settle);
  return read;
}
