import { elapsedLabel, useNow } from "../../chat/runStatus";
import { useTranslation } from "@/i18n";
import { formatTime } from "@/i18n/format";
import type { ScheduledTaskListItem } from "../../types";
import { scheduledTaskTitle, unreadableTask } from "../briefings/scheduledPresentation";
import { useGo } from "../navigation/Open";
import { LoupistAvatar } from "../app/LoupistAvatar";
import type { LoupistAvatarIdentity } from "../../lib/loupistAvatar";

export function BotStandingStrip({ tasks, accountConnected, loupist }: {
  tasks?: ReadonlyMap<string, ScheduledTaskListItem>;
  accountConnected?: boolean;
  loupist: LoupistAvatarIdentity;
}) {
  const { t } = useTranslation("briefings");
  const go = useGo();
  const rows = [...(tasks?.values() ?? [])];
  const active = rows.filter(task => task.status === "active");
  const watches = active.filter(task => task.report === "when_met" || task.preset);
  const briefing = active.find(task => task.kind === "briefing");
  const running = rows.find(task => task.latest_run?.status === "running");
  const offline = rows.find(task => task.latest_run?.error === "unreachable");
  const paused = rows.filter(task => task.status === "paused" && task.last_error && !unreadableTask(task));
  const failures = rows.filter(task => task.latest_run?.failure_streak);
  const day = new Date().toDateString();
  const checkedToday = rows.filter(task => task.kind === "custom" && task.latest_run && new Date(task.latest_run.started_at_ms).toDateString() === day);
  const quiet = checkedToday.filter(task => task.latest_run?.quiet && task.latest_run.status === "succeeded");
  const quietToday = quiet.length > 0 && quiet.length === checkedToday.length;
  const latest = running?.latest_run;
  const now = useNow(Boolean(latest));
  const first = running ?? offline ?? paused[0] ?? failures[0] ?? quiet[0] ?? watches[0] ?? briefing ?? active[0];
  if (!tasks && accountConnected !== false) return null;
  const details = [
    watches.length > 0 && t("standing.watches", { count: watches.length }),
    briefing?.next_run_at_ms != null && t("standing.briefing", { time: formatTime(briefing.next_run_at_ms) }),
    paused.length > 0 && t("standing.paused", { count: paused.length }),
    failures.length > 0 && t("standing.failed", { count: failures.length }),
  ].filter(Boolean).join(" · ");
  const line = accountConnected === false ? t("standing.account")
    : running ? t("standing.running", { title: scheduledTaskTitle(running.kind, running.title), elapsed: elapsedLabel(Math.max(0, now - (latest?.started_at_ms ?? now))) })
    : offline?.latest_run ? t("standing.offline", { time: formatTime(offline.latest_run.scheduled_for_ms) })
    : quietToday && !failures.length && !paused.length ? t("standing.quiet", { count: quiet.length })
    : details || (active.length ? t("schedule.summary", { count: active.length, formattedCount: active.length }) : t("standing.empty"));
  return <div className="bot-standing-strip" data-warning={Boolean(offline || paused.length || failures.length) || undefined}>
    {running && <LoupistAvatar loupist={loupist} size={18} busy />}
    <span role="status">{line}</span>
    {first && go && <button type="button" className="morning-link" onClick={() => {
      if (latest) go({ kind: "run", taskId: first.id, runId: latest.run_id });
      else go({ kind: "schedule", taskId: first.id });
    }}>{latest ? t("standing.openRun") : t("standing.openSchedule")}</button>}
  </div>;
}
