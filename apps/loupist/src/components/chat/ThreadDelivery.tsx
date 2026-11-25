import { useContext, useState } from "react";
import { useChatExecution } from "./ChatExecutionContext";
import { BotScheduleTasks } from "./BotScheduleTasks";
import { SavedScripts } from "./SavedScripts";
import i18n, { useTranslation } from "@/i18n";
import { useOpen } from "../navigation/Open";
import { RunRecordRemoved } from "../../chat/navigation";
import type { ChatMessage } from "../../types";
import { formatDateTime } from "@/i18n/format";
import { scheduledTaskTitle, skippedSavedKeyLabel, skippedToolNames } from "../briefings/scheduledPresentation";
import { BotPostBody } from "./BotPost";
import { briefingFailure } from "../briefings/briefingFailure";
import { MemorySaveWarning } from "./ChatReceipt";
import { ContextTitle, titlePlaceholder } from "../ui/context-title";
import "@/styles/bot-cards.css";

const t = i18n.getFixedT(null, "chat");

/** Bot's copy of a Schedule result opens the original run reader. */
export function ThreadDelivery({ message, loupistName, onAsk, onFeedback }: {
  message: ChatMessage;
  loupistName: string;
  onAsk?: (text: string) => void;
  onFeedback?: (text: string) => void;
}) {
  useTranslation("chat");
  const open = useOpen();
  const [removed, setRemoved] = useState(false);
  const delivery = message.delivery!;
  const scope = useChatExecution();
  const { tasks, refresh } = useContext(BotScheduleTasks);
  const task = tasks?.get(delivery.task_id);
  const [error, setError] = useState(false);
  const [feedback, setFeedback] = useState(false);
  const [busy, setBusy] = useState(false);
  const change = async (action: "notify" | "pause" | "run") => {
    if (!scope || !task || busy) return;
    setBusy(true); setError(false);
    try {
      if (action === "notify") await scope.bridge.scheduledTaskSetDelivery({
        loupistId: task.loupist_id, taskId: task.id, expectedRevision: task.revision,
        notify: task.delivery?.notify === "digest" ? "now" : "digest",
      });
      else {
        const identity = { loupist_id: task.loupist_id, task_id: task.id };
        if (action === "pause") await scope.bridge.scheduledTaskSetStatus({ ...identity, expected_revision: task.revision, status: "paused" });
        else await scope.bridge.scheduledTaskRunNow(identity);
      }
      refresh?.();
    } catch { setError(true); }
    finally { setBusy(false); }
  };
  const openResult = () => void open?.({ kind: "run", runId: delivery.run_id, taskId: delivery.task_id }).catch((failure: unknown) => { if (failure instanceof RunRecordRemoved) setRemoved(true); });
  const skippedSavedKeys = skippedSavedKeyLabel(delivery.skipped_saved_keys);
  const finished = delivery.status === "succeeded";
  const occurrence = task?.latest_run?.run_id === delivery.run_id ? task.latest_run : undefined;
  const failure = briefingFailure(occurrence?.error);
  const skipped = skippedToolNames(occurrence?.skipped_tools ?? []).ordinary;
  const title = scheduledTaskTitle(delivery.task_kind, delivery.task_title);
  const origin = t("thread.scheduleOrigin", { title: titlePlaceholder, time: formatDateTime(delivery.finished_at_ms) });
  return <article className="message assistant thread-delivery" data-bot-post="" data-delivery-run={delivery.run_id} data-status={delivery.status} aria-label={t("thread.deliveryLabel", { title })}>
    <BotPostBody loupistName={loupistName} at={delivery.finished_at_ms}>
      <div className="bot-card">
        <div className="bot-card-kicker"><span className="bot-card-tag" data-tone={finished ? undefined : "warning"}><ContextTitle template={origin} title={title} /></span></div>
        {delivery.trigger && <p className="thread-delivery-source" data-trigger-reading>{i18n.t("briefings:schedule.check.fromYourCheck")}: {delivery.trigger.reading}
          {delivery.trigger.as_of ? ` · ${delivery.trigger.as_of}` : ""}
          {delivery.trigger.sources.length > 0 ? ` · ${delivery.trigger.sources.join(", ")}` : ""}
          {delivery.trigger.coalesced ? ` · ${i18n.t("briefings:schedule.check.checkedAfterClosed", { time: formatDateTime(delivery.trigger.checked_at_ms) })}` : ""}
          {!delivery.trigger.explained && delivery.trigger.reason ? ` · ${i18n.t("briefings:schedule.check.noExplanation", { reason: delivery.trigger.reason })}` : ""}
        </p>}
        <p className="thread-delivery-text">{finished ? message.content : t("thread.couldNotFinish")}</p>
        {!finished && failure !== "generic" && <p className="thread-delivery-source">{i18n.t(`briefings:failure.${failure}`)}</p>}
        {!finished && task?.next_run_at_ms != null && <p className="thread-delivery-source">{t("messageSchedules.nextRun", { date: formatDateTime(task.next_run_at_ms) })}</p>}
        {skipped.length > 0 && <p className="thread-delivery-source">{i18n.t("briefings:skipped", { names: skipped.join(", ") })}</p>}
        {delivery.firstRun && <p className="thread-delivery-source">{t("thread.firstCheck")}</p>}
        {delivery.evidence && <p className="thread-delivery-source">
          <button type="button" className="morning-link" disabled={!open || removed} onClick={openResult}>
            {t("thread.resultEvidence", { figures: delivery.evidence.figuresChecked, sources: delivery.evidence.sources })}
          </button>
          {delivery.evidence.figuresUnmatched > 0 && <span data-tone="warning"> · {t("thread.unmatchedFigures", { count: delivery.evidence.figuresUnmatched })}</span>}
        </p>}
        {delivery.failure_streak && <p className="thread-delivery-source">{t("thread.failureStreak", { count: delivery.failure_streak.count })}</p>}
        {skippedSavedKeys && <p className="thread-delivery-source">{skippedSavedKeys}</p>}
        <MemorySaveWarning reason={delivery.memory_save_skip} items={delivery.discarded_memory_items} />
        {delivery.learned?.scripts?.length
          ? <SavedScripts scripts={delivery.learned.scripts} bridge={scope?.bridge} loupistId={scope?.loupistId ?? ""} />
          : null}
        {open ? <div className="bot-card-foot">
          {!finished && task && <button type="button" className="morning-link" disabled={busy || task.latest_run?.status === "running"} onClick={() => void change("run")}>{i18n.t("briefings:schedule.runNow")}</button>}
          {finished && delivery.run_thread_id && <button type="button" className="morning-link" onClick={() => void open({ kind: "chat", threadId: delivery.run_thread_id! })}>{t("thread.askAboutResult")}</button>}
          {finished && task && scope && <details className="bot-result-menu">
            <summary>{t("thread.moreResultActions")}</summary>
            <button type="button" className="morning-link" disabled={busy} onClick={() => void change("notify")}>{t(task.delivery?.notify === "digest" ? "thread.notifyNow" : "thread.notifyQuietly")}</button>
            {task.status === "active" && <button type="button" className="morning-link" disabled={busy} onClick={() => void change("pause")}>{i18n.t("briefings:schedule.pause")}</button>}
            {onAsk && <button type="button" className="morning-link" onClick={() => onAsk(i18n.t("briefings:schedule.changePrompt", { title }))}>{i18n.t("briefings:schedule.changeAsk")}</button>}
            {onFeedback && <button type="button" className="morning-link" aria-expanded={feedback} onClick={() => setFeedback(!feedback)}>{t("thread.notUseful")}</button>}
            {feedback && onFeedback && (["tooMinor", "knew", "wrong", "stop"] as const).map(key => <button type="button" className="morning-link" key={key} onClick={() => { onFeedback(t(`thread.resultFeedback.${key}`, { title })); setFeedback(false); }}>{t(`thread.feedbackLabels.${key}`)}</button>)}
          </details>}
          <span className="bot-card-spacer" />
          {removed ? <p className="thread-delivery-source" role="status">{t("thread.runRemoved")}</p>
            : <button type="button" className="morning-link" onClick={openResult}>{t("thread.open")}</button>}
        </div> : null}
        {error && <p role="alert" className="inline-error">{i18n.t("briefings:schedule.statusFailed")}</p>}
      </div>
    </BotPostBody>
  </article>;
}
