import { Disclosure } from "../ui/disclosure";
import { formatNumber } from "@/i18n/format";
import i18n, { useTranslation, Trans } from "@/i18n";
import { createContext, memo, useContext, useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { BookOpenIcon, CheckIcon, MessageSquareTextIcon, CirclePauseIcon, CircleSlashIcon, TriangleAlertIcon, CalculatorIcon, ChartLineIcon, ChevronDownIcon, CircleStopIcon, ClipboardCheckIcon, DatabaseIcon, FilePlusIcon, FileTextIcon, FolderIcon, GlobeIcon, ListTreeIcon, PencilIcon, PlayIcon, PuzzleIcon, RefreshCwIcon, SearchIcon, TerminalSquareIcon, UploadIcon, WrenchIcon, type LucideIcon, CornerDownRightIcon } from "lucide-react";
import type {
  ChatApproval,
  ChatMessage,
  ChatPlan,
  ChatProgress,
  ChatProgressCategory,
  ChatProgressItem,
  ChatProgressOperation,
  ChatProgressStatus,
  ChatEvidenceRef,
  MemoryRef,
} from "../../types";
import { SafeMessageResponse } from "./SafeMessageResponse";
import { ChatActivityDetailView } from "./ChatActivityDetailView";
import { ChatExecutionProvider, useChatExecution } from "./ChatExecutionContext";
import { ChatToolResult } from "./ChatToolResult";
import { useConversationScroll } from "../ai-elements/conversation";
import { useChatMap } from "../workspace/ChatMapContext";
import { formatAsOf } from "../../chat/asOf";
import { evidenceTitle } from "../../chat/evidence";
import { parseCredentialFreeHttpUrl } from "../../lib/credentialFreeUrl";
import { planSections } from "../../chat/turnTranscript";
import { readNoun } from "../../chat/progressLabels";
import {
  actionLabel,
  clockLabel,
  currentStepLabel,
  isActionItem as isAction,
  lastStepLabel,
  progressActivityKey,
  scrubIdentifiers,
  useNow,
  useRunHeartbeat,
  useThrottledAnnouncement,
  withoutIdentifiers,
} from "../../chat/runStatus";

const t = i18n.getFixedT(null, "chat");

/** The answer's as-of date: a step its source refused under it reads as skipped, never failed. */
const TimelineAsOf = createContext<string | undefined>(undefined);

type Props = {
  /** A folded research summary, with individual calls available inside. */
  compactTools?: boolean;
  /** Show only public step labels while this turn is working toward an answer. */
  showLiveSteps?: boolean;
  planItemLabel?: string;
  /** Distinct citation subjects attributed to this group by their result IDs. */
  sources?: ChatEvidenceRef[];
  onOpenSource?: (source: ChatEvidenceRef, anchor: HTMLElement) => void;
  progress: ChatProgress;
  status: ChatMessage["status"];
  phase?: ChatMessage["phase"];
  onOpenLink: (href: string) => Promise<void>;
  onLensLink?: (recordId: string) => void;
  memoryReads?: MemoryRef[];
  executionResults?: ChatEvidenceRef[];
  revealedResult?: { id: string; request: number } | null;
  focusItemId?: string | null;
  /** Guidance sent while this reply runs; it applies after the current step. */
  guiding?: readonly string[];
  /** The answer's as-of date (`YYYY-MM-DD`). */
  asOf?: string;
  /**
   * Draws approval cards (composer spec D3). Each sits under the step that waits
   * for it; with the steps folded, or without a known step, they follow the header.
   */
  renderApprovals?: (approvals: ChatApproval[]) => ReactNode;
};

type CommentaryItem = Extract<ChatProgressItem, { kind: "commentary" }>;
type ActionItem = Extract<ChatProgressItem, { kind: "tool" | "system" }>;
type StepGroup = {
  kind: "group";
  id: string;
  item: ActionItem;
  items: ActionItem[];
};
type TimelineEntry = CommentaryItem | ActionItem | StepGroup;

const emptyResults: ChatEvidenceRef[] = [];

const operationIcons: Record<ChatProgressOperation, LucideIcon> = {
  search: SearchIcon,
  read: BookOpenIcon,
  write: FilePlusIcon,
  edit: PencilIcon,
  list: ListTreeIcon,
  calculate: CalculatorIcon,
  publish: UploadIcon,
  execute: PlayIcon,
  submit: ClipboardCheckIcon,
  retry: RefreshCwIcon,
  compact: RefreshCwIcon,
  generic: WrenchIcon,
};

const categoryIcons: Record<ChatProgressCategory, LucideIcon> = {
  web: GlobeIcon,
  memory: DatabaseIcon,
  capability: PuzzleIcon,
  finance: ChartLineIcon,
  files: FolderIcon,
  artifact: FileTextIcon,
  compute: TerminalSquareIcon,
  system: RefreshCwIcon,
};

const groupKey = (item: ActionItem) =>
  `${item.operation}|${item.source?.name ?? item.category}|${item.action}`;

const timelineEntries = (items: ChatProgressItem[]): TimelineEntry[] => {
  const entries: TimelineEntry[] = [];
  for (const item of items) {
    if (!isAction(item) || item.status !== "succeeded") {
      entries.push(item);
      continue;
    }
    const previous = entries.at(-1);
    if (previous && previous.kind === "group" && groupKey(previous.item) === groupKey(item)) {
      previous.items.push(item);
      continue;
    }
    if (previous && previous.kind !== "group" && previous.kind !== "commentary" && previous.status === "succeeded" && groupKey(previous) === groupKey(item)) {
      entries[entries.length - 1] = { kind: "group", id: `group-${previous.id}`, item: previous, items: [previous, item] };
      continue;
    }
    entries.push(item);
  }
  return entries;
};

function LiveStepList({ actions }: { actions: ActionItem[] }) {
  useTranslation("chat");
  return <ol className="chat-live-steps">
    {timelineEntries(actions).map(entry => {
      if (entry.kind === "commentary") return null;
      const item = entry.kind === "group" ? entry.item : entry;
      const count = entry.kind === "group" ? entry.items.length : 1;
      const label = actionLabel(item);
      const Icon = operationIcons[item.operation];
      return <li key={item.id} data-progress-status={item.status}>
        <Icon aria-hidden="true" />
        <span>{count > 1 ? t("chatProgressTimeline.groupSteps", { action: label, count, formattedCount: formatNumber(count) }) : label}</span>
        {item.status === "failed" && !pausedByRule(item) ? statusGlyph("failed") : null}
        {item.status === "stopped" ? statusGlyph("stopped") : null}
        {item.status === "skipped" ? <CircleSlashIcon aria-label={t("asOf.stepStatus")} /> : null}
      </li>;
    })}
  </ol>;
}

const isInternalIdentifier = (value: string) =>
  /private[-_]tool[-_]id/i.test(value) || /^web:[0-9a-f]{6,}$/i.test(value.trim());

const visibleTarget = (target?: string) => {
  const trimmed = target?.trim();
  if (!trimmed || isInternalIdentifier(trimmed)) return undefined;
  const cleaned = scrubIdentifiers(trimmed);
  if (!cleaned || isInternalIdentifier(cleaned)) return undefined;
  return cleaned;
};

const sharedTarget = (items: ActionItem[]) => {
  const targets = items.map((item) => visibleTarget(item.target));
  const first = targets[0];
  return first && targets.every((target) => target === first) ? first : undefined;
};

function CommentaryRow({ item, onOpenLink, onLensLink }: {
  item: CommentaryItem;
  onOpenLink: (href: string) => Promise<void>;
  onLensLink?: (recordId: string) => void;
}) {
  useTranslation("chat");
  const text = withoutIdentifiers(item.text);
  if (!text) return null;
  return <div className="chat-progress-commentary">
    <span className="chat-commentary-label"><MessageSquareTextIcon aria-hidden="true" />{t("chatProgressTimeline.commentary")}</span>
    <SafeMessageResponse text={text} isAnimating={false} onOpenLink={onOpenLink} onLensLink={onLensLink} />
  </div>;
}

const ActionRow = memo(function ActionRow({
  item,
  count = 1,
  members,
  evidence,
  revealedResult,
  expandedResults,
  onToggleResult,
  onOpenLink,
  timelineExpanded,
  focusItemId,
  approvals,
}: {
  item: ActionItem;
  count?: number;
  members?: ActionItem[];
  /** Approval cards for this step, drawn under it. */
  approvals?: ReactNode;
  evidence: ChatEvidenceRef[];
  revealedResult?: { id: string; request: number } | null;
  expandedResults: Set<string>;
  onToggleResult: (id: string) => void;
  onOpenLink: (href: string) => Promise<void>;
  timelineExpanded: boolean;
  focusItemId?: string | null;
}) {
  useTranslation("chat");
  const execution = useChatExecution();
  const conversation = useConversationScroll();
  const asOf = useContext(TimelineAsOf);
  const row = useRef<HTMLDivElement>(null);
  const detailsId = useId();
  const [membersOpen, setMembersOpen] = useState(false);
  const [open, setOpen] = useState(false);
  const [flash, setFlash] = useState(false);
  const grouped = count > 1;
  const skipped = item.status === "skipped";
  const action = skipped && asOf ? t("asOf.stepRefused", { tool: refusedRead(item) }) : actionLabel(item);
  const display = grouped
    ? t(item.operation === "search" ? "chatProgressTimeline.groupQueries" : "chatProgressTimeline.groupSteps", { action, count, formattedCount: formatNumber(count) })
    : action;
  const Icon = item.kind === "tool" && item.source ? categoryIcons[item.category] : operationIcons[item.operation];
  const target = grouped ? sharedTarget(members ?? [item]) : visibleTarget(item.target);
  const failed = item.status === "failed" || members?.some((member) => member.status === "failed");
  const paused = pausedByRule(item);
  const glyph = paused
    ? <span className="chat-progress-status" data-paused=""><CirclePauseIcon role="img" aria-label={t("chatProgressTimeline.paused.status")} /></span>
    : statusGlyph(item.status);
  const canOpen = !grouped && Boolean((item.detailId && execution) || evidence.length || (skipped && asOf));
  const revealedHere = Boolean(revealedResult && evidence.some((result) => result.id === revealedResult.id));
  useEffect(() => {
    if (!focusItemId) return;
    const matched = item.id === focusItemId || members?.some((member) => member.id === focusItemId);
    if (!matched) return;
    if (members?.some((member) => member.id === focusItemId)) setMembersOpen(true);
    if (item.id === focusItemId) setOpen(true);
    row.current?.scrollIntoView({ block: "nearest" });
  }, [focusItemId, item.id, members]);
  useEffect(() => {
    if (!revealedHere) return;
    if (grouped) setMembersOpen(true);
    else setOpen(true);
    // "Show step" from a number: bring the step into view and flash it once.
    const reduced = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    row.current?.scrollIntoView?.({ block: "center", behavior: reduced ? "auto" : "smooth" });
    setFlash(true);
    const handle = window.setTimeout(() => setFlash(false), reduced ? 2000 : 1000);
    return () => window.clearTimeout(handle);
  }, [grouped, revealedHere, revealedResult?.request]);

  const toggle = () => {
    conversation?.holdScrollPosition();
    if (grouped) {
      setMembersOpen((current) => !current);
      return;
    }
    const next = !open;
    if (next) {
      for (const result of evidence) {
        if (!expandedResults.has(result.id)) onToggleResult(result.id);
      }
    }
    setOpen(next);
  };

  return (
    <div
      ref={row}
      className={`chat-progress-entry chat-progress-row ${item.status}${failed ? " failed" : ""}`}
      data-flash={flash || undefined}
      data-progress-category={item.category}
      data-progress-operation={item.operation}
      data-progress-status={item.status}
    >
      {/* A skipped (refused) step leads with the skipped status glyph, not its read glyph. */}
      {skipped
        ? <span className="chat-progress-node" data-status="skipped"><CircleSlashIcon role="img" aria-label={t("asOf.stepStatus")} /></span>
        : <span className="chat-progress-node"><Icon aria-hidden="true" /></span>}
      <div className="chat-progress-copy">
        {grouped || canOpen ? (
          <button
            type="button"
            className="chat-progress-action chat-progress-inspect"
            aria-label={canOpen ? t("chatProgressTimeline.inspectAction", { action: action }) : undefined}
            aria-expanded={grouped ? membersOpen : open}
            aria-controls={canOpen && open ? detailsId : undefined}
            onClick={toggle}
          >
            {display}
          </button>
        ) : (
          <span className="chat-progress-action">{display}</span>
        )}
        {target && item.href ? (
          <button type="button" className="chat-progress-target chat-progress-link" title={target} onClick={() => void onOpenLink(item.href!)}>{target}</button>
        ) : target ? (
          <span className="chat-progress-target" title={target}>{target}</span>
        ) : null}
        <StepNames items={members ?? [item]} />
      </div>
      {glyph}
      {/* A security rule, not a failure: said calmly in the step, with no way around it. */}
      {paused ? <div className="chat-step-paused" role="note">
        <p>{t(`chatProgressTimeline.paused.${pausedWhat(item)}`)}</p>
        <p className="chat-step-paused-why">{t("chatProgressTimeline.paused.why")}</p>
      </div> : null}
      {open && skipped && asOf ? <p id={detailsId} className="chat-progress-skipped-detail">{t("asOf.stepRefusedDetail", { date: formatAsOf(asOf) })}</p> : null}
      {open && item.detailId && execution ? <div id={detailsId} className="chat-execution-detail"><ChatActivityDetailView {...execution} detailId={item.detailId} outputRevision={item.kind === "tool" ? item.outputRevision : undefined} /></div> : null}
      {membersOpen && members ? <div className="chat-progress-children">{members.map((member) => <ActionRow key={member.id} item={member} count={1} evidence={evidence.filter((result) => member.kind === "tool" && member.resultIds?.includes(result.id))} revealedResult={revealedResult} expandedResults={expandedResults} onToggleResult={onToggleResult} onOpenLink={onOpenLink} timelineExpanded={timelineExpanded} focusItemId={focusItemId} />)}</div> : null}
      {open && !grouped ? evidence.map((result) => <ChatToolResult key={result.id} evidence={result} expanded={expandedResults.has(result.id)} focusRequest={timelineExpanded && revealedResult?.id === result.id ? revealedResult.request : undefined} onToggle={() => onToggleResult(result.id)} />) : null}
      {approvals ? <div className="chat-progress-approvals">{approvals}</div> : null}
    </div>
  );
});

/** World names this step read (host-attributed); a click shows them on the chat map. */
function StepNames({ items }: { items: ActionItem[] }) {
  useTranslation("chat");
  const chatMap = useChatMap();
  const ids = [...new Set(items.flatMap((item) => item.kind === "tool" ? item.entityIds ?? [] : []))];
  if (!ids.length) return null;
  const label = t("chatProgressTimeline.namesTouched", { count: ids.length, formattedCount: formatNumber(ids.length) });
  const onMap = chatMap?.map ? ids.filter((id) => chatMap.map!.nodes.some((node) => node.id === id)) : [];
  if (!chatMap || !onMap.length) return <span className="chat-progress-names">{label}</span>;
  return <button type="button" className="chat-progress-names" aria-label={t("chatProgressTimeline.namesTouchedOnMap", { count: ids.length, formattedCount: formatNumber(ids.length) })} onClick={() => chatMap.select(new Set(onMap))}>{label}</button>;
}

/** The agent's checklist (S1): ticks in order, the active item carries its step rows. */
function PlanList({ plan, nested, renderSteps }: {
  plan: ChatPlan;
  nested: Map<string, ChatProgressItem[]>;
  renderSteps: (items: ChatProgressItem[]) => ReactNode;
}) {
  useTranslation("chat");
  return (
    <ol className="chat-plan" aria-label={t("chatProgressTimeline.plan")}>
      {plan.items.map((item) => {
        const steps = nested.get(item.id);
        const label = withoutIdentifiers(item.label) || item.label;
        return (
          <li key={item.id} className="chat-plan-item" data-plan-status={item.status} aria-current={item.status === "active" ? "step" : undefined}>
            <div className="chat-plan-row">
              <span className="chat-plan-glyph" aria-hidden="true">{item.status === "done" ? <CheckIcon /> : <span className="chat-plan-dot" />}</span>
              <span className="chat-plan-label" title={label}>{label}</span>
              {item.status === "not_done" ? <span className="chat-plan-note">{t("chatProgressTimeline.notDone")}</span>
                : item.status === "done" ? <span className="sr-only">{t("chatProgressTimeline.planDone")}</span>
                : item.status === "pending" ? <span className="sr-only">{t("chatProgressTimeline.planPending")}</span>
                : null}
            </div>
            {steps?.length ? <div className="chat-plan-steps">{renderSteps(steps)}</div> : null}
          </li>
        );
      })}
    </ol>
  );
}

const statusGlyph = (status: ChatProgressStatus): ReactNode => {
  if (status === "succeeded") return <span className="chat-progress-status"><CheckIcon role="img" aria-label={t("chatProgressTimeline.planDone")} /></span>;
  if (status === "failed") return <span className="chat-progress-status"><TriangleAlertIcon aria-label={t("chatProgressTimeline.failed")} /></span>;
  if (status === "stopped") return <span className="chat-progress-status"><CircleStopIcon aria-label={t("chatProgressTimeline.stopped")} /></span>;
  return null;
};

/** A step Rust refused because the answer had read third-party text (`untrusted_text_paused`). */
const pausedByRule = (item: ActionItem) => item.kind === "tool" && item.status === "failed" && item.refusal === "untrusted_text_paused";

/** What the pause turned off for this step, in the user's words. */
const pausedWhat = (item: ActionItem): "command" | "site" | "instructions" | "schedule" | "connection" | "other" => {
  const tool = item.kind === "tool" ? item.toolName ?? "" : "";
  if (tool === "web_fetch") return "site";
  if (tool === "write" || tool === "edit") return "instructions";
  if (tool.startsWith("schedule_")) return "schedule";
  if (item.kind === "tool" && (item.source?.kind === "connector" || tool.startsWith("mcp__"))) return "connection";
  if (tool === "bash" || item.category === "compute" || item.operation === "execute") return "command";
  return "other";
};

/** A refused step's source as a noun ("Market data"); the row never repeats the as-of date. */
const refusedRead = (item: ActionItem) => {
  const read = item.kind === "tool" && item.toolName ? readNoun(item.toolName) : actionLabel(item);
  return `${read.charAt(0).toLocaleUpperCase(i18n.language)}${read.slice(1)}`;
};

const emptyGuiding: readonly string[] = [];

/** Use host categories for the subject. Never infer what a shell command did from its text. */
function researchSubject(actions: ActionItem[], results: ChatEvidenceRef[], memoryReads: MemoryRef[]) {
  const labels = new Set<string>();
  for (const item of actions) {
    if (item.kind !== "tool") continue;
    if (item.operation === "calculate") labels.add(t("chatProgressTimeline.research.calculations"));
    else if (item.category === "finance") {
      const leaf = item.toolName?.split("__").at(-1);
      if (leaf && i18n.exists(`chat:asOf.reads.${leaf}`)) {
        const noun = readNoun(leaf);
        labels.add(noun.charAt(0).toLocaleUpperCase(i18n.language) + noun.slice(1));
      } else labels.add(t("chatProgressTimeline.research.finance"));
    } else if (item.category === "web") labels.add(t("chatProgressTimeline.research.sources"));
    else if (item.category === "memory") labels.add(t("chatProgressTimeline.research.memory"));
    else if (item.category === "artifact") labels.add(t("chatProgressTimeline.research.outputs"));
    if (labels.size === 2) break;
  }
  if (!labels.size && memoryReads.length) labels.add(t("chatProgressTimeline.research.memory"));
  if (!labels.size && results.some(result => result.tool_name === "calculate")) labels.add(t("chatProgressTimeline.research.calculations"));
  return [...labels].join(" · ") || t("chatProgressTimeline.research.activity");
}

/** Keep a common verified domain beside the count. Mixed or unknown origins have no shared label. */
function sourcePreview(sources: ChatEvidenceRef[]) {
  const first = sources[0];
  if (!first) return "";
  const domains = new Set(sources.map(source => source.url && parseCredentialFreeHttpUrl(source.url)?.hostname.replace(/^www\./, "")));
  const [domain] = domains;
  if (domains.size === 1 && domain) return domain;
  if (sources.length !== 1 || first.kind !== "original" || /[:/\\]/u.test(first.title)) return "";
  return scrubIdentifiers(evidenceTitle(first)).slice(0, 80);
}

export function ChatProgressTimeline({
  compactTools = false,
  showLiveSteps = false,
  planItemLabel,
  sources = emptyResults,
  onOpenSource,
  progress,
  status,
  phase,
  onOpenLink,
  onLensLink,
  executionResults = emptyResults,
  memoryReads = [],
  revealedResult,
  focusItemId,
  guiding = emptyGuiding,
  asOf,
  renderApprovals,
}: Props) {
  useTranslation("chat");
  const execution = useChatExecution();
  const conversation = useConversationScroll();
  const isRunning = status === "streaming";
  const detailsId = useId();
  const actions = useMemo(() => progress.items.filter(isAction), [progress.items]);
  const problem = (!compactTools && (status === "error" || status === "aborted")) || actions.some((item) => item.status === "failed" || item.status === "stopped");
  const [userChoice, setUserChoice] = useState<boolean | null>(null);
  const [expandedResults, setExpandedResults] = useState<Set<string>>(new Set());
  const hasTrace = progress.items.length > 0 || Boolean(progress.plan) || executionResults.length > 0 || memoryReads.length > 0 || sources.length > 0 || Boolean(progress.approvals?.length);
  const digest = compactTools && hasTrace;
  const approvals = progress.approvals ?? [];
  const pendingApprovals = approvals.filter(approval => approval.state === "waiting");
  const foldedApprovals = digest ? pendingApprovals : approvals;
  const waiting = pendingApprovals.length > 0;
  const expanded = hasTrace && (userChoice ?? (!digest && (problem || (isRunning && (waiting || phase !== "writing")))));

  useEffect(() => {
    if (!revealedResult) return;
    setUserChoice(true);
    setExpandedResults((current) => new Set([...current, revealedResult.id]));
  }, [revealedResult]);
  useEffect(() => {
    if (!focusItemId) return;
    const step = progress.items.find((item) => item.id === focusItemId);
    if (!step || step.kind === "commentary") return;
    setUserChoice(true);
    const ids = step.kind === "tool" ? step.resultIds ?? [] : [];
    if (ids.length === 0) return;
    setExpandedResults((current) => ids.every((id) => current.has(id)) ? current : new Set([...current, ...ids]));
  }, [focusItemId, progress.items]);

  const sections = useMemo(() => planSections(progress.plan, progress.items), [progress.plan, progress.items]);
  const entriesFor = (items: ChatProgressItem[]): TimelineEntry[] => isRunning || compactTools ? items : timelineEntries(items);
  const evidenceById = useMemo(() => new Map(executionResults.map((item) => [item.id, item])), [executionResults]);
  const linkedIds = new Set(actions.flatMap((item) => item.kind === "tool" ? item.resultIds ?? [] : []));
  const unlinked = executionResults.filter((item) => !linkedIds.has(item.id));
  const stepCount = actions.length || executionResults.length || (memoryReads.length ? 1 : 0);
  const hasDraft = progress.items.some((item) => item.kind === "commentary" && item.text.trim().length > 0);
  // A step paused by the untrusted-text rule is not a failure; the header counts it apart.
  const pausedCount = actions.filter(pausedByRule).length;
  const failedCount = actions.filter((item) => item.status === "failed").length - pausedCount;
  const stoppedCount = actions.filter((item) => item.status === "stopped").length;
  const skippedCount = actions.filter(item => item.status === "skipped").length;
  const issueLabel = [
    failedCount > 0 ? digest ? t("chatProgressTimeline.research.failedAttempt", { count: failedCount })
      : t("chatProgressTimeline.failedCount", { failedCount }) : "",
    stoppedCount > 0 ? t("chatProgressTimeline.stoppedCount", { stoppedCount }) : "",
    pausedCount > 0 ? t("chatProgressTimeline.pausedCount", { pausedCount }) : "",
    digest && skippedCount > 0 ? asOf
      ? t("chatProgressTimeline.research.unavailableAsOf", { count: skippedCount, date: formatAsOf(asOf) })
      : t("chatProgressTimeline.research.unavailable", { count: skippedCount }) : "",
    digest && approvals.some(item => item.state === "declined") ? t("chatProgressTimeline.research.declined") : "",
    digest && approvals.some(item => item.state === "expired") ? t("chatProgressTimeline.research.expired") : "",
  ].filter(Boolean).join(" · ");
  const now = useNow(isRunning);
  const stalled = useRunHeartbeat(progressActivityKey(progress, phase), isRunning, phase, now);
  const liveLabel = stalled
    ? t("chatProgressTimeline.stillWorking", { step: lastStepLabel(progress, phase) })
    : currentStepLabel(progress, phase);
  // A multi-stage plan stays inside Details. Its current stage is useful
  // during the run; afterwards a single attributed stage can still name
  // its own group, but the last stage cannot describe a whole turn.
  const singleStage = progress.plan
    ? sections.nested.size === 1 && !sections.rest.length
      ? progress.plan.items.find(item => sections.nested.has(item.id))?.label
      : undefined
    : planItemLabel;
  const stage = isRunning ? planItemLabel ?? progress.plan?.items.find(item => item.status === "active")?.label : singleStage;
  const planSubject = stage && scrubIdentifiers(stage);
  const sourceCount = sources.length ? t("chatProgressTimeline.research.sourceCount", { count: sources.length, formattedCount: formatNumber(sources.length) }) : "";
  const subject = isRunning ? planSubject || sourceCount : sourceCount || planSubject;
  const digestLabel = digest ? subject || researchSubject(actions, executionResults, memoryReads) : "";
  const digestMeta = [digestLabel !== sourceCount && sourceCount, sourcePreview(sources), issueLabel].filter(Boolean).join(" · ");
  const announcement = useThrottledAnnouncement(digest ? [digestLabel, digestMeta].filter(Boolean).join(" · ") : liveLabel, isRunning);
  const end = isRunning ? now : (progress.finishedAtMs ?? progress.startedAtMs);
  const clock = <span className="chat-progress-elapsed">{clockLabel(end - progress.startedAtMs)}</span>;
  const detailScope = useMemo(() => execution ? { ...execution, active: execution.active && expanded } : null, [execution, expanded]);
  const onToggleResult = (id: string) => {
    conversation?.holdScrollPosition();
    setExpandedResults((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };
  // Approvals sit with the step that asked; one whose step is unknown follows the steps.
  const stepIds = new Set(actions.map((item) => item.id));
  const unplaced = approvals.filter((approval) => !approval.itemId || !stepIds.has(approval.itemId));
  const approvalsFor = (items: ActionItem[]) => {
    if (!renderApprovals) return undefined;
    const ids = new Set(items.map((item) => item.id));
    const own = approvals.filter((approval) => approval.itemId && ids.has(approval.itemId));
    return own.length ? renderApprovals(own) : undefined;
  };
  const renderEntry = (entry: TimelineEntry) => {
    if (entry.kind === "commentary") {
      return <CommentaryRow key={entry.id} item={entry} onOpenLink={onOpenLink} onLensLink={onLensLink} />;
    }
    if (entry.kind === "group") {
      return <ActionRow key={entry.id} item={entry.item} count={entry.items.length} members={entry.items} evidence={evidenceFor(entry.items, evidenceById)} revealedResult={revealedResult} expandedResults={expandedResults} onToggleResult={onToggleResult} onOpenLink={onOpenLink} timelineExpanded={expanded} focusItemId={focusItemId} approvals={approvalsFor(entry.items)} />;
    }
    return <ActionRow key={entry.id} item={entry} evidence={evidenceFor([entry], evidenceById)} revealedResult={revealedResult} expandedResults={expandedResults} onToggleResult={onToggleResult} onOpenLink={onOpenLink} timelineExpanded={expanded} focusItemId={focusItemId} approvals={approvalsFor([entry])} />;
  };

  const Header = hasTrace ? "button" : "div";
  return (
    <section className={`chat-progress${problem ? " has-failure" : ""}`} aria-label={t(hasTrace ? "chatProgressTimeline.workProgress" : "chatProgressTimeline.runStatus")} aria-busy={isRunning && !waiting} data-turn-item={hasTrace ? "tools" : "status"} data-run-status={status ?? "complete"} data-phase={isRunning ? phase : undefined} data-expanded={expanded}>
      <Header
        type={hasTrace ? "button" : undefined}
        data-research-summary={digest || undefined}
        data-tool-status={digest && isRunning ? "running" : undefined}
        className={hasTrace ? "chat-progress-toggle" : "chat-run-label"}
        aria-expanded={hasTrace ? expanded : undefined}
        aria-controls={expanded ? detailsId : undefined}
        onClick={hasTrace ? () => { conversation?.holdScrollPosition(); setUserChoice(!expanded); } : undefined}
      >
        {hasTrace && !digest ? <ChevronDownIcon className={`chat-progress-chevron ${expanded ? "open" : ""}`} aria-hidden="true" /> : null}
        {isRunning ? <span className="chat-progress-live" data-heartbeat={stalled || undefined} aria-hidden="true" /> : digest ? <ListTreeIcon aria-hidden="true" /> : null}
        <span className="chat-progress-heading">
          {digest ? <>
            {digestLabel}
            {digestMeta ? <span className="chat-work-count"> · {digestMeta}</span> : null}
          </>
            : progress.plan && !actions.length ? t("chatProgressTimeline.plan")
            : isRunning ? <Trans ns="chat" i18nKey="chatProgressTimeline.runningLabel" values={{ step: liveLabel }} components={[clock]} />
            : status === "aborted" && stepCount === 0 && hasDraft ? t("chatProgressTimeline.stoppedDraft")
            : status === "aborted" || status === "error" ? t(status === "aborted" ? "chatProgressTimeline.stoppedSummary" : "chatProgressTimeline.failedSummary", { count: stepCount, formattedCount: formatNumber(stepCount) })
            // Under a second, or with no recorded end, a duration would read "0:00"; the step count stands alone.
            : !progress.finishedAtMs || end - progress.startedAtMs < 1000
              ? t("chatProgressTimeline.completedSteps", { count: stepCount, formattedCount: formatNumber(stepCount), issues: issueLabel ? ` · ${issueLabel}` : "" })
              : <Trans ns="chat" i18nKey="chatProgressTimeline.completedSummary" count={stepCount} values={{ formattedCount: formatNumber(stepCount), issues: issueLabel ? ` · ${issueLabel}` : "" }} components={[clock]} />}
        </span>
        {digest ? <ChevronDownIcon className={`chat-progress-chevron ${expanded ? "open" : ""}`} aria-hidden="true" /> : null}
      </Header>
      <span className="sr-only" aria-live="polite">{announcement}</span>
      {renderApprovals && foldedApprovals.length && !expanded ? <div className="chat-progress-approvals" data-folded="">{renderApprovals(foldedApprovals)}</div> : null}
      {digest && showLiveSteps && userChoice === null && actions.length > 0 ? <LiveStepList actions={actions} /> : null}
      {isRunning && guiding.length ? <div className="chat-guiding">{guiding.map((text, index) => <p key={index} className="chat-guiding-row" role="status">
        <CornerDownRightIcon aria-hidden="true" />{t("chatProgressTimeline.guiding", { text })}
      </p>)}</div> : null}
      {expanded ? (
        <ChatExecutionProvider value={detailScope}><TimelineAsOf.Provider value={asOf}>
          <div id={detailsId} className="chat-progress-items" data-waiting={waiting || undefined}>
            {sources.length ? <ul className="chat-work-sources" aria-label={t("chatReceipt.sourcesRead")}>
              {sources.map(source => <li key={source.id}>{onOpenSource
                ? <button type="button" onClick={event => onOpenSource(source, event.currentTarget)}>{evidenceTitle(source)}</button>
                : evidenceTitle(source)}</li>)}
            </ul> : null}
            {memoryReads.length ? <Disclosure className="chat-progress-memory" title={<>{t("chatProgressTimeline.consultedNotes", { count: memoryReads.length, formattedCount: formatNumber(memoryReads.length) })}</>}>
              <ul tabIndex={0} aria-label={t("chatProgressTimeline.usedInThisAnswer")}>{memoryReads.map((ref, index) => <li key={index}><strong>{ref.title}</strong>{ref.excerpt ? <p>{ref.excerpt}</p> : null}</li>)}</ul>
            </Disclosure> : null}
            {progress.plan ? <PlanList plan={progress.plan} nested={sections.nested} renderSteps={(items) => entriesFor(items).map(renderEntry)} /> : null}
            {entriesFor(sections.rest).map(renderEntry)}
            {unlinked.length > 0 ? unlinked.map((result) => <ChatToolResult key={result.id} evidence={result} expanded={expandedResults.has(result.id)} focusRequest={revealedResult?.id === result.id ? revealedResult.request : undefined} onToggle={() => onToggleResult(result.id)} />) : null}
            {renderApprovals && unplaced.length ? <div className="chat-progress-approvals">{renderApprovals(unplaced)}</div> : null}
          </div>
        </TimelineAsOf.Provider></ChatExecutionProvider>
      ) : null}
    </section>
  );
}

function evidenceFor(items: ActionItem[], evidenceById: Map<string, ChatEvidenceRef>) {
  return items.flatMap((item) => item.kind === "tool" ? (item.resultIds ?? []).flatMap((id) => {
    const evidence = evidenceById.get(id);
    return evidence ? [evidence] : [];
  }) : []);
}
