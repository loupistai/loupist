import { useState } from "react";
import i18n, { useTranslation } from "@/i18n";
import { useOpen } from "../navigation/Open";
import { RunRecordRemoved } from "../../chat/navigation";
import type { ChatMessage } from "../../types";
import { cadencePhrase, scheduledTaskTitle } from "../briefings/scheduledPresentation";
import { BotPostBody } from "./BotPost";
import "@/styles/bot-cards.css";

const t = i18n.getFixedT(null, "chat");

/**
 * A scheduled result the Loupist posted into its home Thread: the run's first
 * paragraph, the task it came from (the tag) and when it runs. The full answer
 * stays in the run's Chat; once that session is gone with its run, Open shows
 * the task in Schedule, and once the task is gone too the card says so.
 */
export function ThreadDelivery({ message, loupistName }: {
  message: ChatMessage;
  loupistName: string;
}) {
  useTranslation("chat");
  const open = useOpen();
  const [removed, setRemoved] = useState(false);
  const delivery = message.delivery!;
  const finished = delivery.status === "succeeded";
  const title = scheduledTaskTitle(delivery.task_kind, delivery.task_title);
  // The cadence without its time zone: the Schedule tab names the zone once. The tag names the task.
  const cadence = delivery.schedule ? cadencePhrase(delivery.schedule) : null;
  return <article className="message assistant thread-delivery" data-bot-post="" data-status={delivery.status} aria-label={t("thread.deliveryLabel", { title })}>
    <BotPostBody loupistName={loupistName} at={delivery.finished_at_ms}>
      <div className="bot-card">
        <div className="bot-card-kicker"><span className="bot-card-tag" data-tone={finished ? undefined : "warning"}>{title}</span></div>
        <p className="thread-delivery-text">{finished ? message.content : t("thread.couldNotFinish")}</p>
        {cadence || open ? <div className="bot-card-foot">
          {cadence ? <p className="thread-delivery-source"><span>{cadence}</span></p> : null}
          <span className="bot-card-spacer" />
          {removed ? <p className="thread-delivery-source" role="status">{t("thread.runRemoved")}</p>
            : open ? <button type="button" className="morning-link" onClick={() => void open({ kind: "run", threadId: delivery.run_thread_id ?? null, taskId: delivery.task_id }).catch((error: unknown) => { if (error instanceof RunRecordRemoved) setRemoved(true); })}>{t("thread.open")}</button> : null}
        </div> : null}
      </div>
    </BotPostBody>
  </article>;
}
