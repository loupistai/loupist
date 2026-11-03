import type { LoupistBridge, KnowledgeLocation } from "../../types";
export type LibraryTab = "source" | "note";
export type LibraryState = { open: boolean; tab: LibraryTab; views: Partial<Record<LibraryTab, KnowledgeLocation>> };
// Transient navigation only: survives Settings and failed Loupist switches without
// writing knowledge or leaking one bridge/session's state into another.
const sessions = new WeakMap<LoupistBridge, Map<string, LibraryState>>();
export function libraryState(bridge: LoupistBridge, loupistId: string): LibraryState {
  let loupists = sessions.get(bridge);
  if (!loupists) { loupists = new Map(); sessions.set(bridge, loupists); }
  let state = loupists.get(loupistId);
  if (!state) { state = { open: false, tab: "source", views: {} }; loupists.set(loupistId, state); }
  return state;
}
