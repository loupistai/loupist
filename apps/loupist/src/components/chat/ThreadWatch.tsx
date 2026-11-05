import i18n, { useTranslation } from "@/i18n";
import { formatNumber, formatTime } from "@/i18n/format";
import { useEffect, useState } from "react";
import type { ChatMessage, LoupistBridge } from "../../types";
import type { AssetEvidence } from "../../contracts/assets";
import type { WatchEvent } from "../../contracts/watch";
import { Button } from "../ui/button";
import { Spinner } from "../ui/spinner";
import { useAssetPageOpener } from "../assets/assetOpener";
import { useAssetNames } from "../assets/assetName";
import { citationNumbers, SourceChips } from "../assets/assetText";
import { datedWatchSentence, newestFirst } from "../assets/watchText";
import { WATCHING_TASK_ID } from "../briefings/Watching";
import "@/styles/watch.css";

const t = i18n.getFixedT(null, "chat");
/** One message names at most five events; the rest are in Schedule › Watching. */
const SHOWN = 5;
/** On screen this long, the alerts are seen and leave Needs attention. */
const SEEN_AFTER_MS = 1000;

/** Marks alerts seen once they have been on screen for a second while the window is visible. */
export function useSeenAfter(bridge: LoupistBridge | undefined, loupistId: string, eventIds: readonly string[], active = true) {
  const key = eventIds.join(",");
  useEffect(() => {
    if (!bridge?.watchEventsSeen || !active || !eventIds.length) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const begin = () => {
      if (timer) clearTimeout(timer);
      if (document.visibilityState === "hidden") return;
      timer = setTimeout(() => { void bridge.watchEventsSeen({ loupistId, eventIds: [...eventIds] }).catch(() => undefined); }, SEEN_AFTER_MS);
    };
    begin();
    document.addEventListener("visibilitychange", begin);
    return () => { if (timer) clearTimeout(timer); document.removeEventListener("visibilitychange", begin); };
  }, [bridge, loupistId, key, active]);
}

/**
 * One host message per check batch (finance-watchers.md §2.4): up to five
 * dated events with their sources, then "+{n} more". Checks ran on this
 * computer without a model call; there is no OS notification.
 */
export function ThreadWatch({ message, bridge, loupistId, loupistName, active, onOpenLink, onOpenSchedule }: {
  message: ChatMessage;
  bridge?: LoupistBridge;
  loupistId: string;
  loupistName: string;
  active?: boolean;
  onOpenLink?: (url: string) => void;
  onOpenSchedule?: (taskId: string) => void;
}) {
  useTranslation(["chat", "assets"]);
  const watch = message.watch!;
  const openAsset = useAssetPageOpener();
  const names = useAssetNames(bridge, loupistId);
  const [preparing, setPreparing] = useState<string | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const events = newestFirst(watch.events.slice(0, SHOWN));
  const more = watch.more + Math.max(0, watch.events.length - SHOWN);
  useSeenAfter(bridge, loupistId, watch.event_ids, active ?? true);
  const at = Date.parse(message.created_at);
  const numbers = citationNumbers(events.map(event => event.evidence));
  const openPage = (event: WatchEvent, section?: "results") => openAsset?.({ loupistId, assetId: event.assetId, name: names(event), section, eventId: section ? String(event.facts.eventId ?? "") || undefined : undefined });
  const openEvidence = (event: WatchEvent) => (evidence: AssetEvidence) => { if (evidence.url && onOpenLink) onOpenLink(evidence.url); else openPage(event); };
  // "Prepare before and after" captures expectations now, then shows the card.
  const prepare = async (event: WatchEvent) => {
    const eventId = typeof event.facts.eventId === "string" ? event.facts.eventId : undefined;
    if (!bridge || !eventId || preparing) return;
    setPreparing(event.eventId);
    setFailed(null);
    try {
      await bridge.earningsPrepare({ loupistId, assetId: event.assetId, eventId });
      openPage(event, "results");
    } catch { setFailed(event.eventId); }
    finally { setPreparing(null); }
  };
  const openWatching = () => onOpenSchedule?.(WATCHING_TASK_ID);
  return <article className="message assistant thread-watch" aria-label={t("watchMessage.label")}>
    <div className="message-meta">
      <span>{loupistName}</span>
      {Number.isFinite(at) ? <time dateTime={new Date(at).toISOString()}>{formatTime(at)}</time> : null}
    </div>
    <ul className="thread-watch-events">{events.map(event => {
      const name = names(event);
      return <li key={event.eventId} data-kind={event.kind}>
        <p>{datedWatchSentence(event, name)} <SourceChips evidence={event.evidence} numbers={numbers} onOpen={openEvidence(event)} /></p>
        <div className="thread-watch-actions">
          {event.kind === "results" && typeof event.facts.eventId === "string" && bridge
            ? <Button type="button" variant="outline" size="sm" disabled={preparing !== null} aria-label={`${t("watchMessage.prepare")} · ${name}`} onClick={() => void prepare(event)}>{preparing === event.eventId ? <Spinner /> : null}{t("watchMessage.prepare")}</Button>
            : openAsset ? <Button type="button" variant="outline" size="sm" aria-label={`${i18n.t("assets:page.open")} · ${name}`} onClick={() => openPage(event)}>{i18n.t("assets:page.open")}</Button> : null}
          {event.kind === "filings" && openAsset ? <Button type="button" variant="ghost" size="sm" aria-label={`${i18n.t("assets:changed.title")} · ${name}`} onClick={() => openPage(event)}>{i18n.t("assets:changed.title")}</Button> : null}
        </div>
        {failed === event.eventId ? <p className="inline-error" role="alert">{i18n.t("assets:earnings.failed", { name })}</p> : null}
      </li>;
    })}</ul>
    {more > 0 ? <button type="button" className="morning-link" onClick={openWatching}>{t("watchMessage.more", { formattedCount: formatNumber(more) })}</button> : null}
    <p className="thread-delivery-source"><button type="button" className="morning-link" onClick={openWatching}>{t("watchMessage.from")}</button></p>
  </article>;
}
