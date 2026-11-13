import { Disclosure } from "../ui/disclosure";
import { useOpenProps } from "../navigation/Open";
import { formatNumber } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { createContext, memo, useCallback, useContext, useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { CalculatorIcon, ChartLineIcon, CheckIcon, ChevronRightIcon, CirclePauseIcon, CircleSlashIcon, CircleStopIcon, DatabaseIcon, TextIcon, TriangleAlertIcon } from "lucide-react";
import type { ChatEvidenceRef, ChatPlan, ChatProgressItem } from "../../types";
import { ChatActivityDetailView } from "./ChatActivityDetailView";
import { ChatExecutionProvider, useChatExecution } from "./ChatExecutionContext";
import { ChatToolResult } from "./ChatToolResult";
import { SourceFavicon } from "./SourceFavicon";
import { TurnGlyph, Ticker, useArrived } from "./turnMotion";
import { useConversationScroll } from "../ai-elements/conversation";
import { useChatMap } from "../workspace/ChatMapContext";
import { formatAsOf } from "../../chat/asOf";
import { evidenceTitle } from "../../chat/evidence";
import { readNoun } from "../../chat/progressLabels";
import { pausedByRule, planSections, refusedByRule, stepGlyph, type LedgerChip, type TurnSegment } from "../../chat/turnView";
import { actionLabel, isActionItem as isAction, visibleTarget, withoutIdentifiers } from "../../chat/runStatus";

const t = i18n.getFixedT(null, "chat");

/** The answer's as-of date: a step its source refused under it reads as skipped, never failed. */
const RowAsOf = createContext<string | undefined>(undefined);

/** How long chips take to leave at commit (`--gg-motion-fast`). */
const CHIPS_LEAVE_MS = 120;

type Props = {
  segment: Extract<TurnSegment, { kind: "ledger" }>;
  revealedResult?: { id: string; request: number } | null;
  focusItemId?: string | null;
  /** The answer's as-of date (`YYYY-MM-DD`). */
  asOf?: string;
};

type ActionItem = Extract<ChatProgressItem, { kind: "tool" | "system" }>;

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

/** "2.7s" for a step that took a second or more. */
const stepDuration = (item: ActionItem) => {
  if (!item.startedAtMs || item.finishedAtMs == null) return null;
  const milliseconds = item.finishedAtMs - item.startedAtMs;
  if (milliseconds < 1_000) return null;
  return t("ledger.stepSeconds", { value: formatNumber(milliseconds / 1_000, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) });
};

const ActionRow = memo(function ActionRow({
  item,
  evidence,
  revealedResult,
  expandedResults,
  onToggleResult,
  rowExpanded,
  focusItemId,
}: {
  item: ActionItem;
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
  const arrived = useArrived();
  const row = useRef<HTMLDivElement>(null);
  const detailsId = useId();
  const [open, setOpen] = useState(false);
  const [flash, setFlash] = useState(false);
  const skipped = item.status === "skipped";
  const action = skipped && asOf ? t("asOf.stepRefused", { tool: refusedRead(item) }) : actionLabel(item);
  const target = visibleTarget(item.target);
  const paused = pausedByRule(item);
  const duration = item.status === "succeeded" ? stepDuration(item) : null;
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
      className={`chat-progress-entry chat-progress-row ${item.status}`} data-testid="chat-progress-row"
      data-flash={flash || undefined}
      data-enter={arrived || undefined}
      data-progress-category={item.category}
      data-progress-operation={item.operation}
      data-progress-status={item.status}
    >
      {/* A skipped (refused) step leads with the skipped status glyph, not its read glyph. */}
      {skipped
        ? <span className="chat-progress-node" data-status="skipped"><CircleSlashIcon role="img" aria-label={t("asOf.stepStatus")} /></span>
        : <span className="chat-progress-node" aria-hidden="true"><TurnGlyph glyph={stepGlyph(item)} /></span>}
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
        {paused ? <span className="chat-progress-status" data-paused=""><CirclePauseIcon aria-hidden="true" />{t("chatProgressTimeline.paused.status")}</span>
          : item.status === "failed" ? <span className="chat-progress-status"><TriangleAlertIcon aria-hidden="true" />{t("chatProgressTimeline.failed")}</span>
          : item.status === "stopped" ? <span className="chat-progress-status"><CircleStopIcon aria-hidden="true" />{t("chatProgressTimeline.stopped")}</span>
          : null}
        {duration ? <span className="chat-progress-duration">{duration}</span> : null}
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
    </div>
  );
});

/** A note the turn wrote between steps, in its chronological place. */
function NoteRow({ text }: { text: string }) {
  const arrived = useArrived();
  return <div className="chat-progress-entry chat-progress-note" data-enter={arrived || undefined}>
    <span className="chat-progress-node" aria-hidden="true"><TextIcon /></span>
    <p className="chat-progress-note-text">{text}</p>
  </div>;
}

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

const CHIP_ICONS = { data: ChartLineIcon, memory: DatabaseIcon, calculation: CalculatorIcon } as const;

/** One piece of gathered evidence. Not a button: the whole line is the ledger's toggle. */
function Chip({ chip }: { chip: LedgerChip }) {
  const arrived = useArrived();
  const Icon = chip.kind === "source" ? null : CHIP_ICONS[chip.kind];
  return <span className="turn-ledger-chip" data-kind={chip.kind} data-enter={arrived || undefined}>
    {chip.evidence ? <SourceFavicon evidence={chip.evidence} /> : Icon ? <Icon aria-hidden="true" /> : null}
    <span className="turn-ledger-chip-label">{chip.label}</span>
    {chip.count > 1 ? <span className="turn-ledger-chip-count">{formatNumber(chip.count)}</span> : null}
  </span>;
}

/** Chips are shown while the reply runs and fade out at commit; a turn read later has none. */
function useChips(chips: LedgerChip[], settled: boolean) {
  const [gone, setGone] = useState(settled);
  useEffect(() => {
    if (!settled || gone) return;
    const timer = window.setTimeout(() => setGone(true), CHIPS_LEAVE_MS);
    return () => window.clearTimeout(timer);
  }, [settled, gone]);
  return { chips: gone ? [] : chips, leaving: settled && !gone };
}

/**
 * One line per turn: the steps counted and what was read gathered as chips
 * while the reply runs, a one-line receipt once it settles, at the same
 * height. It is the disclosure for every step: the timeline holds the sources,
 * consulted notes, plan, steps and notes. It never opens by itself.
 */
export function TurnLedger({ segment, revealedResult, focusItemId, asOf }: Props) {
  useTranslation("chat");
  const { ledger, progress, evidence, sources, memoryReads, live } = segment;
  const execution = useChatExecution();
  const conversation = useConversationScroll();
  const arrived = useArrived();
  const detailsId = useId();
  const actions = useMemo(() => progress.items.filter(isAction), [progress.items]);
  const [expanded, setExpanded] = useState(false);
  const [opening, setOpening] = useState(false);
  const [expandedResults, setExpandedResults] = useState<Set<string>>(new Set());
  const { chips, leaving } = useChips(ledger.chips, ledger.settled);

  useEffect(() => {
    if (!revealedResult) return;
    setExpanded(true);
    setExpandedResults((current) => new Set([...current, revealedResult.id]));
  }, [revealedResult]);
  useEffect(() => {
    if (!focusItemId) return;
    const step = progress.items.find((item) => item.id === focusItemId);
    if (!step || step.kind === "commentary") return;
    setExpanded(true);
    const ids = step.kind === "tool" ? step.resultIds ?? [] : [];
    if (ids.length === 0) return;
    setExpandedResults((current) => ids.every((id) => current.has(id)) ? current : new Set([...current, ...ids]));
  }, [focusItemId, progress.items]);
  useEffect(() => {
    if (!opening) return;
    const timer = window.setTimeout(() => setOpening(false), 400);
    return () => window.clearTimeout(timer);
  }, [opening]);

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
  const renderEntry = (entry: ChatProgressItem) => entry.kind === "commentary"
    ? <NoteRow key={entry.id} text={withoutIdentifiers(entry.text)} />
    : <ActionRow key={entry.id} item={entry} evidence={evidenceByStep.get(entry.id) ?? NO_EVIDENCE} revealedResult={revealedResult} expandedResults={expandedResults}
      onToggleResult={onToggleResult} rowExpanded={expanded} focusItemId={focusItemId} />;
  const chipsMore = (k: number) => chips.length > k ? <span className="turn-ledger-more" data-k={k}>{t("ledger.more", { formattedCount: formatNumber(chips.length - k) })}</span> : null;

  return (
    <section className="chat-progress turn-ledger" data-testid="chat-progress" aria-label={t("chatProgressTimeline.workProgress")} aria-busy={live || undefined}
      data-turn-item="tools" data-live={live || undefined} data-expanded={expanded} data-enter={arrived || undefined}>
      <button
        type="button"
        data-research-summary=""
        className="chat-progress-toggle turn-ledger-toggle" data-testid="chat-progress-toggle"
        aria-expanded={expanded}
        aria-controls={expanded ? detailsId : undefined}
        onClick={() => { conversation?.holdScrollPosition(); setExpanded(!expanded); setOpening(!expanded); }}
      >
        <ChevronRightIcon className="turn-ledger-chevron" aria-hidden="true" />
        <span className="chat-progress-heading turn-ledger-text" data-testid="chat-progress-heading">
          <Ticker id={ledger.settled ? `settled:${ledger.text}` : `live:${ledger.text}`}>
            {ledger.text}{ledger.failed ? <span className="turn-ledger-failed"> · {ledger.failed}</span> : null}
          </Ticker>
        </span>
        {chips.length ? <span className="turn-ledger-chips" data-leaving={leaving || undefined} aria-hidden="true">
          {chips.map((chip) => <Chip key={chip.key} chip={chip} />)}
          {chipsMore(5)}{chipsMore(3)}
        </span> : null}
      </button>
      {expanded ? (
        <ChatExecutionProvider value={detailScope}><RowAsOf.Provider value={asOf}>
          <div id={detailsId} className="chat-progress-items" data-opening={opening || undefined}>
            {sources.length ? <ul className="chat-work-sources" aria-label={t("chatReceipt.sourcesRead")}>
              {sources.map((source) => <li key={source.id}><WorkSource source={source} /></li>)}
            </ul> : null}
            {memoryReads.length ? <Disclosure className="chat-progress-memory" title={<>{t("chatProgressTimeline.consultedNotes", { count: memoryReads.length, formattedCount: formatNumber(memoryReads.length) })}</>}>
              <ul tabIndex={0} aria-label={t("chatProgressTimeline.usedInThisAnswer")}>{memoryReads.map((ref, index) => <li key={index}><strong>{ref.title}</strong>{ref.excerpt ? <p>{ref.excerpt}</p> : null}</li>)}</ul>
            </Disclosure> : null}
            {progress.plan ? <PlanList plan={progress.plan} nested={sections.nested} renderSteps={(items) => items.map(renderEntry)} /> : null}
            {sections.rest.map(renderEntry)}
            {unlinked.map((result) => <ChatToolResult key={result.id} evidence={result} expanded={expandedResults.has(result.id)} focusRequest={revealedResult?.id === result.id ? revealedResult.request : undefined} onToggle={() => onToggleResult(result.id)} />)}
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
