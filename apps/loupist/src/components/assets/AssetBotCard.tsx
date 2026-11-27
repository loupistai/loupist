import { useEffect, useState } from "react";
import { useTranslation } from "@/i18n";
import type { LoupistBridge } from "../../types";
import type { WatchPreset } from "../../contracts/generated";
import { useBotScheduleTasks } from "../chat/BotScheduleTasks";
import { useUsageSummary } from "../settings/UsageSettings";
import { Switch } from "../ui/switch";
import { Button } from "../ui/button";
import { useGo } from "../navigation/Open";
import { ASSET_DAILY_PRESET, cadenceLabel, learningRowOutcome, otherLearningPrograms, presetRunsOnCheck, trackDailyState, trackDailyTask } from "../briefings/scheduledPresentation";

export function AssetBotCard({ bridge, loupistId, assetId, hasLines, name, followed: followedProp }: {
  bridge: LoupistBridge;
  loupistId: string;
  assetId: string;
  hasLines: boolean;
  /** The page already knows an unfollowed asset; the followed list does not include it. */
  name?: string;
  followed?: boolean;
}) {
  const { t } = useTranslation("briefings");
  const go = useGo();
  const { tasks, refresh } = useBotScheduleTasks(bridge, loupistId, assetId);
  const learningRunning = useUsageSummary(bridge, 1, true)?.allowance.learningRunning === true;
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState(false);
  const [asset, setAsset] = useState<{ name: string; followed: boolean } | null>(null);
  useEffect(() => {
    let live = true;
    void bridge.assetsList({ loupistId }).then(rows => {
      const row = rows.find(item => item.assetId === assetId);
      if (live) setAsset(row ? { name: row.name, followed: row.followedAtMs !== null } : { name: "", followed: true });
    }).catch(() => { if (live) setAsset({ name: "", followed: true }); });
    return () => { live = false; };
  }, [bridge, loupistId, assetId]);
  const subjectTasks = [...(tasks?.values() ?? [])].filter(task =>
    task.subjects?.some(subject => "asset" in subject && subject.asset === assetId));
  const daily = trackDailyTask(subjectTasks, assetId);
  const dailyState = trackDailyState(daily);
  const followed = followedProp ?? asset?.followed !== false;
  const assetName = name || (asset == null ? null : asset.name || t("schedule.learning.thisAsset"));
  const others = otherLearningPrograms(subjectTasks, assetId, daily?.id);
  const dailyOutcome = daily ? learningRowOutcome(daily, learningRunning) : null;
  const presets: { preset: WatchPreset; label: string }[] = [
    { preset: "filings-and-results", label: t("schedule.filings") },
    { preset: "price-move", label: t("schedule.priceMoves") },
    { preset: "fact-changes", label: t("schedule.factChanges") },
    ...(hasLines ? [{ preset: "thesis-lines" as const, label: t("schedule.lines") }] : []),
  ];

  return <section className="asset-card asset-bot-card" aria-label={t("schedule.botOnAsset")}>
    <h2 className="asset-card-title">{t("schedule.botOnAsset")}</h2>
    {presets.map(({ preset, label }) => {
      const task = subjectTasks.find(candidate => candidate.preset === preset && candidate.subjects?.length === 1);
      return <div className="asset-bot-watch" key={preset}>
        <div><strong>{label}</strong>{task && <div className="briefings-meta">{cadenceLabel(task.schedule)}</div>}
          {presetRunsOnCheck(preset) ? <div className="briefings-meta">{t("schedule.check.noModelUntilMet")}</div> : null}</div>
        <Switch aria-label={label} checked={task?.status === "active"} disabled={!tasks || busy !== null || task?.awaiting_confirmation}
          onCheckedChange={active => {
            setBusy(preset); setError(false);
            const save = task ? bridge.scheduledTaskSetStatus({
              loupist_id: loupistId, task_id: task.id, expected_revision: task.revision, status: active ? "active" : "paused",
            }) : bridge.scheduledTaskFromPreset({ loupistId, preset, subjects: [{ asset: assetId }] });
            void save.then(refresh, () => setError(true)).finally(() => setBusy(null));
          }} />
        </div>;
    })}
    <div className="asset-bot-watch asset-track-daily" data-track-daily={dailyState} data-followed={followed ? "true" : "false"}>
      <div>
        <strong>{t("schedule.learning.trackDaily")}</strong>
        {assetName ? <div className="briefings-meta">{followed ? t("schedule.learning.trackDailyLine", { name: assetName }) : t("schedule.learning.followToTrack", { name: assetName })}</div> : null}
        {followed && dailyOutcome ? <div className="briefings-meta" data-learning-outcome>{dailyOutcome}</div> : null}
      </div>
      <Switch aria-label={t("schedule.learning.trackDaily")} checked={dailyState !== "off"} disabled={!tasks || !followed || busy !== null || Boolean(daily?.awaiting_confirmation)}
        onCheckedChange={active => {
          setBusy(ASSET_DAILY_PRESET); setError(false);
          // An existing program resumes or pauses in place. The first turn-on asks the host for the asset-daily preset.
          const save = daily ? bridge.scheduledTaskSetStatus({
            loupist_id: loupistId, task_id: daily.id, expected_revision: daily.revision, status: active ? "active" : "paused",
          }) : bridge.scheduledTaskFromPreset({ loupistId, preset: ASSET_DAILY_PRESET, subjects: [{ asset: assetId }] });
          void save.then(refresh, () => setError(true)).finally(() => setBusy(null));
        }} />
    </div>
    {others.map(program => {
      const outcome = learningRowOutcome(program, learningRunning);
      return <p className="briefings-meta asset-also-learned" key={program.id}>
        {t("schedule.learning.alsoLearned", { program: program.title, outcome: outcome ?? t(`schedule.status.${program.status}`) })}
        {go ? <> · <Button variant="link" size="sm" onClick={() => go({ kind: "schedule", taskId: program.id })}>{t("schedule.open")}</Button></> : null}
      </p>;
    })}
    {subjectTasks.filter(task => task.kind !== "learning" && (task.preset === "results-follow-up" || task.preset === "week-in-review" || !task.preset || (task.subjects?.length ?? 0) > 1)).map(task =>
      <div className="asset-bot-watch" key={task.id}>
        <div><strong>{task.title}</strong><div className="briefings-meta">{cadenceLabel(task.schedule)}</div></div>
        {task.awaiting_confirmation && (!task.trigger || task.replaces) && (task.schedule.kind !== "once" || task.schedule.at_ms > Date.now())
          ? <Button size="sm" disabled={busy !== null} onClick={() => {
            setBusy("results-follow-up"); setError(false);
            void bridge.scheduledTaskConfirm({ loupist_id: loupistId, task_id: task.id, expected_revision: task.revision })
              .then(refresh, () => setError(true)).finally(() => setBusy(null));
          }}>{t("schedule.turnOn")}</Button>
          : go && <Button size="sm" variant="ghost" onClick={() => go({ kind: "schedule", taskId: task.id })}>{t("schedule.open")}</Button>}
      </div>)}
    {presets.some(item => !presetRunsOnCheck(item.preset)) ? <p className="briefings-meta">{t("schedule.watchCost")}</p> : null}
    {error && <p role="alert" className="inline-error">{t("schedule.statusFailed")}</p>}
  </section>;
}
