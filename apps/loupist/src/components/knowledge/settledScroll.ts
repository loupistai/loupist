import { useCallback, useEffect, useRef, type UIEvent } from "react";

const SETTLE_MS = 150;

/**
 * Remembers a pane's scroll offset once scrolling settles instead of on every scroll event,
 * so a long list or reader does not re-render its whole route while it moves. `save` always
 * sees the latest location; a pending save is dropped when `resetKey` changes or the pane
 * unmounts, so it never restores an offset into a different record.
 */
export function useSettledScroll(save: (top: number) => void, resetKey: string) {
  const latest = useRef(save);
  latest.current = save;
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), [resetKey]);
  return useCallback((event: UIEvent<HTMLElement>) => {
    const top = event.currentTarget.scrollTop;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => latest.current(top), SETTLE_MS);
  }, []);
}
