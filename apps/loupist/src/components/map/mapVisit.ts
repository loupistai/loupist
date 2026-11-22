const storageKey = "loupist.mapVisit.v1";
const RECENT_FOCAL = 5;

/** Per-device, per-Loupist Map preferences. */
type Visit = { hintDismissed?: boolean; recentFocal?: string[] };

function read(): Record<string, Visit> {
  try {
    const raw = localStorage.getItem(storageKey);
    const parsed = raw ? JSON.parse(raw) as Record<string, Visit> : {};
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

function write(loupistId: string, patch: Visit) {
  try {
    const all = read();
    all[loupistId] = { ...all[loupistId], ...patch };
    localStorage.setItem(storageKey, JSON.stringify(all));
  } catch {
    // Storage can be unavailable; the Map then opens with its defaults next time.
  }
}

/** The legend's open state is one choice for the whole app, kept once the person opens or collapses it. */
const legendKey = "loupist.mapLegendOpen.v1";

export function storedLegendOpen(): boolean | undefined {
  try {
    const value = localStorage.getItem(legendKey);
    return value === "true" ? true : value === "false" ? false : undefined;
  } catch {
    return undefined;
  }
}

export function rememberLegendOpen(open: boolean) {
  try {
    localStorage.setItem(legendKey, String(open));
  } catch {
    // Storage can be unavailable; the legend then opens again next time.
  }
}

export function mapHintDismissed(loupistId: string) {
  return read()[loupistId]?.hintDismissed === true;
}

export function dismissMapHint(loupistId: string) {
  write(loupistId, { hintDismissed: true });
}

export function rememberRecentFocal(loupistId: string, id: string) {
  write(loupistId, { recentFocal: [id, ...recentFocal(loupistId).filter(other => other !== id)].slice(0, RECENT_FOCAL) });
}

/** Names the Map was centered on, newest first. */
export function recentFocal(loupistId: string) {
  const value = read()[loupistId]?.recentFocal;
  return Array.isArray(value) ? value.filter((id): id is string => typeof id === "string").slice(0, RECENT_FOCAL) : [];
}
