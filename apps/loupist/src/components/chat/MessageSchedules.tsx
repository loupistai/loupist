import { useCurrentVisit } from "../../app/loupistVisit";
import i18n, { useTranslation } from "@/i18n";
import { useGo } from "../navigation/Open";
import { formatDateTime } from "@/i18n/format";
import { useContext, useEffect, useState, type ReactNode } from "react";
import { ChevronRightIcon } from "lucide-react";
import type { ScheduleReceipt, ScheduledTask, ScheduledTaskListItem } from "../../types";
import { cadenceLabel, scheduledTaskTitle, unreadableTask } from "../briefings/scheduledPresentation";
import { Button } from "../ui/button";
import { TriggerCard } from "./TriggerCard";
import { BotScheduleTasks } from "./BotScheduleTasks";
import { useChatExecution } from "./ChatExecutionContext";

const t = i18n.getFixedT(null, "chat");

type TaskReceipt = Exclude<ScheduleReceipt, { question: string }>;
const statuses = (): Record<TaskReceipt["status"], string> => ({
  active: t("messageSchedules.active"), paused: t("messageSchedules.paused"), completed: t("messageSchedules.completed"),
});

export function MessageSchedules({ receipts, onChangeDraft }: {
  receipts: ScheduleReceipt[];
  onChangeDraft?: (text: string) => void;
}) {
  useTranslation("chat");
  const go = useGo();
  const onOpen = go ? (taskId: string) => go({ kind: "schedule", taskId, ...(taskId.startsWith("bot-handoff-") && { handoffReceipt: true }) }) : undefined;
  const scope = useChatExecution();
  const { tasks: botTasks, refresh } = useContext(BotScheduleTasks);
  const currentTask = (id: string) => botTasks
    ? botTasks.get(id) ?? [...botTasks.values()].find(task => task.confirmed_proposal_id === id) ?? null
    : undefined;
  const status = (receipt: TaskReceipt) => scope?.surface === "session" && receipt.action === "created" && receipt.task_id.startsWith("bot-handoff-")
    ? t("messageSchedules.handedToBot", { status: receiptStatus(receipt) }) : receiptStatus(receipt);
  if (!receipts.length) return null;
  return <div className="message-schedules" aria-label={t("messageSchedules.scheduledTasks")}>
    {receipts.map((receipt) => "question" in receipt ? <article className="message-schedule" key={receipt.task_id}>
      <button type="button" className="message-schedule-open" disabled={!onOpen} onClick={() => onOpen?.(receipt.task_id)}>
        <strong>{receipt.title}</strong><span>{t("messageSchedules.needsTime")}</span><ChevronRightIcon aria-hidden="true" />
      </button></article> : scope?.surface === "bot"
      ? <BotTaskCard key={receipt.task_id} receipt={receipt} onChangeDraft={onChangeDraft} onChanged={refresh} task={currentTask(receipt.task_id)} onOpen={onOpen} />
      : receipt.action === "created" && receipt.awaiting_confirmation
      ? <ProposalCard key={`${receipt.task_id}:${receipt.revision}`} receipt={receipt} onOpen={onOpen} />
      : <ScheduleCard key={receipt.task_id} receipt={receipt} onOpen={onOpen} status={status(receipt)} detail={receipt.next_run_at_ms && receipt.status === "active" ? t("messageSchedules.nextRun", { date: formatDateTime(receipt.next_run_at_ms) }) : undefined} />)}
  </div>;
}

function BotTaskCard({ receipt, task, onOpen, onChanged, onChangeDraft }: {
  onChangeDraft?: (text: string) => void;
  onChanged?: () => void; receipt: TaskReceipt; task?: ScheduledTaskListItem | null; onOpen?: (taskId: string) => void;
}) {
  const scope = useChatExecution();
  if (task === null) return <ScheduleCard receipt={receipt} status={t(receipt.awaiting_confirmation ? "messageSchedules.discarded" : "messageSchedules.deleted")} gone />;
  if (!task) return <ScheduleCard receipt={receipt} onOpen={onOpen} showCadence={false} gone={false} />;
  if (unreadableTask(task)) return <ScheduleCard receipt={receipt} status={i18n.t("briefings:schedule.unreadableTask")} showCadence={false} />;
  const current = { ...receipt, task_id: task.id, revision: task.revision, title: scheduledTaskTitle(task.kind, task.title), schedule: task.schedule,
    terms: task.terms, delivery: task.delivery, report: task.report, preset: task.preset };
  if (task.trigger && !task.replaces && scope) {
    return <ScheduleCard receipt={current} onOpen={onOpen} status={statuses()[task.status]} detail={task.awaiting_confirmation ? t("messageSchedules.proposalHint") : undefined}>
      <TriggerCard trigger={task.trigger} bridge={scope.bridge} loupistId={task.loupist_id} taskId={task.id} revision={task.revision} schedule={task.schedule} prompt={task.prompt} onChanged={onChanged} onChangeDraft={onChangeDraft} />
    </ScheduleCard>;
  }
  if (task.awaiting_confirmation && receipt.awaiting_confirmation) {
    if (task.revision !== receipt.revision) return <ScheduleCard receipt={receipt} status={t("messageSchedules.replaced")} gone />;
    return <ProposalCard key={current.revision} receipt={current} onOpen={onOpen} onChanged={onChanged} onChangeDraft={onChangeDraft} awaiting declinedInVisit={task.declined_in_visit} />;
  }
  if (task.terms || task.report || task.preset) {
    return <WatchReceipt receipt={current} task={task} onOpen={onOpen} onChanged={onChanged} onChangeDraft={onChangeDraft} />;
  }
  const next = task.status === "active" && task.next_run_at_ms != null
    ? t("messageSchedules.nextRun", { date: formatDateTime(task.next_run_at_ms) }) : undefined;
  return <ScheduleCard receipt={current} onOpen={onOpen} status={statuses()[task.status]} detail={next} gone={false} />;
}

function WatchTerms({ receipt }: { receipt: TaskReceipt }) {
  const { t: bt } = useTranslation("briefings");
  return <dl className="schedule-delivery">
    {receipt.terms && <div><dt>{bt("schedule.checks")}</dt><dd>{receipt.terms}</dd></div>}
    <div><dt>{bt("schedule.tellsYou")}</dt><dd>{bt(receipt.report === "when_met" ? "schedule.whenMet" : "schedule.everyRun")}</dd></div>
    <div><dt>{bt("schedule.notifies")}</dt><dd>{bt(receipt.delivery?.notify === "now" ? "schedule.notifyNow" : receipt.delivery?.notify === "off" ? "schedule.notifyOff" : receipt.delivery?.notify === "digest" ? "schedule.notifyDigest" : "schedule.notifyInherited")}</dd></div>
  </dl>;
}

function WatchReceipt({ receipt, task, onOpen, onChanged, onChangeDraft }: {
  receipt: TaskReceipt; task: ScheduledTaskListItem; onOpen?: (id: string) => void; onChanged?: () => void; onChangeDraft?: (text: string) => void;
}) {
  const scope = useChatExecution();
  const { t: bt } = useTranslation("briefings");
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const pause = async () => {
    if (!scope || busy) return;
    setBusy(true); setFailed(false);
    try {
      await scope.bridge.scheduledTaskSetStatus({ loupist_id: task.loupist_id, task_id: task.id, expected_revision: task.revision, status: "paused" });
      onChanged?.();
    } catch { setFailed(true); }
    finally { setBusy(false); }
  };
  const checkingFirst = task.status === "active" && task.report === "when_met" &&
    (task.latest_run?.first_run && task.latest_run.status === "running" || task.last_run_at_ms === null);
  const detail = checkingFirst ? t("messageSchedules.firstCheckRunning")
    : task.next_run_at_ms == null ? undefined : t("messageSchedules.nextRun", { date: formatDateTime(task.next_run_at_ms) });
  return <ScheduleCard receipt={receipt} status={statuses()[task.status]} onOpen={onOpen} detail={detail}>
    <WatchTerms receipt={receipt} />
    <p className="message-schedule-spending">{bt("schedule.watchCost")}</p>
    <div className="message-schedule-actions">
      {scope && task.status === "active" && <Button size="sm" variant="outline" disabled={busy} onClick={() => void pause()}>{bt("schedule.pause")}</Button>}
      {onChangeDraft && <Button size="sm" variant="ghost" onClick={() => onChangeDraft(bt("schedule.changePrompt", { title: task.title }))}>{bt("schedule.changeAsk")}</Button>}
      {failed && <span role="alert">{bt("schedule.statusFailed")}</span>}
    </div>
  </ScheduleCard>;
}

function receiptStatus(receipt: TaskReceipt) {
  if (receipt.action === "deleted") return t("messageSchedules.deleted");
  if (receipt.action === "started") return t("messageSchedules.runStarted");
  if (receipt.action === "stopped") return t("messageSchedules.runStopped");
  return statuses()[receipt.status];
}

function ScheduleCard({ receipt, onOpen, status, gone = receipt.action === "deleted", detail, showCadence = true, children }: {
  receipt: TaskReceipt; onOpen?: (taskId: string) => void; status?: string; gone?: boolean; detail?: string; showCadence?: boolean; children?: ReactNode;
}) {
  const summary = <span className="message-schedule-summary">
    <span className="message-schedule-heading"><strong title={receipt.title}>{receipt.title}</strong>{status && <span>{status}</span>}</span>
    {showCadence && !gone && <span className="message-schedule-cadence">{cadenceLabel(receipt.schedule)}</span>}
    {detail && <span className="message-schedule-cadence">{detail}</span>}
  </span>;
  return <article className="message-schedule" aria-label={t("messageSchedules.taskLabel", { title: receipt.title })}>
    {gone ? <div className="message-schedule-open message-schedule-gone">{summary}</div>
      : <button type="button" className="message-schedule-open" disabled={!onOpen}
        onClick={() => onOpen?.(receipt.task_id)} title={t("messageSchedules.openTask", { title: receipt.title })}>
        {summary}<ChevronRightIcon aria-hidden="true" />
      </button>}
    {children}
  </article>;
}

type Proposal = { status: "loading" | "awaiting" | "active" | "paused" | "discarded" | "completed" | "replaced"; nextRunAtMs?: number | null };

const proposalFrom = (task: ScheduledTask | null, displayedRevision: number): Proposal => !task ? { status: "discarded" }
  : task.awaiting_confirmation ? { status: task.revision === displayedRevision ? "awaiting" : "replaced" }
    : { status: task.status, nextRunAtMs: task.next_run_at_ms };

/**
 * A task the Loupist set up from a sentence in the conversation. It stays paused
 * until the user taps Turn on, so a misread time never runs on its own. The
 * card reads the task's current state, so it is right after a reload too.
 */
function ProposalCard({ receipt, onOpen, awaiting, onChanged, onChangeDraft, declinedInVisit }: { receipt: TaskReceipt; declinedInVisit?: number; onChangeDraft?: (text: string) => void; awaiting?: boolean; onChanged?: () => void; onOpen?: (taskId: string) => void }) {
  const scope = useChatExecution();
  const visit = useCurrentVisit(scope?.loupistId ?? "");
  const [proposal, setProposal] = useState<Proposal>({ status: scope && !awaiting ? "loading" : "awaiting" });
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const identity = scope ? { loupist_id: scope.loupistId, task_id: receipt.task_id } : null;

  useEffect(() => {
    if (!scope || awaiting) return undefined;
    let live = true;
    void scope.bridge.scheduledTaskGet({ loupist_id: scope.loupistId, task_id: receipt.task_id })
      .then((task) => { if (live) setProposal(proposalFrom(task, receipt.revision)); })
      .catch(() => { if (live) setProposal({ status: "awaiting" }); });
    return () => { live = false; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed on the scope's fields; a new scope object for the same thread must not re-read.
  }, [scope?.bridge, scope?.loupistId, receipt.task_id, receipt.revision, awaiting]);

  const act = async (action: "confirm" | "discard" | "undo") => {
    if (!scope || !identity || busy) return;
    setBusy(true);
    setFailed(false);
    try {
      if (action === "confirm") setProposal(proposalFrom(await scope.bridge.scheduledTaskConfirm({ ...identity, expected_revision: receipt.revision }), receipt.revision));
      else if (scope.surface === "bot" && visit) await scope.bridge.scheduledProposalSetDeclined({ ...identity, visit: visit.openedAtMs, declined: action === "discard" });
      else if (action === "discard") { await scope.bridge.scheduledTaskDiscard(identity); setProposal({ status: "discarded" }); }
      onChanged?.();
    } catch {
      setFailed(true);
      // The task may have changed elsewhere; show what is true now. A failed
      // re-read says nothing about the task, so the card stays as it was.
      const current = await scope.bridge.scheduledTaskGet(identity).then((task) => proposalFrom(task, receipt.revision), () => undefined);
      if (current) setProposal(current);
    } finally {
      setBusy(false);
    }
  };

  if (declinedInVisit !== undefined) return <ScheduleCard receipt={receipt} status={t("messageSchedules.notTurnedOn")} gone>
    {declinedInVisit === visit?.openedAtMs && <div className="message-schedule-actions">
      <Button size="sm" variant="ghost" disabled={busy} onClick={() => void act("undo")}>{t("messageSchedules.undoDecline")}</Button>
      {failed && <span role="alert">{t("messageSchedules.couldNotChange")}</span>}
    </div>}
  </ScheduleCard>;
  if (proposal.status === "loading") return <ScheduleCard receipt={receipt} onOpen={onOpen} status={t("messageSchedules.awaiting")} />;
  if (proposal.status === "replaced") return <ScheduleCard receipt={receipt} status={t("messageSchedules.replaced")} gone />;
  if (proposal.status === "discarded") return <ScheduleCard receipt={receipt} status={t("messageSchedules.discarded")} gone />;
  if (proposal.status !== "awaiting") {
    const next = proposal.status === "active" && proposal.nextRunAtMs ? t("messageSchedules.nextRun", { date: formatDateTime(proposal.nextRunAtMs) }) : undefined;
    return <ScheduleCard receipt={receipt} onOpen={onOpen} status={statuses()[proposal.status]} detail={next} />;
  }
  if (receipt.schedule.kind === "once" && receipt.schedule.at_ms <= Date.now()) {
    return <ScheduleCard receipt={receipt} status={t("messageSchedules.expired")} gone />;
  }
  return <ScheduleCard receipt={receipt} onOpen={onOpen} status={t("messageSchedules.awaiting")} detail={t("messageSchedules.proposalHint")}>
    {(receipt.report || receipt.terms) && <WatchTerms receipt={receipt} />}
    <p className="message-schedule-spending">{t("messageSchedules.spending")}</p>
    {scope ? <div className="message-schedule-actions">
      <Button type="button" size="sm" disabled={busy} onClick={() => void act("confirm")}>{t("messageSchedules.turnOn")}</Button>
      {onChangeDraft && <Button type="button" size="sm" variant="ghost" disabled={busy} onClick={() => onChangeDraft(i18n.t("briefings:schedule.changePrompt", { title: receipt.title }))}>{i18n.t("briefings:schedule.changeAsk")}</Button>}
      <Button type="button" size="sm" variant="ghost" disabled={busy} onClick={() => void act("discard")}>{receipt.report ? i18n.t("briefings:standing.notNow") : t("messageSchedules.discard")}</Button>
      {failed ? <span role="alert">{t("messageSchedules.couldNotChange")}</span> : null}
    </div> : null}
  </ScheduleCard>;
}
