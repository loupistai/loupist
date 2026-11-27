import { useCallback, useEffect, useRef, useState } from "react";
import type { LoupistBridge, ScheduledRun } from "../types";
import type { ChatSessions } from "./useChatSessions";
import { isEmptyChatThreadId } from "./emptyChat";

const windowVisible = () => document.visibilityState !== "hidden";

/** Discover native work without changing the selected Chat, draft or reader. */
export function useScheduledRuns(bridge: LoupistBridge, loupistId: string | null, chat: ChatSessions) {
  const tracked = useRef(new Map<string, ScheduledRun>());
  const [activity, setActivity] = useState<{ loupistId: string; titles: string[] } | null>(null);
  const { chatRegistry, runRegistry, setThreadsForLoupist, threadsByLoupistRef, activeThreadIdsRef, setActiveThreadForLoupist } = chat;

  /** Reads the Loupist's chats again; resolves with the ids the host has now. */
  const refreshThreads = useCallback(async (id: string) => {
    // Captured before the read, so a session opened while it is in flight is not
    // treated as one the host deleted. A running turn's refresh must not close it.
    const presentAtRead = new Set((threadsByLoupistRef.current[id] ?? []).map((thread) => thread.id));
    const workspace = await bridge.loupistSelect(id);
    const known = new Set(workspace.threads.map((thread) => thread.id));
    const active = activeThreadIdsRef.current[id];
    // A session the host deleted (a scheduled run's, gone with its run or task) is not left open empty.
    const gone = Boolean(active && !isEmptyChatThreadId(active) && !known.has(active) && !chatRegistry.isActive(id, active)
      && presentAtRead.has(active));
    setThreadsForLoupist(id, (current) => {
      const canonical = workspace.threads.map((thread) => chatRegistry.reconcile(thread));
      const kept = current.filter((thread) => !known.has(thread.id) && (
        chatRegistry.isActive(id, thread.id) || !presentAtRead.has(thread.id)
      ));
      return [...canonical, ...kept];
    });
    if (gone && activeThreadIdsRef.current[id] === active) setActiveThreadForLoupist(id, null);
    return known;
  }, [bridge, chatRegistry, setThreadsForLoupist, threadsByLoupistRef, activeThreadIdsRef, setActiveThreadForLoupist]);

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
      const checkingMemory = runRegistry.hasMemoryWaiters;
      if (checkingMemory) void runRegistry.hydrateLoupist(loupistId);
      try {
        const runs = await bridge.scheduledTasksActivity({ loupist_id: loupistId });
        if (!active) return;
        const running = runs.filter((run) => run.status === "running");
        const titles = running.map((run) => run.task_title);
        setActivity(current => {
          if (!titles.length) return null;
          if (current?.loupistId === loupistId && current.titles.length === titles.length
            && current.titles.every((title, index) => title === titles[index])) return current;
          return { loupistId, titles };
        });
        const previous = [...tracked.current.values()].filter((run) => run.loupist_id === loupistId);
        const changed = previous.length !== runs.length || runs.some((run) => {
          const saved = tracked.current.get(run.run_id);
          return !saved || saved.status !== run.status || saved.thread_id !== run.thread_id;
        });
        for (const run of running) chatRegistry.resumeScheduled(run);
        if (!checkingMemory && running.length > 0) void runRegistry.hydrateLoupist(loupistId);
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

  return { refreshThreads, runningTitles: activity?.loupistId === loupistId ? activity.titles : undefined };
}
