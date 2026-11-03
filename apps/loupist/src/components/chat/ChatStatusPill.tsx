import i18n, { Trans, useTranslation } from "@/i18n";
import { ArrowDownIcon } from "lucide-react";
import type { ChatMessage } from "../../types";
import { useConversationScroll } from "../ai-elements/conversation";
import {
  clockLabel,
  currentStepLabel,
  lastStepLabel,
  progressActivityKey,
  useNow,
  useRunHeartbeat,
  useThrottledAnnouncement,
} from "../../chat/runStatus";

const t = i18n.getFixedT(null, "chat");

export type ChatRunStatus = {
  running: boolean;
};

type Props = {
  /** The reply being produced, when it is the latest message. */
  message?: ChatMessage;
  run?: ChatRunStatus;
};

/**
 * Floats above the composer only while the reader is away from the end of the
 * thread: the live run while a reply runs, "Jump to latest" always. Stop stays
 * in the composer, which is always on screen below the pill.
 */
export function ChatStatusPill({ message, run }: Props) {
  useTranslation("chat");
  const conversation = useConversationScroll();
  const running = Boolean(run?.running);
  const progress = message?.status === "streaming" ? message.progress : undefined;
  const phase = message?.status === "streaming" ? message.phase : undefined;
  const now = useNow(running);
  const stalled = useRunHeartbeat(progressActivityKey(progress, phase), running, phase, now);
  const step = stalled ? lastStepLabel(progress, phase) : currentStepLabel(progress, phase);
  const announcement = useThrottledAnnouncement(step, running && !conversation?.isAtBottom);
  if (!conversation || conversation.isAtBottom) return null;
  const startedAtMs = progress?.startedAtMs ?? (message ? Date.parse(message.created_at) : Number.NaN);
  const clock = <span className="chat-status-pill-clock">{Number.isFinite(startedAtMs) ? clockLabel(now - startedAtMs) : ""}</span>;
  const jump = t("chatStatusPill.jumpToLatest");

  return (
    <div className="chat-status-pill" data-running={running || undefined}>
      {running ? <>
        <span className="sr-only" role="status">{announcement}</span>
        <span className="chat-status-pill-dot" data-heartbeat={stalled || undefined} aria-hidden="true" />
        <span className="chat-status-pill-label" aria-hidden="true">
          <Trans ns="chat" i18nKey={stalled ? "chatStatusPill.stillWorking" : "chatStatusPill.working"} values={{ step }} components={[clock]} />
        </span>
      </> : null}
      <button type="button" className="chat-status-pill-jump" aria-label={jump} title={jump} onClick={conversation.scrollToBottom}>
        <ArrowDownIcon aria-hidden="true" />
        <span className="chat-status-pill-jump-label" aria-hidden="true">{jump}</span>
      </button>
    </div>
  );
}
