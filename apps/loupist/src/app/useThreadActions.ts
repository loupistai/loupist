import i18n, { useTranslation } from "@/i18n";
import { useCallback, useState } from "react";
import type { ChatThread, LoupistBridge } from "../types";
import { errorMessage } from "../lib/errors";
import { chatSessionKey } from "../chat/sessionRegistry";
import {
  EMPTY_CHAT_THREAD_ID,
  emptyChatThread,
  isEmptyChatThreadId,
} from "./emptyChat";
import type { ChatSessions } from "./useChatSessions";
import type { LoupistDirectory } from "./useLoupistDirectory";
import type { WorkspacePanel } from "./useWorkspacePanel";

type ThreadActionDeps = {
  chat: ChatSessions;
  loupist: LoupistDirectory;
  workspace: WorkspacePanel;
};

/** Chat thread lifecycle: create, rename, and delete. */
export function useThreadActions(
  bridge: LoupistBridge,
  { chat, loupist, workspace }: ThreadActionDeps,
) {
  useTranslation();
  const [mutationBusy, setMutationBusy] = useState(false);
  const [mutationError, setMutationError] = useState<string | null>(null);
  const {
    chatRegistry,
    setThreadsForLoupist,
    setActiveThreadForLoupist,
    threadsByLoupistRef,
    activeThreadIdsRef,
    activeThreadEpochsRef,
  } = chat;
  const { desiredLoupistIdRef, selectLoupist, setError } = loupist;
  const { adoptSession, removeSession } = workspace;

  const openDraftChatForLoupist = useCallback(
    async (loupistId: string) => {
      if (desiredLoupistIdRef.current !== loupistId) {
        const selected = await selectLoupist(loupistId);
        if (!selected) return;
      }
      setError(null);
      chatRegistry.ensure(emptyChatThread(loupistId));
      if (desiredLoupistIdRef.current !== loupistId) return;
      setActiveThreadForLoupist(loupistId, EMPTY_CHAT_THREAD_ID);
    },
    [
      chatRegistry,
      desiredLoupistIdRef,
      selectLoupist,
      setActiveThreadForLoupist,
      setError,
    ],
  );

  const createThreadForLoupist = useCallback(
    async (loupistId: string, title?: string) => {
      if (desiredLoupistIdRef.current !== loupistId) {
        const selected = await selectLoupist(loupistId);
        if (!selected) return null;
      }
      setError(null);
      const selectionEpoch = activeThreadEpochsRef.current[loupistId] ?? 0;
      try {
        const created = await bridge.chatCreate({ loupist_id: loupistId, ...(title ? { title } : {}) });
        chatRegistry.ensure(created);
        setThreadsForLoupist(loupistId, (current) => [
          created,
          ...current.filter(
            (thread) =>
              thread.id !== created.id && !isEmptyChatThreadId(thread.id),
          ),
        ]);
        if (desiredLoupistIdRef.current !== loupistId || (activeThreadEpochsRef.current[loupistId] ?? 0) !== selectionEpoch) return null;
        adoptSession(chatSessionKey(loupistId, EMPTY_CHAT_THREAD_ID), chatSessionKey(loupistId, created.id));
        setActiveThreadForLoupist(loupistId, created.id);
        return created;
      } catch (cause) {
        if (desiredLoupistIdRef.current === loupistId) {
          setError(errorMessage(cause, i18n.t("app:useThreadActions.couldNotCreateANewChat")));
        }
        return null;
      }
    },
    [
      adoptSession,
      activeThreadEpochsRef,
      bridge,
      chatRegistry,
      desiredLoupistIdRef,
      selectLoupist,
      setActiveThreadForLoupist,
      setError,
      setThreadsForLoupist,
    ],
  );

  const renameThread = async (target: ChatThread, title: string) => {
    if (!title.trim() || mutationBusy) return false;
    setMutationBusy(true);
    setMutationError(null);
    setError(null);
    try {
      const renamed = await bridge.chatRename({
        loupist_id: target.loupist_id,
        thread_id: target.id,
        title: title.trim(),
      });
      setThreadsForLoupist(target.loupist_id, (current) =>
        current.map((thread) =>
          thread.id === renamed.id
            ? { ...renamed, messages: thread.messages }
            : thread,
        ),
      );
      return true;
    } catch (cause) {
      setMutationError(errorMessage(cause, i18n.t("app:useThreadActions.couldNotRenameThisSession")));
      return false;
    } finally {
      setMutationBusy(false);
    }
  };

  const deleteThread = async (thread: ChatThread) => {
    if (mutationBusy) return false;
    setMutationBusy(true);
    setMutationError(null);
    setError(null);
    try {
      await bridge.chatDelete({
        loupist_id: thread.loupist_id,
        thread_id: thread.id,
      });
      const remaining = (threadsByLoupistRef.current[thread.loupist_id] ?? []).filter(
        (item) => item.id !== thread.id,
      );
      chatRegistry.remove(thread.loupist_id, thread.id);
      chatRegistry.ensure(emptyChatThread(thread.loupist_id));
      setThreadsForLoupist(thread.loupist_id, remaining);
      if (activeThreadIdsRef.current[thread.loupist_id] === thread.id) {
        setActiveThreadForLoupist(
          thread.loupist_id,
          remaining.find((item) => item.home)?.id ?? remaining[0]?.id ?? EMPTY_CHAT_THREAD_ID,
        );
      }
      // Native deletion has already succeeded. A browser cleanup failure must
      // not leave a confirmation that invites deleting the same session again.
      await removeSession(thread.loupist_id, thread.id).catch(() => undefined);
      return true;
    } catch (cause) {
      setMutationError(errorMessage(cause, i18n.t("app:useThreadActions.couldNotDeleteThisSession")));
      return false;
    } finally {
      setMutationBusy(false);
    }
  };

  return {
    mutationBusy,
    mutationError,
    setMutationError,
    openDraftChatForLoupist,
    createThreadForLoupist,
    renameThread,
    deleteThread,
  };
}
