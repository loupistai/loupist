import type { KnowledgeMapEdge, KnowledgeMapNode } from "../../contracts/knowledge";
import { nodeCategory } from "./kinds";
import type { MapPoint } from "./layout";

/** How far around the focal name the Map draws: one or two hops over relationships, or everything. */
export type MapScope = "1" | "2" | "all";
export const mapScopes: readonly MapScope[] = ["1", "2", "all"];
export const isMapScope = (value: unknown): value is MapScope => value === "1" || value === "2" || value === "all";
/** Up to this many names the whole Map is already a local view, so the focal and scope controls hide. */
export const LOCAL_VIEW_MIN_NODES = 60;
/** A local view never draws more names than this; the 1-hop ring always comes first. */
export const LOCAL_VIEW_CAP = 150;

export type LocalSet = {
  /** Names and the Lens or Journal items attached to them. */
  ids: Set<string>;
  /** Names within the hops, before the cap. */
  total: number;
  /** Names drawn. */
  shown: number;
  capped: boolean;
  /** Claim neighbors of the focal name. */
  neighbors: number;
};

const byDegree = (a: KnowledgeMapNode, b: KnowledgeMapNode) => b.degree - a.degree || a.label.localeCompare(b.label) || a.id.localeCompare(b.id);

function claimAdjacency(edges: readonly KnowledgeMapEdge[]) {
  const adjacent = new Map<string, Set<string>>();
  const link = (from: string, to: string) => {
    if (from === to) return;
    let set = adjacent.get(from);
    if (!set) adjacent.set(from, set = new Set());
    set.add(to);
  };
  for (const edge of edges) if (edge.kind === "claim") { link(edge.source, edge.target); link(edge.target, edge.source); }
  return adjacent;
}

/**
 * The focal name, the names within `hops` relationships of it, and the Lens and
 * Journal items attached to those names. Expansion follows stated relationships
 * only; a Lens never pulls in further names. Over `cap` names the focal and its
 * first ring stay, then second-ring names by degree. `pinned` names (a path the
 * user asked to see) are always included, wherever they sit.
 */
export function localSet(nodes: readonly KnowledgeMapNode[], edges: readonly KnowledgeMapEdge[], focalId: string, hops: 1 | 2, cap = LOCAL_VIEW_CAP, pinned: Iterable<string> = []): LocalSet {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const adjacent = claimAdjacency(edges);
  const ring = (from: Iterable<string>, exclude: Set<string>) => {
    const next = new Set<string>();
    for (const id of from) for (const other of adjacent.get(id) ?? []) if (!exclude.has(other) && byId.get(other)?.kind === "entity") next.add(other);
    return next;
  };
  if (!byId.has(focalId)) return { ids: new Set(), total: 0, shown: 0, capped: false, neighbors: 0 };
  const first = ring([focalId], new Set([focalId]));
  const second = hops === 2 ? ring(first, new Set([focalId, ...first])) : new Set<string>();
  const ordered = [focalId, ...[...first].map((id) => byId.get(id)!).sort(byDegree).map((node) => node.id), ...[...second].map((id) => byId.get(id)!).sort(byDegree).map((node) => node.id)];
  const names = ordered.slice(0, cap);
  const ids = new Set(names);
  for (const id of pinned) if (byId.get(id)?.kind === "entity") ids.add(id);
  for (const edge of edges) {
    if (edge.kind !== "applies_to" && edge.kind !== "about") continue;
    for (const [inside, outside] of [[edge.source, edge.target], [edge.target, edge.source]]) {
      const attached = byId.get(outside);
      if (ids.has(inside) && attached && attached.kind !== "entity") ids.add(outside);
    }
  }
  return { ids, total: ordered.length, shown: names.length, capped: ordered.length > names.length, neighbors: first.size };
}

/**
 * Where the Map centers on open: the remembered name, else the first watched
 * company by degree, then a held company, then the best-connected company.
 */
export function defaultFocal(nodes: readonly KnowledgeMapNode[], rememberedId?: string) {
  const names = nodes.filter((node) => node.kind === "entity" && !node.archived);
  if (rememberedId && names.some((node) => node.id === rememberedId)) return rememberedId;
  const companies = names.filter((node) => nodeCategory(node) === "company");
  return (companies.filter((node) => node.watched).sort(byDegree)[0]
    ?? companies.filter((node) => node.held).sort(byDegree)[0]
    ?? [...companies].sort(byDegree)[0])?.id;
}

/** Watched names first, then held ones; each by degree. */
export function followedNames(nodes: readonly KnowledgeMapNode[]) {
  const names = nodes.filter((node) => node.kind === "entity" && !node.archived);
  return [...names.filter((node) => node.watched).sort(byDegree), ...names.filter((node) => node.held && !node.watched).sort(byDegree)];
}

/** The focal name's relationship neighbors in clockwise screen order, starting at twelve o'clock. */
export function neighborOrder(focalId: string, positioned: readonly (KnowledgeMapNode & MapPoint)[], edges: readonly KnowledgeMapEdge[]) {
  const focal = positioned.find((node) => node.id === focalId);
  if (!focal) return [];
  const around = claimAdjacency(edges).get(focalId) ?? new Set<string>();
  const angle = (node: MapPoint) => (Math.atan2(node.x - focal.x, -(node.y - focal.y)) + 2 * Math.PI) % (2 * Math.PI);
  return positioned.filter((node) => around.has(node.id)).sort((a, b) => angle(a) - angle(b) || a.id.localeCompare(b.id)).map((node) => node.id);
}
