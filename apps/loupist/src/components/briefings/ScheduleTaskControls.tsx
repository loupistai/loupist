import { useState } from "react";
import { useTranslation } from "@/i18n";
import type { LoupistBridge, ScheduledTask } from "@/types";
import { errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";
import { Spinner } from "../ui/spinner";
import { MoreHorizontalIcon } from "lucide-react";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "../ui/dropdown-menu";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";

type Busy = "status" | "run" | "delete" | "delivery" | null;

/**
 * A task's own controls. Pausing, running once and deleting go straight to the host,
 * so stopping a recurring model run never needs a model call. Time and prompt changes
 * stay in conversation with Bot.
 */
export function ScheduleTaskControls({ bridge, task, title, running, onChanged, onDeleted, onAskHome }: {
  bridge: LoupistBridge; task: ScheduledTask; title: string; running: boolean;
  onChanged: () => void; onDeleted: () => void; onAskHome?: (text: string) => void;
}) {
  const { t } = useTranslation("briefings");
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const identity = { loupist_id: task.loupist_id, task_id: task.id };
  const act = async (kind: Exclude<Busy, null>, work: () => Promise<unknown>, fallback: string) => {
    setBusy(kind); setError(null);
    try { await work(); return true; }
    catch (cause) { setError(errorMessage(cause, fallback)); return false; }
    finally { setBusy(null); }
  };
  const nextStatus = task.status === "active" && !task.awaiting_confirmation ? "paused" : "active";
  const statusLabel = task.awaiting_confirmation ? t("schedule.turnOn") : nextStatus === "paused" ? t("schedule.pause") : t("schedule.resume");
  // A completed one-time task cannot be reactivated without an edit; that edit is a conversation.
  const canToggle = task.status !== "completed"
    && task.declined_in_visit === undefined
    && !(task.awaiting_confirmation && task.trigger && !task.replaces);
  // The briefing is turned off, not deleted.
  const canDelete = task.kind === "custom";
  // A proposed task is not running yet: turning it on is its primary action, as Resume is.
  const paused = nextStatus === "active";
  const statusButton = (variant: "default" | "ghost") => <Button variant={variant} size="sm" disabled={busy !== null} data-schedule-status={nextStatus}
    onClick={() => void act("status", () => {
      const request = { ...identity, expected_revision: task.revision };
      return task.awaiting_confirmation
        ? bridge.scheduledTaskConfirm(request)
        : bridge.scheduledTaskSetStatus({ ...request, status: nextStatus });
    }, t("schedule.statusFailed")).then(ok => { if (ok) onChanged(); })}>
    {busy === "status" ? <Spinner /> : null}{statusLabel}
  </Button>;
  return <div className="schedule-task-controls" data-print="omit">
    {/* Active: Run now, then a quiet Pause. Paused: Resume is the one filled button. Delete sits in More. */}
    <div className="schedule-task-actions">
      {paused && canToggle ? statusButton("default") : null}
      <Button variant="outline" size="sm" disabled={busy !== null || running || task.awaiting_confirmation} data-schedule-run-now title={t("schedule.runNowCost")}
        onClick={() => void act("run", () => bridge.scheduledTaskRunNow(identity), t("schedule.runNowFailed")).then(ok => { if (ok) onChanged(); })}>
        {busy === "run" || running ? <Spinner /> : null}{running ? t("schedule.runningNow") : t("schedule.runNow")}
      </Button>
      {!paused && canToggle ? statusButton("ghost") : null}
      {canDelete ? <DropdownMenu>
        <DropdownMenuTrigger asChild><Button variant="ghost" size="icon-sm" aria-label={t("schedule.moreActions", { title })} disabled={busy !== null}><MoreHorizontalIcon /></Button></DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem disabled={running} onSelect={() => { setError(null); setConfirming(true); }}>{running ? t("schedule.deleteWhileRunning") : t("schedule.deleteEllipsis")}</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu> : null}
    </div>
    <dl className="schedule-delivery">
      {task.terms && <div><dt>{t("schedule.checks")}</dt><dd>{task.terms}</dd></div>}
      <div><dt>{t("schedule.tellsYou")}</dt><dd>{t(task.report === "when_met" ? "schedule.whenMet" : "schedule.everyRun")}</dd></div>
      <div><dt>{t("schedule.postsTo")}</dt><dd><select aria-label={t("schedule.postsTo")} disabled={busy !== null}
        value={task.delivery?.to ?? "bot"} onChange={event => {
          const to = event.currentTarget.value as "bot" | "none";
          void act("delivery", () => bridge.scheduledTaskSetDelivery({ loupistId: task.loupist_id, taskId: task.id, expectedRevision: task.revision, to }), t("schedule.statusFailed")).then(ok => { if (ok) onChanged(); });
        }}>
        <option value="bot">{t("schedule.postBot")}</option>
        <option value="none">{t("schedule.postSchedule")}</option>
      </select></dd></div>
      <div><dt>{t("schedule.notifies")}</dt><dd><select aria-label={t("schedule.notifies")} disabled={busy !== null || task.delivery?.to === "none"}
        value={task.delivery?.notify ?? ""} onChange={event => {
          const notify = event.currentTarget.value as "now" | "digest" | "off";
          void act("delivery", () => bridge.scheduledTaskSetDelivery({ loupistId: task.loupist_id, taskId: task.id, expectedRevision: task.revision, notify }), t("schedule.statusFailed")).then(ok => { if (ok) onChanged(); });
        }}>
        <option value="" disabled>{t("schedule.notifyInherited")}</option>
        <option value="now">{t("schedule.notifyNow")}</option>
        <option value="digest">{t("schedule.notifyDigest")}</option>
        <option value="off">{t("schedule.notifyOff")}</option>
      </select></dd></div>
    </dl>
    {error && !confirming ? <p className="inline-error" role="alert">{error}</p> : null}
    <p className="briefings-meta schedule-task-change">{t(task.kind === "briefing" ? "schedule.briefingChangeHint" : "schedule.changeHint")}{onAskHome ? <> <Button variant="link" className="schedule-task-ask" onClick={() => onAskHome(t("schedule.changePrompt", { title }))}>{t("schedule.changeAsk")}</Button></> : null}</p>
    <Dialog open={confirming} onOpenChange={value => { if (busy !== "delete") setConfirming(value); }}>
      <DialogContent size="sm" className="dialog-form" showCloseButton={busy !== "delete"}>
        <DialogHeader><DialogTitle>{t("schedule.deleteTitle", { title })}</DialogTitle><DialogDescription>{t("schedule.deleteBody")}</DialogDescription></DialogHeader>
        <DialogBody>{error ? <p className="inline-error" role="alert">{error}</p> : null}</DialogBody>
        <DialogFooter>
          <Button variant="outline" autoFocus disabled={busy === "delete"} onClick={() => setConfirming(false)}>{t("schedule.cancel")}</Button>
          <Button variant="destructive" disabled={busy === "delete"} data-schedule-delete-confirm
            onClick={() => void act("delete", () => bridge.scheduledTaskDelete({ ...identity, expected_revision: task.revision }), t("schedule.deleteFailed")).then(ok => { if (ok) { setConfirming(false); onDeleted(); } })}>
            {busy === "delete" ? <Spinner /> : null}{t("schedule.delete")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>;
}
