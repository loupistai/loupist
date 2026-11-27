import { useEffect, useState } from "react";

/** Loading text waits this long, so a quick read never flashes it. */
export const LOADING_DELAY_MS = 300;

/** True once `pending` has lasted `LOADING_DELAY_MS`; false again as soon as it ends. */
export function useSlow(pending: boolean) {
  const [slow, setSlow] = useState(false);
  useEffect(() => {
    if (!pending) { setSlow(false); return undefined; }
    const timer = setTimeout(() => setSlow(true), LOADING_DELAY_MS);
    return () => clearTimeout(timer);
  }, [pending]);
  return slow;
}
