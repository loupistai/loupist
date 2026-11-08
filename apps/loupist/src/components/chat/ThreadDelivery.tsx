import i18n, { useTranslation } from "@/i18n";
import { useGo } from "../navigation/Open";
import { formatTime } from "@/i18n/format";
import type { ChatMessage } from "../../types";
import { cadencePhrase } from "../briefings/scheduledPresentation";

const t = i18n.getFixedT(null, "chat");

/**
 * A scheduled result the Loupist posted into its home Thread: the run's first
 * paragraph and where it came from. The full answer stays in the run's Chat.
 */
export function ThreadDelivery({ message, loupistName }: {
  message: ChatMessage;
  loupistName: string;
}) {
  useTranslation("chat");
  const go = useGo();
  const delivery = message.delivery!;
  const finished = delivery.status === "succeeded";
  const title = delivery.task_kind === "briefing" ? i18n.t("briefings:morning") : delivery.task_title;
  // The cadence without its time zone: the Schedule tab names the zone once.
  const source = [t("thread.from", { title }), delivery.schedule ? cadencePhrase(delivery.schedule) : null].filter(Boolean).join(" · ");
  return <article className="message assistant thread-delivery" data-status={delivery.status} aria-label={t("thread.deliveryLabel", { title })}>
    <div className="message-meta">
      <span>{loupistName}</span>
      <time dateTime={new Date(delivery.finished_at_ms).toISOString()}>{formatTime(delivery.finished_at_ms)}</time>
    </div>
    <p className="thread-delivery-text">{finished ? message.content : t("thread.couldNotFinish", { title })}</p>
    <p className="thread-delivery-source">
      <span>{source}</span>
      {delivery.run_thread_id && go ? <><span aria-hidden="true">·</span><button type="button" className="morning-link" onClick={() => go({ kind: "chat", threadId: delivery.run_thread_id! })}>{t("thread.open")}</button></> : null}
    </p>
  </article>;
}
