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
  }, [setThreadsForLoupist]);

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
    hydrateThreadsForLoupist,
    setActiveThreadForLoupist,
    chatRegistry,
    runRegistry,
    runRegistrySnapshot,
    handlersRef,
    removeLoupist,
  };
}

export type ChatSessions = ReturnType<typeof useChatSessions>;
