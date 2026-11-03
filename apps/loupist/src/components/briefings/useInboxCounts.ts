import { useEffect, useState } from "react";
import type { LoupistBridge } from "@/types";
export function useInboxCounts(bridge: LoupistBridge, loupistIds: string) {
  const [counts, setCounts] = useState<Record<string, number>>({});
  useEffect(() => {
    let active = true; let generation = 0;
    const refresh = async () => { const request = ++generation; try { const rows = await bridge.inboxUnreadCounts(); if (active && request === generation) setCounts(Object.fromEntries(rows.map(row => [row.loupistId, row.count]))); } catch { /* Preserve the last successful count. */ } };
    const unlisten = bridge.onInboxChanged(() => { void refresh(); }); void refresh();
    return () => { active = false; unlisten(); };
  }, [bridge, loupistIds]);
  return counts;
}
