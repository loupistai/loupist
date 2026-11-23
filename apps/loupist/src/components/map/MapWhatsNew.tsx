import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ChevronDownIcon, FileTextIcon, TriangleAlertIcon, XIcon, PlayIcon } from "lucide-react";
import type { KnowledgeCatalog, KnowledgeMap, WorldGrowthSource } from "../../contracts/knowledge";
import type { LoupistBridge } from "../../types";
import { useTranslation } from "../../i18n";
import { formatDate, formatNumber, formatTime } from "../../i18n/format";
import { AppDateField } from "../ui/app-date-field";
import { Button } from "../ui/button";
import { compactOrigin, sourceOrigin, sourceTitle } from "../../knowledge/hostSource";
import { extensionKindLabel } from "../../knowledge/mapKinds";
import { groupByDay, sinceMs, useGrowthSince, whatsNewEntries, whatsNewLinkText, whatsNewRows, whatsNewTotals, type WhatsNewRow, type WhatsNewSince, type WhatsNewSourceGroup } from "./whatsNew";

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
export function MapWhatsNew({ bridge, loupistId, map, onRevisionChange, lastSeenAtMs, nowMs, sources, onOpenRow, onOpenSource, onReplay, onClose }: {
  bridge: LoupistBridge; loupistId: string; map: KnowledgeMap | undefined; lastSeenAtMs: number | undefined; nowMs: number;
  onRevisionChange?: () => void;
  sources?: KnowledgeCatalog["sources"];
  onOpenSource?: (source: WorldGrowthSource) => void;
  onOpenRow: (row: WhatsNewRow) => void; onReplay: (sinceMs: number) => void; onClose: () => void;
}) {
  const { t, i18n } = useTranslation("knowledge");
  const [choice, setChoice] = useState<WhatsNewSince>(() => lastSeenAtMs === undefined ? { kind: "days", days: 7 } : { kind: "lastVisit" });
  const since = sinceMs(choice, lastSeenAtMs, nowMs);
  const load = useGrowthSince(bridge, loupistId, since, !!map, map?.generatedAtMs, map?.worldRevisionId, onRevisionChange);
  const rows = useMemo(() => whatsNewRows(load.events, map, i18n.language, since), [load.events, map, i18n.language, since]);
  const entries = useMemo(() => whatsNewEntries(rows, i18n.language), [rows, i18n.language]);
  const groups = useMemo(() => groupByDay(entries, nowMs), [entries, nowMs]);
  const [expandedSources, setExpandedSources] = useState<ReadonlySet<string>>(() => new Set());
  const counts = whatsNewTotals(rows);
  const savedSources = useMemo(() => new Map(sources?.map(record => [record.recordId, record.objects])), [sources]);
  const titleFor = (source: WorldGrowthSource) => {
    const saved = savedSources.get(source.recordId)?.find(item => item.id === source.sourceId);
    const origin = compactOrigin(saved && sourceOrigin(saved));
    return (saved && sourceTitle(saved).trim()) || source.title.trim()
      || origin.split(/[/\\]/).filter(Boolean).at(-1) || t("sourceWorkspace.sourceSaved");
  };
  const from = since ?? rows.at(-1)?.atMs ?? nowMs;
  const heading = useRef<HTMLHeadingElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const scrollAnchor = useRef<{ row: string; top: number } | undefined>(undefined);
  useLayoutEffect(() => {
    const scroller = list.current;
    if (!scroller) return;
    const anchor = scrollAnchor.current;
    if (anchor && scroller.scrollTop > 0) {
      const row = [...scroller.querySelectorAll<HTMLElement>("[data-growth-row]")].find(item => item.dataset.growthRow === anchor.row);
      if (row) scroller.scrollTop += row.getBoundingClientRect().top - anchor.top;
    }
  }, [groups]);
  useEffect(() => { heading.current?.focus({ preventScroll: true }); }, []);
  const choices: { key: string; label: string; value: WhatsNewSince }[] = [
    ...(lastSeenAtMs === undefined ? [] : [{ key: "lastVisit", label: t("map.sinceLastVisit"), value: { kind: "lastVisit" } as const }]),
    { key: "7", label: t("map.since7"), value: { kind: "days", days: 7 } },
    { key: "30", label: t("map.since30"), value: { kind: "days", days: 30 } },
    { key: "date", label: t("map.sinceDate"), value: { kind: "date", dayMs: choice.kind === "date" ? choice.dayMs : new Date(nowMs - 30 * DAY_MS).setHours(0, 0, 0, 0) } },
  ];
  const active = (value: WhatsNewSince) => value.kind === choice.kind && (value.kind !== "days" || choice.kind !== "days" || value.days === choice.days);
  const displayedCount = (count: number) => count > 99 ? "99+" : formatNumber(count);
  const summaryParts = [
    counts.names ? t("map.newNames", { count: counts.names, formattedCount: displayedCount(counts.names) }) : "",
    counts.links ? t("map.newLinks", { count: counts.links, formattedCount: displayedCount(counts.links) }) : "",
    counts.conflicts ? t("map.newConflicts", { count: counts.conflicts, formattedCount: displayedCount(counts.conflicts) }) : "",
  ].filter(Boolean);
  const dayLabel = (group: { dayMs: number; daysAgo: number }) => group.daysAgo === 0 ? t("map.today") : group.daysAgo === 1 ? t("map.yesterday") : formatDate(group.dayMs, { year: undefined, month: "short", day: "numeric", weekday: "short" });
  return <aside className="map-library map-whats-new" data-testid="map-library" role="dialog" aria-labelledby="map-whats-new-title" onKeyDown={(event) => { if (event.key === "Escape") { event.stopPropagation(); onClose(); } }}>
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
    <div className="map-whats-new-list" ref={list} onScroll={event => {
      const scroller = event.currentTarget;
      const top = scroller.getBoundingClientRect().top;
      const row = scroller.scrollTop > 0 ? [...scroller.querySelectorAll<HTMLElement>("[data-growth-row]")].find(item => item.getBoundingClientRect().bottom > top) : undefined;
      scrollAnchor.current = row?.dataset?.growthRow ? { row: row.dataset.growthRow, top: row.getBoundingClientRect().top } : undefined;
    }} aria-busy={load.status === "loading"}>
      {load.status === "loading" ? <div className="map-whats-new-skeleton" aria-hidden="true"><span /><span /><span /></div>
        : load.status === "failed" ? <div className="map-whats-new-state" role="alert"><p>{t("map.newError")}</p><Button variant="outline" size="sm" onClick={load.retry}>{t("map.retry")}</Button></div>
          : !rows.length ? <p className="map-whats-new-state" role="status">{t("map.newEmpty", { date: shortDate(from) })}</p>
            : groups.map((group) => <section key={group.dayMs} aria-labelledby={`map-whats-new-day-${group.dayMs}`}>
              <h3 id={`map-whats-new-day-${group.dayMs}`}>{dayLabel(group)}</h3>
              <ul>{group.rows.map(row => row.kind === "source"
                ? <WhatsNewGroup key={row.id} group={row} title={titleFor(row.source)} expanded={expandedSources.has(row.id)}
                  onOpenSource={onOpenSource} onOpenRow={onOpenRow} titleFor={titleFor}
                  onToggle={() => setExpandedSources(previous => {
                    const next = new Set(previous);
                    if (!next.delete(row.id)) next.add(row.id);
                    return next;
                  })} />
                : <li key={row.id} data-growth-row={row.id}><WhatsNewRowButton row={row} onOpen={onOpenRow} origin={row.source ? titleFor(row.source) : row.origin} /></li>)}</ul>
            </section>)}
    </div>
  </aside>;
}

function WhatsNewGroup({ group, title, expanded, onToggle, onOpenSource, onOpenRow, titleFor }: {
  group: WhatsNewSourceGroup; title: string; expanded: boolean; onToggle: () => void;
  onOpenSource?: (source: WorldGrowthSource) => void; onOpenRow: (row: WhatsNewRow) => void; titleFor: (source: WorldGrowthSource) => string;
}) {
  const { t } = useTranslation("knowledge");
  const id = useId();
  const titleId = `${id}-title`;
  const parts = [
    group.names ? t("map.newSourceNames", { count: group.names, formattedCount: formatNumber(group.names) }) : "",
    group.facts ? t("map.newFactCount", { count: group.facts, formattedCount: formatNumber(group.facts) }) : "",
  ].filter(Boolean);
  return <li>
    <div className="map-whats-new-group" data-growth-row={group.id}>
      <button type="button" className="map-whats-new-row" data-kind="source" onClick={() => onOpenSource?.(group.source)}>
        <span className="map-whats-new-glyph"><FileTextIcon size={14} aria-hidden="true" /></span>
        <span className="map-whats-new-text">
          <span className="map-whats-new-label" id={titleId}>{title}</span>
          {parts.length ? <small>{parts.join(" · ")}</small> : null}
        </span>
        <span className="map-whats-new-meta"><small className="gg-num">{formatTime(group.atMs)}</small></span>
      </button>
      <Button type="button" variant="ghost" size="icon-sm" aria-expanded={expanded} aria-controls={id}
        aria-label={expanded ? t("map.hideSourceChanges") : t("map.showSourceChanges", { formattedCount: formatNumber(group.rows.length) })} onClick={onToggle}>
        <ChevronDownIcon aria-hidden="true" />
      </Button>
    </div>
    <ul className="map-whats-new-children" id={id} aria-labelledby={titleId} hidden={!expanded}>
      {expanded ? group.rows.map(row => <li key={row.id} data-growth-row={row.id}>
        <WhatsNewRowButton row={row} onOpen={onOpenRow} origin={row.source ? titleFor(row.source) : row.origin} />
      </li>) : null}
    </ul>
  </li>;
}

function WhatsNewRowButton({ row, onOpen, origin }: { row: WhatsNewRow; onOpen: (row: WhatsNewRow) => void; origin?: string }) {
  const { t, i18n } = useTranslation("knowledge");
  const glyph = row.kind === "name" || row.kind === "fact" ? <i className={`map-swatch is-${row.category ?? "other"}`} aria-hidden="true" />
    : row.kind === "relationship" ? <i className="map-swatch is-claim-line" aria-hidden="true" />
      : row.kind === "conflict" ? <TriangleAlertIcon className="map-whats-new-warning" aria-hidden="true" />
        : <i className={`map-swatch is-${row.kind}`} aria-hidden="true" />;
  const linkText = row.link ? whatsNewLinkText(row.link, t) : row.label;
  const text = row.kind === "conflict" ? t("map.newConflict", { summary: linkText })
    : row.kind === "relationship" ? linkText
    : row.kind === "fact" && row.link ? t("map.newFacts", { count: row.count, formattedCount: formatNumber(row.count), name: row.link.a })
    : row.kind === "method" ? t("map.newMethod", { title: row.label })
      : row.kind === "principle" ? t("map.newPrinciple", { title: row.label })
        : row.kind === "subject" ? t("map.newSubject", { title: row.label })
          : row.kind === "idea" ? t("map.newIdea", { title: row.label }) : row.label;
  const kindLabel = row.kind === "name"
    ? extensionKindLabel(row.node?.kindLabels, i18n.language, t(`map.kind.${(row.category ?? "other") as "other"}`)) : undefined;
  const facts = kindLabel && row.facts
    ? t("map.newNameWithFacts", { kind: kindLabel, facts: t("map.newFactCount", { count: row.facts, formattedCount: formatNumber(row.facts) }) }) : undefined;
  return <button type="button" className="map-whats-new-row" data-kind={row.kind} aria-disabled={row.gone || undefined} title={row.gone ? t("map.goneFromMap") : undefined}
    onClick={() => { if (!row.gone) onOpen(row); }}>
    <span className="map-whats-new-glyph">{glyph}</span>
    <span className="map-whats-new-text">
      <span className="map-whats-new-label">{text}</span>
      {facts ? <small>{facts}</small> : null}
      {origin ? <small>{t("map.newFromSource", { title: origin })}</small> : null}
    </span>
    <span className="map-whats-new-meta">{kindLabel && !facts ? <small>{kindLabel}</small> : null}<small className="gg-num">{formatTime(row.atMs)}</small></span>
  </button>;
}
