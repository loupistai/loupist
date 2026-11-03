import { useEffect, useRef } from "react";

/** Per-device "last seen" per Loupist. A preference, not Memory; the Map's What's new reads it too. */
const storageKey = "loupist.loupistVisit.v1";
/** A visit shorter than this does not count, so a quick peek never swallows the diff. */
export const MIN_VISIT_MS = 5 * 60_000;
/** Away (hidden or blurred) for longer than this ends the visit at the moment the user left. */
export const AWAY_ENDS_VISIT_MS = 5 * 60_000;

type Visits = Record<string, { lastSeenAtMs: number }>;

function read(): Visits {
  try {
    const raw = localStorage.getItem(storageKey);
    const parsed: unknown = raw ? JSON.parse(raw) : {};
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed as Visits : {};
  } catch {
    return {};
  }
}

export function lastSeenAt(loupistId: string): number | undefined {
  const value = read()[loupistId]?.lastSeenAtMs;
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

/** Records a finished visit; returns whether it was long enough to count. */
export function recordVisit(loupistId: string, openedAtMs: number, leftAtMs: number): boolean {
  if (leftAtMs - openedAtMs < MIN_VISIT_MS) return false;
  try {
    const visits = read();
    visits[loupistId] = { lastSeenAtMs: leftAtMs };
    localStorage.setItem(storageKey, JSON.stringify(visits));
  } catch {
    // Storage can be unavailable; the morning message then treats every visit as the first.
  }
  return true;
}

/**
 * Tracks the visible Loupist. Leaving it (another Loupist, Settings, or the window
 * away for more than five minutes) records the visit when it lasted five minutes.
 */
export function useLoupistVisit(loupistId: string | null, present: boolean) {
  const visit = useRef<{ loupistId: string; openedAtMs: number } | null>(null);
  useEffect(() => {
    if (!loupistId || !present) return;
    visit.current = { loupistId, openedAtMs: Date.now() };
    let awaySinceMs: number | null = null;
    const leave = () => { if (awaySinceMs === null) awaySinceMs = Date.now(); };
    const back = () => {
      if (awaySinceMs === null) return;
      const away = awaySinceMs;
      awaySinceMs = null;
      if (Date.now() - away <= AWAY_ENDS_VISIT_MS || !visit.current) return;
      recordVisit(visit.current.loupistId, visit.current.openedAtMs, away);
      visit.current = { loupistId, openedAtMs: Date.now() };
    };
    const onVisibility = () => { if (document.visibilityState === "hidden") leave(); else back(); };
    window.addEventListener("blur", leave);
    window.addEventListener("focus", back);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.removeEventListener("blur", leave);
      window.removeEventListener("focus", back);
      document.removeEventListener("visibilitychange", onVisibility);
      const current = visit.current;
      visit.current = null;
      if (current) recordVisit(current.loupistId, current.openedAtMs, awaySinceMs ?? Date.now());
    };
  }, [loupistId, present]);
}
