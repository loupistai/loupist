import type { ChatVersionSummary } from "../../contracts/chat";
import i18n, { useTranslation } from "@/i18n";
import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { type SaveAnswerFeedback } from "./AnswerFeedback";
import {
  Conversation,
  ConversationContent,
} from "@/components/ai-elements/conversation";
import { ChatStatusPill, type ChatRunStatus } from "./ChatStatusPill";
import type { ChatArtifactRef, ChatMessage, GraphJobView } from "../../types";
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
  progressFocus?: string | null;
  /** Guidance sent while the latest reply runs; shown in its timeline. */
  guiding?: readonly string[];
  onReadWorldJob?: (jobId: string) => Promise<GraphJobView>;
  onWorldChanged?: () => void;
  onSuggestion: (suggestion: string) => void;
  /** False with no AI account: the first-visit checklist asks to connect one. */
  accountConnected?: boolean;
  onConnectAccount?: () => void;
  onOpenFollowing?: () => void;
  /** Puts an inline chart in the next question. */
  onAskArtifact?: (artifact: ChatArtifactRef) => void;
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
  /** The Loupist's home Thread: results arrive here and the morning message closes it. */
  home?: boolean;
  /** An empty topic Chat offers starter questions unless its composer already holds a question (Look back). */
  offerQuestions?: boolean;
  /** The composer's as-of date: the morning message leaves out its look-ahead event strip. */
  asOf?: string;
  /** The Loupist's template: a class Loupist's first morning message names the connections it works best with. */
  loupistTemplate?: LoupistTemplateId;
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
  progressFocus,
  guiding,
  onReadWorldJob,
  onWorldChanged,
  onSuggestion,
  accountConnected,
  onConnectAccount,
  onOpenFollowing,
  onAskArtifact,
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
  home = false,
  offerQuestions = true,
  asOf,
  loupistTemplate,
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
  const editUser = useCallback((message: ChatMessage) => {
    const index = messagesRef.current.findIndex(item => item.id === message.id);
    if (index >= 0) onEditUser?.(message, index);
  }, [onEditUser]);
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
  // eslint-disable-next-line react-hooks/exhaustive-deps -- the joined run ids are the key, so the same deliveries are marked once.
  }, [home, active, bridge, loupistId, deliveredRunIds.join(",")]);
  // The morning message loads after the Thread first scrolls; its arrival is part of the scroll revision.
  const [morningSettled, setMorningSettled] = useState(false);
  const morning = (mode: "full" | "news" | "questions") => bridge ? <MorningMessage
    key={`${loupistId}:${mode}`}
    mode={mode}
    bridge={bridge}
    loupistId={loupistId}
    loupistName={loupistName}
    onAsk={onSuggestion}
    accountConnected={accountConnected}
    onConnectAccount={onConnectAccount}
    onOpenFollowing={onOpenFollowing}
    deliveredRunIds={home ? deliveredRunIds : undefined}
    onSettled={() => setMorningSettled(true)}
    asOf={asOf}
    template={loupistTemplate}
  /> : null;
  // Growth inside a reply is followed by the scroll owner's ResizeObserver; only
  // a new message, a settled reply or the morning message needs a layout pass.
  const scrollRevision = useMemo(() => [
    messages.length,
    messageKeys.at(-1) ?? "",
    messages.at(-1)?.status ?? "",
    morningSettled ? "morning" : "",
  ].join(":"), [messageKeys, messages, morningSettled]);
  const latestPromptKey = messageKeys[lastUserIndex] ?? messages[lastUserIndex]?.id ?? "";
  const lastAssistantIndex = messages.reduce((last, message, index) => (message.role === "assistant" ? index : last), -1);
  // The user's question each answer replies to, offered again by a setup card once the connection is ready.
  const previousQuestions = useMemo(() => {
    let question: string | undefined;
    return messages.map((message) => {
      if (message.role === "user" && !message.steer) question = message.content;
      return question;
    });
  }, [messages]);
  const dividerKey = JSON.stringify(messages.map(({ id, created_at }) => [id, created_at]));
  const dividerDay = new Date().toDateString();
  const dividers = useMemo(() => messages.map((_, index) => dividersBefore(messages, index, newSinceId)),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- dividers read only ids/timestamps; streaming text does not change a date boundary.
    [dividerKey, dividerDay, newSinceId, i18n.language]);
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
            {dividers[index]?.map((divider) => <ChatDivider key={divider.kind} divider={divider} onPassed={divider.kind === "new" ? clearNewSince : undefined} />)}
            {message.watch ? (
            <ThreadWatch message={message} bridge={bridge} loupistId={loupistId} loupistName={loupistName} active={active} />
          ) : message.earnings ? (
            <ThreadEarnings message={message} bridge={bridge} loupistId={loupistId} loupistName={loupistName} />
          ) : message.vocabulary ? (
            <ThreadVocabulary message={message} bridge={bridge} loupistId={loupistId} loupistName={loupistName} />
          ) : message.link ? (
            <ThreadLink message={message} bridge={bridge} loupistId={loupistId} loupistName={loupistName} />
          ) : message.alert ? (
            <ThreadAlert message={message} bridge={bridge} loupistId={loupistId} loupistName={loupistName} />
          ) : message.delivery ? (
            <ThreadDelivery message={message} loupistName={loupistName} />
          ) : message.handoff ? (
            <ThreadHandoff message={message} loupistName={loupistName} />
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
              progressFocus={progressFocus}
              guiding={index === messages.length - 1 && message.status === "streaming" ? guiding : undefined}
              stopping={index === messages.length - 1 && Boolean(run?.stopping)}
              onNextStep={index === messages.length - 1 && message.role === "assistant" && message.status === "complete" ? onSuggestion : undefined}
              onFillComposer={message.role === "assistant" ? onSuggestion : undefined}
              previousQuestion={message.role === "assistant" ? previousQuestions[index] : undefined}
              onReadWorldJob={onReadWorldJob}
              onWorldChanged={onWorldChanged}
              onAskArtifact={onAskArtifact}
              askArtifactDisabled={run?.running}
              onReadAttachment={onReadAttachment}
              mentionSkills={mentionSkills}
              mentionPlugins={mentionPlugins}
              onEditUser={message.role === "user" && onEditUser ? editUser : undefined}
              editDisabled={Boolean(run?.running)}
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
