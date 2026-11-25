import { useState } from "react";
import { useTranslation } from "@/i18n";
import type { TaskTrigger, TriggerActiveHours } from "../../contracts/generated";
import type { LoupistBridge, ScheduledCadence } from "../../types";
import { Button } from "../ui/button";

/** Proposal, update and code for a check. Turn on pins the hash the host will run. */
type TriggerCardProps = {
  trigger: TaskTrigger;
  bridge: LoupistBridge;
  loupistId: string;
  taskId: string;
  revision: number;
  schedule: ScheduledCadence;
  prompt?: string;
  onChanged?: () => void;
  onChangeDraft?: (text: string) => void;
};

export function TriggerCard(props: TriggerCardProps) {
  const displayed = props.trigger.pending ?? props.trigger.script;
  return <TriggerReview key={`${props.loupistId}:${props.taskId}:${props.revision}:${displayed.version}:${displayed.sha256}`} {...props} />;
}

// Estimate the reads on a day when this check actually runs.
function projectedDailyReads(schedule: ScheduledCadence, reads: number, hours?: TriggerActiveHours) {
  if (schedule.kind !== "interval") return reads;
  const day = 24 * 60;
  const minutes = (time: string) => {
    const [hour, minute] = time.split(":").map(Number);
    return hour * 60 + minute;
  };
  const active = hours ? (day + minutes(hours.end) - minutes(hours.start)) % day : day;
  return Math.max(Math.ceil(active / schedule.every_minutes), 1) * reads;
}

function TriggerReview({ trigger, bridge, loupistId, taskId, revision, schedule, prompt, onChanged, onChangeDraft }: TriggerCardProps) {
  const { t } = useTranslation(["briefings", "knowledge"]);
  const [code, setCode] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const [tested, setTested] = useState<string | null>(null);
  const dry = trigger.pending ? trigger.pending.test : trigger.proposal_test;
  const configuration = trigger.pending?.configuration ?? (!trigger.pending ? trigger : undefined);
  const summary = configuration?.summary ?? trigger.summary;
  const args = Object.entries(configuration?.args ?? {});
  const version = trigger.pending?.version ?? trigger.script.version;
  const sha = trigger.pending?.sha256 ?? trigger.script.sha256;
  const waiting = trigger.state === "proposed" || Boolean(trigger.pending);
  const canApprove = dry?.scores && !dry.error && (dry.today_outcome === "met" || dry.today_outcome === "not_met")
    && (!trigger.pending || trigger.pending.configuration);
  const act = async (action: "approve" | "test" | "code" | "revert") => {
    const review = { loupist_id: loupistId, task_id: taskId, expected_revision: revision, version, sha256: sha };
    setBusy(true);
    setFailed(false);
    try {
      if (action === "code") {
        const view = await bridge.scheduledTriggerGet({ loupist_id: loupistId, task_id: taskId, version });
        if (view.version !== version || view.sha256 !== sha) throw new Error();
        setCode(view.code);
      } else if (action === "test") {
        const result = await bridge.scheduledTriggerTest(review);
        if (result.version !== version || result.sha256 !== sha) throw new Error();
        setTested(result.reading || result.error || result.outcome);
        onChanged?.();
      } else if (action === "revert") {
        const prior = trigger.approved_versions.at(-2) ?? trigger.approved_versions.at(-1);
        if (!prior) return;
        await bridge.scheduledTriggerRevert({ loupistId, taskId, toVersion: prior.version });
        onChanged?.();
      } else {
        await bridge.scheduledTriggerApprove(review);
        onChanged?.();
      }
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  };
  const savedScores = dry?.scores;
  const baseline = savedScores && "baseline" in savedScores ? savedScores.baseline : undefined;
  // Older comparisons pooled past pins. A saved score must name the approved
  // pin still shown beside the pending candidate.
  const scores = trigger.pending && trigger.approved && baseline && typeof baseline === "object"
    && "version" in baseline && baseline.version === trigger.approved.version
    && "sha256" in baseline && baseline.sha256 === trigger.approved.sha256
    ? savedScores : undefined;
  const notComparable = scores ? scores.days.length - scores.compared : 0;
  return <div className="schedule-delivery" data-trigger-state={trigger.state}>
    <div><dt>{t("schedule.checks")}</dt><dd>{summary}</dd></div>
    {args.length > 0 && <div><dt>{t("knowledge:scripts.args")}</dt><dd><dl>{args.map(([key, value]) =>
      <div key={key}><dt>{key}</dt><dd><code>{JSON.stringify(value)}</code></dd></div>)}</dl></dd></div>}
    <div><dt>{t("schedule.check.reads")}</dt><dd>{configuration?.tools.map(tool => tool.provider ? `${tool.name} (${tool.provider})` : tool.name).join(", ") || "—"}</dd></div>
    {configuration && configuration.reads_per_check > 0 && <div><dt>{t("schedule.check.runs")}</dt><dd>{t(schedule.kind === "weekly" || configuration.active_hours ? "schedule.check.runDayReads" : "schedule.check.dailyReads", { count: projectedDailyReads(schedule, configuration.reads_per_check, configuration.active_hours) })}</dd></div>}
    {dry && <div><dt>{t("schedule.check.dryRun")}</dt><dd>
      {dry.error ?? t(dry.today_met ? "schedule.check.todayMet" : "schedule.check.todayNot", { reading: dry.today_reading })}
      {dry.unreplayed > 0 ? ` ${t("schedule.check.unreplayed", { count: dry.unreplayed })}` : ""}
      {dry.fired_on.length > 0 ? ` ${t("schedule.check.firedOn", { count: dry.fired_on.length, replayed: dry.replayed })}` : ""}
      {dry.days.map(day => <span key={day.date}> {day.date}: {day.reading}</span>)}
    </dd></div>}
    {scores && scores.compared > 0 && <p>{t("schedule.check.scores", { previousMet: scores.previous_met, nextMet: scores.next_met, compared: scores.compared })}</p>}
    {notComparable > 0 && <p>{t("schedule.check.notComparable", { count: notComparable })}</p>}
    {prompt && <div><dt>{t("schedule.check.whenMet")}</dt><dd>{prompt}</dd></div>}
    <p>{t("schedule.check.cost")}</p>
    {trigger.state === "failing" && trigger.last?.error && <p role="status">{t("schedule.check.stopped", { title: trigger.summary, error: trigger.last.error })}</p>}
    {trigger.last?.outcome === "no_data" && <p role="status">{t("schedule.check.couldNotRead", { time: new Date(trigger.last.at_ms).toLocaleTimeString() })}</p>}
    {trigger.last?.outcome === "rate_limited" && <p role="status">{t("schedule.check.waitingRate")}</p>}
    {trigger.last?.outcome === "stale" && <p role="status">{t("schedule.check.stale", { date: trigger.last.as_of ?? "" })}</p>}
    {tested && <p role="status">{tested}</p>}
    {code !== null && <pre data-trigger-code>{code}</pre>}
    <div className="message-schedule-actions">
      {waiting && <Button size="sm" disabled={busy || !canApprove} onClick={() => void act("approve")}>{trigger.pending ? t("schedule.check.update", { version }) : t("schedule.turnOn")}</Button>}
      {onChangeDraft && <Button size="sm" variant="ghost" onClick={() => onChangeDraft(t("schedule.changePrompt", { title: summary }))}>{t("schedule.check.change")}</Button>}
      <Button size="sm" variant="ghost" disabled={busy} onClick={() => void act("test")}>{t("schedule.check.testNow")}</Button>
      <Button size="sm" variant="ghost" disabled={busy} onClick={() => void act("code")}>{t("schedule.check.viewCode")}</Button>
      {trigger.approved_versions.length > 1 && <Button size="sm" variant="ghost" disabled={busy} onClick={() => void act("revert")}>{t("schedule.check.revert")}</Button>}
    </div>
    {failed && <p role="alert">{t("schedule.statusFailed")}</p>}
  </div>;
}
