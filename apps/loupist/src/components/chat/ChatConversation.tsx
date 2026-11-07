import type { ChatVersionSummary } from "../../contracts/chat";
import type { OpenLinkOptions } from "../../chat/links";
import i18n, { useTranslation } from "@/i18n";
import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { type SaveAnswerFeedback } from "./AnswerFeedback";
import {
  Conversation,
  ConversationContent,
} from "@/components/ai-elements/conversation";
import { ChatStatusPill, type ChatRunStatus } from "./ChatStatusPill";
import type {
  ChatArtifactRef,
  ChatEvidenceRef,
  ChatMessage,
  KnowledgeLocation,
  GraphJobView,
} from "../../types";
import type { LoupistBridge, LoupistTemplateId } from "../../types";
import { revealFocused } from "../../lib/revealFocused";
import { ChatMessageCard } from "./ChatMessageCard";
import type { ProviderErrorAction } from "./ChatErrorState";
import { QuoteAsk } from "./QuoteAsk";
import { MorningMessage } from "./MorningMessage";
import { ThreadDelivery } from "./ThreadDelivery";
import { ThreadHandoff } from "./ThreadHandoff";
import { ThreadAlert } from "./ThreadAlert";
import { ThreadWatch } from "./ThreadWatch";
import { ThreadEarnings } from "./ThreadEarnings";
import { ThreadLink } from "./ThreadLink";
import { ThreadVocabulary } from "./ThreadVocabulary";
import { ChatDivider } from "./ChatDivider";
import { chatSeenAt, dividersBefore, firstNewSince, markChatSeen } from "../../chat/dividers";

const t = i18n.getFixedT(null, "chat");



type Props = {
  /** Adds a phrase selected in an answer to the composer as a quote chip. */
  onQuote?: (messageId: string, text: string) => void;
  onSaveFeedback?: SaveAnswerFeedback;
  onTeachFeedback?: (text: string) => void;
  feedbackBusy?: boolean;
  active?: boolean;
  messages: ChatMessage[];
  messageKeys: string[];
  loupistId: string;
  chatId?: string;
  loupistName: string;
  workspacePath?: string;
  onOpenFile?: (path: string) => void;
  onOpenEvidence?: (evidence: ChatEvidenceRef) => void;
  progressFocus?: string | null;
  /** Guidance sent while the latest reply runs; shown in its timeline. */
  guiding?: readonly string[];
  onReadWorldJob?: (jobId: string) => Promise<GraphJobView>;
  onWorldChanged?: () => void;
  onSuggestion: (suggestion: string) => void;
  onOpenMemory: (location: KnowledgeLocation, title: string) => void;
  onOpenMemoryPage: (location: KnowledgeLocation) => void;
  onOpenArtifact: (artifact: ChatArtifactRef) => void;
  /** Puts an inline chart in the next question. */
  onAskArtifact?: (artifact: ChatArtifactRef) => void;
  onOpenSchedule?: (taskId: string) => void;
  onOpenLink: (url: string, options?: OpenLinkOptions) => Promise<void>;
  onReadAttachment: (
    messageId: string,
    attachmentId: string,
  ) => Promise<string>;
  mentionSkills?: ReadonlyArray<{ id: string; name: string }>;
  mentionPlugins?: ReadonlyArray<{ id: string; name: string }>;
  /** Edit forks (X18): editing a sent message starts a new version from it. */
  onEditUser?: (message: ChatMessage, index: number) => void;
  /** The other versions of this chat's tail, and how to show one. */
  versions?: ChatVersionSummary[];
  onSwitchVersion?: (versionId: string) => void;
  onRetry?: () => void;
  /** A provider id to its display name, for failed answers (X11). */
  providerLabel?: (providerId?: string) => string | undefined;
  /** Busy answers in a row in this Chat (X11 countdown). */
  retryAttempt?: number;
  /** An account action from the newest failed answer (Reconnect, Use an API key, Manage usage…). */
  onProviderAction?: (action: ProviderErrorAction, provider: string | undefined) => void;
  bridge?: LoupistBridge;
  /** The active run, for the status pill shown while the reader is scrolled up. */
  run?: ChatRunStatus;
  /** The Loupist's focus, for the morning message's questions. */
  loupistFocus?: string;
  /** Opens a briefing run in the Briefings tab. */
  onOpenBriefing?: (runId: string) => void;
  /** The Loupist's home Thread: results arrive here and the morning message closes it. */
  home?: boolean;
  /** Opens the Chat a scheduled result came from. */
  onOpenRunChat?: (threadId: string) => void;
  /** An empty topic Chat offers starter questions unless its composer already holds a question (Look back). */
  offerQuestions?: boolean;
  /** The composer's as-of date: the morning message leaves out its look-ahead event strip. */
  asOf?: string;
  /** The Loupist's template: a class Loupist's first morning message names the connections it works best with. */
  loupistTemplate?: LoupistTemplateId;
  /** Opens Settings › Connections from the morning message. */
  onOpenConnections?: () => void;
};

export function ChatConversation({
  onQuote,
  active = true,
  messages,
  messageKeys,
  loupistId,
  chatId,
  loupistName,
  workspacePath,
  onOpenFile,
  onOpenEvidence,
  progressFocus,
  guiding,
  onReadWorldJob,
  onWorldChanged,
  onSuggestion,
  onOpenMemory,
  onOpenMemoryPage,
  onOpenArtifact,
  onAskArtifact,
  onOpenSchedule,
  onOpenLink,
  onReadAttachment,
  mentionSkills,
  mentionPlugins,
  onEditUser,
  versions = [],
  onSwitchVersion,
  onRetry,
  providerLabel,
  retryAttempt,
  onProviderAction,
  bridge,
  onSaveFeedback,
  onTeachFeedback,
  feedbackBusy = false,
  run,
  loupistFocus,
  onOpenBriefing,
  home = false,
  onOpenRunChat,
  offerQuestions = true,
  asOf,
  loupistTemplate,
  onOpenConnections,
}: Props) {
  useTranslation("chat");
  // Edit forks (X18): the versions that part from the chat at a sent message, nearest first.
  const versionAt = (index: number, direction: "earlier" | "later", show: (versionId: string) => void) => {
    const anchor = index > 0 ? messages[index - 1]!.id : null;
    const shownAt = Date.parse(messages[index]!.created_at);
    const candidates = versions.filter((version) => version.after_message_id === anchor
      && (direction === "earlier" ? version.first_message_at_ms < shownAt : version.first_message_at_ms > shownAt));
    if (!candidates.length) return undefined;
    const nearest = candidates.reduce((best, version) => (direction === "earlier"
      ? version.first_message_at_ms > best.first_message_at_ms
      : version.first_message_at_ms < best.first_message_at_ms) ? version : best);
    return () => show(nearest.id);
  };
  // "New since you left" (X5): answers that finished after the user last had this chat in view.
  const messagesRef = useRef(messages);
  messagesRef.current = messages;
  // The answers already finished when the window lost focus, until it is focused again.
  const answeredAtBlur = useRef<Set<string> | null>(null);
  const [newSinceId, setNewSinceId] = useState<string | undefined>(() => chatId && active ? firstNewSince(messages, chatSeenAt(chatId)) : undefined);
  const clearNewSince = useCallback(() => {
    setNewSinceId(undefined);
    if (chatId) markChatSeen(chatId);
  }, [chatId]);
  useEffect(() => {
    if (!chatId) return undefined;
    // Coming back shows what finished meanwhile; leaving records what was seen.
    if (active) {
      const pending = firstNewSince(messagesRef.current, chatSeenAt(chatId));
      if (pending) setNewSinceId(pending);
    }
    const onVisibility = () => {
      if (!active) return;
      if (document.visibilityState === "hidden") markChatSeen(chatId);
      else {
        const pending = firstNewSince(messagesRef.current, chatSeenAt(chatId));
        if (pending) setNewSinceId(pending);
      }
    };
    // Another app in front while this chat stays on screen: the answers that finish
    // before the window is focused again are new, whatever their clocks say.
    const onBlur = () => {
      if (active) answeredAtBlur.current = new Set(messagesRef.current.flatMap((message) => settledAnswer(message) ? [message.id] : []));
    };
    const onFocus = () => {
      const before = answeredAtBlur.current;
      answeredAtBlur.current = null;
      if (!active || !before) return;
      const pending = messagesRef.current.find((message) => settledAnswer(message) && !before.has(message.id))?.id;
      if (pending) setNewSinceId(pending);
    };
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("blur", onBlur);
    window.addEventListener("focus", onFocus);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("focus", onFocus);
      if (active) markChatSeen(chatId);
    };
  }, [active, chatId]);
  // Sending a message answers what was new.
  const lastUserIndex = messages.reduce((last, message, index) => message.role === "user" ? index : last, -1);
  const newSinceIndex = newSinceId ? messages.findIndex((message) => message.id === newSinceId) : -1;
  useEffect(() => {
    if (newSinceId && lastUserIndex > newSinceIndex) clearNewSince();
  }, [clearNewSince, lastUserIndex, newSinceId, newSinceIndex]);
  // Once the user writes in the Thread this visit, the morning news has been answered.
  const [visitStartMs] = useState(() => Date.now());
  const hasUserMessage = lastUserIndex >= 0;
  const repliedThisVisit = messages.some((message) => message.role === "user" && Date.parse(message.created_at) > visitStartMs);
  // Results posted into the Thread count as read once the Thread shows them.
  const deliveredRunIds = useMemo(() => messages.flatMap((message) => message.delivery ? [message.delivery.run_id] : []).slice(-20), [messages]);
  useEffect(() => {
    if (!home || !active || !bridge || !deliveredRunIds.length) return;
    const timer = window.setTimeout(() => { void bridge.inboxMarkRead({ loupistId, runIds: deliveredRunIds }).catch(() => {}); }, 1000);
    return () => window.clearTimeout(timer);
  }, [home, active, bridge, loupistId, deliveredRunIds.join(",")]);
  // The morning message loads after the Thread first scrolls; its arrival is part of the scroll revision.
  const [morningSettled, setMorningSettled] = useState(false);
  const morning = (mode: "full" | "news" | "questions") => bridge ? <MorningMessage
    key={`${loupistId}:${mode}`}
    mode={mode}
    bridge={bridge}
    loupistId={loupistId}
    loupistName={loupistName}
    focus={loupistFocus}
    onAsk={onSuggestion}
    onOpenMapNode={({ id, recordId }) => onOpenMemoryPage({ loupistId, surface: "world", recordId, objectId: id, selection: { kind: "node", id } })}
    onOpenMap={() => onOpenMemoryPage({ loupistId, surface: "world", view: { followCurrent: true, whatsNew: true } })}
    onOpenBriefing={onOpenBriefing}
    onOpenJournal={(journalId, title) => onOpenMemory({ loupistId, surface: "note", objectId: journalId, selection: { kind: "journal", journalId } }, title)}
    onOpenLink={url => void onOpenLink(url)}
    deliveredRunIds={home ? deliveredRunIds : undefined}
    onSettled={() => setMorningSettled(true)}
    asOf={asOf}
    template={loupistTemplate}
    onOpenConnections={onOpenConnections}
  /> : null;
  const progressScrollKey = (progress: ChatMessage["progress"]) => {
    const item = progress?.items.at(-1);
    if (!item) return "0";
    const status = "status" in item ? item.status : "";
    const revision = item.kind === "commentary" ? item.text.length : item.outputRevision ?? 0;
    return `${progress!.items.length}:${item.id}:${status}:${revision}`;
  };
  const scrollRevision = useMemo(() => {
    const last = messages.at(-1);
    return [
      messages.length,
      messageKeys.at(-1) ?? "",
      last?.status ?? "",
      // A phase change moves the preamble between the answer and the tool trace.
      last?.phase ?? "",
      last?.content.length ?? 0,
      progressScrollKey(last?.progress),
      morningSettled ? "morning" : "",
    ].join(":");
  }, [messageKeys, messages, morningSettled]);
  const latestPromptIndex = useMemo(() => {
    for (let index = messages.length - 1; index >= 0; index -= 1) {
      if (messages[index]!.role === "user") return index;
    }
    return -1;
  }, [messages]);
  const latestPromptKey = messageKeys[latestPromptIndex] ?? messages[latestPromptIndex]?.id ?? "";
  const lastAssistantIndex = messages.reduce((last, message, index) => (message.role === "assistant" ? index : last), -1);
  // The user's question each answer replies to, offered again by a setup card once the connection is ready.
  const previousQuestions = useMemo(() => {
    let question: string | undefined;
    return messages.map((message) => {
      if (message.role === "user" && !message.steer) question = message.content;
      return question;
    });
  }, [messages]);
  return (
    <Conversation
      className="message-scroll"
      onFocus={revealFocused}
      aria-label={t("chatConversation.conversation")}
      aria-relevant="additions"
      scrollRevision={scrollRevision}
      scrollResetKey={latestPromptKey}
      autoScroll={messages.length > 0}
      footer={messages.length > 0 ? <ChatStatusPill message={messages.at(-1)?.role === "assistant" ? messages.at(-1) : undefined} run={run} /> : null}
    >
      {onQuote && active ? <QuoteAsk onQuote={onQuote} /> : null}
      <ConversationContent className="conversation-content">
        {!messages.length ? (
          // The empty home Thread greets; an empty topic Chat only offers questions.
          home ? morning("full") : offerQuestions ? morning("questions") : null
        ) : (<>
          {messages.map((message, index) => <Fragment key={messageKeys[index] ?? message.id}>
            {dividersBefore(messages, index, newSinceId).map((divider) => <ChatDivider key={divider.kind} divider={divider} onPassed={divider.kind === "new" ? clearNewSince : undefined} />)}
            {message.watch ? (
            <ThreadWatch message={message} bridge={bridge} loupistId={loupistId} loupistName={loupistName} active={active} onOpenLink={url => void onOpenLink(url)} onOpenSchedule={onOpenSchedule} />
          ) : message.earnings ? (
            <ThreadEarnings message={message} bridge={bridge} loupistId={loupistId} loupistName={loupistName} onOpenLink={url => void onOpenLink(url)} />
          ) : message.vocabulary ? (
            <ThreadVocabulary message={message} bridge={bridge} loupistId={loupistId} loupistName={loupistName} />
          ) : message.link ? (
            <ThreadLink message={message} bridge={bridge} loupistId={loupistId} loupistName={loupistName} onOpenMap={(nodeIds) => onOpenMemoryPage({ loupistId, surface: "world", view: { highlightEntityIds: nodeIds } })} />
          ) : message.alert ? (
            <ThreadAlert message={message} bridge={bridge} loupistId={loupistId} loupistName={loupistName} onOpenLink={url => void onOpenLink(url)} />
          ) : message.delivery ? (
            <ThreadDelivery message={message} loupistName={loupistName} onOpenRunChat={onOpenRunChat} />
          ) : message.handoff ? (
            <ThreadHandoff message={message} loupistName={loupistName} onOpenRunChat={onOpenRunChat} />
          ) : (
            <ChatMessageCard
              bridge={bridge}
              active={active}
              message={message}
              onSaveFeedback={onSaveFeedback}
              onTeachFeedback={onTeachFeedback}
              feedbackBusy={feedbackBusy}
              loupistId={loupistId}
              chatId={chatId}
              loupistName={loupistName}
              workspacePath={workspacePath}
              onOpenFile={onOpenFile}
              onOpenEvidence={onOpenEvidence}
              progressFocus={progressFocus}
              guiding={index === messages.length - 1 && message.status === "streaming" ? guiding : undefined}
              onNextStep={index === messages.length - 1 && message.role === "assistant" && message.status === "complete" ? onSuggestion : undefined}
              onFillComposer={message.role === "assistant" ? onSuggestion : undefined}
              previousQuestion={message.role === "assistant" ? previousQuestions[index] : undefined}
              onReadWorldJob={onReadWorldJob}
              onWorldChanged={onWorldChanged}
              onOpenMemory={onOpenMemory}
              onOpenMemoryPage={onOpenMemoryPage}
              onOpenArtifact={onOpenArtifact}
              onAskArtifact={onAskArtifact}
              askArtifactDisabled={run?.running}
              onOpenSchedule={onOpenSchedule}
              onOpenLink={onOpenLink}
              onReadAttachment={onReadAttachment}
              mentionSkills={mentionSkills}
              mentionPlugins={mentionPlugins}
              onEditUser={message.role === "user" && onEditUser && !run?.running ? () => onEditUser(message, index) : undefined}
              onEarlierVersion={message.role === "user" && onSwitchVersion ? versionAt(index, "earlier", onSwitchVersion) : undefined}
              onLaterVersion={message.role === "user" && onSwitchVersion ? versionAt(index, "later", onSwitchVersion) : undefined}
              onRetry={message.role === "assistant" && index === messages.length - 1 && (message.status === "error" || message.status === "aborted") ? onRetry : undefined}
              latest={message.role === "assistant" && index === lastAssistantIndex}
              providerName={message.status === "error" ? providerLabel?.(message.execution_model?.provider) : undefined}
              retryAttempt={index === messages.length - 1 ? retryAttempt : undefined}
              onProviderAction={index === messages.length - 1 ? onProviderAction : undefined}
              note={message.role === "assistant" && message.replaced_model && message.execution_model?.name
                ? t("chatError.modelGone", { old: message.replaced_model, new: message.execution_model.name })
                : undefined}
            />
          )}
          </Fragment>)}
          {home && !hasUserMessage ? morning("full") : home && !repliedThisVisit ? morning("news") : null}
        </>)}
      </ConversationContent>
    </Conversation>
  );
}

/** A finished answer, as the "New since you left" divider counts them. */
const settledAnswer = (message: ChatMessage) => message.role === "assistant" && message.status === "complete";
