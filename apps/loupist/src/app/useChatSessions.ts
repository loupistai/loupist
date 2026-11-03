import { useCallback, useEffect, useRef, useState, type SetStateAction } from "react";
import type {
  ChatArtifactRef,
  ChatThread,
  LoupistBridge,
  LoupistSummary,
  LoupistWorkspace,
} from "../types";
import type { ChatStatus } from "ai";
import {
  ChatSessionRegistry,
  chatSessionKey,
  isActiveChatStatus,
} from "../chat/sessionRegistry";
import {
  LoupistRunRegistry,
  useLoupistRunRegistrySnapshot,
} from "../chat/loupistRunRegistry";

/** Cross-domain callbacks wired by the composition root after every hook exists. */
export type ChatSessionHandlers = {
  publishArtifact: (
    loupistId: string,
    threadId: string,
    artifact: ChatArtifactRef,
  ) => void;
  refreshLoupistAvailability: (loupistId: string) => Promise<LoupistSummary | null>;
  applyRecoveredLoupist: (loupist: LoupistSummary) => void;
  onMemoryChanged?: (loupistId: string) => void;
};

/** Fill in saved history without reverting edits made while it was loading. */
function mergeThreadListing(
  snapshot: ChatThread[],
  previous: ReadonlyMap<string, ChatThread>,
  currentThreads: ChatThread[],
) {
  const current = new Map(currentThreads.map((thread) => [thread.id, thread]));
  const snapshotIds = new Set(snapshot.map((thread) => thread.id));
  const merged = snapshot.flatMap((thread) => {
    if (previous.has(thread.id) && !current.has(thread.id)) return [];
    const latest = current.get(thread.id);
    return [latest && latest !== previous.get(thread.id) ? latest : thread];
  });
  return merged.concat(currentThreads.filter((thread) =>
    !snapshotIds.has(thread.id) && !previous.has(thread.id),
  ));
}

/** Per-loupist chat threads, live statuses, and the chat/run registries. */
export function useChatSessions(bridge: LoupistBridge) {
  const [threadsByLoupist, setThreadsByLoupist] = useState<
    Record<string, ChatThread[]>
  >({});
  const [activeThreadIds, setActiveThreadIds] = useState<
    Record<string, string | null>
  >({});
  const [chatStatuses, setChatStatuses] = useState<Record<string, ChatStatus>>(
    {},
  );
  const threadsByLoupistRef = useRef(threadsByLoupist);
  const activeThreadIdsRef = useRef(activeThreadIds);
  const activeThreadEpochsRef = useRef<Record<string, number>>({});
  const chatRegistryRef = useRef<ChatSessionRegistry | null>(null);
  const runRegistryRef = useRef<LoupistRunRegistry | null>(null);
  const handlersRef = useRef<ChatSessionHandlers | null>(null);
  const pendingThreadLoads = useRef(new Map<string, Promise<void>>());
  const [loadedLoupistIds, setLoadedLoupistIds] = useState<ReadonlySet<string>>(new Set());
  const loadedLoupistIdsRef = useRef(loadedLoupistIds);

  const setThreadsForLoupist = useCallback(
    (loupistId: string, action: SetStateAction<ChatThread[]>) => {
      const current = threadsByLoupistRef.current[loupistId] ?? [];
      const nextThreads =
        typeof action === "function" ? action(current) : action;
      if (nextThreads === current) return;
      const next = {
        ...threadsByLoupistRef.current,
        [loupistId]: nextThreads,
      };
      threadsByLoupistRef.current = next;
      setThreadsByLoupist(next);
    },
    [],
  );

  const setActiveThreadForLoupist = useCallback(
    (loupistId: string, threadId: string | null) => {
      activeThreadEpochsRef.current[loupistId] = (activeThreadEpochsRef.current[loupistId] ?? 0) + 1;
      const next = {
        ...activeThreadIdsRef.current,
        [loupistId]: threadId,
      };
      activeThreadIdsRef.current = next;
      setActiveThreadIds(next);
    },
    [],
  );

  const hydrateThreadsForLoupist = useCallback((loupistId: string, threads: ChatThread[]) => {
    setThreadsForLoupist(loupistId, threads);
    if (loadedLoupistIdsRef.current.has(loupistId)) return;
    loadedLoupistIdsRef.current = new Set([...loadedLoupistIdsRef.current, loupistId]);
    setLoadedLoupistIds(loadedLoupistIdsRef.current);
  }, [setThreadsForLoupist]);

  // loupistSelect is a read-only, host-authorized workspace snapshot. Loading a
  // sidebar group must not change the selected Loupist or mount its Chat session.
  const loadThreadsForLoupist = useCallback((loupistId: string): Promise<void> => {
    if (loadedLoupistIdsRef.current.has(loupistId)) return Promise.resolve();
    const pending = pendingThreadLoads.current.get(loupistId);
    if (pending) return pending;
    const previous = new Map((threadsByLoupistRef.current[loupistId] ?? []).map((thread) => [thread.id, thread]));
    const request = bridge.loupistSelect(loupistId).then((workspace) => {
      if (pendingThreadLoads.current.get(loupistId) !== request || loadedLoupistIdsRef.current.has(loupistId)) return;
      hydrateThreadsForLoupist(loupistId, mergeThreadListing(
        workspace.threads.filter((thread) => thread.loupist_id === loupistId),
        previous,
        threadsByLoupistRef.current[loupistId] ?? [],
      ));
    }).finally(() => {
      if (pendingThreadLoads.current.get(loupistId) === request) pendingThreadLoads.current.delete(loupistId);
    });
    pendingThreadLoads.current.set(loupistId, request);
    return request;
  }, [bridge, hydrateThreadsForLoupist]);

  const updateChatTitle = useCallback(
    (loupistId: string, threadId: string, title: string) => {
      setThreadsForLoupist(loupistId, (current) =>
        current.map((thread) =>
          thread.id === threadId ? { ...thread, title } : thread,
        ),
      );
    },
    [setThreadsForLoupist],
  );

  const updateChatMessages = useCallback(
    (loupistId: string, threadId: string, messages: ChatThread["messages"]) => {
      if (messages.at(-1)?.memory_update) {
        handlersRef.current?.onMemoryChanged?.(loupistId);
        void handlersRef.current
          ?.refreshLoupistAvailability(loupistId)
          .catch(() => undefined);
      }
      const updatedAt = messages.at(-1)?.created_at;
      setThreadsForLoupist(loupistId, (current) =>
        current.map((thread) =>
          thread.id === threadId
            ? {
                ...thread,
                messages,
                updated_at: updatedAt ?? thread.updated_at,
              }
            : thread,
        ),
      );
    },
    [setThreadsForLoupist],
  );

  const updateChatStatus = useCallback(
    (loupistId: string, threadId: string, status: ChatStatus) => {
      const key = chatSessionKey(loupistId, threadId);
      if (isActiveChatStatus(status)) {
        runRegistryRef.current?.claimLocalChat(loupistId, threadId);
      }
      setChatStatuses((current) => {
        if (isActiveChatStatus(status)) {
          if (current[key] === status) return current;
          return { ...current, [key]: status };
        }
        if (!(key in current)) return current;
        const next = { ...current };
        delete next[key];
        return next;
      });
    },
    [],
  );

  const publishChatArtifact = useCallback(
    (loupistId: string, threadId: string, artifact: ChatArtifactRef) => {
      handlersRef.current?.publishArtifact(loupistId, threadId, artifact);
    },
    [],
  );

  const reconcileRecoveredChat = useCallback(
    (workspace: LoupistWorkspace, threadId: string) => {
      const canonical = workspace.threads.find(
        (thread) => thread.id === threadId,
      );
      if (!canonical) return;
      const reconciled =
        chatRegistryRef.current?.reconcile(canonical) ?? canonical;
      setThreadsForLoupist(workspace.loupist.id, (current) =>
        current.some((thread) => thread.id === threadId)
          ? current.map((thread) =>
              thread.id === threadId ? reconciled : thread,
            )
          : [reconciled, ...current],
      );
      if (!activeThreadIdsRef.current[workspace.loupist.id]) {
        setActiveThreadForLoupist(workspace.loupist.id, threadId);
      }
      handlersRef.current?.applyRecoveredLoupist(workspace.loupist);
    },
    [setActiveThreadForLoupist, setThreadsForLoupist],
  );

  const [chatRegistry] = useState(
    () =>
      new ChatSessionRegistry(bridge, {
        onArtifact: publishChatArtifact,
        onMessages: updateChatMessages,
        onStatus: updateChatStatus,
        onTitle: updateChatTitle,
        onAttachedThread: (thread) => setThreadsForLoupist(thread.loupist_id, (current) => current.some((item) => item.id === thread.id)
          ? current.map((item) => item.id === thread.id ? thread : item)
          : [thread, ...current]),
      }),
  );
  chatRegistryRef.current = chatRegistry;
  const [runRegistry] = useState(
    () =>
      new LoupistRunRegistry(bridge, {
        isLocalChatActive: (loupistId, threadId) =>
          chatRegistryRef.current?.isActive(loupistId, threadId) ?? false,
        onRecoveredChatReconciled: reconcileRecoveredChat,
      }),
  );
  runRegistryRef.current = runRegistry;
  const runRegistrySnapshot = useLoupistRunRegistrySnapshot(runRegistry);

  useEffect(() => {
    chatRegistry.activate();
    return () => chatRegistry.deactivate();
  }, [chatRegistry]);

  useEffect(() => {
    runRegistry.activate();
    return () => runRegistry.deactivate();
  }, [runRegistry]);

  /** Drops every session and registry entry owned by one loupist. */
  const removeLoupist = useCallback(
    (loupistId: string) => {
      pendingThreadLoads.current.delete(loupistId);
      loadedLoupistIdsRef.current = new Set([...loadedLoupistIdsRef.current].filter((id) => id !== loupistId));
      setLoadedLoupistIds(loadedLoupistIdsRef.current);
      chatRegistry.removeLoupist(loupistId);
      runRegistry.removeLoupist(loupistId);
      const nextThreadsByLoupist = { ...threadsByLoupistRef.current };
      delete nextThreadsByLoupist[loupistId];
      threadsByLoupistRef.current = nextThreadsByLoupist;
      setThreadsByLoupist(nextThreadsByLoupist);
      const nextActiveThreadIds = { ...activeThreadIdsRef.current };
      delete nextActiveThreadIds[loupistId];
      activeThreadIdsRef.current = nextActiveThreadIds;
      setActiveThreadIds(nextActiveThreadIds);
    },
    [chatRegistry, runRegistry],
  );

  return {
    threadsByLoupist,
    activeThreadIds,
    chatStatuses,
    threadsByLoupistRef,
    activeThreadIdsRef,
    activeThreadEpochsRef,
    setThreadsForLoupist,
    loadThreadsForLoupist,
    hydrateThreadsForLoupist,
    loadedLoupistIds,
    setActiveThreadForLoupist,
    chatRegistry,
    runRegistry,
    runRegistrySnapshot,
    handlersRef,
    removeLoupist,
  };
}

export type ChatSessions = ReturnType<typeof useChatSessions>;
