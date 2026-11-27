import { useEffect, useState } from "react";
import { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { errorMessage } from "../../lib/errors";
import { assetTicker } from "../../contracts/assets";
import type { ScheduleReceipt, ScheduledTask } from "../../types";
import { useCurrentVisit } from "../../app/loupistVisit";
import { useAssetNames } from "../assets/assetName";
import { Button } from "../ui/button";
import { useChatExecution } from "./ChatExecutionContext";
import {
  learningBudgetText, learningCapReached, learningCostNote, learningPresetText, learningDeliveryText, learningDepthText,
  learningKeepsText, learningReportText, learningScopeText, subjectChipModel,
} from "../briefings/scheduledPresentation";

type TaskReceipt = Exclude<ScheduleReceipt, { question: string }>;

/**
 * A learning program proposed in chat. Turn on confirms that revision. Not now
 * declines it for this visit. The same card reads the task again after a reload.
 */
export function LearningProgramCard({ receipt, onOpen }: {
  receipt: TaskReceipt;
  onOpen?: (taskId: string) => void;
}) {
  const { t } = useTranslation("chat");
  const { t: schedule } = useTranslation("briefings");
  const scope = useChatExecution();
  const visit = useCurrentVisit(scope?.loupistId ?? "");
  const nameOf = useAssetNames(scope?.bridge, scope?.loupistId ?? "", Boolean(receipt.subjects?.length));
  const [task, setTask] = useState<ScheduledTask | null | undefined>(undefined);
  const [names, setNames] = useState<ReadonlyMap<string, string>>(new Map());
  const [expanded, setExpanded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<"cap" | "replaced" | null>(null);
  const [cap, setCap] = useState(0);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!scope) return undefined;
    let live = true;
    void scope.bridge.scheduledTaskGet({ loupist_id: scope.loupistId, task_id: receipt.task_id })
      .then(found => { if (live) setTask(found); })
      .catch(() => { if (live) setFailed(true); });
    return () => { live = false; };
  }, [scope, receipt.task_id]);

  useEffect(() => {
    if (!scope) return undefined;
    let live = true;
    void scope.bridge.assetsList({ loupistId: scope.loupistId }).then(rows => {
      if (!live) return;
      setNames(new Map(rows.map(row => [row.assetId, nameOf({
        name: row.name, assetId: row.assetId, symbol: assetTicker(row) ?? undefined, market: row.reference?.exchange,
      })])));
    }).catch(() => undefined);
    return () => { live = false; };
  }, [scope, nameOf]);

  const program = task?.program ?? receipt.program;
  const subjects = task?.subjects ?? receipt.subjects;
  const preset = learningPresetText(program?.preset);
  const brief = task?.prompt?.trim() ?? "";
  const long = brief.split("\n").length > 3 || brief.length > 180;
  const chips = subjectChipModel(subjects, names, 3);
  const gone = task === null;
  const replaced = notice === "replaced" || (task != null && task.awaiting_confirmation && task.revision !== receipt.revision);
  const declined = task?.declined_in_visit !== undefined;
  const turnedOn = task != null && !task.awaiting_confirmation && task.status === "active" && !replaced;
  const fields = program ? [
    [schedule("schedule.learning.depthLabel"), learningDepthText(program.depth)],
    [schedule("schedule.learning.sources"), learningScopeText(program.scope)],
    [schedule("schedule.learning.keeps"), learningKeepsText(program.keeps)],
    [schedule("schedule.learning.reports"), learningReportText(task?.report ?? receipt.report)],
    [schedule("schedule.learning.delivers"), learningDeliveryText(task?.delivery?.to ?? receipt.delivery?.to)],
  ].filter((row): row is [string, string] => Boolean(row[1])) : [];
  const budget = learningBudgetText(task?.budget ?? receipt.budget);
  const status = gone ? t("messageSchedules.deleted")
    : replaced ? t("messageSchedules.learningProgram.replaced")
    : notice === "cap" ? t("messageSchedules.learningProgram.cap", { count: cap, formattedCount: formatNumber(cap) })
    : turnedOn ? t("messageSchedules.learningProgram.on")
    : declined ? t("messageSchedules.learningProgram.notNow")
    : null;
  const showActions = !gone && !replaced && !turnedOn && !declined && notice !== "cap";

  const refresh = async () => {
    if (!scope) return;
    const found = await scope.bridge.scheduledTaskGet({ loupist_id: scope.loupistId, task_id: receipt.task_id }).catch(() => undefined);
    if (found !== undefined) setTask(found);
  };

  const turnOn = async () => {
    if (!scope || busy) return;
    setBusy(true); setFailed(false); setNotice(null);
    try {
      const next = await scope.bridge.scheduledTaskConfirm({ loupist_id: scope.loupistId, task_id: receipt.task_id, expected_revision: task?.revision ?? receipt.revision });
      setTask(next);
    } catch (cause) {
      const reached = learningCapReached(errorMessage(cause, ""));
      if (reached !== null) { setCap(reached); setNotice("cap"); }
      else setFailed(true);
      await refresh();
      if (reached === null) {
        const found = await scope.bridge.scheduledTaskGet({ loupist_id: scope.loupistId, task_id: receipt.task_id }).catch(() => undefined);
        if (found && found.revision !== receipt.revision) setNotice("replaced");
      }
    } finally { setBusy(false); }
  };

  const notNow = async () => {
    if (!scope || busy) return;
    setBusy(true); setFailed(false);
    try {
      await scope.bridge.scheduledProposalSetDeclined({
        loupist_id: scope.loupistId, task_id: receipt.task_id, visit: visit?.openedAtMs ?? Date.now(), declined: true,
      });
      await refresh();
    } catch { setFailed(true); }
    finally { setBusy(false); }
  };

  return <article className="message-schedule learning-program-card" aria-label={t("messageSchedules.taskLabel", { title: receipt.title })} data-learning-proposal={gone ? "deleted" : replaced ? "replaced" : notice ?? (turnedOn ? "on" : declined ? "paused" : "awaiting")}>
    <div className={`message-schedule-open ${gone || replaced ? "message-schedule-gone" : ""}`}>
      <span className="message-schedule-summary">
        <span className="learning-program-kicker">{preset ? t("messageSchedules.learningProgram.kickerPreset", { preset }) : t("messageSchedules.learningProgram.kicker")}</span>
        <span className="message-schedule-heading"><strong title={receipt.title}>{receipt.title}</strong>{status ? <span>{status}</span> : null}</span>
      </span>
      {onOpen && (gone || replaced || turnedOn || declined || notice === "cap") ? <Button variant="link" size="sm" onClick={() => onOpen(receipt.task_id)}>{schedule("schedule.open")}</Button> : null}
    </div>
    {program && !gone && !replaced ? <>
      {brief ? <section aria-label={t("messageSchedules.learningProgram.what")}>
        <h3 className="schedule-section-label">{t("messageSchedules.learningProgram.what")}</h3>
        <p className="schedule-learn-brief" data-lines="3" data-expanded={expanded || undefined}>{brief}</p>
        {long && !expanded ? <Button variant="link" size="sm" onClick={() => setExpanded(true)}>{schedule("schedule.showAll")}</Button> : null}
      </section> : null}
      {chips.shown.length ? <section aria-label={schedule("schedule.learning.about")}>
        <h3 className="schedule-section-label">{schedule("schedule.learning.about")}</h3>
        <ul className="schedule-subject-chips">{chips.shown.map(chip => <li key={chip.key}><span className="schedule-subject-chip">{chip.label}</span></li>)}
          {chips.extra > 0 ? <li className="schedule-subject-chip">{schedule("schedule.learning.moreSubjects", { formattedCount: formatNumber(chips.extra) })}</li> : null}
        </ul>
      </section> : null}
      <dl className="schedule-learning-fields">
        <div><dt>{schedule("schedule.learning.when")}</dt><dd>{t("messageSchedules.learningProgram.when")}</dd></div>
        {fields.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}
        {budget ? <div><dt>{schedule("schedule.learning.budgetLabel")}</dt><dd>{budget}</dd></div> : null}
      </dl>
      <p className="message-schedule-spending">{learningCostNote(task?.budget ?? receipt.budget)}</p>
    </> : null}
    {showActions && scope ? <div className="message-schedule-actions">
      <Button type="button" size="sm" disabled={busy} onClick={() => void turnOn()}>{t("messageSchedules.turnOn")}</Button>
      <Button type="button" size="sm" variant="ghost" disabled={busy} onClick={() => void notNow()}>{schedule("standing.notNow")}</Button>
      {failed ? <span role="alert">{t("messageSchedules.couldNotChange")} <Button variant="ghost" size="sm" onClick={() => void refresh()}>{schedule("retry")}</Button></span> : null}
    </div> : null}
  </article>;
}
