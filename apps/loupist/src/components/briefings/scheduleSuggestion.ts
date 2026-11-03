import type { GrowthEvent } from "../../contracts/knowledge";
import type { WatchlistEntry } from "../../contracts/userContext";

const DAY_MS = 24 * 60 * 60_000;
/** A dismissed suggestion stays away this long. */
export const SUGGESTION_SNOOZE_MS = 14 * DAY_MS;
/** Without a previous briefing, conflicts from this window count. */
const CONFLICT_WINDOW_MS = 7 * DAY_MS;
const WATCHLIST_MINIMUM = 3;
const STORAGE_KEY = "loupist.scheduleSuggestions.v1";

/** `names` are the assets the suggestion is about (at most three), so the card can say which. */
export type ScheduleSuggestion =
  | { kind: "conflict"; count: number; names: string[] }
  | { kind: "watchlist"; count: number; names: string[] };

const MAX_NAMES = 3;

export type SuggestionInputs = {
  nowMs: number;
  /** Start of the latest briefing run, if there was one. */
  lastBriefingAtMs?: number;
  events: readonly GrowthEvent[];
  watchlist: readonly WatchlistEntry[];
  dismissed: Partial<Record<ScheduleSuggestion["kind"], number>>;
  /** The display name a conflict is about, when the Map knows it. */
  nameOf?: (event: GrowthEvent) => string | undefined;
};

/**
 * At most one suggestion for a special briefing, from data on this computer:
 * conflicting evidence World found since the last briefing, then three or more
 * newly followed assets. Earnings weeks need an events source (W16) and are not
 * suggested yet.
 */
export function scheduleSuggestion(input: SuggestionInputs): ScheduleSuggestion | null {
  const since = input.lastBriefingAtMs ?? input.nowMs - CONFLICT_WINDOW_MS;
  const snoozed = (kind: ScheduleSuggestion["kind"]) => (input.dismissed[kind] ?? 0) > input.nowMs;
  const disputed = input.events.filter((event) => event.kind === "claim_disputed" && event.atMs > since);
  const conflicts = new Set(disputed.map((event) => event.subjectId ?? event.eventId)).size;
  const unique = (names: (string | undefined)[]) => [...new Set(names.filter((name): name is string => Boolean(name?.trim())))].slice(0, MAX_NAMES);
  if (conflicts && !snoozed("conflict")) return { kind: "conflict", count: conflicts, names: unique(disputed.map((event) => input.nameOf?.(event))) };
  const added = input.watchlist.filter((entry) => entry.addedAtMs > (input.lastBriefingAtMs ?? 0));
  if (input.lastBriefingAtMs !== undefined && added.length >= WATCHLIST_MINIMUM && !snoozed("watchlist")) {
    return { kind: "watchlist", count: added.length, names: unique(added.map((entry) => entry.displayName?.trim() || entry.symbol)) };
  }
  return null;
}

type Stored = Record<string, Partial<Record<ScheduleSuggestion["kind"], number>>>;

function read(): Stored {
  try {
    const value = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}") as unknown;
    return value && typeof value === "object" ? value as Stored : {};
  } catch {
    return {};
  }
}

/** Per device: a dismissal is a reading preference, not Memory. */
export function dismissedSuggestions(loupistId: string) {
  return read()[loupistId] ?? {};
}

export function dismissSuggestion(loupistId: string, kind: ScheduleSuggestion["kind"], nowMs = Date.now()) {
  const stored = read();
  stored[loupistId] = { ...stored[loupistId], [kind]: nowMs + SUGGESTION_SNOOZE_MS };
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(stored));
  } catch {
    // Storage can be unavailable; the suggestion then returns next time.
  }
}
