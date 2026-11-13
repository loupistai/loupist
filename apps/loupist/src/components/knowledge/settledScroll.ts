import { useCallback, useEffect, useRef, type UIEvent } from "react";

const SETTLE_MS = 150;

/**
 * Remembers a pane's scroll offset once scrolling settles instead of on every scroll event,
 * so a long list or reader does not re-render its whole route while it moves. `save` always
 * sees the latest location. A pending save is dropped when `resetKey` changes, so it never
 * restores an offset into a different record, and when the pane unmounts, unless
 * `flushOnUnmount` (the Map's compact Library, which closes as a row opens and keeps its
 * own store) saves it at once to the location the pane last showed.
 */
export function useSettledScroll(save: (top: number) => void, resetKey: string, flushOnUnmount = false) {
  const latest = useRef(save);
  latest.current = save;
  const timer = useRef<number | undefined>(undefined);
  const pending = useRef<number | null>(null);
  const flush = useRef(flushOnUnmount);
  flush.current = flushOnUnmount;
  // Declared first so it runs before the reset below on unmount.
  useEffect(() => () => {
    window.clearTimeout(timer.current);
    if (flush.current && pending.current !== null) latest.current(pending.current);
    pending.current = null;
  }, []);
  useEffect(() => () => { window.clearTimeout(timer.current); pending.current = null; }, [resetKey]);
  return useCallback((event: UIEvent<HTMLElement>) => {
    const top = event.currentTarget.scrollTop;
    pending.current = top;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => { pending.current = null; latest.current(top); }, SETTLE_MS);
  }, []);
}
