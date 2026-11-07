import i18n, { useTranslation } from "@/i18n";
import { CornerDownRightIcon } from "lucide-react";
import type { ReactNode } from "react";
import { clockLabel, useThrottledAnnouncement } from "../../chat/runStatus";
import type { TurnFooter as FooterSlot } from "../../chat/turnView";

const t = i18n.getFixedT(null, "chat");

type Props = {
  footer: FooterSlot;
  now: number;
  /** The one live region for the whole turn, throttled. */
  announce: string;
  /** Guidance sent while this reply runs; it applies after the current step. */
  guiding?: readonly string[];
  /** The settled row: sources, copy, receipt and feedback, drawn by the card. */
  children?: ReactNode;
};

/**
 * The slot where a turn completes. It reserves the action row's height from
 * the first frame: a status line with a clock while the reply runs, and the
 * actions, receipt and sources in the same place once it settles.
 */
export function TurnFooter({ footer, now, announce, guiding, children }: Props) {
  useTranslation("chat");
  const announced = useThrottledAnnouncement(announce, true);
  return (
    <div className="turn-footer" data-state={footer.kind}>
      {footer.kind === "status" ? (
        <p className="turn-status" data-stalled={footer.stalled || undefined} aria-label={t("chatProgressTimeline.runStatus")}>
          <span className="chat-progress-live" data-heartbeat={footer.stalled || undefined} aria-hidden="true" />
          <span className="turn-status-label">{footer.label}</span>
          <span className="turn-status-clock chat-progress-elapsed">{clockLabel(now - footer.sinceMs)}</span>
        </p>
      ) : <div className="message-action-bar">{children}</div>}
      <span className="sr-only" aria-live="polite">{announced}</span>
      {footer.kind === "status" && guiding?.length ? <div className="chat-guiding">{guiding.map((text, index) => <p key={index} className="chat-guiding-row" role="status">
        <CornerDownRightIcon aria-hidden="true" />{t("chatProgressTimeline.guiding", { text })}
      </p>)}</div> : null}
    </div>
  );
}
