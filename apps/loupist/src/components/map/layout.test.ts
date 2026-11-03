import { frozenLayout } from "./layout";
import type { KnowledgeMapNode } from "../../contracts/knowledge";
const node = (id: string): KnowledgeMapNode => ({ id, label: id, kind: "entity", degree: 1, watched: false, held: false });
it('freezes warm-up positions and preserves existing nodes across refresh and reopening', () => {
  const nodes = [node('a'), node('b')];
  const edges = [{ id: 'edge', kind: 'claim' as const, source: 'a', target: 'b' }];
  const first = frozenLayout('layout-loupist', 'revision-1', nodes, edges);
  expect(first.every((n) => Number.isFinite(n.x) && n.fx === n.x && n.fy === n.y)).toBe(true);
  expect(frozenLayout('layout-loupist', 'revision-1', nodes, edges)).toEqual(first);
  const updated = frozenLayout('layout-loupist', 'revision-2', [...nodes, node('c')], [...edges, { ...edges[0], id: 'new', target: 'c' }]);
  expect(updated.slice(0, 2)).toEqual(first);
  expect(Math.hypot(updated[2].x - first[0].x, updated[2].y - first[0].y)).toBeLessThan(150);
  expect(frozenLayout('layout-loupist', 'revision-1', nodes, edges)).toEqual(first);
});
