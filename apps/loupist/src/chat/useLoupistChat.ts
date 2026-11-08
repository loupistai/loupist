import i18n, { useTranslation } from "@/i18n";
import { useChat, type Chat } from "@ai-sdk/react";
import type { FileUIPart } from "ai";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import type { ChatFeedback, ChatArtifactSelection, ChatKnowledgePin, ChatMessage, ChatThread, LoupistBridge } from "../types";
import { errorMessage } from "../lib/errors";
import type { ChatAdmission } from "./submission";
import {
  fromLoupistUIMessage,
  fromStoreKeepingAnswers,
  toLoupistUIMessage,
  type LoupistUIMessage,
} from "./ai-sdk";

const t = i18n.getFixedT(null, "chat");

type Options = {
  bridge: LoupistBridge;
  chat: Chat<LoupistUIMessage>;
  loupistId: string;
  activeThread: ChatThread | undefined;
  setThreads: Dispatch<SetStateAction<ChatThread[]>>;
};

// Tauri Channels may drain a burst of native token events synchronously. Keep
// progressive rendering responsive while yielding between React store updates.
const STREAM_RENDER_THROTTLE_MS = 32;

// The registry-owned Chat survives navigation and view remounts.
const pendingSteering = new WeakSet<Chat<LoupistUIMessage>>();

const markLatestAssistantAborted = (messages: LoupistUIMessage[]) => {
  let assistantIndex = messages.length - 1;
  while (
    assistantIndex >= 0 &&
    messages[assistantIndex]?.role !== "assistant"
  ) {
    assistantIndex -= 1;
  }
  if (assistantIndex < 0) return messages;

  return messages.map((message, index) => {
    if (index !== assistantIndex) return message;
    const hasText = message.parts.some(
      (part) => part.type === "text" && part.text.length > 0,
    );
    return {
      ...message,
      metadata: { ...message.metadata, status: "aborted" as const },
      parts: hasText
        ? message.parts.map((part) =>
            part.type === "text" ? { ...part, state: "done" as const } : part,
          )
        : [
            ...message.parts,
            {
              type: "text" as const,
              text: t("useLoupistChat.responseStopped"),
              state: "done" as const,
            },
          ],
    };
  });
};

const sameThreadMessages = (
  current: ChatThread["messages"],
  next: ChatThread["messages"],
) =>
  current.length === next.length &&
  current.every(
    (message, index) => message === next[index] || JSON.stringify(message) === JSON.stringify(next[index]),
  );

export function useLoupistChat({
  bridge,
  chat,
  loupistId,
  activeThread,
  setThreads,
}: Options) {
  useTranslation("chat");
  const [announcement, setAnnouncement] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isStopping, setIsStopping] = useState(false);
  const mountedRef = useRef(true);
  const retryGuard = useRef(false);
  const {
    messages: uiMessages,
    sendMessage,
    setMessages,
    stop,
    status,
    error: chatError,
  } = useChat<LoupistUIMessage>({
    chat,
    throttle: STREAM_RENDER_THROTTLE_MS,
  });

  const chatScope = useRef({ loupistId, threadId: activeThread?.id, chat, epoch: 0 });
  if (
    chatScope.current.loupistId !== loupistId ||
    chatScope.current.threadId !== activeThread?.id ||
    chatScope.current.chat !== chat
  ) {
    // Returning to a Chat does not revive operations from an earlier visit.
    chatScope.current = {
      loupistId,
      threadId: activeThread?.id,
      chat,
      epoch: chatScope.current.epoch + 1,
    };
  }
  useEffect(() => {
    setIsStopping(false);
    setError(null);
    setAnnouncement("");
  }, [loupistId, activeThread?.id, chat]);

  // The SDK replaces changed messages with new snapshots. Preserve unchanged
  // projections so token updates do not rebuild every saved answer and its refs.
  // Weak keys release removed/replaced messages; each Chat has its own cache.
  const projections = useMemo(() => new WeakMap<LoupistUIMessage, ChatMessage>(), [chat]);
  const messages = useMemo(() => uiMessages.map((message) => {
    let projected = projections.get(message);
    if (!projected) {
      projected = fromLoupistUIMessage(message);
      projections.set(message, projected);
    }
    return projected;
  }), [projections, uiMessages]);
  const messageKeys = useMemo(
    () => uiMessages.map((message) => message.id),
    [uiMessages],
  );
  const isRunning =
    status === "submitted" || status === "streaming" || isStopping;
  const lastMessageStatus = messages.at(-1)?.status;
  const activeProgress = [...(messages.at(-1)?.progress?.items ?? [])]
    .reverse()
    .find((item) => item.kind !== "commentary" && item.status === "running");
  const visibleAnnouncement = isStopping
    ? t("useLoupistChat.stoppingResponse")
    : isRunning
      ? activeProgress && activeProgress.kind !== "commentary"
        ? activeProgress.action
        : t("useLoupistChat.loupistIsGeneratingAResponse")
      : lastMessageStatus === "aborted"
        ? t("useLoupistChat.responseStopped")
        : lastMessageStatus === "error" || chatError
          ? t("useLoupistChat.responseFailed")
          : lastMessageStatus === "complete"
            ? t("useLoupistChat.responseComplete")
            : announcement;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    if (!activeThread?.id || isRunning) return;
    const threadId = activeThread.id;
    setThreads((current) => {
      const thread = current.find((candidate) => candidate.id === threadId);
      if (!thread || sameThreadMessages(thread.messages, messages)) {
        return current;
      }
      return current.map((candidate) =>
        candidate.id === threadId ? { ...candidate, messages } : candidate,
      );
    });
  }, [activeThread?.id, isRunning, messages, setThreads]);

  const submit = useCallback(
    (
      threadId: string,
      text: string,
      privateChat: boolean,
      modelProfileId: string,
      thinkingLevel: string,
      runOptions: Record<string, string>,
      files: FileUIPart[],
      pins: ChatKnowledgePin[] = [],
      selectedArtifact?: ChatArtifactSelection,
      asOf?: string,
    ): Promise<ChatAdmission> => {
      const capturedEpoch = chatScope.current.epoch;
      const now = new Date().toISOString();
      setThreads((current) =>
        current.map((thread) =>
          thread.id === threadId
            ? {
                ...thread,
                use_memory: !privateChat,
                update_memory: !privateChat,
                updated_at: now,
              }
            : thread,
        ),
      );
      setError(null);
      setAnnouncement(t("useLoupistChat.loupistIsGeneratingAResponse"));
      return new Promise<ChatAdmission>((resolve) => {
        let settled = false;
        let admitted = false;
        const optimisticId = `user-${crypto.randomUUID()}`;
        const onAdmission = (receipt: ChatAdmission) => {
          if (settled) return;
          settled = true;
          admitted = receipt.accepted;
          if (!receipt.accepted && mountedRef.current && chatScope.current.epoch === capturedEpoch) {
            setError(receipt.error);
          }
          resolve(receipt);
        };
        void sendMessage(
          {
            id: optimisticId,
            role: "user",
            parts: [...files, { type: "text", text }],
            metadata: { created_at: now, status: "complete", ...(asOf ? { as_of: asOf } : {}) },
          },
          {
            body: {
              onAdmission,
              loupist_id: loupistId,
              thread_id: threadId,
              privateChat,
              model_profile_id: modelProfileId,
              thinking_level: thinkingLevel,
              run_options: runOptions,
              selected_artifact: selectedArtifact,
              ...(asOf ? { as_of: asOf } : {}),
              pins: pins.map((pin) => ({
                kind: pin.kind,
                recordId: pin.recordId,
                revisionId: pin.revisionId,
                objectId: pin.objectId,
                spanId: pin.spanId,
                ...(pin.selection ? { selection: pin.selection } : {}),
                label: pin.label,
              })),
            },
          },
        ).then(() => {
          onAdmission({ accepted: false, error: t("useLoupistChat.theReplyDidNotStartCheckTheConversationBeforeTryingAgain") });
        }).catch((cause) => {
          onAdmission({ accepted: false, error: errorMessage(cause, t("useLoupistChat.couldNotStartTheResponse")) });
        }).finally(() => {
          // A failed admission belongs in the outbox, not the saved conversation.
          if (!admitted && chat.messages) chat.messages = chat.messages.filter((message) => message.id !== optimisticId);
        });
      });
    },
    [chat, loupistId, sendMessage, setThreads],
  );

  const commitSteers = useCallback(
    (steers: Array<{ id: string; text: string; createdAt: string }>) => {
      if (steers.length === 0) return;
      const append = (current: LoupistUIMessage[]) => {
        const existing = new Set(
          current.map((message) => message.metadata?.native_message_id ?? message.id),
        );
        const incoming = steers
          .filter((item) => !existing.has(item.id))
          .map((item) =>
            toLoupistUIMessage({
              id: item.id,
              role: "user",
              content: item.text,
              steer: true,
              created_at: item.createdAt,
              status: "complete",
            }),
          );
        if (incoming.length === 0) return current;
        const insertAt =
          current.at(-1)?.role === "assistant"
            ? current.length - 1
            : current.length;
        return [
          ...current.slice(0, insertAt),
          ...incoming,
          ...current.slice(insertAt),
        ];
      };
      // A receipt can arrive after this hook has selected a different Chat.
      chat.messages = append(chat.messages);
    },
    [chat],
  );

  const steer = useCallback(
    async (text: string) => {
      if (!activeThread?.id || pendingSteering.has(chat)) return null;
      pendingSteering.add(chat);
      const capturedEpoch = chatScope.current.epoch;
      const stillThisChat = () => mountedRef.current && chatScope.current.epoch === capturedEpoch;
      setError(null);
      try {
        const receipt = await bridge.chatSteer({
          loupist_id: loupistId,
          thread_id: activeThread.id,
          prompt: text,
        });
        if (stillThisChat()) setAnnouncement(t("useLoupistChat.guidanceSentToTheActiveResponse"));
        return {
          id: receipt.message_id,
          text: receipt.prompt,
          createdAt: receipt.created_at,
        };
      } catch (cause) {
        if (!stillThisChat()) return null;
        setError(errorMessage(cause, t("useLoupistChat.couldNotSteerTheActiveResponse")));
        return null;
      } finally {
        pendingSteering.delete(chat);
      }
    },
    [activeThread?.id, bridge, chat, loupistId],
  );


  const abort = useCallback(async () => {
    if (!isRunning || isStopping || !activeThread?.id) return;
    const threadId = activeThread.id;
    const capturedLoupist = loupistId;
    const capturedEpoch = chatScope.current.epoch;
    const expectedMessageCount = uiMessages.length;
    const stillThisChat = () =>
      mountedRef.current &&
      chatScope.current.epoch === capturedEpoch &&
      chatScope.current.chat === chat &&
      chatScope.current.loupistId === capturedLoupist &&
      chatScope.current.threadId === threadId;
    setIsStopping(true);
    try {
      await stop();
      const deadline = Date.now() + 10_000;
      while (Date.now() < deadline) {
        if (!stillThisChat()) return;
        const active = (await bridge.runActivityList()).some(
          (run) =>
            run.kind === "chat" &&
            run.loupist_id === capturedLoupist &&
            run.target === threadId,
        );
        if (!stillThisChat()) return;
        if (!active) {
          const workspace = await bridge.loupistSelect(capturedLoupist);
          if (!stillThisChat()) return;
          const canonical = workspace.threads.find(
            (thread) => thread.id === threadId,
          );
          const terminal = canonical?.messages.at(-1);
          if (
            canonical &&
            canonical.messages.length >= expectedMessageCount &&
            terminal?.role === "assistant" &&
            ["complete", "aborted", "error"].includes(
              terminal.status ?? "complete",
            )
          ) {
            if (!stillThisChat()) return;
            setMessages((current) => fromStoreKeepingAnswers(current, canonical.messages));
            return;
          }
        }
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      }
      if (!stillThisChat()) return;
      throw new Error(
        t("useLoupistChat.nativeChatDidNotFinishSavingTheStoppedResponse"),
      );
    } catch (cause) {
      if (!stillThisChat()) return;
      setMessages(markLatestAssistantAborted);
      setError(errorMessage(cause, t("useLoupistChat.couldNotStopTheResponse")));
    } finally {
      if (stillThisChat()) setIsStopping(false);
    }
  }, [
    activeThread?.id,
    bridge,
    chat,
    loupistId,
    isRunning,
    isStopping,
    setMessages,
    stop,
    uiMessages.length,
  ]);

  const retryLast = useCallback(async (): Promise<{ prompt: string; files: FileUIPart[]; threadId: string; detached: boolean; asOf?: string } | null> => {
    const threadId = activeThread?.id;
    const capturedChat = chat;
    const capturedEpoch = chatScope.current.epoch;
    if (!threadId || isRunning || isStopping || retryGuard.current) return null;
    const assistant = messages.at(-1);
    const request = messages.slice(0, -1).reverse().find(message => !message.steer);
    if (!assistant || assistant.role !== "assistant" || (assistant.status !== "error" && assistant.status !== "aborted")) return null;
    if (!request || request.role !== "user" || (!request.content.trim() && !(request.attachments?.length))) return null;
    const ownsAttempt = () => chatScope.current.chat === capturedChat
      && chatScope.current.epoch === capturedEpoch
      && chatScope.current.threadId === threadId;
    retryGuard.current = true;
    try {
      const readable = (request.attachments ?? []) as Array<{ id: string; filename: string; media_type: string; url?: string }>;
      const asFile = (attachment: { filename: string; media_type: string }, url: string): FileUIPart => ({
        type: "file",
        mediaType: attachment.media_type,
        filename: attachment.filename,
        url,
      });
      const files: FileUIPart[] = [];
      const localOnly = readable.some((attachment) => !attachment.id.startsWith("attachment-"));
      if (localOnly) {
        const inline = readable.flatMap((attachment) => attachment.url?.startsWith("data:") ? [asFile(attachment, attachment.url)] : []);
        if (inline.length === readable.length) files.push(...inline);
        else {
          const workspace = await bridge.loupistSelect(loupistId);
          const stored = workspace.threads.find((thread) => thread.id === threadId)?.messages ?? [];
          const storedAssistant = stored.at(-1);
          const storedUser = stored.slice(0, -1).reverse().find(message => !message.steer);
          if (!storedAssistant || storedAssistant.role !== "assistant" || (storedAssistant.status !== "error" && storedAssistant.status !== "aborted") || storedUser?.role !== "user") {
            throw new Error(t("useLoupistChat.theAttachedFilesFromThatRequestCouldNotBe"));
          }
          for (const attachment of storedUser.attachments ?? []) {
            const read = await bridge.chatAttachmentRead(loupistId, threadId, storedUser.id, attachment.id);
            files.push(asFile(attachment, read.data_url));
          }
        }
      } else {
        for (const attachment of readable) {
          const url = attachment.url?.startsWith("data:")
            ? attachment.url
            : (await bridge.chatAttachmentRead(loupistId, threadId, request.id, attachment.id)).data_url;
          files.push(asFile(attachment, url));
        }
      }
      const receipt = await bridge.chatRetryLast({ loupist_id: loupistId, thread_id: threadId });
      const removed = new Set([request.id, assistant.id]);
      capturedChat.messages = capturedChat.messages.filter(message => !removed.has(message.metadata?.native_message_id ?? message.id));
      setThreads((current) => current.map((thread) => thread.id === threadId ? { ...thread, messages: thread.messages.filter(message => !removed.has(message.id)) } : thread));
      return { prompt: receipt.prompt, files, threadId, detached: !ownsAttempt(), ...(receipt.as_of ? { asOf: receipt.as_of } : {}) };
    } catch (cause) {
      if (ownsAttempt()) setError(errorMessage(cause, t("useLoupistChat.couldNotTryThatResponseAgain")));
      return null;
    } finally {
      retryGuard.current = false;
    }
  }, [activeThread?.id, bridge, chat, loupistId, isRunning, isStopping, messages, setThreads]);

  const saveFeedback = useCallback(async (messageId: string, value: Pick<ChatFeedback, "rating" | "correction">) => {
    if (!activeThread?.id) throw new Error(t("useLoupistChat.chatThreadIsUnavailable"));
    const feedback = await bridge.chatFeedbackSet({ loupist_id: loupistId, thread_id: activeThread.id, message_id: messageId, ...value });
    // Update the captured registry Chat, even if the user has navigated elsewhere.
    chat.messages = chat.messages.map((message) => (message.metadata?.native_message_id ?? message.id) === messageId
      ? { ...message, metadata: { ...message.metadata, feedback } } : message);
    return feedback;
  }, [activeThread?.id, bridge, chat, loupistId]);

  /**
   * Shows a thread the host just rewrote (an edit fork or another version,
   * X18): the SDK's live messages and the thread list both take its messages.
   */
  const replaceThread = useCallback((thread: ChatThread) => {
    setMessages((current) => fromStoreKeepingAnswers(current, thread.messages));
    setThreads((current) => current.map((candidate) => candidate.id === thread.id
      ? { ...candidate, messages: thread.messages, versions: thread.versions ?? [], updated_at: thread.updated_at }
      : candidate));
  }, [setMessages, setThreads]);

  return {
    replaceThread,
    saveFeedback,
    messages,
    messageKeys,
    isRunning,
    isStopping,
    announcement: visibleAnnouncement,
    // A terminal assistant error has its own localized notice. Local operation
    // failures still belong beside the composer, including a failed retry.
    error: error ?? (messages.at(-1)?.role === "assistant" && lastMessageStatus === "error" ? null : chatError?.message ?? null),
    submit,
    steer,
    commitSteers,
    abort,
    retryLast,
  };
}
