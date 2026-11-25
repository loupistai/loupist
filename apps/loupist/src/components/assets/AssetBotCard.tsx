import { useState } from "react";
import { useTranslation } from "@/i18n";
import type { LoupistBridge } from "../../types";
import type { WatchPreset } from "../../contracts/generated";
import { useBotScheduleTasks } from "../chat/BotScheduleTasks";
import { Switch } from "../ui/switch";
import { Button } from "../ui/button";
import { useGo } from "../navigation/Open";
import { cadenceLabel } from "../briefings/scheduledPresentation";

export function AssetBotCard({ bridge, loupistId, assetId, hasLines }: {
  bridge: LoupistBridge;
  loupistId: string;
  assetId: string;
  hasLines: boolean;
}) {
  const { t } = useTranslation("briefings");
  const go = useGo();
  const { tasks, refresh } = useBotScheduleTasks(bridge, loupistId, assetId);
  const [busy, setBusy] = useState<WatchPreset | null>(null);
  const [error, setError] = useState(false);
  const subjectTasks = [...(tasks?.values() ?? [])].filter(task =>
    task.subjects?.some(subject => "asset" in subject && subject.asset === assetId));
  const presets: { preset: WatchPreset; label: string }[] = [
    { preset: "filings-and-results", label: t("schedule.filings") },
    { preset: "price-move", label: t("schedule.priceMoves") },
    ...(hasLines ? [{ preset: "thesis-lines" as const, label: t("schedule.lines") }] : []),
  ];

  return <section className="asset-card asset-bot-card" aria-label={t("schedule.botOnAsset")}>
    <h2 className="asset-card-title">{t("schedule.botOnAsset")}</h2>
    {presets.map(({ preset, label }) => {
      const task = subjectTasks.find(candidate => candidate.preset === preset && candidate.subjects?.length === 1);
      return <div className="asset-bot-watch" key={preset}>
        <div><strong>{label}</strong>{task && <div className="briefings-meta">{cadenceLabel(task.schedule)}</div>}</div>
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
    {subjectTasks.filter(task => task.preset === "results-follow-up" || task.preset === "week-in-review" || !task.preset || (task.subjects?.length ?? 0) > 1).map(task =>
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
    <p className="briefings-meta">{t("schedule.watchCost")}</p>
    {error && <p role="alert" className="inline-error">{t("schedule.statusFailed")}</p>}
  </section>;
}
