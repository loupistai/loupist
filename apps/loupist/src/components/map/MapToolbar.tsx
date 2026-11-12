import { displayPageTitle, HOST_PERSPECTIVE_TITLE } from "../../knowledge/hostDefaults";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { PlayIcon, BookOpenIcon, ChevronDownIcon, HistoryIcon, ImageIcon, ListIcon, MoreHorizontalIcon, NetworkIcon, ScanIcon, SearchIcon, SlidersHorizontalIcon, VideoIcon, WaypointsIcon } from "lucide-react";
import type { KnowledgeMapNode } from "../../contracts/knowledge";
import { useTranslation } from "../../i18n";
import { Button } from "../ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from "../ui/dropdown-menu";
import { Input } from "../ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { entityCategories, nodeCategory } from "../../knowledge/mapKinds";
import { entityLabel } from "../../knowledge/mapItems";
import { mapScopes, type MapScope } from "./localView";
import { foldProps, useScopeOverflow } from "./toolbarOverflow";
import { formatNumber } from "../../i18n/format";
export type MapFilters = { kinds: string[]; personal: boolean; disputed: boolean; metrics: boolean };
export const defaultFilters = (): MapFilters => ({ kinds: [...entityCategories], personal: false, disputed: false, metrics: true });
export function filtersActive(filters: MapFilters) {
  const expected = defaultFilters().kinds;
  return filters.personal || filters.disputed || !filters.metrics || filters.kinds.length !== expected.length || expected.some(kind => !filters.kinds.includes(kind));
}
/** Focal name and scope controls; absent when the whole Map is already small enough to read. */
export type MapLocalControls = {
  focal: KnowledgeMapNode;
  followed: KnowledgeMapNode[];
  recent: KnowledgeMapNode[];
  onFocal: (node: KnowledgeMapNode) => void;
  onPickOnMap: () => void;
  scope: MapScope;
  onScope: (scope: MapScope) => void;
};
export type MapWhatsNewControl = { count: number; open: boolean; onToggle: () => void };
const scopeKey = { "1": "map.scope1", "2": "map.scope2", all: "map.scopeAll" } as const;

export function MapToolbar({ nodes, query, onQuery, filters, onFilters, onSelect, list, onList, onFit, lenses, overlayId, onOverlay, appliedCounts, lensTitles, baseLensIds, libraryOpen, onLibrary, replayActive, onReplay, exportActions, disabled, fitDisabled = false, replayDisabled = false, listDisabled = false, empty = false, local, whatsNew }: {
  disabled?: boolean; fitDisabled?: boolean; replayDisabled?: boolean; listDisabled?: boolean;
  /** Nothing on the Map yet: What's new, Filter and More have nothing to act on. */
  empty?: boolean;
  local?: MapLocalControls; whatsNew?: MapWhatsNewControl;
  replayActive?: boolean; onReplay?: () => void;
  /** Image and replay-video export, offered in More. */
  exportActions?: { image: () => void; video: () => void; disabled?: boolean };
  lenses: KnowledgeMapNode[]; overlayId?: string; onOverlay: (id: string | undefined) => void;
  /** How many World names each Lens is already applied to, and the title to show for a base perspective. */
  appliedCounts?: Record<string, number>; lensTitles?: Record<string, string>; baseLensIds?: ReadonlySet<string>;
  libraryOpen: boolean; onLibrary: () => void;
  nodes: KnowledgeMapNode[]; query: string; onQuery: (value: string) => void; filters: MapFilters; onFilters: (value: MapFilters) => void;
  onSelect: (node: KnowledgeMapNode) => void; list: boolean; onList: () => void; onFit: () => void;
}) {
  const { t, i18n } = useTranslation("knowledge");
  const [filterOpen, setFilterOpen] = useState(false);
  const [open, setOpen] = useState(false);
  const [index, setIndex] = useState(0);
  const resultsId = useId();
  const bar = useRef<HTMLDivElement>(null);
  const scopeFolded = useScopeOverflow(bar);

  useEffect(() => setIndex(0), [nodes]);
  const choose = (node: KnowledgeMapNode) => { onSelect(node); setOpen(false); };
  const listLabel = t(list ? "map.canvasView" : "map.listView");
  return <div className="map-toolbar" ref={bar}>
    <div className="map-search"><SearchIcon aria-hidden="true" /><Input disabled={disabled} role="combobox" aria-label={t("map.searchPlaceholder")} placeholder={t("map.searchPlaceholder")} value={query} aria-expanded={open && !!query.trim()} aria-controls={resultsId} aria-autocomplete="list" aria-activedescendant={open && nodes[index] ? `${resultsId}-${index}` : undefined}
      onFocus={() => setOpen(true)} onBlur={(e) => { if (!e.currentTarget.parentElement?.contains(e.relatedTarget)) setOpen(false); }}
      onChange={(e) => { onQuery(e.target.value); setOpen(true); }} onKeyDown={(e) => {
        if (e.key === "Escape") { setOpen(false); e.stopPropagation(); }
        if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); setOpen(true); setIndex((n) => Math.max(0, Math.min(nodes.length - 1, n + (e.key === "ArrowDown" ? 1 : -1)))); }
        if (e.key === "Enter" && open && nodes[index]) { e.preventDefault(); choose(nodes[index]); }
      }} />
      {open && query.trim() ? <div className="map-search-results" id={resultsId} role="listbox" aria-label={t("map.searchResults")}>
        {nodes.slice(0, 8).map((node, i) => <button type="button" id={`${resultsId}-${i}`} role="option" aria-selected={i === index} key={node.id} onMouseDown={(e) => e.preventDefault()} onClick={() => choose(node)}><i className={`map-swatch is-${nodeCategory(node)}`} /><span>{entityLabel(node, i18n.language)}<small>{t(`map.kind.${nodeCategory(node)}`)}</small></span></button>)}
        {!nodes.length ? <p role="status">{t("map.noMatch")}</p> : null}
      </div> : null}
    </div>
    {local ? <LocalControls local={local} disabled={disabled} scopeFolded={scopeFolded} /> : null}
    {whatsNew ? <Button className="map-toolbar-button" disabled={disabled || empty} variant="ghost" data-map-whats-new aria-expanded={whatsNew.open}
      aria-label={whatsNew.count ? t("map.whatsNewCount", { count: whatsNew.count, formattedCount: formatNumber(whatsNew.count) }) : t("map.whatsNew")} title={t("map.whatsNew")} onClick={whatsNew.onToggle}>
      <HistoryIcon /><span className="map-control-label">{t("map.whatsNew")}</span>
      {whatsNew.count ? <span className="map-badge gg-num" aria-hidden="true">{whatsNew.count > 99 ? "99+" : formatNumber(whatsNew.count)}</span> : null}
    </Button> : null}
    <Popover open={filterOpen} onOpenChange={setFilterOpen}><PopoverTrigger asChild><Button className="map-toolbar-button" disabled={disabled || empty} variant="ghost" aria-label={t("map.filter")} title={t("map.filter")} data-map-filter aria-pressed={filtersActive(filters) || !!overlayId}><SlidersHorizontalIcon /><span className="map-control-label">{t("map.filter")}</span></Button></PopoverTrigger><PopoverContent align="start" className="map-filters">
      {entityCategories.map((kind) => <label key={kind}><input type="checkbox" data-map-filter-kind={kind} checked={filters.kinds.includes(kind)} onChange={(e) => onFilters({ ...filters, kinds: e.target.checked ? [...filters.kinds, kind] : filters.kinds.filter((k) => k !== kind) })} />{t(`map.kind.${kind}`)}</label>)}
      <hr />{(["personal", "disputed"] as const).map((key) => <label key={key}><input type="checkbox" checked={filters[key]} onChange={(e) => onFilters({ ...filters, [key]: e.target.checked })} />{t(`map.filter${key === "personal" ? "Personal" : "Disputed"}`)}</label>)}
      <label><input type="checkbox" checked={filters.metrics} onChange={(e) => onFilters({ ...filters, metrics: e.target.checked })} />{t("map.filterMetrics")}</label>
      {lenses.length ? <details className="map-lens-filter">
        <summary>{t("map.lensOverlay")}</summary>
        <div className="map-lens-menu"><LensMenu lenses={lenses} overlayId={overlayId} onOverlay={onOverlay} disabled={disabled} appliedCounts={appliedCounts} lensTitles={lensTitles} baseLensIds={baseLensIds} onClose={() => setFilterOpen(false)} /></div>
      </details> : null}
      {filtersActive(filters) || overlayId ? <Button disabled={disabled} variant="link" onClick={() => { onFilters(defaultFilters()); onOverlay(undefined); }}>{t("map.clearFilters")}</Button> : null}
    </PopoverContent></Popover>
    <Button className="map-toolbar-button" disabled={disabled} variant="ghost" data-map-library aria-label={t("map.library")} title={t("map.library")} aria-expanded={libraryOpen} onClick={onLibrary}><BookOpenIcon /><span className="map-control-label">{t("map.library")}</span></Button>
    <div className="map-toolbar-spacer" />
    <DropdownMenu>
      <DropdownMenuTrigger asChild><Button className="map-toolbar-button" variant="ghost" data-map-more disabled={disabled || empty} aria-label={t("map.moreActions")} title={t("map.moreActions")}><MoreHorizontalIcon /></Button></DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="map-more-menu">
        <DropdownMenuItem role="menuitemcheckbox" aria-checked={!!replayActive} data-map-replay disabled={disabled || replayDisabled} onSelect={onReplay}><PlayIcon />{t("map.replay.label")}</DropdownMenuItem>
        <DropdownMenuItem data-map-fit disabled={disabled || fitDisabled} onSelect={onFit}><ScanIcon />{t("map.fit")}</DropdownMenuItem>
        <DropdownMenuItem role="menuitemcheckbox" aria-checked={list} data-map-list disabled={disabled || listDisabled} onSelect={onList}>{list ? <WaypointsIcon /> : <ListIcon />}{listLabel}</DropdownMenuItem>
        {exportActions ? <>
          <DropdownMenuSeparator />
          <DropdownMenuLabel>{t("map.export.label")}</DropdownMenuLabel>
          <DropdownMenuItem data-map-export data-map-export-png disabled={disabled || exportActions.disabled} onSelect={exportActions.image}><ImageIcon />{t("map.export.image")}</DropdownMenuItem>
          <DropdownMenuItem data-map-export-video disabled={disabled || exportActions.disabled} onSelect={exportActions.video}><VideoIcon />{t("map.export.video")}</DropdownMenuItem>
        </> : null}
      </DropdownMenuContent>
    </DropdownMenu>
  </div>;
}

const lensGroups = ["perspective", "principle", "method", "subject", "idea"] as const;
function LensMenu({ lenses, overlayId, onOverlay, disabled, appliedCounts, lensTitles, baseLensIds, onClose }: {
  lenses: KnowledgeMapNode[]; overlayId?: string; onOverlay: (id: string | undefined) => void; disabled?: boolean; onClose: () => void;
  appliedCounts?: Record<string, number>; lensTitles?: Record<string, string>; baseLensIds?: ReadonlySet<string>;
}) {
  const { t } = useTranslation("knowledge");
  const defaultPerspective = t("lensWorkspace.defaultPerspective");
  const hostTitle = displayPageTitle("perspective", HOST_PERSPECTIVE_TITLE);
  const titleOf = (node: KnowledgeMapNode) => lensTitles?.[node.id] ?? node.label;
  const groups = lensGroups.map(id => ({
    id,
    items: lenses.filter(node => nodeCategory(node) === id).sort((a, b) => Number(baseLensIds?.has(b.id)) - Number(baseLensIds?.has(a.id)) || titleOf(a).localeCompare(titleOf(b))),
  })).filter(group => group.items.length);
  return <>
    <p className="map-lens-note">{t("map.lensMenuNote")}</p>
    <Button disabled={disabled} variant="ghost" aria-pressed={!overlayId} onClick={() => { onOverlay(undefined); onClose(); }}>{t("map.worldOnly")}</Button>
    {groups.map(group => <section key={group.id} aria-label={t(`map.kind.${group.id}`)}>
      <h3>{t(`map.kind.${group.id}`)}</h3>
      {group.items.map(lens => {
        const title = titleOf(lens);
        const applied = appliedCounts?.[lens.id] ?? 0;
        return <Button disabled={disabled} variant="ghost" key={lens.id} data-lens-id={lens.id} aria-pressed={overlayId === lens.id} onClick={() => { onOverlay(lens.id); onClose(); }}>
          <i className={`map-swatch is-${nodeCategory(lens)}`} aria-hidden="true" />
          <span>{title}{baseLensIds?.has(lens.id) && title !== hostTitle ? <small>{defaultPerspective}</small> : null}<small>{applied ? t("map.appliedTo", { count: applied }) : t("map.notApplied")}</small></span>
        </Button>;
      })}
    </section>)}
  </>;
}

function LocalControls({ local, disabled, scopeFolded }: { local: MapLocalControls; disabled?: boolean; scopeFolded: boolean }) {
  const { t, i18n } = useTranslation("knowledge");
  const [aroundOpen, setAroundOpen] = useState(false);
  const [scopeOpen, setScopeOpen] = useState(false);
  const name = entityLabel(local.focal, i18n.language);
  const choose = (node: KnowledgeMapNode) => { local.onFocal(node); setAroundOpen(false); };
  const option = (node: KnowledgeMapNode) => <Button key={node.id} variant="ghost" disabled={disabled} aria-pressed={node.id === local.focal.id} onClick={() => choose(node)}>
    {node.watched || node.held ? <i className={`map-swatch is-${node.held ? "holding" : "watching"}`} aria-hidden="true" /> : <i className={`map-swatch is-${nodeCategory(node)}`} aria-hidden="true" />}
    {entityLabel(node, i18n.language)}
  </Button>;
  const moveScope = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const next = mapScopes[Math.max(0, Math.min(mapScopes.length - 1, mapScopes.indexOf(local.scope) + (event.key === "ArrowRight" ? 1 : -1)))]!;
    local.onScope(next);
    event.currentTarget.querySelector<HTMLButtonElement>(`[data-scope="${next}"]`)?.focus();
  };
  return <>
    <Popover open={aroundOpen} onOpenChange={setAroundOpen}>
      <PopoverTrigger asChild><Button className="map-toolbar-button map-around" disabled={disabled} variant="ghost" data-map-around aria-label={t("map.around", { name })} title={t("map.around", { name })}>
        <i className="map-swatch is-watching map-around-glyph" aria-hidden="true" /><span className="map-control-label map-around-label">{t("map.around", { name })}</span><ChevronDownIcon className="map-around-chevron" />
      </Button></PopoverTrigger>
      <PopoverContent className="map-lens-menu map-around-menu" align="start" aria-label={t("map.aroundMenu")}>
        {local.followed.length ? <section aria-label={t("map.aroundWatching")}><h3>{t("map.aroundWatching")}</h3>{local.followed.slice(0, 12).map(option)}</section> : null}
        {local.recent.length ? <section aria-label={t("map.aroundRecent")}><h3>{t("map.aroundRecent")}</h3>{local.recent.map(option)}</section> : null}
        <Button variant="ghost" disabled={disabled} data-map-pick onClick={() => { setAroundOpen(false); local.onPickOnMap(); }}>{t("map.aroundPick")}</Button>
      </PopoverContent>
    </Popover>
    <div className="map-segmented map-scope-segment" {...foldProps("scope", scopeFolded)} role="group" aria-label={t("map.scope")} onKeyDown={moveScope}>
      {mapScopes.map((scope) => <button type="button" key={scope} data-scope={scope} disabled={disabled} aria-pressed={local.scope === scope} tabIndex={local.scope === scope ? 0 : -1} onClick={() => local.onScope(scope)}>{t(scopeKey[scope])}</button>)}
    </div>
    <Popover open={scopeOpen} onOpenChange={setScopeOpen}>
      <PopoverTrigger asChild><Button className="map-toolbar-button map-scope-menu-button" disabled={disabled} variant="ghost" data-map-scope-menu data-scope-folded={scopeFolded || undefined} aria-label={`${t("map.scope")}: ${t(scopeKey[local.scope])}`} title={t("map.scope")}><NetworkIcon /></Button></PopoverTrigger>
      <PopoverContent className="map-lens-menu" align="start">
        {mapScopes.map((scope) => <Button key={scope} variant="ghost" disabled={disabled} aria-pressed={local.scope === scope} onClick={() => { local.onScope(scope); setScopeOpen(false); }}>{t(scopeKey[scope])}</Button>)}
      </PopoverContent>
    </Popover>
  </>;
}
