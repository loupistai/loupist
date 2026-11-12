import { Disclosure } from "../ui/disclosure";
import { useOpenProps } from "../navigation/Open";
import { formatNumber } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { createContext, memo, useCallback, useContext, useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { BookOpenIcon, CheckIcon, CirclePauseIcon, CircleSlashIcon, TriangleAlertIcon, CalculatorIcon, ChartLineIcon, ChevronDownIcon, CircleStopIcon, ClipboardCheckIcon, DatabaseIcon, FilePlusIcon, FileTextIcon, FolderIcon, GlobeIcon, ListTreeIcon, PencilIcon, PlayIcon, PuzzleIcon, RefreshCwIcon, SearchIcon, TerminalSquareIcon, UploadIcon, WrenchIcon, type LucideIcon } from "lucide-react";
import type {
  ChatApproval,
  ChatEvidenceRef,
  ChatPlan,
  ChatProgressCategory,
  ChatProgressItem,
  ChatProgressOperation,
  ChatProgressStatus,
  ChatQuestion,
} from "../../types";
import { ChatActivityDetailView } from "./ChatActivityDetailView";
import { ChatExecutionProvider, useChatExecution } from "./ChatExecutionContext";
import { ChatToolResult } from "./ChatToolResult";
import { useConversationScroll } from "../ai-elements/conversation";
import { useChatMap } from "../workspace/ChatMapContext";
import { formatAsOf } from "../../chat/asOf";
import { evidenceTitle } from "../../chat/evidence";
import { readNoun } from "../../chat/progressLabels";
import { pausedByRule, planSections, refusedByRule, type TurnSegment } from "../../chat/turnView";
import { actionLabel, clockLabel, isActionItem as isAction, scrubIdentifiers, withoutIdentifiers } from "../../chat/runStatus";

const t = i18n.getFixedT(null, "chat");

/** The answer's as-of date: a step its source refused under it reads as skipped, never failed. */
const RowAsOf = createContext<string | undefined>(undefined);

type Props = {
  segment: Extract<TurnSegment, { kind: "activity" }>;
  revealedResult?: { id: string; request: number } | null;
  focusItemId?: string | null;
  /** The answer's as-of date (`YYYY-MM-DD`). */
  asOf?: string;
  /**
   * Draws approval cards (composer spec D3). Each sits under the step that waits
   * for it; with the steps folded, or without a known step, they follow the row.
   */
  renderApprovals?: (approvals: ChatApproval[]) => ReactNode;
  /** Draws question cards (`ask_user`) where approval cards go. */
  renderQuestions?: (questions: ChatQuestion[]) => ReactNode;
};

type ActionItem = Extract<ChatProgressItem, { kind: "tool" | "system" }>;

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

const isInternalIdentifier = (value: string) =>
  /private[-_]tool[-_]id/i.test(value) || /^web:[0-9a-f]{6,}$/i.test(value.trim());

const visibleTarget = (target: string | null) => {
  const trimmed = target?.trim();
  if (!trimmed || isInternalIdentifier(trimmed)) return undefined;
  const cleaned = scrubIdentifiers(trimmed);
  if (!cleaned || isInternalIdentifier(cleaned)) return undefined;
  return cleaned;
};

/** A step's page: it opens in the panel browser, or the system browser on Cmd-click. */
function StepLink({ href, target }: { href: string; target: string }) {
  const link = useOpenProps({ kind: "link", url: href });
  if (!link.available) return <span className="chat-progress-target" data-testid="chat-progress-target" title={target}>{target}</span>;
  return <button type="button" className="chat-progress-target chat-progress-link" data-testid="chat-progress-target" title={target} {...link.props}>{target}</button>;
}

function WorkSource({ source }: { source: ChatEvidenceRef }) {
  const link = useOpenProps({ kind: "evidence", evidence: source });
  return link.available ? <button type="button" {...link.props}>{evidenceTitle(source)}</button> : <>{evidenceTitle(source)}</>;
}

const ActionRow = memo(function ActionRow({
  item,
  evidence,
  revealedResult,
  expandedResults,
  onToggleResult,
  rowExpanded,
  focusItemId,
  approvals,
}: {
  item: ActionItem;
  /** Approval cards for this step, drawn under it. */
  approvals?: ReactNode;
  evidence: ChatEvidenceRef[];
  revealedResult?: { id: string; request: number } | null;
  expandedResults: Set<string>;
  onToggleResult: (id: string) => void;
  rowExpanded: boolean;
  focusItemId?: string | null;
}) {
  useTranslation("chat");
  const execution = useChatExecution();
  const conversation = useConversationScroll();
  const asOf = useContext(RowAsOf);
  const row = useRef<HTMLDivElement>(null);
  const detailsId = useId();
  const [open, setOpen] = useState(false);
  const [flash, setFlash] = useState(false);
  const skipped = item.status === "skipped";
  const action = skipped && asOf ? t("asOf.stepRefused", { tool: refusedRead(item) }) : actionLabel(item);
  const Icon = item.kind === "tool" && item.source ? categoryIcons[item.category] : operationIcons[item.operation];
  const target = visibleTarget(item.target);
  const failed = item.status === "failed";
  const paused = pausedByRule(item);
  const glyph = paused
    ? <span className="chat-progress-status" data-paused=""><CirclePauseIcon role="img" aria-label={t("chatProgressTimeline.paused.status")} /></span>
    : statusGlyph(item.status);
  const canOpen = Boolean((item.detailId && execution) || evidence.length || (skipped && asOf));
  const revealedHere = Boolean(revealedResult && evidence.some((result) => result.id === revealedResult.id));
  useEffect(() => {
    if (!focusItemId || item.id !== focusItemId) return;
    setOpen(true);
    row.current?.scrollIntoView({ block: "nearest" });
  }, [focusItemId, item.id]);
  useEffect(() => {
    if (!revealedHere) return;
    setOpen(true);
    // "Show step" from a number: bring the step into view and flash it once.
    const reduced = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    row.current?.scrollIntoView?.({ block: "center", behavior: reduced ? "auto" : "smooth" });
    setFlash(true);
    const handle = window.setTimeout(() => setFlash(false), reduced ? 2000 : 1000);
    return () => window.clearTimeout(handle);
  }, [revealedHere, revealedResult?.request]);

  const toggle = () => {
    conversation?.holdScrollPosition();
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
      className={`chat-progress-entry chat-progress-row ${item.status}${failed ? " failed" : ""}`} data-testid="chat-progress-row"
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
        {canOpen ? (
          <button
            type="button"
            className="chat-progress-action chat-progress-inspect" data-testid="chat-progress-action"
            aria-label={t("chatProgressTimeline.inspectAction", { action })}
            aria-expanded={open}
            aria-controls={open ? detailsId : undefined}
            onClick={toggle}
          >
            {action}
          </button>
        ) : (
          <span className="chat-progress-action" data-testid="chat-progress-action">{action}</span>
        )}
        {target && item.href ? (
          <StepLink href={item.href} target={target} />
        ) : target ? (
          <span className="chat-progress-target" data-testid="chat-progress-target" title={target}>{target}</span>
        ) : null}
        {glyph}
        <StepNames item={item} />
      </div>
      {/* A security rule, not a failure: said calmly in the step, with no way around it. */}
      {paused ? <div className="chat-step-paused" role="note">
        <p>{t(`chatProgressTimeline.paused.${pausedWhat(item)}`)}</p>
        <p className="chat-step-paused-why">{t("chatProgressTimeline.paused.why")}</p>
      </div> : refusedByRule(item) === "protected_path" ? <div className="chat-step-paused" role="note">
        <p>{t("chatProgressTimeline.refused.protectedPath")}</p>
      </div> : refusedByRule(item) === "outside_workspace" ? <div className="chat-step-paused" role="note">
        <p>{t("chatProgressTimeline.refused.outsideWorkspace")}</p>
      </div> : null}
      {open && skipped && asOf ? <p id={detailsId} className="chat-progress-skipped-detail">{t("asOf.stepRefusedDetail", { date: formatAsOf(asOf) })}</p> : null}
      {open && item.detailId && execution ? <div id={detailsId} className="chat-execution-detail"><ChatActivityDetailView {...execution} detailId={item.detailId} outputRevision={item.kind === "tool" ? item.outputRevision : undefined} /></div> : null}
      {open ? evidence.map((result) => <ChatToolResult key={result.id} evidence={result} expanded={expandedResults.has(result.id)} focusRequest={rowExpanded && revealedResult?.id === result.id ? revealedResult.request : undefined} onToggle={() => onToggleResult(result.id)} />) : null}
      {approvals ? <div className="chat-progress-approvals">{approvals}</div> : null}
    </div>
  );
});

/** World names this step read (host-attributed); a click shows them on the chat map. */
function StepNames({ item }: { item: ActionItem }) {
  useTranslation("chat");
  const chatMap = useChatMap();
  const ids = item.kind === "tool" ? [...new Set(item.entityIds ?? [])] : [];
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
  // Success is the norm and needs no mark; a failed or stopped step says so beside its target.
  if (status === "failed") return <span className="chat-progress-status"><TriangleAlertIcon aria-label={t("chatProgressTimeline.failed")} /></span>;
  if (status === "stopped") return <span className="chat-progress-status"><CircleStopIcon aria-label={t("chatProgressTimeline.stopped")} /></span>;
  return null;
};

/** What the pause turned off for this step, in the user's words. */
const pausedWhat = (item: ActionItem): "instructions" | "site" | "schedule" | "connection" | "other" => {
  const tool = item.kind === "tool" ? item.toolName ?? "" : "";
  if (tool === "write" || tool === "edit") return "instructions";
  if (tool === "web_fetch") return "site";
  if (tool.startsWith("schedule_")) return "schedule";
  if (item.kind === "tool" && (item.source?.kind === "connector" || tool.startsWith("mcp__"))) return "connection";
  return "other";
};

/** A refused step's source as a noun ("Market data"); the row never repeats the as-of date. */
const refusedRead = (item: ActionItem) => {
  const read = item.kind === "tool" && item.toolName ? readNoun(item.toolName) : actionLabel(item);
  return `${read.charAt(0).toLocaleUpperCase(i18n.language)}${read.slice(1)}`;
};

/**
 * One line per run of steps: the running step while one runs, a summary of
 * what was read and worked afterwards, at the same height. The details hold
 * the sources, consulted notes, plan and every step.
 */
export function ActivityRow({ segment, revealedResult, focusItemId, asOf, renderApprovals, renderQuestions }: Props) {
  useTranslation("chat");
  const { row, tone, progress, evidence, sources, memoryReads, autoExpand } = segment;
  const execution = useChatExecution();
  const conversation = useConversationScroll();
  const live = row.kind === "live";
  const detailsId = useId();
  const actions = useMemo(() => progress.items.filter(isAction), [progress.items]);
  const [userChoice, setUserChoice] = useState<boolean | null>(null);
  const [expandedResults, setExpandedResults] = useState<Set<string>>(new Set());
  const approvals = progress.approvals ?? [];
  const pendingApprovals = approvals.filter((approval) => approval.state === "waiting");
  const questions = progress.questions ?? [];
  const pendingQuestions = questions.filter((question) => question.state === "waiting");
  const waiting = pendingApprovals.length > 0 || pendingQuestions.length > 0;
  const expanded = userChoice ?? autoExpand;

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
  const evidenceById = useMemo(() => new Map(evidence.map((item) => [item.id, item])), [evidence]);
  const linkedIds = new Set(actions.flatMap((item) => item.kind === "tool" ? item.resultIds ?? [] : []));
  const unlinked = evidence.filter((item) => !linkedIds.has(item.id));
  const detailScope = useMemo(() => execution ? { ...execution, active: execution.active && expanded } : null, [execution, expanded]);
  // Stable inputs keep each memoized step row still while a later step streams.
  const evidenceByStep = useMemo(() => new Map(actions.map((item) => [item.id, evidenceFor(item, evidenceById)])), [actions, evidenceById]);
  const onToggleResult = useCallback((id: string) => {
    conversation?.holdScrollPosition();
    setExpandedResults((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  }, [conversation]);
  // Approvals sit with the step that asked; one whose step is unknown follows the steps.
  const stepIds = new Set(actions.map((item) => item.id));
  const unplaced = approvals.filter((approval) => !approval.itemId || !stepIds.has(approval.itemId));
  const unplacedQuestions = questions.filter((question) => !question.itemId || !stepIds.has(question.itemId));
  const cards = (forApprovals: ChatApproval[], forQuestions: ChatQuestion[]) => {
    const approvalCards = renderApprovals && forApprovals.length ? renderApprovals(forApprovals) : null;
    const questionCards = renderQuestions && forQuestions.length ? renderQuestions(forQuestions) : null;
    return approvalCards || questionCards ? <>{approvalCards}{questionCards}</> : undefined;
  };
  const unplacedCards = cards(unplaced, unplacedQuestions);
  const approvalsFor = (item: ActionItem) =>
    cards(approvals.filter((approval) => approval.itemId === item.id), questions.filter((question) => question.itemId === item.id));
  const renderEntry = (entry: ChatProgressItem) => entry.kind === "commentary" ? null : (
    <ActionRow key={entry.id} item={entry} evidence={evidenceByStep.get(entry.id) ?? NO_EVIDENCE} revealedResult={revealedResult} expandedResults={expandedResults}
      onToggleResult={onToggleResult} rowExpanded={expanded} focusItemId={focusItemId} approvals={approvalsFor(entry)} />
  );

  return (
    <section className="chat-progress" data-testid="chat-progress" aria-label={t("chatProgressTimeline.workProgress")} aria-busy={live && !waiting} data-turn-item="tools" data-tone={tone} data-expanded={expanded}>
      <button
        type="button"
        data-research-summary=""
        className="chat-progress-toggle" data-testid="chat-progress-toggle"
        aria-expanded={expanded}
        aria-controls={expanded ? detailsId : undefined}
        onClick={() => { conversation?.holdScrollPosition(); setUserChoice(!expanded); }}
      >
        <ListTreeIcon aria-hidden="true" data-live={live || undefined} data-heartbeat={live && row.stalled ? "" : undefined} />
        <span className="chat-progress-heading" data-testid="chat-progress-heading">
          {row.kind === "live" ? row.label : <>
            {row.label}
            {row.issues ? <span className="chat-work-count"> · <span className="chat-work-issues">{row.issues}</span></span> : null}
            {/* Under a second a duration would read "0:00"; the summary stands alone. */}
            {row.durationMs !== undefined && row.durationMs >= 1000 ? <span className="chat-work-count"> · <span className="chat-progress-elapsed">{clockLabel(row.durationMs)}</span></span> : null}
          </>}
        </span>
        <ChevronDownIcon className={`chat-progress-chevron ${expanded ? "open" : ""}`} aria-hidden="true" />
      </button>
      {!expanded && waiting ? <div className="chat-progress-approvals" data-folded="">{cards(pendingApprovals, pendingQuestions)}</div> : null}
      {expanded ? (
        <ChatExecutionProvider value={detailScope}><RowAsOf.Provider value={asOf}>
          <div id={detailsId} className="chat-progress-items" data-waiting={waiting || undefined}>
            {sources.length ? <ul className="chat-work-sources" aria-label={t("chatReceipt.sourcesRead")}>
              {sources.map((source) => <li key={source.id}><WorkSource source={source} /></li>)}
            </ul> : null}
            {memoryReads.length ? <Disclosure className="chat-progress-memory" title={<>{t("chatProgressTimeline.consultedNotes", { count: memoryReads.length, formattedCount: formatNumber(memoryReads.length) })}</>}>
              <ul tabIndex={0} aria-label={t("chatProgressTimeline.usedInThisAnswer")}>{memoryReads.map((ref, index) => <li key={index}><strong>{ref.title}</strong>{ref.excerpt ? <p>{ref.excerpt}</p> : null}</li>)}</ul>
            </Disclosure> : null}
            {progress.plan ? <PlanList plan={progress.plan} nested={sections.nested} renderSteps={(items) => items.map(renderEntry)} /> : null}
            {sections.rest.map(renderEntry)}
            {unlinked.map((result) => <ChatToolResult key={result.id} evidence={result} expanded={expandedResults.has(result.id)} focusRequest={revealedResult?.id === result.id ? revealedResult.request : undefined} onToggle={() => onToggleResult(result.id)} />)}
            {unplacedCards ? <div className="chat-progress-approvals">{unplacedCards}</div> : null}
          </div>
        </RowAsOf.Provider></ChatExecutionProvider>
      ) : null}
    </section>
  );
}

const NO_EVIDENCE: ChatEvidenceRef[] = [];

function evidenceFor(item: ActionItem, evidenceById: Map<string, ChatEvidenceRef>) {
  if (item.kind !== "tool" || !item.resultIds?.length) return NO_EVIDENCE;
  return item.resultIds.flatMap((id) => {
    const evidence = evidenceById.get(id);
    return evidence ? [evidence] : [];
  });
}
