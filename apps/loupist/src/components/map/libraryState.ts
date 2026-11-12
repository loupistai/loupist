import { useEffect, useRef } from "react";
import type { LoupistBridge, KnowledgeLocation } from "../../types";
export type LibraryTab = "source" | "journal" | "note";
/** The knowledge surface a Library tab reads: the Journal and research notes are both notes. */
export const librarySurface = (tab: LibraryTab) => (tab === "source" ? "source" : "note") as "source" | "note";
/** `requested`: a `requestLibrary` tab the Library has not shown yet; it wins over the reader's location on mount. */
export type LibraryState = { open: boolean; tab: LibraryTab; views: Partial<Record<LibraryTab, KnowledgeLocation>>; requested?: boolean };
// Transient navigation only: survives Settings and failed Loupist switches without
// writing knowledge or leaking one bridge/session's state into another.
const sessions = new WeakMap<LoupistBridge, Map<string, LibraryState>>();
const listeners = new WeakMap<LoupistBridge, Set<(loupistId: string) => void>>();
export function libraryState(bridge: LoupistBridge, loupistId: string): LibraryState {
  let loupists = sessions.get(bridge);
  if (!loupists) { loupists = new Map(); sessions.set(bridge, loupists); }
  let state = loupists.get(loupistId);
  if (!state) { state = { open: false, tab: "source", views: {} }; loupists.set(loupistId, state); }
  return state;
}

/**
 * Opens the Library on a tab, whether or not the Map is mounted: a closed Map reads it when it
 * mounts, an open one hears it at once. `journalNew` opens the Journal's new-entry form.
 */
export function requestLibrary(bridge: LoupistBridge, loupistId: string, tab: LibraryTab, options: { journalNew?: boolean } = {}) {
  const state = libraryState(bridge, loupistId);
  state.open = true;
  state.tab = tab;
  state.requested = true;
  if (options.journalNew) {
    const current = state.views.journal ?? { loupistId, surface: "note" as const };
    state.views = { ...state.views, journal: { ...current, view: { ...current.view, journalNew: true } } };
  }
  for (const listener of listeners.get(bridge) ?? []) listener(loupistId);
}

/** Runs `onRequest` with the Library's state each time `requestLibrary` targets this Loupist. */
export function useLibraryRequests(bridge: LoupistBridge, loupistId: string, onRequest: (state: LibraryState) => void) {
  const latest = useRef(onRequest);
  useEffect(() => { latest.current = onRequest; });
  useEffect(() => {
    let set = listeners.get(bridge);
    if (!set) { set = new Set(); listeners.set(bridge, set); }
    const listener = (id: string) => { if (id === loupistId) latest.current(libraryState(bridge, id)); };
    set.add(listener);
    return () => { set.delete(listener); };
  }, [bridge, loupistId]);
}
