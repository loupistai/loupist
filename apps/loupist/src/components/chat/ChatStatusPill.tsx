import i18n, { useTranslation } from "@/i18n";
import { ArrowDownIcon } from "lucide-react";
import type { ChatMessage } from "../../types";
import { useConversationScroll } from "../ai-elements/conversation";
import { clockLabel, useNow } from "../../chat/runStatus";
import { deriveTurnView, useLastActivity } from "../../chat/turnView";

const t = i18n.getFixedT(null, "chat");

export type ChatRunStatus = {
  running: boolean;
  /** A stop was requested and the host has not confirmed it. */
  stopping?: boolean;
};

type Props = {
  /** The reply being produced, when it is the latest message. */
  message?: ChatMessage;
  run?: ChatRunStatus;
};

/**
 * Floats above the composer only while the reader is away from the end of the
 * thread: the live run while a reply runs, "Jump to latest" always. Stop stays
 * in the composer, which is always on screen below the pill. The turn's own
 * footer announces the run; the pill repeats it silently.
 */
export function ChatStatusPill({ message, run }: Props) {
  useTranslation("chat");
  const conversation = useConversationScroll();
  const running = Boolean(run?.running);
  const now = useNow(running);
  const lastActivityAt = useLastActivity(message);
  const view = message && message.status === "streaming"
    ? deriveTurnView({ message, now, lastActivityAt, stopping: Boolean(run?.stopping), retryPending: false }) : undefined;
  if (!conversation || conversation.isAtBottom) return null;
  const jump = t("chatStatusPill.jumpToLatest");
  const footer = view?.footer.kind === "status" ? view.footer : undefined;
  const step = view?.state.kind === "working" && view.state.stepRunning ? view.state.step : undefined;

  return (
    <div className="chat-status-pill" data-running={running || undefined}>
      {running && footer ? <>
        <span className="chat-status-pill-dot" data-heartbeat={footer.stalled || undefined} aria-hidden="true" />
        <span className="chat-status-pill-label" aria-hidden="true">
          {footer.label} · <span className="chat-status-pill-clock">{clockLabel(now - footer.sinceMs)}</span>{step ? ` · ${step}` : ""}
        </span>
      </> : null}
      <button type="button" className="chat-status-pill-jump" aria-label={jump} title={jump} onClick={conversation.scrollToBottom}>
        <ArrowDownIcon aria-hidden="true" />
        <span className="chat-status-pill-jump-label" aria-hidden="true">{jump}</span>
      </button>
    </div>
  );
}
