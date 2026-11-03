import { useEffect, useMemo, useRef, useState } from "react";
import { TriangleAlertIcon, XIcon, PlayIcon } from "lucide-react";
import type { KnowledgeMap } from "../../contracts/knowledge";
import type { LoupistBridge } from "../../types";
import { useTranslation } from "../../i18n";
import { formatDate, formatNumber, formatTime } from "../../i18n/format";
import { AppDateField } from "../ui/app-date-field";
import { Button } from "../ui/button";
import { groupByDay, sinceMs, useGrowthSince, whatsNewCounts, whatsNewRows, type WhatsNewRow, type WhatsNewSince } from "./whatsNew";
import { relationName } from "./relations";

const shortDate = (ms: number) => formatDate(ms, { year: undefined, month: "short", day: "numeric" });
const DAY_MS = 86_400_000;
/** `YYYY-MM-DD` of a local day, for the native date input. */
const dateInputValue = (ms: number) => { const day = new Date(ms); return `${day.getFullYear()}-${String(day.getMonth() + 1).padStart(2, "0")}-${String(day.getDate()).padStart(2, "0")}`; };
const localDay = (value: string) => { const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value); return match ? new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3])).getTime() : undefined; };

/**
 * The diff of the Loupist's knowledge since a date: names, relationships,
 * conflicting evidence and Lens entries, newest day first. Rows focus the name
 * on the Map, open the relationship or the Lens entry.
 */
export function MapWhatsNew({ bridge, loupistId, map, lastSeenAtMs, nowMs, refreshKey, onOpenRow, onReplay, onClose }: {
  bridge: LoupistBridge; loupistId: string; map: KnowledgeMap | undefined; lastSeenAtMs: number | undefined; nowMs: number; refreshKey?: unknown;
  onOpenRow: (row: WhatsNewRow) => void; onReplay: (sinceMs: number) => void; onClose: () => void;
}) {
  const { t, i18n } = useTranslation("knowledge");
  const [choice, setChoice] = useState<WhatsNewSince>(() => lastSeenAtMs === undefined ? { kind: "days", days: 7 } : { kind: "lastVisit" });
  const since = sinceMs(choice, lastSeenAtMs, nowMs);
  const load = useGrowthSince(bridge, loupistId, since, true, refreshKey);
  const rows = useMemo(() => whatsNewRows(load.events, map, i18n.language), [load.events, map, i18n.language]);
  const groups = useMemo(() => groupByDay(rows, nowMs), [rows, nowMs]);
  const counts = whatsNewCounts(rows);
  const from = since ?? rows.at(-1)?.atMs ?? nowMs;
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => { heading.current?.focus({ preventScroll: true }); }, []);
  const choices: { key: string; label: string; value: WhatsNewSince }[] = [
    ...(lastSeenAtMs === undefined ? [] : [{ key: "lastVisit", label: t("map.sinceLastVisit"), value: { kind: "lastVisit" } as const }]),
    { key: "7", label: t("map.since7"), value: { kind: "days", days: 7 } },
    { key: "30", label: t("map.since30"), value: { kind: "days", days: 30 } },
    { key: "date", label: t("map.sinceDate"), value: { kind: "date", dayMs: choice.kind === "date" ? choice.dayMs : new Date(nowMs - 30 * DAY_MS).setHours(0, 0, 0, 0) } },
  ];
  const active = (value: WhatsNewSince) => value.kind === choice.kind && (value.kind !== "days" || choice.kind !== "days" || value.days === choice.days);
  const summaryParts = [
    counts.names ? t("map.newNames", { count: counts.names, formattedCount: formatNumber(counts.names) }) : "",
    counts.links ? t("map.newLinks", { count: counts.links, formattedCount: formatNumber(counts.links) }) : "",
    counts.conflicts ? t("map.newConflicts", { count: counts.conflicts, formattedCount: formatNumber(counts.conflicts) }) : "",
  ].filter(Boolean);
  const dayLabel = (group: { dayMs: number; daysAgo: number }) => group.daysAgo === 0 ? t("map.today") : group.daysAgo === 1 ? t("map.yesterday") : formatDate(group.dayMs, { year: undefined, month: "short", day: "numeric", weekday: "short" });
  return <aside className="map-library map-whats-new" role="dialog" aria-labelledby="map-whats-new-title" onKeyDown={(event) => { if (event.key === "Escape") { event.stopPropagation(); onClose(); } }}>
    <header><h2 id="map-whats-new-title" ref={heading} tabIndex={-1}>{t("map.whatsNew")}</h2><Button variant="ghost" size="icon-sm" aria-label={t("map.closeWhatsNew")} onClick={onClose}><XIcon /></Button></header>
    <div className="map-whats-new-controls">
      <div className="map-segmented" role="group" aria-label={t("map.whatsNewSince")}>
        {choices.map((item) => <button type="button" key={item.key} aria-pressed={active(item.value)} onClick={() => setChoice(item.value)}>{item.label}</button>)}
      </div>
      {choice.kind === "date" ? <AppDateField className="map-whats-new-date" aria-label={t("map.sinceDateLabel")} max={dateInputValue(nowMs)} value={dateInputValue(choice.dayMs)} onChange={(next) => { const dayMs = localDay(next); if (dayMs !== undefined) setChoice({ kind: "date", dayMs }); }} /> : null}
      {load.status === "done" && rows.length ? <>
        {/* Each count keeps its noun (no-break spaces in the strings); a line breaks only after a "·". */}
        <p className="map-whats-new-summary gg-num">{[t("map.newFrom", { from: shortDate(from) }), ...summaryParts].join(" · ")}</p>
        <Button variant="ghost" size="sm" className="map-whats-new-replay" onClick={() => onReplay(from)}><PlayIcon aria-hidden="true" />{t("map.replayThese")}</Button>
      </> : null}
    </div>
    <div className="map-whats-new-list" aria-busy={load.status === "loading"}>
      {load.status === "loading" ? <div className="map-whats-new-skeleton" aria-hidden="true"><span /><span /><span /></div>
        : load.status === "failed" ? <div className="map-whats-new-state" role="alert"><p>{t("map.newError")}</p><Button variant="outline" size="sm" onClick={load.retry}>{t("map.retry")}</Button></div>
          : !rows.length ? <p className="map-whats-new-state" role="status">{t("map.newEmpty", { date: shortDate(from) })}</p>
            : groups.map((group) => <section key={group.dayMs} aria-labelledby={`map-whats-new-day-${group.dayMs}`}>
              <h3 id={`map-whats-new-day-${group.dayMs}`}>{dayLabel(group)}</h3>
              <ul>{group.rows.map((row) => <li key={row.id}><WhatsNewRowButton row={row} onOpen={onOpenRow} /></li>)}</ul>
            </section>)}
    </div>
  </aside>;
}

function WhatsNewRowButton({ row, onOpen }: { row: WhatsNewRow; onOpen: (row: WhatsNewRow) => void }) {
  const { t } = useTranslation("knowledge");
  const glyph = row.kind === "name" ? <i className={`map-swatch is-${row.category ?? "other"}`} aria-hidden="true" />
    : row.kind === "relationship" ? <i className="map-swatch is-claim-line" aria-hidden="true" />
      : row.kind === "conflict" ? <TriangleAlertIcon className="map-whats-new-warning" aria-hidden="true" />
        : <i className={`map-swatch is-${row.kind}`} aria-hidden="true" />;
  const linkText = row.link ? row.link.relation ? t("map.newLink", { a: row.link.a, b: row.link.b, relation: relationName(row.link.relation, t) }) : t("map.newLinkPlain", { a: row.link.a, b: row.link.b }) : row.label;
  const text = row.kind === "conflict" ? t("map.newConflict", { summary: linkText })
    : row.kind === "relationship" ? linkText
    : row.kind === "method" ? t("map.newMethod", { title: row.label })
      : row.kind === "principle" ? t("map.newPrinciple", { title: row.label }) : row.label;
  const kindLabel = row.kind === "name" ? t(`map.kind.${(row.category ?? "other") as "other"}`) : undefined;
  return <button type="button" className="map-whats-new-row" data-kind={row.kind} aria-disabled={row.gone || undefined} title={row.gone ? t("map.goneFromMap") : undefined}
    onClick={() => { if (!row.gone) onOpen(row); }}>
    <span className="map-whats-new-glyph">{glyph}</span>
    <span className="map-whats-new-text">
      <span className="map-whats-new-label">{text}</span>
      {row.origin ? <small>{t("map.newFromSource", { title: row.origin })}</small> : null}
    </span>
    <span className="map-whats-new-meta">{kindLabel ? <small>{kindLabel}</small> : null}<small className="gg-num">{formatTime(row.atMs)}</small></span>
  </button>;
}
