import { frozenLayout } from "./layout";
import type { KnowledgeMapNode } from "../../contracts/knowledge";
const node = (id: string): KnowledgeMapNode => ({ id, label: id, kind: "entity", degree: 1, watched: false });
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

it('places a name with values reported about it, which name it in both roles', () => {
  // KRW-BTC as one Farside table left it: six reported flows, each a claim from the name to itself, and one holder.
  const nodes = [node('krw-btc'), node('strategy'), node('other')];
  const edges = [
    ...Array.from({ length: 6 }, (_, index) => ({ id: `flow-${index}`, kind: 'claim' as const, source: 'krw-btc', target: 'krw-btc', relation: 'REPORTS' })),
    { id: 'owns', kind: 'claim' as const, source: 'strategy', target: 'krw-btc', relation: 'OWNS' },
  ];
  const placed = frozenLayout('layout-self-claims', 'revision-1', nodes, edges);
  // A name at NaN drew no dot, and in the dark theme its glow threw and took the whole canvas down.
  expect(placed.every((n) => Number.isFinite(n.x) && Number.isFinite(n.y))).toBe(true);
});

it('keeps names without relationships just outside a large drawing, not in a far ring round it', () => {
  // 320 names joined by 500 relationships, the busiest names gaining the most, and 40 names with none.
  let seed = 7;
  const random = () => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed / 2147483648; };
  const nodes = Array.from({ length: 360 }, (_, index) => node(`n${index}`));
  const ends = [0], pairs: [number, number][] = [];
  for (let index = 1; index < 320; index++) { const target = ends[Math.floor(random() * ends.length)]!; pairs.push([index, target]); ends.push(index, target); }
  while (pairs.length < 500) { const source = Math.floor(random() * 320), target = ends[Math.floor(random() * ends.length)]!; if (source !== target) pairs.push([source, target]); }
  const edges = pairs.map(([source, target], index) => ({ id: `e${index}`, kind: 'claim' as const, source: `n${source}`, target: `n${target}` }));
  const placed = frozenLayout('layout-unlinked', 'revision-1', nodes, edges);
  const distance = (item: (typeof placed)[number]) => Math.hypot(item.x, item.y);
  // Nine in ten linked names lie within this distance of the middle.
  const linked = placed.slice(0, 320).map(distance).sort((a, b) => a - b)[288]!;
  const alone = placed.slice(320).reduce((sum, item) => sum + distance(item), 0) / 40;
  // Unbounded repulsion held them half as far out again (1.5×); now they sit at the drawing's rim.
  expect(alone).toBeLessThan(linked * 1.35);
});
