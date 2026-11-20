import i18n, { useTranslation } from "@/i18n";
import { useGo } from "../navigation/Open";
import { formatNumber } from "@/i18n/format";
import { learnedSummary } from "../../chat/receipt";
import type { ChatMessage, HomeHandoff } from "../../types";
import { BotPostBody, MetaLine } from "./BotPost";
import "@/styles/bot-cards.css";

const t = i18n.getFixedT(null, "chat");

/** How a session ended without an answer. A finished one needs none: "Researched" says it. */
const endedLabel = (handoff: HomeHandoff) => handoff.stopReason === "interrupted" ? t("thread.handoffInterrupted")
  : handoff.status === "failed" ? t("thread.handoffFailed")
  : handoff.status === "stopped" ? t("thread.handoffStopped")
  : null;

/**
 * The host's own words for a finished session with no prose to preview, written in English for
 * the history. The heading already says it, in the interface language.
 */
const hostCaption = (handoff: HomeHandoff, content: string) => {
  const text = content.trim();
  return text === `Researched: ${handoff.title}.` || (Boolean(handoff.connectionId) && text === "Researched: Look into it.");
};

/**
 * Bot's post for a session it handed off, once that session ends: the answer's lead (the host's
 * bounded preview) and what it learned, or how it ended and why; Open goes to the session. A
 * session at work posts nothing here: it is followed, guided and stopped in its own session.
 */
export function ThreadHandoff({ message, loupistName }: { message: ChatMessage; loupistName: string }) {
  useTranslation("chat");
  const go = useGo();
  const handoff = message.handoff!;
  const done = handoff.status === "done";
  const heading = t(done ? "thread.researched" : "thread.handoffLabel", { title: handoff.title });
  // The same count and words as the session answer's receipt.
  const learned = learnedSummary(handoff.learned)?.sections;
  const preview = done && !hostCaption(handoff, message.content) ? message.content.trim() : "";
  const ended = endedLabel(handoff);
  // The host stopped it without running, or the app quit while it ran: say why.
  const reason = handoff.status === "stopped" && handoff.stopReason ? handoff.stopReason : null;
  const at = Date.parse(message.created_at);
  return <article className="message assistant thread-delivery thread-handoff" data-bot-post="" data-status={handoff.status} aria-label={t("thread.handoffLabel", { title: handoff.title })}>
    <BotPostBody loupistName={loupistName} at={at}>
      <div className="bot-card">
        {preview ? <p className="thread-delivery-text">{preview}</p> : null}
        {reason ? <p className="thread-delivery-text" data-stop-reason={reason}>{t(`thread.stopped.${reason}`)}</p> : null}
        <div className="bot-card-foot">
          <MetaLine className="thread-delivery-source" items={[
            { key: "heading", node: heading },
            ended ? { key: "status", node: ended } : null,
            learned ? { key: "learned", node: t("chatReceipt.learned", { count: learned, formattedCount: formatNumber(learned) }) } : null,
            go && { key: "open", node: <button type="button" className="morning-link" onClick={() => go({ kind: "chat", threadId: handoff.chatId })}>{t("thread.open")}</button> },
          ]} />
        </div>
      </div>
    </BotPostBody>
  </article>;
}
