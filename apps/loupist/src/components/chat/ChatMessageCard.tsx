import type { OpenLinkOptions } from "../../chat/links";
import { Disclosure } from "../ui/disclosure";
import { ApprovalCards } from "./ApprovalCard";
import { AssetCard, type AssetRef } from "./AssetCard";
import type { LoupistBridge } from "../../types";
import { formatDate, formatNumber, formatTime } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { memo, useEffect, useMemo, useState, type ReactNode } from "react";
import { AnswerFeedback, type SaveAnswerFeedback } from "./AnswerFeedback";
import {
  ArrowUpRightIcon,
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
import { useConversationScroll } from "../ai-elements/conversation";
import { turnTranscript } from "../../chat/turnTranscript";
import { withoutIdentifiers } from "../../chat/runStatus";
import { deliveredFiles } from "../../chat/deliveredFiles";
import { DeliveredFiles } from "./DeliveredFiles";
import { thinkingLabel } from "../../lib/modelSelection";
import { knownComposerMentions, splitComposerMentions } from "../../chat/composerMentions";
import { mentionLabel, splitAssetTokens } from "../../chat/composerChips";
import { tickerMark } from "../../chat/assetTickers";
import { fileSize } from "../../chat/fileSize";
import { citationSources, evidencePresentation, evidenceTitle, sourceIdentityLabel } from "../../chat/evidence";
import { quoteEvidence } from "../../chat/quoteEvidence";
import { QuoteEvidencePopover } from "./QuoteEvidence";
import type {
  ChatArtifactRef,
  ChatEvidenceRef,
  ChatAttachment,
  ChatMessage,
  ChatPerformance,
  KnowledgeLocation,
  GraphJobView,
} from "../../types";
import { exactMemoryReads, isInlineWorkEvidence, memoryReadForEvidence } from "../../chat/memoryActivity";
import { knowledgeLocationFromMemoryRef } from "../../knowledge/location";
import { ChatProgressTimeline } from "./ChatProgressTimeline";
import { MessageSchedules } from "./MessageSchedules";
import { EarningsChatCard } from "./EarningsChatCard";
import { earningsCards } from "../../chat/earningsLinks";
import { sensitivityParagraphs, type SensitivitySegment } from "../../chat/sensitivity";
import { SensitivityCard } from "./SensitivityCard";
import { ChatArtifactBlock } from "./ChatArtifactBlock";
import { chartBlocks, gridBlocks } from "../../chat/chartBlocks";
import { ChatGridBlock } from "./GridCard";
import { MessageMemoryRow } from "./MessageMemoryRow";
import { NextStepChips, nextStepQuestions } from "./NextStepChips";
import { useChatMap } from "../workspace/ChatMapContext";
import { SourceFavicon } from "./SourceFavicon";
import { SafeMessageResponse } from "./SafeMessageResponse";
import { AnswerOutline } from "./AnswerOutline";
import { SentImages } from "./SentImages";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { BUSY_RETRY_SECONDS, ChatErrorState, type ProviderErrorAction } from "./ChatErrorState";
import { AsOfNote } from "./AsOfNote";
import { AsOfTag } from "./ComposerAsOf";
import type { EvidenceCell } from "./NumberChip";

const t = i18n.getFixedT(null, "chat");

const failureMessages = (): Record<NonNullable<ChatMessage["failure_kind"]>, string> => ({
  empty_reply: t("chatMessageCard.empty_reply"),
  provider_error: t("chatMessageCard.provider_error"),
  rate_limited: t("chatMessageCard.rate_limited"),
  needs_reconnect: t("chatMessageCard.needs_reconnect"),
  plan_limit: t("chatMessageCard.plan_limit"),
  plan_not_eligible: t("chatMessageCard.plan_not_eligible"),
  extra_usage_off: t("chatMessageCard.extra_usage_off"),
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
  /** The asset the question named with `$`; the answer opens with its card (composer D2). */
  asset?: AssetRef;
  onSaveFeedback?: SaveAnswerFeedback;
  onTeachFeedback?: (text: string) => void;
  feedbackBusy?: boolean;
  active?: boolean;
  message: ChatMessage;
  loupistId: string;
  chatId?: string;
  loupistName: string;
  workspacePath?: string;
  onOpenFile?: (path: string) => void;
  onOpenEvidence?: (evidence: ChatEvidenceRef, step?: { label: string; itemId?: string; cell?: EvidenceCell }) => void;
  progressFocus?: string | null;
  guiding?: readonly string[];
  /** Set on the latest finished answer: follow-up questions go into the composer. */
  onNextStep?: (question: string) => void;
  /** Puts text into this Chat's composer; never sends ("Change the rates"). */
  onFillComposer?: (text: string) => void;
  onReadWorldJob?: (jobId: string) => Promise<GraphJobView>;
  onWorldChanged?: () => void;
  onOpenMemory: (location: KnowledgeLocation, title: string) => void;
  onOpenMemoryPage: (location: KnowledgeLocation) => void;
  onOpenArtifact: (artifact: ChatArtifactRef) => void;
  /** Adds a chart to the next question (the composer's selected-artifact chip). */
  onAskArtifact?: (artifact: ChatArtifactRef) => void;
  askArtifactDisabled?: boolean;
  onOpenSchedule?: (taskId: string) => void;
  onOpenLink: (url: string, options?: OpenLinkOptions) => Promise<void>;
  onReadAttachment: (
    messageId: string,
    attachmentId: string,
  ) => Promise<string>;
  mentionSkills?: ReadonlyArray<{ id: string; name: string }>;
  mentionPlugins?: ReadonlyArray<{ id: string; name: string }>;
  onEditUser?: (text: string) => void;
  /** Edit forks (X18): show the version of the chat before or after this message's version. */
  onEarlierVersion?: () => void;
  onLaterVersion?: () => void;
  /** Briefings open the outline of a long answer (X16). */
  outlineOpen?: boolean;
  onRetry?: () => void;
  /** The newest answer keeps its action row visible; earlier ones show it on hover or focus (X1). */
  latest?: boolean;
  /** The failed answer's provider by display name (X11 copy). */
  providerName?: string;
  /** Busy answers in a row in this Chat, counting this one (X11 countdown). */
  retryAttempt?: number;
  /** An account action from the error (Reconnect, Use an API key, Manage usage…), with the answer's provider. */
  onProviderAction?: (action: ProviderErrorAction, provider: string | undefined) => void;
  /** A meta note under the answer, such as "… isn't offered anymore, so this answer used …". */
  note?: string;
  onDismissLensProposal?: (target: { recordId: string; messageId: string }) => Promise<void>;
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
      {splitAssetTokens(body).map((sent, outer) => sent.type === "asset" ? (
        <span key={outer} className="message-mention message-asset" title={sent.market ? `${sent.symbol}:${sent.market}` : sent.symbol}>
          <i className="composer-chip-marker" aria-hidden="true" data-kind={tickerMark(sent.market)} />
          {sent.name && sent.name !== sent.symbol ? `${sent.name} · ${sent.symbol}` : sent.symbol}
        </span>
      ) : splitComposerMentions(sent.value, known).map((part, index) => part.type === "mention" ? (
        <span key={`${outer}:${index}`} className="message-mention" data-kind={part.kind}>{mentionLabel(part.value, skills, plugins) ?? part.value}</span>
      ) : (
        <span key={`${outer}:${index}`}>{part.value}</span>
      )))}
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
  asset,
  active = true,
  message,
  loupistId,
  chatId,
  loupistName,
  workspacePath,
  onOpenFile,
  onOpenEvidence,
  progressFocus,
  guiding,
  onNextStep,
  onFillComposer,
  onReadWorldJob,
  onWorldChanged,
  onOpenMemory,
  onOpenMemoryPage,
  onOpenArtifact,
  onAskArtifact,
  askArtifactDisabled,
  onOpenSchedule,
  onOpenLink,
  onReadAttachment,
  mentionSkills = [],
  mentionPlugins = [],
  onEditUser,
  onEarlierVersion,
  onLaterVersion,
  outlineOpen = false,
  onRetry,
  latest = false,
  providerName,
  retryAttempt = 1,
  onProviderAction,
  note,
  onDismissLensProposal,
  onSaveFeedback,
  onTeachFeedback,
  feedbackBusy = false,
}: Props) {
  useTranslation("chat");
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");
  // While a busy answer counts down to its own retry, it has not failed yet.
  const retryPending = message.status === "error" && message.failure_kind === "rate_limited" && Boolean(onRetry) && retryAttempt <= BUSY_RETRY_SECONDS.length;
  const [revealedResult, setRevealedResult] = useState<{ id: string; request: number } | null>(null);
  const [quoteOpen, setQuoteOpen] = useState<{ evidence: ChatEvidenceRef; anchor: HTMLElement; pointer?: string } | null>(null);
  const conversation = useConversationScroll();
  const evidenceRefs = message.evidence_refs ?? [];
  const citationList = citationSources(evidenceRefs).map((evidence, index) => ({ evidence, number: index + 1 }));
  const workResults = evidenceRefs.filter((evidence) => isInlineWorkEvidence(evidence, message.memory_refs));
  // Memory declines are told once per chat by the memory row, not as connection activity.
  const connectionRefusals = (message.tool_refusals ?? []).filter(refusal => refusal.reason !== "sensitive_memory");
  const openLens = (recordId: string) => onOpenMemory({
    loupistId,
    surface: "lens",
    recordId,
    objectId: recordId,
    selection: { kind: "lens", recordId },
  }, "Lens");
  /** A number's "Show step": the producing step opens inline in the thread; the panel never opens. */
  const showStep = (evidence: ChatEvidenceRef) => {
    conversation?.holdScrollPosition();
    setRevealedResult((previous) => ({ id: evidence.id, request: (previous?.request ?? 0) + 1 }));
  };
  const openEvidence = (evidence: ChatEvidenceRef, cell?: EvidenceCell, anchor?: HTMLElement | null) => {
    const read = memoryReadForEvidence(evidence, message.memory_refs);
    const location = read ? knowledgeLocationFromMemoryRef(loupistId, read) : null;
    if (read && location) {
      onOpenMemory(location, read.title);
      return;
    }
    if (isInlineWorkEvidence(evidence, message.memory_refs)) {
      conversation?.holdScrollPosition();
      setRevealedResult((previous) => ({ id: evidence.id, request: (previous?.request ?? 0) + 1 }));
      return;
    }
    if (quoteEvidence(evidence)) {
      // The clicked control, not document.activeElement: a mouse click in WKWebView
      // can leave focus on the page, and a snapshot of that rectangle opens off screen.
      if (anchor) setQuoteOpen({ evidence, anchor, pointer: cell?.pointer });
      return;
    }
    const step = message.progress?.items.find((entry) => entry.kind === "tool" && entry.resultIds?.includes(evidence.id));
    const link = step?.kind === "tool" ? { label: step.action, itemId: step.id } : undefined;
    // A data number opens its reader at the matched cell.
    onOpenEvidence?.(evidence, cell ? { label: link?.label ?? "", itemId: link?.itemId, cell } : link);
  };
  const recordedStart = Date.parse(message.created_at);
  const usedKnowledge = exactMemoryReads(message.memory_refs);
  const showMemory = message.role === "assistant" && message.status !== "streaming"
    && message.memory_update !== undefined;
  const isLiveAssistantResponse = message.role === "assistant" && message.status === "streaming";
  // Pi streams its current text through progress until it knows whether the
  // message ends with a tool call or an answer. Only project it into the UI.
  const lastItem = message.progress?.items.at(-1);
  const draft = isLiveAssistantResponse && message.phase === "writing" && message.progress?.answerOffset !== undefined && lastItem?.kind === "commentary"
    ? lastItem : undefined;
  const trace = draft && message.progress
    ? { ...message.progress, items: message.progress.items.slice(0, -1) }
    : message.progress;
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
  // A paragraph that only links to a grid result renders as the sensitivity card where the
  // paragraph stood, and one that only links to a results snapshot as the compact results card.
  // Rust counted the links of the answer as written, so the grid paragraphs come out first.
  const visibleContent = visibleMessageContent(message);
  // The settled delta precedes removal of its draft. Use the offset from the
  // progress event so even a second assistant turn after guidance appears once.
  const liveText = draft && visibleContent.length === message.progress?.answerOffset
    ? withoutIdentifiers(draft.text) : "";
  const bodyText = visibleContent && liveText ? `${visibleContent}\n\n${liveText}` : visibleContent || liveText;
  const results = useMemo(() => {
    if (message.role !== "assistant" || message.status !== "complete") return { segments: [{ text: bodyText, numberChecks: message.number_checks }] as SensitivitySegment[], cards: [] };
    const grids = sensitivityParagraphs(bodyText, message.sensitivity_cards, message.number_checks);
    const segments = grids.segments.map((segment) => ({ ...earningsCards(segment.text, message.evidence_refs, segment.numberChecks), card: segment.card }));
    return { segments: segments as SensitivitySegment[], cards: segments.flatMap((segment) => segment.cards) };
  }, [message.role, message.status, bodyText, message.evidence_refs, message.number_checks, message.sensitivity_cards]);
  const documents = (message.artifact_refs ?? []).filter((artifact) => artifact.kind === "markdown");
  // The host's terminal placeholder is explained by the localized run state below.
  const isFailurePlaceholder = message.role === "assistant" && message.status === "error"
    && message.content.trim() === "Response could not be completed.";
  const showMessageBody = message.role === "user" || (!!bodyText && !isFailurePlaceholder);
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
  const failureLabel = message.failure_kind ? failureMessages()[message.failure_kind] : undefined;

  const transcript: ReactNode[] = [];
  if (message.role === "assistant") {
    const parts = turnTranscript(trace ?? { startedAtMs: recordedStart, items: [] }, workResults, usedKnowledge.length > 0, evidenceRefs);
    const firstWork = parts.find(part => part.kind === "work");
    let proseIndex = 0;
    // The same keyed Markdown body stays mounted when Pi identifies its
    // live text as a preamble, or when the settled answer replaces its draft.
    const appendProse = (text: string, streaming: boolean, checks?: ChatMessage["number_checks"]) => {
      if (!text) return;
      transcript.push(<Message key={`prose-${proseIndex++}`} from="assistant" className="max-w-full">
        <MessageContent className="message-content overflow-visible">
          <SafeMessageResponse text={text} isAnimating={streaming} evidenceRefs={message.evidence_refs}
            memoryRefs={message.memory_refs} numberChecks={checks} asOf={message.as_of}
            onOpenEvidence={(evidence, anchor) => openEvidence(evidence, undefined, anchor)}
            onOpenEvidenceCell={openEvidence} onShowStep={showStep} onOpenLink={onOpenLink}
            onOpenFile={onOpenFile} onLensLink={openLens} workspacePath={workspacePath} />
        </MessageContent>
      </Message>);
    };
    if (asset) transcript.push(<AssetCard key="asset" asset={asset} loupistId={loupistId} asOf={message.as_of} onOpenMapNode={location => onOpenMemory(location, asset.symbol)} />);
    for (const part of parts) {
      if (part.kind === "commentary") {
        appendProse(withoutIdentifiers(part.text), false);
        continue;
      }
      const activeStep = part.progress.items.some(item => item.kind !== "commentary" && item.status === "running");
      const lastGroup = part === parts.at(-1);
      const liveGroup = isLiveAssistantResponse && (activeStep || (lastGroup && !part.progress.items.length && !!part.progress.plan) || !!part.progress.approvals?.some(a => a.state === "waiting"));
      transcript.push(<ChatProgressTimeline key={part.key} compactTools
        showLiveSteps={isLiveAssistantResponse && (!bodyText.trim() || message.phase === "tool" || message.phase === "thinking")}
        planItemLabel={liveGroup && lastGroup ? trace?.plan?.items.find(item => item.status === "active")?.label : undefined}
        sources={part.sources} onOpenSource={onOpenEvidence ? (source, anchor) => openEvidence(source, undefined, anchor) : undefined}
        progress={part.progress} executionResults={part.evidence} memoryReads={part === firstWork ? usedKnowledge.map(({ ref }) => ref) : undefined}
        revealedResult={part.evidence.some(e => e.id === revealedResult?.id) ? revealedResult : undefined}
        status={isLiveAssistantResponse && !liveGroup ? "complete" : message.status}
        phase={liveGroup ? message.phase : undefined} focusItemId={progressFocus}
        onOpenLink={onOpenLink} onLensLink={openLens} asOf={message.as_of}
        renderApprovals={bridge ? approvals => <ApprovalCards approvals={approvals} refusals={[]} bridge={bridge} loupistId={loupistId} /> : undefined} />);
    }
    if (bridge && connectionRefusals.length) transcript.push(<ApprovalCards key="refusals" approvals={[]} refusals={connectionRefusals} bridge={bridge} loupistId={loupistId} />);
    if (message.status === "complete" && message.content) transcript.push(<AnswerOutline key="outline" messageId={message.id} markdown={message.content} defaultOpen={outlineOpen} />);
    if (showMessageBody) for (const segment of results.segments) {
      appendProse(segment.text, isLiveAssistantResponse, segment.numberChecks);
      if (segment.card) {
        const card = segment.card;
        transcript.push(<SensitivityCard key={`grid-${card.evidence_id}-${transcript.length}`} grid={card} subject={card.subject}
          evidence={evidenceRefs.find(item => item.id === card.evidence_id)} evidenceRefs={evidenceRefs} asOf={message.as_of}
          actions={{ onOpenEvidence: openEvidence, onShowStep: showStep }} onChangeRates={onFillComposer} />);
      }
    }
    const runningTool = trace?.items.some(item => item.kind !== "commentary" && item.status === "running");
    if (isLiveAssistantResponse && ((!runningTool && message.phase !== "writing" && !(bodyText && !message.phase)) || guiding?.length)) {
      transcript.push(<ChatProgressTimeline key="live-status" progress={{ startedAtMs: trace?.startedAtMs ?? recordedStart, items: [] }}
        status="streaming" phase={message.phase} guiding={guiding} onOpenLink={onOpenLink} />);
    }
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
        {message.status === "aborted" && <span className="message-status">{t("chatMessageCard.stopped")}</span>}
        {message.status === "error" && !retryPending ? <span className="message-status">{t("chatMessageCard.responseFailed")}</span> : null}
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
      {showMessageBody && message.role === "user" ? (
        <Message from={message.role} className="max-w-full">
          <MessageContent className="message-content overflow-visible">
            {(message.content || message.as_of) && <UserMessageText text={message.content} asOf={message.as_of} skills={mentionSkills} plugins={mentionPlugins} />}
          </MessageContent>
        </Message>
      ) : null}
      {transcript}
      {bridge ? results.cards.map((card) => <EarningsChatCard key={card.evidenceId} bridge={bridge} loupistId={loupistId} loupistName={loupistName} assetId={card.assetId} eventId={card.eventId} onOpenLink={(url) => void onOpenLink(url)} />) : null}

      {charts.map((block) => <ChatArtifactBlock key={block.key} block={block} onOpen={onOpenArtifact} onAsk={onAskArtifact} askDisabled={askArtifactDisabled} asOf={message.as_of} />)}
      {grids.map((block) => <ChatGridBlock key={block.key} block={block} actions={{ onOpenEvidence: openEvidence, onShowStep: showStep }} onAsk={onAskArtifact} askDisabled={askArtifactDisabled}
        onOpenMap={(entityId) => onOpenMemoryPage({ loupistId, surface: "world", view: { onlyDisputed: true, highlightEntityIds: [entityId] } })} />)}

      {message.role === "assistant" && <MessageSchedules receipts={message.schedule_receipts ?? []} onOpen={onOpenSchedule} />}

      <DeliveredFiles paths={deliveredPaths} onOpen={onOpenFile}
        onReveal={bridge ? (path) => bridge.workspaceReveal({ loupist_id: loupistId, path }) : undefined} />
      {documents.length > 0 ? (
        <div className="message-outputs" aria-label={t("chatMessageCard.outputs")}>
          {documents.map((artifact) => (
            <button className="message-output-card" type="button" key={`${artifact.artifact_id}:${artifact.revision}`} aria-label={t("chatMessageCard.openDocument", { title: artifact.title })} onClick={() => onOpenArtifact(artifact)}>
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

      {message.role === "assistant" && message.status === "complete" && message.as_of_report ? <AsOfNote report={message.as_of_report} loupistName={loupistName} /> : null}
      {message.role === "assistant" && message.status === "complete" && onNextStep ? <NextSteps loupistId={loupistId} chatId={chatId} messageId={message.id} onAsk={onNextStep} /> : null}
      {message.role === "assistant" ? (
        <MessageMemoryRow
          bridge={bridge}
          active={active}
          loupistId={loupistId}
          chatId={chatId}
          message={message}
          onOpenMemory={onOpenMemory}
          onOpenMemoryPage={onOpenMemoryPage}
          onOpenEvidence={onOpenEvidence ? (evidence, anchor) => openEvidence(evidence, undefined, anchor) : undefined}
          onDismissLensProposal={onDismissLensProposal}
          onReadWorldJob={onReadWorldJob}
          onWorldChanged={onWorldChanged}
        />
      ) : null}

      <div className="message-action-bar">
      {citationList.length > 0 && onOpenEvidence ? (
        <Disclosure className="message-sources" aria-label={t("chatMessageCard.researchSources")} title={<>{t("chatMessageCard.sourceCount", { count: citationList.length })}</>}>
          <ol>{citationList.map(({ evidence, number }) => (
            <li key={evidence.id}><button type="button" onClick={(event) => openEvidence(evidence, undefined, event.currentTarget)}>
              <span className="source-number">{number}</span><span><strong>{evidenceTitle(evidence)}</strong>
                <small className="source-site"><SourceFavicon evidence={evidence} /><span>{sourceIdentityLabel(evidence)} · {evidencePresentation(evidence).label}{evidence.retrieved_at ? ` · ${formatDate(new Date(evidence.retrieved_at), { dateStyle: "medium" })}` : ""}{evidence.excerpt_match ? t("chatMessageCard.citedPassage") : ""}</span></small>
              </span><ArrowUpRightIcon aria-hidden="true" />
            </button></li>
          ))}</ol>
        </Disclosure>
      ) : null}

      {note && message.role === "assistant" ? <p className="message-model-note">{note}</p> : null}
      {(message.status === "aborted" || message.status === "error") && message.role === "assistant" ? (
        <div className="message-run-state" role={message.status === "error" ? "alert" : "status"}>
          {message.status === "error" ? <ChatErrorState retryAfterSeconds={message.retry_after_seconds} kind={message.failure_kind ?? "other"} provider={providerName ?? t("chatError.yourProvider")}
            attempt={retryAttempt} fallback={failureLabel ?? failureMessages().other} onRetry={onRetry}
            onProviderAction={onProviderAction ? (action) => onProviderAction(action, message.execution_model?.provider) : undefined} /> : <>
            <p>{stoppedResult(message, visibleContent)}</p>
            {onRetry ? <button type="button" onClick={onRetry}>{t("chatMessageCard.tryAgain")}</button> : null}
          </>}
        </div>
      ) : null}
      {(message.role === "user" && (onEditUser || onEarlierVersion || onLaterVersion)) || (message.role === "assistant" && !isLiveAssistantResponse && !isFailurePlaceholder && (visibleContent || citationList.length > 0 || showMemory)) ? <div className="message-actions message-action-bar">
        {message.role === "assistant" ? <button type="button" aria-label={copyState === "copied" ? t("chatMessageCard.answerCopied") : t("chatMessageCard.copyAnswer")} title={copyState === "copied" ? t("chatMessageCard.copied") : t("chatMessageCard.copyAnswer")} onClick={() => {
          void navigator.clipboard.writeText(visibleContent).then(() => setCopyState("copied"), () => setCopyState("failed"));
        }}>{copyState === "copied" ? <CheckIcon /> : <CopyIcon />}</button> : null}
        {onEditUser ? <button type="button" aria-label={t("chatMessageCard.editMessage")} title={t("chatMessageCard.edit")} onClick={() => onEditUser(message.content)}>{t("chatMessageCard.edit")}</button> : null}
        {onEarlierVersion ? <button type="button" className="message-version-link" onClick={onEarlierVersion}>{t("chatBranch.earlierVersion")}</button> : null}
        {onLaterVersion ? <button type="button" className="message-version-link" onClick={onLaterVersion}>{t("chatBranch.laterVersion")}</button> : null}
        {copyState === "failed" ? <span role="alert">{t("chatMessageCard.couldNotCopySelectTheAnswerToCopyIt")}</span> : null}
      </div> : null}
      {message.role === "assistant" && message.status === "complete" && onSaveFeedback && onTeachFeedback ? <AnswerFeedback bridge={bridge} loupistId={loupistId} chatId={chatId} onOpenPrinciple={onOpenMemoryPage} message={message} onSave={onSaveFeedback} onTeach={onTeachFeedback} feedbackBusy={feedbackBusy} /> : null}
      </div>
      {quoteOpen ? <QuoteEvidencePopover evidence={quoteOpen.evidence} anchor={quoteOpen.anchor} pointer={quoteOpen.pointer} onClose={() => setQuoteOpen(null)} /> : null}
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

/** What a stopped reply kept (S4): finished steps and any text so far. */
function visibleMessageContent(message: ChatMessage): string {
  if (message.role !== "assistant" || message.status !== "aborted") return message.content;
  const content = message.content.trimEnd();
  if (content === "Response stopped.") return "";
  return content.replace(/\r?\n(?:\r?\n)*Response stopped\.$/u, "").trimEnd();
}

function stoppedResult(message: ChatMessage, visibleContent: string) {
  const steps = (message.progress?.items ?? []).filter((item) => (item.kind === "tool" || item.kind === "system") && item.status === "succeeded").length;
  const text = visibleContent.trim().length > 0;
  const draft = (message.progress?.items ?? []).some((item) => item.kind === "commentary" && item.text.trim().length > 0);
  const values = { count: steps, formattedCount: formatNumber(steps) };
  if (steps && text) return t("chatMessageCard.stoppedKept", values);
  if (steps && draft) return t("chatMessageCard.stoppedKeptStepsAndDraft", values);
  if (steps) return t("chatMessageCard.stoppedKeptSteps", values);
  if (text) return t("chatMessageCard.stoppedKeptText");
  return draft ? t("chatMessageCard.stoppedKeptDraft") : t("chatMessageCard.stoppedBeforeTheAnswerFinished");
}

export const ChatMessageCard = memo(ChatMessageCardComponent);
