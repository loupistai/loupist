import i18n, { useTranslation } from "@/i18n";
import { useCallback, useEffect, useRef, useState } from "react";
import type { LoupistCreateRequest, LoupistImportRequest, LoupistBridge, LoupistSummary } from "../types";
import { errorMessage } from "../lib/errors";
import { EMPTY_CHAT_THREAD_ID, emptyChatThread, isEmptyChatThreadId } from "./emptyChat";
import type { ChatSessions } from "./useChatSessions";

type LoupistDirectoryDeps = {
  chat: ChatSessions;
  /** Closes native browser tabs and drops all workspace sessions of a loupist. */
  removeLoupistSessions: (loupistId: string) => Promise<void>;
  /** Hides the workspace panel. */
  closeWorkspace: () => void;
  /** Invoked when a loupist selection starts (e.g. to close thread dialogs). */
  onSelectionStarted: () => void;
  onFolderReconnected?: () => void;
};

/** Loupist list, selection, and lifecycle actions. */
export function useLoupistDirectory(
  bridge: LoupistBridge,
  { chat, removeLoupistSessions, closeWorkspace, onSelectionStarted, onFolderReconnected }: LoupistDirectoryDeps,
) {
  useTranslation();
  const [loupists, setLoupists] = useState<LoupistSummary[]>([]);
  const [selectedLoupist, setSelectedLoupist] = useState<LoupistSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [failedSelectionId, setFailedSelectionId] = useState<string | null>(null);
  const [mutationBusy, setMutationBusy] = useState(false);
  const [mutationError, setMutationError] = useState<string | null>(null);
  const [recoveryBusy, setRecoveryBusy] = useState(false);
  const [recoveryError, setRecoveryError] = useState<string | null>(null);
  const loupistSelectionRequestRef = useRef(0);
  const desiredLoupistIdRef = useRef<string | null>(null);
  const desiredThreadIdRef = useRef<string | undefined>(undefined);
  const { chatRegistry, runRegistry, hydrateThreadsForLoupist, setActiveThreadForLoupist } =
    chat;

  const refreshLoupistAvailability = useCallback(
    async (loupistId: string) => {
      const next = await bridge.loupistList();
      const refreshed = next.find((loupist) => loupist.id === loupistId) ?? null;
      setLoupists(next);
      if (refreshed && desiredLoupistIdRef.current === loupistId) {
        setSelectedLoupist((current) =>
          current?.id === loupistId ? refreshed : current,
        );
      }
      return refreshed;
    },
    [bridge],
  );

  /** Applies a workspace snapshot recovered by the run registry. */
  const applyRecoveredLoupist = useCallback((loupist: LoupistSummary) => {
    if (desiredLoupistIdRef.current === loupist.id) setSelectedLoupist(loupist);
  }, []);

  const selectLoupist = useCallback(
    async (loupistId: string, knownLoupist?: LoupistSummary, threadId?: string) => {
      const requestId = ++loupistSelectionRequestRef.current;
      const changingLoupist = desiredLoupistIdRef.current !== null;
      desiredLoupistIdRef.current = loupistId;
      desiredThreadIdRef.current = threadId;
      setLoading(true);
      setError(null);
      setFailedSelectionId(null);
      setMutationError(null);
      setRecoveryError(null);
      if (changingLoupist) closeWorkspace();
      onSelectionStarted();
      if (knownLoupist?.availability.status === "recovery_required") {
        setSelectedLoupist(knownLoupist);
        setLoading(false);
        return true;
      }
      try {
        const workspace = await bridge.loupistSelect(loupistId);
        if (requestId !== loupistSelectionRequestRef.current) return;
        const nextThreads = workspace.threads.map((thread) =>
          chatRegistry.reconcile(thread),
        );
        chatRegistry.ensure(emptyChatThread(loupistId));
        const previousActive = threadId ?? chat.activeThreadIdsRef.current[loupistId];
        // Opening a Loupist lands on its home Thread unless a saved Chat or an
        // unsent new Chat was open there before.
        const home = nextThreads.find((thread) => thread.home)?.id;
        const nextActive = isEmptyChatThreadId(previousActive) || nextThreads.some(
          (thread) => thread.id === previousActive,
        )
          ? (previousActive ?? null)
          : (home ?? nextThreads[0]?.id ?? EMPTY_CHAT_THREAD_ID);
        setSelectedLoupist(workspace.loupist);
        hydrateThreadsForLoupist(loupistId, nextThreads);
        setActiveThreadForLoupist(loupistId, nextActive);
        return true;
      } catch (cause) {
        if (requestId !== loupistSelectionRequestRef.current) return;
        setError(errorMessage(cause, i18n.t("app:useLoupistDirectory.couldNotOpenThisLoupist")));
        setFailedSelectionId(loupistId);
        return false;
      } finally {
        if (requestId === loupistSelectionRequestRef.current) setLoading(false);
      }
    },
    [
      bridge,
      chat.activeThreadIdsRef,
      chatRegistry,
      closeWorkspace,
      onSelectionStarted,
      setActiveThreadForLoupist,
      hydrateThreadsForLoupist,
    ],
  );

  useEffect(() => {
    let active = true;
    void bridge
      .loupistList()
      .then(async (next) => {
        if (!active) return;
        setLoupists(next);
        await runRegistry.hydrateActivities(next.map((loupist) => loupist.id));
        if (!active) return;
        let last: unknown;
        try { last = JSON.parse(localStorage.getItem("loupist.lastConversation") ?? "null"); } catch { /* Ignore obsolete preferences. */ }
        const restored = Array.isArray(last) ? next.find(item => item.id === last[0] && item.availability.status === "available") : undefined;
        const first = restored ??
          next.find((loupist) => loupist.availability.status === "available") ??
          next[0];
        if (first) await selectLoupist(first.id, first, restored && Array.isArray(last) && typeof last[1] === "string" ? last[1] : undefined);
        else setLoading(false);
      })
      .catch((cause: unknown) => {
        if (!active) return;
        setError(errorMessage(cause, i18n.t("app:useLoupistDirectory.couldNotLoadYourLoupists")));
        setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [bridge, runRegistry, selectLoupist]);

  const addLoupist = async (request: LoupistCreateRequest) => {
    setLoading(true);
    setMutationBusy(true);
    setMutationError(null);
    let added: LoupistSummary | null = null;
    try {
      added = await bridge.loupistCreate(request);

      if (added) {
        const created = added;
        setLoupists((current) =>
          current.some((loupist) => loupist.id === created.id)
            ? current.map((loupist) => (loupist.id === created.id ? created : loupist))
            : [...current, created],
        );
      }

      const next = await bridge.loupistList();
      setLoupists(next);
      if (added) await selectLoupist(added.id);
      else setLoading(false);
      return added?.id ?? null;
    } catch (cause) {
      setMutationError(
        added
          ? i18n.t("app:useLoupistDirectory.theAgentWasCreatedButTheAgentList")
          : errorMessage(cause, i18n.t("app:useLoupistDirectory.couldNotPrepareThisAgent")),
      );
      setLoading(false);
      return added?.id ?? null;
    } finally {
      setMutationBusy(false);
    }
  };

  const importLoupist = async (request: LoupistImportRequest) => {
    if (mutationBusy) return false;
    setLoading(true);
    setMutationBusy(true);
    setMutationError(null);
    try {
      const imported = await bridge.loupistImport(request);
      setLoupists((current) => [...current.filter((loupist) => loupist.id !== imported.id), imported]);
      await selectLoupist(imported.id, imported);
      return true;
    } catch (cause) {
      setMutationError(errorMessage(cause, i18n.t("onboarding:appDialogs.couldNotImportThisFolder")));
      setLoading(false);
      return false;
    } finally {
      setMutationBusy(false);
    }
  };

  const reconnectLoupist = async (workspace_path: string) => {
    if (!selectedLoupist || mutationBusy) return false;
    const loupistId = selectedLoupist.id;
    setMutationBusy(true);
    setMutationError(null);
    try {
      const updated = await bridge.loupistReconnect({ loupist_id: loupistId, workspace_path });
      const activeThread = chat.activeThreadIdsRef.current[loupistId] ?? undefined;
      await removeLoupistSessions(loupistId);
      chat.removeLoupist(loupistId);
      onFolderReconnected?.();
      setLoupists((current) => current.map((item) => item.id === loupistId ? updated : item));
      if (desiredLoupistIdRef.current === loupistId) await selectLoupist(loupistId, updated, activeThread);
      return true;
    } catch (cause) { setMutationError(errorMessage(cause, i18n.t("app:useLoupistDirectory.couldNotReconnectThisFolder"))); return false; }
    finally { setMutationBusy(false); }
  };

  const recoverSelectedLoupist = async () => {
    if (
      !selectedLoupist ||
      selectedLoupist.availability.status !== "recovery_required" ||
      recoveryBusy
    ) {
      return;
    }
    const loupistId = selectedLoupist.id;
    const action = selectedLoupist.availability.action;
    setRecoveryBusy(true);
    setRecoveryError(null);
    try {
      const recovered = await bridge.loupistRecover({ loupist_id: loupistId, action });
      setLoupists((current) =>
        current.map((loupist) => (loupist.id === recovered.id ? recovered : loupist)),
      );
      if (desiredLoupistIdRef.current === loupistId) {
        await selectLoupist(loupistId, recovered);
      }
    } catch (cause) {
      if (desiredLoupistIdRef.current === loupistId) {
        setRecoveryError(
          errorMessage(cause, i18n.t("app:useLoupistDirectory.memoryRecoveryCouldNotFinishSafely")),
        );
      }
    } finally {
      setRecoveryBusy(false);
    }
  };

  const deleteLoupist = async () => {
    if (!selectedLoupist || mutationBusy) return;
    const loupistAtStart = selectedLoupist;
    const deletedIndex = loupists.findIndex((loupist) => loupist.id === loupistAtStart.id);
    setMutationBusy(true);
    setMutationError(null);
    try {
      await bridge.loupistDelete({ loupist_id: loupistAtStart.id });
      await removeLoupistSessions(loupistAtStart.id);
      const next = loupists.filter((loupist) => loupist.id !== loupistAtStart.id);
      setLoupists(next);
      chat.removeLoupist(loupistAtStart.id);

      if (desiredLoupistIdRef.current === loupistAtStart.id) {
        loupistSelectionRequestRef.current += 1;
        desiredLoupistIdRef.current = null;
        setSelectedLoupist(null);
        closeWorkspace();
      }

      const replacement =
        next[Math.min(deletedIndex, next.length - 1)] ?? next[0];
      if (replacement) await selectLoupist(replacement.id);
      else setLoading(false);
    } catch (cause) {
      setMutationError(errorMessage(cause, i18n.t("app:useLoupistDirectory.couldNotDeleteThisAgent")));
    } finally {
      setMutationBusy(false);
    }
  };

  const updateLoupist = useCallback((updated: LoupistSummary) => {
    setLoupists((current) =>
      current.map((loupist) => (loupist.id === updated.id ? updated : loupist)),
    );
    if (desiredLoupistIdRef.current === updated.id) setSelectedLoupist(updated);
  }, []);

  /** A Loupist Rust created outside this hook (a shared Loupist added as new) joins the list and opens. */
  const adoptLoupist = useCallback((adopted: LoupistSummary) => {
    setLoupists((current) => [...current.filter((loupist) => loupist.id !== adopted.id), adopted]);
    return selectLoupist(adopted.id, adopted);
  }, [selectLoupist]);

  const retrySelection = useCallback(() => {
    const target = failedSelectionId;
    if (!target) return;
    return selectLoupist(target, loupists.find(candidate => candidate.id === target), desiredThreadIdRef.current);
  }, [failedSelectionId, loupists, selectLoupist]);

  const keepCurrentLoupist = useCallback(() => {
    desiredLoupistIdRef.current = selectedLoupist?.id ?? null;
    setFailedSelectionId(null);
    setError(null);
  }, [selectedLoupist]);

  const recordLastModel = useCallback((loupistId: string, profileId: string) => {
    setSelectedLoupist((current) =>
      current?.id === loupistId
        ? { ...current, last_model_profile_id: profileId }
        : current,
    );
    setLoupists((current) =>
      current.map((loupist) =>
        loupist.id === loupistId
          ? { ...loupist, last_model_profile_id: profileId }
          : loupist,
      ),
    );
  }, []);

  return {
    loupists,
    selectedLoupist,
    loading,
    error,
    failedSelectionId,
    retrySelection,
    keepCurrentLoupist,
    setError,
    mutationBusy,
    mutationError,
    setMutationError,
    recoveryBusy,
    recoveryError,
    desiredLoupistIdRef,
    refreshLoupistAvailability,
    applyRecoveredLoupist,
    selectLoupist,
    addLoupist,
    importLoupist,
    recoverSelectedLoupist,
    reconnectLoupist,
    deleteLoupist,
    updateLoupist,
    adoptLoupist,
    recordLastModel,
  };
}

export type LoupistDirectory = ReturnType<typeof useLoupistDirectory>;
