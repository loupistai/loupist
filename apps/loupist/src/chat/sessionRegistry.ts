import { Chat } from "@ai-sdk/react";
import type { ChatStatus } from "ai";
import type {
  ChatArtifactRef,
  ChatThread,
  LoupistBridge,
  ScheduledRun,
} from "../types";
import {
  fromLoupistUIMessage,
  TauriChatTransport,
  toLoupistUIMessage,
  type LoupistUIMessage,
} from "./ai-sdk";

export type ChatSessionHandlers = {
  onArtifact: (
    loupistId: string,
    threadId: string,
    artifact: ChatArtifactRef,
  ) => void;
  onMessages: (
    loupistId: string,
    threadId: string,
    messages: ChatThread["messages"],
  ) => void;
  onStatus: (
    loupistId: string,
    threadId: string,
    status: ChatStatus,
  ) => void;
  onTitle: (loupistId: string, threadId: string, title: string) => void;
  onAttachedThread?: (thread: ChatThread) => void;
};

type RegistryEntry = {
  chat: Chat<LoupistUIMessage>;
  loupistId: string;
  threadId: string;
  unsubscribeStatus: () => void;
};

export const chatSessionKey = (loupistId: string, threadId: string) =>
  `${encodeURIComponent(loupistId)}:${encodeURIComponent(threadId)}`;

export const isActiveChatStatus = (status: ChatStatus | undefined) =>
  status === "submitted" || status === "streaming";

const latestAssistantTime = (messages: ChatThread["messages"]) =>
  messages.reduce((latest, message) => message.role === "assistant"
    ? Math.max(latest, Date.parse(message.created_at) || 0)
    : latest, 0);

/**
 * Owns the long-lived AI SDK Chat object for every Loupist/thread pair.
 *
 * React views may mount and unmount as the user changes workspaces. The Chat
 * objects stay here so those view changes never become execution lifecycle
 * changes. Rust remains the authority for persistence and cancellation.
 */
export class ChatSessionRegistry {
  readonly #entries = new Map<string, RegistryEntry>();
  readonly #transport: TauriChatTransport;
  #disposed = false;
  #lifecycleGeneration = 0;
  readonly #scheduledRuns = new Map<string, Promise<void>>();

  constructor(
    bridge: LoupistBridge,
    private readonly handlers: ChatSessionHandlers,
  ) {
    this.#transport = new TauriChatTransport(
      bridge,
      (loupistId, threadId, title) => {
        if (!this.#disposed) {
          this.handlers.onTitle(loupistId, threadId, title);
        }
      },
      (loupistId, threadId, artifact) => {
        if (!this.#disposed) {
          this.handlers.onArtifact(loupistId, threadId, artifact);
        }
      },
    );
  }

  ensure(thread: ChatThread) {
    const key = chatSessionKey(thread.loupist_id, thread.id);
    const existing = this.#entries.get(key);
    if (existing) return existing.chat;

    const loupistId = thread.loupist_id;
    const threadId = thread.id;
    const chat = new Chat<LoupistUIMessage>({
      id: key,
      messages: thread.messages.map(toLoupistUIMessage),
      transport: this.#transport,
      onError: () => {
        if (!this.#disposed) {
          this.handlers.onStatus(loupistId, threadId, chat.status);
        }
      },
      onFinish: ({ messages }) => {
        if (!this.#disposed) {
          this.handlers.onMessages(
            loupistId,
            threadId,
            messages.map(fromLoupistUIMessage),
          );
        }
      },
    });
    const unsubscribeStatus = chat["~registerStatusCallback"](() => {
      if (!this.#disposed) {
        this.handlers.onStatus(loupistId, threadId, chat.status);
      }
    });
    this.#entries.set(key, {
      chat,
      loupistId,
      threadId,
      unsubscribeStatus,
    });
    this.handlers.onStatus(loupistId, threadId, chat.status);
    return chat;
  }

  get(loupistId: string, threadId: string) {
    return this.#entries.get(chatSessionKey(loupistId, threadId))?.chat;
  }

  isActive(loupistId: string, threadId: string) {
    return isActiveChatStatus(this.get(loupistId, threadId)?.status);
  }

  /** Join a native scheduled run without sending a prompt or changing selection. */
  resumeScheduled(run: ScheduledRun) {
    if (this.#disposed || !run.thread_id || this.#scheduledRuns.has(run.run_id) || this.isActive(run.loupist_id, run.thread_id)) return;
    const chat = this.ensure({ id: run.thread_id, loupist_id: run.loupist_id, title: run.task_title, use_memory: false, update_memory: false, messages: [], updated_at: new Date(run.started_at_ms).toISOString() });
    const baselineMessages = chat.messages;
    let attached = false;
    const pending = (async () => {
      // Waiting inside AI SDK resumeStream would leave status ready and a late
      // null reconnect could reset a newer local stream. Prepare outside it.
      const connection = await this.#transport.prepareScheduled(run);
      if (!connection) return;
      try {
        if (this.#disposed || this.get(run.loupist_id, run.thread_id!) !== chat || isActiveChatStatus(chat.status) || chat.messages !== baselineMessages) return;
        attached = true;
        chat.messages = connection.thread.messages.map(toLoupistUIMessage);
        this.handlers.onAttachedThread?.(connection.thread);
        await chat.resumeStream({ body: { scheduledConnection: connection } });
      } finally { connection.release(); }
    })().finally(() => {
      // Admission may still be preparing. A failed channel can also be retried
      // from the host's complete baseline while that exact run remains active.
      if (!attached || chat.status === "error") this.#scheduledRuns.delete(run.run_id);
      // Keep completed identities briefly so a stale activity poll cannot add
      // a duplicate response. This is renderer state, not execution authority.
      while (this.#scheduledRuns.size > 128) this.#scheduledRuns.delete(this.#scheduledRuns.keys().next().value!);
    });
    this.#scheduledRuns.set(run.run_id, pending);
    void pending.catch(() => undefined);
  }

  activate() {
    this.#lifecycleGeneration += 1;
    this.#disposed = false;
  }

  /**
   * React StrictMode performs a setup → cleanup → setup probe. Deferring the
   * destructive detach by one microtask lets the second setup cancel that
   * probe while a real unmount still releases every status subscription.
   */
  deactivate() {
    const generation = ++this.#lifecycleGeneration;
    queueMicrotask(() => {
      if (generation === this.#lifecycleGeneration) this.dispose();
    });
  }

  /**
   * Refresh an idle Chat from canonical SQLite data. A live Chat is never
   * replaced; its in-flight stream remains the freshest local projection.
   */
  reconcile(thread: ChatThread) {
    const chat = this.ensure(thread);
    const localMessages = chat.messages.map(fromLoupistUIMessage);
    if (isActiveChatStatus(chat.status)) {
      return {
        ...thread,
        messages: localMessages,
      };
    }

    // A Loupist selection may have started before a just-completed response was
    // committed. Do not let that older snapshot overwrite the finished Chat.
    // A retry replaces an assistant message, so its count can stay unchanged.
    // Native terminal timestamps distinguish that newer answer from a selection
    // snapshot fetched before the retry finished. Thread metadata still refreshes.
    if (localMessages.length > thread.messages.length ||
      (localMessages.length === thread.messages.length &&
        latestAssistantTime(localMessages) > latestAssistantTime(thread.messages))) {
      return {
        ...thread,
        messages: localMessages,
      };
    }

    // The store never holds the key the settled answer's Markdown node was drawn
    // under; carrying it over keeps that node mounted through the refresh.
    const answerKeys = new Map(chat.messages.flatMap((message) =>
      message.metadata?.answer_key ? [[message.metadata.native_message_id ?? message.id, message.metadata.answer_key]] : []));
    chat.messages = thread.messages.map((message) => {
      const ui = toLoupistUIMessage(message);
      const answerKey = answerKeys.get(message.id);
      return answerKey ? { ...ui, metadata: { ...ui.metadata, answer_key: answerKey } } : ui;
    });
    return thread;
  }

  remove(loupistId: string, threadId: string) {
    const key = chatSessionKey(loupistId, threadId);
    const entry = this.#entries.get(key);
    if (!entry) return;
    entry.unsubscribeStatus();
    this.#entries.delete(key);
    this.handlers.onStatus(loupistId, threadId, "ready");
  }

  removeLoupist(loupistId: string) {
    for (const entry of [...this.#entries.values()]) {
      if (entry.loupistId === loupistId) this.remove(loupistId, entry.threadId);
    }
  }

  dispose() {
    this.#lifecycleGeneration += 1;
    this.#disposed = true;
    for (const entry of this.#entries.values()) entry.unsubscribeStatus();
    this.#entries.clear();
    this.#scheduledRuns.clear();
  }
}
