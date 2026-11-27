import { whatsNewLinkText, whatsNewRows } from "../map/whatsNew";
import { readKnowledgeMap } from "../../knowledge/mapRead";
import { briefingFailure } from "./briefingFailure";
import { ChatMapProvider } from "../workspace/ChatMapContext";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "@/i18n";
import { formatDate, formatNumber, formatTime } from "@/i18n/format";
import type { TriggerEvaluation } from "../../contracts/generated";
import type { GrowthEvent, KnowledgeMap } from "../../contracts/knowledge";
import type { BriefingContext, BriefingSettings, ChatMessage, LoupistSummary, LoupistBridge, InboxItem, ScheduledRun, ScheduledTask, ScheduledTaskListItem, ModelCatalog } from "@/types";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import { Spinner } from "../ui/spinner";
import { Badge } from "../ui/badge";
import { ChevronRightIcon } from "lucide-react";
import { SegmentedControl, SegmentedControlItem } from "../ui/segmented-control";
import { LoginItemHint } from "./LoginItemHint";
import { ScheduleTaskControls } from "./ScheduleTaskControls";
import { LearningProgramActions } from "./LearningProgramActions";
import { TriggerCard } from "../chat/TriggerCard";
import { Disclosure } from "../ui/disclosure";
import { ChatMessageCard } from "../chat/ChatMessageCard";
import { MemorySaveWarning } from "../chat/ChatReceipt";
import { ChatExecutionProvider } from "../chat/ChatExecutionContext";
import { useUsageSummary } from "../settings/UsageSettings";
import { assetTicker } from "../../contracts/assets";
import { useAssetNames } from "../assets/assetName";
import { useGo } from "../navigation/Open";
import type { LearnedDigest } from "../../contracts/lens";
import { allowanceHeader, holdSentence, learnedOutcomeText, learnedThingCount, learningBudgetText, learningDeliveryText, learningDepthText, learningPresetText, learningKeepsText, learningReportText, learningRowOutcome, learningScopeText, relativeRun, runNowRefusal, runUndoText, runUsageText, runsNewestFirst, scheduledTaskTitle, scheduleSection, skippedSavedKeyLabel, skippedToolNames, stoppedForBudget, subjectChipModel, taskKindKey, unreadableTask, cadenceLabel, cadencePhrase, friendlyTimeZone, runWording } from "./scheduledPresentation";
import { revealFocused } from "../../lib/revealFocused";
import { printReader } from "../print/printDocuments";
import { PrinterIcon } from "lucide-react";
import { OpenProvider } from "../navigation/Open";
import type { ChatDestination, Open, OpenHow } from "../../chat/navigation";
import "@/styles/briefings.css";

type Props = { onOpenModelSettings?: () => void; bridge: LoupistBridge; loupist: LoupistSummary; theme: "light" | "dark";
  /** Provider status for each task's model: an account at its plan limit or needing reconnect makes the task wait. */
  modelCatalog?: ModelCatalog | null; initialRunId?: string; initialTaskId?: string; initialRun?: ScheduledRun;
  /** Opens what a run's answer cites beside that run's own Chat. */
  open: (run: InboxItem, to: ChatDestination, how?: OpenHow) => Promise<void>;
  onOpenMap?: (disputedEntityIds?: string[]) => void;
  /** Opens the run's own session; resolves false when that session is gone. */
  onInitialConsumed?: () => void; onOpenChat: (item: InboxItem) => Promise<boolean>;
  /** Puts a request into Home's composer (never sends): creating a task or changing its time or prompt is a conversation. */
  onAskHome?: (text: string) => void };

/** The reader works on inbox items; run history arrives as stored runs. */
type ReadableRun = InboxItem & {
  memorySaveSkip?: ScheduledRun["memory_save_skip"];
  discardedMemoryItems?: ScheduledRun["discarded_memory_items"];
  usage?: ScheduledRun["usage"];
  budget?: ScheduledRun["budget"];
  undo?: ScheduledRun["undo"];
};

export const inboxItemOf = (run: ScheduledRun): ReadableRun => ({
  memorySaveSkip: run.memory_save_skip,
  discardedMemoryItems: run.discarded_memory_items,
  usage: run.usage, budget: run.budget, undo: run.undo,
  briefingContext: run.briefing_context ?? null, error: run.error ?? null,
  runId: run.run_id, taskId: run.task_id, kind: run.kind, title: run.task_title,
  status: run.status, summary: run.summary, startedAtMs: run.started_at_ms,
  finishedAtMs: run.finished_at_ms ?? null, readAtMs: run.read_at_ms, threadId: run.thread_id ?? null,
  messageId: run.message_id, skippedTools: run.skipped_tools,
});

const HISTORY_ROWS = 10;

/** Active shows tasks that are on, waiting for confirmation, or running. */
function taskListed(task: ScheduledTaskListItem, filter: "active" | "all") {
  return unreadableTask(task) || filter === "all" || task.status === "active" || Boolean(task.awaiting_confirmation) || task.latest_run?.status === "running";
}
function taskRank(task: ScheduledTaskListItem) {
  if (task.awaiting_confirmation) return 0;
  if (task.latest_run?.status === "running" || task.status === "active") return 1;
  if (task.status === "paused") return 2;
  return 3;
}

/** A run's answer opens what it cites beside the Chat that made it. */
function RunLinks({ run, open, children }: { run: InboxItem; open: Props["open"]; children: React.ReactNode }) {
  const openInRun = useCallback<Open>((to, how) => open(run, to, how), [open, run]);
  return <OpenProvider open={openInRun} externalLinks={!run.threadId}>{children}</OpenProvider>;
}

/**
 * Scheduled tasks: one-time and recurring work, with an optional briefing among
 * them. Creating or changing a custom task seeds the Thread and waits for
 * confirmation; an alert is a custom task whose prompt states its condition. Results are read in the Thread; a run opened here shows its
 * structured briefing and full answer.
 */
export function ScheduleView({ onOpenModelSettings, modelCatalog, bridge, loupist, theme, initialRun, initialRunId, initialTaskId, onInitialConsumed, onOpenChat, open, onOpenMap, onAskHome }: Props) {
  const { t, i18n } = useTranslation("briefings");
  const [settings, setSettings] = useState<BriefingSettings | null>(null);
  const [tasks, setTasks] = useState<ScheduledTaskListItem[]>([]);
  // Nothing is open until the user picks a task or a deep link names one.
  const [focusTaskId, setFocusTaskId] = useState<string | null>(initialTaskId ?? null);
  const [focusExplicit, setFocusExplicit] = useState(Boolean(initialTaskId));
  const [history, setHistory] = useState<Record<string, ScheduledRun[]>>({});
  const [checkHistory, setCheckHistory] = useState<{ loupistId: string; taskId: string; rows: TriggerEvaluation[] }>();
  const [selected, setSelected] = useState<ReadableRun | null>(null);
  const [filter, setFilter] = useState<"active" | "all">("active");
  const [enabling, setEnabling] = useState(false);
  const [enableError, setEnableError] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [message, setMessage] = useState<ChatMessage | null>(null);
  const [readerError, setReaderError] = useState<string | null>(null);
  const [readerLoading, setReaderLoading] = useState(false);
  // "Print or save as PDF…" prints the reader as drawn now (shell-briefings §2.6).
  const readerRef = useRef<HTMLElement>(null);
  const [printFailed, setPrintFailed] = useState(false);
  useEffect(() => { setPrintFailed(false); }, [selected]);
  const [readerRevision, setReaderRevision] = useState(0);
  // A run that failed is tried again by running its task now; reloading would only show the same failure.
  const [rerun, setRerun] = useState<"starting" | "failed" | null>(null);
  const [runRefusal, setRunRefusal] = useState<string | null>(null);
  // A run's session goes with the run, so a reader opened before that finds none.
  const [continuedSession, setContinuedSession] = useState<string | null>(null);
  const [sessionGone, setSessionGone] = useState(false);
  useEffect(() => { setRerun(null); setSessionGone(false); }, [selected?.runId]);
  const generation = useRef(0);
  const failure = briefingFailure(selected?.error || readerError, message?.failure_kind);
  const briefingTask = tasks.find(task => task.kind === "briefing") ?? settings?.task ?? null;
  const running = tasks.filter(task => task.latest_run?.status === "running");

  const load = useCallback(async () => {
    const request = ++generation.current;
    try {
      const [next, list] = await Promise.all([bridge.briefingSettingsGet({ loupistId: loupist.id }), bridge.scheduledTasksList({ loupist_id: loupist.id })]);
      if (request !== generation.current) return;
      setSettings(next); setTasks(list); setError(false);
    } catch { if (request === generation.current) setError(true); }
    finally { if (request === generation.current) setLoading(false); }
  }, [bridge, loupist.id]);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- a request counter, not a DOM node: cleanup invalidates the request that is in flight.
  useEffect(() => { void load(); const off = bridge.onInboxChanged(event => { if (event.loupistId === loupist.id) void load(); }); return () => { generation.current++; off(); }; }, [load, bridge, loupist.id, modelCatalog]);

  const loadHistory = useCallback(async (taskId: string) => {
    try { const runs = await bridge.scheduledTaskRuns({ loupist_id: loupist.id, task_id: taskId }); setHistory(current => ({ ...current, [taskId]: runs })); }
    catch { setError(true); }
  }, [bridge, loupist.id]);
  useEffect(() => {
    if (focusTaskId && tasks.some(task => task.id === focusTaskId && !unreadableTask(task))) void loadHistory(focusTaskId);
  }, [focusTaskId, loadHistory, tasks]);
  const checkTask = tasks.find(task => task.id === focusTaskId && task.trigger);
  const checkTaskId = checkTask?.id;
  useEffect(() => {
    if (!checkTaskId) return;
    let current = true;
    void bridge.scheduledTriggerEvaluations({ loupist_id: loupist.id, task_id: checkTaskId }).then(
      rows => { if (current) setCheckHistory({ loupistId: loupist.id, taskId: checkTaskId, rows }); },
      () => { if (current) setError(true); },
    );
    return () => { current = false; };
  }, [bridge, checkTaskId, loupist.id, tasks]);
  const evaluations = checkHistory?.loupistId === loupist.id && checkHistory.taskId === focusTaskId
    ? checkHistory.rows : undefined;
  const continueInChat = (item: InboxItem) => void onOpenChat(item).then(opened => {
    if (opened) return;
    setSessionGone(true); void load(); if (focusTaskId) void loadHistory(focusTaskId);
  }).catch(() => undefined);
  // A run left open while it runs (Run now): the refreshed task list and history carry its
  // result, so the reader takes the new snapshot and loads the answer.
  useEffect(() => {
    if (!selected) return;
    // One list may be a read behind the other; a settled copy wins over a running one.
    // A copy that already carries the stored undo wins over one fetched before that marker existed.
    const copies = [...tasks.flatMap(task => task.latest_run ? [task.latest_run] : []), ...Object.values(history).flat()]
      .filter(run => run.run_id === selected.runId);
    const settled = copies.find(run => run.status !== "running") ?? copies[0];
    const fresh = copies.find(run => run.undo) ?? settled;
    if (!fresh || (fresh.status === selected.status && fresh.message_id === selected.messageId && fresh.memory_save_skip === selected.memorySaveSkip &&
      fresh.discarded_memory_items?.length === selected.discardedMemoryItems?.length &&
      fresh.usage?.requests === selected.usage?.requests && fresh.budget?.outcome === selected.budget?.outcome &&
      fresh.undo?.undone === selected.undo?.undone && fresh.undo?.kept === selected.undo?.kept)) return;
    const next = inboxItemOf(fresh);
    setSelected(current => current?.runId === next.runId ? { ...next, readAtMs: current.readAtMs ?? next.readAtMs } : current);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- reconciles on each refresh, keyed on the open run's id and state.
  }, [tasks, history, selected?.runId, selected?.status, selected?.messageId, selected?.memorySaveSkip, selected?.discardedMemoryItems?.length, selected?.usage?.requests, selected?.budget?.outcome, selected?.undo?.undone, selected?.undo?.kept]);

  // A run opened from a notification or a Thread message: find it across pages.
  useEffect(() => {
    if (loading || !initialRunId) return;
    let active = true;
    const open = async () => {
      if (initialRun?.run_id === initialRunId) {
        setSelected(inboxItemOf(initialRun)); setFocusTaskId(initialRun.task_id); setFocusExplicit(true);
        return;
      }
      let cursor: string | null | undefined;
      for (let page = 0; page < 10; page += 1) {
        const result = await bridge.inboxList({ loupistId: loupist.id, cursor, limit: 100 });
        const found = result.items.find(item => item.runId === initialRunId);
        if (found) {
          const runs = await bridge.scheduledTaskRuns({ loupist_id: loupist.id, task_id: found.taskId }).catch(() => []);
          const run = runs.find(record => record.run_id === found.runId);
          if (active) { setSelected(run ? inboxItemOf(run) : found); setFocusTaskId(found.taskId); setFocusExplicit(true); }
          return;
        }
        cursor = result.nextCursor;
        if (!cursor) break;
      }
      if (active) setError(true);
    };
    void open().catch(() => { if (active) setError(true); }).finally(() => { if (active) onInitialConsumed?.(); });
    return () => { active = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- consumed once after loading; later refreshes do not reopen it.
  }, [loading, initialRunId, initialRun]);
  useEffect(() => {
    if (loading || !initialTaskId || initialRunId) return;
    setFocusTaskId(initialTaskId); setFocusExplicit(true);
    const opened = tasks.find(task => task.id === initialTaskId);
    if (opened && !taskListed(opened, "active")) setFilter("all");
    document.querySelector(`[data-schedule-task="${CSS.escape(initialTaskId)}"]`)?.scrollIntoView({ block: "nearest" });
    onInitialConsumed?.();
  // eslint-disable-next-line react-hooks/exhaustive-deps -- consumed once after loading; a later task list must not refocus it.
  }, [loading, initialTaskId]);

  useEffect(() => {
    if (!selected || selected.readAtMs !== null || selected.status === "running") return;
    let timer: ReturnType<typeof setTimeout>;
    const begin = () => { clearTimeout(timer); if (document.visibilityState !== "hidden") timer = setTimeout(() => { void bridge.inboxMarkRead({ loupistId: loupist.id, runIds: [selected.runId] }).then(() => setSelected(current => current && current.runId === selected.runId ? { ...current, readAtMs: Date.now() } : current)).catch(() => setError(true)); }, 1000); };
    begin(); document.addEventListener("visibilitychange", begin);
    return () => { clearTimeout(timer); document.removeEventListener("visibilitychange", begin); };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed on the briefing's run and read state, not the object.
  }, [bridge, loupist.id, selected?.runId, selected?.readAtMs, selected?.status]);
  useEffect(() => {
    setMessage(null); setReaderError(null); if (!selected) return;
    let active = true; setReaderLoading(true);
    void bridge.loupistSelect(loupist.id).then(workspace => {
      if (!active) return;
      const thread = workspace.threads.find(thread => thread.id === selected.threadId);
      setContinuedSession(thread?.messages.some(item => item.role === "user" && !item.scheduled) ? thread.id : null);
      const answer = thread?.messages.find(message => message.id === selected.messageId) ?? null;
      setMessage(answer && selected.memorySaveSkip
        ? { ...answer, memory_update: { status: "no_change", commitId: null, changes: [], skipReason: selected.memorySaveSkip,
          discardedMemoryItems: selected.discardedMemoryItems ?? answer.memory_update?.discardedMemoryItems } }
        : answer);
      if (selected.status === "failed" || selected.status === "interrupted") setReaderError(selected.error || answer?.content || t("failed"));
    }).catch(() => { if (active) setReaderError(t("loadError")); }).finally(() => { if (active) setReaderLoading(false); });
    return () => { active = false; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed on the briefing's run, status and answer, not the object.
  }, [bridge, loupist.id, selected?.runId, selected?.status, selected?.messageId, selected?.memorySaveSkip, selected?.discardedMemoryItems, readerRevision, t]);

  // The answer's cards (grids, conflicts, charts) read their artifacts in the run's own Chat.
  const executionScope = useMemo(() => selected?.threadId
    ? { bridge, loupistId: loupist.id, threadId: selected.threadId, surface: "session" as const, theme, active: true }
    : null, [bridge, loupist.id, selected?.threadId, theme]);
  const displayTitle = (item: { kind: string; title: string; edition?: string | null }) => scheduledTaskTitle(item.kind, item.title, item.edition);
  /** A run is worded by its task's kind: a briefing, a learning program, or scheduled research. */
  const runKind = runWording;
  const usage = useUsageSummary(bridge, 1, true);
  const allowance = allowanceHeader(usage?.allowance.background);
  const learningRunning = usage?.allowance.learningRunning === true;
  const needsNames = tasks.some(task => task.kind === "learning");
  const nameOf = useAssetNames(bridge, loupist.id, needsNames);
  const [assetRows, setAssetRows] = useState<Awaited<ReturnType<LoupistBridge["assetsList"]>>>([]);
  useEffect(() => {
    if (!needsNames) return;
    let live = true;
    void bridge.assetsList({ loupistId: loupist.id }).then(rows => { if (live) setAssetRows(rows); }).catch(() => {});
    return () => { live = false; };
  }, [bridge, loupist.id, needsNames]);
  const assetNames = useMemo(() => new Map(assetRows.map(row => [row.assetId, nameOf({ name: row.name, assetId: row.assetId, symbol: assetTicker(row) ?? undefined, market: row.reference?.exchange })] as const)), [assetRows, nameOf]);
  const go = useGo();
  /** Why a run failed, when it is known; an unknown failure is said by the title alone, with Details beside it. */
  const failureReason = (kind: ReturnType<typeof briefingFailure>) => kind === "generic" ? null : t(`failure.${kind}`);
  const lastFailure = (task: ScheduledTaskListItem) => {
    const reason = failureReason(briefingFailure(task.latest_run?.error ?? task.last_error));
    return reason ? t("schedule.lastFailedReason", { reason }) : t("schedule.lastFailed");
  };
  const listed = (task: ScheduledTaskListItem) => taskListed(task, filter);
  // A task on an account that cannot answer right now waits instead of failing (providers spec §3.1).
  const waiting = (task: ScheduledTask) => {
    if (task.status !== "active") return null;
    const provider = task.model_selection?.model?.provider;
    const status = [...modelCatalog?.providers ?? [], ...modelCatalog?.custom_providers ?? []].find(option => option.id === provider)?.status;
    return status?.notice?.kind === "plan_limit" ? t("schedule.waitingPlan") : status?.state === "needs_reconnect" ? t("schedule.waitingReconnect") : null;
  };
  const taskStatus = (task: ScheduledTask) => task.awaiting_confirmation ? t("schedule.status.awaiting") : waiting(task) ?? t(`schedule.status.${task.status}`);
  const nextRun = (task: ScheduledTask) => task.status === "active" && task.next_run_at_ms
    ? t("schedule.next", { time: relativeRun(task.next_run_at_ms, Date.now(), task.schedule.kind === "weekly" || task.schedule.kind === "daily" ? task.schedule.timezone : undefined) })
    : null;
  const ordered = tasks.filter(task => task.declined_in_visit === undefined).sort((a, b) => taskRank(a) - taskRank(b) || (a.next_run_at_ms ?? Number.MAX_SAFE_INTEGER) - (b.next_run_at_ms ?? Number.MAX_SAFE_INTEGER) || displayTitle(a).localeCompare(displayTitle(b)));
  const visible = ordered.filter(listed);
  // The built-in briefing always has a row: today's row while it is on or waiting for an OK, and an
  // offer to turn it on while it is off (never created, or turned off, which pauses it).
  const editionOf = (task: { kind: string; edition?: string | null }) => task.kind === "briefing" && task.edition === "evening" ? "evening" : task.kind === "briefing" ? "morning" : null;
  const briefingFor = (edition: "morning" | "evening") => ordered.find(task => editionOf(task) === edition) ?? null;
  const morningBriefing = briefingFor("morning");
  const eveningBriefing = briefingFor("evening");
  const briefingIsOff = (task: ScheduledTaskListItem | null) => !task || (task.status !== "active" && !task.awaiting_confirmation);
  const morningOff = briefingIsOff(morningBriefing);
  const eveningOff = briefingIsOff(eveningBriefing);
  const scheduled = (task: ScheduledTaskListItem) => {
    const edition = editionOf(task);
    return edition === "morning" ? !morningOff : edition === "evening" ? !eveningOff : true;
  };
  const rows = visible.filter(scheduled);
  const hiddenByFilter = ordered.some(task => scheduled(task) && !listed(task));
  const focusTask = !focusExplicit ? null : ordered.find(task => task.id === focusTaskId && !unreadableTask(task)) ?? null;
  const readerOpen = Boolean(selected) || Boolean(focusTask);
  // One time zone for every timed task is named once in the header instead of on each row.
  const zones = new Set(ordered.flatMap(task => task.schedule.kind === "daily" || task.schedule.kind === "weekly" ? [task.schedule.timezone] : []));
  const sharedZone = zones.size === 1 ? [...zones][0]! : null;
  const cadence = (task: ScheduledTask) => sharedZone ? cadencePhrase(task.schedule) : cadenceLabel(task.schedule);
  const openTask = (task: ScheduledTask) => { setSelected(null); setFocusTaskId(task.id); setFocusExplicit(true); };
  const runFailed = selected?.status === "failed" || selected?.status === "interrupted";
  const taskRunning = Boolean(selected && tasks.find(task => task.id === selected.taskId)?.latest_run?.status === "running");
  /** Runs the failed run's task once more and shows that task, where the new run appears as it runs. */
  const runAgain = async (run: InboxItem, confirm = false) => {
    setRerun("starting");
    try {
      await bridge.scheduledTaskRunNow(confirm
        ? { loupist_id: loupist.id, task_id: run.taskId, confirm: true }
        : { loupist_id: loupist.id, task_id: run.taskId });
      setRunRefusal(null);
      setRerun(null); setSelected(null); setFocusTaskId(run.taskId); setFocusExplicit(true);
      void load(); void loadHistory(run.taskId);
    } catch (cause) {
      const refusal = confirm ? null : runNowRefusal(cause);
      if (refusal) { setRerun(null); setRunRefusal(refusal); return; }
      setRerun("failed");
    }
  };

  const enableBriefing = async (edition?: "morning" | "evening") => {
    setEnabling(true);
    setEnableError(false);
    try {
      await bridge.briefingSettingsSet({ loupistId: loupist.id, enabled: true, edition });
      await load();
    } catch {
      setEnableError(true);
    } finally {
      setEnabling(false);
    }
  };
  const runRow = (run: ScheduledRun) => {
    const failed = run.status === "failed" || run.status === "interrupted";
    const budgetStop = stoppedForBudget(run.budget?.outcome);
    // A check that found nothing to report posted nothing, so it never waits to be read.
    const quiet = run.quiet && run.status === "succeeded";
    const unread = run.read_at_ms === null && run.status !== "running" && !quiet;
    return <li key={run.run_id}><button type="button" className="briefings-item" data-schedule-run={run.run_id} aria-current={selected?.runId === run.run_id || undefined} data-unread={unread} data-quiet={quiet || undefined} onClick={() => { setSelected(inboxItemOf(run)); setFocusExplicit(true); }}>
      <span className="briefings-item-heading">{unread && <><span className="briefings-unread" aria-hidden="true" /><span className="sr-only">{t("unread")}</span></>}<strong>{formatDate(run.started_at_ms, { month: "short", day: "numeric", year: undefined })}</strong><time>{formatTime(run.started_at_ms)}</time>{quiet ? <span className="schedule-run-quiet">{t("schedule.quiet")}</span> : null}</span>
      <span className="briefings-summary" data-failed={failed && !budgetStop}>{run.status === "running" ? <><Spinner />{t(`preparingBody.${runKind(run.kind)}`)}</> : budgetStop ? t("schedule.learning.stoppedLimit") : failed ? t(`failure.title.${runKind(run.kind)}`) : run.summary}</span>
    </button></li>;
  };
  const taskRow = (task: ScheduledTaskListItem) => {
    if (unreadableTask(task)) return <li key={task.id} className="schedule-task" data-schedule-task={task.id} data-status="unreadable">
      <div className="schedule-task-summary"><strong>{t("schedule.unreadableTask")}</strong><span className="briefings-meta">{task.id}</span></div>
    </li>;
    const next = nextRun(task);
    const state = rowState(task);
    const outcome = task.kind === "learning" ? learningRowOutcome(task, learningRunning) : holdSentence(task.hold?.reason);
    const current = focusTask?.id === task.id;
    const showState = state && !(outcome && task.status === "active" && !task.awaiting_confirmation && !waiting(task));
    return <li key={task.id} className="schedule-task" data-schedule-task={task.id} data-kind={task.kind} data-status={task.awaiting_confirmation ? "awaiting" : task.status} data-hold={task.hold?.reason} data-current={current || undefined}>
      <div className="schedule-task-row">
        <button type="button" className="schedule-task-summary" aria-current={current || undefined} onClick={() => openTask(task)}>
          <span className="schedule-task-heading"><strong title={displayTitle(task)}>{displayTitle(task)}</strong>{task.kind === "briefing" && builtIn}{showState ? <span className="schedule-task-status">{state}</span> : null}</span>
          <span className="briefings-meta">{cadence(task)}{next ? ` · ${next}` : ""}</span>
          {outcome ? <span className="briefings-summary" data-learning-outcome={task.kind === "learning" || undefined}>{outcome}</span> : null}
        </button>
        <ChevronRightIcon className="schedule-task-chevron" aria-hidden="true" />
      </div>
      {task.kind === "learning" ? <SubjectChips subjects={task.subjects} names={assetNames} onOpenAsset={go ? (assetId, name) => go({ kind: "asset", target: { loupistId: loupist.id, assetId, name } }) : undefined} /> : null}
    </li>;
  };
  const briefingRow = (edition: "morning" | "evening") => {
    const off = edition === "morning" ? morningOff : eveningOff;
    const briefing = edition === "morning" ? morningBriefing : eveningBriefing;
    if (!off && briefing && rows.some(item => item.id === briefing.id)) return taskRow(briefing);
    if (!off) return null;
    return <BriefingOffRow key={edition} marker={edition} description={t(edition === "morning" ? "schedule.briefingDescription" : "schedule.eveningDescription")} title={displayTitle(briefing ?? { kind: "briefing", title: edition === "evening" ? "Evening briefing" : "", edition })} builtIn={builtIn} enabling={enabling} enableError={enableError}
      current={Boolean(briefing && focusTask?.id === briefing.id)} onOpen={briefing ? () => openTask(briefing) : undefined} onTurnOn={() => void enableBriefing(edition === "morning" ? undefined : "evening")} />;
  };
  const sectionRows = (key: string, title: string, items: Array<React.ReactNode | null | false>) => {
    const present = items.filter(Boolean);
    if (!present.length) return [];
    return [<li key={`${key}-heading`} className="schedule-section-label" data-schedule-section={key}><h3><span className="schedule-section-name">{title}</span><span className="schedule-section-count" aria-hidden="true">{formatNumber(present.length)}</span></h3></li>, ...present];
  };
  const checks = rows.filter(task => scheduleSection(task) === "checks");
  const research = rows.filter(task => scheduleSection(task) === "research");
  const learning = rows.filter(task => scheduleSection(task) === "learning");
  const otherBriefings = rows.filter(task => scheduleSection(task) === "briefings" && task.id !== morningBriefing?.id && task.id !== eveningBriefing?.id);
  const headerMeta = [
    rows.length ? t("schedule.summary", { count: rows.length, formattedCount: formatNumber(rows.length) }) : null,
    running.length ? t("schedule.runningCount", { count: running.length, formattedCount: formatNumber(running.length) }) : null,
    sharedZone ? friendlyTimeZone(sharedZone, i18n.language) : null,
  ].filter(Boolean).join(" · ");
  const rowState = (task: ScheduledTaskListItem) => task.awaiting_confirmation || task.status !== "active" || waiting(task) ? taskStatus(task) : null;
  const introductory = !ordered.length && !readerOpen;
  const listShown = !introductory && (rows.length > 0 || morningOff || eveningOff);
  const spending = <p className="briefings-meta schedule-spending" data-schedule-spending>{t("schedule.spending")}</p>;
  const builtIn = <Badge variant="outline">{t("schedule.builtIn")}</Badge>;
  const skipped = skippedToolNames(selected?.skippedTools ?? []);
  const skippedSavedKeys = skippedSavedKeyLabel(skipped.savedKeys);

  return <section className="briefings-view schedule-view" aria-label={t("schedule.title")}>
    {ordered.length ? <header className="schedule-header">
      {headerMeta ? <p className="briefings-meta">{headerMeta}</p> : null}
      {allowance ? <p className="briefings-meta schedule-allowance" data-tone={allowance.tone} role="status">{allowance.text}</p> : null}
      {!readerOpen && (morningOff || eveningOff) && spending}
    </header> : null}
    {error && <p role="alert" className="briefings-error">{t("loadError")} <Button variant="ghost" onClick={() => void load()}>{t("retry")}</Button></p>}
    {loading ? <p role="status">{t("loading")}</p> : <div className="briefings-layout" data-run-open={Boolean(selected) || undefined} data-intro={introductory || undefined} data-reader-open={readerOpen || undefined} data-empty={(!listShown && !selected) || undefined}>
      <div className="schedule-list">
        {ordered.length ? <SegmentedControl className="schedule-filter" value={filter} onValueChange={value => setFilter(value as typeof filter)} aria-label={t("schedule.filterLabel")} data-schedule-filter={filter}>
          <SegmentedControlItem value="active">{t("schedule.filterActive")}</SegmentedControlItem>
          <SegmentedControlItem value="all">{t("schedule.filterAll")}</SegmentedControlItem>
        </SegmentedControl> : null}
        {running.length ? <section aria-label={t("schedule.running")}>
          <h2 className="schedule-section-label">{t("schedule.running")}</h2>
          <ul className="schedule-running">{running.map(task => <li key={task.id}>
            <Spinner /><span className="schedule-running-title">{displayTitle(task)}</span>{task.kind === "briefing" && builtIn}
            <time>{formatTime(task.latest_run!.started_at_ms)}</time>
            <Button variant="ghost" size="sm" onClick={() => { setFocusTaskId(task.id); setSelected(inboxItemOf(task.latest_run!)); setFocusExplicit(true); }}>{t("schedule.open")}</Button>
          </li>)}</ul>
        </section> : null}
        <LoginItemHint bridge={bridge} active={tasks.some(task => task.status === "active")} />
        <section aria-label={t("schedule.tasks")}>
          {listShown ? <h2 className="schedule-section-label">{t("schedule.tasks")}</h2> : null}
          {introductory ? <div className="schedule-empty">
            <h2>{t("schedule.emptyTitle")}</h2>
            <div className="schedule-options">
              <div className="start-option-card schedule-option">
                <h3>{t("morning")}</h3>
                <p>{t("schedule.briefingDescription")}</p>
                <Button type="button" data-schedule-morning disabled={enabling} onClick={() => void enableBriefing()}>{t("schedule.turnOn")}</Button>
                {enableError && <p role="alert" className="inline-error">{t("schedule.statusFailed")}</p>}
              </div>
              <div className="start-option-card schedule-option">
                <h3>{t("evening")}</h3>
                <p>{t("schedule.eveningDescription")}</p>
                <Button type="button" data-schedule-evening disabled={enabling} onClick={() => void enableBriefing("evening")}>{t("schedule.turnOn")}</Button>
              </div>
              <div className="start-option-card schedule-option">
                <h3>{t("schedule.filings")}</h3>
                <Button type="button" variant="outline" disabled={enabling} onClick={() => {
                  setEnabling(true); setEnableError(false);
                  void bridge.scheduledTaskFromPreset({ loupistId: loupist.id, preset: "filings-and-results" })
                    .then(() => load(), () => setEnableError(true)).finally(() => setEnabling(false));
                }}>{t("schedule.turnOn")}</Button>
              </div>
              <div className="start-option-card schedule-option">
                <h3>{t("schedule.ownTask")}</h3>
                <p>{t("schedule.ownDescription", { example: t("schedule.ownExample") })}</p>
                <Button type="button" variant="outline" disabled={!onAskHome} onClick={() => onAskHome?.(t("schedule.ownExample"))}>{t("schedule.askBot")}</Button>
              </div>
            </div>
            {spending}
          </div>
            : <>{!rows.length && hiddenByFilter ? <div className="briefings-empty"><h3>{t("schedule.noActiveTitle")}</h3>
              <Button type="button" variant="outline" onClick={() => setFilter("all")}>{t("schedule.showAll")}</Button></div> : null}
            {listShown ? <ul className="schedule-tasks">{sectionRows("briefings", t("schedule.sections.briefings"), [briefingRow("morning"), briefingRow("evening"), ...otherBriefings.map(taskRow)])}{sectionRows("checks", t("schedule.checks"), checks.map(taskRow))}{sectionRows("research", t("schedule.kind.custom"), research.map(taskRow))}{sectionRows("learning", t("schedule.kind.learning"), learning.map(taskRow))}</ul> : null}</>}
        </section>
        {ordered.length && onAskHome ? <div className="schedule-footer"><Button variant="link" className="schedule-task-ask" data-schedule-footer-ask onClick={() => onAskHome("")}>{t("schedule.askBot")}</Button></div> : null}
      </div>
      {readerOpen ? <div className="briefings-reader-scroll" onFocus={revealFocused}>{selected ? <article className="briefings-reader" ref={readerRef}><Button variant="ghost" className="briefings-back" data-print="omit" onClick={() => setSelected(null)}>{t("schedule.backTo", { title: focusTask ? displayTitle({ kind: focusTask.kind, title: focusTask.title }) : t("schedule.title") })}</Button><header><h2>{displayTitle(selected)}</h2><p className="briefings-meta">{formatDate(selected.startedAtMs)} · {formatTime(selected.startedAtMs)} · {loupist.name}</p></header>
        {selected.kind === "learning" ? <LearningRunNote bridge={bridge} loupistId={loupist.id} run={selected} /> : null}
        {selected.briefingContext?.changed ? <BriefingSections bridge={bridge} loupistId={loupist.id} context={selected.briefingContext} nextRun={briefingTask ? nextRun(briefingTask) : null} skipped={skipped.ordinary} skippedSavedKeys={skippedSavedKeys}
          onOpenMap={onOpenMap} onOpenCheck={(journalId, title) => void open(selected, { kind: "memory", location: { loupistId: loupist.id, surface: "note", objectId: journalId, selection: { kind: "journal", journalId } }, title })} /> : null}
        {readerLoading ? <p role="status">{t("loading")}</p> : readerError ? <div role="alert" className="briefings-failure"><h3>{t(`failure.title.${runKind(selected.kind)}`)}</h3>{failureReason(failure) ? <p>{failureReason(failure)}</p> : null}{failure === "auth" && onOpenModelSettings ? <Button variant="link" onClick={onOpenModelSettings}>{t("failure.models")}</Button> : null}<Disclosure className="briefings-failure-details" title={t("failure.details")}><pre>{readerError}</pre></Disclosure>
          {runFailed ? <Button data-briefing-run-again disabled={rerun === "starting" || taskRunning} title={t("schedule.runNowCost")} onClick={() => void runAgain(selected)}>{rerun === "starting" || taskRunning ? <Spinner /> : null}{taskRunning ? t("schedule.runningNow") : t("schedule.runNow")}</Button>
            : <Button onClick={() => setReaderRevision(value => value + 1)}>{t("retry")}</Button>}
          {rerun === "failed" ? <p className="inline-error" role="alert">{t("schedule.runNowFailed")}</p> : null}
          <Dialog open={runRefusal !== null} onOpenChange={open => { if (!open && rerun !== "starting") setRunRefusal(null); }}>
            <DialogContent size="sm" className="dialog-form" showCloseButton={rerun !== "starting"}>
              <DialogHeader><DialogTitle>{t("schedule.runNow")}</DialogTitle>{runRefusal ? <DialogDescription>{runRefusal}</DialogDescription> : null}</DialogHeader>
              <DialogBody />
              <DialogFooter>
                <Button variant="outline" autoFocus disabled={rerun === "starting"} onClick={() => setRunRefusal(null)}>{t("schedule.cancel")}</Button>
                <Button data-schedule-run-confirm disabled={rerun === "starting"} onClick={() => { if (selected) void runAgain(selected, true); }}>{rerun === "starting" ? <Spinner /> : null}{t("schedule.runNow")}</Button>
              </DialogFooter>
            </DialogContent>
          </Dialog></div> : message ? <ChatMapProvider bridge={bridge} loupistId={loupist.id} chatId={selected.threadId ?? ""} messages={[message]} onOpen={() => onOpenMap ? onOpenMap() : continueInChat(selected)}><RunLinks run={selected} open={open}><ChatExecutionProvider value={executionScope}><ChatMessageCard showIdentity={false} message={message} loupistId={loupist.id} chatId={selected.threadId ?? undefined} loupistName={loupist.name} workspacePath={loupist.workspace_path} onReadAttachment={async (messageId, attachmentId) => (await bridge.chatAttachmentRead(loupist.id, selected.threadId!, messageId, attachmentId)).data_url} /></ChatExecutionProvider></RunLinks></ChatMapProvider> : <>
          <p>{selected.status === "running" ? t(`preparingBody.${runKind(selected.kind)}`) : selected.summary}</p>
          <MemorySaveWarning reason={selected.memorySaveSkip} items={selected.discardedMemoryItems} />
        </>}
        {!selected.briefingContext?.changed && skippedSavedKeys ? <p className="briefings-meta">{skippedSavedKeys}</p> : null}
        {(selected.briefingContext || skipped.ordinary.length > 0) && <Disclosure title={t("checked")}>
          {selected.briefingContext && <><p>{t("companies", { symbols: selected.briefingContext.symbols.join(", ") || t("noCompanies") })}</p><p>{t("principles", { count: selected.briefingContext.principlesChecked })}</p><p>{t("facts", { count: selected.briefingContext.newFacts })}</p></>}
          {skipped.ordinary.length > 0 && <p>{t("skipped", { names: skipped.ordinary.join(", ") })}</p>}
        </Disclosure>}
        <div className="briefings-actions" data-print="omit">
          {sessionGone ? <p className="briefings-meta" role="status">{t("schedule.sessionRemoved")}</p>
            : selected.threadId && !readerLoading && <Button variant="ghost" className="briefings-continue" onClick={() => continueInChat(selected)}>{t(continuedSession === selected.threadId ? "schedule.openSession" : "continue")}</Button>}
          {message && !readerLoading && !readerError ? <Button variant="ghost" data-briefing-print onClick={() => {
            const reader = readerRef.current;
            if (!reader) return;
            setPrintFailed(false);
            void printReader(reader, `${displayTitle(selected)} · ${formatDate(selected.startedAtMs)}`).catch(() => setPrintFailed(true));
          }}><PrinterIcon aria-hidden="true" />{t("print")}</Button> : null}
          {printFailed ? <span className="briefings-print-error" role="alert">{t("printFailed")}</span> : null}
        </div>
        <p className="briefings-disclosure">{t("disclosure")}</p>
      </article> : focusTask ? <article className="briefings-reader schedule-task-detail" aria-label={displayTitle(focusTask)}>
        <Button variant="ghost" className="briefings-back" onClick={() => { setFocusExplicit(false); setSelected(null); }}>{t("schedule.back")}</Button>
        <header className="schedule-detail-header"><div><div className="schedule-detail-title"><h2>{displayTitle(focusTask)}</h2>{focusTask.kind === "briefing" && builtIn}</div>
          <p className="briefings-meta schedule-detail-status"><strong>{taskStatus(focusTask)}</strong>{[t(taskKindKey(focusTask)) === displayTitle(focusTask) ? null : t(taskKindKey(focusTask)), cadence(focusTask), nextRun(focusTask)].filter(Boolean).map(part => ` · ${part}`).join("")}</p></div>
          </header>
        {focusTask.kind === "learning" ? <LearningProgram task={focusTask} when={cadence(focusTask)} names={assetNames} onOpenAsset={go ? (assetId, name) => go({ kind: "asset", target: { loupistId: loupist.id, assetId, name } }) : undefined} anotherRunning={learningRunning} /> : holdSentence(focusTask.hold?.reason) ? <p className="briefings-meta" data-hold={focusTask.hold?.reason}>{holdSentence(focusTask.hold?.reason)}</p> : null}
        {focusTask.kind === "learning" ? <LearningProgramActions key={focusTask.id} bridge={bridge} task={focusTask} names={assetNames} onChanged={() => { void load(); void loadHistory(focusTask.id); }} /> : null}
        {focusTask.model_selection && <p className="briefings-meta">{t("schedule.model", { model: focusTask.model_selection.model?.name ?? i18n.t(focusTask.model_selection.unavailable_reason === "no_connected_account" ? "chat:chatModelMenu.noAccount" : "chat:chatPendingQueue.unavailableModel") })}</p>}
        {focusTask.last_error === "nothing_left_to_watch" && <p className="briefings-meta" role="status">{t("schedule.emptyWatch")}</p>}
        {focusTask.latest_run && ["failed", "interrupted"].includes(focusTask.latest_run.status) && !stoppedForBudget(focusTask.latest_run.budget?.outcome) ? <p className="inline-error schedule-task-error" role="status">{lastFailure(focusTask)}{briefingFailure(focusTask.latest_run.error ?? focusTask.last_error) === "auth" && onOpenModelSettings ? <> <Button variant="link" onClick={onOpenModelSettings}>{t("failure.models")}</Button></> : null}</p> : null}
        {focusTask.trigger && !focusTask.replaces && <TriggerCard trigger={focusTask.trigger} bridge={bridge} loupistId={loupist.id} taskId={focusTask.id} revision={focusTask.revision} schedule={focusTask.schedule} prompt={focusTask.prompt} preset={focusTask.preset} onChanged={() => { void load(); }} onChangeDraft={onAskHome} />}
        <ScheduleTaskControls key={`${focusTask.id}:${focusTask.revision}`} bridge={bridge} task={focusTask} title={displayTitle(focusTask)} running={focusTask.latest_run?.status === "running"} onAskHome={onAskHome}
          onChanged={() => { void load(); void loadHistory(focusTask.id); }} onDeleted={() => { setFocusExplicit(false); setFocusTaskId(null); void load(); }} />
        {/* A prompt that only repeats the title says nothing new; any other prompt is labelled as the request. */}
        {focusTask.kind === "custom" && !(focusTask.trigger && !focusTask.replaces) && focusTask.prompt?.trim() && ![focusTask.title.trim(), displayTitle(focusTask).trim()].includes(focusTask.prompt.trim())
          ? <><h3 className="schedule-section-label">{t("schedule.prompt")}</h3><p className="schedule-task-prompt">{focusTask.prompt}</p></> : null}
        {focusTask.trigger && <><h3 className="schedule-section-label">{t("schedule.checkHistory")}</h3>
          {evaluations?.length ? <ol className="schedule-history" aria-label={t("schedule.checkHistory")}>
            {evaluations.slice(0, HISTORY_ROWS).map((evaluation, index) => <li key={`${evaluation.at_ms}:${index}`} className="schedule-evaluation" data-trigger-last={index === 0 || undefined}>
              <span className="briefings-item-heading">
                <strong>{t(`schedule.checkOutcome.${evaluation.outcome}`)}</strong>
                <time dateTime={new Date(evaluation.at_ms).toISOString()}>{formatDate(evaluation.at_ms)} · {formatTime(evaluation.at_ms)}</time>
              </span>
              {evaluation.reading || evaluation.error && evaluation.outcome === "error"
                ? <span className="briefings-summary">{evaluation.reading || evaluation.error}</span> : null}
            </li>)}
          </ol> : evaluations ? <p className="briefings-meta">{t("schedule.noChecks")}</p>
            : !error && <p role="status" className="briefings-meta">{t("loading")}</p>}
        </>}
        {(!focusTask.trigger || runsNewestFirst(history[focusTask.id] ?? []).length > 0) && <>
          <h3 className="schedule-section-label">{t("schedule.runs")}</h3>
          {runsNewestFirst(history[focusTask.id] ?? []).length ? <ol className="schedule-history" aria-label={t("schedule.history", { title: displayTitle(focusTask) })}>{runsNewestFirst(history[focusTask.id] ?? []).slice(0, HISTORY_ROWS).map(runRow)}</ol>
            : <p className="briefings-meta">{t("schedule.noRuns")}</p>}
        </>}
      </article> : null}</div> : null}
    </div>}
  </section>;
}

/**
 * The built-in briefing while it is off: its name, what it does, then Off beside Turn on at the row's
 * end, centred together on the row. A briefing that was turned off keeps its task, so its name still
 * opens the detail with its run history.
 */
function BriefingOffRow({ title, builtIn, enabling, enableError, current, onOpen, onTurnOn, description, marker }: {
  title: string; builtIn: React.ReactNode; enabling: boolean; enableError: boolean; current: boolean; onOpen?: () => void; onTurnOn: () => void;
  description: string; marker: "morning" | "evening";
}) {
  const { t } = useTranslation("briefings");
  const summary = <>
    <span className="schedule-task-heading"><strong title={title}>{title}</strong>{builtIn}</span>
    <span className="briefings-meta">{description}</span>
  </>;
  return <li className="schedule-task" data-schedule-briefing={marker === "morning" ? "off" : undefined} data-schedule-evening={marker === "evening" ? "off" : undefined} data-status="off" data-current={current || undefined}>
    <div className="schedule-task-row">
      {onOpen ? <button type="button" className="schedule-task-summary" aria-current={current || undefined} onClick={onOpen}>{summary}</button>
        : <div className="schedule-task-summary">{summary}</div>}
      <div className="schedule-task-off">
        <span className="schedule-task-status">{t("off")}</span>
        <Button type="button" size="sm" className="schedule-task-turn-on" data-schedule-morning={marker === "morning" || undefined} data-schedule-evening={marker === "evening" || undefined} disabled={enabling} onClick={onTurnOn}>{t("schedule.turnOn")}</Button>
      </div>
    </div>
    {enableError ? <p role="alert" className="inline-error schedule-task-error">{t("schedule.statusFailed")}</p> : null}
  </li>;
}

/** Sections the host computed when the briefing ran; the narrative follows unparsed. */
function BriefingSections({ bridge, loupistId, context, nextRun, skipped, skippedSavedKeys, onOpenMap, onOpenCheck }: {
  bridge: LoupistBridge; loupistId: string; context: BriefingContext; nextRun: string | null; skipped: string[]; skippedSavedKeys: string | null;
  onOpenMap?: (disputedEntityIds?: string[]) => void; onOpenCheck: (journalId: string, title: string) => void;
}) {
  const { t, i18n } = useTranslation("briefings");
  const { t: tKnowledge } = useTranslation("knowledge");
  // Stored summaries are English; rows speak the interface language through the Map's names and relations.
  const [map, setMap] = useState<KnowledgeMap | null>(null);
  // A followed asset the briefing names by its ticker reads as its saved name; the list is read again on `assets-changed`.
  const [names, setNames] = useState<Record<string, string>>({});
  const [followedRevision, setFollowedRevision] = useState(0);
  useEffect(() => bridge.onAssetsChanged?.(() => setFollowedRevision(value => value + 1)), [bridge]);
  useEffect(() => {
    let live = true;
    void readKnowledgeMap(bridge, loupistId).then(next => { if (live) setMap(next); }).catch(() => {});
    void Promise.resolve().then(() => bridge.assetsList({ loupistId })).then(rows => {
      if (live) setNames(Object.fromEntries(rows.flatMap(row => row.reference?.ticker?.trim() ? [[row.reference.ticker.trim(), row.name]] : [])));
    }).catch(() => {});
    return () => { live = false; };
  }, [bridge, loupistId, followedRevision]);
  const rowText = (row: NonNullable<BriefingContext["changed"]>["rows"][number]) => {
    if (row.kind === "source_retained" || !row.subjectId) return row.summary;
    const event: GrowthEvent = { eventId: `${row.kind}:${row.subjectId}`, loupistId, atMs: 0, kind: row.kind as GrowthEvent["kind"], subjectId: row.subjectId, recordId: null, originChatId: null, originMessageId: null, actor: "loupist", summary: row.summary, beforeSummary: null, afterSummary: null };
    const link = whatsNewRows([event], map, i18n.language)[0]?.link;
    return link ? whatsNewLinkText(link, tKnowledge) : row.summary;
  };
  const changed = context.changed!;
  const quiet = !changed.names && !changed.relations && !changed.conflicts;
  const ahead = context.ahead ?? [];
  return <div className="briefing-sections">
    <section aria-labelledby="briefing-changed"><h3 id="briefing-changed" className="schedule-section-label">{t("sections.changed")}</h3>
      {quiet ? <p>{t("changed.none")}</p> : <>
        <p className="gg-num">{t("changed.totals", {
          names: t("changed.names", { count: changed.names, formattedCount: formatNumber(changed.names) }),
          relations: t("changed.relations", { count: changed.relations, formattedCount: formatNumber(changed.relations) }),
          conflicts: t("changed.conflicts", { count: changed.conflicts, formattedCount: formatNumber(changed.conflicts) }),
        })}</p>
        {changed.rows.length ? <ul className="briefing-changes">{changed.rows.map((row, index) => <li key={`${row.kind}:${row.subjectId ?? index}`}>
          <span>{rowText(row)}</span> <small>{t(row.kind === "claim_disputed" ? "changed.conflict" : row.kind === "source_retained" ? "changed.source" : "changed.new")}</small>
          {row.kind === "claim_disputed" && context.disputedEntityIds.length && onOpenMap ? <> <Button variant="link" className="briefing-row-link" onClick={() => onOpenMap(context.disputedEntityIds)}>{t("changed.seeOnMap")}</Button></> : null}
        </li>)}</ul> : null}
        {changed.conflicts && context.disputedEntityIds.length && onOpenMap ? <Button variant="link" className="briefings-conflicts" data-briefings-conflicts onClick={() => onOpenMap(context.disputedEntityIds)}>{t("conflictingEvidence", { count: context.disputedEntityIds.length })}</Button> : null}
      </>}
    </section>
    {context.symbols.length ? <section aria-labelledby="briefing-companies"><h3 id="briefing-companies" className="schedule-section-label">{t("sections.companies")}</h3>
      <p>{context.symbols.map(symbol => names[symbol.split(":")[0]!] ?? symbol).join(" · ")}</p>
    </section> : null}
    {ahead.length || nextRun || skipped.length || skippedSavedKeys ? <section aria-labelledby="briefing-ahead"><h3 id="briefing-ahead" className="schedule-section-label">{t("sections.ahead")}</h3>
      <ul className="briefing-ahead">
        {ahead.map(check => <li key={check.journalId}><Button variant="link" onClick={() => onOpenCheck(check.journalId, check.title)}>{check.byDate ? t("ahead.checkDue", { date: formatDate(Date.parse(`${check.byDate}T00:00:00`), { month: "short", day: "numeric", year: undefined }), title: check.title }) : check.title}</Button></li>)}
        {nextRun ? <li>{nextRun}</li> : null}
        {skipped.length ? <li>{t("ahead.skipped", { names: skipped.join(", ") })}</li> : null}
        {skippedSavedKeys ? <li>{skippedSavedKeys}</li> : null}
      </ul>
    </section> : null}
    {context.backgroundRuns?.length ? <section aria-labelledby="briefing-background"><h3 id="briefing-background" className="schedule-section-label">{t("sections.background")}</h3>
      <ul className="briefing-ahead">{context.backgroundRuns.map(run => <li key={run.runId}>{run.task}</li>)}</ul>
    </section> : null}
  </div>;
}

function SubjectChips({ subjects, names, onOpenAsset, limit = 3 }: {
  subjects?: ScheduledTask["subjects"];
  names: ReadonlyMap<string, string>;
  onOpenAsset?: (assetId: string, name: string) => void;
  limit?: number;
}) {
  const { t } = useTranslation("briefings");
  const { shown, extra } = subjectChipModel(subjects, names, limit);
  if (!shown.length) return null;
  return <ul className="schedule-subject-chips">
    {shown.map(chip => <li key={chip.key}>{chip.assetId && onOpenAsset
      ? <button type="button" className="schedule-subject-chip" onClick={() => onOpenAsset(chip.assetId!, chip.label)}>{chip.label}</button>
      : <span className="schedule-subject-chip">{chip.label}</span>}</li>)}
    {extra > 0 ? <li className="schedule-subject-chip" data-subject-more>{t("schedule.learning.moreSubjects", { formattedCount: formatNumber(extra) })}</li> : null}
  </ul>;
}

function LearningProgram({ task, when, names, onOpenAsset, anotherRunning }: {
  task: ScheduledTaskListItem;
  when: string;
  names: ReadonlyMap<string, string>;
  onOpenAsset?: (assetId: string, name: string) => void;
  anotherRunning: boolean;
}) {
  const { t } = useTranslation("briefings");
  const [expanded, setExpanded] = useState(false);
  const brief = task.prompt?.trim() ?? "";
  const long = brief.split("\n").length > 6 || brief.length > 420;
  const outcome = learningRowOutcome(task, anotherRunning);
  const depth = task.program ? learningDepthText(task.program.depth) : null;
  const sources = task.program ? learningScopeText(task.program.scope) : null;
  const keeps = task.program ? learningKeepsText(task.program.keeps) : null;
  const budget = learningBudgetText(task.budget);
  const preset = learningPresetText(task.program?.preset);
  const fields: Array<[string, string]> = [
    ...(preset ? [[t("schedule.learning.preset.label"), preset] as [string, string]] : []),
    [t("schedule.learning.when"), when],
    ...(depth ? [[t("schedule.learning.depthLabel"), depth] as [string, string]] : []),
    ...(sources ? [[t("schedule.learning.sources"), sources] as [string, string]] : []),
    ...(keeps ? [[t("schedule.learning.keeps"), keeps] as [string, string]] : []),
    [t("schedule.learning.reports"), learningReportText(task.report)],
    [t("schedule.learning.delivers"), learningDeliveryText(task.delivery?.to)],
    ...(budget ? [[t("schedule.learning.budgetLabel"), budget] as [string, string]] : []),
  ];
  return <div className="schedule-learning-program">
    {outcome ? <p className="briefings-meta" data-learning-outcome>{outcome}</p> : null}
    {brief ? <section aria-label={t("schedule.learning.what")}>
      <h3 className="schedule-section-label">{t("schedule.learning.what")}</h3>
      <p className="schedule-learn-brief" data-expanded={expanded || undefined}>{brief}</p>
      {long && !expanded ? <Button variant="link" size="sm" onClick={() => setExpanded(true)}>{t("schedule.showAll")}</Button> : null}
    </section> : null}
    {task.subjects?.length ? <section aria-label={t("schedule.learning.about")}>
      <h3 className="schedule-section-label">{t("schedule.learning.about")}</h3>
      <SubjectChips subjects={task.subjects} names={names} onOpenAsset={onOpenAsset} limit={task.subjects.length} />
    </section> : null}
    <dl className="schedule-learning-fields">{fields.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>
  </div>;
}

function LearningRunNote({ bridge, loupistId, run }: { bridge: LoupistBridge; loupistId: string; run: ReadableRun }) {
  const { t } = useTranslation("briefings");
  const [retry, setRetry] = useState(0);
  const [undo, setUndo] = useState(run.undo ?? null);
  const [undoing, setUndoing] = useState(false);
  const [undoError, setUndoError] = useState(false);
  const [digest, setDigest] = useState<LearnedDigest[] | "loading" | "error">(run.messageId ? "loading" : []);
  useEffect(() => { setUndo(run.undo ?? null); }, [run.undo]);
  useEffect(() => {
    if (!run.messageId) { setDigest([]); return; }
    let live = true;
    setDigest("loading");
    void bridge.memoryLearned({ loupistId, messageIds: [run.messageId] }).then(
      rows => { if (live) setDigest(rows); },
      () => { if (live) setDigest("error"); },
    );
    return () => { live = false; };
  }, [bridge, loupistId, run.messageId, retry]);
  const usage = runUsageText(run);
  const undoRun = async () => {
    if (!run.messageId || undoing) return;
    setUndoing(true); setUndoError(false);
    try {
      const result = await bridge.memoryMessageUndo({ loupistId, messageId: run.messageId });
      setUndo(current => result.undone === 0 && result.kept === 0 && current ? current : result);
      setRetry(value => value + 1);
    } catch { setUndoError(true); }
    finally { setUndoing(false); }
  };
  const pages = Array.isArray(digest) ? digest.find(item => item.messageId === run.messageId)?.pages ?? [] : [];
  const things = learnedThingCount(pages);
  return <div className="schedule-learning-run">
    {usage ? <p className="briefings-meta" data-learning-usage>{usage}</p> : null}
    {digest === "loading" ? <p role="status">{t("loading")}</p> : null}
    {digest === "error" ? <p role="alert">{t("loadError")} <Button variant="ghost" size="sm" onClick={() => setRetry(value => value + 1)}>{t("retry")}</Button></p> : null}
    {undo ? <p role="status" data-learning-undo>{runUndoText(undo.kept)}</p> : run.messageId ? <div className="briefings-actions">
      <Button variant="outline" size="sm" disabled={undoing} onClick={() => void undoRun()}>{undoing ? <Spinner /> : null}{t("schedule.learning.undo")}</Button>
      {undoError ? <span role="alert">{t("loadError")} <Button variant="ghost" size="sm" onClick={() => void undoRun()}>{t("retry")}</Button></span> : null}
    </div> : null}
    {things > 0 ? <>
      <p>{t("schedule.learning.learned", { count: things, formattedCount: formatNumber(things) })}</p>
      <ul className="schedule-learned">{pages.map((page, index) => {
        const outcome = learnedOutcomeText(page.outcomes);
        return <li key={`${page.recordId}:${index}`}><span>{page.title}</span>{outcome ? <span className="briefings-meta"> · {outcome}</span> : null}</li>;
      })}</ul>
    </> : null}
  </div>;
}
