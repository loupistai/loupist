import { useCallback, useEffect, useRef } from "react";
import type { LoupistBridge, ScheduledRun } from "../types";
import type { ChatSessions } from "./useChatSessions";

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

  const onRunStarted = useCallback((run: ScheduledRun) => {
    void runRegistry.hydrateLoupist(run.loupist_id);
    if (run.status === "running") chatRegistry.resumeScheduled(run);
  }, [chatRegistry, runRegistry]);

  useEffect(() => {
    if (!loupistId) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
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
        if (changed) {
          await refreshThreads(loupistId);
          if (!active) return;
          // A failed refresh must leave the previous receipt set intact so the
          // next poll retries even when no run was ever observed live.
          for (const [id, run] of tracked.current) if (run.loupist_id === loupistId) tracked.current.delete(id);
          for (const run of runs) tracked.current.set(run.run_id, run);
        }
      } catch {
        // Retain run identities and retry; a discovery failure is not completion.
      }
      if (active) timer = setTimeout(() => void poll(), 2000);
    };
    void poll();
    const unsubscribe = bridge.onInboxChanged(event => { if (event.loupistId === loupistId) void refreshThreads(loupistId).catch(() => {}); });
    return () => { active = false; clearTimeout(timer); unsubscribe(); };
  }, [bridge, chatRegistry, loupistId, refreshThreads, runRegistry]);

  return { onRunStarted, refreshThreads };
}
