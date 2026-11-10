import { forceCenter, forceCollide, forceLink, forceManyBody, forceSimulation, forceX, forceY } from "d3-force";
import type { KnowledgeMapEdge, KnowledgeMapNode } from "../../contracts/knowledge";
import { nodeRadius } from "../../knowledge/mapKinds";
export type MapPoint = { x: number; y: number };
export type PositionedNode = KnowledgeMapNode & MapPoint & { fx: number; fy: number };
// Coordinates only, never canonical data. The bounded cache survives route changes.
const layouts = new Map<string, Map<string, MapPoint>>();
const latest = new Map<string, Map<string, MapPoint>>();
const hash = (id: string) => [...id].reduce((n, c) => Math.imul(n ^ c.charCodeAt(0), 16777619) >>> 0, 2166136261);
export function frozenLayout(loupistId: string, revision: string | null, nodes: KnowledgeMapNode[], edges: KnowledgeMapEdge[]): PositionedNode[] {
  const key = JSON.stringify([loupistId, revision]);
  const cached = layouts.get(key);
  const previous = cached ?? latest.get(loupistId);
  const ids = new Set(nodes.map((n) => n.id));
  const links = edges.filter((e) => ids.has(e.source) && ids.has(e.target)).map((e) => ({ source: e.source, target: e.target }));
  const seeds = nodes.map((node, index) => {
    const saved = previous?.get(node.id);
    if (saved) return { ...node, ...saved, fx: saved.x, fy: saved.y };
    const neighbors = edges.filter((e) => e.source === node.id || e.target === node.id).map((e) => previous?.get(e.source === node.id ? e.target : e.source)).filter((p): p is MapPoint => Boolean(p));
    const angle = hash(node.id) / 4294967296 * Math.PI * 2;
    const distance = neighbors.length ? 12 : 12 * Math.sqrt(index + 1);
    return { ...node, x: (neighbors.reduce((n, p) => n + p.x, 0) / (neighbors.length || 1)) + Math.cos(angle) * distance, y: (neighbors.reduce((n, p) => n + p.y, 0) / (neighbors.length || 1)) + Math.sin(angle) * distance, fx: undefined as number | undefined, fy: undefined as number | undefined };
  });
  if (seeds.some((n) => n.fx === undefined)) {
    const simulation = forceSimulation(seeds).stop().randomSource(() => .5)
      .force("charge", forceManyBody().strength(-60))
      .force("link", forceLink(links).id((n) => (n as KnowledgeMapNode).id).distance(40))
      .force("collide", forceCollide<(typeof seeds)[number]>((n) => nodeRadius(n) + 4))
      .force("center", previous ? null : forceCenter(0, 0))
      // Weak gravity keeps nodes without links (for example a new principle) at the cluster's edge
      // instead of letting charge push them far away, which shrinks Fit and exports.
      .force("gravityX", forceX(0).strength(0.06))
      .force("gravityY", forceY(0).strength(0.06));
    for (let tick = 0; tick < (previous ? 60 : 300) && simulation.alpha() > simulation.alphaMin(); tick++) simulation.tick();
    simulation.stop();
  }
  const positions = new Map(previous);
  const result = seeds.map((n, i) => { positions.set(n.id, { x: n.x, y: n.y }); return { ...nodes[i], x: n.x, y: n.y, fx: n.x, fy: n.y }; });
  layouts.delete(key); layouts.set(key, positions); latest.delete(loupistId); latest.set(loupistId, positions);
  while (layouts.size > 12) layouts.delete(layouts.keys().next().value!);
  while (latest.size > 12) latest.delete(latest.keys().next().value!);
  return result;
}
