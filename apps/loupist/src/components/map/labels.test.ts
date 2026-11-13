import { nodeRadius } from "../../knowledge/mapKinds";
import { conflictMark, LIVE_GLOW_REACH, nodeRings, placeLabels } from "./labels";
import type { PositionedNode } from "./layout";
const node = (id: string, x = 0, y = 0, extra: Partial<PositionedNode> = {}): PositionedNode => ({ id, label: id, x, y, fx: x, fy: y, kind: "entity", degree: 1, watched: false, held: false, ...extra });
const measure = (text: string) => text.length * 6;
const named = { label: "Hanbit Battery", localNames: ["한빛배터리", "東京"] };
it("draws a Hangul alias for Korean", () => {
  const company = node("hanbit", 0, 0, named);
  expect(placeLabels([company], [company], measure, 1, {}, Infinity, () => true, 180, [], "ko")[0].text).toBe("한빛배터리");
});
it("greedily omits colliding boxes and boxes covering another node", () => {
  const nodes = [node("selected", 0), node("nearby", 5), node("free", 100), node("blocked", 200), node("obstacle", 200, 19)];
  const placed = placeLabels(nodes.slice(0,4), nodes, measure, 1, { selectedId: "selected" });
  expect(placed.map(item => item.node.id)).toEqual(["selected", "blocked", "free"]);
  const box = (id: string) => placed.find(item => item.node.id === id)!.box;
  expect(box("selected").y).toBeGreaterThan(0);
  expect(box("blocked").y).toBeLessThan(0);
  for (const a of placed) for (const b of placed) if (a !== b) expect(a.box.x < b.box.x+b.box.w && a.box.x+a.box.w > b.box.x && a.box.y < b.box.y+b.box.h && a.box.y+a.box.h > b.box.y).toBe(false);
});
it("prioritizes the selection, its neighbours, then held or watched nodes", () => {
  const nodes = [node("degree",0,0,{degree:30}),node("hub",0,0,{degree:2}),node("neighbor"),node("watched",0,0,{watched:true}),node("hovered"),node("selected")];
  const priority = {selectedId:"selected", hoveredId:"hovered", neighbors:new Set(["neighbor"]),hubs:new Set(["hub"])};
  expect(placeLabels(nodes,[],measure,1,priority).map(item => item.node.id)).toEqual(["selected","neighbor","watched","hovered"]);
});
it("with no selection, labels held and watched before a learned link, then degree", () => {
  const held = node("held", 0, 0, { held: true, degree: 1 });
  const watched = node("watched", 220, 0, { watched: true, degree: 1 });
  const linked = node("linked", 440, 0, { degree: 2 });
  const hub = node("hub", 660, 0, { degree: 40 });
  const placed = placeLabels([hub, linked, watched, held], [hub, linked, watched, held], measure, 1, { learnedLinkedIds: new Set(["linked"]) }, 3);
  expect(placed.map(item => item.node.id)).toEqual(["held", "watched", "linked"]);
});
it("never attaches a label nearer another node than its owner", () => {
  const held = node("Bluehaven Logistics", 0, 0, { held: true, degree: 6 });
  const blockers = [];
  for (let angle = 0; angle < 16; angle++) {
    const theta = angle / 16 * Math.PI * 2;
    blockers.push(node(`near-${angle}`, Math.cos(theta) * 28, Math.sin(theta) * 28));
    blockers.push(node(`mid-${angle}`, Math.cos(theta) * 56, Math.sin(theta) * 56));
  }
  const world = [held, ...blockers];
  const gap = (box: { x: number; y: number; w: number; h: number }, other: PositionedNode) => Math.hypot(other.x - Math.max(box.x, Math.min(other.x, box.x + box.w)), other.y - Math.max(box.y, Math.min(other.y, box.y + box.h))) - nodeRadius(other);
  for (const placed of placeLabels(world, world, measure, 0.86, {})) {
    for (const other of world) if (other.id !== placed.node.id) expect(gap(placed.box, other)).toBeGreaterThan(gap(placed.box, placed.node));
  }
});
it("hides a label when every side collides, and still keeps the selection", () => {
  const method = node("Test whether capacity costs are temporary", 0, 0, { kind: "lens_method" });
  const blockers = [];
  for (let x = -120; x <= 120; x += 16) for (let y = -120; y <= 120; y += 16) if (x || y) blockers.push(node(`b${x}_${y}`, x, y));
  const world = [method, ...blockers];
  for (const priority of [{}, { highlightIds: new Set([method.id]) }, { learnedIds: new Set([method.id]) }]) {
    expect(placeLabels([method], world, measure, 1, priority)).toHaveLength(0);
  }
  expect(placeLabels([method], world, measure, 1, { selectedId: method.id })).toHaveLength(1);
});
it("omits metric labels unless that metric is selected or hovered", () => {
  const company = node("Hanbit", 0, 0, { entityKind: "organization", degree: 4 });
  const metric = node("Cobalt Wafer Q3", 240, 0, { entityKind: "Metric", degree: 6 });
  const event = node("Filing", 480, 0, { entityKind: "event", degree: 2 });
  const extension = node("Capacity plan", 720, 0, { entityKind: "CapacityPlan", categoryOverride: "event", kindLabels: { en: "Capacity plan" }, degree: 3 });
  const nodes = [company, metric, event, extension];
  expect(placeLabels(nodes, nodes, measure, 1).map(item => item.node.id)).toEqual(["Hanbit", "Capacity plan", "Filing"]);
  expect(placeLabels([metric], [metric], measure, 1, { selectedId: metric.id })).toHaveLength(1);
  expect(placeLabels([metric], [metric], measure, 1, { hoveredId: metric.id })).toHaveLength(1);
  expect(placeLabels([metric], [metric], measure, 1, { selectedId: "other" })).toHaveLength(0);
});
it("export considers all labels but still omits collisions", () => {
  const nodes = Array.from({length:220},(_,i)=>node(`label-${i}`,i*100));
  expect(placeLabels(nodes,nodes,measure,1)).toHaveLength(220);
  expect(placeLabels(nodes,nodes,measure,1,{},200)).toHaveLength(200);
});
it.each([.6, 2])("keeps ring-aware labels outside every drawn ring at scale %s", scale => {
  const nodes = [node('selected', 0, 0, {held:true}), node('learned', 100, 0, {kind:'lens_method'}), node('watched', 200, 0, {watched:true}), node('conflict', 300), node('obstacle', 0, 35)];
  const state = {selectedId:'selected', learnedIds:new Set(['learned']), disputedIds:new Set(['selected','conflict','obstacle'])};
  const placed = placeLabels(nodes, nodes, text => measure(text) / scale, scale, state);
  expect(placed.length).toBeGreaterThan(0);
  for (const {box} of placed) for (const other of nodes) for (const ring of nodeRings(other, scale, state)) {
    const dx = other.x - Math.max(box.x, Math.min(other.x, box.x + box.w));
    const dy = other.y - Math.max(box.y, Math.min(other.y, box.y + box.h));
    const outer = ring.radius + ring.width / 2;
    expect(ring.square ? Math.max(Math.abs(dx), Math.abs(dy)) : Math.hypot(dx,dy)).toBeGreaterThanOrEqual(outer);
  }
});
it("truncates a long lens label to the pixel cap and keeps the full title", () => {
  const label = "A very long lens title that should not paint across the whole map";
  const lens = node("lens", 0, 0, { kind: "lens_method", label, degree: 1 });
  const width = (text: string) => text.length * 8;
  const wide = placeLabels([lens], [lens], width, 1, {}, Infinity, () => true, 180);
  expect(wide[0].text.endsWith("…")).toBe(true);
  expect(width(wide[0].text)).toBeLessThanOrEqual(180);
  expect(wide[0].node.label).toBe(label);
  const narrow = placeLabels([lens], [lens], width, 1, {}, Infinity, () => true, 140);
  expect(width(narrow[0].text)).toBeLessThanOrEqual(140);
  const zoomed = placeLabels([lens], [lens], width, 2, {}, Infinity, () => true, 180);
  expect(width(zoomed[0].text)).toBeLessThanOrEqual(90);
  const plain = node("plain", 300, 0, { label: "x".repeat(40) });
  expect(placeLabels([plain], [plain], width, 1)[0].text.endsWith("…")).toBe(true);
  expect([...placeLabels([plain], [plain], width, 1)[0].text].length).toBeLessThanOrEqual(36);
});
it("never lays a label across one of its own edges", () => {
  const owner = node("Hanul Precision", 0, 0, { degree: 3 });
  const above = node("Morrow", 0, -40, { degree: 2 });
  const below = node("Capacity", 0, 40, { degree: 1 });
  const placed = placeLabels([owner], [owner, above, below], measure, 1, {}, Infinity, () => true, 180, [{ source: owner, target: above }, { source: owner, target: below }]);
  expect(placed).toHaveLength(1);
  // Above and below lie on the vertical edges, so the name moves to the side.
  expect(placed[0].box.y).toBeLessThan(0);
  expect(placed[0].box.y + placed[0].box.h).toBeGreaterThan(0);
});
it("names a hub whose spokes block every side, while ordinary names stay off their edges", () => {
  const hub = node("Hanul Precision", 0, 0, { degree: 4 });
  const spokes = [node("north", 0, -80), node("south", 0, 80), node("east", 80, 0), node("west", -80, 0)];
  const edges = spokes.map((spoke) => ({ source: hub, target: spoke }));
  const nodes = [hub, ...spokes];
  expect(placeLabels([hub], nodes, measure, 1, {}, Infinity, () => true, 180, edges)).toEqual([]);
  const placed = placeLabels([hub], nodes, measure, 1, { topHubIds: new Set([hub.id]) }, Infinity, () => true, 180, edges);
  expect(placed.map((item) => item.node.id)).toEqual([hub.id]);
});
it("names a hub ringed by its much smaller leaves, but not an ordinary node", () => {
  const hub = node("Hanul Precision", 0, 0, { degree: 16 });
  const leaves = Array.from({ length: 16 }, (_, index) => {
    const theta = index / 16 * Math.PI * 2;
    return node(`leaf-${index}`, Math.cos(theta) * 26, Math.sin(theta) * 26);
  });
  const world = [hub, ...leaves];
  const edges = leaves.map((leaf) => ({ source: hub, target: leaf }));
  expect(placeLabels([hub], world, measure, .5, {}, Infinity, () => true, 180, edges)).toEqual([]);
  expect(placeLabels([hub], world, measure, .5, { topHubIds: new Set([hub.id]) }, Infinity, () => true, 180, edges).map((item) => item.node.id)).toEqual([hub.id]);
});
it("keeps a top hub identifiable with a callout when a comparable dot blocks the only usable slot", () => {
  const hub = node("Research 0001", 0, 0, { degree: 20 });
  const world = [hub, node("same-size-blocker", 0, 40, { degree: 20 })];
  const width = (text: string) => text.length * 20;
  const fits = (box: { x: number; y: number; w: number }) => box.y > 0 && Math.abs(box.x + box.w / 2) < 1;
  expect(placeLabels([hub], world, width, .3, {}, Infinity, fits)).toHaveLength(0);
  const placed = placeLabels([hub], world, width, .3, { topHubIds: new Set([hub.id]) }, Infinity, fits);
  expect(placed).toHaveLength(1);
  expect(placed[0].callout).toBe(true);
});
it("names the focal centre first, then held or watched names", () => {
  const held = node("held", 0, 0, { held: true, degree: 9 });
  const focal = node("focal", 300, 0);
  expect(placeLabels([held, focal], [held, focal], measure, 1).map((item) => item.node.id)).toEqual(["held", "focal"]);
  expect(placeLabels([held, focal], [held, focal], measure, 1, { focalId: "focal" }).map((item) => item.node.id)).toEqual(["focal", "held"]);
});
it("marks conflicting evidence with a dot, not a ring that reads like the watch ring", () => {
  const disputed = node("disputed", 0, 0, { degree: 4 });
  expect(nodeRings(disputed, 1, { disputedIds: new Set(["disputed"]) })).toEqual([]);
  const watched = node("watched", 0, 0, { degree: 4, watched: true });
  expect(nodeRings(watched, 1, { disputedIds: new Set(["watched"]) }).map((ring) => ring.color)).toEqual(["holding-ring"]);
  const mark = conflictMark(disputed, 1), radius = nodeRadius(disputed);
  expect(mark.x).toBeGreaterThan(disputed.x);
  expect(mark.y).toBeLessThan(disputed.y);
  expect(mark.radius).toBeLessThanOrEqual(radius * .8);
  // Zoomed far out it stays visible, but never outgrows the node it marks.
  expect(conflictMark(disputed, .1).radius).toBe(radius * .8);
});
it("keeps names outside a live node's glow halo, its own and a neighbour's", () => {
  const live = node("live", 0, 0, { degree: 4 });
  const neighbour = node("neighbour", 0, 24);
  const nodes = [live, neighbour];
  const gap = (box: { x: number; y: number; w: number; h: number }) =>
    Math.hypot(live.x - Math.max(box.x, Math.min(live.x, box.x + box.w)), live.y - Math.max(box.y, Math.min(live.y, box.y + box.h)));
  const halo = nodeRadius(live) + LIVE_GLOW_REACH;
  // Without the glow the name sits just past the mark, where the halo is painted.
  expect(gap(placeLabels([live], [live], measure, 1)[0]!.box)).toBeLessThan(halo);
  const placed = placeLabels(nodes, nodes, measure, 1, { liveIds: new Set(["live"]) });
  expect(placed.map((item) => item.node.id).sort()).toEqual(["live", "neighbour"]);
  for (const { box } of placed) expect(gap(box)).toBeGreaterThanOrEqual(halo);
});
// The pane edge sits just left of the ring and the legend covers the row to its right.
const paneEdge = (box: { x: number; y: number; w: number; h: number }) => box.x >= -8 && (box.y > 5 || box.y + box.h < -5);
it("names a Journal ring at the pane edge on its inward side", () => {
  const journal = node("Inventory days below 50", 0, 0, { kind: "journal", degree: 0 });
  const company = node("Hanul", 200, 0, { degree: 4 });
  const nodes = [journal, company];
  const [placed] = placeLabels([journal], nodes, measure, 1, {}, Infinity, paneEdge);
  expect(placed.text).toBe(journal.label);
  expect(placed.box.x).toBeGreaterThanOrEqual(-8);
  expect(placed.box.x + placed.box.w / 2).toBeGreaterThan(journal.x);
  // An ordinary name in the same place keeps to its own slots.
  expect(placeLabels([{ ...journal, kind: "entity" }], nodes, measure, 1, {}, Infinity, paneEdge)).toHaveLength(0);
});
it("shortens a Journal ring's name before dropping it", () => {
  const journal = node("Inventory days below fifty for two quarters", 0, 0, { kind: "journal", degree: 0 });
  const fits = (box: { x: number; y: number; w: number; h: number }) => paneEdge(box) && box.x + box.w <= 120;
  const [placed] = placeLabels([journal], [journal], measure, 1, {}, Infinity, fits);
  expect(placed.text.endsWith("…")).toBe(true);
  expect(placed.box.x + placed.box.w).toBeLessThanOrEqual(120);
  expect(placeLabels([{ ...journal, kind: "entity" }], [journal], measure, 1, {}, Infinity, fits)).toHaveLength(0);
});
it("names ordinary nodes before a Journal ring", () => {
  const journal = node("journal", 0, 0, { kind: "journal", degree: 0 });
  const company = node("Hanul", 60, 60, { degree: 1 });
  expect(placeLabels([journal, company], [journal, company], measure, 1, {}, 1).map((item) => item.node.id)).toEqual(["Hanul"]);
});
