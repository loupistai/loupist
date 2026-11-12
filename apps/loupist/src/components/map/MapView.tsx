import { CanvasBoundary } from "./CanvasBoundary";
import { useConflictHighlights } from "./useConflictHighlights";
import { mapStatsText } from "./stats";
import { XIcon } from "lucide-react";
import { Suspense, lazy, useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { LoupistBridge, KnowledgeCatalog, KnowledgeLocation, KnowledgePin } from "../../types";
import type { KnowledgeMapNode } from "../../contracts/knowledge";
import { useTranslation } from "../../i18n";
import { formatDate, formatNumber } from "../../i18n/format";
import { lastSeenAt } from "../../app/loupistVisit";
import { Button } from "../ui/button";
import { MapToolbar, defaultFilters, filtersActive, type MapFilters } from "./MapToolbar";
import { MapClaimInspector } from "./MapClaimInspector";
import { MapInspector } from "./MapInspector";
import { isNewMethod, useLearningState } from "./learningState";
import { libraryState } from "./libraryState";
import { MapLibrary } from "./MapLibrary";
import { MapReader } from "./MapReader";
import { MapReplayBar } from "./MapReplayBar";
import { useMapExport } from "./MapExport";
import { replayStart, useMapReplay, useReplayVisible } from "./replay";
import { MapLegend } from "./MapLegend";
import { dismissMapHint, mapHintDismissed, recentFocal, rememberFocal, rememberLegendOpen, rememberScope, storedFocal, storedLegendOpen, storedScope } from "./mapVisit";
import { MapWhatsNew } from "./MapWhatsNew";
import { useGrowthSince, whatsNewRows, type WhatsNewRow } from "./whatsNew";
import { defaultFocal, followedNames, localSet, neighborOrder, LOCAL_VIEW_MIN_NODES, type MapScope } from "./localView";
import { MapList } from "./MapList";
import { entityCategory, isMetricNode, nodeCategory } from "../../knowledge/mapKinds";
import { isBasePerspective, isLensNode, lensAppliedEntities, lensDisplayTitle } from "./lensReading";
import { entityLabel, matchesMapQuery } from "../../knowledge/mapItems";
import { frozenLayout } from "./layout";
import { useMapData, useMapDetail, weeklyDrawnCount } from "./useMapData";
import type { MapCanvasControls } from "./MapCanvas";
import "../../styles/map.css";
const MapCanvas = lazy(() => import("./MapCanvas"));

export type MapViewProps = {
  bridge: LoupistBridge; catalog: KnowledgeCatalog; location: KnowledgeLocation; active?: boolean; refreshKey?: unknown;
  onPatch: (location: KnowledgeLocation) => void; onOpen: (location: KnowledgeLocation) => void; onAsk: (pin: KnowledgePin, threadId?: string) => void;
  readerLocation?: KnowledgeLocation; onReaderPatch?: (location: KnowledgeLocation) => void; onBack?: () => void; onChanged?: () => void; onManageProcessing?: () => void; onOpenLink?: (url: string) => void | Promise<void>;
  onOpenChat?: (chatId: string) => void; onAskInChat?: () => void; onAddSource?: () => void;
  /** "Explain the disagreement": a new topic Chat with the entity and its disputed relations, never sent. */
  onExplainDisagreement?: (pins: KnowledgePin[]) => void;
};
function relatedNodeId(reading: KnowledgeLocation, nodes: KnowledgeMapNode[]) {
  if (reading.surface === "note") {
    const journalId = reading.selection?.kind === "journal" ? reading.selection.journalId : reading.objectId;
    return journalId ? nodes.find(node => node.kind === "journal" && (node.id === journalId || node.recordId === journalId))?.id : undefined;
  }
  if (reading.surface === "source") return nodes.find(node => node.recordId && node.recordId === reading.recordId)?.id;
  return undefined;
}
function filterMapNodes(nodes: KnowledgeMapNode[], disputed: Set<string>, filters: MapFilters) {
  return nodes.filter((node) => {
    const kindOk = node.kind === "journal" || filters.kinds.includes(entityCategory(node.entityKind));
    return kindOk && (filters.metrics || !isMetricNode(node)) && (!filters.personal || node.watched || node.held) && (!filters.disputed || disputed.has(node.id));
  });
}
export function MapView({ bridge, catalog, location, active = true, refreshKey, onPatch, onOpen, onAsk, onAskInChat, onAddSource, readerLocation, onReaderPatch = onPatch, onBack, onChanged = () => {}, onManageProcessing, onOpenLink, onOpenChat, onExplainDisagreement }: MapViewProps) {
  const { t, i18n } = useTranslation("knowledge");
  const data = useMapData(bridge, location.loupistId, refreshKey ?? catalog, active);
  const learning = useLearningState(bridge, location.loupistId);
  const learnedIds = useMemo(() => new Set(data.map?.nodes.filter(node => isNewMethod(node, learning.acknowledged)).map(node => node.id)), [data.map, learning.acknowledged]);
  const learningGlowIds = useMemo(() => {
    if (!active || !learning.glow || !data.map) return;
    const method = data.map.nodes.find(node => node.recordId === learning.glow?.recordId);
    if (method) return new Set(lensAppliedEntities(data.map.nodes, data.map.edges, method.id).map(node => node.id));
  }, [active, data.map, learning.glow]);
  const query = location.view?.search ?? "";
  const setQuery = (search: string) => onPatch({ ...location, view: { ...location.view, search } });
  const [keyboardFocus, setKeyboardFocus] = useState(false);
  const [libraryOpen, setLibraryOpen] = useState(() => libraryState(bridge, location.loupistId).open || (!!readerLocation && !readerLocation.selection && !readerLocation.recordId));
  // The legend stays as the person last left it, in every Loupist. Before that it opens on a
  // first visit, except below the floating-legend width, where it would cover the Map.
  const [legendChoice, setLegendChoice] = useState(storedLegendOpen);
  const [showMapHint, setShowMapHint] = useState(() => !mapHintDismissed(location.loupistId));
  const [whatsNewOpen, setWhatsNewOpen] = useState(false);
  const [visit, setVisit] = useState(() => ({ loupistId: location.loupistId, lastSeenAtMs: lastSeenAt(location.loupistId), nowMs: Date.now() }));
  const [focalChoice, setFocalChoice] = useState(() => storedFocal(location.loupistId));
  const [scopeChoice, setScopeChoice] = useState(() => storedScope(location.loupistId));
  /** "Replay these" shows the whole Map while it plays, then puts the scope back. */
  const [scopeOverride, setScopeOverride] = useState<MapScope>();
  const [focalHistory, setFocalHistory] = useState<string[]>([]);
  const [picking, setPicking] = useState(false);
  const [cursorId, setCursorId] = useState<string>();
  const [replayFrom, setReplayFrom] = useState<number>();
  const [pendingReplay, setPendingReplay] = useState(false);
  const viewRef = useRef<HTMLDivElement>(null);
  const [mapWidth, setMapWidth] = useState(() => window.innerWidth);
  const compactMap = mapWidth > 0 && mapWidth < 900;
  // The key strip is open until the user closes it, at every size.
  const legendOpen = legendChoice ?? true;
  useEffect(() => {
    setShowMapHint(!mapHintDismissed(location.loupistId));
    setVisit({ loupistId: location.loupistId, lastSeenAtMs: lastSeenAt(location.loupistId), nowMs: Date.now() });
    setFocalChoice(storedFocal(location.loupistId)); setScopeChoice(storedScope(location.loupistId)); setScopeOverride(undefined);
    setFocalHistory([]); setPicking(false); setCursorId(undefined); setWhatsNewOpen(false); setReplayFrom(undefined);
  }, [location.loupistId]);
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const update = () => setMapWidth(view.clientWidth);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(view);
    return () => observer.disconnect();
  }, []);
  useEffect(() => { libraryState(bridge, location.loupistId).open = libraryOpen; }, [bridge, location.loupistId, libraryOpen]);
  const [overlayId, setOverlayId] = useState<string>();
  const reading = readerLocation && (readerLocation.recordId || readerLocation.selection) ? readerLocation : undefined;
  const [debounced, setDebounced] = useState("");
  const [filters, setFilters] = useState(defaultFilters);
  useEffect(() => { if (location.view?.onlyDisputed !== undefined) setFilters(current => ({ ...current, disputed: !!location.view?.onlyDisputed })); }, [location.loupistId, location.view?.onlyDisputed, location.view?.highlightEntityIds]);
  const highlightIds = useMemo(() => location.view?.highlightEntityIds ? new Set(location.view.highlightEntityIds) : undefined, [location.view?.highlightEntityIds]);
  const [canvasFailed, setCanvasFailed] = useState(false);
  const [searchNodes, setSearchNodes] = useState<{ query: string; nodes: KnowledgeMapNode[] }>({ query: "", nodes: [] });
  const controls = useRef<MapCanvasControls | null>(null);
  const toolbar = useRef<HTMLDivElement>(null);
  const list = location.view?.listMode || canvasFailed;
  const highlights = useConflictHighlights(bridge, location, data.map);
  const baseMap = highlights.map;
  const selectedId = reading && baseMap ? relatedNodeId(reading, baseMap.nodes) : reading ? undefined : location.selection?.kind === "node" ? location.selection.id : undefined;
  const selectedClaimId = !reading && location.selection?.kind === "claim" ? location.selection.id : undefined;
  const missingDetail = useMapDetail(bridge, location, baseMap && selectedId && !baseMap.nodes.some((n) => n.id === selectedId) ? selectedId : undefined);
  const missingEntity = missingDetail.graph?.nodes.find((n) => n.id === selectedId);
  const map = useMemo(() => {
    if (!baseMap || !selectedId || baseMap.nodes.some((n) => n.id === selectedId)) return baseMap;
    const extra = searchNodes.nodes.find((n) => n.id === selectedId) ?? (missingEntity ? { id: missingEntity.id, kind: "entity" as const, entityKind: missingEntity.kind, label: missingEntity.name, degree: missingDetail.graph?.edges.length ?? 0, watched: false, held: false } : undefined);
    if (!extra) return baseMap;
    const nodes = [...baseMap.nodes.slice(0, 999), extra];
    const ids = new Set(nodes.map((n) => n.id));
    return { ...baseMap, nodes, edges: baseMap.edges.filter((e) => ids.has(e.source) && ids.has(e.target)) };
  }, [baseMap, selectedId, missingEntity, searchNodes.nodes, missingDetail.graph]);
  // The host seeds a standing Lens before the World has any drawable names.
  const worldNodes = useMemo(() => map?.nodes.filter(node => !isLensNode(node)) ?? [], [map]);
  const worldEdges = useMemo(() => map?.edges.filter(edge => edge.kind === "claim" || edge.kind === "about") ?? [], [map]);
  const hasWorldNodes = worldNodes.length > 0;
  const overlay = map?.nodes.find(n => n.id === overlayId);
  const appliedEntities = useMemo(() => overlay && map ? lensAppliedEntities(map.nodes, map.edges, overlay.id) : [], [overlay, map]);
  const selected = map?.nodes.find((node) => node.id === selectedId);
  useEffect(() => {
    if (!showMapHint || (!selected && !selectedClaimId && !reading)) return;
    dismissMapHint(location.loupistId);
    setShowMapHint(false);
  }, [showMapHint, selected, selectedClaimId, reading, location.loupistId]);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- filters reset when the Loupist changes; a later view patch does not reset them.
  useEffect(() => { setFilters({ ...defaultFilters(), disputed: !!location.view?.onlyDisputed }); setOverlayId(undefined); setSearchNodes({ query: "", nodes: [] }); }, [location.loupistId]);
  useEffect(() => { const timer = setTimeout(() => setDebounced(query.trim()), 120); return () => clearTimeout(timer); }, [query]);
  useEffect(() => {
    if (!debounced || !location.recordId || !location.revisionId) return;
    let cancelled = false;
    void bridge.knowledgeWorldQuery({ loupistId: location.loupistId, recordId: location.recordId, revisionId: location.revisionId, search: debounced, kinds: [], relations: [], bindingRoles: [], statuses: [], byteLimit: 524288, nodeLimit: 8, edgeLimit: 8 }).then((graph) => {
      if (!cancelled) setSearchNodes({ query: debounced, nodes: graph.nodes.filter((n) => `${n.name} ${n.aliases.join(" ")}`.toLocaleLowerCase().includes(debounced.toLocaleLowerCase())).map((n) => baseMap?.nodes.find((m) => m.id === n.id) ?? { id: n.id, kind: "entity", label: n.name, entityKind: n.kind, degree: 0, watched: false, held: false }) });
    }).catch(() => { if (!cancelled) setSearchNodes({ query: debounced, nodes: [] }); });
    return () => { cancelled = true; };
  }, [bridge, debounced, location.loupistId, location.recordId, location.revisionId, baseMap]);
  const searchResults = useMemo(() => {
    if (debounced !== query.trim()) return [];
    const matches = worldNodes.filter((n) => matchesMapQuery(n, debounced));
    const unique = new Map(matches.map((n) => [n.id, n]));
    if (searchNodes.query === debounced) for (const node of searchNodes.nodes) unique.set(node.id, node);
    return [...unique.values()].slice(0, 8);
  }, [worldNodes, debounced, searchNodes, query]);
  // Past sixty names the Map opens around one company; smaller Maps are already a local view.
  const localAvailable = worldNodes.length > LOCAL_VIEW_MIN_NODES;
  const focalId = map && localAvailable ? defaultFocal(worldNodes, focalChoice) : undefined;
  const focal = focalId ? map?.nodes.find((node) => node.id === focalId) : undefined;
  const scope: MapScope = focal ? scopeOverride ?? scopeChoice ?? "1" : "all";
  const local = useMemo(() => map && focalId && scope !== "all" ? localSet(worldNodes, worldEdges, focalId, scope === "2" ? 2 : 1, undefined, highlightIds ?? []) : undefined, [map, worldNodes, worldEdges, focalId, scope, highlightIds]);
  const visible = useMemo(() => {
    const disputed = new Set(worldEdges.filter((e) => e.status === "disputed").flatMap((e) => [e.source, e.target]));
    return filterMapNodes(worldNodes.filter((node) => !local || local.ids.has(node.id)), disputed, filters);
  }, [worldNodes, worldEdges, filters, local]);
  const shownIds = useMemo(() => new Set(visible.map((n) => n.id)), [visible]);
  const edges = useMemo(() => worldEdges.filter((e) => shownIds.has(e.source) && shownIds.has(e.target) && (!filters.disputed || e.status === "disputed")), [worldEdges, shownIds, filters.disputed]);
  const [recording, setRecording] = useState(false);
  const timelineStart = useMemo(() => replayFrom ?? replayStart(worldNodes, worldEdges, Date.now()), [worldNodes, worldEdges, replayFrom]);
  const replay = useMapReplay(timelineStart, location.loupistId);
  const closeReplay = () => {
    replay.close();
    if (replayFrom !== undefined) { setReplayFrom(undefined); setScopeOverride(undefined); }
    // Replay lives in "More"; focus returns there.
    toolbar.current?.querySelector<HTMLButtonElement>("[data-map-more]")?.focus();
  };
  useEffect(() => {
    if (!pendingReplay || replay.start !== replayFrom) return;
    setPendingReplay(false);
    replay.open();
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `replay.open` runs once its start is ready.
  }, [pendingReplay, replay.start, replayFrom]);
  const replayed = useReplayVisible(visible, edges, replay.active ? replay.time : undefined);
  const positions = useMemo(() => map ? frozenLayout(location.loupistId, map.worldRevisionId, worldNodes, worldEdges) : [], [map, worldNodes, worldEdges, location.loupistId]);
  const positioned = useMemo(() => positions.filter((n) => shownIds.has(n.id)), [positions, shownIds]);
  const changeFocal = (id: string) => {
    if (!focalId || id === focalId) return;
    setFocalHistory((history) => [focalId, ...history.filter((item) => item !== focalId)].slice(0, 10));
    setFocalChoice(id);
    rememberFocal(location.loupistId, id, { recent: true });
    setCursorId(undefined);
  };
  const setScope = (next: MapScope) => { setScopeOverride(undefined); setScopeChoice(next); rememberScope(location.loupistId, next); };
  const selectOnly = (node: KnowledgeMapNode) => {
    setPicking(false); setCursorId(undefined);
    // The Map reopens around the name the user last looked at.
    if (isLensNode(node) && node.recordId) {
      onOpen({ loupistId: location.loupistId, surface: "lens", recordId: node.recordId });
      return;
    }
    if (node.kind === "entity" && localAvailable) rememberFocal(location.loupistId, node.id);
    if (isMetricNode(node)) setFilters(current => current.metrics ? current : { ...current, metrics: true }); if (!map?.nodes.some(item => item.id === node.id)) setSearchNodes(current => ({ ...current, nodes: [...current.nodes.filter(item => item.id !== node.id), node] })); onPatch({ ...location, selection: { kind: "node", id: node.id } }); controls.current?.focus(node.id);
  };
  /** Selecting keeps the focal name, unless the name is outside the drawn set or the user is picking a new center. */
  const select = (node: KnowledgeMapNode) => {
    if (node.kind === "entity" && focal && (picking || (local && !local.ids.has(node.id)))) changeFocal(node.id);
    selectOnly(node);
  };
  const viewKey = `${focalId ?? ""}:${scope}`;
  // The line of the relationship chip the inspector points at.
  const [pointedEdge, setPointedEdge] = useState<string | undefined>();
  const fittedView = useRef(viewKey);
  useEffect(() => {
    if (fittedView.current === viewKey) return;
    fittedView.current = viewKey;
    const frame = requestAnimationFrame(() => controls.current?.fit());
    return () => cancelAnimationFrame(frame);
  }, [viewKey]);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `focalChoice` re-reads the stored recent list after a choice.
  const recent = useMemo(() => recentFocal(location.loupistId), [location.loupistId, focalChoice]);
  const badge = useGrowthSince(bridge, location.loupistId, visit.lastSeenAtMs, active && visit.loupistId === location.loupistId && visit.lastSeenAtMs !== undefined, data.map?.generatedAtMs);
  const badgeCount = useMemo(() => visit.lastSeenAtMs === undefined || badge.status !== "done" ? 0 : whatsNewRows(badge.events, map, i18n.language).length, [visit.lastSeenAtMs, badge.status, badge.events, map, i18n.language]);
  const openWhatsNew = () => { setLibraryOpen(false); setWhatsNewOpen(true); };
  const closeWhatsNew = () => { setWhatsNewOpen(false); toolbar.current?.querySelector<HTMLButtonElement>("[data-map-whats-new]")?.focus(); };
  useEffect(() => {
    if (!location.view?.whatsNew) return;
    openWhatsNew();
    onPatch({ ...location, view: { ...location.view, whatsNew: undefined } });
  // eslint-disable-next-line react-hooks/exhaustive-deps -- consumes the What's new request once; `onPatch` and `location` are read at that moment.
  }, [location.view?.whatsNew]);
  const narrowMap = () => (toolbar.current?.clientWidth ?? 0) > 0 && toolbar.current!.clientWidth <= 900;
  const openWhatsNewRow = (row: WhatsNewRow) => {
    if (narrowMap()) setWhatsNewOpen(false);
    if (row.kind === "name" && row.node) { changeFocal(row.node.id); selectOnly(row.node); return; }
    if ((row.kind === "relationship" || row.kind === "conflict") && row.edge && row.claimId) {
      const ends = [row.edge.source, row.edge.target].map((id) => map?.nodes.find((node) => node.id === id)).filter((node): node is KnowledgeMapNode => Boolean(node));
      const anchor = ends.find((node) => !isMetricNode(node)) ?? ends[0];
      if (anchor) changeFocal(anchor.id);
      setPicking(false); setCursorId(undefined);
      onPatch({ ...location, selection: { kind: "claim", id: row.claimId } });
      return;
    }
    if (row.recordId) onOpen({ loupistId: location.loupistId, surface: "lens", recordId: row.recordId });
  };
  const replayThese = (from: number) => {
    setWhatsNewOpen(false);
    if (focal && scope !== "all") setScopeOverride("all");
    setReplayFrom(from);
    setPendingReplay(true);
  };
  const cursor = cursorId ? map?.nodes.find((node) => node.id === cursorId) : undefined;
  const cursorIds = useMemo(() => cursorId ? new Set([cursorId]) : undefined, [cursorId]);
  const stageKeys = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget || !focal || recording) return;
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      const order = neighborOrder(focal.id, positioned, edges);
      if (!order.length) return;
      event.preventDefault();
      const at = cursorId ? order.indexOf(cursorId) : -1;
      const next = order[at < 0 ? (event.key === "ArrowRight" ? 0 : order.length - 1) : (at + (event.key === "ArrowRight" ? 1 : -1) + order.length) % order.length]!;
      setCursorId(next);
      controls.current?.focus(next);
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (event.shiftKey) { if (cursor) changeFocal(cursor.id); }
      else selectOnly(cursor ?? focal);
    } else if (event.key === "Backspace" && focalHistory.length) {
      event.preventDefault();
      const [previous, ...rest] = focalHistory;
      setFocalHistory(rest); setFocalChoice(previous); rememberFocal(location.loupistId, previous!); setCursorId(undefined);
    } else if (event.key === "1" || event.key === "2" || event.key === "0") {
      event.preventDefault();
      setScope(event.key === "0" ? "all" : event.key);
    }
  };
  const clear = () => { onPatch({ ...location, selection: undefined }); toolbar.current?.querySelector<HTMLInputElement>("input")?.focus(); };
  const stats = mapStatsText(t, replayed.nodes.length, replayed.edges.length);
  const weekly = weeklyDrawnCount(replayed.nodes);
  const showStats = !map || hasWorldNodes;
  const emptyLens = overlay && appliedEntities.length === 0 ? overlay : undefined;
  const processing = catalog.sources.some((s) => s.objects.some((o) => o.processing?.status === "queued" || o.processing?.status === "running"));
  const bounded = !!map?.counts.entity.truncated;
  const total = map ? map.counts.entity.total : 0;
  const focalName = focal ? entityLabel(focal, i18n.language) : "";
  const scopeLabel = t(scope === "1" ? "map.scope1" : scope === "2" ? "map.scope2" : "map.scopeAll");
  const totalNames = map ? map.nodes.filter((node) => node.kind === "entity").length : 0;
  const localStats = local ? t(local.capped ? "map.boundedAround" : "map.localStats", { shown: formatNumber(local.shown), total: formatNumber(local.capped ? local.total : totalNames), name: focalName }) : undefined;
  const exportStats = local ? `${stats} · ${t("map.exportScope", { name: focalName, scope: scopeLabel })}` : stats;
  const mapExport = useMapExport({ bridge, loupistId: location.loupistId, controls, replay, stats: exportStats, onRecording: setRecording });
  const conflicts = replayed.edges.filter((edge) => edge.status === "disputed").length;
  const appliedInView = appliedEntities.filter(node => shownIds.has(node.id));
  const overlayHighlight = appliedInView.length ? new Set(appliedInView.map(node => node.id)) : undefined;
  // eslint-disable-next-line react-hooks/exhaustive-deps -- Lens titles follow the interface language; `t` is the recompute trigger.
  const lensTitles = useMemo(() => Object.fromEntries((map?.nodes ?? []).filter(isLensNode).map(node => [node.id, lensDisplayTitle(node, catalog.lenses)])), [map, catalog.lenses, t]);
  const baseLensIds = useMemo(() => new Set((map?.nodes ?? []).filter(node => isBasePerspective(node, catalog.lenses)).map(node => node.id)), [map, catalog.lenses]);
  const appliedCounts = useMemo(() => Object.fromEntries((map?.nodes ?? []).filter(isLensNode).map(node => [node.id, map ? lensAppliedEntities(map.nodes, map.edges, node.id).length : 0])), [map]);
  const overlayTitle = overlay ? lensTitles[overlay.id] ?? overlay.label : "";
  const overlayOutsideScope = !!overlay && appliedEntities.length > 0 && appliedInView.length === 0;
  const noNeighbors = !!local && local.neighbors === 0 && !replay.active;
  // The coach mark yields to anything else the user opened or is reading, so notes never pile up on the canvas.
  const lensBanner = emptyLens ? <div className="map-lens-banner" role="status"><p>{t("map.overlayEmpty", { title: overlayTitle })}</p><Button variant="outline" onClick={onAskInChat} disabled={!onAskInChat}>{t("map.askInChat")}</Button><Button size="icon-sm" variant="ghost" aria-label={t("map.exitOverlay")} onClick={() => setOverlayId(undefined)}><XIcon /></Button></div> : null;
  return <div ref={viewRef} className={`map-view${recording ? " is-recording" : ""}`} data-testid="map-view" data-keyboard-focus={keyboardFocus} onPointerDownCapture={() => setKeyboardFocus(false)} onKeyDownCapture={() => setKeyboardFocus(true)} aria-busy={data.loading} onKeyDown={(e) => { if (e.key === "Escape") { if (picking) { e.preventDefault(); setPicking(false); } else if (cursorId) { e.preventDefault(); setCursorId(undefined); } else if (overlayId) { e.preventDefault(); setOverlayId(undefined); } else if (selected || selectedClaimId || reading) { e.preventDefault(); clear(); } else if (whatsNewOpen) { e.preventDefault(); closeWhatsNew(); } else if (libraryOpen) { e.preventDefault(); setLibraryOpen(false); } } }}>
    {mapExport.status}
    <div ref={toolbar}><MapToolbar disabled={recording} empty={!!map && !hasWorldNodes} replayActive={replay.active} onReplay={replay.active ? closeReplay : replay.open} exportActions={{ image: mapExport.image, video: mapExport.video, disabled: !!list || !hasWorldNodes || mapExport.busy }} lenses={(map?.nodes ?? []).filter(isLensNode)} appliedCounts={appliedCounts} lensTitles={lensTitles} baseLensIds={baseLensIds} overlayId={overlayId} onOverlay={setOverlayId} libraryOpen={libraryOpen} onLibrary={() => { setWhatsNewOpen(false); setLibraryOpen(!libraryOpen); }}
      local={focal && map ? { focal, followed: followedNames(map.nodes), recent: recent.filter((id) => id !== focal.id).flatMap((id) => map.nodes.filter((node) => node.id === id)), onFocal: (node) => changeFocal(node.id), onPickOnMap: () => setPicking(true), scope, onScope: setScope } : undefined}
      whatsNew={{ count: badgeCount, open: whatsNewOpen, onToggle: () => { if (whatsNewOpen) closeWhatsNew(); else openWhatsNew(); } }} nodes={searchResults} query={query} onQuery={setQuery} filters={filters} onFilters={setFilters} onSelect={select} list={!!list} onList={() => onPatch({ ...location, view: { ...location.view, listMode: !list } })} onFit={() => controls.current?.fit()} fitDisabled={!!map && !hasWorldNodes} replayDisabled={!!map && !hasWorldNodes} listDisabled={!!map && !hasWorldNodes} /></div>
    {lensBanner ? <div className="map-hint-row" data-map-caption={compactMap || undefined}>{lensBanner}</div> : null}
    <div inert={recording} className={`map-body${selected || selectedClaimId || reading ? " has-selection" : ""}`}>
      <div className="map-stage" tabIndex={focal && !list ? 0 : undefined} role={focal && !list ? "group" : undefined} aria-label={focal && !list ? t("map.stageLabel", { name: focalName }) : undefined} aria-keyshortcuts={focal && !list ? "ArrowLeft ArrowRight Enter Shift+Enter Backspace 1 2 0" : undefined} onKeyDown={stageKeys}>
        {focal && !list ? <p className="sr-only" aria-live="polite">{cursor ? `${entityLabel(cursor, i18n.language)} · ${t(`map.kind.${nodeCategory(cursor) as "other"}`)}` : ""}</p> : null}
        {highlights.error ? <div className="map-error" role="alert">{t("map.detailError")}<Button variant="ghost" onClick={highlights.retry}>{t("map.retry")}</Button></div> : null}
        {data.error ? <div className="map-error" role="alert">{t("map.error")}<Button variant="ghost" onClick={data.retry}>{t("map.retry")}</Button></div> : null}
        <div className="map-status-stack">{showStats ? <div className="map-stats" data-testid="map-stats" role="status">{data.loading && !map ? t("map.loading") : <><span title={bounded || local?.capped ? t("map.searchForOthers") : undefined}>{localStats ?? stats}</span>{weekly > 0 ? <span className="map-growth" title={t("map.weeklyTip")}>{t("map.weekly", { n: formatNumber(weekly) })}</span> : null}{conflicts > 0 ? <span className="map-conflict-count" title={t("map.conflictTip")}>{t("map.conflictCount", { n: formatNumber(conflicts) })}</span> : null}{processing && !!map?.nodes.length && (!replay.active || replay.playing) ? <span className="map-updating"><i />{t("map.updating")}</span> : null}{bounded ? <small title={t("map.searchForOthers")}>{t("map.bounded", { shown: formatNumber(visible.length), total: formatNumber(total) })}</small> : null}{filtersActive(filters) ? <Button variant="link" onClick={() => { setFilters(defaultFilters()); setOverlayId(undefined); }}>{t("map.clearFilters")}</Button> : null}</>}</div> : null}{overlay && !emptyLens ? <div className="map-overlay-pill" role="status" data-lens-applied={overlay.id}><span><span className="map-overlay-title">{t("map.viewingThrough", { title: overlayTitle })}</span><small>{overlayOutsideScope ? t("map.overlayOutsideScope") : t("map.overlayLines")}</small></span><div className="map-applied-names" aria-label={t("map.appliedNames")}>{appliedEntities.map(node => <button type="button" key={node.id} onClick={() => select(node)}>{entityLabel(node, i18n.language)}</button>)}</div>{overlayOutsideScope ? <Button variant="link" onClick={() => setScope("all")}>{t("map.showWholeMap")}</Button> : null}<Button size="icon-sm" variant="ghost" aria-label={t("map.exitOverlay")} onClick={() => setOverlayId(undefined)}><XIcon /></Button></div> : null}{picking ? <p className="map-pick-hint" role="status">{t("map.pickHint")}</p> : null}{noNeighbors && focal ? <div className="map-no-neighbors" role="status"><p>{t("map.noNeighbors", { name: focalName })}</p><Button variant="outline" size="sm" onClick={onAskInChat} disabled={!onAskInChat}>{t("map.askInChat")}</Button></div> : null}</div>
        {data.loading && !map ? <div className="map-skeleton" /> : map && !hasWorldNodes ? <div className="map-empty" data-testid="map-empty"><h2>{t("map.emptyTitle")}</h2><p>{t("map.emptyBody")}</p><div><Button onClick={onAskInChat} disabled={!onAskInChat}>{t("map.askInChat")}</Button>{onAddSource ? <Button variant="outline" onClick={onAddSource}>{t("map.addSource")}</Button> : null}</div></div> : map ? <>
          {list ? <MapList nodes={replayed.nodes} edges={replayed.edges} onSelect={select} onClaim={(id) => onPatch({ ...location, selection: { kind: "claim", id } })} /> : <CanvasBoundary onError={() => setCanvasFailed(true)}><Suspense fallback={<div className="map-skeleton" />}><MapCanvas learnedIds={learnedIds} learningGlowIds={learningGlowIds} replayAt={replay.active ? replay.time : undefined} key={location.loupistId} scopeKey={`${location.loupistId}:${map.worldRevisionId}:${viewKey}`} nodes={positioned} edges={edges} selectedId={selectedId} focalId={local ? focalId : undefined} overlayIds={overlayHighlight ?? highlightIds} highlightIds={highlightIds ?? cursorIds} emphasisEdgeId={pointedEdge} controls={controls} onSelect={select} onClear={clear} stats={stats} dataArrivedAt={data.arrivedAt} reserveLegend={!list} /></Suspense></CanvasBoundary>}
          {!visible.length ? <div className="map-empty" data-testid="map-empty"><p>{t("map.noFiltered")}</p><Button variant="outline" onClick={() => { setFilters(defaultFilters()); setOverlayId(undefined); }}>{t("map.clearFilters")}</Button></div> : null}
          {!list ? <MapLegend open={legendOpen} lead={showMapHint} nodes={replayed.nodes} conflicts={conflicts > 0} lensLines={!!overlayHighlight} onOpenChange={(open) => { setLegendChoice(open); rememberLegendOpen(open); }} /> : null}
        </> : null}
      </div>
      {libraryOpen ? <MapLibrary key={`${location.loupistId}:library`} bridge={bridge} catalog={catalog} location={readerLocation ?? location} onOpen={next => { onOpen(next); if ((toolbar.current?.clientWidth ?? 0) > 0 && toolbar.current!.clientWidth <= 900) setLibraryOpen(false); }} onAsk={onAsk} onChanged={onChanged} onClose={() => setLibraryOpen(false)} onAddSource={onAddSource} onManageProcessing={onManageProcessing} /> : null}
      {whatsNewOpen ? <MapWhatsNew key={`${location.loupistId}:whats-new`} bridge={bridge} loupistId={location.loupistId} map={map} lastSeenAtMs={visit.lastSeenAtMs} nowMs={visit.nowMs} refreshKey={data.map?.generatedAtMs} onOpenRow={openWhatsNewRow} onReplay={replayThese} onClose={closeWhatsNew} /> : null}
      {reading ? <MapReader key={`${location.loupistId}:${reading.surface}`} bridge={bridge} catalog={catalog} location={reading} onOpen={onOpen} onPatch={onReaderPatch} onAsk={onAsk} onChanged={onChanged} onClose={clear} onBack={onBack} onOpenLink={onOpenLink} onManageProcessing={onManageProcessing} /> : null}
      {selectedClaimId ? <MapClaimInspector bridge={bridge} location={location} claimId={selectedClaimId} onClose={clear} onOpen={onOpen} onAsk={onAsk} /> : null}
      {!reading && selected && map ? <MapInspector key={`${location.loupistId}:inspector`} node={selected} map={map} onChanged={onChanged} onOpenChat={onOpenChat} catalog={catalog} bridge={bridge} location={location} providedGraph={missingDetail.graph} onClose={() => { setPointedEdge(undefined); clear(); }} onSelect={select} onOpen={onOpen} onAsk={onAsk} onExplainDisagreement={onExplainDisagreement} onPointRelationship={setPointedEdge} /> : null}
    </div>
    {replay.active ? <MapReplayBar replay={{ ...replay, close: closeReplay }} disabled={recording} range={replayFrom !== undefined ? t("map.replayRange", { from: formatDate(replayFrom, { year: undefined, month: "short", day: "numeric" }) }) : undefined} /> : null}
  </div>;
}
