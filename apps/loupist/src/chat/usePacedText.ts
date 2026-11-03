import { useLayoutEffect, useRef, useState } from "react";

// Pace bursts of provider text over a few paints. A slow stream has nothing
// to queue. A large burst catches up within the same short display window.
const WINDOW_MS = 160;
const words = typeof Intl.Segmenter === "function" ? new Intl.Segmenter(undefined, { granularity: "word" }) : undefined;
const motionQuery = "(prefers-reduced-motion: no-preference)";

function nextEnd(text: string, from: number, count: number) {
  if (!words || from + count >= text.length) return text.length;
  const part = words.segment(text.slice(from)).containing(count - 1);
  return part ? from + part.index + part.segment.length : text.length;
}

export function usePacedText(text: string, streaming: boolean) {
  const paced = streaming && typeof window !== "undefined" && !!window.matchMedia?.(motionQuery).matches;
  const [shown, setShown] = useState(() => paced ? text.slice(0, nextEnd(text, 0, 4)) : text);
  const cursor = useRef(shown);
  const target = useRef(text);
  const frame = useRef<number | undefined>(undefined);

  useLayoutEffect(() => {
    const append = text.startsWith(target.current);
    target.current = text;
    const publish = (value: string) => {
      cursor.current = value;
      setShown(value);
    };
    if (!paced || !append || document.hidden || text.length - cursor.current.length <= 4) {
      if (frame.current !== undefined) cancelAnimationFrame(frame.current);
      frame.current = undefined;
      publish(text);
      return;
    }
    if (frame.current !== undefined) return;
    let last = performance.now();
    const deadline = last + WINDOW_MS;
    const tick = (now: number) => {
      const value = target.current;
      const from = cursor.current.length;
      const fraction = Math.min(1, (now - last) / Math.max(1, deadline - last));
      const count = Math.max(4, Math.ceil((value.length - from) * fraction));
      last = now;
      const end = nextEnd(value, from, count);
      publish(value.slice(0, end));
      frame.current = end < value.length ? requestAnimationFrame(tick) : undefined;
    };
    frame.current = requestAnimationFrame(tick);
  }, [text, paced]);

  useLayoutEffect(() => () => {
    if (frame.current !== undefined) cancelAnimationFrame(frame.current);
    frame.current = undefined;
  }, []);
  return paced && text.startsWith(shown) ? shown : text;
}
