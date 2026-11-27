import { useEffect, useRef, useState } from "react";
import { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import type { ProgramUndo } from "../../contracts/scheduled";
import type { LoupistBridge, ScheduledTask } from "../../types";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import { Spinner } from "../ui/spinner";
import { LearningProgramEditor } from "./LearningProgramEditor";
import { useSlow } from "./useSlow";

type Preview = { state: "loading" } | { state: "error" } | { state: "ready"; counts: ProgramUndo };

/** Counts in words; "{n} runs" and "{k} sections changed since" each take their own plural. */
function undoneLine(t: (key: string, options?: Record<string, unknown>) => string, counts: ProgramUndo) {
  return t("programs.undoAll.done", {
    runs: t("programs.undoAll.runs", { count: counts.runs, formattedCount: formatNumber(counts.runs) }),
    kept: t("programs.undoAll.keptSince", { count: counts.kept, formattedCount: formatNumber(counts.kept) }),
  });
}

/**
 * A learning program's own edits in its Schedule detail: Edit opens the program editor, and Undo all
 * runs states what it would undo (a dry call) before it undoes every run in one transaction.
 */
export function LearningProgramActions({ bridge, task, names, onChanged }: {
  bridge: LoupistBridge;
  task: ScheduledTask;
  names: ReadonlyMap<string, string>;
  onChanged: () => void;
}) {
  const { t } = useTranslation("briefings");
  const [editing, setEditing] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [preview, setPreview] = useState<Preview>({ state: "loading" });
  const [attempt, setAttempt] = useState(0);
  const [undoing, setUndoing] = useState(false);
  const [undoFailed, setUndoFailed] = useState(false);
  const [done, setDone] = useState<ProgramUndo | null>(null);
  const editButton = useRef<HTMLButtonElement>(null);
  useEffect(() => { setDone(null); }, [task.id]);
  useEffect(() => {
    if (!confirming) return undefined;
    let live = true;
    setPreview({ state: "loading" });
    bridge.learningProgramUndoAll({ loupistId: task.loupist_id, taskId: task.id, preview: true }).then(
      counts => { if (live) setPreview({ state: "ready", counts }); },
      () => { if (live) setPreview({ state: "error" }); },
    );
    return () => { live = false; };
  }, [bridge, confirming, task.loupist_id, task.id, attempt]);
  const undoAll = async () => {
    if (undoing) return;
    setUndoing(true); setUndoFailed(false);
    try {
      const counts = await bridge.learningProgramUndoAll({ loupistId: task.loupist_id, taskId: task.id });
      setDone(counts); setConfirming(false); onChanged();
    } catch { setUndoFailed(true); }
    finally { setUndoing(false); }
  };
  const counts = preview.state === "ready" ? preview.counts : null;
  const slow = useSlow(confirming && preview.state === "loading");
  const nothing = counts !== null && counts.runs === 0;
  return <div className="briefings-actions schedule-program-actions" data-print="omit">
    <Button ref={editButton} variant="outline" size="sm" data-program-edit onClick={() => setEditing(true)}>{t("programs.edit")}</Button>
    <Button variant="ghost" size="sm" data-program-undo-all onClick={() => { setUndoFailed(false); setConfirming(true); }}>{t("programs.undoAll.button")}</Button>
    {done ? <p role="status" className="briefings-meta" data-program-undone>{undoneLine(t, done)}</p> : null}
    <LearningProgramEditor bridge={bridge} task={task} names={names} open={editing}
      onOpenChange={open => { setEditing(open); if (!open) requestAnimationFrame(() => editButton.current?.focus()); }}
      onSaved={() => onChanged()} />
    <Dialog open={confirming} onOpenChange={value => { if (!undoing) setConfirming(value); }}>
      <DialogContent size="sm" className="dialog-form" showCloseButton={!undoing} data-program-undo-confirm-dialog>
        <DialogHeader>
          <DialogTitle>{t("programs.undoAll.title", { title: task.title })}</DialogTitle>
          <DialogDescription>{t("programs.undoAll.keeps")}</DialogDescription>
        </DialogHeader>
        <DialogBody>
          {preview.state === "loading" && slow ? <p role="status" className="briefings-meta"><Spinner />{t("programs.undoAll.counting")}</p> : null}
          {preview.state === "error" ? <p role="alert">{t("programs.undoAll.previewFailed")} <Button variant="ghost" size="sm" onClick={() => setAttempt(value => value + 1)}>{t("retry")}</Button></p> : null}
          {counts && nothing ? <p data-program-undo-nothing>{t("programs.undoAll.nothing")}</p> : null}
          {counts && !nothing ? <ul data-program-undo-preview>
            <li>{t("programs.undoAll.previewRuns", { count: counts.runs, formattedCount: formatNumber(counts.runs) })}</li>
            <li>{t("programs.undoAll.previewUndone", { count: counts.undone, formattedCount: formatNumber(counts.undone) })}</li>
            {counts.kept > 0 ? <li>{t("programs.undoAll.previewKept", { count: counts.kept, formattedCount: formatNumber(counts.kept) })}</li> : null}
          </ul> : null}
          {undoFailed ? <p className="inline-error" role="alert">{t("programs.undoAll.failed")}</p> : null}
        </DialogBody>
        <DialogFooter>
          <Button variant="outline" autoFocus disabled={undoing} onClick={() => setConfirming(false)}>{t("schedule.cancel")}</Button>
          <Button variant="destructive" data-program-undo-confirm disabled={undoing || !counts || nothing} onClick={() => void undoAll()}>{undoing ? <Spinner /> : null}{t("programs.undoAll.confirm")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>;
}
