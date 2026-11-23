import type { ChatVersionSummary } from "../../contracts/chat";
import i18n, { useTranslation } from "@/i18n";
import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { type SaveAnswerFeedback } from "./AnswerFeedback";
import {
  Conversation,
  ConversationContent,
} from "@/components/ai-elements/conversation";
import { ChatStatusPill, type ChatRunStatus } from "./ChatStatusPill";
import type { ChatArtifactRef, ChatMessage, ChatThread, GraphJobView } from "../../types";
import type { HomeHandoff, LoupistBridge, LoupistTemplateId, ResearchHandoffChoices } from "../../types";
import { revealFocused } from "../../lib/revealFocused";
import { ChatMessageCard } from "./ChatMessageCard";
import type { ProviderErrorAction } from "./ChatErrorState";
import { QuoteAsk } from "./QuoteAsk";
import { useSelectedCitations } from "./useSelectedCitations";
import { MorningMessage } from "./MorningMessage";
import { BotGreeting } from "./BotGreeting";
import { ChatSurfaceProvider, type ChatSurface } from "./turnMotion";
import type { LoupistAvatarId } from "../../contracts/loupist";
import { ThreadDelivery } from "./ThreadDelivery";
import { BotScheduleTasks, useBotScheduleTasks } from "./BotScheduleTasks";
import { ThreadHandoff } from "./ThreadHandoff";
import { ThreadAlert } from "./ThreadAlert";
import { ThreadLink } from "./ThreadLink";
import { ThreadVocabulary } from "./ThreadVocabulary";
import { ChatDivider } from "./ChatDivider";
import { chatSeenAt, dividersBefore, firstNewSince, markChatSeen } from "../../chat/dividers";
import { lastTurnMessageIndex } from "../../chat/turnMessages";

const t = i18n.getFixedT(null, "chat");



type Props = {
  /** Adds a phrase selected in an answer to the composer as a quote chip. */
  onQuote?: (messageId: string, text: string) => void;
  onSaveFeedback?: SaveAnswerFeedback;
  onTeachFeedback?: (text: string) => void;
  active?: boolean;
  messages: ChatMessage[];
  messageKeys: string[];
  loupistId: string;
  chatId?: string;
  loupistName: string;
  /** The Loupist's face; Bot's messages carry it. */
  loupistAvatar?: LoupistAvatarId;
  workspacePath?: string;
  progressFocus?: string | null;
  /** The Loupist's conversations, to name the one a saving reply waits for. */
  threads?: readonly ChatThread[];
  /** Skips one reply's pending save, by its run (a reply's own Skip saving). */
  onSkipSave?: (runId: string) => void;
  /** Saves whose skip was asked for and has not resolved yet. */
  skippingSaves?: ReadonlySet<string>;
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
  /** The Loupist's template: a class Loupist's first morning message names the connections it works best with. */
  loupistTemplate?: LoupistTemplateId;
  /** The composer's run choices, read when a link's "Look into it" starts research. */
  composerChoices?: () => ResearchHandoffChoices;
  researchSessions?: readonly ChatThread[];
  /** Look into it sessions at work, by link: Bot posts nothing for them until they end. */
  runningLinks?: ReadonlyMap<string, HomeHandoff>;
};

export function ChatConversation({
  onQuote,
  active = true,
  messages,
  messageKeys,
  loupistId,
  chatId,
  loupistName,
  loupistAvatar,
  workspacePath,
  progressFocus,
  threads,
  onSkipSave,
  skippingSaves,
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
  run,
  home = false,
  offerQuestions = true,
  loupistTemplate,
  composerChoices,
  researchSessions,
  runningLinks,
}: Props) {
  useTranslation("chat");
  useSelectedCitations(active);
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
    // A visible window behind another app still shows this chat: losing focus is not leaving.
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
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
  const contentRef = useRef<HTMLDivElement>(null);
  const deliveredRunIds = useMemo(() => messages.flatMap(message => message.delivery ? [message.delivery.run_id] : []), [messages]);
  const deliveryKey = JSON.stringify(deliveredRunIds);
  useEffect(() => {
    if (!home || !active || !bridge || !contentRef.current) return;
    const cards = [...contentRef.current.querySelectorAll<HTMLElement>("[data-delivery-run]")];
    if (!cards.length) return;
    const markRead = (ids: string[]) => bridge.inboxMarkRead({ loupistId, runIds: ids }).catch(() => {});
    // Older webviews cannot report visibility. Keep their existing batch behaviour.
    if (typeof IntersectionObserver === "undefined") {
      const timer = window.setTimeout(() => void markRead(cards.slice(-20).map(card => card.dataset.deliveryRun!)), 1000);
      return () => window.clearTimeout(timer);
    }
    const timers = new Map<Element, number>();
    const observer = new IntersectionObserver(entries => {
      for (const entry of entries) {
        window.clearTimeout(timers.get(entry.target));
        timers.delete(entry.target);
        if (!entry.isIntersecting || document.hidden) continue;
        timers.set(entry.target, window.setTimeout(() => {
          if (!document.hidden) {
            void markRead([(entry.target as HTMLElement).dataset.deliveryRun!]);
            observer.unobserve(entry.target);
          }
          timers.delete(entry.target);
        }, 1000));
      }
    });
    const observeVisible = () => {
      observer.disconnect();
      timers.forEach(timer => window.clearTimeout(timer));
      timers.clear();
      if (!document.hidden) cards.forEach(card => observer.observe(card));
    };
    observeVisible();
    document.addEventListener("visibilitychange", observeVisible);
    return () => {
      observer.disconnect();
      timers.forEach(timer => window.clearTimeout(timer));
      document.removeEventListener("visibilitychange", observeVisible);
    };
  }, [home, active, bridge, loupistId, deliveryKey]);
  // Home shown again (back from Following, Settings…): the checklist reads its steps again.
  const [wasActive, setWasActive] = useState(active);
  const [homeRevision, setHomeRevision] = useState(0);
  if (active !== wasActive) {
    setWasActive(active);
    if (active) setHomeRevision(homeRevision + 1);
  }
  const lastWrote = lastUserIndex >= 0 ? Date.parse(messages[lastUserIndex]!.created_at) : Number.NaN;
  const morning = (mode: "news" | "questions") => bridge ? <MorningMessage
    key={`${loupistId}:${mode}`}
    mode={mode}
    bridge={bridge}
    loupistId={loupistId}
    loupistName={loupistName}
    onAsk={onSuggestion}
    lastWroteAtMs={Number.isFinite(lastWrote) ? lastWrote : undefined}
    refreshRevision={homeRevision}
    deliveredRunIds={home ? deliveredRunIds.slice(-20) : undefined}
    template={loupistTemplate}
  /> : null;
  // Bot is a conversation with the Loupist: its face, bubbles and the greeting. A research session stays a document.
  const surface = useMemo<ChatSurface>(() => home ? { kind: "bot", loupist: { id: loupistId, name: loupistName, avatar: loupistAvatar } } : { kind: "session" },
    [home, loupistId, loupistName, loupistAvatar]);
  const greeting = bridge ? <BotGreeting key={loupistId} bridge={bridge} loupistId={loupistId} loupist={{ id: loupistId, name: loupistName, avatar: loupistAvatar }}
    onAsk={onSuggestion} accountConnected={accountConnected} onConnectAccount={onConnectAccount} onOpenFollowing={onOpenFollowing} refreshRevision={homeRevision} /> : null;
  const latestPromptKey = messageKeys[lastUserIndex] ?? messages[lastUserIndex]?.id ?? "";
  // Messages already here when the thread opened are drawn settled; a question sent now rises in.
  const [openedWith] = useState(() => new Set(messageKeys));
  const last = messages.at(-1);
  // The reply's shell appears as soon as the question is sent, before the host starts (DIRECTION 5.1).
  const shellAt = run?.running && last?.role === "user" && !last.steer ? latestPromptKey : "";
  const shell = useMemo<ChatMessage | null>(() => shellAt
    ? { id: `pending-reply:${shellAt}`, role: "assistant", status: "streaming", content: "", created_at: new Date().toISOString() }
    : null, [shellAt]);
  const turns = useMemo(() => turnGroups(messages), [messages]);
  // A link's latest research: a session still at work, else the last one Bot posted.
  const linkResearch = useMemo(() => new Map([...messages.flatMap(({ handoff }) =>
    handoff?.connectionId ? [[handoff.connectionId, handoff] as const] : []), ...(runningLinks ?? [])]), [messages, runningLinks]);
  // The latest turn keeps a viewport of room below its question while it is the one being read.
  const reserve = Boolean(shell) || last?.status === "streaming" || (lastUserIndex >= 0 && !openedWith.has(latestPromptKey));
  const lastAssistantIndex = messages.reduce((last, message, index) => (message.role === "assistant" ? index : last), -1);
  // Editing a request sets aside every reply after it; one still saving to Memory waits for its receipt.
  const lastSavingIndex = messages.reduce((last, message, index) => (message.memorySaving ? index : last), -1);
  // The user's question each answer replies to, offered again by a setup card once the connection is ready.
  const previousQuestions = useMemo(() => {
    let question: string | undefined;
    return messages.map((message) => {
      if (message.role === "user" && !message.steer) question = message.content;
      return question;
    });
  }, [messages]);
  // A scheduled run's request is run again by its task, not sent again as the user's turn.
  const lastTurnIndex = lastTurnMessageIndex(messages);
  const lastRequest = messages[lastTurnMessageIndex(messages, lastTurnIndex)];
  const lastRequestScheduled = lastRequest?.role === "user" ? lastRequest.scheduled : undefined;
  // A tool can replace a receipt without changing the revision, as when a read is followed by a delete.
  const receiptKey = home && active ? messages
    .flatMap(message => message.schedule_receipts ?? [])
    .filter(receipt => "revision" in receipt)
    .map(({ task_id, revision, action, status, next_run_at_ms }) => [task_id, revision, action, status, next_run_at_ms].join(":"))
    .join(",") : "";
  const botTasks = useBotScheduleTasks(bridge, loupistId, receiptKey);
  const dividerKey = JSON.stringify(messages.map(({ id, created_at }) => [id, created_at]));
  const dividerDay = new Date().toDateString();
  // Bot keeps the day between messages; "new since you left" belongs to research sessions read later.
  const dividers = useMemo(() => messages.map((_, index) => dividersBefore(messages, index, newSinceId).filter((divider) => !home || divider.kind !== "new")),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- dividers read only ids/timestamps; streaming text does not change a date boundary.
    [dividerKey, dividerDay, newSinceId, i18n.language, home]);
  return (
    <ChatSurfaceProvider value={surface}><BotScheduleTasks.Provider value={botTasks}>
    <Conversation
      className="message-scroll"
      data-surface={home ? "bot" : undefined}
      onFocus={revealFocused}
      aria-label={t("chatConversation.conversation")}
      aria-relevant="additions"
      scrollResetKey={latestPromptKey}
      running={Boolean(run?.running) || last?.status === "streaming"}
      autoScroll={messages.length > 0}
      footer={messages.length > 0 ? <ChatStatusPill message={last?.role === "assistant" ? last : undefined} run={run} /> : null}
    >
      {onQuote && active ? <QuoteAsk onQuote={onQuote} /> : null}
      <ConversationContent ref={contentRef} className="conversation-content">
        {!messages.length ? (
          // The empty Bot greets; an empty topic Chat only offers questions.
          home ? greeting : offerQuestions ? morning("questions") : null
        ) : (<>
          {turns.map((turn, turnIndex) => {
            const latestTurn = turnIndex === turns.length - 1;
            return <section key={turn.start === null ? "lead" : messageKeys[turn.start] ?? messages[turn.start]?.id} className="turn" data-turn=""
              data-turn-latest={latestTurn || undefined} data-turn-reserve={(latestTurn && reserve) || undefined}>
          {turn.indexes.map((index) => { const message = messages[index]!; return <Fragment key={messageKeys[index] ?? message.id}>
            {dividers[index]?.map((divider) => <ChatDivider key={divider.kind} divider={divider} onPassed={divider.kind === "new" ? clearNewSince : undefined} />)}
            {message.vocabulary ? (
            <ThreadVocabulary message={message} bridge={bridge} loupistId={loupistId} loupistName={loupistName} />
          ) : message.link ? (
            <ThreadLink message={message} bridge={bridge} loupistId={loupistId} loupistName={loupistName} composerChoices={composerChoices}
              research={linkResearch.get(message.link.connectionId)} />
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
              blockingChat={message.memorySaving?.blocking_chat_id ? threads?.find(thread => thread.id === message.memorySaving?.blocking_chat_id) : undefined}
              // A reply whose stream has ended carries its own Skip saving; a live one has the composer's.
              onSkipSave={message.memorySaving && message.status !== "streaming" ? onSkipSave : undefined}
              skippingSave={Boolean(message.memorySaving && skippingSaves?.has(message.memorySaving.run_id))}
              loupistAvatar={message.role === "bot_handoff" ? loupistAvatar : undefined}
              researchSession={home && message.role === "assistant" && message.research_session_id
                ? researchSessions?.find(thread => thread.id === message.research_session_id && thread.loupist_id === loupistId && !thread.home) : undefined}
              feedbackHeld={message.role === "assistant" && Boolean(run?.running)}
              onSaveFeedback={onSaveFeedback}
              onTeachFeedback={onTeachFeedback}
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
              // Run flags reach only the cards that use them, so a send or a commit redraws no other answer.
              askArtifactDisabled={hasCards(message) && run?.running && !last?.memorySaving}
              onReadAttachment={onReadAttachment}
              mentionSkills={mentionSkills}
              mentionPlugins={mentionPlugins}
              // A chosen action has no words of the user's to edit.
              onEditUser={message.role === "user" && !message.handoff_action && !message.scheduled && onEditUser ? editUser : undefined}
              editDisabled={message.role === "user" && (Boolean(run?.running) || index < lastSavingIndex)}
              onEarlierVersion={message.role === "user" && onSwitchVersion ? versionAt(index, "earlier", onSwitchVersion) : undefined}
              onLaterVersion={message.role === "user" && onSwitchVersion ? versionAt(index, "later", onSwitchVersion) : undefined}
              onRetry={message.role === "assistant" && index === lastTurnIndex && (message.status === "error" || message.status === "aborted") && lastRequest?.role === "user" && !lastRequestScheduled ? onRetry : undefined}
              scheduledRetry={message.role === "assistant" && index === lastTurnIndex && (message.status === "error" || message.status === "aborted") ? lastRequestScheduled : undefined}
              latest={message.role === "assistant" ? index === lastAssistantIndex : index === lastUserIndex}
              providerName={message.status === "error" ? providerLabel?.(message.execution_model?.provider) : undefined}
              retryAttempt={index === messages.length - 1 ? retryAttempt : undefined}
              onProviderAction={index === messages.length - 1 ? onProviderAction : undefined}
              note={message.role === "assistant" && message.replaced_model && message.execution_model?.name
                ? t("chatError.modelGone", { old: message.replaced_model, new: message.execution_model.name })
                : undefined}
              entering={message.role === "user" && !message.steer && !openedWith.has(messageKeys[index] ?? message.id)}
            />
          )}
          </Fragment>; })}
            {latestTurn && shell ? <ChatMessageCard key={shell.id} bridge={bridge} active={active} message={shell} loupistId={loupistId} chatId={chatId} loupistName={loupistName}
              workspacePath={workspacePath} guiding={guiding} stopping={Boolean(run?.stopping)} onReadAttachment={onReadAttachment} entering /> : null}
          </section>; })}
          {/* Bot greets until the user writes; setup steps live in the greeting, never under the conversation. */}
          {home && !hasUserMessage ? greeting : home && !repliedThisVisit ? morning("news") : null}
        </>)}
        {/* The first selectable point after the thread: a drag below the last message ends here (chat.css). */}
        <div className="thread-selection-end" aria-hidden="true" />
      </ConversationContent>
    </Conversation>
    </BotScheduleTasks.Provider></ChatSurfaceProvider>
  );
}

/**
 * The thread as turns: each question (not a steer) starts one and holds every
 * message up to the next. Messages before the first question form a lead group.
 */
function turnGroups(messages: readonly ChatMessage[]): Array<{ start: number | null; indexes: number[] }> {
  const turns: Array<{ start: number | null; indexes: number[] }> = [];
  messages.forEach((message, index) => {
    if (message.role === "user" && !message.steer) turns.push({ start: index, indexes: [index] });
    else if (turns.length) turns.at(-1)!.indexes.push(index);
    else turns.push({ start: null, indexes: [index] });
  });
  return turns;
}

/** An answer that draws a chart or table card, whose Ask button waits while a reply runs. */
const hasCards = (message: ChatMessage) => message.role === "assistant" && (Boolean(message.artifact_refs?.length)
  || (message.progress?.items ?? []).some((item) => item.kind === "tool" && (item.toolName === "chart_publish" || item.toolName === "grid_publish")));
