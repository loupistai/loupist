import type { KnowledgeMapNode } from "../../contracts/knowledge";
import type { PositionedNode } from "./layout";
import { isMetricNode, nodeRadius } from "../../knowledge/mapKinds";
import { entityLabel } from "../../knowledge/mapItems";
export type LabelBox = { x: number; y: number; w: number; h: number };
const LABEL_MAX = 36;
/** Non-lens labels stay short. Lens titles use a pixel cap instead. */
function mapLabelText(label: string) {
  const chars = [...label];
  return chars.length > LABEL_MAX ? `${chars.slice(0, LABEL_MAX - 1).join("").trimEnd()}…` : label;
}
/** Truncate to a measured width. The caller keeps the full string for the hover title. */
function fitLabel(label: string, maxWidth: number, measure: (text: string) => number) {
  if (measure(label) <= maxWidth) return label;
  const chars = [...label];
  let low = 0;
  let high = chars.length;
  while (low < high) {
    const mid = Math.ceil((low + high) / 2);
    const candidate = `${chars.slice(0, mid).join("").trimEnd()}…`;
    if (measure(candidate) <= maxWidth) low = mid;
    else high = mid - 1;
  }
  const cut = chars.slice(0, low).join("").trimEnd();
  return cut ? `${cut}…` : "…";
}
export type PlacedLabel = { node: PositionedNode; box: LabelBox; text: string; callout?: true };
export type LabelPriority = {
  selectedId?: string; hoveredId?: string; neighbors?: ReadonlySet<string>; linkedIds?: ReadonlySet<string>; learnedLinkedIds?: ReadonlySet<string>;
  hubs?: ReadonlySet<string>; highlightIds?: ReadonlySet<string>; learnedIds?: ReadonlySet<string>; disputedIds?: ReadonlySet<string>;
  /** The name a focal view is centred on. */
  focalId?: string;
  /** The best-connected names (top 10 by degree): named before the rest. */
  topHubIds?: ReadonlySet<string>;
  /** Names a running step reads: the canvas draws a glow halo around each (`LIVE_GLOW_REACH`). */
  liveIds?: ReadonlySet<string>;
};
/** How far a live node's glow halo reaches past its mark, in graph units. */
export const LIVE_GLOW_REACH = 10;
export type NodeRing = { radius: number; width: number; color: string; dash: number[]; square?: boolean; clearance?: boolean };
/** Shared geometry keeps the painted rings and label exclusion area identical.
 *  Clearance entries (the selection, a live glow) only reserve space. The canvas paints a halo and does not stroke them. */
export function nodeRings(node: PositionedNode, scale: number, state: LabelPriority): NodeRing[] {
  const r = nodeRadius(node), rings: NodeRing[] = [];
  // Conflicting evidence is a dot at the upper right (`conflictMark`), never a ring: dashed rings mean "you watch".
  // Above the usual 1.5× fit, the ring keeps its on-screen weight instead of thickening with the zoom.
  const thin = Math.min(1, 1.5 / Math.max(scale, 0.001));
  if (node.watched || node.held || state.highlightIds?.has(node.id)) rings.push({ radius: r + 3.5, width: (node.held ? 2.5 : 1.5) * thin, color: "holding-ring", dash: node.watched && !node.held ? [3 * thin, 2 * thin] : [] });
  if (state.learnedIds?.has(node.id)) rings.push({ radius: 8 + 4 / scale, width: 2 / scale, color: "accent", dash: [], square: true });
  if (node.id === state.selectedId) {
    const radius = Math.max(r, ...rings.map(ring => ring.radius + ring.width / 2)) + 3 / scale;
    rings.push({ radius, width: 2 / scale, color: "journal", dash: [], clearance: true }, { radius: radius + 4 / scale, width: 2 / scale, color: "focus", dash: [], clearance: true });
  }
  // A name sits outside the glow, never across it.
  if (state.liveIds?.has(node.id)) rings.push({ radius: r + LIVE_GLOW_REACH, width: 0, color: "new-glow", dash: [], clearance: true });
  return rings;
}
/** The conflicting-evidence dot: centre and radius in graph units, kept readable when zoomed out. */
export function conflictMark(node: PositionedNode, scale: number) {
  const r = nodeRadius(node);
  const radius = Math.min(r * .8, Math.max(r * .42, 2.5 / Math.max(scale, 0.001)));
  return { x: node.x + r * .72, y: node.y - r * .72, radius };
}
function labelOffset(node: PositionedNode, scale: number, state: LabelPriority) {
  return Math.max(nodeRadius(node) + 1 / scale, ...nodeRings(node, scale, state).map(ring => ring.radius + ring.width)) + 3 / scale;
}
const lensKind = (node: PositionedNode) => node.kind.startsWith("lens_");
/** A metric is a label candidate only while the reader has selected or hovered it. */
export function showMetricLabel(node: Pick<KnowledgeMapNode, "id" | "kind" | "entityKind" | "categoryOverride" | "kindLabels">, selectedId?: string, hoveredId?: string) {
  return !isMetricNode(node) || node.id === selectedId || node.id === hoveredId;
}
/** With a selection: that node, its neighbours, the focal name, held or watched, a linked Lens, then degree.
 *  With none: the focal name, held or watched, the newly learned item's linked nodes, the top hubs, then degree. */
function labelPriority(node: PositionedNode, priority: LabelPriority) {
  if (node.id === priority.selectedId) return 6;
  // The other side of a dispute with the selection is part of what the user is reading.
  if (priority.selectedId && priority.neighbors?.has(node.id) && priority.disputedIds?.has(node.id) && priority.disputedIds.has(priority.selectedId)) return 6;
  if (priority.selectedId && priority.neighbors?.has(node.id)) return 4;
  // A focal view is about its centre, so the centre is named first.
  if (node.id === priority.focalId) return priority.selectedId ? 4 : 6;
  if (node.held || node.watched) return priority.selectedId ? 3 : 5;
  if (!priority.selectedId && priority.learnedLinkedIds?.has(node.id)) return 4;
  if (priority.topHubIds?.has(node.id)) return 3;
  if (lensKind(node) && priority.linkedIds?.has(node.id)) return 2;
  if (node.id === priority.hoveredId) return 1;
  return 0;
}
const overlaps = (a: LabelBox, b: LabelBox) => a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
/** Below, above, right, then left. Each slot starts outside the node's own rings.
 *  A selection, its neighbours, and — when nothing is selected — held, watched, and
 *  learned-link labels also try shifted and diagonal gaps. */
function labelSlots(node: PositionedNode, scale: number, priority: LabelPriority, textWidth: number): LabelBox[] {
  const w = textWidth + 4 / scale, h = 16 / scale, offset = labelOffset(node, scale, priority), x = node.x - w / 2;
  const below = { x, y: node.y + offset, w, h };
  const above = { x, y: node.y - offset - h, w, h };
  const right = { x: node.x + offset, y: node.y - h / 2, w, h };
  const left = { x: node.x - offset - w, y: node.y - h / 2, w, h };
  const cardinals = [below, above, right, left];
  const rank = labelPriority(node, priority);
  if (rank < 3) return cardinals;
  const shift = Math.max(w * 0.6, offset);
  const near = [
    ...cardinals,
    { ...below, x: x - shift },
    { ...below, x: x + shift },
    { ...above, x: x - shift },
    { ...above, x: x + shift },
    { x: node.x + offset, y: node.y + offset, w, h },
    { x: node.x - offset - w, y: node.y + offset, w, h },
    { x: node.x + offset, y: node.y - offset - h, w, h },
    { x: node.x - offset - w, y: node.y - offset - h, w, h },
  ];
  // Held and watched names are long. A dense cluster leaves no gap beside the ring,
  // so keep stepping outward until the label can sit in open space.
  if (rank < 5) return near;
  const outward: LabelBox[] = [];
  for (const step of [2, 3, 4, 5]) {
    const reach = offset * step;
    for (const [dx, dy] of [[0, 1], [0, -1], [1, 0], [-1, 0], [1, 1], [1, -1], [-1, 1], [-1, -1]]) {
      const len = Math.hypot(dx, dy);
      const cx = node.x + (dx / len) * (reach + w / 2);
      const cy = node.y + (dy / len) * (reach + h / 2);
      outward.push({ x: cx - w / 2, y: cy - h / 2, w, h });
    }
  }
  return [...near, ...outward];
}
/** A metric with no canvas name is not a mark a reader would assign that name to. */
function namedMark(node: PositionedNode, priority: LabelPriority) {
  return !isMetricNode(node) || node.id === priority.selectedId || node.id === priority.hoveredId;
}
type MarkGeometry = { radius: number; outer: number; rings: NodeRing[]; named: boolean; inFocus: boolean };
type GeometryFor = (node: PositionedNode) => MarkGeometry;
function hitsNode(box: LabelBox, other: PositionedNode, scale: number, geometryFor: GeometryFor) {
  const geometry = geometryFor(other);
  if (!geometry.named) return false;
  if (geometry.rings.some(ring => {
    const radius = ring.radius + ring.width / 2 + 1.5 / scale;
    if (ring.square) return box.x < other.x + radius && box.x + box.w > other.x - radius && box.y < other.y + radius && box.y + box.h > other.y - radius;
    const dx = other.x - Math.max(box.x, Math.min(other.x, box.x + box.w));
    const dy = other.y - Math.max(box.y, Math.min(other.y, box.y + box.h));
    return Math.hypot(dx, dy) < radius;
  })) return true;
  const dx = other.x - Math.max(box.x, Math.min(other.x, box.x + box.w));
  const dy = other.y - Math.max(box.y, Math.min(other.y, box.y + box.h));
  return Math.hypot(dx, dy) < geometry.radius;
}
function labelText(node: PositionedNode, measure: (text: string) => number, scale: number, labelCapPx: number, language: string) {
  const label = lensKind(node) ? node.label : entityLabel(node, language);
  if (!lensKind(node)) return mapLabelText(label);
  return fitLabel(label, labelCapPx / Math.max(scale, 0.001), measure);
}
/** Edge-to-box distance: how far a node's outermost ring sits from any part of the label. */
function gapToBox(box: LabelBox, other: PositionedNode, geometryFor: GeometryFor) {
  const outer = geometryFor(other).outer;
  return Math.hypot(other.x - Math.max(box.x, Math.min(other.x, box.x + box.w)), other.y - Math.max(box.y, Math.min(other.y, box.y + box.h))) - outer;
}
/** A label reads as its owner's only when every comparable node sits clearly farther from it, and not nearer its centre.
 *  Readers attribute a name to the more prominent mark, so a much smaller node only has to be no closer. */
function ownsBox(box: LabelBox, node: PositionedNode, nodes: PositionedNode[], scale: number, geometryFor: GeometryFor) {
  const own = gapToBox(box, node, geometryFor), margin = 5 / scale, radius = geometryFor(node).radius;
  const cx = box.x + box.w / 2, cy = box.y + box.h / 2, ownCentre = Math.hypot(cx - node.x, cy - node.y);
  // With a selection, faded nodes still show, so they only have to be no closer than the owner.
  const inFocus = (other: PositionedNode) => geometryFor(other).inFocus;
  // Names sit below their marks by default, so a name centred just under another mark reads as that mark's.
  // Only the owner's own below slot may sit there.
  const under = (other: PositionedNode) => other.y < box.y && Math.abs(other.x - cx) <= box.w / 2;
  const ownBelow = under(node);
  return !nodes.some(other => {
    if (other.id === node.id || !geometryFor(other).named) return false;
    const gap = gapToBox(box, other, geometryFor), centre = Math.hypot(cx - other.x, cy - other.y);
    if (!ownBelow && inFocus(other) && under(other) && gap < box.h * 2.5) return true;
    if (!inFocus(other) || geometryFor(other).radius < radius * 0.75) return gap < own || centre < ownCentre * 0.8;
    return gap < own + margin || centre < ownCentre;
  });
}
/** Geometry is in graph units. A label that hits a node, a placed label, or `fits` is omitted, and so is one that
 *  attaches nearer another node: a hidden name is better than a name read as a neighbour's.
 *  A metric that is not selected or hovered has no canvas name, so its dot and spokes do not hide a neighbour's.
 *  The selected name stays: if no slot is free it sits on the default slot, and every later label treats that box as taken. */
/** True when segment a–b passes through the box. */
function crossesBox(box: LabelBox, a: { x: number; y: number }, b: { x: number; y: number }) {
  let t0 = 0, t1 = 1;
  const dx = b.x - a.x, dy = b.y - a.y;
  for (const [p, q] of [[-dx, a.x - box.x], [dx, box.x + box.w - a.x], [-dy, a.y - box.y], [dy, box.y + box.h - a.y]]) {
    if (p === 0) { if (q < 0) return false; continue; }
    const t = q / p;
    if (p < 0) { if (t > t1) return false; if (t > t0) t0 = t; } else { if (t < t0) return false; if (t < t1) t1 = t; }
  }
  return true;
}
export type LabelEdge = { source: { id?: string; x?: number; y?: number } | string; target: { id?: string; x?: number; y?: number } | string };
export function placeLabels(candidates: PositionedNode[], nodes: PositionedNode[], measure: (text: string) => number, scale: number, priority: LabelPriority = {}, limit = Infinity, fits: (box: LabelBox) => boolean = () => true, labelCapPx = 180, edges: readonly LabelEdge[] = [], language = "en"): PlacedLabel[] {
  const placed: PlacedLabel[] = [];
  // A placement pass tests many boxes against each mark. Zoom and priority are
  // fixed within the pass, so classify and measure each mark only once.
  const geometry = new Map<PositionedNode, MarkGeometry>();
  const geometryFor: GeometryFor = node => {
    let mark = geometry.get(node);
    if (!mark) {
      const radius = nodeRadius(node), rings = nodeRings(node, scale, priority);
      mark = { radius, rings, outer: Math.max(radius, ...rings.map(ring => ring.radius + ring.width / 2)), named: namedMark(node, priority),
        inFocus: !priority.selectedId || node.id === priority.selectedId || !!priority.neighbors?.has(node.id) || !!priority.linkedIds?.has(node.id) };
      geometry.set(node, mark);
    }
    return mark;
  };
  const ordered = [...candidates].sort((a, b) => labelPriority(b, priority) - labelPriority(a, priority) || b.degree - a.degree || a.id.localeCompare(b.id));
  // A label lying on a short edge to a node in focus sits between the two nodes and reads as either one's.
  // Long or faded edges pass by the label without making it ambiguous.
  const inFocus = (id?: string) => !priority.selectedId || id === priority.selectedId || !!priority.neighbors?.has(id ?? "") || !!priority.linkedIds?.has(id ?? "");
  const byId = new Map(nodes.map(node => [node.id, node]));
  const recededMetric = (id?: string) => {
    if (!id || id === priority.selectedId || id === priority.hoveredId) return false;
    const other = byId.get(id);
    return !!other && isMetricNode(other);
  };
  type NearbyEdge = { a: { x: number; y: number }; b: { x: number; y: number }; otherId?: string; length: number };
  const edgesByNode = new Map<string, NearbyEdge[]>();
  for (const edge of edges) {
    const source = typeof edge.source === "string" ? undefined : edge.source;
    const target = typeof edge.target === "string" ? undefined : edge.target;
    if (!source || !target || source.x === undefined || source.y === undefined || target.x === undefined || target.y === undefined) continue;
    const a = { x: source.x, y: source.y }, b = { x: target.x, y: target.y };
    const length = Math.hypot(b.x - a.x, b.y - a.y);
    if (source.id) {
      const own = edgesByNode.get(source.id) ?? [];
      own.push({ a, b, otherId: target.id, length });
      edgesByNode.set(source.id, own);
    }
    if (target.id && target.id !== source.id) {
      const own = edgesByNode.get(target.id) ?? [];
      own.push({ a, b, otherId: source.id, length });
      edgesByNode.set(target.id, own);
    }
  }
  const ownEdges = (node: PositionedNode) => {
    const reach = 3 * (labelOffset(node, scale, priority) + 16 / scale);
    return (edgesByNode.get(node.id) ?? [])
      .filter(edge => inFocus(edge.otherId) && !recededMetric(edge.otherId) && edge.length <= reach);
  };
  const openSlot = (node: PositionedNode, text: string, rank: number) => {
    const own = ownEdges(node);
    const slots = labelSlots(node, scale, priority, measure(text));
    const free = (candidate: LabelBox) => fits(candidate) && !placed.some(({ box: prior }) => overlaps(candidate, prior)) && !nodes.some(other => hitsNode(candidate, other, scale, geometryFor)) && ownsBox(candidate, node, nodes, scale, geometryFor);
    const clear = slots.find(candidate => !own.some(({ a, b }) => crossesBox(candidate, a, b)) && free(candidate));
    // A hub's spokes leave no slot clear of its own edges. Its name may lie across one,
    // since it still sits nearest that hub; ordinary names stay off their edges.
    if (clear || rank < 3) return clear;
    const crossing = slots.find(free);
    if (crossing) return crossing;
    // Zoomed out, a hub's own small leaves surround it on every side. Its name may sit over
    // them right beside the hub (the halo keeps it legible): readers give a name to the much
    // larger mark. A mark at least two thirds the hub's size still blocks it.
    const prominent = (other: PositionedNode) => other.id !== node.id && geometryFor(other).radius >= geometryFor(node).radius * 2 / 3;
    const besideSmallMarks = slots.slice(0, 4).find(candidate => fits(candidate) && !placed.some(({ box: prior }) => overlaps(candidate, prior)) && !nodes.some(other => prominent(other) && hitsNode(candidate, other, scale, geometryFor)));
    if (besideSmallMarks) return besideSmallMarks;
    // In a dense overview, a top hub may have no dot-free slot. Keep its name
    // only where the label still reads as that hub's, even if it covers a dot.
    if (priority.topHubIds?.has(node.id)) return slots.find(candidate => fits(candidate) && !placed.some(({ box: prior }) => overlaps(candidate, prior)) && ownsBox(candidate, node, nodes, scale, geometryFor));
    return undefined;
  };
  const placeNode = (node: PositionedNode, force: boolean) => {
    const rank = labelPriority(node, priority);
    const full = labelText(node, measure, scale, labelCapPx, language);
    const texts = [full];
    if (rank >= 5) for (const screenPx of [110, 84, 64]) {
      const shorter = fitLabel(full, screenPx / Math.max(scale, 0.001), measure);
      if (shorter !== full && shorter !== "…" && !texts.includes(shorter)) texts.push(shorter);
    }
    let placedText = "";
    let box: LabelBox | undefined;
    let callout = false;
    for (const text of texts) {
      box = openSlot(node, text, rank);
      if (box) { placedText = text; break; }
    }
    if (!box && !priority.selectedId && priority.topHubIds?.has(node.id)) {
      // The All overview can be too dense for a dot-free label. A leader line
      // makes the owning hub explicit when its name has to cover nearby dots.
      box = labelSlots(node, scale, priority, measure(full)).find(candidate => fits(candidate) && !placed.some(({ box: prior }) => overlaps(candidate, prior)));
      if (box) { placedText = full; callout = true; }
    }
    // The halo keeps a colliding selection readable. Later labels see this box as occupied.
    if (!box && force) {
      box = labelSlots(node, scale, priority, measure(full))[0];
      placedText = full;
    }
    if (box) placed.push({ node, box, text: placedText, ...(callout ? { callout: true as const } : {}) });
  };
  const selected = priority.selectedId ? ordered.find(node => node.id === priority.selectedId) : undefined;
  if (selected) placeNode(selected, true);
  for (const node of ordered) {
    if (node.id === priority.selectedId) continue;
    if (!showMetricLabel(node, priority.selectedId, priority.hoveredId)) continue;
    // The selection and its neighbours stay labelled even after the ordinary cap.
    const rank = labelPriority(node, priority);
    if (placed.length >= limit && rank < 4) break;
    placeNode(node, false);
  }
  return placed;
}
