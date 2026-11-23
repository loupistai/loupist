import type { ChatSubmission } from "./submission";
import type { VisibleSteer } from "./visibleSteers";

export const MEMORY_BUSY = Symbol("memory_busy");
/** A stopped or failed reply holds the queue; its notice names queued messages only while one waits. */
export const REPLY_STOPPED = Symbol("reply_stopped");
export const REPLY_FAILED = Symbol("reply_failed");
export type QueueHold = string | typeof MEMORY_BUSY | typeof REPLY_STOPPED | typeof REPLY_FAILED | null;

export type ChatDraftSessionState = {
  queued: Record<string, ChatSubmission[]>;
  steers: Record<string, VisibleSteer[]>;
  holds: Record<string, QueueHold>;
  consumeThreads: Record<string, boolean>;
};
export type ChatDraftSession = ReturnType<typeof createChatDraftSession>;

/** One app-owned state per Loupist, observed by every visit to its Chat view. */
export function createChatDraftSession() {
  let state: ChatDraftSessionState = { queued: {}, steers: {}, holds: {}, consumeThreads: {} };
  const listeners = new Set<() => void>();
  return {
    getSnapshot: () => state,
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    update: (updater: (current: ChatDraftSessionState) => ChatDraftSessionState) => {
      const next = updater(state);
      if (next === state) return;
      state = next;
      listeners.forEach((listener) => listener());
    },
  };
}
