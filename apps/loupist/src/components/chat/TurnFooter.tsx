import i18n, { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { CornerDownRightIcon } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { clockLabel, useNow, useThrottledAnnouncement } from "../../chat/runStatus";
import type { LiveLine as LiveLineView, TurnFooter as FooterSlot } from "../../chat/turnView";
import { Ticker, TurnGlyph, useArrived } from "./turnMotion";

const t = i18n.getFixedT(null, "chat");

/** A label stays at least this long; changes that arrive sooner wait (DIRECTION 4.3). */
export const DWELL_MS = 600;
/** After a step ends, its label holds this long before the line reads Thinking. */
export const STEP_HOLD_MS = 800;
/** The live line's exit at commit (`--gg-motion-fast`). */
const LEAVE_MS = 120;

/**
 * The label the live line shows: at least 600 ms per label, so a burst of
 * quick steps never strobes, and a finished step's label held 800 ms so the
 * gap before the next step never flashes "Thinking". A stop or a question is
 * the reader's business and shows at once. "+N" follows the latest state.
 */
export function useShownLine(line: LiveLineView): LiveLineView {
  const [shown, setShown] = useState(() => ({ line, at: Date.now() }));
  const state = useRef({ shown, want: line, stepEnded: null as number | null });
  state.current.want = line;
  useEffect(() => {
    const current = state.current;
    let timer: number | undefined;
    const evaluate = () => {
      const { want, shown: showing } = current;
      const now = Date.now();
      if (want.kind === "step") current.stepEnded = null;
      else if (current.stepEnded === null && showing.line.kind === "step") current.stepEnded = now;
      if (want.key === showing.line.key) return;
      const urgent = want.kind === "stopping" || want.kind === "waiting";
      let readyAt = urgent ? now : showing.at + DWELL_MS;
      if (!urgent && (want.kind === "thinking" || want.kind === "still") && current.stepEnded !== null) {
        readyAt = Math.max(readyAt, current.stepEnded + STEP_HOLD_MS);
      }
      if (readyAt > now) { timer = window.setTimeout(evaluate, readyAt - now); return; }
      current.shown = { line: want, at: now };
      setShown(current.shown);
    };
    evaluate();
    return () => window.clearTimeout(timer);
  }, [line.key]);
  if (shown.line.key === line.key) return line;
  return shown.line;
}

/** "m:ss" since the reply started; a leaf, so a tick redraws only the clock. */
function LiveClock({ since }: { since: number }) {
  const now = useNow(true);
  return <span className="turn-live-clock" aria-hidden="true">{clockLabel(now - since)}</span>;
}

function LiveLine({ line, sinceMs }: { line: LiveLineView; sinceMs: number }) {
  useTranslation("chat");
  const shown = useShownLine(line);
  return (
    <p className="turn-live" data-testid="turn-live" data-kind={shown.kind} data-sheen={shown.sheen || undefined} data-tone={shown.kind === "waiting" ? "you" : undefined}
      aria-label={t("chatProgressTimeline.runStatus")}>
      <span className="turn-live-glyph" aria-hidden="true"><TurnGlyph glyph={shown.glyph} /></span>
      <Ticker id={shown.key} className="turn-live-text">
        <span className="turn-live-label">{shown.label}</span>
        {shown.target ? <span className="turn-live-target" aria-hidden="true">{shown.target}</span> : null}
      </Ticker>
      {shown.more > 0 ? <span className="turn-live-more" aria-hidden="true">{t("ledger.more", { formattedCount: formatNumber(shown.more) })}</span> : null}
      <LiveClock since={sinceMs} />
    </p>
  );
}

type Props = {
  footer: FooterSlot;
  /** The one live region for the whole turn, throttled. */
  announce: string;
  /** Guidance sent while this reply runs; it applies after the current step. */
  guiding?: readonly string[];
  /** The settled row: sources, copy, receipt and feedback, drawn by the card. */
  children?: ReactNode;
};

/**
 * The slot where a turn completes. It keeps one control's height from the
 * first frame: the live line while the reply runs, then, in the same place,
 * the actions, receipt and sources. At commit the live line fades out as the
 * action row fades in; nothing above the slot moves.
 */
export function TurnFooter({ footer, announce, guiding, children }: Props) {
  useTranslation("chat");
  const announced = useThrottledAnnouncement(announce, true);
  const arrived = useArrived();
  // The live line the reader last saw, kept for its exit when the slot settles.
  const [last, setLast] = useState<{ line: LiveLineView; sinceMs: number } | null>(footer.kind === "live" ? footer : null);
  const [committed, setCommitted] = useState(false);
  if (footer.kind === "live" && last?.line !== footer.line) setLast({ line: footer.line, sinceMs: footer.sinceMs });
  if (footer.kind === "settled" && last && !committed) setCommitted(true);
  useEffect(() => {
    if (!committed || !last) return;
    const timer = window.setTimeout(() => setLast(null), LEAVE_MS);
    return () => window.clearTimeout(timer);
  }, [committed, last]);
  // The row arrives with its entrance when the reader watched the reply finish, not when the turn is read later.
  const arriving = committed || (arrived && footer.kind === "settled");
  return (
    <div className="turn-footer" data-state={footer.kind}>
      {footer.kind === "live" ? <LiveLine line={footer.line} sinceMs={footer.sinceMs} />
        : <>
          {last ? <p className="turn-live" data-leaving="" aria-hidden="true">
            <span className="turn-live-glyph"><TurnGlyph glyph={last.line.glyph} /></span>
            <span className="turn-live-label">{last.line.label}</span>
          </p> : null}
          <div className="message-action-bar" data-arriving={arriving || undefined}>{children}</div>
        </>}
      <span className="sr-only" aria-live="polite">{announced}</span>
      {footer.kind === "live" && guiding?.length ? <div className="chat-guiding">{guiding.map((text, index) => <p key={index} className="chat-guiding-row" role="status">
        <CornerDownRightIcon aria-hidden="true" />{t("chatProgressTimeline.guiding", { text })}
      </p>)}</div> : null}
    </div>
  );
}
