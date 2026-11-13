import { useEffect, useLayoutEffect, useRef, useState } from "react";

/**
 * The reveal governor (DIRECTION 8.2). Text arrives at the provider's pace, in
 * bursts and stalls; the reader sees it at an even pace about half a second
 * behind. Each frame the reveal rate eases toward the rate that drains the
 * backlog in half a second, and whole words are released from a budget, so a
 * stall shorter than the lag is invisible and a burst is spread out.
 */
const LAG_SECONDS = 0.5;
/** Once the host has finished, the rest drains faster, so the end never crawls. */
const FINAL_SECONDS = 0.35;
const MIN_RATE = 40;
const MAX_RATE = 900;
const SMOOTHING_MS = 250;
/** A long unit (a link, a number) costs at most this much budget. */
const UNIT_COST = 24;
/** The commit waits for the last words at most this long after the host finished. */
export const DRAIN_LIMIT_MS = 400;
/** Reduced motion: whole sentences or blocks, at most four updates a second. */
export const REDUCED_EVERY_MS = 250;

const words = typeof Intl !== "undefined" && typeof Intl.Segmenter === "function" ? new Intl.Segmenter(undefined, { granularity: "word" }) : undefined;
const motionQuery = "(prefers-reduced-motion: reduce)";
const reducedMotion = () => typeof window !== "undefined" && !!window.matchMedia?.(motionQuery).matches;

/**
 * Where the next reveal unit ends: one word (one eojeol in Korean) with the
 * spaces and punctuation before it. Null while the last word may still grow.
 */
export function nextUnitEnd(text: string, from: number, complete: boolean): number | null {
  if (from >= text.length) return null;
  const window = text.slice(from, from + 200);
  if (!words) {
    const match = /^\s*\S+/u.exec(window);
    const end = from + (match ? match[0].length : window.length);
    return end >= text.length && !complete ? null : end;
  }
  let end = from;
  for (const part of words.segment(window)) {
    end = from + part.index + part.segment.length;
    if (part.isWordLike) return end >= text.length && !complete ? null : end;
  }
  return end;
}

/** The last sentence or block end in `text` after `from`; reduced motion reveals up to it. */
export function lastSentenceEnd(text: string, from: number): number {
  let found = -1;
  for (const match of text.slice(from).matchAll(/[.?!。](?=\s)|\)(?=\s)|\n/gu)) found = from + match.index + match[0].length;
  return found;
}

type Options = {
  /** The host is still sending this text. */
  live: boolean;
  /** Stop or failure confirmed: everything received shows at once, with no fade. */
  halted?: boolean;
  /** A stop was requested: nothing more is revealed until the host confirms. */
  paused?: boolean;
  /** The segment mounted while the reader watched; one that mounts with text (a resumed run) shows it at once. */
  arrived?: boolean;
  /** Called once when the host has finished and every word received is shown. */
  onDrained?: () => void;
};

export type PacedText = {
  text: string;
  /** Words are still being revealed: Streamdown keeps its word fades. */
  animating: boolean;
};

export function usePacedText(text: string, { live, halted = false, paused = false, arrived = false, onDrained }: Options): PacedText {
  const [revealed, setRevealed] = useState(() => (live && arrived ? 0 : text.length));
  const state = useRef({
    text,
    revealed: live && arrived ? 0 : text.length,
    rate: 0,
    budget: 0,
    live,
    paused,
    endedAt: live ? null as number | null : performance.now() - DRAIN_LIMIT_MS,
    lastChunk: -Infinity,
    grewAt: performance.now(),
    frame: undefined as number | undefined,
    last: 0,
  });
  const drained = useRef(onDrained);
  drained.current = onDrained;

  useLayoutEffect(() => {
    const current = state.current;
    const appended = text.startsWith(current.text.slice(0, current.revealed));
    if (text.length !== current.text.length) current.grewAt = performance.now();
    current.text = text;
    current.live = live;
    current.paused = paused;
    if (!live && current.endedAt === null) current.endedAt = performance.now();
    if (live) current.endedAt = null;
    const publish = (value: number) => {
      if (current.revealed === value) return;
      current.revealed = value;
      setRevealed(value);
    };
    const stop = () => {
      if (current.frame !== undefined) cancelAnimationFrame(current.frame);
      current.frame = undefined;
    };
    if (halted || !appended || (typeof document !== "undefined" && document.hidden)) {
      stop();
      publish(text.length);
      return;
    }
    if (paused) { stop(); return; }
    if (current.revealed >= text.length || current.frame !== undefined) return;
    current.last = performance.now();
    const tick = (now: number) => {
      current.frame = undefined;
      const dt = Math.min(64, Math.max(0, now - current.last));
      current.last = now;
      const value = current.text;
      const final = !current.live;
      if (current.paused) return;
      if (final && current.endedAt !== null && now - current.endedAt >= DRAIN_LIMIT_MS) { publish(value.length); return; }
      if (reducedMotion()) {
        if (now - current.lastChunk >= REDUCED_EVERY_MS) {
          const end = final ? value.length : lastSentenceEnd(value, current.revealed);
          if (end > current.revealed) { current.lastChunk = now; publish(end); }
        }
      } else {
        const backlog = value.length - current.revealed;
        let goal = backlog <= 0 ? 0 : Math.min(MAX_RATE, Math.max(MIN_RATE, backlog / LAG_SECONDS));
        if (final && backlog > 0) goal = Math.max(goal, backlog / FINAL_SECONDS);
        current.rate += (goal - current.rate) * (1 - Math.exp(-dt / SMOOTHING_MS));
        current.budget += current.rate * dt / 1_000;
        let at = current.revealed;
        // A last word with nothing after it for longer than the lag is taken as whole.
        const settled = final || now - current.grewAt >= LAG_SECONDS * 1_000;
        for (;;) {
          const end = nextUnitEnd(value, at, settled);
          if (end === null) { current.budget = Math.min(current.budget, value.length - at); break; }
          const cost = Math.min(end - at, UNIT_COST);
          if (current.budget < cost) break;
          current.budget -= cost;
          at = end;
        }
        publish(at);
      }
      if (current.revealed < current.text.length || current.live) {
        if (current.revealed < current.text.length) current.frame = requestAnimationFrame(tick);
      }
    };
    current.frame = requestAnimationFrame(tick);
  }, [text, live, halted, paused]);

  useLayoutEffect(() => () => {
    const current = state.current;
    if (current.frame !== undefined) cancelAnimationFrame(current.frame);
    current.frame = undefined;
  }, []);

  const shown = halted ? text.length : Math.min(revealed, text.length);
  const done = !live && shown >= text.length;
  useEffect(() => { if (done) drained.current?.(); }, [done]);
  return { text: text.slice(0, shown), animating: !halted && !done };
}
