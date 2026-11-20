import { createLabelFrame } from "./labelFrame";
import { conflictMark, drawnRadius, LIVE_GLOW_REACH, nodeRings, placeLabels, showMetricLabel } from "./labels";
import { entityLabel, isDrawnRelation } from "../../knowledge/mapItems";
import { relationName } from "./relations";
import { LOCAL_VIEW_MIN_NODES } from "./localView";
import { useEffect, useMemo, useRef, useState, type MutableRefObject } from "react";
import ForceGraph, { type ForceGraphMethods } from "react-force-graph-2d";
import type { KnowledgeMapEdge, KnowledgeMapNode } from "../../contracts/knowledge";
import { useTranslation } from "../../i18n";
import { dateValue } from "../../knowledge/presentation";
import { isMetricNode, nodeCategory, nodeColorKey, nodeRadius } from "../../knowledge/mapKinds";
import { presentAt, useReducedMapMotion } from "./replay";
import type { PositionedNode } from "./layout";
export type MapCanvasControls = { fit: () => void; focus: (id: string) => void; canvas: () => HTMLCanvasElement | null; exportLabels?: (enabled: boolean) => void };
export type MapCanvasLink = Omit<KnowledgeMapEdge, "source" | "target"> & { source: string | PositionedNode; target: string | PositionedNode };
type Link = MapCanvasLink;
const endpoint = (value: Link["source"]) => typeof value === "string" ? value : value.id;
function readColors() {
  const style = getComputedStyle(document.documentElement);
  const colors: Record<string, string> = {};
  for (const name of ["bg", "company", "product", "technology", "industry", "person", "market", "event", "metric", "other", "method", "perspective", "principle", "subject", "idea", "followed-ring", "edge", "edge-active", "edge-disputed", "label", "label-halo", "new-glow"]) colors[name] = style.getPropertyValue(`--gg-color-map-${name}`).trim() || "transparent";
  for (const [key, token] of [["focus", "focus"], ["principle-mark", "bg-raised"], ["dim-label", "text-tertiary"]]) colors[key] = style.getPropertyValue(`--gg-color-${token}`).trim() || "transparent";
  colors.journal = style.getPropertyValue("--gg-color-text-primary").trim() || "transparent";
  colors.warning = style.getPropertyValue("--gg-color-map-edge-disputed").trim() || "transparent";
  colors.accent = style.getPropertyValue("--gg-color-accent").trim() || "transparent";
  const spring = (style.getPropertyValue("--gg-ease-spring").match(/-?\d*\.?\d+/g) ?? [".34", "1.4", ".64", "1"]).map(Number);
  const pulseMs = parseFloat(style.getPropertyValue("--gg-motion-map-enter")) || 600;
  const staggerMs = parseFloat(style.getPropertyValue("--gg-motion-map-stagger")) || 40;
  const dark = document.documentElement.dataset.theme === "dark";
  const selectionAlpha = dark ? .3 : .22;
  return { spring, pulseMs, staggerMs, colors, selectionAlpha, dark, dim: Number(style.getPropertyValue("--gg-color-map-dim")) || .25, font: style.getPropertyValue("--gg-font-ui").trim() || "sans-serif" };
}
/** Names a running reply reads keep the glow token at full strength; after the run it fades over the growth glow's tail. */
const LIVE_GLOW = 1;
const LIVE_FADE_MS = 3400;
/** Names entering or leaving on a scope or focal change fade over this long (map spec §5). */
export const SCOPE_FADE_MS = 120;
/** Opening a Map, names spread out from the middle of their drawing and settle into place over this long. */
export const SETTLE_MS = 700;
/** Pointing at or selecting a name brightens its neighbourhood and dims the rest over this long. */
export const FOCUS_FADE_MS = 160;
/** 0 below `from`, 1 above `to`, eased between: names fade in as the Map zooms in. */
const ramp = (value: number, from: number, to: number) => { const t = Math.min(1, Math.max(0, (value - from) / (to - from))); return t * t * (3 - 2 * t); };
type Offset = { dx: number; dy: number };
type Focus = { ids: ReadonlySet<string> | null; id?: string; both: boolean };
type ScopeFade = { key: string; start: number; entering: ReadonlySet<string>; leaving: PositionedNode[]; leavingEdges: KnowledgeMapEdge[] };
// Evaluate the CSS spring curve for the two halves of the canvas pulse.
function springProgress(progress: number, [x1, y1, x2, y2]: number[]) {
  const cubic = (t: number, a: number, b: number) => 3 * (1 - t) ** 2 * t * a + 3 * (1 - t) * t ** 2 * b + t ** 3;
  let low = 0, high = 1;
  for (let i = 0; i < 14; i++) { const mid = (low + high) / 2; if (cubic(mid, x1, x2) < progress) low = mid; else high = mid; }
  return cubic((low + high) / 2, y1, y2);
}
function parseRgb(value: string): [number, number, number] | null {
  const hex = /^#([\da-f]{6})$/i.exec(value.trim());
  if (hex) { const n = Number.parseInt(hex[1], 16); return [(n >> 16) & 255, (n >> 8) & 255, n & 255]; }
  const rgb = /rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)/i.exec(value);
  return rgb ? [Number(rgb[1]), Number(rgb[2]), Number(rgb[3])] : null;
}
/** A soft light around a dot: its colour (`rgb`, "r,g,b") at `alpha` on the dot's rim, gone `reach` further out. */
function glow(ctx: CanvasRenderingContext2D, rgb: string | undefined, x: number, y: number, radius: number, reach: number, alpha: number) {
  if (!rgb) return;
  const light = ctx.createRadialGradient(x, y, radius, x, y, radius + reach);
  light.addColorStop(0, `rgba(${rgb},${alpha})`); light.addColorStop(1, `rgba(${rgb},0)`); // token-exception: a map token's own channels, fading out
  ctx.fillStyle = light; ctx.beginPath(); ctx.arc(x, y, radius + reach, 0, Math.PI * 2); ctx.fill();
}
const linearChannel = (value: number) => { const s = value / 255; return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; };
const luminance = (rgb: [number, number, number]) => 0.2126 * linearChannel(rgb[0]) + 0.7152 * linearChannel(rgb[1]) + 0.0722 * linearChannel(rgb[2]);
/** Lowest alpha of `foreground` over `background` that reaches a 3:1 contrast. */
export function coverAlpha(foreground: string, background: string, minimum = 3) {
  const fg = parseRgb(foreground), bg = parseRgb(background);
  if (!fg || !bg) return 1;
  const bgL = luminance(bg);
  const ratio = (alpha: number) => {
    const blended = luminance(fg.map((channel, index) => channel * alpha + bg[index] * (1 - alpha)) as [number, number, number]);
    return (Math.max(blended, bgL) + 0.05) / (Math.min(blended, bgL) + 0.05);
  };
  if (ratio(1) < minimum) return 1;
  let low = 0, high = 1;
  for (let step = 0; step < 18; step++) { const mid = (low + high) / 2; if (ratio(mid) >= minimum) high = mid; else low = mid; }
  return high;
}
type Camera = { x: number; y: number; scale: number };
/** A name the renderer has given a position. Until it has, its x and y are undefined and any sum over them is NaN. */
const placedNode = (node: PositionedNode) => Number.isFinite(node.x) && Number.isFinite(node.y);
const finiteCamera = (view: Camera) => Number.isFinite(view.x) && Number.isFinite(view.y) && Number.isFinite(view.scale) && view.scale > 0;
const cameras = new Map<string, Camera>();
/** Screen radius stays at most 1.5× the unscaled node radius, except that a handful
 *  of names may draw larger so a small map does not sit as a speck in its canvas. */
const MAX_FIT_ZOOM = 1.5;
// A young map fills the canvas: up to 60 names may zoom in further than a large one.
const fitZoomCap = (count: number) => count <= 12 ? 3 : count <= 60 ? 2.6 : MAX_FIT_ZOOM;
/** Up to this many names, a fit never zooms out so far that a dot is under 6 px across. */
const MIN_DOT_FIT_COUNT = 150;
const minFitZoom = (shown: readonly PositionedNode[]) => shown.length > MIN_DOT_FIT_COUNT ? .3 : Math.max(.3, 3 / Math.min(...shown.map(nodeRadius)));
/** Below this canvas size the legend floats over the map instead of shrinking the fit. */
/** Labelled before the rest: the best-connected names. */
const TOP_HUBS = 10;
/** Room kept between a fitted drawing's outermost dots and the free area's edge: 24 px, less in a short strip such as the onboarding mini map. */
const fitPadding = (width: number, height: number) => Math.min(24, Math.max(12, Math.min(width, height) * .1));
const MIN_SELECTION_STRIP = 120;
type ScreenBox = { l: number; t: number; r: number; b: number };
type PanelRect = { left: number; right: number; top: number; bottom: number; width: number; height: number };
/**
 * Where a panel sits once it has arrived: its drawn box less its own translation. What's new slides
 * in from 24 px to the left, and nothing resizes when it lands, so a box read while it moves would
 * leave the overlays and the framing 24 px under it.
 */
function arrivedRect(element: Element): PanelRect {
  const rect = element.getBoundingClientRect();
  const matrix = /^matrix(3d)?\(([^)]+)\)$/.exec(getComputedStyle(element).transform);
  const values = matrix?.[2]!.split(",").map(Number) ?? [];
  const [dx, dy] = (matrix?.[1] ? [values[12], values[13]] : [values[4], values[5]]).map(value => Number.isFinite(value) ? value! : 0);
  return { left: rect.left - dx!, right: rect.right - dx!, top: rect.top - dy!, bottom: rect.bottom - dy!, width: rect.width, height: rect.height };
}
type Insets = { left: number; right: number; bottom: number; top: number; card: ScreenBox | null; obstacles: ScreenBox[] };
const sameCard = (a: ScreenBox | null, b: ScreenBox | null) => a === b || (!!a && !!b && a.l === b.l && a.t === b.t && a.r === b.r && a.b === b.b);
function freeBox(size: { width: number; height: number }, insets: Insets) {
  const x = insets.left, y = insets.top;
  const w = Math.max(0, size.width - insets.left - insets.right);
  const h = Math.max(0, size.height - insets.top - insets.bottom);
  const card = insets.card;
  if (!card) return { x, y, w, h };
  const candidates = [
    { x, y, w, h: Math.max(0, card.t - y) },
    { x, y: Math.max(y, card.b), w, h: Math.max(0, y + h - card.b) },
    { x, y, w: Math.max(0, card.l - x), h },
    { x: Math.max(x, card.r), y, w: Math.max(0, x + w - card.r), h },
  ].filter(box => box.w > 48 && box.h > 48);
  return candidates.sort((a, b) => b.w * b.h - a.w * a.h)[0] ?? { x, y, w, h };
}
type Size = { width: number; height: number };
/** The view that holds every shown name in the free area, centred on `around` when one is given. */
function framing(all: readonly PositionedNode[], size: Size, insets: Insets, centre?: PositionedNode): Camera {
  // Names without a position yet are left out: one of them would make the whole camera NaN.
  const shown = all.filter(placedNode), around = centre && placedNode(centre) ? centre : undefined;
  const box = freeBox(size, insets);
  const pad = fitPadding(box.w, box.h);
  const xs = shown.map(node => node.x), ys = shown.map(node => node.y);
  const minX = Math.min(...xs), maxX = Math.max(...xs), minY = Math.min(...ys), maxY = Math.max(...ys);
  const cx = around?.x ?? (minX + maxX) / 2, cy = around?.y ?? (minY + maxY) / 2;
  // Every dot, its own radius included, fits in half the free area either side of the centre, less the padding.
  // A dot `d` units out reaches (d + r) · zoom on screen up to the dot cap and less beyond it, so this errs small only there.
  let fitX = Infinity, fitY = Infinity;
  for (const node of shown) {
    const radius = nodeRadius(node);
    fitX = Math.min(fitX, (box.w / 2 - pad) / (Math.abs(node.x - cx) + radius));
    fitY = Math.min(fitY, (box.h / 2 - pad) / (Math.abs(node.y - cy) + radius));
  }
  const scale = Math.max(minFitZoom(shown), Math.min(fitZoomCap(shown.length), fitX, fitY));
  return { x: cx - (box.x + box.w / 2 - size.width / 2) / scale, y: cy - (box.y + box.h / 2 - size.height / 2) / scale, scale };
}
/** A saved view still shows the names: at least half sit in the free area, drawn at no less than half the fitted zoom. */
function framesNames(camera: Camera, shown: readonly PositionedNode[], size: Size, insets: Insets) {
  const box = freeBox(size, insets);
  const inside = shown.filter(node => {
    const x = size.width / 2 + (node.x - camera.x) * camera.scale, y = size.height / 2 + (node.y - camera.y) * camera.scale;
    return x >= box.x && x <= box.x + box.w && y >= box.y && y <= box.y + box.h;
  }).length;
  return inside * 2 >= shown.length && camera.scale * 2 >= framing(shown, size, insets).scale;
}
export default function MapCanvas({ nodes, edges, scopeKey, selectedId, focalId, controls, onSelect, onClear, stats, dataArrivedAt, overlayId, overlayIds, highlightIds, replayAt, compact, learnedIds, learningGlowIds, emphasisEdgeId, reserveLegend = false, reserveOverlay = false }: {
  /** A relationship the inspector points at (chip hover): its line is drawn wider. */
  emphasisEdgeId?: string;
  learnedIds?: ReadonlySet<string>; learningGlowIds?: ReadonlySet<string>;
  /** `liveNodeIds`: names the running reply reads; they glow steadily for the run, then fade. */
  compact?: { highlightIds?: ReadonlySet<string>; glowNodeIds?: ReadonlySet<string>; glowEdgeIds?: ReadonlySet<string>; liveNodeIds?: ReadonlySet<string> };
  replayAt?: number;
  overlayId?: string; overlayIds?: Set<string>; highlightIds?: ReadonlySet<string>;
  dataArrivedAt?: number;
  nodes: PositionedNode[]; edges: KnowledgeMapEdge[]; scopeKey: string; selectedId?: string; controls: MutableRefObject<MapCanvasControls | null>;
  /** The name a focal view is centred on; it is named first. */
  focalId?: string;
  onSelect: (node: KnowledgeMapNode) => void; onClear: () => void; stats: string;
  reserveLegend?: boolean; reserveOverlay?: boolean;
}) {
  const { t, i18n } = useTranslation("knowledge");
  const host = useRef<HTMLDivElement>(null);
  const reduced = useReducedMapMotion();
  const compactGlowStart = useRef(performance.now());
  const glowNodeIds = learningGlowIds ?? compact?.glowNodeIds;
  const glowEdgeIds = compact?.glowEdgeIds;
  useEffect(() => { compactGlowStart.current = performance.now(); }, [glowNodeIds, glowEdgeIds]);
  const compactOrder = useMemo(() => new Map([...(glowNodeIds ?? [])].map((id, index) => [id, index])), [glowNodeIds]);
  const compactGlowing = !reduced && Boolean(glowNodeIds?.size || glowEdgeIds?.size);
  // A steady ring is not motion, so it stays under reduced motion; only the fade after the run is skipped.
  const liveIds = compact?.liveNodeIds;
  const liveFade = useRef<{ ids: ReadonlySet<string>; start: number }>({ ids: new Set(), start: 0 });
  const priorLive = useRef(liveIds);
  const [liveFading, setLiveFading] = useState(false);
  useEffect(() => {
    const ended = [...(priorLive.current ?? [])].filter(id => !liveIds?.has(id));
    priorLive.current = liveIds;
    if (!ended.length || reduced) return;
    liveFade.current = { ids: new Set(ended), start: performance.now() };
    setLiveFading(true);
    const timer = setTimeout(() => setLiveFading(false), LIVE_FADE_MS);
    return () => { clearTimeout(timer); setLiveFading(false); };
  }, [liveIds, reduced]);
  // A scope or focal change: names that stay keep their place, names leaving fade out and
  // names entering fade in. Leaving names are drawn, not interactive, until their fade ends.
  /** Names on their way into place: each one's offset from its final position at the start. */
  const settle = useRef<{ start: number; from: ReadonlyMap<string, Offset> } | null>(null);
  /** This frame's share of each offset still to travel (negative while the spring overshoots), and the time elapsed (0 → 1). */
  const settleFrame = useRef({ factor: 0, time: 1 });
  const [settleRun, setSettleRun] = useState(0);
  const shown = useRef({ nodes, edges });
  const scopeFade = useRef<ScopeFade>({ key: scopeKey, start: 0, entering: new Set(), leaving: [], leavingEdges: [] });
  if (scopeFade.current.key !== scopeKey) {
    const before = shown.current;
    const next = new Set(nodes.map((n) => n.id));
    const prior = new Set(before.nodes.map((n) => n.id));
    const animate = !reduced && replayAt === undefined && before.nodes.length > 0;
    const leaving = animate ? before.nodes.filter((n) => !next.has(n.id)) : [];
    const gone = new Set(leaving.map((n) => n.id));
    scopeFade.current = {
      key: scopeKey,
      start: performance.now(),
      entering: animate ? new Set(nodes.filter((n) => !prior.has(n.id)).map((n) => n.id)) : new Set(),
      leaving,
      leavingEdges: before.edges.filter((e) => (gone.has(e.source) || gone.has(e.target)) && prior.has(e.source) && prior.has(e.target)),
    };
    // A name that arrives grows out of a name it links to that was already drawn.
    if (animate) {
      const byId = new Map(nodes.map((n) => [n.id, n]));
      const from = new Map<string, Offset>();
      for (const edge of edges) for (const [id, other] of [[edge.source, edge.target], [edge.target, edge.source]] as const) {
        const anchor = prior.has(other) ? byId.get(other) : undefined, node = !prior.has(id) && !from.has(id) ? byId.get(id) : undefined;
        if (anchor && node) from.set(id, { dx: anchor.x - node.x, dy: anchor.y - node.y });
      }
      if (from.size) settle.current = { start: performance.now(), from };
    }
  }
  shown.current = { nodes, edges };
  const [scopeFading, setScopeFading] = useState(false);
  useEffect(() => {
    const { entering, leaving, start } = scopeFade.current;
    // A change with nothing to fade still ends a fade the previous change started, whose timer its cleanup cleared.
    if (!entering.size && !leaving.length) { setScopeFading(false); return undefined; }
    setScopeFading(true);
    const timer = setTimeout(() => setScopeFading(false), Math.max(0, start + SCOPE_FADE_MS - performance.now()) + 16);
    return () => clearTimeout(timer);
  }, [scopeKey]);
  // Names settling into place keep the canvas drawing until they arrive.
  const [settling, setSettling] = useState(false);
  const settleStart = settle.current?.start ?? 0;
  useEffect(() => {
    if (!settle.current || reduced) { settle.current = null; setSettling(false); return undefined; }
    setSettling(true);
    const timer = setTimeout(() => { settle.current = null; settleFrame.current = { factor: 0, time: 1 }; setSettling(false); }, Math.max(0, settle.current.start + SETTLE_MS - performance.now()) + 16);
    return () => clearTimeout(timer);
  }, [settleStart, settleRun, reduced]);
  const offsetOf = (id: string) => {
    const factor = settleFrame.current.factor, from = factor ? settle.current?.from.get(id) : undefined;
    return from ? { x: from.dx * factor, y: from.dy * factor } : undefined;
  };
  const fadeLive = () => performance.now() - scopeFade.current.start < SCOPE_FADE_MS;
  /** 0 → 1 for a name entering, 1 → 0 for one leaving, 1 otherwise. */
  const scopeAlpha = (id: string) => {
    const fade = scopeFade.current;
    const progress = Math.min(1, Math.max(0, (performance.now() - fade.start) / SCOPE_FADE_MS));
    if (ghostIds.has(id)) return 1 - progress;
    return fade.entering.has(id) ? progress : 1;
  };
  const [replayAnimating, setReplayAnimating] = useState(false);
  const appearances = useRef(new Map<string, number>());
  const edgeAppearances = useRef(new Map<string, number>());
  const priorReplay = useRef<number | undefined>(undefined);
  const visibleIds = useMemo(() => new Set(nodes.filter(n => replayAt === undefined || presentAt(n, replayAt)).map(n => n.id)), [nodes, replayAt]);
  const edgeVisible = (edge: MapCanvasLink) => (replayAt === undefined || presentAt(edge, replayAt)) && visibleIds.has(endpoint(edge.source)) && visibleIds.has(endpoint(edge.target));
  useEffect(() => {
    if (replayAt === undefined || priorReplay.current === undefined || replayAt < priorReplay.current) { appearances.current.clear(); edgeAppearances.current.clear(); }
    if (replayAt !== undefined) {
      let stagger = 0;
      for (const node of nodes) {
        if (!visibleIds.has(node.id)) appearances.current.delete(node.id);
        else if (!appearances.current.has(node.id)) { appearances.current.set(node.id, performance.now() + (reduced ? 0 : Math.min(800, stagger++ * palette.staggerMs))); }
      }
      for (const edge of edges) {
        if (!edgeVisible(edge)) edgeAppearances.current.delete(edge.id);
        else if (!edgeAppearances.current.has(edge.id)) edgeAppearances.current.set(edge.id, Math.max(performance.now(), appearances.current.get(edge.source) ?? 0, appearances.current.get(edge.target) ?? 0));
      }
    }
    priorReplay.current = replayAt;
    const until = replayAt === undefined ? 0 : Math.max(0, ...appearances.current.values()) + (reduced ? 120 : palette.pulseMs + 2000);
    setReplayAnimating(until > performance.now());
    const timer = setTimeout(() => setReplayAnimating(false), Math.max(0, until - performance.now()));
    return () => clearTimeout(timer);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- replay staging re-runs only when the replay, data or motion setting changes; it reads the palette and visibility then.
  }, [replayAt, nodes, edges, reduced]);
  const api = useRef<ForceGraphMethods<PositionedNode, Link> | undefined>(undefined);
  const [size, setSize] = useState({ width: 0, height: 0 });
  const [insets, setInsets] = useState<Insets>({ left: 0, right: 0, bottom: 0, top: 0, card: null, obstacles: [] });
  const [palette, setPalette] = useState(readColors);
  // Contrast depends only on the palette, not on a node or animation frame.
  const compactDimAlpha = useMemo(() => new Map(Object.values(palette.colors).map(color => [color, Math.max(palette.dim, coverAlpha(color, palette.colors.bg))])), [palette]);
  /** Each palette colour's "r,g,b", for the glow's fading gradient. */
  const channels = useMemo(() => new Map(Object.values(palette.colors).map(color => [color, parseRgb(color)?.join(",")])), [palette]);
  const [exportLabels, setExportLabels] = useState(false);
  const disputedIds = useMemo(() => new Set(edges.filter(edge => edge.status === "disputed").flatMap(edge => [edge.source, edge.target])), [edges]);
  const [hovered, setHovered] = useState<KnowledgeMapNode | null>(null);
  const [hoveredEdge, setHoveredEdge] = useState<MapCanvasLink | null>(null);
  const [tooltip, setTooltip] = useState<KnowledgeMapNode | null>(null);
  const [pulsing, setPulsing] = useState(false);
  const pulseStart = useRef(0);
  useEffect(() => {
    if (!overlayId || window.matchMedia("(prefers-reduced-motion: reduce)").matches) { setPulsing(false); return; }
    pulseStart.current = performance.now(); setPulsing(true);
    const timer = setTimeout(() => setPulsing(false), palette.pulseMs);
    return () => clearTimeout(timer);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- pulses once per overlay change.
  }, [overlayId]);
  const [probe, setProbe] = useState("");
  const frameTimes = useRef<number[]>([]);
  const lastFrame = useRef(0);
  const firstPaint = useRef<{ start?: number; duration?: number }>({ start: dataArrivedAt });
  if (firstPaint.current.start !== dataArrivedAt) firstPaint.current = { start: dataArrivedAt };
  const camera = useRef(cameras.get(scopeKey));
  const probeEnabled = import.meta.env.DEV && new URLSearchParams(location.search).get("probe") === "1";
  const drawProbe = import.meta.env.DEV && new URLSearchParams(location.search).get("probe") === "draw";
  // Leaving names stay in the drawing only while they fade.
  // eslint-disable-next-line react-hooks/exhaustive-deps -- the leaving names live in a ref; these values are its recompute triggers.
  const ghosts = useMemo(() => fadeLive() ? { nodes: scopeFade.current.leaving, edges: scopeFade.current.leavingEdges } : { nodes: [], edges: [] }, [nodes, edges, scopeFading]);
  const ghostIds = useMemo(() => new Set(ghosts.nodes.map((n) => n.id)), [ghosts]);
  const ghostEdgeIds = useMemo(() => new Set(ghosts.edges.map((e) => e.id)), [ghosts]);
  const data = useMemo(() => {
    const positioned = [...nodes, ...ghosts.nodes].map((node) => ({ ...node }));
    const lookup = new Map(positioned.map((node) => [node.id, node]));
    // Frozen graphs never tick the renderer's simulation. Resolve endpoints here
    // so drawing does not depend on the library's deferred force initialization.
    const links: Link[] = [...edges, ...ghosts.edges].flatMap((edge) => {
      if (!isDrawnRelation(edge)) return [];
      const source = lookup.get(edge.source), target = lookup.get(edge.target);
      return source && target ? [{ ...edge, source, target }] : [];
    });
    return { nodes: positioned, links };
  }, [nodes, edges, ghosts]);
  const neighbors = useMemo(() => {
    if (compact?.highlightIds?.size) return compact.highlightIds;
    if (overlayIds) return overlayIds;
    // A selection that is not drawn (an unlinked principle) dims nothing.
    const id = hovered?.id ?? (selectedId && nodes.some((n) => n.id === selectedId) ? selectedId : undefined);
    return id ? new Set([id, ...edges.filter((e) => e.source === id || e.target === id).flatMap((e) => [e.source, e.target])]) : null;
  }, [hovered, selectedId, nodes, edges, overlayIds, compact?.highlightIds]);
  // Pointing at or selecting a name eases its neighbourhood forward and the rest back, instead of snapping.
  const focusState = useMemo<Focus>(() => ({ ids: neighbors, id: hovered?.id ?? selectedId, both: Boolean(overlayIds || compact?.highlightIds?.size) }), [neighbors, hovered, selectedId, overlayIds, compact?.highlightIds]);
  const focusFade = useRef({ from: focusState, to: focusState, start: 0 });
  if (focusFade.current.to !== focusState) focusFade.current = { from: focusFade.current.to, to: focusState, start: performance.now() };
  const focusProgress = useRef(1);
  const [focusFading, setFocusFading] = useState(false);
  useEffect(() => {
    if (reduced || !focusFade.current.start) { setFocusFading(false); return undefined; }
    setFocusFading(true);
    const timer = setTimeout(() => setFocusFading(false), FOCUS_FADE_MS + 16);
    return () => clearTimeout(timer);
  }, [focusState, reduced]);
  const hubs = useMemo(() => new Set([...nodes].sort((a, b) => b.degree - a.degree).slice(0, Math.ceil(nodes.length * .15)).map((n) => n.id)), [nodes]);
  const topHubs = useMemo(() => new Set(nodes.filter((n) => n.kind === "entity" && !isMetricNode(n) && n.degree > 0).sort((a, b) => b.degree - a.degree || a.id.localeCompare(b.id)).slice(0, TOP_HUBS).map((n) => n.id)), [nodes]);
  const selectedNeighbors = useMemo(() => new Set(edges.filter(edge => edge.source === selectedId || edge.target === selectedId).flatMap(edge => [edge.source, edge.target])), [edges, selectedId]);
  const linkedIds = useMemo(() => new Set(edges.filter(edge => edge.kind === "applies_to" && (edge.source === selectedId || edge.target === selectedId)).flatMap(edge => [edge.source, edge.target])), [edges, selectedId]);
  const learnedLinkedIds = useMemo(() => {
    const linked = new Set<string>();
    if (!learnedIds?.size) return linked;
    for (const edge of edges) {
      if (learnedIds.has(edge.source) && !learnedIds.has(edge.target)) linked.add(edge.target);
      if (learnedIds.has(edge.target) && !learnedIds.has(edge.source)) linked.add(edge.source);
    }
    return linked;
  }, [edges, learnedIds]);
  const visibleNodes = useMemo(() => data.nodes.filter(node => visibleIds.has(node.id)), [data.nodes, visibleIds]);
  useEffect(() => { const timer = setTimeout(() => setTooltip(hovered), 120); setTooltip(null); return () => clearTimeout(timer); }, [hovered]);
  useEffect(() => {
    const element = host.current;
    if (!element) return;
    const update = () => setSize({ width: element.clientWidth, height: element.clientHeight });
    update(); const observer = new ResizeObserver(update); observer.observe(element);
    const theme = new MutationObserver(() => { setPalette(readColors()); });
    theme.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "data-theme"] });
    return () => { observer.disconnect(); theme.disconnect(); };
  }, []);
  useEffect(() => {
    const element = host.current, body = element?.closest(".map-body"), root = body ?? element?.parentElement, stage = element?.parentElement;
    if (!element || !root) return;
    const measure = () => {
      const bounds = element.getBoundingClientRect();
      // The Library and What's new share the left edge; Library requests can open one over the other.
      const library = [...body?.querySelectorAll(".map-library") ?? []].map(arrivedRect).filter(rect => rect.width > 0).sort((a, b) => b.right - a.right)[0];
      const inspector = body?.querySelector(".map-inspector");
      const sheet = inspector && getComputedStyle(inspector).position === "absolute" ? arrivedRect(inspector) : undefined;
      const sidePanel = sheet && sheet.width < bounds.width && sheet.height >= bounds.height - 1;
      const cover = (rect?: DOMRect) => rect && rect.width > 0 && rect.height > 0 ? Math.max(0, rect.bottom - bounds.top) : 0;
      const stack = root.querySelector(".map-status-stack")?.getBoundingClientRect();
      const stats = root.querySelector(".map-stats")?.getBoundingClientRect();
      const lensPill = root.querySelector(".map-overlay-pill")?.getBoundingClientRect();
      const left = library ? Math.max(0, Math.min(bounds.width, library.right - bounds.left)) : 0;
      const right = sidePanel ? Math.max(0, bounds.right - sheet.left) : 0;
      const sheetBottom = sheet && !sidePanel ? Math.max(0, Math.min(bounds.height, bounds.bottom - sheet.top)) : 0;
      const parent = element.parentElement;
      // The legend reads these vars. Keep its own height out of --map-covered-bottom or it climbs forever.
      parent?.style.setProperty("--map-covered-left", `${left}px`);
      parent?.style.setProperty("--map-covered-right", `${right}px`);
      parent?.style.setProperty("--map-covered-bottom", `${sheetBottom}px`);
      const caption = (element.closest(".map-view") ?? root).querySelector("[data-map-caption]")?.getBoundingClientRect();
      const captionTop = caption && caption.height > 0 ? Math.max(0, caption.bottom - bounds.top) : 0;
      const top = Math.round(Math.max(cover(stack), cover(stats), cover(lensPill), captionTop));
      let besideStats = 0;
      for (const child of root.querySelector(".map-status-stack")?.children ?? []) {
        if (child.classList.contains("map-stats")) continue;
        besideStats = Math.max(besideStats, cover(child.getBoundingClientRect()));
      }
      const topWithoutStats = Math.round(Math.max(besideStats, cover(lensPill), captionTop));
      // A bottom sheet is the context at compact width, and a strip shorter than one node cannot hold the pill.
      const bottomSheet = Boolean(sheet && !sidePanel);
      const hidePill = bottomSheet || (bounds.height - top - sheetBottom < MIN_SELECTION_STRIP && cover(stats) > 0);
      (element.closest(".map-view") ?? body ?? root).toggleAttribute("data-hide-map-stats", hidePill);
      const cameraTop = hidePill ? topWithoutStats : top;
      parent?.style.setProperty("--map-covered-top", `${cameraTop}px`);
      // The key strip is one or two short lines at every size: its band is always reserved so no node sits under it.
      const legend = reserveLegend ? parent?.querySelector("details.map-legend")?.getBoundingClientRect() : undefined;
      const legendBand = legend && legend.height > 0 ? Math.round(Math.max(0, Math.min(bounds.height, bounds.bottom - legend.top))) : 0;
      const bottom = Math.max(sheetBottom, legendBand);
      parent?.style.setProperty("--map-overlay-bottom", `${bottom}px`);
      const cardRect = reserveOverlay ? parent?.querySelector(".map-lens-overlay-card")?.getBoundingClientRect() : undefined;
      const card = cardRect && cardRect.width > 0 && cardRect.height > 0 ? {
        l: Math.round(cardRect.left - bounds.left), t: Math.round(cardRect.top - bounds.top),
        r: Math.round(cardRect.right - bounds.left), b: Math.round(cardRect.bottom - bounds.top),
      } : null;
      const obstacleOf = (rect: DOMRect): ScreenBox | null => {
        if (rect.width <= 0 || rect.height <= 0) return null;
        const box = { l: rect.left - bounds.left, t: rect.top - bounds.top, r: rect.right - bounds.left, b: rect.bottom - bounds.top };
        return box.r > 0 && box.b > 0 && box.l < bounds.width && box.t < bounds.height ? box : null;
      };
      const seen = new Set<Element>();
      const obstacles = [...root.querySelectorAll(".map-stats, .map-overlay-pill, details.map-legend, .learning-toast"), ...document.querySelectorAll(".learning-toast"), ...(sheet ? [inspector!] : [])].flatMap(panel => {
        if (seen.has(panel) || (hidePill && panel.classList.contains("map-stats"))) return [];
        seen.add(panel);
        const box = obstacleOf(panel.getBoundingClientRect());
        return box ? [box] : [];
      });
      const next = { left: Math.round(left), right: Math.round(right), bottom, top: cameraTop, card, obstacles };
      setInsets(old => {
        const sameObstacles = old.obstacles.length === obstacles.length && old.obstacles.every((box, index) => box.l === obstacles[index].l && box.t === obstacles[index].t && box.r === obstacles[index].r && box.b === obstacles[index].b);
        return old.left === next.left && old.right === next.right && old.bottom === next.bottom && old.top === next.top && sameCard(old.card, next.card) && sameObstacles ? old : next;
      });
    };
    const observer = new ResizeObserver(measure);
    const observe = () => {
      observer.disconnect(); observer.observe(element);
      root.querySelectorAll(".map-library,.map-inspector,.map-stats,.map-overlay-pill,.map-status-stack,.map-legend,.map-first-hint,.map-lens-banner,.learning-toast,.map-lens-overlay-card").forEach(panel => observer.observe(panel));
      // The learned toast is portaled on the app shell, outside this map body.
      document.querySelectorAll(".learning-toast").forEach(panel => observer.observe(panel));
      measure();
    };
    const mutations = new MutationObserver(observe); mutations.observe(root, { childList: true, subtree: !body });
    const toastWatch = new MutationObserver(records => {
      if (records.some(record => [...record.addedNodes, ...record.removedNodes].some(node => node instanceof Element && (node.classList.contains("learning-toast") || node.classList.contains("learning-toasts") || node.querySelector(".learning-toast"))))) observe();
    });
    toastWatch.observe(document.body, { childList: true, subtree: true });
    observe();
    return () => {
      observer.disconnect(); mutations.disconnect(); toastWatch.disconnect(); (element.closest(".map-view") ?? body ?? root).removeAttribute("data-hide-map-stats");
      // An empty Map draws no canvas: its message must not keep the last canvas's insets.
      for (const name of ["--map-covered-left", "--map-covered-right", "--map-covered-top", "--map-covered-bottom", "--map-overlay-bottom"]) stage?.style.removeProperty(name);
    };
  }, [reserveLegend, reserveOverlay]);
  const panFrame = useRef(0);
  useEffect(() => () => cancelAnimationFrame(panFrame.current), []);
  const duration = () => reduced ? 0 : parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--gg-motion-normal")) || 180;
  const pan = (x: number, y: number) => {
    cancelAnimationFrame(panFrame.current);
    const graph = api.current;
    if (!graph) return;
    const ms = duration(), start = graph.centerAt();
    if (!ms || !start) { graph.centerAt(x, y, 0); return; }
    const curve = (getComputedStyle(document.documentElement).getPropertyValue("--gg-ease-in-out").match(/-?\d*\.?\d+/g) ?? [".4", "0", ".2", "1"]).map(Number);
    const began = performance.now();
    const tick = () => {
      const progress = Math.min(1, (performance.now() - began) / ms), eased = springProgress(progress, curve);
      graph.centerAt(start.x + (x - start.x) * eased, start.y + (y - start.y) * eased, 0);
      if (progress < 1) panFrame.current = requestAnimationFrame(tick);
    };
    panFrame.current = requestAnimationFrame(tick);
  };
  // d3-zoom anchors a camera move on the canvas element's own layout box, not on the size measured
  // here. A move while that box is missing or still another size lands off-centre: with no box at
  // all its target goes to the top-left corner and is saved as the scope's camera.
  const laidOut = () => {
    const canvas = host.current?.querySelector("canvas");
    return !!canvas && canvas.clientWidth > 0 && Math.abs(canvas.clientWidth - size.width) <= 1 && Math.abs(canvas.clientHeight - size.height) <= 1;
  };
  /** The camera shows an automatic framing the person has not moved since: the opening view, or a Fit.
   *  Until they pan, zoom or select, it is framed again whenever the canvas or its free area changes size. */
  const following = useRef<"frame" | "fit" | null>(null);
  const focus = (id: string) => {
    const node = data.nodes.find(n => n.id === id), graph = api.current;
    if (!node || !placedNode(node) || !graph || !laidOut()) return;
    following.current = null;
    const zoom = graph.zoom() || 1;
    // Pan only. The selected centre sits in the visible rect: canvas minus the sheet,
    // inspector, drawer, and reserved top overlays. A short strip still centres.
    const box = freeBox(size, insets);
    pan(node.x - (box.x + box.w / 2 - size.width / 2) / zoom, node.y - (box.y + box.h / 2 - size.height / 2) / zoom);
  };
  const shownNodes = () => data.nodes.filter(node => visibleIds.has(node.id));
  /** Moves the camera; false, with nothing moved, for a view that is not a finite camera. */
  const placedUntil = useRef(0);
  const place = (view: Camera, ms: number) => {
    if (!finiteCamera(view)) return false;
    cancelAnimationFrame(panFrame.current);
    // The renderer cannot stop a camera move in flight, and one that ends later puts its own view
    // back: a Fit still easing when What's new opens would undo the reframe. This move ends last.
    const now = performance.now(), inFlight = placedUntil.current - now;
    const length = inFlight > 0 ? Math.max(ms, inFlight + 17) : ms;
    if (length > 0) placedUntil.current = now + length;
    api.current?.zoom(view.scale, length);
    api.current?.centerAt(view.x, view.y, length);
    return true;
  };
  const fit = (ms = duration()) => {
    const shown = shownNodes();    if (!api.current || !shown.length || !laidOut()) return;
    if (place(framing(shown, size, insets), ms)) following.current = "fit";
  };
  /** The opening view: every shown name in the free area, centred on the selection when it is drawn. */
  const frame = (ms = 0) => {
    const shown = shownNodes();
    return !!api.current && shown.length > 0 && laidOut() && place(framing(shown, size, insets, shown.find(node => node.id === selectedId)), ms);
  };
  /** Draws one frame of the current state. The renderer has no repaint call; a zero-length zoom to the
   *  current scale is one, and it also re-applies the camera to the canvas before that frame is drawn. */
  const repaint = () => {
    const graph = api.current, scale = graph?.zoom();
    if (graph && scale) graph.zoom(scale, 0);
  };
  const drawn = size.width > 0 && size.height > 0 && nodes.length > 0;
  const opened = useRef(false), openTries = useRef(0);
  /** Places the opening view; false while the canvas element has no box of the measured size yet, or
   *  while the renderer has not yet given every shown name a position (WebKit can run this first). */
  const open = () => {
    if (opened.current) return true;
    if (!api.current || !laidOut()) return false;
    const shown = shownNodes();
    // After about two seconds, frame the names that have positions rather than wait for the rest.
    if (!shown.every(placedNode) && openTries.current++ < 120) return false;
    // A view saved for this scope, or the one the canvas had before it was hidden, returns only
    // while it still frames these names; a drawn selection always opens framed around itself.
    const saved = camera.current;
    if (saved && finiteCamera(saved) && !shown.some(node => node.id === selectedId) && (!shown.length || framesNames(saved, shown, size, insets))) { place(saved, 0); following.current = null; }
    else if (frame()) following.current = "frame";
    else return false;
    opened.current = true;
    openTries.current = 0;
    const settled = shown.filter(placedNode);
    if (!reduced && !compact && replayAt === undefined && settled.length > 1) {
      const cx = settled.reduce((sum, node) => sum + node.x, 0) / settled.length, cy = settled.reduce((sum, node) => sum + node.y, 0) / settled.length;
      settle.current = { start: performance.now(), from: new Map(settled.map(node => [node.id, { dx: (cx - node.x) * .6, dy: (cy - node.y) * .6 }])) };
      setSettleRun(run => run + 1);
    }
    return true;
  };
  const openLatest = useRef(open);
  openLatest.current = open;
  useEffect(() => {
    if (!drawn) { opened.current = false; openTries.current = 0; following.current = null; return undefined; }
    // A scope change on a drawn canvas keeps its view: the Map fits a new focal name or scope itself.
    // The canvas element takes its measured size as the renderer lays it out; until then, try each frame.
    let retry = 0;
    const attempt = () => { if (!openLatest.current()) retry = requestAnimationFrame(attempt); };
    attempt();
    return () => cancelAnimationFrame(retry);
  }, [drawn]);
  useEffect(() => {
    controls.current = {
      canvas: () => host.current?.querySelector("canvas") ?? null,
      exportLabels: setExportLabels,
      fit,
      focus,
    };
    return () => { controls.current = null; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `fit` and `focus` close over the listed values.
  }, [controls, data, size, insets, visibleIds, replayAt, reduced]);
  const selectionDrawn = !!selectedId && data.nodes.some(node => node.id === selectedId);
  const priorViewport = useRef({ width: 0, height: 0, left: 0, right: 0, top: 0, bottom: 0, card: "", selectedId: undefined as string | undefined, drawn: false });
  useEffect(() => {
    if (!size.width || !size.height) return;
    const prior = priorViewport.current;
    const card = insets.card ? `${insets.card.l}:${insets.card.t}:${insets.card.r}:${insets.card.b}` : "";
    const covered = insets.left || insets.right || insets.top || insets.bottom || insets.card;
    const captionToast = host.current?.closest(".map-view")?.querySelector("[data-map-caption] .learning-toast");
    const moved = prior.width && (prior.width !== size.width || (!!captionToast && prior.height !== size.height) || prior.left !== insets.left || prior.right !== insets.right || prior.top !== insets.top || prior.bottom !== insets.bottom || prior.card !== card);
    const reader = host.current?.closest(".map-body")?.querySelector(".map-reader");
    const readerBox = reader?.getBoundingClientRect();
    const bounds = host.current?.getBoundingClientRect();
    const readerSheet = reader instanceof HTMLElement && getComputedStyle(reader).position === "absolute" && !!readerBox && !!bounds && readerBox.height > 0 && readerBox.height < bounds.height - 1;
    const resized = prior.width !== size.width || prior.height !== size.height || prior.left !== insets.left || prior.right !== insets.right || prior.top !== insets.top || prior.bottom !== insets.bottom || prior.card !== card;
    // An automatic framing nobody has moved is framed again for the size the page settles at (any change
    // of canvas size or free area), at once while the opening settle runs. The first measurement is the
    // one the opening view was just framed against.
    if (following.current && !readerSheet && (!prior.width || (prior.selectedId === selectedId && prior.drawn === selectionDrawn))) {
      const ms = settle.current ? 0 : duration();
      if (prior.width && resized) { if (following.current === "fit") fit(ms); else frame(ms); }
    }
    // A selection that gets its first position after the Map opened is framed as opening on it would be.
    else if (selectionDrawn && prior.width && prior.selectedId === selectedId && !prior.drawn) frame();
    else if (selectionDrawn) focus(selectedId!);
    else if (readerSheet) { /* A reader sheet with no related node keeps the current zoom and centre. */ }
    else if (!compact && ((!prior.width && covered) || moved)) fit();
    else if (compact && ((prior.top !== insets.top && insets.top) || (!prior.width && (insets.bottom || card)) || (prior.width && (prior.bottom !== insets.bottom || prior.card !== card)))) fit();
    priorViewport.current = { ...size, left: insets.left, right: insets.right, top: insets.top, bottom: insets.bottom, card, selectedId, drawn: selectionDrawn };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- re-places the view on selection, size and inset changes; `fit` and `focus` read the current data.
  }, [selectedId, selectionDrawn, size.width, size.height, insets]);
  // Between transitions the renderer draws only when asked. A timer can end a transition (settle, scope or
  // focus fade, pulse, glow, replay) before any frame showed its final state, so ending one draws that frame.
  const pausable = !probeEnabled && !pulsing && !compactGlowing && !liveFading && !replayAnimating && !scopeFading && !settling && !focusFading;
  useEffect(() => {
    if (pausable) repaint();
  }, [pausable]);
  const activeLink = (link: Link) => endpoint(link.source) === (hovered?.id ?? selectedId) || endpoint(link.target) === (hovered?.id ?? selectedId);
  useEffect(() => {
    if (!drawProbe || !size.width || !nodes.length) return;
    let cancelled = false;
    let attempts = 0;
    const run = () => {
      if (cancelled) return;
      const canvas = host.current?.querySelector("canvas");
      if (!(canvas instanceof HTMLCanvasElement)) {
        // The graph appends its canvas after mount. A hidden WebView throttles
        // requestAnimationFrame, so wait on a timer instead.
        if (attempts < 40) {
          attempts += 1;
          window.setTimeout(run, 50);
        }
        return;
      }
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      const scale = 1;
      const samples: number[] = [];
      // Same work as one steady overview frame: every link, every node, then
      // labels for the hub set the renderer keeps when zoomed out.
      const labelNodes = data.nodes.filter((node) => hubs.has(node.id) && showMetricLabel(node));
      const labelFrame = createLabelFrame();
      for (let sample = 0; sample < 200; sample += 1) {
        const started = performance.now();
        ctx.save();
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.clearRect(0, 0, canvas.width, canvas.height);
        ctx.fillStyle = palette.colors.bg;
        ctx.fillRect(0, 0, canvas.width, canvas.height);
        for (const link of data.links) {
          if (typeof link.source === "string" || typeof link.target === "string") continue;
          ctx.beginPath();
          ctx.strokeStyle = link.status === "disputed" ? palette.colors["edge-disputed"] : palette.colors.edge;
          ctx.lineWidth = link.status === "disputed" ? 1.5 : 1;
          ctx.setLineDash(link.status === "disputed" ? [4, 3] : []);
          ctx.moveTo(link.source.x, link.source.y);
          ctx.lineTo(link.target.x, link.target.y);
          ctx.stroke();
        }
        for (const node of data.nodes) {
          const radius = nodeRadius(node);
          const color = palette.colors[nodeColorKey(node)];
          if (palette.dark && topHubs.has(node.id)) glow(ctx, channels.get(color), node.x, node.y, radius, 10, .35);
          ctx.beginPath();
          ctx.arc(node.x, node.y, radius, 0, Math.PI * 2);
          ctx.fillStyle = color;
          ctx.fill();
          ctx.lineWidth = 1.5;
          ctx.strokeStyle = palette.colors.bg;
          ctx.setLineDash([]);
          ctx.stroke();
          for (const ring of nodeRings(node, scale, { disputedIds })) {
            ctx.beginPath();
            ctx.arc(node.x, node.y, ring.radius, 0, Math.PI * 2);
            ctx.strokeStyle = palette.colors[ring.color];
            ctx.lineWidth = ring.width;
            ctx.setLineDash(ring.dash);
            ctx.stroke();
          }
        }
        ctx.font = `12px ${palette.font}`;
        const placed = labelFrame(data.nodes, scale, { x: 0, y: 0 }, () => placeLabels(labelNodes, data.nodes, (text) => ctx.measureText(text).width, scale, { hubs, disputedIds }, 200, () => true, 180, [], i18n.language));
        for (const { box, text } of placed) {
          ctx.strokeStyle = palette.colors["label-halo"];
          ctx.lineWidth = 5;
          ctx.strokeText(text, box.x + box.w / 2, box.y);
          ctx.fillStyle = palette.colors.label;
          ctx.fillText(text, box.x + box.w / 2, box.y);
        }
        ctx.restore();
        samples.push(performance.now() - started);
      }
      samples.sort((left, right) => left - right);
      const summary = `${nodes.length} nodes · draw median ${samples[Math.floor(samples.length * .5)].toFixed(2)} ms · draw p95 ${samples[Math.floor(samples.length * .95)].toFixed(2)} ms`;
      setProbe(summary);
      console.info("[map-probe]", summary);
    };
    run();
    return () => { cancelled = true; };
  }, [drawProbe, size.width, size.height, nodes.length, data, palette, channels, hubs, topHubs, disputedIds, i18n.language]);
  const labelFrame = createLabelFrame();
  // A pan or zoom of the person's own makes the view theirs: it is no longer framed again on resize.
  const takeCamera = () => { following.current = null; };
  return <div ref={host} className="map-canvas" data-testid="map-canvas" role="img" aria-label={stats} onPointerDown={takeCamera} onWheel={takeCamera}>
    {drawn ? <ForceGraph<PositionedNode, Link> ref={api} width={size.width} height={size.height} graphData={data}
      nodeVisibility={node => visibleIds.has(node.id) || ghostIds.has(node.id)} linkVisibility={link => edgeVisible(link) || ghostEdgeIds.has(link.id)}
      autoPauseRedraw={pausable} backgroundColor={palette.colors.bg} minZoom={.3} maxZoom={4} cooldownTicks={0} warmupTicks={0} enableNodeDrag={false} nodeLabel={() => ""}
      onRenderFramePre={() => {
        // One clock read per frame for every name: how far names have settled and how far a focus change has eased.
        const now = performance.now(), run = settle.current;
        const time = run ? Math.min(1, (now - run.start) / SETTLE_MS) : 1;
        settleFrame.current = { factor: run && time < 1 ? 1 - springProgress(time, palette.spring) : 0, time };
        focusProgress.current = reduced ? 1 : Math.min(1, (now - focusFade.current.start) / FOCUS_FADE_MS);
      }}
      nodeCanvasObjectMode={() => "replace"} nodeCanvasObject={(node, ctx, scale) => {
        const r = drawnRadius(node, scale); const color = palette.colors[nodeColorKey(node)];
        const dimAlpha = compact ? compactDimAlpha.get(color)! : palette.dim;
        const { from, to } = focusFade.current, lit = !to.ids || to.ids.has(node.id);
        const focusAlpha = (!from.ids || from.ids.has(node.id) ? 1 : dimAlpha) + ((lit ? 1 : dimAlpha) - (!from.ids || from.ids.has(node.id) ? 1 : dimAlpha)) * focusProgress.current;
        ctx.save(); ctx.globalAlpha = focusAlpha * scopeAlpha(node.id);
        const offset = offsetOf(node.id);
        if (offset) ctx.translate(offset.x, offset.y);
        // A soft halo: the pointed name, and in the dark theme the best-connected names and a lit neighbourhood.
        const pointed = lit && node.id === hovered?.id;
        if (pointed || (palette.dark && lit && (to.ids ? !to.both : topHubs.has(node.id)))) glow(ctx, channels.get(color), node.x, node.y, r, (palette.dark ? 10 : 7) / scale, palette.dark ? (pointed ? .5 : .35) : .3);
        if (pointed && !reduced) { ctx.translate(node.x, node.y); ctx.scale(1.12, 1.12); ctx.translate(-node.x, -node.y); }
        if (pulsing && node.id === overlayId) { const progress = Math.min(1, (performance.now() - pulseStart.current) / palette.pulseMs); const scale = progress < .5 ? 1 + .25 * springProgress(progress * 2, palette.spring) : 1.25 - .25 * springProgress((progress - .5) * 2, palette.spring); ctx.translate(node.x, node.y); ctx.scale(scale, scale); ctx.translate(-node.x, -node.y); }
        const live = liveIds?.has(node.id) ? LIVE_GLOW : liveFading && liveFade.current.ids.has(node.id) ? LIVE_GLOW * Math.max(0, 1 - (performance.now() - liveFade.current.start) / LIVE_FADE_MS) : 0;
        if (live) { ctx.save(); ctx.globalAlpha *= live; ctx.fillStyle = palette.colors["new-glow"]; ctx.beginPath(); ctx.arc(node.x, node.y, r + LIVE_GLOW_REACH, 0, Math.PI * 2); ctx.fill(); ctx.restore(); }
        if (compactGlowing && (learningGlowIds ?? compact?.glowNodeIds)?.has(node.id)) {
          const elapsed = Math.max(0, performance.now() - compactGlowStart.current - Math.min(800, (compactOrder.get(node.id) ?? 0) * palette.staggerMs));
          const progress = Math.min(1, elapsed / palette.pulseMs);
          const glow = elapsed < palette.pulseMs ? (progress < .5 ? progress * 2 : 1 - (progress - .5) * 1.3) : .35 * Math.max(0, (4000 - elapsed) / (4000 - palette.pulseMs));
          ctx.save(); ctx.globalAlpha *= glow; ctx.fillStyle = palette.colors["new-glow"]; ctx.beginPath(); ctx.arc(node.x, node.y, r + 10, 0, Math.PI * 2); ctx.fill(); ctx.restore();
          const scale = .6 + .4 * springProgress(progress, palette.spring); ctx.translate(node.x, node.y); ctx.scale(scale, scale); ctx.translate(-node.x, -node.y);
        }
        if (replayAt !== undefined) {
          const elapsed = performance.now() - (appearances.current.get(node.id) ?? performance.now());
          if (elapsed < 0) { ctx.restore(); return; }
          const progress = Math.min(1, elapsed / (reduced ? 120 : palette.pulseMs));
          if (reduced) ctx.globalAlpha *= progress;
          else {
            const glow = elapsed < palette.pulseMs ? (progress < .5 ? progress * 2 : 1 - (progress - .5) * 1.3) : .35 * Math.max(0, 1 - (elapsed - palette.pulseMs) / 2000);
            ctx.save(); ctx.globalAlpha *= glow; ctx.fillStyle = palette.colors["new-glow"]; ctx.beginPath(); ctx.arc(node.x, node.y, r + 10, 0, Math.PI * 2); ctx.fill(); ctx.restore();
            const scale = .6 + .4 * springProgress(progress, palette.spring); ctx.translate(node.x, node.y); ctx.scale(scale, scale); ctx.translate(-node.x, -node.y);
          }
        }
        if (node.id === selectedId) {
          ctx.save();
          ctx.globalAlpha *= palette.selectionAlpha;
          ctx.fillStyle = palette.colors.accent;
          ctx.beginPath();
          ctx.arc(node.x, node.y, r + 8 / scale, 0, Math.PI * 2);
          ctx.fill();
          ctx.restore();
        }
        ctx.beginPath();
        if (node.kind === "lens_method" || node.kind === "lens_principle" || node.kind === "lens_subject") ctx.roundRect(node.x - 7, node.y - 7, 14, 14, 3);
        else if (node.kind === "lens_perspective") { ctx.moveTo(node.x, node.y - 8); ctx.lineTo(node.x + 8, node.y); ctx.lineTo(node.x, node.y + 8); ctx.lineTo(node.x - 8, node.y); ctx.closePath(); }
        else ctx.arc(node.x, node.y, r, 0, Math.PI * 2);
        const hollow = node.kind === "lens_method" || node.kind === "lens_idea";
        ctx.fillStyle = hollow ? palette.colors.bg : color; if (node.kind !== "journal") ctx.fill();
        // A hairline in the canvas colour keeps touching names apart without outlining them.
        ctx.lineWidth = node.kind === "journal" || hollow ? 2 : Math.min(1.5, 1.2 / scale); ctx.strokeStyle = node.kind === "journal" ? palette.colors.journal : hollow ? color : palette.colors.bg; ctx.stroke();
        if (node.kind === "lens_principle") { ctx.beginPath(); ctx.arc(node.x + 6, node.y - 6, 3, 0, Math.PI * 2); ctx.fillStyle = palette.colors["principle-mark"]; ctx.fill(); }
        for (const ring of nodeRings(node, scale, { selectedId, highlightIds: highlightIds ?? compact?.highlightIds, learnedIds, disputedIds })) {
          if (ring.clearance) continue;
          ctx.beginPath();
          if (ring.square) ctx.roundRect(node.x - ring.radius, node.y - ring.radius, 2 * ring.radius, 2 * ring.radius, ring.radius - 4);
          else ctx.arc(node.x, node.y, ring.radius, 0, Math.PI * 2);
          ctx.strokeStyle = palette.colors[ring.color]; ctx.lineWidth = ring.width; ctx.setLineDash(ring.dash); ctx.stroke();
        }
        ctx.setLineDash([]);
        if (disputedIds.has(node.id)) {
          // Conflicting evidence: a filled warning dot on a background rim, distinct from the followed ring.
          const mark = conflictMark(node, scale);
          ctx.beginPath(); ctx.arc(mark.x, mark.y, mark.radius + 1.5 / scale, 0, Math.PI * 2); ctx.fillStyle = palette.colors.bg; ctx.fill();
          ctx.beginPath(); ctx.arc(mark.x, mark.y, mark.radius, 0, Math.PI * 2); ctx.fillStyle = palette.colors.warning; ctx.fill();
        }
        ctx.restore();
      }} nodePointerAreaPaint={(node, color, ctx) => { if (ghostIds.has(node.id)) return; ctx.fillStyle = color; ctx.beginPath(); ctx.arc(node.x, node.y, Math.max(9, nodeRadius(node)), 0, Math.PI * 2); ctx.fill(); }}
      linkLabel={() => ""} onLinkHover={setHoveredEdge}
      linkCanvasObjectMode={() => "replace"} linkCanvasObject={(link, ctx, scale) => {
        if (typeof link.source === "string" || typeof link.target === "string") return;
        const source = link.source, target = link.target;
        const active = activeLink(link);
        const { from, to } = focusFade.current;
        const linkAlpha = (state: Focus) => !state.ids || (state.both ? state.ids.has(source.id) && state.ids.has(target.id) : source.id === state.id || target.id === state.id) ? 1 : palette.dim;
        ctx.save(); ctx.globalAlpha = (linkAlpha(from) + (linkAlpha(to) - linkAlpha(from)) * focusProgress.current) * (link.kind === "applies_to" ? .35 : link.kind === "lens_link" ? .45 : 1);
        if (replayAt !== undefined) ctx.globalAlpha *= Math.max(0, Math.min(1, (performance.now() - (edgeAppearances.current.get(link.id) ?? performance.now())) / (reduced ? 120 : 300)));
        ctx.globalAlpha *= Math.min(scopeAlpha(source.id), scopeAlpha(target.id));
        ctx.strokeStyle = link.status === "disputed" ? palette.colors["edge-disputed"] : link.kind !== "claim" ? palette.colors.method : palette.colors[active ? "edge-active" : "edge"];
        // Widths and dashes are screen pixels, so lines stay thin and quiet at every zoom.
        ctx.lineWidth = (link.status === "disputed" || active ? 1.5 : 1) / scale;
        // The relationship a chip in the inspector points at (map spec §5: 2.5 px).
        if (link.id === emphasisEdgeId) { ctx.globalAlpha = Math.max(ctx.globalAlpha, Math.min(scopeAlpha(source.id), scopeAlpha(target.id))); ctx.lineWidth = 2.5 / scale; if (link.kind === "claim" && link.status !== "disputed") ctx.strokeStyle = palette.colors["edge-active"]; }
        if (compactGlowing && compact?.glowEdgeIds?.has(link.id)) {
          ctx.strokeStyle = palette.colors["edge-active"];
          ctx.shadowColor = palette.colors["new-glow"];
          ctx.shadowBlur = 10 * Math.max(0, 1 - (performance.now() - compactGlowStart.current) / 4000);
          ctx.lineWidth = 2 / scale;
        }
        ctx.setLineDash(link.status === "disputed" ? [4 / scale, 3 / scale] : link.kind === "applies_to" ? [2 / scale, 3 / scale] : []);
        const sourceOffset = offsetOf(source.id), targetOffset = offsetOf(target.id);
        const sx = source.x + (sourceOffset?.x ?? 0), sy = source.y + (sourceOffset?.y ?? 0), tx = target.x + (targetOffset?.x ?? 0), ty = target.y + (targetOffset?.y ?? 0);
        ctx.beginPath(); ctx.moveTo(sx, sy);
        if (link.kind === "applies_to") ctx.quadraticCurveTo((sx + tx) / 2 - (ty - sy) * .2, (sy + ty) / 2 + (tx - sx) * .2, tx, ty);
        else ctx.lineTo(tx, ty);
        ctx.stroke(); ctx.restore();
      }} onRenderFramePost={(ctx, scale) => {
        if (probeEnabled) {
          const now = performance.now();
          if (firstPaint.current.duration === undefined && firstPaint.current.start !== undefined) {
            firstPaint.current.duration = now - firstPaint.current.start;
            host.current?.setAttribute("data-first-paint-ms", firstPaint.current.duration.toFixed(2));
            console.info("[map-probe] first paint", nodes.length, firstPaint.current.duration.toFixed(2), "ms");
          }
          if (lastFrame.current && now - lastFrame.current < 250) frameTimes.current.push(now - lastFrame.current);
          lastFrame.current = now;
          if (frameTimes.current.length >= 180) {
            const times = frameTimes.current.splice(0).sort((a, b) => a - b);
            const summary = `${nodes.length} nodes · median ${times[Math.floor(times.length * .5)].toFixed(2)} ms · p95 ${times[Math.floor(times.length * .95)].toFixed(2)} ms`;
            setProbe(summary); console.info("[map-probe]", summary);
          }
        }
        ctx.save(); ctx.textAlign = "center"; ctx.textBaseline = "top";
        ctx.font = `${12 / scale}px ${palette.font}`;
        const visible = replayAt === undefined ? visibleNodes : visibleNodes.filter(node => performance.now() >= (appearances.current.get(node.id) ?? Infinity));
        const glowIds = learningGlowIds ?? compact?.glowNodeIds;
        const activeHighlights = highlightIds ?? compact?.highlightIds;
        const placedSelection = selectedId && visible.some(node => node.id === selectedId) ? selectedId : undefined;
        const zoomAlpha = new Map<string, number>();
        const candidates = visible.filter(node => {
          if (!showMetricLabel(node, selectedId, hovered?.id)) return false;
          const disputeWithSelection = !!selectedId && disputedIds.has(selectedId) && disputedIds.has(node.id) && selectedNeighbors.has(node.id);
          const emphasized = node.id === selectedId || node.id === hovered?.id || activeHighlights?.has(node.id) || learnedIds?.has(node.id) || glowIds?.has(node.id) || liveIds?.has(node.id) || disputeWithSelection;
          const pinned = !selectedId && (node.watched || learnedLinkedIds.has(node.id));
          // The focal name, followed names and the top hubs keep their names when zoomed out.
          const anchored = node.id === focalId || node.watched || topHubs.has(node.id);
          if (!exportLabels && scale < .6 && !emphasized && !pinned && !anchored) return false;
          // A small mini map names every node; a larger one names only what this chat touched.
          if (compact && neighbors && !neighbors.has(node.id) && !emphasized && visible.length > 8) return false;
          // With a drawn selection, dimmed nodes stay unnamed so the labels describe only what is in focus.
          if (placedSelection && !emphasized && !selectedNeighbors.has(node.id) && !linkedIds.has(node.id)) return false;
          // A small drawing names every node: the mini map up to 8, a focal view or young map up to 30.
          const fewNodes = visible.length <= (compact ? 8 : LOCAL_VIEW_MIN_NODES);
          const prominent = exportLabels || emphasized || pinned || anchored || fewNodes || selectedNeighbors.has(node.id) || linkedIds.has(node.id);
          // The best-connected names fade in first as the Map zooms in, every other name from 1.2× (full at 1.6×).
          const alpha = prominent ? 1 : Math.max(hubs.has(node.id) ? ramp(scale, .6, .85) : 0, ramp(scale, 1.2, 1.6));
          if (alpha < .02) return false;
          const screen = api.current?.graph2ScreenCoords(node.x, node.y);
          if (!screen || screen.x < 0 || screen.x > size.width || screen.y < 0 || screen.y > size.height) return false;
          zoomAlpha.set(node.id, alpha);
          return true;
        });
        const boxClear = (corner: { x: number; y: number }, w: number, h: number) => {
          if (corner.x < insets.left || corner.x + w * scale > size.width - insets.right || corner.y < insets.top || corner.y + h * scale > size.height - insets.bottom) return false;
          const right = corner.x + w * scale, bottom = corner.y + h * scale;
          if (insets.obstacles.some(box => right > box.l && corner.x < box.r && bottom > box.t && corner.y < box.b)) return false;
          const card = insets.card;
          if (!card) return true;
          return right <= card.l || corner.x >= card.r || bottom <= card.t || corner.y >= card.b;
        };
        const onScreen = (box: { x: number; y: number; w: number; h: number }) => {
          const corner = api.current?.graph2ScreenCoords(box.x, box.y);
          return !!corner && boxClear(corner, box.w, box.h);
        };
        const labelCapPx = size.width > 0 && size.width < 900 ? 140 : 180;
        // Names stay clear of a glow until it has faded out, so they do not shift while it shows.
        const glowingLive = liveFading ? new Set([...(liveIds ?? []), ...liveFade.current.ids]) : liveIds;
        const placed = labelFrame(visible, scale, api.current?.graph2ScreenCoords(0, 0) ?? { x: 0, y: 0 }, () => placeLabels(candidates, visible, text => ctx.measureText(text).width, scale, { selectedId: placedSelection, hoveredId: hovered?.id, neighbors: selectedNeighbors, linkedIds, learnedLinkedIds, hubs, highlightIds: activeHighlights, learnedIds, disputedIds, focalId, topHubIds: topHubs, liveIds: glowingLive }, exportLabels ? Infinity : compact ? 10 : 200, onScreen, labelCapPx, data.links, i18n.language));
        const dimSet = selectedId ? selectedNeighbors : neighbors;
        for (const { node, box, text, callout } of placed) {
          const corner = api.current?.graph2ScreenCoords(box.x, box.y);
          if (!corner || !boxClear(corner, box.w, box.h)) continue;
          const arriving = settleFrame.current.factor && settle.current?.from.has(node.id);
          ctx.globalAlpha = scopeAlpha(node.id) * (zoomAlpha.get(node.id) ?? 1) * (arriving ? ramp(settleFrame.current.time, .65, 1) : 1);
          if (!ctx.globalAlpha) continue;
          if (callout) {
            const edgeX = Math.max(box.x, Math.min(node.x, box.x + box.w));
            const edgeY = Math.max(box.y, Math.min(node.y, box.y + box.h));
            ctx.strokeStyle = palette.colors["edge-active"];
            ctx.lineWidth = 1.5 / scale;
            ctx.beginPath(); ctx.moveTo(node.x, node.y); ctx.lineTo(edgeX, edgeY); ctx.stroke();
          }
          const textX = box.x + box.w / 2;
          // A 5 px halo in the canvas color hides edges that pass under a name.
          ctx.strokeStyle = palette.colors["label-halo"]; ctx.lineWidth = 5 / scale; ctx.lineJoin = "round"; ctx.strokeText(text, textX, box.y);
          ctx.fillStyle = dimSet && !dimSet.has(node.id) ? palette.colors["dim-label"] : palette.colors.label; ctx.fillText(text, textX, box.y);
        }
        ctx.restore();
      }} onNodeHover={setHovered} onNodeClick={(node) => onSelect(node)} onBackgroundClick={onClear}
      onZoom={(transform) => { const center = api.current?.centerAt(); if (center && finiteCamera({ ...center, scale: transform.k })) { camera.current = { ...center, scale: transform.k }; cameras.set(scopeKey, camera.current); while (cameras.size > 12) cameras.delete(cameras.keys().next().value!); } }}
    /> : null}
    {tooltip ? <div className="map-tooltip" role="tooltip"><strong>{entityLabel(tooltip, i18n.language)}</strong><small>{t(`map.kind.${nodeCategory(tooltip)}`)} · {t("map.relationships", { count: tooltip.degree })}{tooltip.firstSeenAtMs ? ` · ${t("map.firstSeen", { date: dateValue(new Date(tooltip.firstSeenAtMs).toISOString()) })}` : ""}</small></div> : null}
    {hoveredEdge?.relation && !tooltip ? <div className="map-tooltip" role="tooltip">{relationName(hoveredEdge.relation, t)}</div> : null}
    {probeEnabled || drawProbe ? <output className="map-probe" data-testid="map-probe">{probe || t("map.measuring")}</output> : null}
  </div>;
}
