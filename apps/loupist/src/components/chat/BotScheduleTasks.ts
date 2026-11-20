import { createContext, useEffect, useMemo, useReducer, useState } from "react";
import type { LoupistBridge, ScheduledTaskListItem } from "../../types";

type Tasks = ReadonlyMap<string, ScheduledTaskListItem>;
export const BotScheduleTasks = createContext<{ tasks?: Tasks; refresh?: () => void }>({});

/** One Schedule snapshot for every task card in the visible Bot conversation. */
export function useBotScheduleTasks(bridge: LoupistBridge | undefined, loupistId: string, receiptKey: string) {
  const [revision, refresh] = useReducer((value: number) => value + 1, 0);
  const [snapshot, setSnapshot] = useState<{ owner: string; key: string; tasks: Tasks }>();
  useEffect(() => {
    if (!bridge || !receiptKey) {
      setSnapshot(undefined);
      return;
    }
    let generation = 0;
    const load = async () => {
      const request = ++generation;
      setSnapshot(undefined);
      try {
        const tasks = await bridge.scheduledTasksList({ loupist_id: loupistId });
        if (request === generation) setSnapshot({ owner: loupistId, key: receiptKey, tasks: new Map(tasks.map(task => [task.id, task])) });
      } catch { /* A failed list cannot establish a task's status or deletion. */ }
    };
    void load();
    window.addEventListener("focus", load);
    const off = bridge.onInboxChanged(event => { if (event.loupistId === loupistId) void load(); });
    return () => { generation++; off(); window.removeEventListener("focus", load); };
  }, [bridge, loupistId, receiptKey, revision]);
  const tasks = snapshot?.owner === loupistId && snapshot.key === receiptKey ? snapshot.tasks : undefined;
  return useMemo(() => ({ tasks, refresh }), [tasks]);
}
