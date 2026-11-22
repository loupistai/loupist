import { useState } from "react";
import i18n, { useTranslation } from "@/i18n";
import { useOpen } from "../navigation/Open";
import { RunRecordRemoved } from "../../chat/navigation";
import type { ChatMessage } from "../../types";
import { formatDateTime } from "@/i18n/format";
import { scheduledTaskTitle, skippedSavedKeyLabel } from "../briefings/scheduledPresentation";
import { BotPostBody } from "./BotPost";
import { MemorySaveWarning } from "./ChatReceipt";
import { ContextTitle, titlePlaceholder } from "../ui/context-title";
import "@/styles/bot-cards.css";

const t = i18n.getFixedT(null, "chat");

/** Bot's copy of a Schedule result opens the original run reader. */
export function ThreadDelivery({ message, loupistName }: {
  message: ChatMessage;
  loupistName: string;
}) {
  useTranslation("chat");
  const open = useOpen();
  const [removed, setRemoved] = useState(false);
  const delivery = message.delivery!;
  const skippedSavedKeys = skippedSavedKeyLabel(delivery.skipped_saved_keys);
  const finished = delivery.status === "succeeded";
  const title = scheduledTaskTitle(delivery.task_kind, delivery.task_title);
  const origin = t("thread.scheduleOrigin", { title: titlePlaceholder, time: formatDateTime(delivery.finished_at_ms) });
  return <article className="message assistant thread-delivery" data-bot-post="" data-delivery-run={delivery.run_id} data-status={delivery.status} aria-label={t("thread.deliveryLabel", { title })}>
    <BotPostBody loupistName={loupistName} at={delivery.finished_at_ms}>
      <div className="bot-card">
        <div className="bot-card-kicker"><span className="bot-card-tag" data-tone={finished ? undefined : "warning"}><ContextTitle template={origin} title={title} /></span></div>
        <p className="thread-delivery-text">{finished ? message.content : t("thread.couldNotFinish")}</p>
        {skippedSavedKeys && <p className="thread-delivery-source">{skippedSavedKeys}</p>}
        <MemorySaveWarning reason={delivery.memory_save_skip} items={delivery.discarded_memory_items} />
        {open ? <div className="bot-card-foot">
          <span className="bot-card-spacer" />
          {removed ? <p className="thread-delivery-source" role="status">{t("thread.runRemoved")}</p>
            : <button type="button" className="morning-link" onClick={() => void open({ kind: "run", runId: delivery.run_id, taskId: delivery.task_id }).catch((error: unknown) => { if (error instanceof RunRecordRemoved) setRemoved(true); })}>{t("thread.open")}</button>}
        </div> : null}
      </div>
    </BotPostBody>
  </article>;
}
