import i18n, { useTranslation } from "@/i18n";
import { ArrowDownIcon } from "lucide-react";
import type { ChatMessage } from "../../types";
import { useConversationScroll, useConversationState } from "../ai-elements/conversation";
import { clockLabel, useNow } from "../../chat/runStatus";
import { deriveTurnStatus, useLastActivity } from "../../chat/turnView";

const t = i18n.getFixedT(null, "chat");

export type ChatRunStatus = {
  running: boolean;
  /** A stop was requested and the host has not confirmed it. */
  stopping?: boolean;
};

type Props = {
  /** The latest reply, when it is the latest message. */
  message?: ChatMessage;
  run?: ChatRunStatus;
};

/** What the pill says beside its button, by the state of the latest turn (DIRECTION 4.4). */
function PillLabel({ message, run }: { message: ChatMessage; run?: ChatRunStatus }) {
  useTranslation("chat");
  const streaming = message.status === "streaming";
  const now = useNow(streaming);
  const lastActivityAt = useLastActivity(message);
  if (message.status === "error") return <span className="chat-status-pill-label" aria-hidden="true">{t("chatStatusPill.failed")}</span>;
  if (!streaming) return null;
  const { footer } = deriveTurnStatus({ message, now, lastActivityAt, stopping: Boolean(run?.stopping), retry: { attempt: 1, offered: false } });
  if (footer.kind !== "live") return null;
  const waiting = footer.line.kind === "waiting";
  return <span className="chat-status-pill-label" data-tone={waiting ? "you" : undefined} aria-hidden="true">
    {waiting ? <><span className="turn-live-dot" />{footer.line.label}</>
      : <>{footer.line.label} · <span className="chat-status-pill-clock">{clockLabel(now - footer.sinceMs)}</span></>}
  </span>;
}

/**
 * Floats over the bottom of the thread, 12 px above the composer, only while
 * the end of the latest turn is out of view and the reader is not following
 * it: the live label and clock while a reply runs, then "Jump to latest".
 * It never takes the thread's height. Stop stays in the composer.
 */
export function ChatStatusPill({ message, run }: Props) {
  useTranslation("chat");
  const conversation = useConversationScroll();
  const state = useConversationState();
  if (!conversation || !state?.away) return null;
  const jump = t("chatStatusPill.jumpToLatest");
  const running = Boolean(run?.running) && message?.status === "streaming";
  const label = message ? <PillLabel message={message} run={run} /> : null;
  return (
    <div className="chat-status-pill" data-running={running || undefined} data-failed={message?.status === "error" || undefined}>
      {label}
      <button type="button" className="chat-status-pill-jump" aria-label={jump} title={jump} onClick={conversation.jumpToLatest}>
        <ArrowDownIcon aria-hidden="true" />
        <span className="chat-status-pill-jump-label" aria-hidden="true">{jump}</span>
      </button>
    </div>
  );
}
