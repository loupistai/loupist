import { useEffect, useRef, useState } from "react";
import type { ChatThread } from "../types";
import type { HostedWatchPhase } from "./sessionRegistry";

/** Joins the host's run on a Chat; the returned release leaves it running. */
export type WatchHostedRun = (threadId: string, onPhase: (phase: HostedWatchPhase) => void) => () => void;

/** The run of the Bot hand-off a research session is running, as the host reports it on the session. */
export const runningHandoffRun = (threads: readonly ChatThread[], chatId: string | undefined) =>
  chatId ? threads.find((thread) => thread.id === chatId)?.handoff_run?.run_id : undefined;

/**
 * The research sessions running a Bot hand-off. Bot posts nothing while one works: the host
 * reports the run on the session's thread (`handoff_run`) while this app holds it, and stops
 * reporting it once the run ends or the app no longer runs it.
 */
export const runningHandoffChats = (threads: readonly ChatThread[]): ReadonlySet<string> =>
  new Set(threads.flatMap((thread) => thread.handoff_run ? [thread.id] : []));

/**
 * Joins a host-started run while its session is shown, and leaves it (the run goes on)
 * when the view shows another Chat. A session the host stops reporting as running keeps
 * the join already made: the stream's own last event settles the reply, which a detach
 * would cut short.
 * `pending` covers the subscribe, so a message typed meanwhile guides the run instead of
 * starting another reply.
 */
export function useHostedRun(watch: WatchHostedRun | undefined, threadId: string | undefined, runId: string | undefined) {
  const joined = useRef<{ threadId?: string; runId?: string }>({});
  if (joined.current.threadId !== threadId) joined.current = { threadId };
  if (runId) joined.current.runId = runId;
  const target = watch && threadId ? joined.current.runId : undefined;
  const key = target ? `${threadId}\n${target}` : "";
  const [reported, setReported] = useState<{ key: string; phase: HostedWatchPhase } | null>(null);
  useEffect(() => {
    if (!watch || !threadId || !target) return;
    const watchKey = `${threadId}\n${target}`;
    return watch(threadId, (phase) => setReported({ key: watchKey, phase }));
  }, [watch, threadId, target]);
  const phase: HostedWatchPhase = !key ? "idle" : reported?.key === key ? reported.phase : "attaching";
  return { pending: phase !== "idle", runId: target };
}
