import { useCallback, useEffect, useRef, useState, type SetStateAction } from "react";
import type {
  ChatArtifactRef,
  ChatEvidenceRef,
  LoupistBridge,
  KnowledgeLocation,
  WorkspaceUiState,
} from "../types";
import { parseCredentialFreeHttpUrl } from "../lib/credentialFreeUrl";
import { citationPageUrl } from "../chat/evidence";
import type { AppTab } from "./navigation";
import { chatSessionKey } from "../chat/sessionRegistry";
import {
  activateWorkspaceTab,
  badgeWorkspaceTab,
  EVIDENCE_TAB_ID,
  WORK_TAB_ID,
  workTab,
  emptyEvidenceTab,
  evidenceEntry,
  evidenceWorkspaceTab,
  filesWorkspaceTab,
  workWorkspaceTab,
  bindActiveBrowserPassage,
  browserWorkspaceTab,
  emptyWorkspaceSession,
  knowledgeEntry,
  moveEvidenceHistory,
  pushEvidenceEntry,
  stashEvidenceEntry,
  selectWorkspaceTab,
  type BrowserWorkspaceTab,
  type ChatWorkspaceSession,
  type ChatWorkspaceTab,
  type EvidenceEntry,
  type EvidenceStepLink,
  type WorkspacePlacement,
} from "../chat/workspace";
import { workspaceOverlayMedia } from "../components/workspace/useWorkspacePanelResize";
import { defaultWorkspaceLayout, restoreWorkspaceUiState, workspaceUiSnapshot, type DockKind, type WorkspaceLayout, type WorkspaceSessions } from "./workspaceUiState";

/** Where each content kind docks until the user chooses (side-panel.md P7). */
const DEFAULT_DOCK: Record<DockKind, WorkspacePlacement> = { chart: "bottom", reading: "side" };

export type VisibleWorkspace = {
  loupistId: string | null;
  threadId: string | null;
  tab: AppTab;
};

const isNativeBrowserTab = (
  tab: ChatWorkspaceTab,
): tab is BrowserWorkspaceTab =>
  tab.kind === "browser" && Boolean(tab.native_id);

/** The Map opens beside the chat by itself only when both fit side by side (`side-panel.md` §3.1). */
const MAP_REVEAL_MIN_WIDTH = 1280;

export type WorkspacePanelKind = "lens" | "world" | "materials";

/** Chat workspace panel: per-thread tab sessions, open state, and geometry. */
export function useWorkspacePanel(bridge: LoupistBridge) {
  const [kind, setKind] = useState<WorkspacePanelKind>("materials");
  const kindRef = useRef(kind);
  const changeKind = useCallback((next: WorkspacePanelKind) => { kindRef.current = next; setKind(next); }, []);
  const [selectedArtifacts, setSelectedArtifacts] = useState<Record<string, ChatArtifactRef | undefined>>({});
  const selectArtifactForChat = useCallback((key: string, artifact?: ChatArtifactRef) => {
    setSelectedArtifacts((current) => ({ ...current, [key]: artifact }));
  }, []);
  const consumeArtifactForChat = useCallback((key: string, artifact: ChatArtifactRef) => {
    setSelectedArtifacts((current) => current[key] === artifact ? { ...current, [key]: undefined } : current);
  }, []);
  const [layout, setLayout] = useState(defaultWorkspaceLayout);
  const layoutRef = useRef(layout);
  const { open, width: panelWidth, height: panelHeight, placement } = layout;
  const [hydrated, setHydrated] = useState(false);
  const hydratedRef = useRef(false);
  const changedLayoutFields = useRef(new Set<keyof WorkspaceLayout>());
  const pendingSessionChanges = useRef<((current: WorkspaceSessions) => WorkspaceSessions)[]>([]);
  const startup = useRef<{ bridge: LoupistBridge; read: Promise<WorkspaceUiState | null> } | null>(null);
  const lastQueued = useRef<string | null>(null);
  const writeQueue = useRef<{ busy: boolean; pending: WorkspaceUiState | null }>({ busy: false, pending: null });
  const updateLayout = useCallback(<K extends keyof WorkspaceLayout>(field: K, action: SetStateAction<WorkspaceLayout[K]>) => {
    const previous = layoutRef.current[field];
    const next = typeof action === "function" ? action(previous) : action;
    if (!hydratedRef.current) changedLayoutFields.current.add(field);
    if (next === previous) return;
    layoutRef.current = { ...layoutRef.current, [field]: next };
    setLayout(layoutRef.current);
  }, []);
  const setOpen = useCallback((next: SetStateAction<boolean>) => updateLayout("open", next), [updateLayout]);
  const setPanelWidth = useCallback((next: SetStateAction<number>) => updateLayout("width", next), [updateLayout]);
  const setPanelHeight = useCallback((next: SetStateAction<number>) => updateLayout("height", next), [updateLayout]);
  const setPlacement = useCallback((next: SetStateAction<WorkspacePlacement>) => updateLayout("placement", next), [updateLayout]);
  const [sessions, setSessions] = useState<
    Record<string, ChatWorkspaceSession>
  >({});
  const sessionsRef = useRef(sessions);
  const updateSessions = useCallback((action: SetStateAction<Record<string, ChatWorkspaceSession>>) => {
    const apply = (current: WorkspaceSessions) => {
      const next = typeof action === "function" ? action(current) : action;
      if (next === current) return current;
      // Keep touched sessions last so bounded persistence discards older readers first.
      const ordered = { ...next };
      for (const key of Object.keys(next)) if (current[key] !== next[key]) {
        delete ordered[key];
        ordered[key] = next[key];
      }
      return ordered;
    };
    const next = apply(sessionsRef.current);
    if (next === sessionsRef.current) return;
    if (!hydratedRef.current) pendingSessionChanges.current.push(apply);
    sessionsRef.current = next;
    setSessions(next);
  }, []);

  useEffect(() => {
    let active = true;
    // StrictMode reuses the same startup promise. Native handles are reset before
    // any restored browser tab can open a replacement child.
    if (startup.current?.bridge !== bridge) {
      startup.current = { bridge, read: bridge.browserTabsReset().then(() => bridge.workspaceUiRead()) };
    }
    void startup.current.read.then((saved) => {
      if (!active) return;
      const restored = restoreWorkspaceUiState(saved);
      lastQueued.current = JSON.stringify(workspaceUiSnapshot(restored.layout, restored.sessions));
      const nextLayout = { ...restored.layout };
      for (const field of changedLayoutFields.current) Object.assign(nextLayout, { [field]: layoutRef.current[field] });
      const nextSessions = pendingSessionChanges.current.reduce((current, apply) => apply(current), restored.sessions);
      layoutRef.current = nextLayout;
      sessionsRef.current = nextSessions;
      changedLayoutFields.current.clear();
      pendingSessionChanges.current = [];
      hydratedRef.current = true;
      setLayout(nextLayout);
      setSessions(nextSessions);
      setHydrated(true);
    }).catch(() => {
      // A failed read must never replace an existing layout with empty defaults.
      // Readers remain usable for this app session.
    });
    return () => { active = false; };
  }, [bridge]);

  useEffect(() => {
    if (!hydrated) return;
    const state = workspaceUiSnapshot(layout, sessions);
    const serialized = JSON.stringify(state);
    if (lastQueued.current === serialized) return;
    lastQueued.current = serialized;
    const queue = writeQueue.current;
    queue.pending = state;
    if (queue.busy) return;
    queue.busy = true;
    void (async () => {
      try {
        while (queue.pending) {
          const next = queue.pending;
          queue.pending = null;
          try { await bridge.workspaceUiWrite(next); }
          catch {
            if (lastQueued.current === JSON.stringify(next)) lastQueued.current = null;
          }
        }
      } finally { queue.busy = false; }
    })();
  }, [bridge, hydrated, layout, sessions]);
  const visibleRef = useRef<VisibleWorkspace>({
    loupistId: null,
    threadId: null,
    tab: "chat",
  });

  /** Pointer or key activity inside the panel; recent activity turns content arrival into a badge. */
  const lastInteraction = useRef(0);
  const noteInteraction = useCallback(() => { lastInteraction.current = Date.now(); }, []);

  const updateSession = useCallback((key: string, update: (session: ChatWorkspaceSession) => ChatWorkspaceSession) => {
    updateSessions((current) => {
      const previous = current[key] ?? emptyWorkspaceSession();
      const next = update(previous);
      return next === previous ? current : { ...current, [key]: next };
    });
  }, [updateSessions]);

  /** The user opened the panel in this chat: it may open by itself here again. */
  const show = useCallback((key?: string) => {
    if (key && sessionsRef.current[key]?.dismissed) {
      updateSession(key, (session) => {
        const next = { ...session };
        delete next.dismissed;
        return next;
      });
    }
    setOpen(true);
  }, [updateSession]);

  /** The user closed the panel in this chat: nothing reopens it until they open it again. */
  const dismiss = useCallback((key?: string) => {
    if (key && !sessionsRef.current[key]?.dismissed) updateSession(key, (session) => ({ ...session, dismissed: true }));
    setOpen(false);
  }, [updateSession]);

  /** A deep link the user followed: always switch to its tab. */
  const openTab = useCallback(
    (loupistId: string, threadId: string, tab: ChatWorkspaceTab) => {
      const key = chatSessionKey(loupistId, threadId);
      changeKind("materials");
      updateSession(key, (session) => activateWorkspaceTab(session, tab));
      show(key);
    },
    [changeKind, show, updateSession],
  );

  const openEvidenceEntry = useCallback((loupistId: string, threadId: string, entry: EvidenceEntry) => {
    const key = chatSessionKey(loupistId, threadId);
    changeKind("materials");
    updateSession(key, (session) => activateWorkspaceTab(session, pushEvidenceEntry(evidenceWorkspaceTab(session), entry)));
    show(key);
  }, [changeKind, show, updateSession]);

  /** Background publish from a run: opens or marks the panel only while this chat is on screen. */
  const publishArtifact = useCallback(
    (loupistId: string, threadId: string, artifact: ChatArtifactRef) => {
      // Charts and tables render as cards in the answer; only documents belong beside the chat.
      if (artifact.kind !== "markdown") return;
      const key = chatSessionKey(loupistId, threadId);
      const session = sessionsRef.current[key];
      const artifactTab = workWorkspaceTab(artifact, threadId);
      const visible = visibleRef.current;
      const onScreen = visible.tab === "chat" && visible.loupistId === loupistId && visible.threadId === threadId;
      // The overlay sheet would take keyboard focus, so it never opens by itself.
      const canOpen = kindRef.current === "materials" && layoutRef.current.open && onScreen && !session?.dismissed && !window.matchMedia(workspaceOverlayMedia).matches;
      const open = layoutRef.current.open;
      const reading = open && onScreen && Date.now() - lastInteraction.current < 10_000 && session?.active_tab_id !== artifactTab.id;
      if (!canOpen || reading) {
        updateSession(key, (current) => badgeWorkspaceTab(current, artifactTab));
        return;
      }
      updateSession(key, (current) => activateWorkspaceTab(current, artifactTab));
    },
    [updateSession],
  );

  /**
   * A chat's first map growth: open the panel on the Map beside a wide window (A5), mark the Map tab while the
   * user reads another tab, never open in the overlay sheet or after the user closed the panel in this chat.
   */
  const revealMap = useCallback((loupistId: string, threadId: string): "opened" | "badged" | "none" => {
    const key = chatSessionKey(loupistId, threadId);
    const visible = visibleRef.current;
    const onScreen = visible.tab === "chat" && visible.loupistId === loupistId && visible.threadId === threadId;
    if (!onScreen || sessionsRef.current[key]?.dismissed || window.matchMedia(workspaceOverlayMedia).matches || window.innerWidth < MAP_REVEAL_MIN_WIDTH) return "none";
    if (kindRef.current !== "materials" || (layoutRef.current.open && Date.now() - lastInteraction.current < 10_000)) return "badged";
    if (!layoutRef.current.open) setOpen(true);
    return "opened";
  }, []);

  /** Opens a chart or document in the Work tab; it may come from another chat of the Loupist. */
  const openArtifact = useCallback(
    (loupistId: string, threadId: string, artifact: ChatArtifactRef, artifactThreadId = threadId) =>
      openTab(loupistId, threadId, workWorkspaceTab(artifact, artifactThreadId)),
    [openTab],
  );

  /** Saved Memory (Sources, Lens, notes) opens in the chat's one Evidence reader. */
  const openMemory = useCallback(
    (loupistId: string, threadId: string, location: KnowledgeLocation, title: string) =>
      openEvidenceEntry(loupistId, threadId, knowledgeEntry(location, title)),
    [openEvidenceEntry],
  );

  const openFiles = useCallback((loupistId: string, threadId: string, filePath?: string) => {
    openTab(loupistId, threadId, filesWorkspaceTab(filePath));
  }, [openTab]);
  const openBrowser = useCallback(
    (loupistId: string, threadId: string, url = "") => {
      const parsed = url ? parseCredentialFreeHttpUrl(url) : null;
      if (url && !parsed) return;
      const normalized = parsed?.href ?? "";
      const existing = normalized ? sessionsRef.current[chatSessionKey(loupistId, threadId)]?.tabs.find(
        (tab): tab is BrowserWorkspaceTab => tab.kind === "browser" && parseCredentialFreeHttpUrl(tab.url)?.href === normalized,
      ) : undefined;
      openTab(loupistId, threadId, existing ?? browserWorkspaceTab(normalized));
    },
    [openTab],
  );

  /**
   * A website citation opens the page itself and remembers that citation on the tab.
   * The saved passage stays in Evidence for a failed open or when the reader asks for it.
   * Finance, files and saved research keep the retained reader. Steps, quotes and Memory
   * reads never reach here: `evidenceTarget` sends them elsewhere first.
   */
  const openEvidence = useCallback((loupistId: string, threadId: string, evidence: ChatEvidenceRef, step?: EvidenceStepLink) => {
    const page = citationPageUrl(evidence);
    if (page) {
      const entry = evidenceEntry(evidence, step);
      const key = chatSessionKey(loupistId, threadId);
      openBrowser(loupistId, threadId, page);
      updateSession(key, (session) => bindActiveBrowserPassage(stashEvidenceEntry(session, entry), entry.key));
      return;
    }
    openEvidenceEntry(loupistId, threadId, evidenceEntry(evidence, step));
  }, [openBrowser, openEvidenceEntry, updateSession]);

  const moveEvidence = useCallback((key: string, delta: -1 | 1) => {
    updateSession(key, (session) => {
      const tab = evidenceWorkspaceTab(session);
      const moved = tab ? moveEvidenceHistory(tab, delta) : tab;
      return moved && moved !== tab ? { ...session, tabs: session.tabs.map((item) => item.id === EVIDENCE_TAB_ID ? moved : item) } : session;
    });
  }, [updateSession]);

  const updateTab = useCallback(
    (
      key: string,
      tabId: string,
      update: (tab: ChatWorkspaceTab) => ChatWorkspaceTab,
    ) => {
      updateSessions((current) => {
        const session = current[key];
        if (!session?.tabs.some((tab) => tab.id === tabId)) return current;
        return {
          ...current,
          [key]: {
            ...session,
            tabs: session.tabs.map((tab) =>
              tab.id === tabId ? update(tab) : tab,
            ),
          },
        };
      });
    },
    [updateSessions],
  );

  const selectTab = useCallback((key: string, tabId: string) => {
    updateSessions((current) => {
      const session = current[key] ?? (tabId === EVIDENCE_TAB_ID || tabId === WORK_TAB_ID ? emptyWorkspaceSession() : undefined);
      if (!session) return current;
      // Evidence and Work are fixed tabs: selecting one before it holds anything shows its empty reader or list.
      if (tabId === EVIDENCE_TAB_ID && !evidenceWorkspaceTab(session)) return { ...current, [key]: activateWorkspaceTab(session, emptyEvidenceTab()) };
      if (tabId === WORK_TAB_ID && !workTab(session)) return { ...current, [key]: activateWorkspaceTab(session, workWorkspaceTab()) };
      if (!session.tabs.some((tab) => tab.id === tabId)) return current;
      return { ...current, [key]: selectWorkspaceTab(session, tabId) };
    });
  }, [updateSessions]);

  const closeTab = useCallback(
    (key: string, tab: ChatWorkspaceTab) => {
      if (!sessionsRef.current[key]?.tabs.some((item) => item.id === tab.id)) return;
      if (tab.kind === "browser" && tab.native_id) {
        void bridge.browserTabClose(tab.native_id).catch(() => undefined);
      }
      updateSessions((current) => {
        const session = current[key];
        const index = session.tabs.findIndex((item) => item.id === tab.id);
        const tabs = session.tabs.filter((item) => item.id !== tab.id);
        const active_tab_id =
          session.active_tab_id === tab.id
            ? tabs[Math.min(Math.max(index, 0), tabs.length - 1)]?.id
            : session.active_tab_id;
        const badges = session.badges?.filter((id) => id !== tab.id);
        return { ...current, [key]: { ...session, tabs, active_tab_id, badges: badges?.length ? badges : undefined } };
      });
      if (!sessionsRef.current[key]?.tabs.length) setOpen(false);
    },
    [bridge, updateSessions],
  );

  /** Moves a pending session (e.g. the empty-thread draft) onto a persisted thread. */
  const adoptSession = useCallback((fromKey: string, toKey: string) => {
    if (fromKey === toKey) return;
    updateSessions((current) => {
      const pending = current[fromKey];
      if (!pending) return current;
      const next = { ...current, [toKey]: pending };
      delete next[fromKey];
      return next;
    });
    setSelectedArtifacts((current) => {
      if (!(fromKey in current)) return current;
      const next = { ...current, [toKey]: current[fromKey] };
      delete next[fromKey];
      return next;
    });
  }, [updateSessions]);

  /** Closes native browser tabs and drops the session for one thread. */
  const removeSession = useCallback(
    async (loupistId: string, threadId: string) => {
      const key = chatSessionKey(loupistId, threadId);
      setSelectedArtifacts((current) => {
        const next = { ...current };
        delete next[key];
        return next;
      });
      const browserTabs = (sessionsRef.current[key]?.tabs ?? []).filter(
        isNativeBrowserTab,
      );
      updateSessions((current) => {
        const next = { ...current };
        delete next[key];
        return next;
      });
      if (
        visibleRef.current.loupistId === loupistId &&
        visibleRef.current.threadId === threadId
      ) {
        setOpen(false);
      }
      await Promise.all(
        browserTabs.map((tab) =>
          bridge.browserTabClose(tab.native_id!).catch(() => undefined),
        ),
      );
    },
    [bridge, updateSessions],
  );

  /** Closes native browser tabs and drops every session owned by one loupist. */
  const removeLoupistSessions = useCallback(
    async (loupistId: string) => {
      const prefix = `${encodeURIComponent(loupistId)}:`;
      setSelectedArtifacts((current) =>
        Object.fromEntries(Object.entries(current).filter(([key]) => !key.startsWith(prefix))),
      );
      const browserTabs = Object.entries(sessionsRef.current)
        .filter(([key]) => key.startsWith(prefix))
        .flatMap(([, session]) => session.tabs)
        .filter(isNativeBrowserTab);
      updateSessions((current) =>
        Object.fromEntries(
          Object.entries(current).filter(([key]) => !key.startsWith(prefix)),
        ),
      );
      if (visibleRef.current.loupistId === loupistId) setOpen(false);
      await Promise.all(
        browserTabs.map((tab) =>
          bridge.browserTabClose(tab.native_id!).catch(() => undefined),
        ),
      );
    },
    [bridge, updateSessions],
  );

  /** Canonical header entry: switching panels never changes the active chat or its draft. */
  const selectKind = useCallback((next: WorkspacePanelKind, key?: string) => {
    changeKind(next);
    const visible = visibleRef.current;
    show(key ?? (visible.loupistId && visible.threadId ? chatSessionKey(visible.loupistId, visible.threadId) : undefined));
  }, [changeKind, show]);

  /** Header split choice applies to every reader so switching content keeps the chosen split. */
  const selectSplit = useCallback((next: WorkspacePlacement, key?: string) => {
    updateLayout("dockByKind", { chart: next, reading: next });
    setPlacement(next);
    if (layoutRef.current.open && placement === next) dismiss(key);
    else show(key);
  }, [dismiss, placement, setPlacement, show, updateLayout]);

  /** Programmatic close (overlay hand-offs, Loupist changes); it is not the user's choice for the chat. */
  const close = useCallback(() => setOpen(false), []);
  /** The Dock control: re-docks the panel and remembers the choice for the content kind on screen. */
  const togglePlacement = useCallback((next: WorkspacePlacement, key?: string, kind?: DockKind) => {
    if (open && placement === next) {
      dismiss(key);
    } else {
      if (kind) updateLayout("dockByKind", (current) => ({ ...current, [kind]: next }));
      setPlacement(next);
      show(key);
    }
  }, [dismiss, open, placement, show, updateLayout]);

  /** Content of another kind came on screen: dock where that kind was last docked, or its default. */
  const applyDockKind = useCallback((kind: DockKind) => {
    setPlacement(layoutRef.current.dockByKind[kind] ?? DEFAULT_DOCK[kind]);
  }, [setPlacement]);

  /**
   * "Always dock here" (side-panel.md P7): every content kind docks at the current placement,
   * remembered like a Dock press; turning it off returns each kind to its default.
   */
  const pinPlacement = useCallback((pinned: boolean) => {
    const here = layoutRef.current.placement;
    updateLayout("dockByKind", pinned ? { chart: here, reading: here } : {});
  }, [updateLayout]);
  const placementPinned = (Object.keys(DEFAULT_DOCK) as DockKind[])
    .every((kind) => (layout.dockByKind[kind] ?? DEFAULT_DOCK[kind]) === placement);

  return {
    kind,
    selectKind,
    selectSplit,
    open,
    hydrated,
    show,
    dismiss,
    noteInteraction,
    moveEvidence,
    selectedArtifacts,
    selectArtifactForChat,
    consumeArtifactForChat,
    setOpen,
    close,
    togglePlacement,
    applyDockKind,
    pinPlacement,
    placementPinned,
    panelWidth,
    setPanelWidth,
    panelHeight,
    setPanelHeight,
    placement,
    sessions,
    visibleRef,
    publishArtifact,
    revealMap,
    openArtifact,
    openFiles,
    openMemory,
    openEvidence,
    openBrowser,
    updateTab,
    selectTab,
    closeTab,
    adoptSession,
    removeSession,
    removeLoupistSessions,
  };
}

export type WorkspacePanel = ReturnType<typeof useWorkspacePanel>;
