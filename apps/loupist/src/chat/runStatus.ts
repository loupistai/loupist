import i18n from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { useEffect, useRef, useState } from "react";
import type { ChatProgressItem } from "../types";
import { catalogStepLabel } from "./progressLabels";

const t = i18n.getFixedT(null, "chat");

/** Screen readers hear the live label at most this often. */
const ANNOUNCE_EVERY_MS = 5_000;

type ActionItem = Extract<ChatProgressItem, { kind: "tool" | "system" }>;

const webHash = /\bweb:[0-9a-f]{6,}\b/gi;
const privateToolId = /private[-_]tool[-_]id/gi;

export const withoutIdentifiers = (value: string) => value.replace(webHash, "").replace(privateToolId, "");

export const scrubIdentifiers = (value: string) =>
  withoutIdentifiers(value)
    .replace(/\s{2,}/g, " ")
    .replace(/\s*·\s*(?:·\s*)+/g, " · ")
    .replace(/^\s*·\s*|\s*·\s*$/g, "")
    .trim();

export const isActionItem = (item: ChatProgressItem): item is ActionItem => item.kind !== "commentary";

/**
 * A step's own words: running while it runs, paused when refused, its action
 * afterwards. A tool the catalog knows takes the catalog's localized label;
 * the host's wording stands only for a tool the catalog does not have.
 */
export const actionLabel = (item: ActionItem) => {
  if (item.kind === "system" && item.operation === "compact") {
    return t(item.status === "running" ? "conversationReview.running" : item.status === "succeeded" ? "conversationReview.done" : "conversationReview.incomplete");
  }
  if (item.kind === "tool" && item.status === "failed" && item.refusal === "untrusted_text_paused") return t("chatProgressTimeline.paused.status");
  const running = item.status === "running";
  const catalog = item.kind === "tool" ? catalogStepLabel(item.toolName, running ? "running" : "done") : undefined;
  if (catalog) return catalog;
  const raw = running && item.kind === "tool" && item.running ? item.running : item.action;
  return scrubIdentifiers(raw) || item.action;
};

const isInternalIdentifier = (value: string) =>
  /private[-_]tool[-_]id/i.test(value) || /^web:[0-9a-f]{6,}$/i.test(value.trim());

/** A step's target as the reader may see it, without internal identifiers. */
export const visibleTarget = (target: string | null | undefined) => {
  const trimmed = target?.trim();
  if (!trimmed || isInternalIdentifier(trimmed)) return undefined;
  const cleaned = scrubIdentifiers(trimmed);
  if (!cleaned || isInternalIdentifier(cleaned)) return undefined;
  return cleaned;
};

/** "m:ss"; the CSS sets tabular digits. */
export const clockLabel = (milliseconds: number) => {
  const total = Math.max(0, Math.floor(milliseconds / 1_000));
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${formatNumber(minutes)}:${formatNumber(seconds, { minimumIntegerDigits: 2, useGrouping: false })}`;
};

/** A one-second clock while `running`; frozen otherwise. */
export function useNow(running: boolean) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!running) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [running]);
  return now;
}

/** Throttles a changing label for an aria-live region. */
export function useThrottledAnnouncement(label: string, enabled: boolean) {
  const [announced, setAnnounced] = useState(enabled ? label : "");
  const lastAt = useRef(0);
  useEffect(() => {
    if (!enabled) { setAnnounced(""); return; }
    const wait = Math.max(0, lastAt.current + ANNOUNCE_EVERY_MS - Date.now());
    const timer = window.setTimeout(() => { lastAt.current = Date.now(); setAnnounced(label); }, wait);
    return () => window.clearTimeout(timer);
  }, [enabled, label]);
  return announced;
}
