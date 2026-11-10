import i18n, { useTranslation } from "@/i18n";
import { useGo } from "../navigation/Open";
import { formatDateTime } from "@/i18n/format";
import { useEffect, useState, type ReactNode } from "react";
import { ChevronRightIcon } from "lucide-react";
import type { ScheduleReceipt, ScheduledTask } from "../../types";
import { cadenceLabel } from "../briefings/scheduledPresentation";
import { Button } from "../ui/button";
import { useChatExecution } from "./ChatExecutionContext";

const t = i18n.getFixedT(null, "chat");

type TaskReceipt = Exclude<ScheduleReceipt, { question: string }>;
const statuses = (): Record<TaskReceipt["status"], string> => ({
  active: t("messageSchedules.active"), paused: t("messageSchedules.paused"), completed: t("messageSchedules.completed"),
});

export function MessageSchedules({ receipts }: {
  receipts: ScheduleReceipt[];
}) {
  useTranslation("chat");
  const go = useGo();
  const onOpen = go ? (taskId: string) => go({ kind: "schedule", taskId }) : undefined;
  const scope = useChatExecution();
  const status = (receipt: TaskReceipt) => scope?.surface === "session" && receipt.action === "created" && receipt.task_id.startsWith("bot-handoff-")
    ? t("messageSchedules.handedToBot", { status: receiptStatus(receipt) }) : receiptStatus(receipt);
  if (!receipts.length) return null;
  return <div className="message-schedules" aria-label={t("messageSchedules.scheduledTasks")}>
    {receipts.map((receipt) => "question" in receipt ? <article className="message-schedule" key={receipt.task_id}>
      <button type="button" className="message-schedule-open" disabled={!onOpen} onClick={() => onOpen?.(receipt.task_id)}>
        <strong>{receipt.title}</strong><span>{t("messageSchedules.needsTime")}</span><ChevronRightIcon aria-hidden="true" />
      </button></article> : receipt.action === "created" && receipt.awaiting_confirmation
      ? <ProposedSchedule key={receipt.task_id} receipt={receipt} onOpen={onOpen} />
      : <ScheduleCard key={receipt.task_id} receipt={receipt} onOpen={onOpen} status={status(receipt)} detail={receipt.next_run_at_ms && receipt.status === "active" ? t("messageSchedules.nextRun", { date: formatDateTime(receipt.next_run_at_ms) }) : undefined} />)}
  </div>;
}

function receiptStatus(receipt: TaskReceipt) {
  if (receipt.action === "deleted") return t("messageSchedules.deleted");
  if (receipt.action === "started") return t("messageSchedules.runStarted");
  if (receipt.action === "stopped") return t("messageSchedules.runStopped");
  return statuses()[receipt.status];
}

function ScheduleCard({ receipt, onOpen, status, gone = receipt.action === "deleted", detail, children }: {
  receipt: TaskReceipt; onOpen?: (taskId: string) => void; status: string; gone?: boolean; detail?: string; children?: ReactNode;
}) {
  return <article className="message-schedule" aria-label={t("messageSchedules.taskLabel", { title: receipt.title })}>
    <button type="button" className="message-schedule-open" disabled={gone || !onOpen}
      onClick={() => onOpen?.(receipt.task_id)} title={t("messageSchedules.openTask", { title: receipt.title })}>
      <span className="message-schedule-summary">
        <span className="message-schedule-heading"><strong title={receipt.title}>{receipt.title}</strong><span>{status}</span></span>
        <span className="message-schedule-cadence">{cadenceLabel(receipt.schedule)}</span>
        {detail ? <span className="message-schedule-cadence">{detail}</span> : null}
      </span>
      {!gone && <ChevronRightIcon aria-hidden="true" />}
    </button>
    {children}
  </article>;
}

type Proposal = { status: "loading" | "awaiting" | "active" | "paused" | "discarded" | "completed"; nextRunAtMs?: number | null };

const proposalFrom = (task: ScheduledTask | null): Proposal => !task ? { status: "discarded" }
  : task.awaiting_confirmation ? { status: "awaiting" }
    : { status: task.status, nextRunAtMs: task.next_run_at_ms };

/**
 * A task the Loupist set up from a sentence in the conversation. It stays paused
 * until the user taps Turn on, so a misread time never runs on its own. The
 * card reads the task's current state, so it is right after a reload too.
 */
function ProposedSchedule({ receipt, onOpen }: { receipt: TaskReceipt; onOpen?: (taskId: string) => void }) {
  const scope = useChatExecution();
  const [proposal, setProposal] = useState<Proposal>({ status: scope ? "loading" : "awaiting" });
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const identity = scope ? { loupist_id: scope.loupistId, task_id: receipt.task_id } : null;

  useEffect(() => {
    if (!scope) return undefined;
    let live = true;
    void scope.bridge.scheduledTaskGet({ loupist_id: scope.loupistId, task_id: receipt.task_id })
      .then((task) => { if (live) setProposal(proposalFrom(task)); })
      .catch(() => { if (live) setProposal({ status: "awaiting" }); });
    return () => { live = false; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed on the scope's fields; a new scope object for the same thread must not re-read.
  }, [scope?.bridge, scope?.loupistId, receipt.task_id]);

  const act = async (action: "confirm" | "discard") => {
    if (!scope || !identity || busy) return;
    setBusy(true);
    setFailed(false);
    try {
      if (action === "confirm") setProposal(proposalFrom(await scope.bridge.scheduledTaskConfirm(identity)));
      else { await scope.bridge.scheduledTaskDiscard(identity); setProposal({ status: "discarded" }); }
    } catch {
      setFailed(true);
      // The task may have changed elsewhere; show what is true now.
      setProposal(proposalFrom(await scope.bridge.scheduledTaskGet(identity).catch(() => null)));
    } finally {
      setBusy(false);
    }
  };

  if (proposal.status === "loading") return <ScheduleCard receipt={receipt} onOpen={onOpen} status={t("messageSchedules.awaiting")} />;
  if (proposal.status === "discarded") return <ScheduleCard receipt={receipt} status={t("messageSchedules.discarded")} gone />;
  if (proposal.status !== "awaiting") {
    const next = proposal.status === "active" && proposal.nextRunAtMs ? t("messageSchedules.nextRun", { date: formatDateTime(proposal.nextRunAtMs) }) : undefined;
    return <ScheduleCard receipt={receipt} onOpen={onOpen} status={statuses()[proposal.status]} detail={next} />;
  }
  return <ScheduleCard receipt={receipt} onOpen={onOpen} status={t("messageSchedules.awaiting")} detail={t("messageSchedules.proposalHint")}>
    {scope ? <div className="message-schedule-actions">
      <Button type="button" size="sm" disabled={busy} onClick={() => void act("confirm")}>{t("messageSchedules.turnOn")}</Button>
      <Button type="button" size="sm" variant="ghost" disabled={busy} onClick={() => void act("discard")}>{t("messageSchedules.discard")}</Button>
      {failed ? <span role="alert">{t("messageSchedules.couldNotChange")}</span> : null}
    </div> : null}
  </ScheduleCard>;
}
