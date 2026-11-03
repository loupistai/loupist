import { whatsNewRows } from "../map/whatsNew";
import { briefingFailure } from "./briefingFailure";
import { ChatMapProvider } from "../workspace/ChatMapContext";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "@/i18n";
import { formatDate, formatNumber, formatTime } from "@/i18n/format";
import type { GrowthEvent, KnowledgeMap } from "../../contracts/knowledge";
import type { BriefingContext, BriefingSettings, ChatArtifactRef, ChatEvidenceRef, KnowledgeLocation, ChatMessage, LoupistSummary, LoupistBridge, InboxItem, RunSummaryItem, ScheduledRun, ScheduledTask, ModelCatalog } from "@/types";
import { Button } from "../ui/button";
import { Spinner } from "../ui/spinner";
import { Disclosure } from "../ui/disclosure";
import { ChatMessageCard } from "../chat/ChatMessageCard";
import { cadenceLabel, cadencePhrase, friendlyTimeZone, relativeRun, scheduledTaskTitle } from "./scheduledPresentation";
import { relationName } from "../map/relations";
import { BriefingAssets } from "../assets/BriefingAssets";
import { useWatchSettings } from "../assets/WatchFor";
import { followedCount, WATCHING_TASK_ID, WatchingItem, WatchingReader } from "./Watching";
import { BriefingFollowRows, BriefingAheadRows, hasAheadRows, hasFollowRows } from "./BriefingFinance";
import { runSummary } from "./runSummary";
import { useAssetNames } from "../assets/assetName";
import { printReader } from "../print/printDocuments";
import { PrinterIcon } from "lucide-react";
import "@/styles/briefings.css";

type Props = { onOpenModelSettings?: () => void; bridge: LoupistBridge; loupist: LoupistSummary;
  /** Provider status for each task's model: an account at its plan limit or needing reconnect makes the task wait. */
  modelCatalog?: ModelCatalog | null; initialRunId?: string; initialTaskId?: string;
  onOpenEvidence?: (item: InboxItem, evidence: ChatEvidenceRef, step?: { label: string; itemId?: string }) => void;
  onOpenMemory?: (item: InboxItem, location: KnowledgeLocation, title: string) => void;
  onOpenMemoryPage?: (location: KnowledgeLocation) => void;
  onOpenArtifact?: (item: InboxItem, artifact: ChatArtifactRef) => void;
  onOpenMap?: (disputedEntityIds?: string[]) => void;
  onInitialConsumed?: () => void; onOpenChat: (item: InboxItem) => Promise<void> };

/** The reader works on inbox items; run history arrives as stored runs. */
export const inboxItemOf = (run: ScheduledRun): InboxItem => ({
  briefingContext: run.briefing_context ?? null, error: run.error ?? null,
  runId: run.run_id, taskId: run.task_id, kind: run.kind, title: run.task_title,
  status: run.status, summary: run.summary, summaryItems: run.summary_items, startedAtMs: run.started_at_ms,
  finishedAtMs: run.finished_at_ms ?? null, readAtMs: run.read_at_ms, threadId: run.thread_id ?? null,
  messageId: run.message_id, skippedTools: run.skipped_tools,
});

const HISTORY_ROWS = 10;

/** Active shows tasks that are on, waiting for confirmation, or running. */
function taskListed(task: ScheduledTask, filter: "active" | "all") {
  return filter === "all" || task.status === "active" || Boolean(task.awaiting_confirmation) || task.latest_run?.status === "running";
}
function taskRank(task: ScheduledTask) {
  if (task.awaiting_confirmation) return 0;
  if (task.latest_run?.status === "running" || task.status === "active") return 1;
  if (task.status === "paused") return 2;
  return 3;
}

/**
 * Scheduled tasks: one-time and recurring work, with an optional briefing among
 * them. Creating or changing a custom task seeds the Thread and waits for
 * confirmation. Results are read in the Thread; a run opened here shows its
 * structured briefing and full answer.
 */
export function ScheduleView({ onOpenModelSettings, modelCatalog, bridge, loupist, initialRunId, initialTaskId, onInitialConsumed, onOpenChat, onOpenEvidence, onOpenMemory, onOpenMemoryPage, onOpenArtifact, onOpenMap }: Props) {
  const { t, i18n } = useTranslation("briefings");
  const [settings, setSettings] = useState<BriefingSettings | null>(null);
  const [tasks, setTasks] = useState<ScheduledTask[]>([]);
  // Nothing is open until the user picks a task or a deep link names one.
  const [focusTaskId, setFocusTaskId] = useState<string | null>(initialTaskId ?? null);
  const [focusExplicit, setFocusExplicit] = useState(Boolean(initialTaskId));
  const [history, setHistory] = useState<Record<string, ScheduledRun[]>>({});
  const [selected, setSelected] = useState<InboxItem | null>(null);
  const [filter, setFilter] = useState<"active" | "all">("active");
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
  const generation = useRef(0);
  const names = useAssetNames(bridge, loupist.id);
  // A host run (the results task) records what it did as items Rust wrote; a model run, its answer's first paragraph.
  const summaryText = (items: readonly RunSummaryItem[] | undefined, summary: string) => items?.length ? runSummary(items, name => names({ name })) : summary;
  const failure =briefingFailure(selected?.error || readerError, message?.failure_kind);
  const briefingTask = tasks.find(task => task.kind === "briefing") ?? settings?.task ?? null;
  // Watching is a pinned item, not a scheduled task: Rust checks on this computer without a model call.
  const { settings: watch, setSettings: setWatch } = useWatchSettings(bridge, loupist.id);
  const watchingShown = followedCount(watch) > 0;
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
  useEffect(() => { void load(); const off = bridge.onInboxChanged(event => { if (event.loupistId === loupist.id) void load(); }); return () => { generation.current++; off(); }; }, [load, bridge, loupist.id]);

  const loadHistory = useCallback(async (taskId: string) => {
    try { const runs = await bridge.scheduledTaskRuns({ loupist_id: loupist.id, task_id: taskId }); setHistory(current => ({ ...current, [taskId]: runs })); }
    catch { setError(true); }
  }, [bridge, loupist.id]);
  useEffect(() => { if (focusTaskId && focusTaskId !== WATCHING_TASK_ID) void loadHistory(focusTaskId); }, [focusTaskId, loadHistory, tasks]);

  // A run opened from a notification or a Thread message: find it across pages.
  useEffect(() => {
    if (loading || !initialRunId) return;
    let active = true;
    const open = async () => {
      let cursor: string | null | undefined;
      for (let page = 0; page < 10; page += 1) {
        const result = await bridge.inboxList({ loupistId: loupist.id, cursor, limit: 100 });
        const found = result.items.find(item => item.runId === initialRunId);
        if (found) { if (active) { setSelected(found); setFocusTaskId(found.taskId); setFocusExplicit(true); } return; }
        cursor = result.nextCursor;
        if (!cursor) break;
      }
      if (active) setError(true);
    };
    void open().catch(() => { if (active) setError(true); }).finally(() => { if (active) onInitialConsumed?.(); });
    return () => { active = false; };
    // Consumed once after loading; later refreshes do not reopen it.
  }, [loading, initialRunId]);
  useEffect(() => {
    if (loading || !initialTaskId || initialRunId) return;
    setFocusTaskId(initialTaskId); setFocusExplicit(true);
    const opened = tasks.find(task => task.id === initialTaskId);
    if (opened && !taskListed(opened, "active")) setFilter("all");
    document.querySelector(`[data-schedule-task="${CSS.escape(initialTaskId)}"]`)?.scrollIntoView({ block: "nearest" });
    onInitialConsumed?.();
  }, [loading, initialTaskId]);

  useEffect(() => {
    if (!selected || selected.readAtMs !== null || selected.status === "running") return;
    let timer: ReturnType<typeof setTimeout>;
    const begin = () => { clearTimeout(timer); if (document.visibilityState !== "hidden") timer = setTimeout(() => { void bridge.inboxMarkRead({ loupistId: loupist.id, runIds: [selected.runId] }).then(() => setSelected(current => current && current.runId === selected.runId ? { ...current, readAtMs: Date.now() } : current)).catch(() => setError(true)); }, 1000); };
    begin(); document.addEventListener("visibilitychange", begin);
    return () => { clearTimeout(timer); document.removeEventListener("visibilitychange", begin); };
  }, [bridge, loupist.id, selected?.runId, selected?.readAtMs, selected?.status]);
  useEffect(() => {
    setMessage(null); setReaderError(null); if (!selected) return;
    let active = true; setReaderLoading(true);
    void bridge.loupistSelect(loupist.id).then(workspace => {
      if (!active) return;
      const thread = workspace.threads.find(thread => thread.id === selected.threadId);
      const answer = thread?.messages.find(message => message.id === selected.messageId) ?? null;
      setMessage(answer);
      if (selected.status === "failed" || selected.status === "interrupted") setReaderError(selected.error || answer?.content || t("failed"));
    }).catch(() => { if (active) setReaderError(t("loadError")); }).finally(() => { if (active) setReaderLoading(false); });
    return () => { active = false; };
  }, [bridge, loupist.id, selected?.runId, selected?.status, selected?.messageId, readerRevision, t]);

  const displayTitle = (item: { kind: string; title: string }) => scheduledTaskTitle(item.kind, item.title);
  const listed = (task: ScheduledTask) => taskListed(task, filter);
  // A task on an account that cannot answer right now waits instead of failing (providers spec §3.1).
  const waiting = (task: ScheduledTask) => {
    if (task.status !== "active" || !task.model_profile_id) return null;
    const provider = modelCatalog?.models.find(model => model.id === task.model_profile_id)?.provider ?? task.model_profile_id.split("/")[0];
    const status = modelCatalog?.providers.find(option => option.id === provider)?.status;
    return status?.notice?.kind === "plan_limit" ? t("schedule.waitingPlan") : status?.state === "needs_reconnect" ? t("schedule.waitingReconnect") : null;
  };
  const taskStatus = (task: ScheduledTask) => task.awaiting_confirmation ? t("schedule.status.awaiting") : waiting(task) ?? t(`schedule.status.${task.status}`);
  const nextRun = (task: ScheduledTask) => task.status === "active" && task.next_run_at_ms
    ? t("schedule.next", { time: relativeRun(task.next_run_at_ms, Date.now(), task.schedule.kind === "weekly" || task.schedule.kind === "daily" ? task.schedule.timezone : undefined) })
    : null;
  const ordered = [...tasks].sort((a, b) => taskRank(a) - taskRank(b) || (a.next_run_at_ms ?? Number.MAX_SAFE_INTEGER) - (b.next_run_at_ms ?? Number.MAX_SAFE_INTEGER) || displayTitle(a).localeCompare(displayTitle(b)));
  const visible = ordered.filter(listed);
  const watchingFocus = Boolean(watchingShown && focusExplicit && focusTaskId === WATCHING_TASK_ID && !selected);
  const focusTask = watchingFocus || !focusExplicit ? null : ordered.find(task => task.id === focusTaskId) ?? null;
  const readerOpen = Boolean(selected) || Boolean(focusTask) || watchingFocus;
  // One time zone for every timed task is named once in the header instead of on each row.
  const zones = new Set(ordered.flatMap(task => task.schedule.kind === "daily" || task.schedule.kind === "weekly" ? [task.schedule.timezone] : []));
  const sharedZone = zones.size === 1 ? [...zones][0]! : null;
  const cadence = (task: ScheduledTask) => task.kind === "earnings" ? t("schedule.earningsCadence") : sharedZone ? cadencePhrase(task.schedule) : cadenceLabel(task.schedule);
  const openTask = (task: ScheduledTask) => { setSelected(null); setFocusTaskId(task.id); setFocusExplicit(true); };

  const runRow = (run: ScheduledRun) => {
    const failed = run.status === "failed" || run.status === "interrupted";
    return <li key={run.run_id}><button type="button" className="briefings-item" data-schedule-run={run.run_id} aria-current={selected?.runId === run.run_id || undefined} data-unread={run.read_at_ms === null && run.status !== "running"} onClick={() => { setSelected(inboxItemOf(run)); setFocusExplicit(true); }}>
      <span className="briefings-item-heading">{run.read_at_ms === null && run.status !== "running" && <><span className="briefings-unread" aria-hidden="true" /><span className="sr-only">{t("unread")}</span></>}<strong>{formatDate(run.started_at_ms, { month: "short", day: "numeric", year: undefined })}</strong><time>{formatTime(run.started_at_ms)}</time></span>
      <span className="briefings-summary" data-failed={failed}>{run.status === "running" ? <><Spinner />{t("preparingBody")}</> : failed ? t("failedSummary") : summaryText(run.summary_items, run.summary)}</span>
    </button></li>;
  };
  const headerMeta = [
    tasks.length ? t("schedule.summary", { count: tasks.length, formattedCount: formatNumber(tasks.length) }) : null,
    running.length ? t("schedule.runningCount", { count: running.length, formattedCount: formatNumber(running.length) }) : null,
    sharedZone ? friendlyTimeZone(sharedZone, i18n.language) : null,
  ].filter(Boolean).join(" · ");
  const rowState = (task: ScheduledTask) => task.awaiting_confirmation || task.status !== "active" || waiting(task) ? taskStatus(task) : null;
  return <section className="briefings-view schedule-view" aria-label={t("schedule.title")}>
    <header className="briefings-bar"><h1>{t("schedule.title")}</h1>
      <p className="briefings-meta">{headerMeta}</p>
    </header>
    {error && <p role="alert" className="briefings-error">{t("loadError")} <Button variant="ghost" onClick={() => void load()}>{t("retry")}</Button></p>}
    {loading ? <p role="status">{t("loading")}</p> : <div className="briefings-layout" data-reader-open={readerOpen || undefined} data-empty={(!visible.length && !selected && !watchingShown) || undefined}>
      <div className="schedule-list">
        <div className="schedule-filter" role="group" aria-label={t("schedule.filterLabel")} data-schedule-filter={filter}>
          <Button type="button" size="sm" variant={filter === "active" ? "secondary" : "ghost"} aria-pressed={filter === "active"} onClick={() => setFilter("active")}>{t("schedule.filterActive")}</Button>
          <Button type="button" size="sm" variant={filter === "all" ? "secondary" : "ghost"} aria-pressed={filter === "all"} onClick={() => setFilter("all")}>{t("schedule.filterAll")}</Button>
        </div>
        {watch && watchingShown ? <ul className="schedule-tasks schedule-pinned" aria-label={t("watch.title")}>
          <WatchingItem settings={watch} current={watchingFocus && !selected} onOpen={() => { setSelected(null); setFocusTaskId(WATCHING_TASK_ID); setFocusExplicit(true); }} />
        </ul> : null}
        {running.length ? <section aria-label={t("schedule.running")}>
          <h2 className="schedule-section-label">{t("schedule.running")}</h2>
          <ul className="schedule-running">{running.map(task => <li key={task.id}>
            <Spinner /><span className="schedule-running-title">{displayTitle(task)}</span>
            <time>{formatTime(task.latest_run!.started_at_ms)}</time>
            <Button variant="ghost" size="sm" onClick={() => { setFocusTaskId(task.id); setSelected(inboxItemOf(task.latest_run!)); setFocusExplicit(true); }}>{t("schedule.open")}</Button>
          </li>)}</ul>
        </section> : null}
        <section aria-label={t("schedule.tasks")}>
          {visible.length ? <h2 className="schedule-section-label">{t("schedule.tasks")}</h2> : null}
          {!ordered.length ? <div className="briefings-empty"><h3>{t("schedule.emptyTitle")}</h3><p>{t("schedule.emptyBody")}</p>
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
                  <span className="schedule-task-heading"><strong title={task.title}>{displayTitle(task)}</strong>{state ? <span className="schedule-task-status">{state}</span> : null}</span>
                  <span className="briefings-meta">{cadence(task)}{next ? ` · ${next}` : ""}</span>
                </button>
              </div>
            </li>;
          })}</ul>}
        </section>
      </div>
      {readerOpen ? <div className="briefings-reader-scroll">{!selected && watchingFocus && watch ? <WatchingReader bridge={bridge} loupist={loupist} settings={watch} onSettings={setWatch} onBack={() => { setFocusExplicit(false); setFocusTaskId(null); }} />
        : selected ? <article className="briefings-reader" ref={readerRef}><Button variant="ghost" className="briefings-back" data-print="omit" onClick={() => setSelected(null)}>{t("schedule.backTo", { title: focusTask ? displayTitle({ kind: focusTask.kind, title: focusTask.title }) : t("schedule.title") })}</Button><header><h2>{displayTitle(selected)}</h2><p className="briefings-meta">{formatDate(selected.startedAtMs)} · {formatTime(selected.startedAtMs)} · {loupist.name}</p></header>
        {selected.briefingContext?.changed ? <BriefingSections bridge={bridge} loupistId={loupist.id} loupistName={loupist.name} context={selected.briefingContext} nextRun={briefingTask ? nextRun(briefingTask) : null} skipped={selected.skippedTools.map(tool => tool.name)}
          onOpenMap={onOpenMap} onOpenCheck={(journalId, title) => onOpenMemory?.(selected, { loupistId: loupist.id, surface: "note", objectId: journalId, selection: { kind: "journal", journalId } }, title)} /> : null}
        {readerLoading ? <p role="status">{t("loading")}</p> : readerError ? <div role="alert" className="briefings-failure"><h3>{t("failure.title")}</h3><p>{t(`failure.${failure}`)}</p>{failure === "auth" && onOpenModelSettings ? <Button variant="link" onClick={onOpenModelSettings}>{t("failure.models")}</Button> : null}<Disclosure className="briefings-failure-details" title={t("failure.details")}><pre>{readerError}</pre></Disclosure><Button onClick={() => setReaderRevision(value => value + 1)}>{t("retry")}</Button></div> : message ? <ChatMapProvider bridge={bridge} loupistId={loupist.id} chatId={selected.threadId ?? ""} messages={[message]} onOpen={() => onOpenMap ? onOpenMap() : void onOpenChat(selected)}><ChatMessageCard outlineOpen message={message} loupistId={loupist.id} chatId={selected.threadId ?? undefined} loupistName={loupist.name} onOpenMemory={(location, title) => onOpenMemory ? onOpenMemory(selected, location, title) : void onOpenChat(selected)} onOpenMemoryPage={location => onOpenMemoryPage ? onOpenMemoryPage(location) : void onOpenChat(selected)} onOpenArtifact={artifact => onOpenArtifact ? onOpenArtifact(selected, artifact) : void onOpenChat(selected)} onOpenEvidence={(evidence, step) => onOpenEvidence ? onOpenEvidence(selected, evidence, step) : void onOpenChat(selected)} onOpenLink={url => bridge.openExternalUrl(url)} onReadAttachment={async (messageId, attachmentId) => (await bridge.chatAttachmentRead(loupist.id, selected.threadId!, messageId, attachmentId)).data_url} /></ChatMapProvider> : <p>{selected.status === "running" ? t("preparingBody") : summaryText(selected.summaryItems, selected.summary)}</p>}
        {(selected.briefingContext || selected.skippedTools.length > 0) && <Disclosure title={t("checked")}>
          {selected.briefingContext && <><p>{t("companies", { symbols: selected.briefingContext.symbols.join(", ") || t("noCompanies") })}</p><p>{t("principles", { count: selected.briefingContext.principlesChecked })}</p><p>{t("facts", { count: selected.briefingContext.newFacts })}</p></>}
          {!!selected.skippedTools.length && <p>{t("skipped", { names: selected.skippedTools.map(tool => tool.name).join(", ") })}</p>}
        </Disclosure>}
        <div className="briefings-actions" data-print="omit">
          {selected.threadId && <Button variant="ghost" className="briefings-continue" onClick={() => void onOpenChat(selected)}>{t("continue")}</Button>}
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
        <header className="schedule-detail-header"><div><h2>{displayTitle(focusTask)}</h2>
          <p className="briefings-meta">{[t(`schedule.kind.${focusTask.kind}`), cadence(focusTask), taskStatus(focusTask), nextRun(focusTask)].filter(Boolean).join(" · ")}</p></div>
          </header>
        {focusTask.kind === "custom" && focusTask.prompt ? <p className="schedule-task-prompt">{focusTask.prompt}</p> : null}
        <h3 className="schedule-section-label">{t("schedule.runs")}</h3>
        {(history[focusTask.id] ?? []).length ? <ol className="schedule-history" aria-label={t("schedule.history", { title: displayTitle(focusTask) })}>{(history[focusTask.id] ?? []).slice(0, HISTORY_ROWS).map(runRow)}</ol>
          : <p className="briefings-meta">{t("schedule.noRuns")}</p>}
      </article> : null}</div> : null}
    </div>}
  </section>;
}

/** Sections the host computed when the briefing ran; the narrative follows unparsed. */
function BriefingSections({ bridge, loupistId, loupistName, context, nextRun, skipped, onOpenMap, onOpenCheck }: {
  loupistName: string;
  bridge: LoupistBridge; loupistId: string; context: BriefingContext; nextRun: string | null; skipped: string[];
  onOpenMap?: (disputedEntityIds?: string[]) => void; onOpenCheck: (journalId: string, title: string) => void;
}) {
  const { t, i18n } = useTranslation("briefings");
  const { t: tKnowledge } = useTranslation("knowledge");
  // Stored summaries are English; rows speak the interface language through the Map's names and relations.
  const [map, setMap] = useState<KnowledgeMap | null>(null);
  // Followed assets are stored as symbols; people read their names.
  const [names, setNames] = useState<Record<string, string>>({});
  useEffect(() => {
    let live = true;
    void bridge.knowledgeMap({ loupistId, maxNodes: 1000 }).then(next => { if (live) setMap(next); }).catch(() => {});
    void bridge.watchlistList().then(entries => {
      if (live) setNames(Object.fromEntries(entries.flatMap(entry => entry.displayName?.trim() ? [[entry.symbol, entry.displayName.trim()]] : [])));
    }).catch(() => {});
    return () => { live = false; };
  }, [bridge, loupistId]);
  const rowText = (row: NonNullable<BriefingContext["changed"]>["rows"][number]) => {
    if (row.kind === "source_retained" || !row.subjectId) return row.summary;
    const event: GrowthEvent = { eventId: `${row.kind}:${row.subjectId}`, loupistId, atMs: 0, kind: row.kind as GrowthEvent["kind"], subjectId: row.subjectId, recordId: null, originChatId: null, originMessageId: null, actor: "loupist", summary: row.summary, beforeSummary: null, afterSummary: null };
    const link = whatsNewRows([event], map, i18n.language)[0]?.link;
    if (!link) return row.summary;
    return link.relation ? tKnowledge("map.newLink", { a: link.a, b: link.b, relation: relationName(link.relation, tKnowledge) }) : tKnowledge("map.newLinkPlain", { a: link.a, b: link.b });
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
    {context.assets?.length || hasFollowRows(context) ? <section aria-labelledby="briefing-companies"><h3 id="briefing-companies" className="schedule-section-label">{t("sections.companies")}</h3>
      {context.assets?.length ? <BriefingAssets assets={context.assets} bridge={bridge} loupistId={loupistId} loupistName={loupistName} onOpenLink={url => void bridge.openExternalUrl(url)} /> : null}
      <BriefingFollowRows context={context} bridge={bridge} loupistId={loupistId} onOpenLink={url => void bridge.openExternalUrl(url)} />
    </section> : context.symbols.length ? <section aria-labelledby="briefing-companies"><h3 id="briefing-companies" className="schedule-section-label">{t("sections.companies")}</h3>
      <p>{context.symbols.map(symbol => names[symbol.split(":")[0]!] ?? symbol).join(" · ")}</p>
    </section> : null}
    {ahead.length || nextRun || skipped.length || hasAheadRows(context) ? <section aria-labelledby="briefing-ahead"><h3 id="briefing-ahead" className="schedule-section-label">{t("sections.ahead")}</h3>
      <ul className="briefing-ahead">
        <BriefingAheadRows context={context} bridge={bridge} loupistId={loupistId} onOpenLink={url => void bridge.openExternalUrl(url)} />
        {ahead.map(check => <li key={check.journalId}><Button variant="link" onClick={() => onOpenCheck(check.journalId, check.title)}>{check.byDate ? t("ahead.checkDue", { date: formatDate(Date.parse(`${check.byDate}T00:00:00`), { month: "short", day: "numeric", year: undefined }), title: check.title }) : check.title}</Button></li>)}
        {nextRun ? <li>{nextRun}</li> : null}
        {skipped.length ? <li>{t("ahead.skipped", { names: skipped.join(", ") })}</li> : null}
      </ul>
    </section> : null}
  </div>;
}
