import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ForceGraphProps } from "react-force-graph-2d";
import MapCanvas, { coverAlpha, FOCUS_FADE_MS, SCOPE_FADE_MS, SETTLE_MS, type MapCanvasControls, type MapCanvasLink } from "./MapCanvas";
import type { PositionedNode } from "./layout";
// `log` records, in order, each render's redraw mode ("paused" lets the renderer stop drawing) and each camera call.
// `moves` records how long each zoom the canvas asks for takes, in ms.
const renderer = vi.hoisted(() => ({ props: null as ForceGraphProps<PositionedNode, MapCanvasLink> | null, center: { x: 0, y: 0 }, zoom: 1, renders: 0, log: [] as string[], moves: [] as number[] }));
vi.mock("react-force-graph-2d", async () => {
  const { forwardRef, useImperativeHandle } = await import("react");
  return { default: forwardRef((props: ForceGraphProps<PositionedNode, MapCanvasLink>, ref) => {
    renderer.props = props; renderer.renders++; renderer.log.push(props.autoPauseRedraw ? "paused" : "drawing");
    useImperativeHandle(ref, () => ({
      centerAt: (x?: number, y?: number) => { if (x !== undefined && y !== undefined) { renderer.center = { x, y }; renderer.log.push("center"); } return renderer.center; },
      zoom: (scale?: number, ms?: number) => { if (scale !== undefined) { renderer.zoom = scale; renderer.log.push("zoom"); renderer.moves.push(ms ?? 0); } return renderer.zoom; },
      graph2ScreenCoords: (x: number, y: number) => ({ x: x + 450, y: y + 350 }),
    }));
    return <canvas />;
  }) };
});
const node = (id: string, x = 0): PositionedNode => ({ id, label: id, kind: "entity", entityKind: "organization", degree: 4, watched: id === 'a', x, y: 0, fx: x, fy: 0 });
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

it('repaints cached names and remeasures after zoom or selection changes', () => {
  const nodes = [node('a'), node('b', 150)];
  const props = { ...base, nodes, scopeKey: 'label-frame-invalidation' };
  const view = render(<MapCanvas {...props} />);
  const ctx = { save: vi.fn(), restore: vi.fn(), measureText: vi.fn((text: string) => ({ width: text.length * 6 })), strokeText: vi.fn(), fillText: vi.fn() };
  const paint = (scale = 1) => renderer.props!.onRenderFramePost?.(ctx as unknown as CanvasRenderingContext2D, scale);
  paint();
  const measured = ctx.measureText.mock.calls.length;
  const painted = ctx.fillText.mock.calls.length;
  expect(measured).toBeGreaterThan(0);
  expect(painted).toBeGreaterThan(0);
  paint();
  expect(ctx.measureText).toHaveBeenCalledTimes(measured);
  expect(ctx.fillText).toHaveBeenCalledTimes(painted * 2);
  paint(2);
  expect(ctx.measureText.mock.calls.length).toBeGreaterThan(measured);
  ctx.measureText.mockClear(); ctx.fillText.mockClear();
  view.rerender(<MapCanvas {...props} selectedId="b" />);
  paint(2);
  expect(ctx.measureText).toHaveBeenCalled();
  expect(ctx.fillText).toHaveBeenCalledWith('b', expect.any(Number), expect.any(Number));
});

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

it('centres a selection above the bottom sheet it opens without zooming', async () => {
  reducedMotion();
  rects({ 'map-inspector': { top: 280, bottom: 700, height: 420, left: 0, right: 900, width: 900 } });
  const canvas = (selectedId?: string) => <div className="map-body"><div className="map-stage"><MapCanvas {...base} selectedId={selectedId} nodes={[node('a'), node('b', 40)]} scopeKey="sheet" /></div>{selectedId ? <aside className="map-inspector" style={{ position: 'absolute' }} /> : null}</div>;
  const view = render(canvas());
  const zoom = renderer.zoom;
  view.rerender(canvas('a'));
  await waitFor(() => expect(350 + (0 - renderer.center.y) * renderer.zoom).toBeCloseTo(140, 2));
  expect(450 + (0 - renderer.center.x) * renderer.zoom).toBeCloseTo(450, 2);
  expect(renderer.zoom).toBe(zoom);
});

it('re-centres the selection into the visible area when the Library opens and closes', async () => {
  reducedMotion();
  rects({ 'map-library': { top: 0, bottom: 700, height: 700, left: 0, right: 360, width: 360 } });
  const canvas = <div className="map-stage"><MapCanvas {...base} selectedId="a" nodes={[node('a')]} scopeKey="library-visible-selection" /></div>;
  const view = render(<div className="map-body">{canvas}</div>);
  const zoom = renderer.zoom;
  view.rerender(<div className="map-body">{canvas}<aside className="map-library" /></div>);
  await waitFor(() => expect(renderer.center.x).toBeCloseTo(-180 / zoom, 6));
  expect(renderer.zoom).toBe(zoom);
  view.rerender(<div className="map-body">{canvas}</div>);
  await waitFor(() => expect(renderer.center.x).toBe(0));
});

it('counts What\'s new where it lands while it is still sliding in', async () => {
  reducedMotion();
  // Mid-slide the panel is drawn 24 px left of where it lands, and nothing resizes when it arrives.
  rects({ 'map-library': { top: 0, bottom: 700, height: 700, left: -24, right: 336, width: 360 } });
  const canvas = <div className="map-stage"><MapCanvas {...base} nodes={[node('a', -100), node('b', 100)]} scopeKey="whats-new-slide" /></div>;
  const view = render(<div className="map-body">{canvas}</div>);
  view.rerender(<div className="map-body">{canvas}<aside className="map-library map-whats-new" style={{ transform: 'matrix(1, 0, 0, 1, -24, 0)' }} /></div>);
  const stage = view.container.querySelector<HTMLElement>('.map-stage')!;
  await waitFor(() => expect(stage.style.getPropertyValue('--map-covered-left')).toBe('360px'));
  await waitFor(() => { for (const x of [-100, 100]) expect(450 + (x - renderer.center.x) * renderer.zoom).toBeGreaterThanOrEqual(360); });
  // An empty Map has no canvas to measure: its message must not keep these insets.
  view.unmount();
  expect(stage.style.getPropertyValue('--map-covered-left')).toBe('');
});

it('reframes for a panel that opens while a Fit still eases, and keeps that view', async () => {
  vi.spyOn(window, 'matchMedia').mockReturnValue({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() } as unknown as MediaQueryList);
  const controls = { current: null as MapCanvasControls | null };
  rects({ 'map-library': { top: 0, bottom: 700, height: 700, left: 0, right: 360, width: 360 } });
  const canvas = <div className="map-stage"><MapCanvas {...base} nodes={[node('a', -100), node('b', 100)]} scopeKey="move-in-flight" controls={controls} /></div>;
  const view = render(<div className="map-body">{canvas}</div>);
  renderer.moves.length = 0;
  act(() => controls.current?.fit());
  expect(renderer.moves).toEqual([180]);
  // The opening settle is still running, so the reframe for the panel is instant; the renderer would
  // put the Fit's view back when the Fit ends, so the reframe ends after it.
  view.rerender(<div className="map-body">{canvas}<aside className="map-library" /></div>);
  await waitFor(() => expect(renderer.moves).toHaveLength(2));
  expect(renderer.moves[1]).toBeGreaterThan(150);
  expect(450 + (-100 - renderer.center.x) * renderer.zoom).toBeGreaterThanOrEqual(360);
});

it('keeps zoom and centre when a reader sheet opens without a related node', async () => {
  rects({ 'map-reader': { top: 280, bottom: 700, height: 420, left: 0, right: 700, width: 700 } });
  const canvas = (reading: boolean) => <div className="map-body"><div className="map-stage"><MapCanvas {...base} nodes={[node('a', -100), node('b', 100)]} scopeKey="reader-sheet" /></div>{reading ? <aside className="map-inspector map-reader" style={{ position: 'absolute' }} /> : null}</div>;
  const view = render(canvas(false));
  act(() => { renderer.zoom = 1.4; renderer.center = { x: 12, y: -8 }; });
  view.rerender(canvas(true));
  await waitFor(() => expect(view.container.querySelector<HTMLElement>('.map-stage')!.style.getPropertyValue('--map-covered-bottom')).toBe('420px'));
  await act(async () => {});
  expect(renderer.zoom).toBe(1.4);
  expect(renderer.center).toEqual({ x: 12, y: -8 });
});

it('opens on a selection framed around it in the area the stats pill and legend leave free', () => {
  reducedMotion();
  rects({ 'map-stats': { top: 0, bottom: 60, height: 60, left: 12, right: 600, width: 588 }, 'map-legend': { top: 640, bottom: 700, height: 60, left: 0, right: 900, width: 900 } });
  renderer.zoom = 1; renderer.center = { x: 0, y: 0 };
  // The selected name sits at the bottom-right corner of its drawing, which reaches 800 units up and left.
  const nodes = [node('selected'), { ...node('far', -800), y: -600, fy: -600 }, { ...node('mid', -400), y: -300, fy: -300 }];
  render(<div className="map-body"><div className="map-stage"><div className="map-stats" /><MapCanvas {...base} reserveLegend selectedId="selected" nodes={nodes} scopeKey="open-on-selection" /><details className="map-legend"><summary>Legend</summary></details></div></div>);
  const screen = (item: PositionedNode) => ({ x: 450 + (item.x - renderer.center.x) * renderer.zoom, y: 350 + (item.y - renderer.center.y) * renderer.zoom });
  // Free area: 60 px of pill above, 60 px of legend below.
  expect(screen(nodes[0]!).x).toBeCloseTo(450, 2);
  expect(screen(nodes[0]!).y).toBeCloseTo(350, 2);
  for (const item of nodes) {
    expect(screen(item).x).toBeGreaterThanOrEqual(0);
    expect(screen(item).y).toBeGreaterThanOrEqual(60);
    expect(screen(item).y).toBeLessThanOrEqual(640);
  }
});

it('restores a saved view only while it still frames the names', () => {
  reducedMotion();
  const props = { ...base, nodes: [node('a', -100), node('b', 100)], scopeKey: 'saved-off-the-names' };
  const first = render(<MapCanvas {...props} />);
  // A view saved far from the drawing, as a camera placed while the canvas was hidden would be.
  act(() => { renderer.center = { x: 5000, y: 5000 }; renderer.zoom = 2; renderer.props!.onZoom?.({ x: 0, y: 0, k: 2 }); });
  first.unmount();
  render(<MapCanvas {...props} />);
  expect(renderer.center).toEqual({ x: 0, y: 0 });
  expect(450 + (-100 - renderer.center.x) * renderer.zoom).toBeGreaterThanOrEqual(0);
  expect(450 + (100 - renderer.center.x) * renderer.zoom).toBeLessThanOrEqual(900);
});

it('places the view when names arrive on a canvas that had none, and centres a selection once it is drawn', () => {
  reducedMotion();
  renderer.center = { x: 999, y: 999 }; renderer.zoom = 1;
  const props = { ...base, scopeKey: 'names-arrive', selectedId: 'late' };
  const view = render(<MapCanvas {...props} nodes={[]} />);
  view.rerender(<MapCanvas {...props} nodes={[node('a', -100), node('b', 100)]} />);
  expect(renderer.center).toEqual({ x: 0, y: 0 });
  // The selected name arrives with a later update, at the drawing's right edge.
  view.rerender(<MapCanvas {...props} nodes={[node('a', -100), node('b', 100), node('late', 300)]} />);
  expect(renderer.center.x).toBeCloseTo(300, 6);
  expect(450 + (-100 - renderer.center.x) * renderer.zoom).toBeGreaterThanOrEqual(0);
});

it('leaves the camera alone while the canvas has no layout box', () => {
  reducedMotion();
  const controls = { current: null as MapCanvasControls | null };
  render(<MapCanvas {...base} nodes={[node('a', -100), node('b', 100)]} scopeKey="hidden-canvas" controls={controls} />);
  act(() => { renderer.center = { x: 7, y: 7 }; renderer.zoom = 1.2; });
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(0);
  act(() => { controls.current?.fit(); controls.current?.focus('a'); });
  expect(renderer.center).toEqual({ x: 7, y: 7 });
  expect(renderer.zoom).toBe(1.2);
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
  const parse = vi.spyOn(Number, 'parseInt');
  renderer.props!.nodeCanvasObject?.(dimmed, ctx as unknown as CanvasRenderingContext2D, 1);
  renderer.props!.nodeCanvasObject?.(dimmed, ctx as unknown as CanvasRenderingContext2D, 1);
  expect(parse).not.toHaveBeenCalled();
  parse.mockRestore();
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
    // The entering name is still sliding out of the name it links to; drawing stops once it arrives.
    act(() => { vi.advanceTimersByTime(SETTLE_MS); });
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

/** Records where and how a node or link draws: the translation, every fill's alpha, line widths and dashes. */
function drawing() {
  const record = { translate: [] as number[][], fills: [] as { color: string; alpha: number }[], widths: [] as number[], dashes: [] as number[][], moves: [] as number[][], texts: [] as { text: string; alpha: number }[] };
  const stack: number[] = [];
  const ctx = {
    globalAlpha: 1, fillStyle: '', strokeStyle: '', lineWidth: 0, font: '', textAlign: '', textBaseline: '', lineJoin: '',
    save() { stack.push(this.globalAlpha); }, restore() { const prior = stack.pop(); if (prior !== undefined) this.globalAlpha = prior; },
    translate(x: number, y: number) { record.translate.push([x, y]); }, scale() {}, beginPath() {}, arc() {}, roundRect() {}, closePath() {}, quadraticCurveTo() {},
    moveTo(x: number, y: number) { record.moves.push([x, y]); }, lineTo() {},
    setLineDash(dash: number[]) { record.dashes.push(dash); },
    fill() { record.fills.push({ color: String(this.fillStyle), alpha: this.globalAlpha }); },
    stroke() { record.widths.push(this.lineWidth); },
    measureText: (text: string) => ({ width: text.length * 6 }), strokeText() {},
    createRadialGradient: () => ({ addColorStop() {} }),
    fillText(text: string) { record.texts.push({ text, alpha: this.globalAlpha }); },
  };
  return { ctx: ctx as unknown as CanvasRenderingContext2D, record };
}
const motion = () => vi.spyOn(window, 'matchMedia').mockReturnValue({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() } as unknown as MediaQueryList);

it('opens with names spreading out from the middle of their drawing and settling into place', () => {
  motion();
  vi.useFakeTimers();
  try {
    const nodes = [node('a', -100), node('b', 100)];
    const edges = [{ id: 'ab', kind: 'claim' as const, source: 'a', target: 'b' }];
    render(<MapCanvas {...base} nodes={nodes} edges={edges} scopeKey="settle-open" />);
    expect(renderer.props!.autoPauseRedraw).toBe(false);
    const drawNode = (id: string) => {
      const { ctx, record } = drawing();
      renderer.props!.onRenderFramePre?.(ctx, 1);
      renderer.props!.nodeCanvasObject?.(renderer.props!.graphData!.nodes.find(n => n.id === id)!, ctx, 1);
      return record;
    };
    // At the start each name sits 60% of the way to the middle (x 0) and moves out from there.
    const start = drawNode('a').translate[0]!;
    expect(start[0]).toBeCloseTo(60, 1);
    expect(start[1]).toBeCloseTo(0, 5);
    const link = drawing();
    renderer.props!.linkCanvasObject?.(renderer.props!.graphData!.links[0]!, link.ctx, 1);
    expect(link.record.moves[0]![0]).toBeCloseTo(-40, 1);
    act(() => { vi.advanceTimersByTime(SETTLE_MS / 2); });
    const halfway = drawNode('a').translate[0]![0]!;
    expect(Math.abs(halfway)).toBeLessThan(60);
    act(() => { vi.advanceTimersByTime(SETTLE_MS); });
    expect(drawNode('a').translate).toEqual([]);
    expect(renderer.props!.autoPauseRedraw).toBe(true);
  } finally {
    vi.useRealTimers();
  }
});

it('opens in place under reduced motion', () => {
  reducedMotion();
  render(<MapCanvas {...base} nodes={[node('a', -100), node('b', 100)]} scopeKey="settle-reduced" />);
  expect(renderer.props!.autoPauseRedraw).toBe(true);
  const { ctx, record } = drawing();
  renderer.props!.onRenderFramePre?.(ctx, 1);
  renderer.props!.nodeCanvasObject?.(renderer.props!.graphData!.nodes[0]!, ctx, 1);
  expect(record.translate).toEqual([]);
});

it('eases a pointed name\'s neighbourhood forward and the rest back', () => {
  motion();
  vi.useFakeTimers();
  document.documentElement.style.setProperty('--gg-color-map-dim', '.2');
  try {
    const nodes = [node('a'), node('b', 100), node('c', 200)];
    const edges = [{ id: 'ab', kind: 'claim' as const, source: 'a', target: 'b' }];
    render(<MapCanvas {...base} nodes={nodes} edges={edges} scopeKey="focus-fade" />);
    act(() => { vi.advanceTimersByTime(SETTLE_MS * 2); });
    const alphaOf = (id: string) => {
      const { ctx, record } = drawing();
      renderer.props!.onRenderFramePre?.(ctx, 1);
      renderer.props!.nodeCanvasObject?.(renderer.props!.graphData!.nodes.find(n => n.id === id)!, ctx, 1);
      return record.fills.at(-1)!.alpha;
    };
    act(() => renderer.props!.onNodeHover?.(nodes[0], null));
    expect(renderer.props!.autoPauseRedraw).toBe(false);
    expect(alphaOf('c')).toBeCloseTo(1, 2);
    act(() => { vi.advanceTimersByTime(FOCUS_FADE_MS / 2); });
    expect(alphaOf('c')).toBeGreaterThan(.2);
    expect(alphaOf('c')).toBeLessThan(1);
    act(() => { vi.advanceTimersByTime(FOCUS_FADE_MS); });
    expect(alphaOf('c')).toBeCloseTo(.2, 5);
    expect(alphaOf('b')).toBe(1);
    expect(renderer.props!.autoPauseRedraw).toBe(true);
  } finally {
    vi.useRealTimers();
    document.documentElement.style.removeProperty('--gg-color-map-dim');
  }
});

it('keeps lines one screen pixel thin at every zoom, dashes included', () => {
  reducedMotion();
  const nodes = [node('a'), node('b', 100), node('c', 200)];
  const edges = [{ id: 'ab', kind: 'claim' as const, source: 'a', target: 'b' }, { id: 'bc', kind: 'claim' as const, source: 'b', target: 'c', status: 'disputed' as const }];
  render(<MapCanvas {...base} nodes={nodes} edges={edges} scopeKey="thin-lines" />);
  const { ctx, record } = drawing();
  for (const link of renderer.props!.graphData!.links) renderer.props!.linkCanvasObject?.(link, ctx, 2);
  expect(record.widths).toEqual([.5, .75]);
  expect(record.dashes).toEqual([[], [2, 1.5]]);
});

it('fades ordinary names in as World zooms in, keeping prominent ones named', () => {
  reducedMotion();
  // A hundred names on a grid: the ten best-connected keep their names, the next hubs fade in first.
  const nodes = Array.from({ length: 100 }, (_, index) => ({ ...node(`n${index}`, (index % 10) * 80 - 400), y: Math.floor(index / 10) * 60 - 280, fy: Math.floor(index / 10) * 60 - 280, watched: false, degree: 100 - index }));
  render(<MapCanvas {...base} nodes={nodes} scopeKey="label-zoom-fade" />);
  const labelled = (scale: number) => {
    const { ctx, record } = drawing();
    renderer.props!.onRenderFramePost?.(ctx, scale);
    return new Map(record.texts.map(item => [item.text, item.alpha]));
  };
  const far = labelled(.7);
  expect(far.get('n0')).toBe(1);
  expect(far.get('n12')).toBeGreaterThan(0);
  expect(far.get('n12')).toBeLessThan(1);
  expect(far.has('n50')).toBe(false);
  // Between 1.2× and 1.6× every other name fades in.
  const middle = labelled(1.4);
  expect(middle.get('n50')).toBeGreaterThan(0);
  expect(middle.get('n50')).toBeLessThan(1);
  expect(labelled(1.7).get('n50')).toBe(1);
});

it('draws the final state once the last transition ends, even when no frame was drawn during it', () => {
  motion();
  vi.useFakeTimers();
  try {
    const edges = [{ id: 'ab', kind: 'claim' as const, source: 'a', target: 'b' }, { id: 'bd', kind: 'claim' as const, source: 'b', target: 'd' }];
    const view = render(<MapCanvas {...base} nodes={[node('a', -100), node('b', 100)]} edges={edges.slice(0, 1)} scopeKey="final-frame:one" />);
    // The opening settle, a scope fade that brings d in, and a focus fade that points at a name and away again, all at once.
    view.rerender(<MapCanvas {...base} nodes={[node('a', -100), node('b', 100), node('d', 200)]} edges={edges} scopeKey="final-frame:two" />);
    act(() => renderer.props!.onNodeHover?.(renderer.props!.graphData!.nodes[0]!, null));
    act(() => renderer.props!.onNodeHover?.(null, null));
    expect(renderer.props!.autoPauseRedraw).toBe(false);
    renderer.log.length = 0;
    // Their timers end every transition before the renderer draws one frame, an order a busy WebView can produce.
    act(() => { vi.advanceTimersByTime(SETTLE_MS * 2); });
    expect(renderer.props!.autoPauseRedraw).toBe(true);
    // After the renderer was told it may stop drawing, the canvas still asked it for a frame.
    const paused = renderer.log.indexOf('paused');
    expect(paused).toBeGreaterThanOrEqual(0);
    expect(renderer.log.slice(paused)).toContain('zoom');
    // That frame shows the final state: every name in its place at full strength.
    const { ctx, record } = drawing();
    renderer.props!.onRenderFramePre?.(ctx, 1);
    for (const item of renderer.props!.graphData!.nodes) renderer.props!.nodeCanvasObject?.(item, ctx, 1);
    expect(renderer.props!.graphData!.nodes.map(item => item.id)).toEqual(['a', 'b', 'd']);
    expect(record.translate).toEqual([]);
    expect(record.fills.map(fill => fill.alpha)).toEqual([1, 1, 1]);
  } finally {
    vi.useRealTimers();
  }
});

it('frames the opening view for the size the page settles at, until the person moves it', () => {
  reducedMotion();
  const observers: ResizeObserverCallback[] = [];
  vi.stubGlobal('ResizeObserver', class { constructor(callback: ResizeObserverCallback) { observers.push(callback); } observe() {} unobserve() {} disconnect() {} });
  try {
    let height = 300;
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(() => height);
    const nodes = [node('a'), { ...node('up'), y: -300, fy: -300 }, { ...node('down'), y: 300, fy: 300 }];
    render(<MapCanvas {...base} nodes={nodes} scopeKey="opening-final-size" />);
    const resize = (next: number) => { height = next; act(() => observers.forEach(callback => callback([], {} as ResizeObserver))); };
    const screenY = (item: PositionedNode) => height / 2 + (item.y - renderer.center.y) * renderer.zoom;
    const opening = renderer.zoom;
    // The page finishes its layout after the Map opened: the canvas grows from 300 to 700 px tall.
    resize(700);
    expect(renderer.zoom).toBeGreaterThan(opening * 2);
    // The drawing spans the new height but for a modest margin.
    expect(screenY(nodes[1]!)).toBeGreaterThanOrEqual(0);
    expect(screenY(nodes[1]!)).toBeLessThan(40);
    expect(screenY(nodes[2]!)).toBeGreaterThan(660);
    expect(screenY(nodes[2]!)).toBeLessThanOrEqual(700);
    // Once the person zooms, the view is theirs and a later resize keeps it.
    fireEvent.wheel(screen.getByTestId('map-canvas'));
    const theirs = renderer.zoom;
    resize(500);
    expect(renderer.zoom).toBe(theirs);
  } finally {
    vi.unstubAllGlobals();
  }
});

it('waits for the canvas element to take its size before placing the opening view', () => {
  reducedMotion();
  let canvasReady = false;
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(function (this: HTMLElement) { return this instanceof HTMLCanvasElement && !canvasReady ? 0 : 900; });
  const frames: FrameRequestCallback[] = [];
  vi.spyOn(window, 'requestAnimationFrame').mockImplementation(callback => frames.push(callback));
  vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => {});
  renderer.center = { x: 999, y: 999 }; renderer.log.length = 0;
  render(<MapCanvas {...base} nodes={[node('a', -100), node('b', 100)]} scopeKey="canvas-box-later" />);
  // A camera move now would be anchored on an empty box and land in the top-left corner, so nothing moves yet.
  expect(renderer.log).not.toContain('center');
  canvasReady = true;
  act(() => frames.splice(0).forEach(callback => callback(performance.now())));
  expect(renderer.center).toEqual({ x: 0, y: 0 });
});

it('frames a large drawing to fill the free area but for a modest margin', () => {
  reducedMotion();
  rects({ 'map-stats': { top: 0, bottom: 60, height: 60, left: 12, right: 600, width: 588 }, 'map-legend': { top: 640, bottom: 700, height: 60, left: 0, right: 900, width: 900 } });
  // Forty names on an oval 1,000 units wide and 800 tall.
  const nodes = Array.from({ length: 40 }, (_, index) => {
    const angle = index / 40 * Math.PI * 2, y = Math.sin(angle) * 400;
    return { ...node(`oval-${index}`, Math.cos(angle) * 500), y, fy: y };
  });
  render(<div className="map-body"><div className="map-stage"><div className="map-stats" /><MapCanvas {...base} reserveLegend nodes={nodes} scopeKey="fill-free-area" /><details className="map-legend"><summary>Legend</summary></details></div></div>);
  const ys = nodes.map(item => 350 + (item.y - renderer.center.y) * renderer.zoom);
  // Each dot has an 8-unit radius, drawn at full size at this zoom.
  const radius = 8 * renderer.zoom;
  // Free area: 60 px of pill above and 60 px of legend below leave 580 px; the dots, whole, stay inside it.
  expect(Math.min(...ys) - radius).toBeGreaterThanOrEqual(60);
  expect(Math.max(...ys) + radius).toBeLessThanOrEqual(640);
  expect(Math.max(...ys) - Math.min(...ys)).toBeGreaterThan(580 * .88);
});

it('opens on a finite camera while a name has no position yet, and frames it once it has one', () => {
  reducedMotion();
  renderer.center = { x: 3, y: 3 }; renderer.zoom = 1;
  // WebKit can run the opening view before the renderer gives every name a position.
  const unplaced = { ...node('late'), x: undefined, y: undefined, fx: undefined, fy: undefined } as unknown as PositionedNode;
  const props = { ...base, scopeKey: 'unplaced-name', selectedId: 'late' };
  const view = render(<MapCanvas {...props} nodes={[node('a', -100), node('b', 100), unplaced]} />);
  expect(Number.isFinite(renderer.zoom)).toBe(true);
  expect(Number.isFinite(renderer.center.x)).toBe(true);
  expect(Number.isFinite(renderer.center.y)).toBe(true);
  view.rerender(<MapCanvas {...props} nodes={[node('a', -100), node('b', 100), node('late', 40)]} />);
  expect(Number.isFinite(renderer.zoom)).toBe(true);
  expect(Number.isFinite(renderer.center.x)).toBe(true);
});
