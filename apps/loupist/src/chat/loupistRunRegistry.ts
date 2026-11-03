import { useSyncExternalStore } from "react";
import { errorMessage } from "../lib/errors";
import type { LoupistBridge, LoupistWorkspace, RunActivity } from "../types";

export type RecoveredChatRun = {
  run_id: string;
  loupist_id: string;
  thread_id: string;
  status: "running" | "reconciling";
  abort_requested: boolean;
  error?: string;
};

type Handlers = {
  isLocalChatActive?: (loupistId: string, threadId: string) => boolean;
  onRecoveredChatReconciled?: (
    workspace: LoupistWorkspace,
    threadId: string,
  ) => void | Promise<void>;
};

type LoupistRunSnapshot = {
  chats: Readonly<Record<string, RecoveredChatRun>>;
  hydration: "idle" | "loading" | "ready" | "error";
};

export type LoupistRunRegistrySnapshot = {
  version: number;
  active_loupist_ids: ReadonlySet<string>;
  active_chat_threads: readonly { loupist_id: string; thread_id: string }[];
};

const EMPTY: LoupistRunSnapshot = { chats: {}, hydration: "idle" };
const timerKey = (loupistId: string, threadId: string) =>
  JSON.stringify([loupistId, threadId]);

/** Keeps native Chat runs addressable while the user navigates or reloads. */
export class LoupistRunRegistry {
  readonly #states = new Map<string, LoupistRunSnapshot>();
  readonly #listeners = new Set<() => void>();
  readonly #hydrations = new Map<string, Promise<void>>();
  readonly #monitors = new Map<string, number>();
  readonly #abortRetries = new Map<string, number>();
  readonly #epochs = new Map<string, number>();
  #disposed = false;
  #generation = 0;
  #snapshot: LoupistRunRegistrySnapshot = {
    version: 0,
    active_loupist_ids: new Set(),
    active_chat_threads: [],
  };

  constructor(
    private readonly bridge: LoupistBridge,
    private readonly handlers: Handlers = {},
  ) {}

  readonly subscribe = (listener: () => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };
  readonly getSnapshot = () => this.#snapshot;
  getLoupistSnapshot(loupistId: string) {
    return this.#states.get(loupistId) ?? EMPTY;
  }
  getRecoveredChat(loupistId: string, threadId: string) {
    return this.getLoupistSnapshot(loupistId).chats[threadId];
  }

  activate() {
    this.#generation += 1;
    this.#disposed = false;
  }
  deactivate() {
    const generation = ++this.#generation;
    queueMicrotask(() => {
      if (generation === this.#generation) this.dispose();
    });
  }
  dispose() {
    this.#generation += 1;
    this.#disposed = true;
    for (const timer of this.#monitors.values()) window.clearTimeout(timer);
    for (const timer of this.#abortRetries.values()) window.clearTimeout(timer);
    this.#monitors.clear();
    this.#abortRetries.clear();
    this.#listeners.clear();
  }
  removeLoupist(loupistId: string) {
    for (const threadId of Object.keys(this.getLoupistSnapshot(loupistId).chats))
      this.#clearTimers(loupistId, threadId);
    this.#epochs.set(loupistId, (this.#epochs.get(loupistId) ?? 0) + 1);
    this.#hydrations.delete(loupistId);
    if (this.#states.delete(loupistId)) this.#emit();
  }

  hydrateLoupist(loupistId: string) {
    const existing = this.#hydrations.get(loupistId);
    if (existing) return existing;
    this.#update(loupistId, (state) => ({ ...state, hydration: "loading" }));
    const epoch = this.#epochs.get(loupistId) ?? 0;
    const hydration = this.#loadActivities(loupistId, epoch)
      .then(() => {
        if (!this.#disposed && (this.#epochs.get(loupistId) ?? 0) === epoch)
          this.#update(loupistId, (state) => ({ ...state, hydration: "ready" }));
      })
      .catch(() => {
        if (!this.#disposed && (this.#epochs.get(loupistId) ?? 0) === epoch)
          this.#update(loupistId, (state) => ({ ...state, hydration: "error" }));
      })
      .finally(() => {
        if (this.#hydrations.get(loupistId) === hydration)
          this.#hydrations.delete(loupistId);
      });
    this.#hydrations.set(loupistId, hydration);
    return hydration;
  }
  async hydrateActivities(loupistIds: readonly string[]) {
    const epochs = new Map(
      loupistIds.map((loupistId) => [loupistId, this.#epochs.get(loupistId) ?? 0]),
    );
    try {
      const activities = await this.bridge.runActivityList();
      if (this.#disposed) return;
      for (const activity of activities)
        if (
          activity.kind === "chat" &&
          epochs.get(activity.loupist_id) === (this.#epochs.get(activity.loupist_id) ?? 0)
        )
          this.#attach(activity);
    } catch {
      // Activity discovery is best-effort; canonical Chat remains in SQLite.
    }
  }
  async #loadActivities(loupistId: string, epoch: number) {
    const activities = await this.bridge.runActivityList();
    if (this.#disposed || (this.#epochs.get(loupistId) ?? 0) !== epoch) return;
    for (const activity of activities)
      if (activity.loupist_id === loupistId && activity.kind === "chat")
        this.#attach(activity);
  }

  claimLocalChat(loupistId: string, threadId: string) {
    const run = this.getRecoveredChat(loupistId, threadId);
    if (!run) return false;
    this.#remove(loupistId, threadId, run.run_id);
    return true;
  }
  abortRecoveredChat(loupistId: string, threadId: string) {
    const run = this.getRecoveredChat(loupistId, threadId);
    if (!run || run.abort_requested) return false;
    this.#updateChat(loupistId, threadId, (current) => ({
      ...current,
      abort_requested: true,
      error: undefined,
    }));
    this.#pollAbort(loupistId, threadId, run.run_id, 0);
    return true;
  }

  #attach(activity: RunActivity) {
    const loupistId = activity.loupist_id;
    const threadId = activity.target;
    if (this.handlers.isLocalChatActive?.(loupistId, threadId)) {
      this.claimLocalChat(loupistId, threadId);
      return;
    }
    const previous = this.getRecoveredChat(loupistId, threadId);
    if (previous?.run_id === activity.run_id) {
      const key = timerKey(loupistId, threadId);
      if (!this.#monitors.has(key))
        this.#monitor(loupistId, threadId, activity.run_id);
      return;
    }
    if (previous) this.#clearTimers(loupistId, threadId);
    this.#update(loupistId, (state) => ({
      ...state,
      chats: {
        ...state.chats,
        [threadId]: {
          run_id: activity.run_id,
          loupist_id: loupistId,
          thread_id: threadId,
          status: "running",
          abort_requested: false,
        },
      },
    }));
    this.#monitor(loupistId, threadId, activity.run_id);
  }

  #monitor(loupistId: string, threadId: string, runId: string) {
    const key = timerKey(loupistId, threadId);
    this.#clear(this.#monitors, key);
    const poll = async () => {
      if (!this.#isRun(loupistId, threadId, runId)) return;
      try {
        const active = (await this.bridge.runActivityList()).some(
          (activity) =>
            activity.kind === "chat" &&
            activity.run_id === runId &&
            activity.loupist_id === loupistId &&
            activity.target === threadId,
        );
        if (!this.#isRun(loupistId, threadId, runId)) return;
        if (active) {
          this.#updateChat(loupistId, threadId, (current) => ({
            ...current,
            status: "running",
            error: undefined,
          }));
        } else {
          this.#updateChat(loupistId, threadId, (current) => ({
            ...current,
            status: "reconciling",
            error: undefined,
          }));
          try {
            const workspace = await this.bridge.loupistSelect(loupistId);
            if (!this.#isRun(loupistId, threadId, runId)) return;
            await this.handlers.onRecoveredChatReconciled?.(workspace, threadId);
            this.#remove(loupistId, threadId, runId);
            return;
          } catch (cause) {
            if (this.#isRun(loupistId, threadId, runId))
              this.#updateChat(loupistId, threadId, (current) => ({
                ...current,
                status: "reconciling",
                error: errorMessage(
                  cause,
                  "The response finished, but the saved Chat could not be refreshed yet.",
                ),
              }));
          }
        }
      } catch {
        // Keep exact run identity locked until Rust confirms completion.
      }
      if (this.#isRun(loupistId, threadId, runId))
        this.#monitors.set(key, window.setTimeout(poll, 400));
    };
    this.#monitors.set(key, window.setTimeout(poll, 0));
  }

  #pollAbort(loupistId: string, threadId: string, runId: string, attempt: number) {
    const key = timerKey(loupistId, threadId);
    this.#clear(this.#abortRetries, key);
    void this.bridge.chatAbort(runId).catch((cause) => {
      const run = this.getRecoveredChat(loupistId, threadId);
      if (this.#disposed || run?.run_id !== runId || !run.abort_requested) return;
      if (attempt >= 49) {
        this.#updateChat(loupistId, threadId, (current) => ({
          ...current,
          abort_requested: false,
          error: errorMessage(cause, "Could not stop the recovered response."),
        }));
      } else {
        this.#abortRetries.set(
          key,
          window.setTimeout(
            () => this.#pollAbort(loupistId, threadId, runId, attempt + 1),
            100,
          ),
        );
      }
    });
  }

  #isRun(loupistId: string, threadId: string, runId: string) {
    return (
      !this.#disposed &&
      this.getRecoveredChat(loupistId, threadId)?.run_id === runId
    );
  }
  #updateChat(
    loupistId: string,
    threadId: string,
    updater: (current: RecoveredChatRun) => RecoveredChatRun,
  ) {
    const current = this.getRecoveredChat(loupistId, threadId);
    if (!current) return;
    this.#update(loupistId, (state) => ({
      ...state,
      chats: { ...state.chats, [threadId]: updater(current) },
    }));
  }
  #remove(loupistId: string, threadId: string, runId: string) {
    if (!this.#isRun(loupistId, threadId, runId)) return;
    this.#clearTimers(loupistId, threadId);
    this.#update(loupistId, (state) => {
      const chats = { ...state.chats };
      delete chats[threadId];
      return { ...state, chats };
    });
  }
  #update(loupistId: string, updater: (state: LoupistRunSnapshot) => LoupistRunSnapshot) {
    this.#states.set(loupistId, updater(this.getLoupistSnapshot(loupistId)));
    this.#emit();
  }
  #emit() {
    const activeLoupistIds = new Set<string>();
    const activeChatThreads: Array<{ loupist_id: string; thread_id: string }> = [];
    for (const [loupistId, state] of this.#states) {
      for (const threadId of Object.keys(state.chats))
        activeChatThreads.push({ loupist_id: loupistId, thread_id: threadId });
      if (Object.keys(state.chats).length > 0) activeLoupistIds.add(loupistId);
    }
    this.#snapshot = {
      version: this.#snapshot.version + 1,
      active_loupist_ids: activeLoupistIds,
      active_chat_threads: activeChatThreads,
    };
    if (!this.#disposed)
      for (const listener of this.#listeners) listener();
  }
  #clear(timers: Map<string, number>, key: string) {
    const timer = timers.get(key);
    if (timer !== undefined) window.clearTimeout(timer);
    timers.delete(key);
  }
  #clearTimers(loupistId: string, threadId: string) {
    const key = timerKey(loupistId, threadId);
    this.#clear(this.#monitors, key);
    this.#clear(this.#abortRetries, key);
  }
}

export const useLoupistRunRegistrySnapshot = (registry: LoupistRunRegistry) =>
  useSyncExternalStore(
    registry.subscribe,
    registry.getSnapshot,
    registry.getSnapshot,
  );
