import { useEffect, useRef } from "react";

export const OPEN_DELAY_MS = 150;
export const CLOSE_DELAY_MS = 120;

/** Numbers and citations share the same single-preview rule. */
const previews = new Set<{ close: () => void }>();

export function useEvidencePopover(open: boolean, onClose: () => void) {
  const callback = useRef(onClose);
  callback.current = onClose;
  useEffect(() => {
    if (!open) return;
    const preview = { close: () => callback.current() };
    for (const other of previews) other.close();
    previews.add(preview);
    return () => { previews.delete(preview); };
  }, [open]);
}
