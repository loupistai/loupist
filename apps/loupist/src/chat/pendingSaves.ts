import { useSyncExternalStore } from "react";
import type { ChatMessage, ChatSaveChangedEvent, ChatSaveState, LoupistBridge } from "../types";

/** A reply's save, by the id of the stored reply it belongs to. */
export type PendingSaveMap = ReadonlyMap<string, ChatSaveState>;

type Settled = (loupistId: string, threadId: string, message: ChatMessage) => void;

const EMPTY: PendingSaveMap = new Map();
/** Resolved saves remembered so a read that crossed their receipt cannot bring them back. */
const SETTLED_KEPT = 512;

/**
 * Replies whose Memory saves wait on their own while their conversation takes new
 * messages (docs/runtime/chat-turns.md#saves-that-wait-on-their-own). Rust owns the
 * state; this follows `chat-save-changed` and the `chat_saves` read, so a pending
 * save shows on its reply after a reload or a return to the conversation.
 */
export class PendingSaves {
  readonly #byLoupist = new Map<string, PendingSaveMap>();
  readonly #settled = new Set<string>();
  readonly #listeners = new Set<() => void>();

  constructor(
    private readonly bridge: Pick<LoupistBridge, "chatSaves" | "onChatSaveChanged">,
    private readonly onSettled: Settled,
  ) {}

  readonly subscribe = (listener: () => void) => {
    this.#listeners.add(listener);
    return () => { this.#listeners.delete(listener); };
  };

  readonly get = (loupistId: string) => this.#byLoupist.get(loupistId) ?? EMPTY;

  /** Follows the host's changes; read with `hydrate` after this, so none falls between. */
  listen() {
    return this.bridge.onChatSaveChanged((event) => this.apply(event));
  }

  apply(event: ChatSaveChangedEvent) {
    if (event.pending) {
      if (!this.#settled.has(event.run_id)) this.#set(event.loupist_id, [event.pending], []);
      return;
    }
    this.#settled.add(event.run_id);
    while (this.#settled.size > SETTLED_KEPT) this.#settled.delete(this.#settled.values().next().value!);
    this.#set(event.loupist_id, [], [event.run_id]);
    if (event.message) this.onSettled(event.loupist_id, event.thread_id, event.message);
  }

  /** Adds the saves Rust lists now; a change that arrived meanwhile wins. */
  async hydrate(loupistId: string) {
    let saves: ChatSaveState[];
    try { saves = await this.bridge.chatSaves({ loupist_id: loupistId }); }
    catch { return; }
    this.#set(loupistId, saves.filter((save) => !this.#settled.has(save.run_id) && !this.#hasRun(loupistId, save.run_id)), []);
  }

  #hasRun(loupistId: string, runId: string) {
    return [...this.get(loupistId).values()].some((save) => save.run_id === runId);
  }

  #set(loupistId: string, added: readonly ChatSaveState[], removedRuns: readonly string[]) {
    const current = this.get(loupistId);
    const removed = new Set(removedRuns);
    const next = new Map([...current].filter(([, save]) => !removed.has(save.run_id)));
    for (const save of added) next.set(save.message_id, save);
    if (next.size === current.size && [...next].every(([id, save]) => current.get(id) === save)) return;
    this.#byLoupist.set(loupistId, next);
    for (const listener of this.#listeners) listener();
  }
}

const noChanges = () => () => {};

export const usePendingSaves = (store: PendingSaves | undefined, loupistId: string): PendingSaveMap => {
  const read = () => store?.get(loupistId) ?? EMPTY;
  return useSyncExternalStore(store?.subscribe ?? noChanges, read, read);
};

/**
 * The conversation as shown: each reply whose save is pending carries the same
 * saving state a live reply does, so it shows its wait and its own Skip saving.
 */
export function withPendingSaves(messages: readonly ChatMessage[], saves: PendingSaveMap): ChatMessage[] {
  if (saves.size === 0) return messages as ChatMessage[];
  return messages.map((message) => {
    const save = saves.get(message.id);
    if (!save || message.memorySaving || message.status === "streaming") return message;
    return {
      ...message,
      memorySaving: {
        run_id: save.run_id,
        waiting_for_other_chat: save.waiting_for_other_chat,
        blocking_chat_id: save.blocking_chat_id ?? null,
      },
      progress: {
        ...(message.progress ?? { startedAtMs: save.save_activity.startedAtMs, finishedAtMs: null, items: [] }),
        saveActivity: save.save_activity,
      },
    };
  });
}
