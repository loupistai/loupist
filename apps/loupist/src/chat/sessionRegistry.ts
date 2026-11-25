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
  fromStoreKeepingAnswers,
  TauriChatTransport,
  toLoupistUIMessage,
  type HostedStreamConnection,
  type LoupistUIMessage,
} from "./ai-sdk";

/** A session view's join of a host-started run: subscribing, streaming into the Chat, or not joined. */
export type HostedWatchPhase = "attaching" | "live" | "idle";

/** Host stream restarts one watch follows before it leaves the session to the stored thread. */
const MAX_HOSTED_RESETS = 10;

/** The Chat without the reply a detached stream left unfinished; the store holds the rest. */
const withoutStreamingReply = (messages: LoupistUIMessage[]) => {
  const last = messages.at(-1);
  return last?.role === "assistant" && last.metadata?.status === "streaming" ? messages.slice(0, -1) : messages;
};

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
    preserveRecency?: boolean,
  ) => void;
  onStatus: (
    loupistId: string,
    threadId: string,
    status: ChatStatus,
  ) => void;
  onTitle: (loupistId: string, threadId: string, title: string) => void;
  onSaving?: (loupistId: string, threadId: string) => void;
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
  readonly #bridge: LoupistBridge;
  #disposed = false;
  #lifecycleGeneration = 0;
  readonly #scheduledRuns = new Map<string, Promise<void>>();
  readonly #scheduledThreads = new Set<string>();

  constructor(
    bridge: LoupistBridge,
    private readonly handlers: ChatSessionHandlers,
  ) {
    this.#bridge = bridge;
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
      (loupistId, threadId) => {
        if (!this.#disposed) this.handlers.onSaving?.(loupistId, threadId);
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
          const settled = messages.map(fromLoupistUIMessage);
          if (this.#scheduledThreads.has(key)) this.handlers.onMessages(loupistId, threadId, settled, true);
          else this.handlers.onMessages(loupistId, threadId, settled);
        }
      },
    });
    const unsubscribeStatus = chat["~registerStatusCallback"](() => {
      if (this.#disposed) return;
      // The request is in the Chat when it is submitted. The session list carries it from then on, so
      // another reply's save that waits on this session names it from its request while the first
      // answer runs (the host titles a session only once that answer ends).
      if (chat.status === "submitted" && chat.lastMessage?.role === "user") {
        this.handlers.onMessages(loupistId, threadId, chat.messages.map(fromLoupistUIMessage), true);
      }
      this.handlers.onStatus(loupistId, threadId, chat.status);
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
      const key = chatSessionKey(run.loupist_id, run.thread_id!);
      try {
        if (this.#disposed || this.get(run.loupist_id, run.thread_id!) !== chat || isActiveChatStatus(chat.status) || chat.messages !== baselineMessages) return;
        attached = true;
        chat.messages = connection.thread.messages.map(toLoupistUIMessage);
        this.handlers.onAttachedThread?.(connection.thread);
        this.#scheduledThreads.add(key);
        await chat.resumeStream({ body: { scheduledConnection: connection } });
      } finally {
        if (attached) this.#scheduledThreads.delete(key);
        connection.release();
      }
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

  /**
   * Joins the run the host is doing on a Chat (a research hand-off) while its session is
   * shown. The replay and the live events stream into the same Chat as a reply sent here,
   * so the view, guidance and Stop work as they do for one. Nothing running: the stored
   * thread is read again, since the run may have finished meanwhile. A host reset reloads
   * and attaches again. The returned release detaches and unsubscribes; the run goes on.
   */
  watchHosted(loupistId: string, threadId: string, onPhase: (phase: HostedWatchPhase) => void) {
    let released = false;
    let current: HostedStreamConnection | null = null;
    let resets = 0;
    const report = (phase: HostedWatchPhase) => { if (!released && !this.#disposed) onPhase(phase); };
    const attach = async (): Promise<void> => {
      const chat = this.get(loupistId, threadId);
      // A reply streaming here already shows the run.
      if (released || this.#disposed || !chat || isActiveChatStatus(chat.status)) return report("idle");
      report("attaching");
      const connection = await this.#transport.prepareHosted({ loupist_id: loupistId, thread_id: threadId, subscription_id: crypto.randomUUID() })
        .catch(() => undefined);
      if (connection === null && !released) await this.#readStored(loupistId, threadId);
      if (!connection) return report("idle");
      // A refresh may have rewritten the idle Chat meanwhile; the snapshot replaces it anyway.
      if (released || this.#disposed || this.get(loupistId, threadId) !== chat || isActiveChatStatus(chat.status)) {
        connection.release();
        return report("idle");
      }
      current = connection;
      chat.messages = fromStoreKeepingAnswers(chat.messages, connection.thread.messages);
      this.handlers.onAttachedThread?.(connection.thread);
      report("live");
      try {
        await chat.resumeStream({ body: { hostedConnection: connection } });
      } finally {
        current = null;
        connection.release();
      }
      if (released || connection.reset) {
        // The run goes on without this view: drop the reply it left half drawn.
        const kept = withoutStreamingReply(chat.messages);
        if (kept !== chat.messages) {
          chat.messages = kept;
          if (!this.#disposed) this.handlers.onMessages(loupistId, threadId, kept.map(fromLoupistUIMessage), true);
        }
      }
      if (connection.reset && !released && resets < MAX_HOSTED_RESETS) {
        resets += 1;
        return attach();
      }
      report("idle");
    };
    void attach().catch(() => report("idle"));
    return () => {
      released = true;
      current?.detach();
    };
  }

  async #readStored(loupistId: string, threadId: string) {
    try {
      const workspace = await this.#bridge.loupistSelect(loupistId);
      const stored = workspace.threads.find((thread) => thread.id === threadId);
      if (stored && !this.#disposed) this.handlers.onAttachedThread?.(this.reconcile(stored));
    } catch {
      // The next workspace refresh reads it again.
    }
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

    chat.messages = fromStoreKeepingAnswers(chat.messages, thread.messages);
    return thread;
  }

  /**
   * Shows one stored reply as the host has it now: a reply whose save resolved
   * after its stream ended. Only that message changes, so a newer reply may be
   * streaming meanwhile; it keeps its view key and its answer's node key.
   */
  replaceMessage(loupistId: string, threadId: string, message: ChatThread["messages"][number]) {
    const chat = this.get(loupistId, threadId);
    if (!chat) return;
    let replaced = false;
    const messages = chat.messages.map((shown) => {
      if ((shown.metadata?.native_message_id ?? shown.id) !== message.id) return shown;
      replaced = true;
      const stored = toLoupistUIMessage(message);
      const answerKey = shown.metadata?.answer_key;
      return { ...stored, id: shown.id, metadata: { ...stored.metadata, ...(answerKey ? { answer_key: answerKey } : {}) } };
    });
    if (replaced) chat.messages = messages;
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
