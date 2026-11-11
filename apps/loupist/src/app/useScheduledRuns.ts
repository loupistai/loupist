import { useCallback, useEffect, useRef } from "react";
import type { LoupistBridge, ScheduledRun } from "../types";
import type { ChatSessions } from "./useChatSessions";

const windowVisible = () => document.visibilityState !== "hidden";

/** Discover native work without changing the selected Chat, draft or reader. */
export function useScheduledRuns(bridge: LoupistBridge, loupistId: string | null, chat: ChatSessions) {
  const tracked = useRef(new Map<string, ScheduledRun>());
  const { chatRegistry, runRegistry, setThreadsForLoupist } = chat;

  const refreshThreads = useCallback(async (id: string) => {
    const workspace = await bridge.loupistSelect(id);
    setThreadsForLoupist(id, (current) => {
      const canonical = workspace.threads.map((thread) => chatRegistry.reconcile(thread));
      const known = new Set(canonical.map((thread) => thread.id));
      return [...canonical, ...current.filter((thread) => !known.has(thread.id) && chatRegistry.isActive(id, thread.id))];
    });
  }, [bridge, chatRegistry, setThreadsForLoupist]);

  useEffect(() => {
    if (!loupistId) return;
    let active = true;
    let polling = false;
    let timer: ReturnType<typeof setTimeout>;
    // A host write the last refresh could not read; the next poll reads it again.
    let unread = false;
    const refreshAfterHostWrite = () => refreshThreads(loupistId).then(() => { unread = false; }, () => { unread = true; });
    const scheduleNextPoll = () => {
      clearTimeout(timer);
      if (active) timer = setTimeout(() => void poll(), windowVisible() ? 2_000 : 30_000);
    };
    const poll = async () => {
      if (!active || polling) return;
      clearTimeout(timer);
      polling = true;
      try {
        const runs = await bridge.scheduledTasksActivity({ loupist_id: loupistId });
        if (!active) return;
        const previous = [...tracked.current.values()].filter((run) => run.loupist_id === loupistId);
        const changed = previous.length !== runs.length || runs.some((run) => {
          const saved = tracked.current.get(run.run_id);
          return !saved || saved.status !== run.status || saved.thread_id !== run.thread_id;
        });
        for (const run of runs) {
          if (run.status === "running") chatRegistry.resumeScheduled(run);
        }
        if (runs.some((run) => run.status === "running")) void runRegistry.hydrateLoupist(loupistId);
        if (changed || unread) {
          await refreshThreads(loupistId);
          unread = false;
          if (!active) return;
          // A failed refresh must leave the previous receipt set intact so the
          // next poll retries even when no run was ever observed live.
          for (const [id, run] of tracked.current) if (run.loupist_id === loupistId) tracked.current.delete(id);
          for (const run of runs) tracked.current.set(run.run_id, run);
        }
      } catch {
        // Retain run identities and retry; a discovery failure is not completion.
      } finally {
        polling = false;
        scheduleNextPoll();
      }
    };
    const visibilityChanged = () => {
      if (windowVisible()) void poll();
      else scheduleNextPoll();
    };
    const focused = () => { void poll(); };
    void poll();
    document.addEventListener("visibilitychange", visibilityChanged);
    window.addEventListener("focus", focused);
    const unsubscribe = bridge.onInboxChanged(event => { if (event.loupistId === loupistId) void refreshAfterHostWrite(); });
    return () => { active = false; clearTimeout(timer); document.removeEventListener("visibilitychange", visibilityChanged); window.removeEventListener("focus", focused); unsubscribe(); };
  }, [bridge, chatRegistry, loupistId, refreshThreads, runRegistry]);

  return { refreshThreads };
}
