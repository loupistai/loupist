import i18n, { useTranslation } from "@/i18n";
import { formatNumber, formatTime } from "@/i18n/format";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { LoupistBridge, LoupistSummary } from "../../types";
import type { AssetEvidence } from "../../contracts/assets";
import { WATCH_KIND_GROUPS, type WatchAsset, type WatchEvent, type WatchKind, type WatchSettings } from "../../contracts/watch";
import { Button } from "../ui/button";
import { useAssetPageOpener } from "../assets/assetOpener";
import { useAssetNames } from "../assets/assetName";
import { citationNumbers, shortDay, SourceChips } from "../assets/assetText";
import { newestFirst, watchKindColumn, watchSentence } from "../assets/watchText";
import { connectionText } from "../marketplace/connectionCopy";
import "@/styles/watch.css";

const t = i18n.getFixedT(null, "briefings");

/** The Schedule tab's id for the pinned Watching item (an initial task id the Thread can open). */
export const WATCHING_TASK_ID = "watching";
const tDynamic = t as unknown as (key: string) => string;
const ROWS = 50;
const DAY_MS = 86_400_000;

/** Followed assets that could be listed at all; none means the Watching item is omitted. */
export const followedCount = (settings: WatchSettings | null) => settings ? settings.watched.length + settings.notWatched.length : 0;

/** The pinned first item of the task list: active cadence or a clear setup action. */
export function WatchingItem({ settings, current, onOpen }: { settings: WatchSettings; current: boolean; onOpen: () => void }) {
  useTranslation("briefings");
  const empty = settings.watched.length === 0;
  const status = settings.consentOff ? t("watch.consentOff") : t(settings.paused ? "watch.statusPaused" : empty ? "watch.statusSetup" : "schedule.status.active");
  const state = settings.paused || settings.consentOff ? "paused" : empty ? "setup" : "active";
  const summary = empty
    ? t("watch.summaryChoose", { count: settings.notWatched.length, formattedCount: formatNumber(settings.notWatched.length) })
    : t("watch.summary", { count: settings.watched.length, formattedCount: formatNumber(settings.watched.length) });
  return <li className="schedule-task watching-item" data-schedule-task={WATCHING_TASK_ID} data-status={state} data-current={current || undefined}>
    <div className="schedule-task-row">
      <button type="button" className="schedule-task-summary" aria-current={current || undefined} onClick={onOpen}>
        <span className="schedule-task-heading"><strong>{t("watch.title")}</strong><span className="schedule-task-status">{status}</span></span>
        <span className="briefings-meta">{summary}</span>
      </button>
    </div>
  </li>;
}

/**
 * Schedule › Watching (finance-watchers.md §2.3): what this Loupist checks on
 * this computer, hourly while the app is open, and what it found. The privacy
 * line is always shown; nothing here starts a model run.
 */
/** The sideways fade is only for a table that is actually wider than the pane. */
function MatrixScroll({ group, label, children }: { group: string; label: string; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [overflows, setOverflows] = useState(false);
  useEffect(() => {
    const el = ref.current;
    if (!el) return undefined;
    const measure = () => setOverflows(el.scrollWidth > el.clientWidth + 1);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    const table = el.querySelector("table");
    if (table) observer.observe(table);
    return () => observer.disconnect();
  }, [children]);
  return <div ref={ref} className="watching-matrix-scroll" role="region" tabIndex={0} data-group={group} data-overflows={overflows || undefined} aria-label={label}>{children}</div>;
}

export function WatchingReader({ bridge, loupist, settings, onBack }: {
  bridge: LoupistBridge;
  loupist: LoupistSummary;
  settings: WatchSettings;
  onBack: () => void;
}) {
  useTranslation(["briefings", "assets", "chat"]);
  const loupistId = loupist.id;
  const openAsset = useAssetPageOpener();
  const names = useAssetNames(bridge, loupistId);
  const [events, setEvents] = useState<WatchEvent[] | null>(null);
  const [allRows, setAllRows] = useState(false);
  const [shownAlerts, setShownAlerts] = useState(ROWS);

  const loadEvents = useCallback(async () => {
    try { setEvents(await bridge.watchEventsList({ loupistId, sinceMs: Date.now() - 30 * DAY_MS, limit: 200 })); }
    catch { setEvents([]); }
  }, [bridge, loupistId]);
  useEffect(() => { void loadEvents(); }, [loadEvents, settings.lastCheckedMs]);
  // Opening Watching shows every alert: they are seen.
  const unseen = useMemo(() => (events ?? []).filter(event => !event.seenAtMs).map(event => event.eventId), [events]);
  useEffect(() => {
    if (!unseen.length) return;
    void bridge.watchEventsSeen({ loupistId, eventIds: unseen }).catch(() => undefined);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- the joined ids are the key, so the same unseen events are marked once.
  }, [bridge, loupistId, unseen.join(",")]);

  const nameOf = (asset: Pick<WatchAsset, "name" | "assetId" | "symbol" | "market">) => names(asset);
  const rows = allRows ? settings.watched : settings.watched.slice(0, ROWS);
  // One table per asset class, each with its own kinds as columns.
  const groups = WATCH_KIND_GROUPS
    .map(group => ({ ...group, rows: rows.filter(asset => asset.kinds.some(state => (group.kinds as readonly WatchKind[]).includes(state.kind))) }))
    .filter(group => group.rows.length);
  // Newest first by the date each fact happened; Rust's detection order breaks ties.
  const sorted = newestFirst(events ?? []);
  const alerts = sorted.slice(0, shownAlerts);
  // A kind that needs a connection says so once under the matrix, beside its disabled boxes.
  const needed = new Map<WatchKind, string>();
  for (const asset of rows) for (const state of asset.kinds) if (state.needsConnection && !needed.has(state.kind)) needed.set(state.kind, connectionText(state.needsConnection.name));
  const numbers = citationNumbers(alerts.map(event => event.evidence));
  const noWatchedAssets = settings.watched.length === 0;
  const openPage = (event: Pick<WatchEvent, "assetId" | "name" | "symbol" | "market">) => openAsset?.({ loupistId, assetId: event.assetId, name: nameOf(event) });
  const openEvidence = (event: WatchEvent) => (evidence: AssetEvidence) => { if (evidence.url) void bridge.openExternalUrl(evidence.url); else openPage(event); };
  const status = settings.paused ? <><strong className="watching-paused">{t("watch.statusPaused")}</strong> · {t("watch.pausedDetail", { loupist: loupist.name })}</>
    : settings.consentOff ? t("watch.consentOff")
      : noWatchedAssets ? t("watch.setupDetail")
    : settings.lastCheckedMs ? t("watch.cadence", { loupist: loupist.name, time: formatTime(settings.lastCheckedMs) })
      : t("watch.cadenceFirst", { loupist: loupist.name });

  return <article className="briefings-reader watching-reader" aria-label={t("watch.title")}>
    <Button variant="ghost" className="briefings-back" onClick={onBack}>{t("schedule.back")}</Button>
    <header className="watching-head">
      <div>
        <h2>{t("watch.title")}</h2>
        <p className="briefings-meta watching-status" data-paused={settings.paused || undefined}>{status}</p>
      </div>
    </header>
    <p className="watching-privacy">{t("watch.privacy")}</p>
    {/* Each matrix scrolls sideways inside its own box; the asset column stays. With more than one class, each table names its class. */}
    {groups.map(group => {
      const label = tDynamic(`watch.group.${group.id}`);
      return <MatrixScroll key={group.id} group={group.id} label={groups.length > 1 ? `${label} · ${t("watch.scroll")}` : t("watch.scroll")}>
        {groups.length > 1 ? <p className="watching-group" aria-hidden="true">{label}</p> : null}
        <table className="watching-matrix" data-paused={settings.paused || undefined}>
          <caption className="sr-only">{groups.length > 1 ? `${t("watch.title")} · ${label}` : t("watch.title")}</caption>
          <thead><tr>
            <th scope="col" className="watching-asset-head"><span className="sr-only">{t("watch.asset")}</span></th>
            {group.kinds.map(kind => <th key={kind} scope="col" className="watching-kind">{watchKindColumn(kind)}</th>)}
            <th scope="col" className="watching-last">{t("watch.lastAlert")}</th>
          </tr></thead>
          <tbody>{group.rows.map(asset => {
            const name = nameOf(asset);
            return <tr key={asset.assetId} data-asset={asset.assetId}>
              <th scope="row" className="watching-asset-cell"><span className="watching-asset"><strong>{name}</strong><small className="gg-num">{asset.symbol} · {asset.market}</small></span></th>
              {group.kinds.map(kind => <td key={kind} className="watching-kind">
                {asset.kinds.find(state => state.kind === kind)?.enabled ? "✓" : "—"}
              </td>)}
              <td className="watching-last">{asset.lastAlert ? <><span className="watching-last-date gg-num">{shortDay(asset.lastAlert.factDate)}</span><span className="watching-last-kind">{watchKindColumn(asset.lastAlert.kind)}</span></> : "—"}</td>
            </tr>;
          })}</tbody>
        </table>
      </MatrixScroll>;
    })}
    {[...needed].map(([kind, connection]) => <p key={kind} className="watching-hint">{t("watch.kindNeeds", { kind: watchKindColumn(kind), connection })}</p>)}
    {settings.watched.length > rows.length ? <Button variant="ghost" size="sm" onClick={() => setAllRows(true)}>{t("watch.showAll")}</Button> : null}

    <section className="watching-section" aria-labelledby="watching-recent">
      <h3 id="watching-recent" className="schedule-section-label">{t("watch.recent")}</h3>
      {events === null ? <p role="status">{t("loading")}</p>
        : alerts.length ? <ol className="watching-alerts">{alerts.map(event => {
          const name = nameOf(event);
          return <li key={event.eventId} className="watching-alert">
            <span className="asset-change-day gg-num">{shortDay(event.factDate)}</span>
            <p>{watchSentence(event, name, { dated: true })} <SourceChips evidence={event.evidence} numbers={numbers} onOpen={openEvidence(event)} /></p>
            {openAsset ? <Button variant="ghost" size="sm" aria-label={`${i18n.t("assets:page.open")} · ${name}`} onClick={() => openPage(event)}>{i18n.t("assets:page.open")}</Button> : null}
          </li>;
        })}</ol>
          : <p className="briefings-meta" data-watching-empty="">{t("watch.empty")}</p>}
      {(events?.length ?? 0) > alerts.length ? <Button variant="ghost" size="sm" onClick={() => setShownAlerts(value => value + ROWS)}>{t("watch.showEarlier")}</Button> : null}
    </section>

  </article>;
}
