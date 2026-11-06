import i18n, { useTranslation } from "@/i18n";
import { formatTime } from "@/i18n/format";
import type { ChatMessage, HomeHandoff } from "../../types";

const t = i18n.getFixedT(null, "chat");

const statusLabel = (status: HomeHandoff["status"]) => ({
  running: t("thread.handoffRunning"),
  done: t("thread.handoffDone"),
  failed: t("thread.handoffFailed"),
  stopped: t("thread.handoffStopped"),
})[status];

export function ThreadHandoff({ message, loupistName, onOpenRunChat }: {
  message: ChatMessage;
  loupistName: string;
  onOpenRunChat?: (threadId: string) => void;
}) {
  useTranslation("chat");
  const handoff = message.handoff!;
  const done = handoff.status === "done";
  const heading = done ? t("thread.researched", { title: handoff.title }) : t("thread.researching", { title: handoff.title });
  const learned = handoff.learned?.pages.length;
  return <article className="message assistant thread-delivery" data-status={handoff.status} aria-label={t("thread.handoffLabel", { title: handoff.title })}>
    <div className="message-meta">
      <span>{loupistName}</span>
      <time dateTime={message.created_at}>{formatTime(Date.parse(message.created_at))}</time>
    </div>
    {done ? <p className="thread-delivery-text">{message.content}</p> : null}
    <p className="thread-delivery-source">
      <span>{heading}</span>
      <span aria-hidden="true">·</span>
      <span>{statusLabel(handoff.status)}</span>
      {learned ? <><span aria-hidden="true">·</span><span>{t("thread.learned", { count: learned })}</span></> : null}
      {onOpenRunChat ? <><span aria-hidden="true">·</span><button type="button" className="morning-link" onClick={() => onOpenRunChat(handoff.chatId)}>{t("thread.open")}</button></> : null}
    </p>
  </article>;
}
