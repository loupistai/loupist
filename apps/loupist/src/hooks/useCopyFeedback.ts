import { useCallback, useEffect, useRef, useState } from "react";

type CopyState = "idle" | "copied" | "failed";

/** Copy feedback belongs to the current content and the latest copy attempt. */
export function useCopyFeedback(contentKey: unknown, duration = 2_000) {
  const [state, setState] = useState<CopyState>("idle");
  const generation = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const reset = useCallback(() => {
    generation.current += 1;
    clearTimeout(timer.current);
    setState("idle");
  }, []);
  useEffect(() => {
    setState("idle");
    return () => {
      generation.current += 1;
      clearTimeout(timer.current);
    };
  }, [contentKey]);
  const copy = useCallback(async (text: string) => {
    const request = ++generation.current;
    clearTimeout(timer.current);
    setState("idle");
    try {
      await navigator.clipboard.writeText(text);
      if (request !== generation.current) return false;
      setState("copied");
      timer.current = setTimeout(() => setState("idle"), duration);
      return true;
    } catch {
      if (request === generation.current) setState("failed");
      return false;
    }
  }, [duration]);
  return { state, copy, reset };
}
