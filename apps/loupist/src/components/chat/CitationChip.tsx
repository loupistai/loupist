import { useTranslation } from "@/i18n";
import { formatDate } from "@/i18n/format";
import { Popover } from "radix-ui";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import type { ChatEvidenceRef } from "../../types";
import { evidenceExcerptParts } from "../../chat/evidence";
import { quoteEvidence } from "../../chat/quoteEvidence";
import { quotePreviewLine } from "./QuoteEvidence";
import { narrowWindow } from "../../chat/narrowWindow";
import { useOpenEvidenceId } from "./OpenEvidence";

/** Hover dwell before the card opens, and the grace period for moving onto it. */
const OPEN_DELAY_MS = 150;
const CLOSE_DELAY_MS = 120;
/** How long a press is held before it opens the card instead of Evidence (narrow windows). */
export const LONG_PRESS_MS = 500;

const sourceDate = (evidence: ChatEvidenceRef) => {
  const value = evidence.published_at ?? evidence.retrieved_at;
  const time = value ? Date.parse(value) : Number.NaN;
  return Number.isFinite(time) ? formatDate(time, /^\d{4}-\d{2}-\d{2}$/.test(value ?? "") ? { timeZone: "UTC" } : {}) : "";
};

/**
 * A numbered citation in an answer (chat spec X2). Hovering for a moment, or
 * focusing it from the keyboard, shows a card with the source's title, its
 * domain and date, and the retained passage or a short quote line. Clicking
 * opens that citation. Esc closes the card, and only one is open at a time.
 *
 * In a narrow window (700 px) hovering opens nothing, so passing over the text
 * never covers it: pressing and holding the chip opens the card as a popover
 * that stays until a press elsewhere or Esc, and a tap still opens Evidence.
 */
export function CitationChip({ evidence, label, title, source, onOpen, children }: {
  evidence: ChatEvidenceRef;
  /** Accessible name of the chip ("Source 1: …"). */
  label: string;
  title: string;
  /** Domain, or the kind of source when there is no domain. */
  source: string;
  onOpen: (anchor: HTMLElement) => void;
  children: ReactNode;
}) {
  const { t } = useTranslation("chat");
  const [open, setOpen] = useState(false);
  const [byKeyboard, setByKeyboard] = useState(false);
  // Opened by a long press: it stays open until dismissed, like a clicked popover.
  const [pinned, setPinned] = useState(false);
  const timer = useRef<number | null>(null);
  const holdTimer = useRef<number | null>(null);
  // The press that opened the card ends in a click, which must not also open Evidence.
  const heldOpen = useRef(false);
  // Focus that follows a press is a click; focus without one came from the keyboard.
  const pressed = useRef(false);
  const cardId = useId();
  const date = sourceDate(evidence);
  // The chip whose passage the Evidence reader shows now reads as the current one.
  const current = useOpenEvidenceId() === evidence.id;
  const schedule = (next: boolean, delay: number) => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => { timer.current = null; setOpen(next); }, delay);
  };
  const cancelHold = () => {
    if (holdTimer.current !== null) window.clearTimeout(holdTimer.current);
    holdTimer.current = null;
  };
  const close = () => { setOpen(false); setPinned(false); };
  useEffect(() => () => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    if (holdTimer.current !== null) window.clearTimeout(holdTimer.current);
  }, []);

  return <Popover.Root open={open} onOpenChange={(next) => { if (next) setOpen(true); else close(); }}>
    <Popover.Anchor asChild>
      <button type="button" className="loupist-citation" data-evidence-id={evidence.id} aria-label={label} aria-current={current ? "true" : undefined} aria-describedby={open ? cardId : undefined}
        aria-expanded={pinned ? true : undefined} aria-controls={pinned ? cardId : undefined}
        onPointerEnter={() => { if (narrowWindow()) return; setByKeyboard(false); schedule(true, OPEN_DELAY_MS); }}
        onPointerLeave={() => { cancelHold(); if (!pinned) schedule(false, CLOSE_DELAY_MS); }}
        onPointerDown={(event) => {
          pressed.current = true;
          heldOpen.current = false;
          if (!narrowWindow() || event.button > 0) return;
          cancelHold();
          holdTimer.current = window.setTimeout(() => {
            holdTimer.current = null;
            heldOpen.current = true;
            if (timer.current !== null) window.clearTimeout(timer.current);
            setByKeyboard(false);
            setPinned(true);
            setOpen(true);
          }, LONG_PRESS_MS);
        }}
        onPointerUp={cancelHold}
        onPointerCancel={cancelHold}
        // A held touch would otherwise also bring up the system's own menu.
        onContextMenu={(event) => { if (heldOpen.current || holdTimer.current !== null) event.preventDefault(); }}
        onFocus={() => { if (!pressed.current) { setByKeyboard(true); schedule(true, 0); } }}
        onBlur={() => { pressed.current = false; if (!pinned) schedule(false, CLOSE_DELAY_MS); }}
        onKeyDown={(event) => { if (event.key === "Escape" && open) { event.stopPropagation(); close(); } }}
        onClick={(event) => {
          if (heldOpen.current) { heldOpen.current = false; return; }
          close();
          schedule(false, 0);
          onOpen(event.currentTarget);
        }}>
        {children}
      </button>
    </Popover.Anchor>
    <Popover.Portal>
      <Popover.Content id={cardId} role={byKeyboard || pinned ? "dialog" : "tooltip"} aria-label={byKeyboard || pinned ? title : undefined} className="citation-preview"
        data-pinned={pinned || undefined} side="top" align="start" sideOffset={6}
        // The app header and the chat sub-header sit above the thread; flip below the chip rather than cover them.
        collisionPadding={{ top: 112, right: 8, bottom: 8, left: 8 }} onOpenAutoFocus={(event) => event.preventDefault()} onCloseAutoFocus={(event) => event.preventDefault()}
        onPointerEnter={() => { if (!pinned) schedule(true, 0); }} onPointerLeave={() => { if (!pinned) schedule(false, CLOSE_DELAY_MS); }}>
        <strong>{title}</strong>
        <small className="source-site"><span>{[source, date].filter(Boolean).join(" · ")}</span></small>
        {(() => {
          const quote = quoteEvidence(evidence);
          if (quote) {
            const line = quotePreviewLine(quote);
            return <p>{line || t("quoteEvidence.coverageNone")}</p>;
          }
          if (!evidence.excerpt) return null;
          // The cited words are marked inside the retained passage, as in the Evidence reader.
          const parts = evidenceExcerptParts(evidence);
          return <p>{parts ? <>{parts.before}<mark>{parts.match}</mark>{parts.after}</> : evidence.excerpt}</p>;
        })()}
      </Popover.Content>
    </Popover.Portal>
  </Popover.Root>;
}
