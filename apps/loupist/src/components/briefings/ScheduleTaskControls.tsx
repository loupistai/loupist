import { useState } from "react";
import { useTranslation } from "@/i18n";
import type { LoupistBridge, ScheduledTask } from "@/types";
import { errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";
import { Spinner } from "../ui/spinner";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";

type Busy = "status" | "run" | "delete" | null;

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
  const canToggle = task.status !== "completed";
  // The briefing is turned off, not deleted; the results task is paused, not deleted.
  const canDelete = task.kind === "custom";
  return <div className="schedule-task-controls" data-print="omit">
    <div className="schedule-task-actions">
      {canToggle ? <Button variant="outline" size="sm" disabled={busy !== null} data-schedule-status={nextStatus}
        onClick={() => void act("status", () => bridge.scheduledTaskSetStatus({ ...identity, expected_revision: task.revision, status: nextStatus }), t("schedule.statusFailed")).then(ok => { if (ok) onChanged(); })}>
        {busy === "status" ? <Spinner /> : null}{statusLabel}
      </Button> : null}
      <Button variant="outline" size="sm" disabled={busy !== null || running} data-schedule-run-now
        onClick={() => void act("run", () => bridge.scheduledTaskRunNow(identity), t("schedule.runNowFailed")).then(ok => { if (ok) onChanged(); })}>
        {busy === "run" ? <Spinner /> : null}{running ? t("schedule.runningNow") : t("schedule.runNow")}
      </Button>
      {canDelete ? <Button variant="ghost" size="sm" disabled={busy !== null || running} title={running ? t("schedule.deleteWhileRunning") : undefined} onClick={() => { setError(null); setConfirming(true); }}>{t("schedule.delete")}</Button> : null}
    </div>
    {error && !confirming ? <p className="inline-error" role="alert">{error}</p> : null}
    <p className="briefings-meta schedule-task-change">{t("schedule.changeHint")}{onAskHome ? <> <Button variant="link" className="schedule-task-ask" onClick={() => onAskHome(t("schedule.changePrompt", { title }))}>{t("schedule.changeAsk")}</Button></> : null}</p>
    <Dialog open={confirming} onOpenChange={value => { if (busy !== "delete") setConfirming(value); }}>
      <DialogContent size="sm" className="dialog-form" showCloseButton={busy !== "delete"}>
        <DialogHeader><DialogTitle>{t("schedule.deleteTitle", { title })}</DialogTitle><DialogDescription>{t("schedule.deleteBody")}</DialogDescription></DialogHeader>
        <DialogBody>{error ? <p className="inline-error" role="alert">{error}</p> : null}</DialogBody>
        <DialogFooter>
          <Button variant="outline" disabled={busy === "delete"} onClick={() => setConfirming(false)}>{t("schedule.cancel")}</Button>
          <Button variant="destructive" disabled={busy === "delete"} data-schedule-delete-confirm
            onClick={() => void act("delete", () => bridge.scheduledTaskDelete({ ...identity, expected_revision: task.revision }), t("schedule.deleteFailed")).then(ok => { if (ok) { setConfirming(false); onDeleted(); } })}>
            {busy === "delete" ? <Spinner /> : null}{t("schedule.delete")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>;
}
