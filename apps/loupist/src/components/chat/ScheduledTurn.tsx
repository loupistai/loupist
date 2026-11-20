import i18n, { useTranslation } from "@/i18n";
import { useState } from "react";
import { CalendarClockIcon } from "lucide-react";
import { formatTime } from "@/i18n/format";
import { Disclosure } from "../ui/disclosure";
import { Spinner } from "../ui/spinner";
import { useGo } from "../navigation/Open";
import { scheduledTaskTitle } from "../briefings/scheduledPresentation";
import type { ChatMessage, LoupistBridge, ScheduledTurn as ScheduledRequest } from "../../types";

const t = i18n.getFixedT(null, "chat");
const tSchedule = i18n.getFixedT(null, "briefings");

/**
 * The request a scheduled run sent into its session. The user did not write it
 * then, so it names the task instead of standing as their bubble. A custom
 * task's saved prompt is behind Request; the briefing's request is the host's,
 * written for the model, and is left out.
 */
export function ScheduledTurn({ message }: { message: ChatMessage }) {
  useTranslation("chat");
  const turn = message.scheduled!;
  const title = scheduledTaskTitle(turn.task_kind, turn.task_title);
  return <div data-message-id={message.id} className="chat-scheduled-turn" data-task-kind={turn.task_kind}>
    <p className="chat-guiding-row">
      <CalendarClockIcon aria-hidden="true" />
      <span>{t("scheduledTurn.ran", { title })}</span>
      <time dateTime={message.created_at}>{formatTime(new Date(message.created_at))}</time>
    </p>
    {turn.task_kind === "briefing" ? null : <Disclosure className="chat-scheduled-request" title={t("scheduledTurn.request")}>
      <p>{message.content}</p>
    </Disclosure>}
  </div>;
}

/**
 * A scheduled run that did not finish runs again through its task, as the
 * Schedule reader offers it: sent again here, its request would run as the
 * user's own turn. Once started, the task opens in Schedule, where the new run
 * appears.
 */
export function RunScheduledAgain({ bridge, loupistId, request }: { bridge: LoupistBridge; loupistId: string; request: ScheduledRequest }) {
  useTranslation("briefings");
  const go = useGo();
  const [state, setState] = useState<"starting" | "failed" | null>(null);
  const run = async () => {
    setState("starting");
    try {
      await bridge.scheduledTaskRunNow({ loupist_id: loupistId, task_id: request.task_id });
      setState(null);
      go?.({ kind: "schedule", taskId: request.task_id });
    } catch { setState("failed"); }
  };
  return <>
    <button type="button" data-scheduled-run-again disabled={state === "starting"} title={tSchedule("schedule.runNowCost")} onClick={() => void run()}>
      {state === "starting" ? <Spinner /> : null}{tSchedule("schedule.runNow")}
    </button>
    {state === "failed" ? <p className="inline-error" role="alert">{tSchedule("schedule.runNowFailed")}</p> : null}
  </>;
}
