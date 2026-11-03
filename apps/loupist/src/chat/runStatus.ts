import i18n from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { useEffect, useRef, useState } from "react";
import type { ChatMessage, ChatProgress, ChatProgressItem } from "../types";
import { localizedProgressLabel } from "./progressLabels";

const t = i18n.getFixedT(null, "chat");

/** A run with no new progress for this long shows "Still working" instead of a frozen label. */
export const HEARTBEAT_AFTER_MS = 15_000;
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

/** A step's own words: running while it runs, paused when refused, its action afterwards. */
export const actionLabel = (item: ActionItem) => {
  if (item.kind === "tool" && item.status === "failed" && item.refusal === "untrusted_text_paused") return t("chatProgressTimeline.paused.status");
  const raw = item.status === "running" && item.kind === "tool" && item.running ? item.running : item.action;
  return localizedProgressLabel(scrubIdentifiers(raw) || item.action, item.kind === "tool" ? item.toolName : undefined);
};

/** "m:ss"; the CSS sets tabular digits. */
export const clockLabel = (milliseconds: number) => {
  const total = Math.max(0, Math.floor(milliseconds / 1_000));
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${formatNumber(minutes)}:${formatNumber(seconds, { minimumIntegerDigits: 2, useGrouping: false })}`;
};

const phaseLabel = (phase: ChatMessage["phase"]) => {
  if (phase === "writing") return t("chatProgressTimeline.writing");
  if (phase === "thinking") return t("chatProgressTimeline.thinkingItThrough");
  if (phase === "tool") return t("chatProgressTimeline.waitingForNextStep");
  return t("chatProgressTimeline.waitingForModel");
};

const lastRunning = (progress: ChatProgress | undefined) =>
  [...(progress?.items ?? [])].reverse().find((item): item is ActionItem => isActionItem(item) && item.status === "running");

/** The current step's own words, never a phase word, unless no step is running. */
export function currentStepLabel(progress: ChatProgress | undefined, phase: ChatMessage["phase"]) {
  const running = lastRunning(progress);
  return running ? actionLabel(running) : phaseLabel(phase);
}

/** The step the reader last saw, for "Still working · {step}". */
export function lastStepLabel(progress: ChatProgress | undefined, phase: ChatMessage["phase"]) {
  const last = lastRunning(progress) ?? [...(progress?.items ?? [])].reverse().find(isActionItem);
  return last ? actionLabel(last) : phaseLabel(phase);
}

/** Changes whenever the host reports anything new for this run. */
export function progressActivityKey(progress: ChatProgress | undefined, phase: ChatMessage["phase"]) {
  const items = progress?.items ?? [];
  return `${phase ?? ""}|${items.map((item) => isActionItem(item) ? `${item.id}:${item.status}:${item.outputRevision ?? 0}` : `${item.id}:${item.text.length}`).join(",")}`;
}

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

/**
 * True after HEARTBEAT_AFTER_MS without a new progress event. Answer text
 * streams without progress events, so the writing phase never reads as stalled.
 */
export function useRunHeartbeat(activityKey: string, running: boolean, phase: ChatMessage["phase"], now: number) {
  const [changed, setChanged] = useState(() => ({ key: activityKey, at: Date.now() }));
  if (changed.key !== activityKey) setChanged({ key: activityKey, at: Date.now() });
  return running && phase !== "writing" && changed.key === activityKey && now - changed.at >= HEARTBEAT_AFTER_MS;
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
