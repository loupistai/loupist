import { Disclosure } from "../ui/disclosure";
import { ApprovalCards } from "./ApprovalCard";
import type { LoupistBridge } from "../../types";
import { formatNumber, formatTime } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { memo, useEffect, useLayoutEffect, useMemo, useState, type ReactNode } from "react";
import { useConversationScroll } from "../ai-elements/conversation";
import { AnswerFeedback, type SaveAnswerFeedback } from "./AnswerFeedback";
import {
  CheckIcon,
  CopyIcon,
  CornerDownRightIcon,
  DownloadIcon,
  FileIcon,
  FileTextIcon,
  SlidersHorizontalIcon,
  InfoIcon,
} from "lucide-react";
import { Message, MessageContent } from "@/components/ai-elements/message";
import { useNow } from "../../chat/runStatus";
import { deriveTurnView, useLastActivity } from "../../chat/turnView";
import { deliveredFiles } from "../../chat/deliveredFiles";
import { DeliveredFiles } from "./DeliveredFiles";
import { thinkingLabel } from "../../lib/modelSelection";
import { knownComposerMentions, splitComposerMentions } from "../../chat/composerMentions";
import { mentionLabel } from "../../chat/composerChips";
import { fileSize } from "../../chat/fileSize";
import type {
  ChatArtifactRef,
  ChatAttachment,
  ChatMessage,
  ChatPerformance,
  GraphJobView,
} from "../../types";
import { ActivityRow } from "./ActivityRow";
import { ProseSegment } from "./ProseSegment";
import { TurnFooter } from "./TurnFooter";
import { TurnProvider, useTurn } from "./TurnProvider";
import { SourceRow } from "./SourceRow";
import { useGo } from "../navigation/Open";
import { MessageSchedules } from "./MessageSchedules";
import { EarningsChatCard } from "./EarningsChatCard";
import { earningsCards } from "../../chat/earningsLinks";
import { sensitivityParagraphs, type SensitivitySegment } from "../../chat/sensitivity";
import { SensitivityCard } from "./SensitivityCard";
import { ChatArtifactBlock } from "./ChatArtifactBlock";
import { chartBlocks, gridBlocks } from "../../chat/chartBlocks";
import { connectionSetupBlocks } from "../../chat/connectionSetupBlocks";
import { ConnectionSetupCard } from "./ConnectionSetupCard";
import { ChatGridBlock } from "./GridCard";
import { MessageMemoryRow } from "./MessageMemoryRow";
import { NextStepChips, nextStepQuestions } from "./NextStepChips";
import { useChatMap } from "../workspace/ChatMapContext";
import { AnswerOutline } from "./AnswerOutline";
import { SentImages } from "./SentImages";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { ChatErrorState, type ProviderErrorAction } from "./ChatErrorState";
import { AsOfNote } from "./AsOfNote";
import { AsOfTag } from "./ComposerAsOf";

const t = i18n.getFixedT(null, "chat");

const failureMessages = (): Record<NonNullable<ChatMessage["failure_kind"]>, string> => ({
  empty_reply: t("chatMessageCard.empty_reply"),
  provider_error: t("chatMessageCard.provider_error"),
  rate_limited: t("chatMessageCard.rate_limited"),
  needs_reconnect: t("chatMessageCard.needs_reconnect"),
  plan_limit: t("chatMessageCard.plan_limit"),
  plan_not_eligible: t("chatMessageCard.plan_not_eligible"),
  out_of_credit: t("chatMessageCard.out_of_credit"),
  model_gone: t("chatMessageCard.model_gone"),
  offline: t("chatMessageCard.offline"),
  provider_unavailable: t("chatMessageCard.provider_unavailable"),
  context_too_long: t("chatMessageCard.context_too_long"),
  provider_timeout: t("chatMessageCard.provider_timeout"),
  pi_exited: t("chatMessageCard.pi_exited"),
  protocol_error: t("chatMessageCard.protocol_error"),
  output_limit: t("chatMessageCard.output_limit"),
  memory_commit_failed: t("chatMessageCard.memory_commit_failed"),
  store_failed: t("chatMessageCard.store_failed"),
  cancelled_by_conflict: t("chatMessageCard.cancelled_by_conflict"),
  other: t("chatMessageCard.other"),
});

const attachmentSize = fileSize;

const responseDuration = (milliseconds: number) =>
  milliseconds < 1_000
    ? t("chatMessageCard.milliseconds", { duration: formatNumber(milliseconds) })
    : t("chatMessageCard.responseSeconds", { value1: formatNumber(milliseconds / 1_000, { minimumFractionDigits: milliseconds < 10_000 ? 1 : 0, maximumFractionDigits: milliseconds < 10_000 ? 1 : 0 }) });

const knownTokenCount = (value: unknown): number | undefined =>
  typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : undefined;
const tokenCountLabel = (value: number | undefined) =>
  value === undefined ? t("chatMessageCard.unavailable") : t("chatMessageCard.tokenCount", { count: value, formattedCount: formatNumber(value) });

function responseTokenLines(performance: ChatPerformance | undefined): string[] {
  if (!performance) return [t("chatMessageCard.tokenUsageUnavailable")];
  const input = knownTokenCount(performance.inputTokens);
  const cacheRead = knownTokenCount(performance.cacheReadTokens);
  const cacheWrite = knownTokenCount(performance.cacheWriteTokens);
  const totalInput = input !== undefined && cacheRead !== undefined && cacheWrite !== undefined
    ? knownTokenCount(input + cacheRead + cacheWrite) : undefined;
  return [
    t("chatMessageCard.inputTokens", { value1: tokenCountLabel(totalInput) }),
    t("chatMessageCard.outputTokens", { value1: tokenCountLabel(knownTokenCount(performance.outputTokens)) }),
  ];
}

function MessageAttachment({
  attachment,
  messageId,
  onRead,
}: {
  attachment: ChatAttachment;
  messageId: string;
  onRead: (messageId: string, attachmentId: string) => Promise<string>;
}) {
  useTranslation("chat");
  const isImage = attachment.media_type.startsWith("image/");
  const [url, setUrl] = useState(attachment.url);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!isImage || url || failed) return;
    let active = true;
    void onRead(messageId, attachment.id)
      .then((next) => {
        if (active) setUrl(next);
      })
      .catch(() => {
        if (active) setFailed(true);
      });
    return () => {
      active = false;
    };
  }, [attachment.id, failed, isImage, messageId, onRead, url]);

  const download = async () => {
    try {
      const source = url ?? (await onRead(messageId, attachment.id));
      const link = document.createElement("a");
      link.href = source;
      link.download = attachment.filename;
      link.click();
    } catch {
      setFailed(true);
    }
  };

  return (
    <div className="message-attachment">
      {isImage && url ? (
        <img src={url} alt={attachment.filename} />
      ) : (
        <FileIcon aria-hidden="true" />
      )}
      <span>
        <strong>{attachment.filename}</strong>
        <small>
          {attachmentSize(attachment.size_bytes)}
        </small>
        {failed && <small>{t("chatMessageCard.previewUnavailable")}</small>}
      </span>
      <button
        type="button"
        aria-label={t("chatMessageCard.downloadAttachment", { filename: attachment.filename })}
        title={t("chatMessageCard.downloadAttachment", { filename: attachment.filename })}
        onClick={() => void download()}
      >
        <DownloadIcon />
      </button>
    </div>
  );
}



type Props = {
  bridge?: LoupistBridge;
  onSaveFeedback?: SaveAnswerFeedback;
  onTeachFeedback?: (text: string) => void;
  feedbackBusy?: boolean;
  active?: boolean;
  message: ChatMessage;
  loupistId: string;
  chatId?: string;
  loupistName: string;
  workspacePath?: string;
  progressFocus?: string | null;
  guiding?: readonly string[];
  /** Set on the latest finished answer: follow-up questions go into the composer. */
  onNextStep?: (question: string) => void;
  /** Puts text into this Chat's composer; never sends ("Change the rates"). */
  onFillComposer?: (text: string) => void;
  /** The user's message this answer replies to, which a setup card offers to ask again. */
  previousQuestion?: string;
  onReadWorldJob?: (jobId: string) => Promise<GraphJobView>;
  onWorldChanged?: () => void;
  /** Adds a chart to the next question (the composer's selected-artifact chip). */
  onAskArtifact?: (artifact: ChatArtifactRef) => void;
  askArtifactDisabled?: boolean;
  onReadAttachment: (
    messageId: string,
    attachmentId: string,
  ) => Promise<string>;
  mentionSkills?: ReadonlyArray<{ id: string; name: string }>;
  mentionPlugins?: ReadonlyArray<{ id: string; name: string }>;
  onEditUser?: (text: string) => void;
  /** Editing waits for the running reply; the control stays in place so the thread does not jump when it ends. */
  editDisabled?: boolean;
  /** Edit forks (X18): show the version of the chat before or after this message's version. */
  onEarlierVersion?: () => void;
  onLaterVersion?: () => void;
  /** Briefings open the outline of a long answer (X16). */
  outlineOpen?: boolean;
  onRetry?: () => void;
  /** The newest answer keeps its action row visible; earlier ones show it on hover or focus (X1). */
  latest?: boolean;
  /** A stop was requested for this reply and the host has not confirmed it yet. */
  stopping?: boolean;
  /** The failed answer's provider by display name (X11 copy). */
  providerName?: string;
  /** Busy answers in a row in this Chat, counting this one (X11 countdown). */
  retryAttempt?: number;
  /** An account action from the error (Reconnect, Use an API key, Manage usage…), with the answer's provider. */
  onProviderAction?: (action: ProviderErrorAction, provider: string | undefined) => void;
  /** A meta note under the answer, such as "… isn't offered anymore, so this answer used …". */
  note?: string;
};

function UserMessageText({
  text,
  asOf,
  skills,
  plugins,
}: {
  text: string;
  asOf?: string;
  skills: ReadonlyArray<{ id: string; name: string }>;
  plugins: ReadonlyArray<{ id: string; name: string }>;
}) {
  useTranslation("chat");
  const known = knownComposerMentions(skills, plugins);
  // Saved turns encode tool selections as a separate first paragraph.
  // Only move a whole paragraph of known tokens; keep mentions in prose where they were typed.
  const split = text.indexOf("\n\n");
  const prefix = split < 0 ? text : text.slice(0, split);
  const tokens = prefix.trim().split(/\s+/u);
  const tools = !prefix.includes("\n") && tokens.every(token => known.has(token)) ? tokens : [];
  const body = tools.length ? (split < 0 ? "" : text.slice(split + 2)) : text;
  return <>
    {body && <p className="message-user-text">
      {splitComposerMentions(body, known).map((part, index) => part.type === "mention" ? (
        <span key={index} className="message-mention" data-kind={part.kind}>{mentionLabel(part.value, skills, plugins) ?? part.value}</span>
      ) : (
        <span key={index}>{part.value}</span>
      ))}
    </p>}
    {(tools.length > 0 || asOf) && <div className="message-user-context">
      {tools.map((token, index) => <span className="message-tool-label" key={index} title={token}>
        <SlidersHorizontalIcon aria-hidden="true" />{mentionLabel(token, skills, plugins)}
      </span>)}
      {asOf && <AsOfTag asOf={asOf} kind="request" />}
    </div>}
  </>;
}

function ChatMessageCardComponent({
  bridge,
  active = true,
  message,
  loupistId,
  chatId,
  loupistName,
  workspacePath,
  progressFocus,
  guiding,
  onNextStep,
  onFillComposer,
  previousQuestion,
  onReadWorldJob,
  onWorldChanged,
  onAskArtifact,
  askArtifactDisabled,
  onReadAttachment,
  mentionSkills = [],
  mentionPlugins = [],
  onEditUser,
  editDisabled = false,
  onEarlierVersion,
  onLaterVersion,
  outlineOpen = false,
  onRetry,
  latest = false,
  stopping = false,
  providerName,
  retryAttempt = 1,
  onProviderAction,
  note,
  onSaveFeedback,
  onTeachFeedback,
  feedbackBusy = false,
}: Props) {
  useTranslation("chat");
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");
  const revealedResult = useTurn()?.revealedResult ?? null;
  const go = useGo();
  const evidenceRefs = message.evidence_refs ?? [];
  // Memory declines are told once per chat by the memory row, not as connection activity.
  const connectionRefusals = (message.tool_refusals ?? []).filter(refusal => refusal.reason !== "sensitive_memory");
  const now = useNow(message.status === "streaming");
  const lastActivityAt = useLastActivity(message);
  const retryOffered = Boolean(onRetry);
  const view = useMemo(() => deriveTurnView({ message, now, lastActivityAt, stopping, retry: { attempt: retryAttempt, offered: retryOffered } }), [message, now, lastActivityAt, stopping, retryAttempt, retryOffered]);
  // A new row or a settled footer grows the turn in this commit; follow it now, a frame before the ResizeObserver would.
  const conversation = useConversationScroll();
  const segmentCount = view.segments.length;
  useLayoutEffect(() => { conversation?.followGrowth(); }, [conversation, segmentCount, view.state.kind]);
  const complete = view.state.kind === "complete";
  const settled = view.footer.kind === "settled" ? view.footer : null;
  const copyText = settled?.copyText ?? null;
  // Delivered files (X13): written by this answer and linked from it; a write the answer never links stays a step.
  const deliveredPaths = useMemo(() => {
    if (message.role !== "assistant") return [];
    const written = new Set((message.progress?.items ?? []).flatMap((item) =>
      item.kind === "tool" && item.status === "succeeded" && (item.operation === "write" || item.operation === "edit") && item.filePath
        ? [item.filePath] : [],
    ));
    if (!written.size) return [];
    const delivered = deliveredFiles(message.content, workspacePath);
    return [...written].filter((path) => delivered.has(path));
  }, [message.role, message.progress?.items, message.content, workspacePath]);
  // Charts render as cards in the answer; documents stay in the outputs row and open the panel.
  const charts = useMemo(() => chartBlocks(message), [message]);
  // Grid answers and conflict cards render in the answer too, after its charts.
  const grids = useMemo(() => gridBlocks(message), [message]);
  const connectionSetups = useMemo(() => connectionSetupBlocks(message), [message]);
  // A paragraph that only links to a grid result renders as the sensitivity card where the
  // paragraph stood, and one that only links to a results snapshot as the compact results card.
  // Rust counted the links of the answer as written, so the grid paragraphs come out first.
  const answerText = complete ? copyText : null;
  const settledAnswer = useMemo(() => {
    if (message.role !== "assistant" || answerText === null) return null;
    const grids = sensitivityParagraphs(answerText, message.sensitivity_cards, message.number_checks);
    const segments = grids.segments.map((segment) => ({ ...earningsCards(segment.text, message.evidence_refs, segment.numberChecks), card: segment.card }));
    return { segments: segments as SensitivitySegment[], cards: segments.flatMap((segment) => segment.cards) };
  }, [message.role, answerText, message.evidence_refs, message.number_checks, message.sensitivity_cards]);
  const documents = (message.artifact_refs ?? []).filter((artifact) => artifact.kind === "markdown");
  const executionModelTitle = message.execution_model
    ? [
        message.execution_model.name,
        t("chatMessageCard.reasoningLevel", { value1: thinkingLabel(message.execution_model.thinking_level) }),
      ].join(" · ")
    : undefined;
  const performanceTitle = message.performance
    ? t("chatMessageCard.responseTime", { value1: responseDuration(message.performance.totalMs) })
    : undefined;
  const tokenLines = responseTokenLines(message.performance);
  const prose =(key: string, text: string, streaming: boolean, tone: "answer" | "narration", checks?: ChatMessage["number_checks"]) =>
    <ProseSegment key={key} text={text} streaming={streaming} tone={tone} checks={checks} workspacePath={workspacePath} />;

  const transcript: ReactNode[] = [];
  if (message.role === "assistant") {
    for (const segment of view.segments) {
      if (segment.kind === "activity") {
        transcript.push(<ActivityRow key={segment.key} segment={segment} focusItemId={progressFocus} asOf={message.as_of}
          revealedResult={segment.evidence.some(e => e.id === revealedResult?.id) ? revealedResult : undefined}
          renderApprovals={bridge ? approvals => <ApprovalCards approvals={approvals} refusals={[]} bridge={bridge} loupistId={loupistId} /> : undefined} />);
        continue;
      }
      // The settled answer alone splits around its cards; the first piece keeps the answer's key.
      if (settledAnswer && segment.tone === "answer" && !segment.streaming) {
        settledAnswer.segments.forEach((piece, index) => {
          if (piece.text) transcript.push(prose(index ? `${segment.key}:${index}` : segment.key, piece.text, false, "answer", piece.numberChecks));
          if (piece.card) {
            const card = piece.card;
            transcript.push(<SensitivityCard key={`grid-${card.evidence_id}-${index}`} grid={card} subject={card.subject}
              evidence={evidenceRefs.find(item => item.id === card.evidence_id)} evidenceRefs={evidenceRefs} asOf={message.as_of}
              onChangeRates={onFillComposer} />);
          }
        });
        continue;
      }
      transcript.push(prose(segment.key, segment.text, segment.streaming, segment.tone, segment.checks));
    }
    if (bridge && connectionRefusals.length) transcript.push(<ApprovalCards key="refusals" approvals={[]} refusals={connectionRefusals} bridge={bridge} loupistId={loupistId} />);
  }

  // Guidance the reply accepted reads as the row it was while the reply ran, not a new question.
  if (message.role === "user" && message.steer) return (
    <p data-message-id={message.id} className="chat-guiding-row chat-guided">
      <CornerDownRightIcon aria-hidden="true" />{t("chatProgressTimeline.guided", { text: message.content })}
    </p>
  );

  return (
    <article data-message-id={message.id} data-latest={latest || undefined} className={`message ${message.role} ${message.status ?? ""}`}>
      <div className="message-meta">
        <span>{message.role === "user" ? t("chatMessageCard.you") : loupistName}</span>
        <time>{formatTime(new Date(message.created_at))}</time>
        {message.role === "assistant" && message.as_of ? <AsOfTag asOf={message.as_of} kind="answer" /> : null}
        {message.execution_model || message.performance || message.host_composed ? <Popover>
          <PopoverTrigger asChild><button type="button" className="message-details-trigger" aria-label={t("chatMessageCard.responseDetails")} aria-description={[executionModelTitle, performanceTitle, ...tokenLines].filter(Boolean).join(" · ")}><InfoIcon /></button></PopoverTrigger>
          <PopoverContent className="message-details-panel" align="start" aria-label={t("chatMessageCard.responseDetails")}>
            <strong>{t("chatMessageCard.responseDetails")}</strong>
            {executionModelTitle ? <p>{executionModelTitle}</p> : null}
            {performanceTitle ? <p>{performanceTitle}</p> : null}
            {message.host_composed ? <p>{t("chatMessageCard.thisAnswerListsWhatWasSavedBecauseTheModel")}</p> : null}
            <div className="message-token-usage">{tokenLines.map((line) => <p key={line}>{line}</p>)}</div>
          </PopoverContent>
        </Popover> : null}
      </div>
      {message.role === "user" ? <SentImages message={message} /> : null}
      {message.role === "user" ? (
        <Message from={message.role} className="max-w-full">
          <MessageContent className="message-content overflow-visible">
            {(message.content || message.as_of) && <UserMessageText text={message.content} asOf={message.as_of} skills={mentionSkills} plugins={mentionPlugins} />}
          </MessageContent>
        </Message>
      ) : null}
      {transcript}
      {bridge && settledAnswer ? settledAnswer.cards.map((card) => <EarningsChatCard key={card.evidenceId} bridge={bridge} loupistId={loupistId} loupistName={loupistName} assetId={card.assetId} eventId={card.eventId} />) : null}

      {charts.map((block) => <ChatArtifactBlock key={block.key} block={block} onAsk={onAskArtifact} askDisabled={askArtifactDisabled} asOf={message.as_of} />)}
      {bridge ? connectionSetups.map((block) => <ConnectionSetupCard key={block.key} bridge={bridge} loupistId={loupistId} entryId={block.entryId}
        onAskAgain={onFillComposer && previousQuestion ? () => onFillComposer(previousQuestion) : undefined} />) : null}
      {grids.map((block) => <ChatGridBlock key={block.key} block={block} onAsk={onAskArtifact} askDisabled={askArtifactDisabled} />)}

      {message.role === "assistant" && <MessageSchedules receipts={message.schedule_receipts ?? []} />}

      <DeliveredFiles paths={deliveredPaths}
        onReveal={bridge ? (path) => bridge.workspaceReveal({ loupist_id: loupistId, path }) : undefined} />
      {documents.length > 0 ? (
        <div className="message-outputs" aria-label={t("chatMessageCard.outputs")}>
          {documents.map((artifact) => (
            <button className="message-output-card" type="button" key={`${artifact.artifact_id}:${artifact.revision}`} aria-label={t("chatMessageCard.openDocument", { title: artifact.title })} disabled={!go} onClick={() => go?.({ kind: "artifact", artifact })}>
              <FileTextIcon aria-hidden="true" />
              <span><strong>{artifact.title}</strong><small>{t("chatMessageCard.document")}</small></span>
            </button>
          ))}
        </div>
      ) : null}

      {/* Sent images are thumbnails above the text (SentImages); other files stay as rows. */}
      {!!message.attachments?.some((attachment) => !attachment.media_type.startsWith("image/")) && (
        <div className="message-attachments" aria-label={t("chatMessageCard.messageAttachments")}>
          {message.attachments.filter((attachment) => !attachment.media_type.startsWith("image/")).map((attachment) => (
            <MessageAttachment
              key={attachment.id}
              attachment={attachment}
              messageId={message.id}
              onRead={onReadAttachment}
            />
          ))}
        </div>
      )}

      {message.role === "assistant" ? <TurnFooter footer={view.footer} now={now} announce={view.announce} guiding={guiding}>
        {settled ? <>
          {settled.sources.length > 0 ? (
            <Disclosure className="message-sources" aria-label={t("chatMessageCard.researchSources")} title={<>{t("chatMessageCard.sourceCount", { formattedCount: formatNumber(settled.sources.length) })}</>}>
              <ol>{settled.sources.map((evidence, index) => <SourceRow key={evidence.id} evidence={evidence} number={index + 1} />)}</ol>
            </Disclosure>
          ) : null}
          <MessageMemoryRow
            bridge={bridge}
            active={active}
            loupistId={loupistId}
            chatId={chatId}
            message={message}
            onReadWorldJob={onReadWorldJob}
            onWorldChanged={onWorldChanged}
          />
          {copyText !== null ? <div className="message-actions">
            <button type="button" aria-label={copyState === "copied" ? t("chatMessageCard.answerCopied") : t("chatMessageCard.copyAnswer")} title={copyState === "copied" ? t("chatMessageCard.copied") : t("chatMessageCard.copyAnswer")} onClick={() => {
              void navigator.clipboard.writeText(copyText).then(() => setCopyState("copied"), () => setCopyState("failed"));
            }}>{copyState === "copied" ? <CheckIcon /> : <CopyIcon />}</button>
            {copyState === "failed" ? <span role="alert">{t("chatMessageCard.couldNotCopySelectTheAnswerToCopyIt")}</span> : null}
          </div> : null}
          {settled.feedback && onSaveFeedback && onTeachFeedback ? <AnswerFeedback bridge={bridge} loupistId={loupistId} chatId={chatId} message={message} onSave={onSaveFeedback} onTeach={onTeachFeedback} feedbackBusy={feedbackBusy} /> : null}
          {note ? <p className="message-model-note">{note}</p> : null}
          {settled.runState ? (
            <div className="message-run-state" role={settled.runState.kind === "failed" ? "alert" : "status"}>
              {settled.runState.kind === "failed" ? <ChatErrorState kind={settled.runState.failure} retryInSeconds={settled.runState.retryInSeconds} provider={providerName ?? t("chatError.yourProvider")}
                fallback={failureMessages()[settled.runState.failure]} onRetry={onRetry}
                onProviderAction={onProviderAction ? (action) => onProviderAction(action, message.execution_model?.provider) : undefined} /> : <>
                <p>{settled.runState.label}</p>
                {onRetry ? <button type="button" onClick={onRetry}>{t("chatMessageCard.tryAgain")}</button> : null}
              </>}
            </div>
          ) : null}
        </> : null}
      </TurnFooter> : null}
      {message.role === "assistant" && complete && message.as_of_report ? <AsOfNote report={message.as_of_report} loupistName={loupistName} /> : null}
      {message.role === "assistant" && answerText ? <AnswerOutline messageId={message.id} markdown={answerText} defaultOpen={outlineOpen} /> : null}
      {message.role === "assistant" && complete && onNextStep ? <NextSteps loupistId={loupistId} chatId={chatId} messageId={message.id} onAsk={onNextStep} /> : null}
      {message.role === "user" && (onEditUser || onEarlierVersion || onLaterVersion) ? <div className="message-actions message-action-bar">
        {onEditUser ? <button type="button" aria-label={t("chatMessageCard.editMessage")} title={t("chatMessageCard.edit")} disabled={editDisabled} onClick={() => onEditUser(message.content)}>{t("chatMessageCard.edit")}</button> : null}
        {onEarlierVersion ? <button type="button" className="message-version-link" onClick={onEarlierVersion}>{t("chatBranch.earlierVersion")}</button> : null}
        {onLaterVersion ? <button type="button" className="message-version-link" onClick={onLaterVersion}>{t("chatBranch.laterVersion")}</button> : null}
      </div> : null}
    </article>
  );
}

/** Follow-up chips from what this answer changed on the Map (S7). */
function NextSteps({ loupistId, chatId, messageId, onAsk }: { loupistId: string; chatId?: string; messageId: string; onAsk: (question: string) => void }) {
  const { i18n: instance } = useTranslation("chat");
  const chatMap = useChatMap();
  if (!chatMap?.map || chatMap.loupistId !== loupistId || !chatId || chatMap.chatId !== chatId) return null;
  const events = chatMap.events.filter((event) => event.originChatId === chatId && event.originMessageId === messageId);
  return <NextStepChips questions={nextStepQuestions(chatMap.map, events, instance.language)} onAsk={onAsk} />;
}

function ChatMessageCardInTurn(props: Props) {
  return <TurnProvider message={props.message} loupistId={props.loupistId}><ChatMessageCardComponent {...props} /></TurnProvider>;
}

export const ChatMessageCard = memo(ChatMessageCardInTurn);
