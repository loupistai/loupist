import i18n from "@/i18n";
import { formatDate } from "@/i18n/format";
import type { ChatMessage } from "../types";

const t = i18n.getFixedT(null, "chat");
const STORAGE_KEY = "loupist.chatSeen.v1";
/** Remember this many chats; the oldest visits are forgotten first. */
const MAX_CHATS = 200;

/** A day change, or "New since you left" (carrying the day when both fall on the same message). */
export type Divider = { kind: "day"; label: string } | { kind: "new"; day?: string };

type SeenMap = Record<string, number>;

function readSeen(): SeenMap {
  try {
    const parsed = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "{}") as unknown;
    return parsed && typeof parsed === "object" ? parsed as SeenMap : {};
  } catch {
    return {};
  }
}

/** When the user last had this chat in front of them on this device. */
export function chatSeenAt(chatId: string): number | undefined {
  const value = readSeen()[chatId];
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

/** Records that the user saw this chat up to now (they left it, hid the window, or sent). */
export function markChatSeen(chatId: string, atMs = Date.now()) {
  try {
    const seen = readSeen();
    seen[chatId] = atMs;
    const kept = Object.entries(seen).sort((a, b) => b[1] - a[1]).slice(0, MAX_CHATS);
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(Object.fromEntries(kept)));
  } catch {
    // A per-device convenience; without storage the divider simply does not show.
  }
}

const timeOf = (message: ChatMessage) => Date.parse(message.created_at);

/** The first answer that finished after the user last saw the chat, if any. */
export function firstNewSince(messages: readonly ChatMessage[], seenAtMs: number | undefined): string | undefined {
  if (seenAtMs === undefined) return undefined;
  return messages.find((message) => message.role === "assistant" && message.status === "complete" && timeOf(message) > seenAtMs)?.id;
}

const startOfDay = (ms: number) => { const day = new Date(ms); day.setHours(0, 0, 0, 0); return day.getTime(); };

/** "Today", "Yesterday", or a short date such as "Oct 27". */
export function dayLabel(ms: number, nowMs = Date.now()) {
  const days = Math.round((startOfDay(nowMs) - startOfDay(ms)) / 86_400_000);
  if (days === 0) return t("chatDivider.today");
  if (days === 1) return t("chatDivider.yesterday");
  return formatDate(ms, { year: new Date(ms).getFullYear() === new Date(nowMs).getFullYear() ? undefined : "numeric", month: "short", day: "numeric" });
}

/**
 * Dividers to draw before a message (chat spec X5): a day divider when the
 * previous message fell on another local day, then "New since you left" before
 * the first answer that finished while the chat was out of sight.
 */
export function dividersBefore(messages: readonly ChatMessage[], index: number, newSinceId: string | undefined, nowMs = Date.now()): Divider[] {
  const message = messages[index];
  if (!message) return [];
  const previous = messages[index - 1];
  const at = timeOf(message);
  let day: string | undefined;
  if (previous && Number.isFinite(at) && Number.isFinite(timeOf(previous)) && startOfDay(at) !== startOfDay(timeOf(previous))) {
    day = dayLabel(at, nowMs);
  } else if (!previous && Number.isFinite(at) && messages.some((other) => Number.isFinite(timeOf(other)) && startOfDay(timeOf(other)) !== startOfDay(at))) {
    // A conversation that spans days names the first day too, so the later divider has something to follow.
    day = dayLabel(at, nowMs);
  }
  // A day change and "New since you left" on the same message read as one divider.
  if (newSinceId && message.id === newSinceId) return [day ? { kind: "new", day } : { kind: "new" }];
  return day ? [{ kind: "day", label: day }] : [];
}
