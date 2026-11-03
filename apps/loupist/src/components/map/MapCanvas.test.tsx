import { act, render, screen, waitFor } from "@testing-library/react";
import type { ForceGraphProps } from "react-force-graph-2d";
import MapCanvas, { coverAlpha, SCOPE_FADE_MS, type MapCanvasControls, type MapCanvasLink } from "./MapCanvas";
import type { PositionedNode } from "./layout";
const renderer = vi.hoisted(() => ({ props: null as ForceGraphProps<PositionedNode, MapCanvasLink> | null, center: { x: 0, y: 0 }, zoom: 1, fit: vi.fn(), renders: 0 }));
vi.mock("react-force-graph-2d", async () => {
  const { forwardRef, useImperativeHandle } = await import("react");
  return { default: forwardRef((props: ForceGraphProps<PositionedNode, MapCanvasLink>, ref) => {
    renderer.props = props; renderer.renders++;
    useImperativeHandle(ref, () => ({
      centerAt: (x?: number, y?: number) => { if (x !== undefined && y !== undefined) renderer.center = { x, y }; return renderer.center; },
      zoom: (scale?: number) => { if (scale !== undefined) renderer.zoom = scale; return renderer.zoom; },
      zoomToFit: renderer.fit,
      graph2ScreenCoords: (x: number, y: number) => ({ x: x + 450, y: y + 350 }),
    }));
    return <canvas />;
  }) };
});
const node = (id: string, x = 0): PositionedNode => ({ id, label: id, kind: "entity", entityKind: "organization", degree: 4, watched: id === 'a', held: false, x, y: 0, fx: x, fy: 0 });
const base = { edges: [], controls: { current: null }, onSelect: vi.fn(), onClear: vi.fn(), stats: 'map' };
const labelsAt = (scale: number) => {
  const seen: string[] = [];
  const ctx = { save: vi.fn(), restore: vi.fn(), measureText: (text: string) => ({ width: text.length * 6 }), strokeText: vi.fn(), globalAlpha: 1, fillStyle: '', fillText(text: string) { seen.push(text); } };
  renderer.props!.onRenderFramePost?.(ctx as unknown as CanvasRenderingContext2D, scale);
  return seen;
};
const rects = (overlays: Record<string, Partial<DOMRect>>, frame: Partial<DOMRect> = { top: 0, bottom: 700, height: 700, left: 0, right: 900, width: 900 }) =>
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    const match = Object.keys(overlays).find(name => this.classList.contains(name));
    return (match ? overlays[match] : frame) as DOMRect;
  });
const reducedMotion = () => vi.spyOn(window, 'matchMedia').mockReturnValue({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() } as unknown as MediaQueryList);
beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(900);
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(700);
});
afterEach(() => vi.restoreAllMocks());

it('selects a node without changing zoom, fits on demand, and deselects empty space', () => {
  const onSelect = vi.fn(), onClear = vi.fn(); const controls = { current: null as MapCanvasControls | null };
  render(<MapCanvas {...base} nodes={[node('a'), node('b', 40), node('c', 80)]} scopeKey="camera-controls" controls={controls} onSelect={onSelect} onClear={onClear} stats="3 names" />);
  const zoom = renderer.zoom;
  act(() => renderer.props!.onNodeClick?.(node('a'), new MouseEvent('click')));
  expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ id: 'a' }));
  act(() => controls.current?.focus('a'));
  expect(renderer.zoom).toBe(zoom);
  act(() => controls.current?.fit());
  // A handful of names may draw up to 3× so a small map is not a speck in its canvas.
  expect(renderer.zoom).toBe(3);
  expect(renderer.center).toEqual({ x: 40, y: 0 });
  act(() => renderer.props!.onBackgroundClick?.(new MouseEvent('click'))); expect(onClear).toHaveBeenCalledOnce();
  expect(screen.getByRole('img', { name: '3 names' })).toBeVisible();
});

it('restores the camera after route unmount and avoids React renders during gestures', () => {
  const props = { ...base, nodes: [node('a')], scopeKey: 'camera-return' };
  const first = render(<MapCanvas {...props} />);
  const before = renderer.renders;
  act(() => { renderer.center = { x: 80, y: -20 }; renderer.zoom = 1.7; renderer.props!.onZoom?.({ x: 0, y: 0, k: 1.7 }); });
  expect(renderer.renders).toBe(before);
  first.unmount(); renderer.center = { x: 0, y: 0 }; renderer.zoom = 1;
  render(<MapCanvas {...props} />);
  expect(renderer.center).toEqual({ x: 80, y: -20 }); expect(renderer.zoom).toBe(1.7);
});

it('dims nodes and edges outside the Lens application set even while another node is hovered', () => {
  document.documentElement.style.setProperty('--gg-color-map-dim', '.2');
  const nodes = [node('a'), node('b', 100), node('c', 200)];
  const edges = [{ id: 'ab', kind: 'claim' as const, source: 'a', target: 'b' }, { id: 'bc', kind: 'claim' as const, source: 'b', target: 'c' }];
  render(<MapCanvas {...base} nodes={nodes} edges={edges} scopeKey="lens-dim" overlayIds={new Set(['a', 'b'])} />);
  act(() => renderer.props!.onNodeHover?.(nodes[2], null));
  const fills: number[] = [], strokes: number[] = [];
  const ctx = { save: vi.fn(), restore: vi.fn(), beginPath: vi.fn(), arc: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), setLineDash: vi.fn(), globalAlpha: 1, fill() { fills.push(this.globalAlpha); }, stroke() { strokes.push(this.globalAlpha); } };
  nodes.forEach(n => renderer.props!.nodeCanvasObject?.(n, ctx as unknown as CanvasRenderingContext2D, 1));
  expect(fills).toEqual([1, 1, .2]);
  strokes.length = 0;
  renderer.props!.graphData!.links.forEach(e => renderer.props!.linkCanvasObject?.(e, ctx as unknown as CanvasRenderingContext2D, 1));
  expect(strokes).toEqual([1, .2]);
  document.documentElement.style.removeProperty('--gg-color-map-dim');
});

it('replays visibility without changing the frozen graph or camera', () => {
  const nodes = [{ ...node('a'), firstSeenAtMs: 10 }, { ...node('b', 100), firstSeenAtMs: 20 }, node('undated', 200)];
  const edges = [{ id: 'ab', kind: 'claim' as const, source: 'a', target: 'b', firstSeenAtMs: 30 }];
  const props = { ...base, nodes, edges, scopeKey: 'replay-camera' };
  const view = render(<MapCanvas {...props} replayAt={15} />);
  const graph = renderer.props!.graphData;
  const camera = { ...renderer.center }, zoom = renderer.zoom;
  const nodeVisible = () => renderer.props!.nodeVisibility as (n: PositionedNode) => boolean;
  const edgeVisible = () => renderer.props!.linkVisibility as (e: MapCanvasLink) => boolean;
  expect(nodes.filter(nodeVisible()).map(n => n.id)).toEqual(['a', 'undated']);
  expect(edgeVisible()(edges[0])).toBe(false);
  view.rerender(<MapCanvas {...props} replayAt={25} />);
  expect(nodes.filter(nodeVisible()).map(n => n.id)).toEqual(['a', 'b', 'undated']);
  view.rerender(<MapCanvas {...props} replayAt={30} />);
  expect(edgeVisible()(edges[0])).toBe(true);
  expect(renderer.props!.graphData).toBe(graph); expect(renderer.center).toEqual(camera); expect(renderer.zoom).toBe(zoom);
  view.rerender(<MapCanvas {...props} />);
  expect(nodes.every(nodeVisible())).toBe(true);
});

it('names a disputed relation in its tooltip and labels every node only while exporting', () => {
  const controls = { current: null as MapCanvasControls | null };
  // b and c have no relationships, so they are not top hubs and lose their names when zoomed out.
  render(<MapCanvas {...base} nodes={[node('a'), { ...node('b', 100), degree: 0 }, { ...node('c', 200), degree: 0 }]} edges={[{ id: 'ab', kind: 'claim', source: 'a', target: 'b', status: 'disputed', relation: 'SUPPLIES' }]} scopeKey="export-labels" controls={controls} />);
  act(() => renderer.props!.onLinkHover?.(renderer.props!.graphData!.links[0], null));
  expect(screen.getByRole('tooltip')).toHaveTextContent('supplies');
  expect(labelsAt(.5)).toEqual(['a']);
  act(() => controls.current!.exportLabels!(true));
  expect(labelsAt(.5)).toEqual(['a', 'b', 'c']);
  act(() => controls.current!.exportLabels!(false));
  expect(labelsAt(.5)).toEqual(['a']);
});

it('centres the selection above a bottom sheet without zooming', async () => {
  reducedMotion();
  rects({ 'map-inspector': { top: 280, bottom: 700, height: 420, left: 0, right: 900, width: 900 } });
  renderer.zoom = 1.5;
  render(<div className="map-body"><div className="map-stage"><MapCanvas {...base} selectedId="a" nodes={[node('a')]} scopeKey="sheet" /></div><aside className="map-inspector" style={{ position: 'absolute' }} /></div>);
  await waitFor(() => expect(350 + (0 - renderer.center.y) * renderer.zoom).toBeCloseTo(140, 2));
  expect(renderer.zoom).toBe(1.5);
});

it('re-centres the selection into the visible area when the Library opens and closes', async () => {
  reducedMotion();
  rects({ 'map-library': { top: 0, bottom: 700, height: 700, left: 0, right: 360, width: 360 } });
  renderer.zoom = 1;
  const canvas = <div className="map-stage"><MapCanvas {...base} selectedId="a" nodes={[node('a')]} scopeKey="library-visible-selection" /></div>;
  const view = render(<div className="map-body">{canvas}</div>);
  view.rerender(<div className="map-body">{canvas}<aside className="map-library" /></div>);
  await waitFor(() => expect(renderer.center.x).toBe(-180));
  expect(renderer.zoom).toBe(1);
  view.rerender(<div className="map-body">{canvas}</div>);
  await waitFor(() => expect(renderer.center.x).toBe(0));
});

it('keeps zoom and centre when a reader sheet opens without a related node', () => {
  rects({ 'map-reader': { top: 280, bottom: 700, height: 420, left: 0, right: 700, width: 700 } });
  renderer.zoom = 1.4;
  renderer.center = { x: 12, y: -8 };
  render(<div className="map-body"><div className="map-stage"><MapCanvas {...base} nodes={[node('a', -100), node('b', 100)]} scopeKey="reader-sheet" /></div><aside className="map-inspector map-reader" style={{ position: 'absolute' }} /></div>);
  expect(renderer.zoom).toBe(1.4);
  expect(renderer.center).toEqual({ x: 12, y: -8 });
});

it('fits the graph clear of an open legend and an open Library', () => {
  reducedMotion();
  rects({ 'map-legend': { top: 560, bottom: 688, height: 128, left: 12, right: 220, width: 208 }, 'map-library': { top: 0, bottom: 700, height: 700, left: 0, right: 360, width: 360 } });
  const nodes = [node('a', -100), { ...node('b', 100), y: -500, fy: -500 }];
  render(<div className="map-body"><div className="map-stage"><MapCanvas {...base} reserveLegend nodes={nodes} scopeKey="legend-fit" /><details className="map-legend" open><summary>Legend</summary></details></div><aside className="map-library" /></div>);
  for (const item of nodes) {
    expect(350 + (item.y - renderer.center.y) * renderer.zoom).toBeLessThan(560);
    expect(450 + (item.x - renderer.center.x) * renderer.zoom).toBeGreaterThanOrEqual(360);
  }
});

it('labels the selection and its neighbour at the minimum window size, but not under a toast', () => {
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(700);
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(560);
  const props = { ...base, nodes: [node('selected'), node('neighbor', 160)], edges: [{ id: 'link', kind: 'claim' as const, source: 'selected', target: 'neighbor' }], selectedId: 'selected' };
  render(<div className="map-body"><MapCanvas {...props} scopeKey="label-selection" /></div>);
  expect(labelsAt(1)).toEqual(expect.arrayContaining(['selected', 'neighbor']));
  rects({}, { top: 0, bottom: 560, left: 0, right: 700, width: 700, height: 560 });
  render(<div className="map-body"><div className="learning-toast">Learned</div><MapCanvas {...props} scopeKey="label-covered" /></div>);
  expect(labelsAt(1)).toEqual([]);
});

it('labels a newly learned or highlighted Lens below the ordinary zoom threshold', () => {
  const label = 'Separate durable margins from temporary capacity costs';
  const method = { ...node('learned', 0), kind: 'lens_method' as const, label, watched: false, degree: 1 };
  const props = { ...base, nodes: [method], scopeKey: 'learned-label' };
  const view = render(<MapCanvas {...props} learnedIds={new Set(['learned'])} />);
  expect(labelsAt(.5)).toEqual([label]);
  view.rerender(<MapCanvas {...props} highlightIds={new Set(['learned'])} />);
  expect(labelsAt(.5)).toEqual([label]);
  view.rerender(<MapCanvas {...props} />);
  expect(labelsAt(.5)).toEqual([]);
});

it('labels a metric only when it is hovered', async () => {
  const metric = { ...node('margin', 0), entityKind: 'metric', degree: 25, watched: false, label: 'Operating margin' };
  const filing = { ...node('filing', 220), entityKind: 'event', degree: 1, watched: false };
  render(<MapCanvas {...base} nodes={[metric, filing]} edges={[{ id: 'reports', kind: 'claim', source: 'filing', target: 'margin' }]} scopeKey="metric-recede" />);
  expect(labelsAt(2)).toContain('filing');
  expect(labelsAt(2)).not.toContain('Operating margin');
  act(() => renderer.props!.onNodeHover?.(metric, null));
  await waitFor(() => expect(screen.getByRole('tooltip')).toHaveTextContent('Operating margin'));
  expect(labelsAt(2)).toContain('Operating margin');
});

it('keeps a dimmed compact node at least 3:1 against the dark map background and names a small mini map', () => {
  document.documentElement.dataset.theme = 'dark';
  document.documentElement.style.setProperty('--gg-color-map-other', '#908e86');
  document.documentElement.style.setProperty('--gg-color-map-bg', '#131312');
  document.documentElement.style.setProperty('--gg-color-map-dim', '0.22');
  const dimmed = { ...node('margin', 80), entityKind: 'Entity' as const, degree: 1, watched: false };
  const hub = { ...node('hanul'), watched: false };
  render(<MapCanvas {...base} compact={{ highlightIds: new Set(['hanul']) }} nodes={[hub, dimmed]} scopeKey="compact-contrast" />);
  const fills: { color: string; alpha: number }[] = [];
  const stack: number[] = [];
  const ctx = {
    globalAlpha: 1, fillStyle: '', strokeStyle: '', lineWidth: 0,
    save() { stack.push(this.globalAlpha); },
    restore() { const previous = stack.pop(); if (previous !== undefined) this.globalAlpha = previous; },
    beginPath() {}, setLineDash() {}, roundRect() {}, arc() {}, translate() {}, scale() {}, stroke() {},
    fill() { fills.push({ color: String(this.fillStyle), alpha: this.globalAlpha }); },
  };
  renderer.props!.nodeCanvasObject?.(dimmed, ctx as unknown as CanvasRenderingContext2D, 1);
  expect(fills.find(fill => fill.color.toLowerCase() === '#908e86')?.alpha).toBeCloseTo(coverAlpha('#908e86', '#131312'), 2);
  expect(labelsAt(2).sort()).toEqual(['hanul', 'margin']);
  delete document.documentElement.dataset.theme;
  for (const name of ['--gg-color-map-other', '--gg-color-map-bg', '--gg-color-map-dim']) document.documentElement.style.removeProperty(name);
});

it('keeps a steady glow on names a running reply reads, then fades it after the run', () => {
  vi.spyOn(window, 'matchMedia').mockReturnValue({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() } as unknown as MediaQueryList);
  vi.useFakeTimers();
  try {
    document.documentElement.style.setProperty('--gg-color-map-new-glow', '#ffd166');
    const hanul = node('hanul');
    const glowAlpha = () => {
      const fills: { color: string; alpha: number }[] = []; const stack: number[] = [];
      const ctx = { globalAlpha: 1, fillStyle: '', strokeStyle: '', lineWidth: 0, save() { stack.push(this.globalAlpha); }, restore() { const previous = stack.pop(); if (previous !== undefined) this.globalAlpha = previous; }, beginPath() {}, setLineDash() {}, roundRect() {}, arc() {}, translate() {}, scale() {}, stroke() {}, fill() { fills.push({ color: String(this.fillStyle), alpha: this.globalAlpha }); } };
      renderer.props!.nodeCanvasObject?.(hanul, ctx as unknown as CanvasRenderingContext2D, 1);
      return fills.find(fill => fill.color === '#ffd166')?.alpha ?? 0;
    };
    const view = render(<MapCanvas {...base} compact={{ liveNodeIds: new Set(['hanul']) }} nodes={[hanul]} scopeKey="compact-live" />);
    expect(glowAlpha()).toBe(1);
    act(() => { vi.advanceTimersByTime(10_000); });
    expect(glowAlpha()).toBe(1);
    expect(renderer.props!.autoPauseRedraw).toBe(true);
    view.rerender(<MapCanvas {...base} compact={{ liveNodeIds: new Set() }} nodes={[hanul]} scopeKey="compact-live" />);
    expect(renderer.props!.autoPauseRedraw).toBe(false);
    act(() => { vi.advanceTimersByTime(1_700); });
    expect(glowAlpha()).toBeGreaterThan(0);
    expect(glowAlpha()).toBeLessThan(1);
    act(() => { vi.advanceTimersByTime(1_700); });
    expect(glowAlpha()).toBe(0);
    expect(renderer.props!.autoPauseRedraw).toBe(true);
    reducedMotion();
    view.unmount();
    const still = render(<MapCanvas {...base} compact={{ liveNodeIds: new Set(['hanul']) }} nodes={[hanul]} scopeKey="compact-live-reduced" />);
    expect(glowAlpha()).toBe(1);
    still.rerender(<MapCanvas {...base} compact={{ liveNodeIds: new Set() }} nodes={[hanul]} scopeKey="compact-live-reduced" />);
    expect(glowAlpha()).toBe(0);
    expect(renderer.props!.autoPauseRedraw).toBe(true);
  } finally {
    vi.useRealTimers();
    document.documentElement.style.removeProperty('--gg-color-map-new-glow');
  }
});

it('fades names out and in on a scope change, keeping leaving names out of reach until they are gone', () => {
  vi.spyOn(window, 'matchMedia').mockReturnValue({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() } as unknown as MediaQueryList);
  vi.useFakeTimers();
  try {
    const edges = [{ id: 'ab', kind: 'claim' as const, source: 'a', target: 'b' }, { id: 'ac', kind: 'claim' as const, source: 'a', target: 'c' }, { id: 'ad', kind: 'claim' as const, source: 'a', target: 'd' }];
    const view = render(<MapCanvas {...base} nodes={[node('a'), node('b', 40), node('c', 80)]} edges={edges.slice(0, 2)} scopeKey="fade:1hop" />);
    const alphaOf = (id: string) => {
      const target = renderer.props!.graphData!.nodes.find(n => n.id === id)!;
      const fills: number[] = [];
      const ctx = { save: vi.fn(), restore: vi.fn(), beginPath: vi.fn(), arc: vi.fn(), setLineDash: vi.fn(), roundRect: vi.fn(), translate: vi.fn(), scale: vi.fn(), stroke: vi.fn(), globalAlpha: 1, fill() { fills.push(this.globalAlpha); } };
      renderer.props!.nodeCanvasObject?.(target, ctx as unknown as CanvasRenderingContext2D, 1);
      return fills[0] ?? 1;
    };
    expect(alphaOf('c')).toBe(1);
    // Another scope: c leaves, d enters, a and b stay.
    view.rerender(<MapCanvas {...base} nodes={[node('a'), node('b', 40), node('d', 120)]} edges={[edges[0]!, edges[2]!]} scopeKey="fade:other" />);
    expect(renderer.props!.autoPauseRedraw).toBe(false);
    expect(renderer.props!.graphData!.nodes.map(n => n.id)).toEqual(['a', 'b', 'd', 'c']);
    const visible = renderer.props!.nodeVisibility as (n: PositionedNode) => boolean;
    expect(visible(node('c', 80))).toBe(true);
    act(() => { vi.advanceTimersByTime(SCOPE_FADE_MS / 2); });
    expect(alphaOf('a')).toBe(1);
    expect(alphaOf('c')).toBeGreaterThan(0);
    expect(alphaOf('c')).toBeLessThan(1);
    expect(alphaOf('d')).toBeGreaterThan(0);
    expect(alphaOf('d')).toBeLessThan(1);
    // A leaving name takes no pointer.
    const pointer = { fillStyle: '', beginPath: vi.fn(), arc: vi.fn(), fill: vi.fn() };
    renderer.props!.nodePointerAreaPaint?.(renderer.props!.graphData!.nodes.find(n => n.id === 'c')!, '#000001', pointer as unknown as CanvasRenderingContext2D, 1);
    expect(pointer.fill).not.toHaveBeenCalled();
    act(() => { vi.advanceTimersByTime(SCOPE_FADE_MS); });
    expect(renderer.props!.graphData!.nodes.map(n => n.id)).toEqual(['a', 'b', 'd']);
    expect(alphaOf('d')).toBe(1);
    expect(renderer.props!.autoPauseRedraw).toBe(true);
    // Reduced motion: the change is instant.
    view.unmount();
    reducedMotion();
    const still = render(<MapCanvas {...base} nodes={[node('a'), node('b', 40), node('c', 80)]} edges={edges.slice(0, 2)} scopeKey="fade:reduced-1" />);
    still.rerender(<MapCanvas {...base} nodes={[node('a'), node('b', 40)]} edges={[edges[0]!]} scopeKey="fade:reduced-2" />);
    expect(renderer.props!.graphData!.nodes.map(n => n.id)).toEqual(['a', 'b']);
    expect(renderer.props!.autoPauseRedraw).toBe(true);
  } finally {
    vi.useRealTimers();
  }
});

it('widens the line of the relationship the inspector points at', () => {
  const nodes = [node('a'), node('b', 100), node('c', 200)];
  const edges = [{ id: 'ab', kind: 'claim' as const, source: 'a', target: 'b' }, { id: 'ac', kind: 'claim' as const, source: 'a', target: 'c' }];
  render(<MapCanvas {...base} nodes={nodes} edges={edges} scopeKey="emphasis" selectedId="a" emphasisEdgeId="ac" />);
  const widths: number[] = [];
  const ctx = { save: vi.fn(), restore: vi.fn(), beginPath: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), setLineDash: vi.fn(), globalAlpha: 1, strokeStyle: '', lineWidth: 0, stroke() { widths.push(this.lineWidth); } };
  renderer.props!.graphData!.links.forEach(e => renderer.props!.linkCanvasObject?.(e, ctx as unknown as CanvasRenderingContext2D, 1));
  expect(widths).toEqual([1.5, 2.5]);
});

it('keeps a pointed Lens line dotted and in the method colour while widening it', () => {
  const nodes = [node('a'), { ...node('l', 100), kind: 'lens_principle' as const }];
  const edges = [{ id: 'la', kind: 'applies_to' as const, source: 'l', target: 'a' }];
  document.documentElement.style.setProperty('--gg-color-map-method', 'rgb(1, 2, 3)');
  document.documentElement.style.setProperty('--gg-color-map-edge-active', 'rgb(4, 5, 6)');
  onTestFinished(() => { document.documentElement.style.removeProperty('--gg-color-map-method'); document.documentElement.style.removeProperty('--gg-color-map-edge-active'); });
  render(<MapCanvas {...base} nodes={nodes} edges={edges} scopeKey="emphasis-lens" selectedId="a" emphasisEdgeId="la" />);
  const drawn: { width: number; dash: number[]; style: string }[] = [];
  let dash: number[] = [];
  const ctx = { save: vi.fn(), restore: vi.fn(), beginPath: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), quadraticCurveTo: vi.fn(), setLineDash: (value: number[]) => { dash = value; }, globalAlpha: 1, strokeStyle: '', lineWidth: 0,
    stroke() { drawn.push({ width: this.lineWidth, dash, style: this.strokeStyle }); } };
  renderer.props!.graphData!.links.forEach(e => renderer.props!.linkCanvasObject?.(e, ctx as unknown as CanvasRenderingContext2D, 1));
  expect(drawn).toHaveLength(1);
  expect(drawn[0]).toEqual({ width: 2.5, dash: [2, 3], style: 'rgb(1, 2, 3)' });
});
