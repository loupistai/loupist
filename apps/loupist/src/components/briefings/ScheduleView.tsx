import { whatsNewLinkText, whatsNewRows } from "../map/whatsNew";
import { readKnowledgeMap } from "../../knowledge/mapRead";
import { briefingFailure } from "./briefingFailure";
import { ChatMapProvider } from "../workspace/ChatMapContext";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "@/i18n";
import { formatDate, formatNumber, formatTime } from "@/i18n/format";
import type { GrowthEvent, KnowledgeMap } from "../../contracts/knowledge";
import type { BriefingContext, BriefingSettings, ChatMessage, LoupistSummary, LoupistBridge, InboxItem, ScheduledRun, ScheduledTask, ScheduledTaskListItem, ModelCatalog } from "@/types";
import { Button } from "../ui/button";
import { Spinner } from "../ui/spinner";
import { Badge } from "../ui/badge";
import { ChevronRightIcon } from "lucide-react";
import { SegmentedControl, SegmentedControlItem } from "../ui/segmented-control";
import { LoginItemHint } from "./LoginItemHint";
import { ScheduleTaskControls } from "./ScheduleTaskControls";
import { Disclosure } from "../ui/disclosure";
import { ChatMessageCard } from "../chat/ChatMessageCard";
import { MemorySaveWarning } from "../chat/ChatReceipt";
import { ChatExecutionProvider } from "../chat/ChatExecutionContext";
import { cadenceLabel, cadencePhrase, friendlyTimeZone, relativeRun, scheduledTaskTitle, skippedSavedKeyLabel, skippedToolNames } from "./scheduledPresentation";
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
};

export const inboxItemOf = (run: ScheduledRun): ReadableRun => ({
  memorySaveSkip: run.memory_save_skip,
  discardedMemoryItems: run.discarded_memory_items,
  briefingContext: run.briefing_context ?? null, error: run.error ?? null,
  runId: run.run_id, taskId: run.task_id, kind: run.kind, title: run.task_title,
  status: run.status, summary: run.summary, startedAtMs: run.started_at_ms,
  finishedAtMs: run.finished_at_ms ?? null, readAtMs: run.read_at_ms, threadId: run.thread_id ?? null,
  messageId: run.message_id, skippedTools: run.skipped_tools,
});

const HISTORY_ROWS = 10;

/** Active shows tasks that are on, waiting for confirmation, or running. */
function taskListed(task: ScheduledTaskListItem, filter: "active" | "all") {
  return filter === "all" || task.status === "active" || Boolean(task.awaiting_confirmation) || task.latest_run?.status === "running";
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
  useEffect(() => { if (focusTaskId) void loadHistory(focusTaskId); }, [focusTaskId, loadHistory, tasks]);
  const continueInChat = (item: InboxItem) => void onOpenChat(item).then(opened => {
    if (opened) return;
    setSessionGone(true); void load(); if (focusTaskId) void loadHistory(focusTaskId);
  }).catch(() => undefined);
  // A run left open while it runs (Run now): the refreshed task list and history carry its
  // result, so the reader takes the new snapshot and loads the answer.
  useEffect(() => {
    if (!selected) return;
    // One list may be a read behind the other; a settled copy wins over a running one.
    const copies = [...tasks.flatMap(task => task.latest_run ? [task.latest_run] : []), ...Object.values(history).flat()]
      .filter(run => run.run_id === selected.runId);
    const fresh = copies.find(run => run.status !== "running") ?? copies[0];
    if (!fresh || (fresh.status === selected.status && fresh.message_id === selected.messageId && fresh.memory_save_skip === selected.memorySaveSkip &&
      fresh.discarded_memory_items?.length === selected.discardedMemoryItems?.length)) return;
    const next = inboxItemOf(fresh);
    setSelected(current => current?.runId === next.runId ? { ...next, readAtMs: current.readAtMs ?? next.readAtMs } : current);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- reconciles on each refresh, keyed on the open run's id and state.
  }, [tasks, history, selected?.runId, selected?.status, selected?.messageId, selected?.memorySaveSkip, selected?.discardedMemoryItems?.length]);

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
  const displayTitle = (item: { kind: string; title: string }) => scheduledTaskTitle(item.kind, item.title);
  /** A run is worded by its task's kind: a briefing, or scheduled research for any other task. */
  const runKind = (kind: string) => kind === "briefing" ? "briefing" : "custom";
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
  const ordered = [...tasks].sort((a, b) => taskRank(a) - taskRank(b) || (a.next_run_at_ms ?? Number.MAX_SAFE_INTEGER) - (b.next_run_at_ms ?? Number.MAX_SAFE_INTEGER) || displayTitle(a).localeCompare(displayTitle(b)));
  const visible = ordered.filter(listed);
  const focusTask = !focusExplicit ? null : ordered.find(task => task.id === focusTaskId) ?? null;
  const readerOpen = Boolean(selected) || Boolean(focusTask);
  // One time zone for every timed task is named once in the header instead of on each row.
  const zones = new Set(ordered.flatMap(task => task.schedule.kind === "daily" || task.schedule.kind === "weekly" ? [task.schedule.timezone] : []));
  const sharedZone = zones.size === 1 ? [...zones][0]! : null;
  const cadence = (task: ScheduledTask) => sharedZone ? cadencePhrase(task.schedule) : cadenceLabel(task.schedule);
  const openTask = (task: ScheduledTask) => { setSelected(null); setFocusTaskId(task.id); setFocusExplicit(true); };
  const runFailed = selected?.status === "failed" || selected?.status === "interrupted";
  const taskRunning = Boolean(selected && tasks.find(task => task.id === selected.taskId)?.latest_run?.status === "running");
  /** Runs the failed run's task once more and shows that task, where the new run appears as it runs. */
  const runAgain = async (run: InboxItem) => {
    setRerun("starting");
    try {
      await bridge.scheduledTaskRunNow({ loupist_id: loupist.id, task_id: run.taskId });
      setRerun(null); setSelected(null); setFocusTaskId(run.taskId); setFocusExplicit(true);
      void load(); void loadHistory(run.taskId);
    } catch { setRerun("failed"); }
  };

  const enableBriefing = async () => {
    setEnabling(true);
    setEnableError(false);
    try {
      await bridge.briefingSettingsSet({ loupistId: loupist.id, enabled: true });
      await load();
    } catch {
      setEnableError(true);
    } finally {
      setEnabling(false);
    }
  };
  const runRow = (run: ScheduledRun) => {
    const failed = run.status === "failed" || run.status === "interrupted";
    // A check that found nothing to report posted nothing, so it never waits to be read.
    const quiet = run.quiet && run.status === "succeeded";
    const unread = run.read_at_ms === null && run.status !== "running" && !quiet;
    return <li key={run.run_id}><button type="button" className="briefings-item" data-schedule-run={run.run_id} aria-current={selected?.runId === run.run_id || undefined} data-unread={unread} data-quiet={quiet || undefined} onClick={() => { setSelected(inboxItemOf(run)); setFocusExplicit(true); }}>
      <span className="briefings-item-heading">{unread && <><span className="briefings-unread" aria-hidden="true" /><span className="sr-only">{t("unread")}</span></>}<strong>{formatDate(run.started_at_ms, { month: "short", day: "numeric", year: undefined })}</strong><time>{formatTime(run.started_at_ms)}</time>{quiet ? <span className="schedule-run-quiet">{t("schedule.quiet")}</span> : null}</span>
      <span className="briefings-summary" data-failed={failed}>{run.status === "running" ? <><Spinner />{t(`preparingBody.${runKind(run.kind)}`)}</> : failed ? t(`failure.title.${runKind(run.kind)}`) : run.summary}</span>
    </button></li>;
  };
  const headerMeta = [
    visible.length ? t("schedule.summary", { count: visible.length, formattedCount: formatNumber(visible.length) }) : null,
    running.length ? t("schedule.runningCount", { count: running.length, formattedCount: formatNumber(running.length) }) : null,
    sharedZone ? friendlyTimeZone(sharedZone, i18n.language) : null,
  ].filter(Boolean).join(" · ");
  const rowState = (task: ScheduledTaskListItem) => task.awaiting_confirmation || task.status !== "active" || waiting(task) ? taskStatus(task) : null;
  const introductory = !ordered.length && !readerOpen;
  const spending = <p className="briefings-meta schedule-spending" data-schedule-spending>{t("schedule.spending")}</p>;
  const builtIn = <Badge variant="outline">{t("schedule.builtIn")}</Badge>;
  const skipped = skippedToolNames(selected?.skippedTools ?? []);
  const skippedSavedKeys = skippedSavedKeyLabel(skipped.savedKeys);

  return <section className="briefings-view schedule-view" aria-label={t("schedule.title")}>
    {ordered.length ? <header className="schedule-header">
      {headerMeta ? <p className="briefings-meta">{headerMeta}</p> : null}
      {!readerOpen && spending}
    </header> : null}
    {error && <p role="alert" className="briefings-error">{t("loadError")} <Button variant="ghost" onClick={() => void load()}>{t("retry")}</Button></p>}
    {loading ? <p role="status">{t("loading")}</p> : <div className="briefings-layout" data-run-open={Boolean(selected) || undefined} data-intro={introductory || undefined} data-reader-open={readerOpen || undefined} data-empty={(!visible.length && !selected) || undefined}>
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
          {visible.length ? <h2 className="schedule-section-label">{t("schedule.tasks")}</h2> : null}
          {introductory ? <div className="schedule-empty">
            <h2>{t("schedule.emptyTitle")}</h2>
            <p>{t("schedule.emptyBody")}</p>
            <p className="briefings-meta">{t("schedule.locationRule")}</p>
            <div className="schedule-options">
              <div className="start-option-card schedule-option">
                <h3>{t("morning")}</h3>
                <p>{t("schedule.briefingDescription")}</p>
                <Button type="button" data-schedule-morning disabled={enabling} onClick={() => void enableBriefing()}>{t("schedule.turnOn")}</Button>
                {enableError && <p role="alert" className="inline-error">{t("schedule.statusFailed")}</p>}
              </div>
              <div className="start-option-card schedule-option">
                <h3>{t("schedule.ownTask")}</h3>
                <p>{t("schedule.ownDescription", { example: t("schedule.ownExample") })}</p>
                <Button type="button" variant="outline" disabled={!onAskHome} onClick={() => onAskHome?.(t("schedule.ownExample"))}>{t("schedule.askBot")}</Button>
              </div>
            </div>
            {spending}
          </div>
            : !visible.length ? <div className="briefings-empty"><h3>{t("schedule.noActiveTitle")}</h3><p>{t("schedule.noActiveBody")}</p>
              <Button type="button" variant="outline" onClick={() => setFilter("all")}>{t("schedule.showAll")}</Button></div>
            : <ul className="schedule-tasks">{visible.map(task => {
            const next = nextRun(task);
            const state = rowState(task);
            const current = focusTask?.id === task.id;
            return <li key={task.id} className="schedule-task" data-schedule-task={task.id} data-status={task.awaiting_confirmation ? "awaiting" : task.status} data-current={current || undefined}>
              <div className="schedule-task-row">
                <button type="button" className="schedule-task-summary" aria-current={current || undefined} onClick={() => openTask(task)}>
                  <span className="schedule-task-heading"><strong title={displayTitle(task)}>{displayTitle(task)}</strong>{task.kind === "briefing" && builtIn}{state ? <span className="schedule-task-status">{state}</span> : null}</span>
                  <span className="briefings-meta">{cadence(task)}{next ? ` · ${next}` : ""}</span>
                </button>
                <ChevronRightIcon className="schedule-task-chevron" aria-hidden="true" />
              </div>
            </li>;
          })}</ul>}
        </section>
        {/* The footer says to tell Bot, so it leads there: Bot opens with its composer focused. */}
        {ordered.length ? <p className="briefings-meta schedule-footer">{t("schedule.locationRule")} {t("schedule.footer")}{onAskHome ? <> <Button variant="link" className="schedule-task-ask" data-schedule-footer-ask onClick={() => onAskHome("")}>{t("schedule.changeAsk")}</Button></> : null}</p> : null}
      </div>
      {readerOpen ? <div className="briefings-reader-scroll" onFocus={revealFocused}>{selected ? <article className="briefings-reader" ref={readerRef}><Button variant="ghost" className="briefings-back" data-print="omit" onClick={() => setSelected(null)}>{t("schedule.backTo", { title: focusTask ? displayTitle({ kind: focusTask.kind, title: focusTask.title }) : t("schedule.title") })}</Button><header><h2>{displayTitle(selected)}</h2><p className="briefings-meta">{formatDate(selected.startedAtMs)} · {formatTime(selected.startedAtMs)} · {loupist.name}</p></header>
        {selected.briefingContext?.changed ? <BriefingSections bridge={bridge} loupistId={loupist.id} context={selected.briefingContext} nextRun={briefingTask ? nextRun(briefingTask) : null} skipped={skipped.ordinary} skippedSavedKeys={skippedSavedKeys}
          onOpenMap={onOpenMap} onOpenCheck={(journalId, title) => void open(selected, { kind: "memory", location: { loupistId: loupist.id, surface: "note", objectId: journalId, selection: { kind: "journal", journalId } }, title })} /> : null}
        {readerLoading ? <p role="status">{t("loading")}</p> : readerError ? <div role="alert" className="briefings-failure"><h3>{t(`failure.title.${runKind(selected.kind)}`)}</h3>{failureReason(failure) ? <p>{failureReason(failure)}</p> : null}{failure === "auth" && onOpenModelSettings ? <Button variant="link" onClick={onOpenModelSettings}>{t("failure.models")}</Button> : null}<Disclosure className="briefings-failure-details" title={t("failure.details")}><pre>{readerError}</pre></Disclosure>
          {runFailed ? <Button data-briefing-run-again disabled={rerun === "starting" || taskRunning} title={t("schedule.runNowCost")} onClick={() => void runAgain(selected)}>{rerun === "starting" || taskRunning ? <Spinner /> : null}{taskRunning ? t("schedule.runningNow") : t("schedule.runNow")}</Button>
            : <Button onClick={() => setReaderRevision(value => value + 1)}>{t("retry")}</Button>}
          {rerun === "failed" ? <p className="inline-error" role="alert">{t("schedule.runNowFailed")}</p> : null}</div> : message ? <ChatMapProvider bridge={bridge} loupistId={loupist.id} chatId={selected.threadId ?? ""} messages={[message]} onOpen={() => onOpenMap ? onOpenMap() : continueInChat(selected)}><RunLinks run={selected} open={open}><ChatExecutionProvider value={executionScope}><ChatMessageCard showIdentity={false} message={message} loupistId={loupist.id} chatId={selected.threadId ?? undefined} loupistName={loupist.name} workspacePath={loupist.workspace_path} onReadAttachment={async (messageId, attachmentId) => (await bridge.chatAttachmentRead(loupist.id, selected.threadId!, messageId, attachmentId)).data_url} /></ChatExecutionProvider></RunLinks></ChatMapProvider> : <>
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
            : selected.threadId && !readerLoading && <div><Button variant="ghost" className="briefings-continue" onClick={() => continueInChat(selected)}>{t(continuedSession === selected.threadId ? "schedule.openSession" : "continue")}</Button>
              {continuedSession !== selected.threadId && <p className="briefings-meta">{t("schedule.continueHelp")}</p>}</div>}
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
          <p className="briefings-meta schedule-detail-status"><strong>{taskStatus(focusTask)}</strong>{[t(`schedule.kind.${focusTask.kind}`) === displayTitle(focusTask) ? null : t(`schedule.kind.${focusTask.kind}`), cadence(focusTask), nextRun(focusTask)].filter(Boolean).map(part => ` · ${part}`).join("")}</p></div>
          </header>
        {focusTask.model_selection && <p className="briefings-meta">{t("schedule.model", { model: focusTask.model_selection.model?.name ?? i18n.t(focusTask.model_selection.unavailable_reason === "no_connected_account" ? "chat:chatModelMenu.noAccount" : "chat:chatPendingQueue.unavailableModel") })}</p>}
        {focusTask.latest_run && ["failed", "interrupted"].includes(focusTask.latest_run.status) ? <p className="inline-error schedule-task-error" role="status">{lastFailure(focusTask)}{briefingFailure(focusTask.latest_run.error ?? focusTask.last_error) === "auth" && onOpenModelSettings ? <> <Button variant="link" onClick={onOpenModelSettings}>{t("failure.models")}</Button></> : null}</p> : null}
        <ScheduleTaskControls key={`${focusTask.id}:${focusTask.revision}`} bridge={bridge} task={focusTask} title={displayTitle(focusTask)} running={focusTask.latest_run?.status === "running"} onAskHome={onAskHome}
          onChanged={() => { void load(); void loadHistory(focusTask.id); }} onDeleted={() => { setFocusExplicit(false); setFocusTaskId(null); void load(); }} />
        {/* A prompt that only repeats the title says nothing new; any other prompt is labelled as the request. */}
        {focusTask.kind === "custom" && focusTask.prompt?.trim() && ![focusTask.title.trim(), displayTitle(focusTask).trim()].includes(focusTask.prompt.trim())
          ? <><h3 className="schedule-section-label">{t("schedule.prompt")}</h3><p className="schedule-task-prompt">{focusTask.prompt}</p></> : null}
        <h3 className="schedule-section-label">{t("schedule.runs")}</h3>
        {(history[focusTask.id] ?? []).length ? <ol className="schedule-history" aria-label={t("schedule.history", { title: displayTitle(focusTask) })}>{(history[focusTask.id] ?? []).slice(0, HISTORY_ROWS).map(runRow)}</ol>
          : <p className="briefings-meta">{t("schedule.noRuns")}</p>}
      </article> : null}</div> : null}
    </div>}
  </section>;
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
  </div>;
}
